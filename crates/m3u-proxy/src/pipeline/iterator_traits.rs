//! Plugin iterator interface for streaming data between pipeline stages
//!
//! This module provides the core iterator interface that allows plugins to stream
//! data through the pipeline with bounded memory usage and backpressure.

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

/// Iterator result that carries either data or exhaustion signal
#[derive(Debug, Clone)]
pub enum IteratorResult<T> {
    /// Data chunk from producer
    Chunk(Vec<T>),
    /// Null record indicating end of stream
    Exhausted,
}

/// Plugin iterator trait for streaming data with dynamic chunk sizing
#[async_trait]
pub trait PluginIterator<T>: Send + Sync {
    /// Get next chunk of data or exhaustion signal with default chunk size
    async fn next_chunk(&mut self) -> Result<IteratorResult<T>>;
    
    /// Get next chunk with specific requested size (production-ready dynamic sizing)
    async fn next_chunk_with_size(&mut self, requested_size: usize) -> Result<IteratorResult<T>>;
    
    /// Set buffer size for this iterator (cascades upstream if needed)
    async fn set_buffer_size(&mut self, buffer_size: usize) -> Result<()>;
    
    /// Get current buffer size
    fn get_current_buffer_size(&self) -> usize;
    
    /// Get current chunk size this iterator is configured for
    fn get_chunk_size(&self) -> usize;
    
    /// Check if iterator is exhausted (optional optimization)
    fn is_exhausted(&self) -> bool;
    
    /// Close iterator early and signal upstream providers
    async fn close(&mut self) -> Result<()>;
}

/// Bounded buffer for stage bridging with backpressure
pub struct BoundedBuffer<T> {
    sender: mpsc::Sender<Vec<T>>,
    receiver: mpsc::Receiver<Vec<T>>,
    max_buffer_size: usize,
    current_size: usize,
}

impl<T: Send + Sync + 'static> BoundedBuffer<T> {
    pub fn new(max_buffer_size: usize) -> Self {
        let (sender, receiver) = mpsc::channel(max_buffer_size);
        Self {
            sender,
            receiver,
            max_buffer_size,
            current_size: 0,
        }
    }
    
    /// Push data with backpressure
    pub async fn push(&mut self, data: Vec<T>) -> Result<()> {
        self.current_size += data.len();
        self.sender.send(data).await?;
        Ok(())
    }
    
    /// Pull data from buffer
    pub async fn pull(&mut self) -> Option<Vec<T>> {
        if let Some(data) = self.receiver.recv().await {
            self.current_size = self.current_size.saturating_sub(data.len());
            Some(data)
        } else {
            None
        }
    }
    
    pub fn current_size(&self) -> usize {
        self.current_size
    }
    
    pub fn is_full(&self) -> bool {
        self.current_size >= self.max_buffer_size
    }
}

/// Bridge between pipeline stages with bounded memory
pub struct StageBridge<T> {
    producer_iterator: Box<dyn PluginIterator<T>>,
    buffer: BoundedBuffer<T>,
    exhausted: bool,
    closed: bool,
}

impl<T> StageBridge<T> 
where 
    T: Send + Sync + Clone + 'static,
{
    pub fn new(
        producer_iterator: Box<dyn PluginIterator<T>>,
        buffer_size: usize,
    ) -> Self {
        Self {
            producer_iterator,
            buffer: BoundedBuffer::new(buffer_size),
            exhausted: false,
            closed: false,
        }
    }
    
    /// Get next chunk for consumer with backpressure
    pub async fn next_for_consumer(&mut self) -> Result<IteratorResult<T>> {
        if self.exhausted || self.closed {
            return Ok(IteratorResult::Exhausted);
        }
        
        // Try to get data from buffer first
        if let Some(buffered_data) = self.buffer.pull().await {
            return Ok(IteratorResult::Chunk(buffered_data));
        }
        
        // Buffer is empty, get from producer
        match self.producer_iterator.next_chunk().await? {
            IteratorResult::Chunk(data) => {
                // Buffer the data with backpressure
                self.buffer.push(data.clone()).await?;
                Ok(IteratorResult::Chunk(data))
            }
            IteratorResult::Exhausted => {
                self.exhausted = true;
                Ok(IteratorResult::Exhausted)
            }
        }
    }
    
    /// Close the bridge early and signal upstream producer
    pub async fn close(&mut self) -> Result<()> {
        if !self.closed {
            self.closed = true;
            self.producer_iterator.close().await?;
        }
        Ok(())
    }
    
    /// Check if bridge is exhausted or closed
    pub fn is_exhausted(&self) -> bool {
        self.exhausted || self.closed
    }
    
    /// Get current buffer usage
    pub fn buffer_usage(&self) -> (usize, usize) {
        (self.buffer.current_size(), self.buffer.max_buffer_size)
    }
}

/// Memory efficiency levels for plugins
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryEfficiency {
    /// Most efficient (chunked/streamed or file spill)
    Low,
    /// Moderate (chunked/streamed but in memory)
    Medium,
    /// Least efficient (full dataset in memory)
    High,
}

impl From<&str> for MemoryEfficiency {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "low" => MemoryEfficiency::Low,
            "medium" => MemoryEfficiency::Medium,
            "high" => MemoryEfficiency::High,
            _ => MemoryEfficiency::Medium, // Default fallback
        }
    }
}

/// Plugin metadata with simplified memory efficiency
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub supported_stages: Vec<String>,
    pub memory_efficiency: MemoryEfficiency,
}

/// Determine optimal chunk size based on memory efficiency and system pressure
pub fn determine_chunk_size(
    memory_efficiency: MemoryEfficiency,
    memory_pressure_pct: f64,
) -> usize {
    let base_size = match memory_efficiency {
        MemoryEfficiency::Low => 2000,    // Large chunks OK
        MemoryEfficiency::Medium => 1000, // Medium chunks
        MemoryEfficiency::High => 500,    // Small chunks
    };
    
    // Adjust for memory pressure
    let pressure_factor = if memory_pressure_pct > 70.0 {
        0.2 // Emergency small chunks
    } else if memory_pressure_pct > 50.0 {
        0.5 // Reduce chunk size under pressure
    } else {
        1.0 // Normal chunk size
    };
    
    ((base_size as f64 * pressure_factor) as usize).max(100) // Minimum 100 items
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_memory_efficiency_conversion() {
        assert_eq!(MemoryEfficiency::from("low"), MemoryEfficiency::Low);
        assert_eq!(MemoryEfficiency::from("medium"), MemoryEfficiency::Medium);
        assert_eq!(MemoryEfficiency::from("high"), MemoryEfficiency::High);
        assert_eq!(MemoryEfficiency::from("invalid"), MemoryEfficiency::Medium);
    }
    
    #[test]
    fn test_chunk_size_determination() {
        // Low memory efficiency, low pressure
        assert_eq!(determine_chunk_size(MemoryEfficiency::Low, 30.0), 2000);
        
        // High memory efficiency, high pressure
        assert_eq!(determine_chunk_size(MemoryEfficiency::High, 80.0), 100);
        
        // Medium efficiency, medium pressure
        assert_eq!(determine_chunk_size(MemoryEfficiency::Medium, 60.0), 500);
    }
}