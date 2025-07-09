//! Rolling buffer iterator for sophisticated multi-source data loading
//!
//! This module implements the rolling buffer pattern described in the orchestrator
//! documentation, enabling concurrent source loading with configurable buffer management.

use anyhow::Result;
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::database::Database;
use crate::pipeline::chunk_manager::ChunkSizeManager;
use crate::pipeline::iterator_traits::{IteratorResult, PipelineIterator};

/// Configuration for rolling buffer behavior with cascading buffer integration
#[derive(Debug, Clone)]
pub struct BufferConfig {
    /// Initial buffer size (number of items) - will be adjusted by cascade requests
    pub initial_buffer_size: usize,
    /// Maximum buffer size (safety limit to prevent memory saturation)
    pub max_buffer_size: usize,
    /// Threshold to trigger next source loading (0.0-1.0)
    pub trigger_threshold: f32,
    /// Initial chunk size for database queries - will be adjusted by cascade requests
    pub initial_chunk_size: usize,
    /// Maximum concurrent sources being loaded
    pub max_concurrent_sources: usize,
    /// Whether to enable cascading buffer integration
    pub enable_cascade_integration: bool,
}

impl Default for BufferConfig {
    fn default() -> Self {
        Self {
            initial_buffer_size: 1000,
            max_buffer_size: 50000, // Safety limit to prevent memory issues
            trigger_threshold: 0.5,
            initial_chunk_size: 100,
            max_concurrent_sources: 2,
            enable_cascade_integration: true,
        }
    }
}

/// State tracking for each source being processed
#[derive(Debug, Clone)]
struct SourceState {
    /// Index in the sources vector
    source_index: usize,
    /// Current offset for pagination
    current_offset: usize,
    /// Whether this source is exhausted
    is_exhausted: bool,
    /// Whether this source is currently being loaded
    is_loading: bool,
    /// Total items loaded from this source
    total_loaded: usize,
}

/// Generic trait for loading data from sources with active filtering
#[async_trait]
pub trait ActiveDataLoader<T, S> {
    /// Load a chunk of data from an active source with offset and limit
    async fn load_chunk_from_active_source(
        &self,
        database: &Arc<Database>,
        source: &S,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<T>>;

    /// Get the identifier for a source (for logging)
    fn get_source_id(&self, source: &S) -> String;

    /// Get the priority for a source (for logging)
    fn get_source_priority(&self, source: &S) -> i32;

    /// Get the type name for logging
    fn get_type_name(&self) -> &'static str;
}

/// Rolling buffer iterator that implements sophisticated buffer management with cascading buffer integration
pub struct RollingBufferIterator<T, S, L> {
    database: Arc<Database>,
    sources: Vec<S>,
    loader: L,
    config: BufferConfig,
    buffer: VecDeque<T>,
    source_states: Vec<SourceState>,
    current_source_index: usize,
    total_processed: usize,
    exhausted: bool,
    closed: bool,
    /// Chunk manager for coordinating with pipeline cascade system
    chunk_manager: Option<Arc<ChunkSizeManager>>,
    /// Stage name for registering with cascade system
    stage_name: String,
    /// Current dynamic buffer size (adjusted by cascade requests)
    current_buffer_size: usize,
    /// Current dynamic chunk size (adjusted by cascade requests)
    current_chunk_size: usize,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, S, L> RollingBufferIterator<T, S, L>
where
    L: ActiveDataLoader<T, S>,
{
    pub fn new(
        database: Arc<Database>,
        sources: Vec<S>,
        loader: L,
        config: BufferConfig,
    ) -> Self {
        info!(
            "Creating rolling buffer {} iterator for {} sources with initial buffer size {} (trigger at {}%)",
            loader.get_type_name(),
            sources.len(),
            config.initial_buffer_size,
            config.trigger_threshold * 100.0
        );

        // Initialize source states
        let source_states = (0..sources.len())
            .map(|index| SourceState {
                source_index: index,
                current_offset: 0,
                is_exhausted: false,
                is_loading: false,
                total_loaded: 0,
            })
            .collect();

        Self {
            database,
            sources,
            loader,
            current_buffer_size: config.initial_buffer_size,
            current_chunk_size: config.initial_chunk_size,
            config,
            buffer: VecDeque::new(),
            source_states,
            current_source_index: 0,
            total_processed: 0,
            exhausted: false,
            closed: false,
            chunk_manager: None,
            stage_name: "source_loading".to_string(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Create a rolling buffer iterator with chunk manager integration for cascading buffers
    pub fn new_with_chunk_manager(
        database: Arc<Database>,
        sources: Vec<S>,
        loader: L,
        config: BufferConfig,
        chunk_manager: Option<Arc<ChunkSizeManager>>,
        stage_name: String,
    ) -> Self {
        info!(
            "Creating rolling buffer {} iterator for {} sources with initial buffer size {} (trigger at {}%) - cascade integration: {}",
            loader.get_type_name(),
            sources.len(),
            config.initial_buffer_size,
            config.trigger_threshold * 100.0,
            config.enable_cascade_integration && chunk_manager.is_some()
        );
        
        // Initialize source states
        let source_states = (0..sources.len())
            .map(|index| SourceState {
                source_index: index,
                current_offset: 0,
                is_exhausted: false,
                is_loading: false,
                total_loaded: 0,
            })
            .collect();

        Self {
            database,
            sources,
            loader,
            current_buffer_size: config.initial_buffer_size,
            current_chunk_size: config.initial_chunk_size,
            config,
            buffer: VecDeque::new(),
            source_states,
            current_source_index: 0,
            total_processed: 0,
            exhausted: false,
            closed: false,
            chunk_manager,
            stage_name,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Calculate the trigger point for loading next sources (uses current dynamic buffer size)
    fn get_trigger_size(&self) -> usize {
        (self.current_buffer_size as f32 * self.config.trigger_threshold) as usize
    }

    /// Update buffer and chunk sizes based on cascading buffer requests
    async fn update_from_cascade_requests(&mut self) -> Result<bool> {
        if !self.config.enable_cascade_integration {
            return Ok(false);
        }

        let Some(chunk_manager) = &self.chunk_manager else {
            return Ok(false);
        };

        // Get the current optimal chunk size for our stage
        let optimal_chunk_size = chunk_manager.get_chunk_size(&self.stage_name).await;
        // Buffer size should match chunk size for maximum efficiency
        let optimal_buffer_size = optimal_chunk_size
            .min(self.config.max_buffer_size)
            .max(self.config.initial_buffer_size);

        let mut updated = false;

        // Update chunk size if cascade system suggests a different size
        if optimal_chunk_size != self.current_chunk_size {
            debug!(
                "Cascade update: chunk size {} → {} for stage '{}'",
                self.current_chunk_size, optimal_chunk_size, self.stage_name
            );
            self.current_chunk_size = optimal_chunk_size;
            updated = true;
        }

        // Update buffer size if cascade system suggests a different size
        if optimal_buffer_size != self.current_buffer_size {
            debug!(
                "Cascade update: buffer size {} → {} for stage '{}'",
                self.current_buffer_size, optimal_buffer_size, self.stage_name
            );
            self.current_buffer_size = optimal_buffer_size;
            updated = true;
        }

        Ok(updated)
    }

    /// Check if we should trigger loading from next sources
    fn should_trigger_next_source(&self) -> bool {
        let trigger_size = self.get_trigger_size();
        self.buffer.len() <= trigger_size && self.has_unloaded_sources()
    }

    /// Check if there are sources that haven't been started yet
    fn has_unloaded_sources(&self) -> bool {
        self.source_states
            .iter()
            .any(|state| !state.is_exhausted && !state.is_loading && state.current_offset == 0)
    }

    /// Get the next source that should be loaded
    fn get_next_source_to_load(&self) -> Option<usize> {
        self.source_states
            .iter()
            .find(|state| !state.is_exhausted && !state.is_loading && state.current_offset == 0)
            .map(|state| state.source_index)
    }

    /// Load data from a specific source
    async fn load_from_source(&mut self, source_index: usize) -> Result<Vec<T>> {
        if source_index >= self.sources.len() {
            return Ok(Vec::new());
        }

        let source = &self.sources[source_index];
        let state = &mut self.source_states[source_index];

        if state.is_exhausted {
            return Ok(Vec::new());
        }

        state.is_loading = true;

        let source_id = self.loader.get_source_id(source);
        let priority = self.loader.get_source_priority(source);

        debug!(
            "Loading {} chunk from active source {} (priority {}, index {}/{}) at offset {} with limit {}",
            self.loader.get_type_name(),
            source_id,
            priority,
            source_index + 1,
            self.sources.len(),
            state.current_offset,
            self.current_chunk_size
        );

        let data = self
            .loader
            .load_chunk_from_active_source(&self.database, source, state.current_offset, self.current_chunk_size)
            .await;

        state.is_loading = false;

        match data {
            Ok(items) => {
                if items.is_empty() {
                    // Source is exhausted
                    state.is_exhausted = true;
                    debug!(
                        "Source {} (priority {}) exhausted after loading {} total items",
                        source_id, priority, state.total_loaded
                    );
                } else {
                    // Update state for next load
                    state.current_offset += items.len();
                    state.total_loaded += items.len();
                    debug!(
                        "Loaded {} {} items from source {} (priority {}, total: {})",
                        items.len(),
                        self.loader.get_type_name(),
                        source_id,
                        priority,
                        state.total_loaded
                    );
                }
                Ok(items)
            }
            Err(e) => {
                warn!(
                    "Failed to load from source {} (priority {}): {}",
                    source_id, priority, e
                );
                // Mark source as exhausted on error to prevent retry loops
                state.is_exhausted = true;
                Err(e)
            }
        }
    }

    /// Fill buffer from current and next sources as needed
    async fn fill_buffer(&mut self) -> Result<()> {
        // Check for cascade requests and update buffer/chunk sizes if needed
        if let Ok(updated) = self.update_from_cascade_requests().await {
            if updated {
                debug!(
                    "Rolling buffer updated from cascade: buffer_size={}, chunk_size={}",
                    self.current_buffer_size, self.current_chunk_size
                );
            }
        }

        // First, continue loading from current source if it's not exhausted
        if self.current_source_index < self.sources.len() {
            let current_state = &self.source_states[self.current_source_index];
            if !current_state.is_exhausted && self.buffer.len() < self.current_buffer_size {
                match self.load_from_source(self.current_source_index).await {
                    Ok(items) => {
                        for item in items {
                            self.buffer.push_back(item);
                        }
                    }
                    Err(e) => {
                        warn!("Error loading from current source: {}", e);
                        // Continue with other sources
                    }
                }
            }
        }

        // Check if we should trigger loading from next sources
        while self.should_trigger_next_source() && self.buffer.len() < self.current_buffer_size {
            if let Some(next_source_index) = self.get_next_source_to_load() {
                debug!(
                    "Buffer trigger: {} items remaining (trigger at {}), starting source {}",
                    self.buffer.len(),
                    self.get_trigger_size(),
                    next_source_index + 1
                );

                match self.load_from_source(next_source_index).await {
                    Ok(items) => {
                        for item in items {
                            self.buffer.push_back(item);
                        }
                    }
                    Err(e) => {
                        warn!("Error loading from next source {}: {}", next_source_index, e);
                        // Continue with other sources
                    }
                }
            } else {
                break;
            }
        }

        Ok(())
    }

    /// Check if all sources are exhausted
    fn all_sources_exhausted(&self) -> bool {
        self.source_states.iter().all(|state| state.is_exhausted)
    }

    /// Get total items loaded across all sources
    fn get_total_loaded(&self) -> usize {
        self.source_states.iter().map(|state| state.total_loaded).sum()
    }
}

#[async_trait]
impl<T, S, L> PipelineIterator<T> for RollingBufferIterator<T, S, L>
where
    T: Send + Sync,
    S: Send + Sync,
    L: ActiveDataLoader<T, S> + Send + Sync,
{
    async fn next_chunk(&mut self) -> Result<IteratorResult<T>> {
        if self.exhausted || self.closed {
            return Ok(IteratorResult::Exhausted);
        }

        // Fill buffer from sources as needed
        self.fill_buffer().await?;

        // Check if we have data in buffer
        if self.buffer.is_empty() {
            // No data in buffer, check if all sources are exhausted
            if self.all_sources_exhausted() {
                self.exhausted = true;
                info!(
                    "{} rolling buffer iterator exhausted after processing {} total items (loaded: {}) from {} sources",
                    self.loader.get_type_name(),
                    self.total_processed,
                    self.get_total_loaded(),
                    self.sources.len()
                );
                return Ok(IteratorResult::Exhausted);
            } else {
                // Some sources might still have data, but buffer is empty
                // This could be a temporary state, try filling again
                self.fill_buffer().await?;
                
                if self.buffer.is_empty() {
                    // Still no data, likely all sources are exhausted
                    self.exhausted = true;
                    return Ok(IteratorResult::Exhausted);
                }
            }
        }

        // Return a chunk from the buffer
        let chunk_size = std::cmp::min(self.current_chunk_size, self.buffer.len());
        let mut chunk = Vec::with_capacity(chunk_size);

        for _ in 0..chunk_size {
            if let Some(item) = self.buffer.pop_front() {
                chunk.push(item);
            } else {
                break;
            }
        }

        if chunk.is_empty() {
            self.exhausted = true;
            return Ok(IteratorResult::Exhausted);
        }

        self.total_processed += chunk.len();

        debug!(
            "Returning rolling buffer chunk of {} {} items (total processed: {}, buffer remaining: {})",
            chunk.len(),
            self.loader.get_type_name(),
            self.total_processed,
            self.buffer.len()
        );

        Ok(IteratorResult::Chunk(chunk))
    }

    fn is_exhausted(&self) -> bool {
        self.exhausted || self.closed
    }

    async fn close(&mut self) -> Result<()> {
        if !self.closed {
            self.closed = true;
            self.buffer.clear();
            info!(
                "{} rolling buffer iterator closed early after processing {} items (loaded: {})",
                self.loader.get_type_name(),
                self.total_processed,
                self.get_total_loaded()
            );
        }
        Ok(())
    }

    async fn next_chunk_with_size(&mut self, chunk_size: usize) -> Result<IteratorResult<T>> {
        let old_chunk_size = self.current_chunk_size;
        self.current_chunk_size = chunk_size;
        let result = self.next_chunk().await;
        self.current_chunk_size = old_chunk_size;
        result
    }

    async fn set_buffer_size(&mut self, buffer_size: usize) -> Result<()> {
        let clamped_size = buffer_size.min(self.config.max_buffer_size);
        self.current_buffer_size = clamped_size;
        debug!(
            "Rolling buffer size updated to {} for {} iterator (requested: {}, max: {})",
            clamped_size,
            self.loader.get_type_name(),
            buffer_size,
            self.config.max_buffer_size
        );
        Ok(())
    }

    fn get_current_buffer_size(&self) -> usize {
        self.buffer.len()
    }

    fn get_chunk_size(&self) -> usize {
        self.current_chunk_size
    }
    
    fn reset(&mut self) -> Result<()> {
        self.buffer.clear();
        self.current_source_index = 0;
        // Reset all source states
        for state in &mut self.source_states {
            state.current_offset = 0;
            state.is_exhausted = false;
        }
        self.total_processed = 0;
        self.exhausted = false;
        self.closed = false;
        info!(
            "{} rolling buffer iterator reset to beginning", 
            self.loader.get_type_name()
        );
        Ok(())
    }
}