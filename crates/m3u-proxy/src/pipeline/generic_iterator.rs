//! Generic ordered multi-source iterator implementation
//!
//! This module provides a reusable iterator pattern that eliminates duplication
//! across different data types in the pipeline orchestrator.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info};

use crate::database::Database;
use crate::pipeline::iterator_traits::{IteratorResult, PipelineIterator};

/// Generic trait for loading data from a source with pagination
#[async_trait]
pub trait DataLoader<T, S> {
    /// Load a chunk of data from the given source with offset and limit
    async fn load_chunk(&self, database: &Arc<Database>, source: &S, offset: usize, limit: usize) -> Result<Vec<T>>;
    
    /// Get the identifier for a source (for logging)
    fn get_source_id(&self, source: &S) -> String;
    
    /// Get the priority for a source (for logging)
    fn get_source_priority(&self, source: &S) -> i32;
    
    /// Get the type name for logging
    fn get_type_name(&self) -> &'static str;
}

/// Multi-source iterator that can work with any data type and source type
/// (Renamed for consistency - was OrderedMultiSourceIterator)
pub struct MultiSourceIterator<T, S, L> {
    database: Arc<Database>,
    sources: Vec<S>,
    loader: L,
    current_source_index: usize,
    current_offset: usize,
    chunk_size: usize,
    total_processed: usize,
    exhausted: bool,
    closed: bool,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, S, L> MultiSourceIterator<T, S, L>
where
    L: DataLoader<T, S>,
{
    pub fn new(
        database: Arc<Database>,
        sources: Vec<S>,
        loader: L,
        chunk_size: usize,
    ) -> Self {
        debug!(
            "Creating ordered {} iterator for {} sources with chunk size {}",
            loader.get_type_name(),
            sources.len(),
            chunk_size
        );
        
        Self {
            database,
            sources,
            loader,
            current_source_index: 0,
            current_offset: 0,
            chunk_size,
            total_processed: 0,
            exhausted: false,
            closed: false,
            _phantom: std::marker::PhantomData,
        }
    }
    
    async fn load_chunk_from_current_source(&mut self) -> Result<Vec<T>> {
        if self.current_source_index >= self.sources.len() {
            return Ok(Vec::new());
        }
        
        let source = &self.sources[self.current_source_index];
        let source_id = self.loader.get_source_id(source);
        let priority = self.loader.get_source_priority(source);
        
        debug!(
            "Loading {} chunk from source {} (priority {}, index {}/{}) at offset {} with limit {}",
            self.loader.get_type_name(),
            source_id,
            priority,
            self.current_source_index + 1,
            self.sources.len(),
            self.current_offset,
            self.chunk_size
        );
        
        let data = self.loader
            .load_chunk(&self.database, source, self.current_offset, self.chunk_size)
            .await?;
        
        if data.is_empty() {
            // No more data from this source, move to next source
            self.current_source_index += 1;
            self.current_offset = 0;
            debug!(
                "Finished {} source {} (priority {}), moving to next source",
                self.loader.get_type_name(),
                source_id,
                priority
            );
        } else {
            // More data available from this source
            self.current_offset += data.len();
            debug!(
                "Loaded {} {} items from source {} (priority {})",
                data.len(),
                self.loader.get_type_name(),
                source_id,
                priority
            );
        }
        
        Ok(data)
    }
}

#[async_trait]
impl<T, S, L> PipelineIterator<T> for MultiSourceIterator<T, S, L>
where
    T: Send + Sync,
    S: Send + Sync,
    L: DataLoader<T, S> + Send + Sync,
{
    async fn next_chunk(&mut self) -> Result<IteratorResult<T>> {
        if self.exhausted || self.closed {
            return Ok(IteratorResult::Exhausted);
        }
        
        // Try to load more data from current source
        loop {
            let data = self.load_chunk_from_current_source().await?;
            
            if data.is_empty() {
                // Check if we've processed all sources
                if self.current_source_index >= self.sources.len() {
                    self.exhausted = true;
                    info!(
                        "{} iterator exhausted after processing {} total items from {} sources",
                        self.loader.get_type_name(),
                        self.total_processed,
                        self.sources.len()
                    );
                    return Ok(IteratorResult::Exhausted);
                }
                // Continue to next source
                continue;
            }
            
            self.total_processed += data.len();
            debug!(
                "Returning chunk of {} {} items (total processed: {})",
                data.len(),
                self.loader.get_type_name(),
                self.total_processed
            );
            
            return Ok(IteratorResult::Chunk(data));
        }
    }
    
    fn is_exhausted(&self) -> bool {
        self.exhausted || self.closed
    }
    
    async fn close(&mut self) -> Result<()> {
        if !self.closed {
            self.closed = true;
            info!(
                "{} iterator closed early after processing {} items",
                self.loader.get_type_name(),
                self.total_processed
            );
        }
        Ok(())
    }
    
    async fn next_chunk_with_size(&mut self, chunk_size: usize) -> Result<IteratorResult<T>> {
        let old_chunk_size = self.chunk_size;
        self.chunk_size = chunk_size;
        let result = self.next_chunk().await;
        self.chunk_size = old_chunk_size;
        result
    }
    
    async fn set_buffer_size(&mut self, buffer_size: usize) -> Result<()> {
        self.chunk_size = buffer_size;
        Ok(())
    }
    
    fn get_current_buffer_size(&self) -> usize {
        self.chunk_size
    }
    
    fn get_chunk_size(&self) -> usize {
        self.chunk_size
    }
    
    fn reset(&mut self) -> Result<()> {
        self.current_source_index = 0;
        self.current_offset = 0;
        self.total_processed = 0;
        self.exhausted = false;
        self.closed = false;
        info!(
            "{} iterator reset to beginning", 
            self.loader.get_type_name()
        );
        Ok(())
    }
}

/// Single-source iterator for data that doesn't come from multiple prioritized sources
/// Single-source iterator for consistent naming
/// (Renamed for consistency - was OrderedSingleSourceIterator)
pub struct SingleSourceIterator<T, L> {
    database: Arc<Database>,
    loader: L,
    source_id: uuid::Uuid,
    current_offset: usize,
    chunk_size: usize,
    total_processed: usize,
    exhausted: bool,
    closed: bool,
    _phantom: std::marker::PhantomData<T>,
}

/// Generic trait for loading data from a single source
#[async_trait]
pub trait SingleSourceLoader<T> {
    /// Load a chunk of data with offset and limit
    async fn load_chunk(&self, database: &Arc<Database>, source_id: uuid::Uuid, offset: usize, limit: usize) -> Result<Vec<T>>;
    
    /// Get the type name for logging
    fn get_type_name(&self) -> &'static str;
}

impl<T, L> SingleSourceIterator<T, L>
where
    L: SingleSourceLoader<T>,
{
    pub fn new(
        database: Arc<Database>,
        loader: L,
        source_id: uuid::Uuid,
        chunk_size: usize,
    ) -> Self {
        debug!(
            "Creating ordered {} iterator for source {} with chunk size {}",
            loader.get_type_name(),
            source_id,
            chunk_size
        );
        
        Self {
            database,
            loader,
            source_id,
            current_offset: 0,
            chunk_size,
            total_processed: 0,
            exhausted: false,
            closed: false,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<T, L> PipelineIterator<T> for SingleSourceIterator<T, L>
where
    T: Send + Sync,
    L: SingleSourceLoader<T> + Send + Sync,
{
    async fn next_chunk(&mut self) -> Result<IteratorResult<T>> {
        if self.exhausted || self.closed {
            return Ok(IteratorResult::Exhausted);
        }
        
        let data = self.loader
            .load_chunk(&self.database, self.source_id, self.current_offset, self.chunk_size)
            .await?;
        
        if data.is_empty() {
            self.exhausted = true;
            info!(
                "{} iterator exhausted after processing {} total items",
                self.loader.get_type_name(),
                self.total_processed
            );
            return Ok(IteratorResult::Exhausted);
        }
        
        self.current_offset += data.len();
        self.total_processed += data.len();
        
        debug!(
            "Loaded {} {} items (total processed: {})",
            data.len(),
            self.loader.get_type_name(),
            self.total_processed
        );
        
        Ok(IteratorResult::Chunk(data))
    }
    
    fn is_exhausted(&self) -> bool {
        self.exhausted || self.closed
    }
    
    async fn close(&mut self) -> Result<()> {
        if !self.closed {
            self.closed = true;
            info!(
                "{} iterator closed early after processing {} items",
                self.loader.get_type_name(),
                self.total_processed
            );
        }
        Ok(())
    }
    
    async fn next_chunk_with_size(&mut self, chunk_size: usize) -> Result<IteratorResult<T>> {
        let old_chunk_size = self.chunk_size;
        self.chunk_size = chunk_size;
        let result = self.next_chunk().await;
        self.chunk_size = old_chunk_size;
        result
    }
    
    async fn set_buffer_size(&mut self, buffer_size: usize) -> Result<()> {
        self.chunk_size = buffer_size;
        Ok(())
    }
    
    fn get_current_buffer_size(&self) -> usize {
        self.chunk_size
    }
    
    fn get_chunk_size(&self) -> usize {
        self.chunk_size
    }
    
    fn reset(&mut self) -> Result<()> {
        self.current_offset = 0;
        self.total_processed = 0;
        self.exhausted = false;
        self.closed = false;
        info!(
            "{} iterator reset to beginning for source {}", 
            self.loader.get_type_name(),
            self.source_id
        );
        Ok(())
    }
}

/// Simple iterator for in-memory Vec data
pub struct VecIterator<T> {
    data: Vec<T>,
    position: usize,
    chunk_size: usize,
    exhausted: bool,
    closed: bool,
}

impl<T> VecIterator<T> {
    pub fn new(data: Vec<T>) -> Self {
        let total_items = data.len();
        debug!("Created VecIterator with {} items", total_items);
        Self {
            data,
            position: 0,
            chunk_size: 100,
            exhausted: false,
            closed: false,
        }
    }
}

/// Iterator that maps values from one type to another
pub struct MappingIterator<S, T, F> {
    source: Box<dyn PipelineIterator<S> + Send + Sync>,
    mapper: F,
    _phantom: std::marker::PhantomData<T>,
}

impl<S, T, F> MappingIterator<S, T, F>
where
    S: Send + Sync + 'static,
    T: Send + Sync + 'static,
    F: Fn(S) -> Result<T> + Send + Sync,
{
    pub fn new(source: Box<dyn PipelineIterator<S> + Send + Sync>, mapper: F) -> Self {
        Self {
            source,
            mapper,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<S, T, F> PipelineIterator<T> for MappingIterator<S, T, F>
where
    S: Send + Sync + 'static,
    T: Send + Sync + 'static,
    F: Fn(S) -> Result<T> + Send + Sync,
{
    async fn next_chunk(&mut self) -> Result<IteratorResult<T>> {
        match self.source.next_chunk().await? {
            IteratorResult::Chunk(chunk) => {
                let mapped_chunk: Result<Vec<T>> = chunk
                    .into_iter()
                    .map(|item| (self.mapper)(item))
                    .collect();
                Ok(IteratorResult::Chunk(mapped_chunk?))
            }
            IteratorResult::Exhausted => Ok(IteratorResult::Exhausted),
        }
    }

    async fn next_chunk_with_size(&mut self, chunk_size: usize) -> Result<IteratorResult<T>> {
        match self.source.next_chunk_with_size(chunk_size).await? {
            IteratorResult::Chunk(chunk) => {
                let mapped_chunk: Result<Vec<T>> = chunk
                    .into_iter()
                    .map(|item| (self.mapper)(item))
                    .collect();
                Ok(IteratorResult::Chunk(mapped_chunk?))
            }
            IteratorResult::Exhausted => Ok(IteratorResult::Exhausted),
        }
    }

    async fn set_buffer_size(&mut self, buffer_size: usize) -> Result<()> {
        self.source.set_buffer_size(buffer_size).await
    }

    async fn close(&mut self) -> Result<()> {
        self.source.close().await
    }

    fn reset(&mut self) -> Result<()> {
        self.source.reset()
    }
    
    fn get_current_buffer_size(&self) -> usize {
        self.source.get_current_buffer_size()
    }
    
    fn get_chunk_size(&self) -> usize {
        self.source.get_chunk_size()
    }
    
    fn is_exhausted(&self) -> bool {
        self.source.is_exhausted()
    }
}

#[async_trait]
impl<T> PipelineIterator<T> for VecIterator<T>
where
    T: Send + Sync + Clone,
{
    async fn next_chunk(&mut self) -> Result<IteratorResult<T>> {
        if self.exhausted || self.closed {
            return Ok(IteratorResult::Exhausted);
        }
        
        if self.position >= self.data.len() {
            self.exhausted = true;
            info!("VecIterator exhausted after processing {} total items", self.position);
            return Ok(IteratorResult::Exhausted);
        }
        
        let end_pos = std::cmp::min(self.position + self.chunk_size, self.data.len());
        let chunk: Vec<T> = self.data[self.position..end_pos].iter().cloned().collect();
        let chunk_size = chunk.len();
        
        self.position = end_pos;
        
        debug!("VecIterator loaded {} items (total processed: {})", chunk_size, self.position);
        Ok(IteratorResult::Chunk(chunk))
    }
    
    async fn next_chunk_with_size(&mut self, chunk_size: usize) -> Result<IteratorResult<T>> {
        self.chunk_size = chunk_size;
        self.next_chunk().await
    }
    
    async fn set_buffer_size(&mut self, _buffer_size: usize) -> Result<()> {
        // No-op for Vec iterator
        Ok(())
    }
    
    fn get_current_buffer_size(&self) -> usize {
        self.data.len()
    }
    
    fn get_chunk_size(&self) -> usize {
        self.chunk_size
    }
    
    fn is_exhausted(&self) -> bool {
        self.exhausted || self.position >= self.data.len()
    }
    
    async fn close(&mut self) -> Result<()> {
        self.closed = true;
        info!("VecIterator closed after processing {} of {} items", self.position, self.data.len());
        Ok(())
    }
    
    fn reset(&mut self) -> Result<()> {
        self.position = 0;
        self.exhausted = false;
        self.closed = false;
        info!("VecIterator reset to beginning with {} items", self.data.len());
        Ok(())
    }
}