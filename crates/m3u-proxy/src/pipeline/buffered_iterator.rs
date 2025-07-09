//! Production-ready buffered iterators with dynamic chunk size management
//!
//! This module provides buffered iterator implementations that can dynamically
//! resize their buffers and serve variable chunk sizes efficiently.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};
use tracing::{debug, error, info, warn};

use crate::pipeline::iterator_traits::{IteratorResult, PipelineIterator};
use crate::pipeline::chunk_manager::ChunkSizeManager;

/// Production-ready buffered iterator with dynamic resizing
pub struct BufferedIterator<T> {
    /// Internal buffer storing data chunks
    buffer: Arc<RwLock<VecDeque<T>>>,
    
    /// Maximum buffer size (total items, not chunks)
    max_buffer_size: Arc<RwLock<usize>>,
    
    /// Current chunk size for this iterator
    chunk_size: Arc<RwLock<usize>>,
    
    /// Reference to chunk size manager for coordination
    chunk_manager: Arc<ChunkSizeManager>,
    
    /// Stage name for this iterator
    stage_name: String,
    
    /// Whether this iterator is exhausted
    exhausted: Arc<RwLock<bool>>,
    
    /// Semaphore to control buffer access and backpressure
    buffer_semaphore: Arc<Semaphore>,
    
    /// Data source (could be database, another iterator, etc.)
    data_source: Arc<dyn DataSource<T>>,
}

/// Trait for data sources that can provide data to buffered iterators
#[async_trait]
pub trait DataSource<T>: Send + Sync {
    /// Fetch next batch of data with specified size
    async fn fetch_batch(&self, batch_size: usize) -> Result<Vec<T>>;
    
    /// Check if data source is exhausted
    fn is_exhausted(&self) -> bool;
    
    /// Get estimated total items (if known)
    fn estimated_total(&self) -> Option<usize>;
}

impl<T> BufferedIterator<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Create a new buffered iterator
    pub fn new(
        stage_name: String,
        chunk_manager: Arc<ChunkSizeManager>,
        data_source: Arc<dyn DataSource<T>>,
        initial_buffer_size: usize,
        initial_chunk_size: usize,
    ) -> Self {
        let max_permits = initial_buffer_size.max(1000); // At least 1000 permits
        
        Self {
            buffer: Arc::new(RwLock::new(VecDeque::with_capacity(initial_buffer_size))),
            max_buffer_size: Arc::new(RwLock::new(initial_buffer_size)),
            chunk_size: Arc::new(RwLock::new(initial_chunk_size)),
            chunk_manager,
            stage_name,
            exhausted: Arc::new(RwLock::new(false)),
            buffer_semaphore: Arc::new(Semaphore::new(max_permits)),
            data_source,
        }
    }
    
    /// Fill buffer from data source if needed
    async fn ensure_buffer_filled(&self) -> Result<()> {
        let buffer_len = {
            let buffer = self.buffer.read().await;
            buffer.len()
        };
        
        let chunk_size = *self.chunk_size.read().await;
        let max_buffer = *self.max_buffer_size.read().await;
        
        // Only fill if buffer is less than one chunk and not exhausted
        if buffer_len < chunk_size && !*self.exhausted.read().await && !self.data_source.is_exhausted() {
            // Calculate how much to fetch (aim for 2x chunk size in buffer)
            let target_buffer_size = (chunk_size * 2).min(max_buffer);
            let fetch_size = target_buffer_size.saturating_sub(buffer_len);
            
            if fetch_size > 0 {
                debug!("├─ Filling buffer for '{}': current={}, fetching={}", self.stage_name, buffer_len, fetch_size);
                
                // Acquire semaphore permits for buffer space
                let _permits = self.buffer_semaphore.acquire_many(fetch_size as u32).await
                    .map_err(|e| anyhow::anyhow!("Failed to acquire buffer permits: {}", e))?;
                
                match self.data_source.fetch_batch(fetch_size).await {
                    Ok(new_data) => {
                        let mut buffer = self.buffer.write().await;
                        buffer.extend(new_data.into_iter());
                        
                        debug!("└─ Buffer filled for '{}': new size={}", self.stage_name, buffer.len());
                    }
                    Err(e) => {
                        error!("Failed to fetch data for '{}': {}", self.stage_name, e);
                        // Mark as exhausted on fetch error
                        *self.exhausted.write().await = true;
                    }
                }
            }
        }
        
        // Check if data source is exhausted
        if self.data_source.is_exhausted() {
            let buffer_empty = {
                let buffer = self.buffer.read().await;
                buffer.is_empty()
            };
            
            if buffer_empty {
                *self.exhausted.write().await = true;
            }
        }
        
        Ok(())
    }
    
    /// Take a chunk of specified size from the buffer
    async fn take_chunk_from_buffer(&self, requested_size: usize) -> Result<Vec<T>> {
        let mut buffer = self.buffer.write().await;
        let available = buffer.len();
        let take_size = requested_size.min(available);
        
        let mut chunk = Vec::with_capacity(take_size);
        for _ in 0..take_size {
            if let Some(item) = buffer.pop_front() {
                chunk.push(item);
            } else {
                break;
            }
        }
        
        // Release semaphore permits for the items we took
        self.buffer_semaphore.add_permits(chunk.len());
        
        debug!("└─ Took chunk from '{}': requested={}, actual={}, remaining={}", 
               self.stage_name, requested_size, chunk.len(), buffer.len());
        
        Ok(chunk)
    }
}

#[async_trait]
impl<T> PipelineIterator<T> for BufferedIterator<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Get next chunk with default chunk size
    async fn next_chunk(&mut self) -> Result<IteratorResult<T>> {
        let chunk_size = *self.chunk_size.read().await;
        self.next_chunk_with_size(chunk_size).await
    }
    
    /// Get next chunk with specific requested size (production-ready)
    async fn next_chunk_with_size(&mut self, requested_size: usize) -> Result<IteratorResult<T>> {
        // Request chunk size from manager (this handles cascade)
        let actual_size = self.chunk_manager.request_chunk_size(&self.stage_name, requested_size).await?;
        
        // Update our chunk size if manager returned different value
        if actual_size != requested_size {
            debug!("Chunk size adjusted for '{}': {} → {}", self.stage_name, requested_size, actual_size);
            *self.chunk_size.write().await = actual_size;
        }
        
        // Ensure buffer has data
        self.ensure_buffer_filled().await?;
        
        // Check if exhausted
        if *self.exhausted.read().await {
            let buffer_len = {
                let buffer = self.buffer.read().await;
                buffer.len()
            };
            
            if buffer_len == 0 {
                info!("Iterator '{}' is exhausted", self.stage_name);
                return Ok(IteratorResult::Exhausted);
            }
        }
        
        // Take chunk from buffer
        let chunk = self.take_chunk_from_buffer(actual_size).await?;
        
        if chunk.is_empty() {
            Ok(IteratorResult::Exhausted)
        } else {
            info!("├─ Iterator '{}' returned chunk: size={}", self.stage_name, chunk.len());
            Ok(IteratorResult::Chunk(chunk))
        }
    }
    
    /// Set buffer size (production-ready with validation)
    async fn set_buffer_size(&mut self, buffer_size: usize) -> Result<()> {
        let clamped_size = buffer_size.clamp(100, 100000); // Reasonable bounds
        
        if clamped_size != buffer_size {
            warn!("Buffer size clamped for '{}': {} → {}", self.stage_name, buffer_size, clamped_size);
        }
        
        // Update buffer size
        *self.max_buffer_size.write().await = clamped_size;
        
        // Update semaphore permits
        let current_permits = self.buffer_semaphore.available_permits();
        if clamped_size > current_permits {
            self.buffer_semaphore.add_permits(clamped_size - current_permits);
        }
        
        // Notify chunk manager
        self.chunk_manager.set_buffer_size(&self.stage_name, clamped_size).await?;
        
        info!("Buffer size updated for '{}': {}", self.stage_name, clamped_size);
        Ok(())
    }
    
    /// Get current buffer size
    fn get_current_buffer_size(&self) -> usize {
        // Use blocking read since this should be fast
        futures::executor::block_on(async {
            *self.max_buffer_size.read().await
        })
    }
    
    /// Get current chunk size
    fn get_chunk_size(&self) -> usize {
        futures::executor::block_on(async {
            *self.chunk_size.read().await
        })
    }
    
    /// Check if iterator is exhausted
    fn is_exhausted(&self) -> bool {
        futures::executor::block_on(async {
            *self.exhausted.read().await
        })
    }
    
    /// Close iterator and clean up resources
    async fn close(&mut self) -> Result<()> {
        *self.exhausted.write().await = true;
        
        // Clear buffer to free memory
        {
            let mut buffer = self.buffer.write().await;
            let cleared_count = buffer.len();
            buffer.clear();
            
            // Release all semaphore permits
            self.buffer_semaphore.add_permits(cleared_count);
        }
        
        info!("Iterator '{}' closed and cleaned up", self.stage_name);
        Ok(())
    }
    
    /// Reset iterator (not supported for buffered iterators)
    fn reset(&mut self) -> Result<()> {
        Err(anyhow::anyhow!("Reset not supported for buffered iterators"))
    }
}

/// Mock data source for testing
pub struct MockDataSource<T> {
    data: Vec<T>,
    current_index: std::sync::atomic::AtomicUsize,
}

impl<T> MockDataSource<T>
where
    T: Clone,
{
    pub fn new(data: Vec<T>) -> Self {
        Self {
            data,
            current_index: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl<T> DataSource<T> for MockDataSource<T>
where
    T: Clone + Send + Sync,
{
    async fn fetch_batch(&self, batch_size: usize) -> Result<Vec<T>> {
        use std::sync::atomic::Ordering;
        
        let start_index = self.current_index.load(Ordering::SeqCst);
        let end_index = (start_index + batch_size).min(self.data.len());
        
        if start_index >= self.data.len() {
            return Ok(Vec::new());
        }
        
        let batch = self.data[start_index..end_index].to_vec();
        self.current_index.store(end_index, Ordering::SeqCst);
        
        // Simulate some async work
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        
        Ok(batch)
    }
    
    fn is_exhausted(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.current_index.load(Ordering::SeqCst) >= self.data.len()
    }
    
    fn estimated_total(&self) -> Option<usize> {
        Some(self.data.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_buffered_iterator_basic_functionality() {
        let chunk_manager = Arc::new(ChunkSizeManager::default());
        let test_data: Vec<i32> = (0..1000).collect();
        let data_source = Arc::new(MockDataSource::new(test_data));
        
        let mut iterator = BufferedIterator::new(
            "test_stage".to_string(),
            chunk_manager,
            data_source,
            500, // buffer size
            100, // chunk size
        );
        
        // Test getting chunks
        let result = iterator.next_chunk().await.unwrap();
        match result {
            IteratorResult::Chunk(chunk) => {
                assert!(!chunk.is_empty());
                assert!(chunk.len() <= 100);
            }
            IteratorResult::Exhausted => panic!("Should not be exhausted"),
        }
    }
    
    #[tokio::test]
    async fn test_dynamic_chunk_size_request() {
        let chunk_manager = Arc::new(ChunkSizeManager::default());
        let test_data: Vec<i32> = (0..1000).collect();
        let data_source = Arc::new(MockDataSource::new(test_data));
        
        let mut iterator = BufferedIterator::new(
            "test_stage".to_string(),
            chunk_manager,
            data_source,
            500, // buffer size
            100, // initial chunk size
        );
        
        // Request larger chunk size
        let result = iterator.next_chunk_with_size(300).await.unwrap();
        match result {
            IteratorResult::Chunk(chunk) => {
                assert!(chunk.len() <= 300);
            }
            IteratorResult::Exhausted => panic!("Should not be exhausted"),
        }
        
        // Check that chunk size was updated
        assert_eq!(iterator.get_chunk_size(), 300);
    }
    
    #[tokio::test]
    async fn test_buffer_resize() {
        let chunk_manager = Arc::new(ChunkSizeManager::default());
        let test_data: Vec<i32> = (0..100).collect();
        let data_source = Arc::new(MockDataSource::new(test_data));
        
        let mut iterator = BufferedIterator::new(
            "test_stage".to_string(),
            chunk_manager,
            data_source,
            200, // initial buffer size
            50,  // chunk size
        );
        
        // Resize buffer
        iterator.set_buffer_size(500).await.unwrap();
        assert_eq!(iterator.get_current_buffer_size(), 500);
    }
}