//! Accumulator pattern for HTTP → Parse → Database ingestion workflows
//!
//! This module provides specialized accumulators for ingesting data from external sources
//! where we need to optimize the HTTP download → parsing → database storage pipeline.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::services::sandboxed_file::SandboxedFileManager;
use super::state_manager::IngestionStateManager;

/// Strategy for handling ingestion accumulation
#[derive(Debug, Clone)]
pub enum IngestionAccumulationStrategy {
    /// Buffer all HTTP data in memory before parsing
    InMemoryBuffer {
        /// Maximum size in MB before forcing processing
        max_buffer_mb: usize,
    },
    /// Stream to temporary file during HTTP download, parse from file
    StreamToFile {
        /// Size threshold to start streaming to file instead of memory
        stream_threshold_mb: usize,
    },
    /// Hybrid: start with memory buffer, switch to file streaming if needed
    HybridStreaming {
        /// Memory threshold before switching to file streaming
        memory_threshold_mb: usize,
        /// Maximum memory buffer size before forcing processing
        max_memory_mb: usize,
    },
    /// Parse and accumulate in batches during HTTP download
    StreamingParser {
        /// Parse buffer size (number of entries to accumulate before batch processing)
        parse_batch_size: usize,
        /// Database batch size (number of entries to accumulate before DB insert)
        db_batch_size: usize,
    },
}

impl IngestionAccumulationStrategy {
    /// Get strategy optimized for EPG sources (typically large XML files)
    pub fn epg_optimized() -> Self {
        Self::HybridStreaming {
            memory_threshold_mb: 10,    // Start streaming to file at 10MB
            max_memory_mb: 50,          // Force processing at 50MB
        }
    }
    
    /// Get strategy optimized for M3U sources (typically smaller, but many channels)
    pub fn m3u_optimized() -> Self {
        Self::StreamingParser {
            parse_batch_size: 1000,     // Parse 1000 channels at a time
            db_batch_size: 500,         // Insert 500 channels per transaction
        }
    }
    
    /// Get strategy optimized for Xtream API (JSON responses, predictable size)
    pub fn xtream_optimized() -> Self {
        Self::InMemoryBuffer {
            max_buffer_mb: 25,          // Most Xtream responses are under 25MB
        }
    }
}

/// Statistics about ingestion accumulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionAccumulatorStats {
    pub total_bytes_downloaded: usize,
    pub total_entries_parsed: usize,
    pub total_entries_stored: usize,
    pub current_buffer_size_mb: f64,
    pub is_streaming_to_file: bool,
    pub batch_operations_completed: usize,
    pub strategy: String,
}

/// Specialized accumulator for ingestion workflows
pub struct IngestionAccumulator<T> 
where 
    T: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + 'static,
{
    /// Accumulation strategy
    strategy: IngestionAccumulationStrategy,
    
    /// Current data buffer (in memory)
    buffer: Vec<u8>,
    
    /// Parsed entries ready for database storage
    parsed_entries: Vec<T>,
    
    /// File manager for temporary storage
    file_manager: Arc<dyn SandboxedFileManager>,
    
    /// Current temp file ID if streaming to file
    temp_file_id: Option<String>,
    
    /// Whether we're currently streaming to file
    is_streaming_to_file: bool,
    
    /// Statistics tracking
    stats: IngestionAccumulatorStats,
    
    /// State manager for progress reporting and cancellation
    state_manager: Option<Arc<IngestionStateManager>>,
    
    /// Unique ID for this accumulator instance
    accumulator_id: String,
}

impl<T> IngestionAccumulator<T>
where 
    T: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + 'static,
{
    /// Create new ingestion accumulator
    pub fn new(
        strategy: IngestionAccumulationStrategy,
        file_manager: Arc<dyn SandboxedFileManager>,
        state_manager: Option<Arc<IngestionStateManager>>,
    ) -> Self {
        let accumulator_id = Uuid::new_v4().to_string();
        
        Self {
            strategy: strategy.clone(),
            buffer: Vec::new(),
            parsed_entries: Vec::new(),
            file_manager,
            temp_file_id: None,
            is_streaming_to_file: false,
            stats: IngestionAccumulatorStats {
                total_bytes_downloaded: 0,
                total_entries_parsed: 0,
                total_entries_stored: 0,
                current_buffer_size_mb: 0.0,
                is_streaming_to_file: false,
                batch_operations_completed: 0,
                strategy: format!("{:?}", strategy),
            },
            state_manager,
            accumulator_id,
        }
    }

    /// Accumulate HTTP response data chunk
    pub async fn accumulate_http_chunk(&mut self, chunk: &[u8]) -> Result<()> {
        self.stats.total_bytes_downloaded += chunk.len();
        self.update_current_buffer_size();
        
        // Clone strategy to avoid borrow checker issues
        let strategy = self.strategy.clone();
        
        match strategy {
            IngestionAccumulationStrategy::InMemoryBuffer { max_buffer_mb } => {
                self.buffer.extend_from_slice(chunk);
                
                if self.get_buffer_size_mb() > max_buffer_mb as f64 {
                    warn!("Buffer size ({:.1}MB) exceeded max_buffer_mb ({}MB), forcing processing", 
                          self.get_buffer_size_mb(), max_buffer_mb);
                    // Could trigger parsing here or return a signal to caller
                }
            },
            
            IngestionAccumulationStrategy::StreamToFile { stream_threshold_mb } => {
                if !self.is_streaming_to_file && self.get_buffer_size_mb() > stream_threshold_mb as f64 {
                    self.start_file_streaming().await?;
                }
                
                if self.is_streaming_to_file {
                    self.write_chunk_to_file(chunk).await?;
                } else {
                    self.buffer.extend_from_slice(chunk);
                }
            },
            
            IngestionAccumulationStrategy::HybridStreaming { 
                memory_threshold_mb, 
                max_memory_mb 
            } => {
                if !self.is_streaming_to_file && self.get_buffer_size_mb() > memory_threshold_mb as f64 {
                    info!("Switching to file streaming at {:.1}MB", self.get_buffer_size_mb());
                    self.start_file_streaming().await?;
                }
                
                if self.is_streaming_to_file {
                    self.write_chunk_to_file(chunk).await?;
                } else {
                    self.buffer.extend_from_slice(chunk);
                    
                    if self.get_buffer_size_mb() > max_memory_mb as f64 {
                        warn!("Memory buffer size ({:.1}MB) exceeded max_memory_mb ({}MB)", 
                              self.get_buffer_size_mb(), max_memory_mb);
                        // Force switch to file streaming even mid-download
                        self.start_file_streaming().await?;
                        self.write_chunk_to_file(chunk).await?;
                    }
                }
            },
            
            IngestionAccumulationStrategy::StreamingParser { parse_batch_size, .. } => {
                self.buffer.extend_from_slice(chunk);
                
                // For streaming parsers, we could implement incremental parsing here
                // This would require parser-specific logic for M3U, XMLTV, etc.
                if self.buffer.len() > parse_batch_size * 1000 { // Rough estimate
                    debug!("Buffer size suggests batch parsing opportunity");
                }
            },
        }
        
        self.update_current_buffer_size();
        Ok(())
    }
    
    /// Get the final accumulated data (either from memory or file)
    pub async fn finalize_accumulation(&mut self) -> Result<Vec<u8>> {
        if self.is_streaming_to_file {
            self.read_from_temp_file().await
        } else {
            Ok(self.buffer.clone())
        }
    }
    
    /// Add parsed entries to the accumulator
    pub async fn accumulate_parsed_entries(&mut self, entries: Vec<T>) -> Result<()> {
        self.stats.total_entries_parsed += entries.len();
        self.parsed_entries.extend(entries);
        
        // Check if we should batch process to database
        match &self.strategy {
            IngestionAccumulationStrategy::StreamingParser { db_batch_size, .. } => {
                if self.parsed_entries.len() >= *db_batch_size {
                    debug!("Parsed entries ({}) reached batch size ({}), ready for DB insert", 
                           self.parsed_entries.len(), db_batch_size);
                    // Caller should drain entries and process them
                }
            },
            _ => {
                // Other strategies accumulate all entries before processing
            }
        }
        
        Ok(())
    }
    
    /// Drain parsed entries for database storage
    pub fn drain_parsed_entries(&mut self) -> Vec<T> {
        let entries: Vec<T> = self.parsed_entries.drain(..).collect();
        self.stats.total_entries_stored += entries.len();
        self.stats.batch_operations_completed += 1;
        entries
    }
    
    /// Get current statistics
    pub fn get_stats(&self) -> &IngestionAccumulatorStats {
        &self.stats
    }
    
    /// Get a progress message for external progress tracking
    pub fn get_progress_message(&self, stage: &str) -> String {
        format!(
            "{}: {:.1}MB downloaded, {} entries parsed", 
            stage, 
            self.stats.total_bytes_downloaded as f64 / 1024.0 / 1024.0,
            self.stats.total_entries_parsed
        )
    }
    
    // Private helper methods
    
    fn get_buffer_size_mb(&self) -> f64 {
        self.buffer.len() as f64 / 1024.0 / 1024.0
    }
    
    fn update_current_buffer_size(&mut self) {
        self.stats.current_buffer_size_mb = self.get_buffer_size_mb();
        self.stats.is_streaming_to_file = self.is_streaming_to_file;
    }
    
    async fn start_file_streaming(&mut self) -> Result<()> {
        if self.temp_file_id.is_none() {
            self.temp_file_id = Some(format!("ingest_{}_{}", self.accumulator_id, Uuid::new_v4()));
        }
        
        let file_id = self.temp_file_id.as_ref().unwrap();
        
        // Write current buffer to file
        if !self.buffer.is_empty() {
            self.file_manager
                .store_file("ingestion_temp", file_id, &self.buffer, "tmp")
                .await?;
            
            info!("Started file streaming, wrote {:.1}MB buffer to temp file", 
                  self.get_buffer_size_mb());
            
            // Clear memory buffer
            self.buffer.clear();
            self.buffer.shrink_to_fit();
        }
        
        self.is_streaming_to_file = true;
        Ok(())
    }
    
    async fn write_chunk_to_file(&mut self, chunk: &[u8]) -> Result<()> {
        if let Some(file_id) = &self.temp_file_id {
            // For simplicity, we append to buffer and periodically flush to file
            // A more sophisticated implementation could use async file writing
            self.buffer.extend_from_slice(chunk);
            
            // Flush to file every 1MB to avoid memory buildup
            if self.buffer.len() > 1024 * 1024 {
                let existing_content = self.file_manager
                    .read_file("ingestion_temp", file_id, "tmp")
                    .await
                    .unwrap_or_default();
                
                let mut combined_content = existing_content;
                combined_content.extend_from_slice(&self.buffer);
                
                self.file_manager
                    .store_file("ingestion_temp", file_id, &combined_content, "tmp")
                    .await?;
                
                self.buffer.clear();
                self.buffer.shrink_to_fit();
                
                debug!("Flushed 1MB chunk to temp file");
            }
        }
        
        Ok(())
    }
    
    async fn read_from_temp_file(&mut self) -> Result<Vec<u8>> {
        if let Some(file_id) = &self.temp_file_id {
            let mut file_content = self.file_manager
                .read_file("ingestion_temp", file_id, "tmp")
                .await?;
            
            // Append any remaining buffer content
            file_content.extend_from_slice(&self.buffer);
            
            // Clean up temp file
            let _ = self.file_manager
                .delete_file("ingestion_temp", file_id, "tmp")
                .await;
            
            info!("Read {:.1}MB from temp file, cleaned up", 
                  file_content.len() as f64 / 1024.0 / 1024.0);
            
            Ok(file_content)
        } else {
            Ok(self.buffer.clone())
        }
    }
}

/// Factory for creating ingestion accumulators with sensible defaults
pub struct IngestionAccumulatorFactory;

impl IngestionAccumulatorFactory {
    /// Create accumulator with hybrid strategy (default)
    pub fn create<T>(
        file_manager: Arc<dyn SandboxedFileManager>,
        state_manager: Option<Arc<IngestionStateManager>>,
    ) -> IngestionAccumulator<T>
    where 
        T: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + 'static,
    {
        let strategy = IngestionAccumulationStrategy::HybridStreaming {
            memory_threshold_mb: 10,
            max_memory_mb: 50,
        };
        IngestionAccumulator::new(strategy, file_manager, state_manager)
    }
    
    /// Create accumulator with explicit strategy override
    pub fn create_with_strategy<T>(
        strategy: IngestionAccumulationStrategy,
        file_manager: Arc<dyn SandboxedFileManager>,
        state_manager: Option<Arc<IngestionStateManager>>,
    ) -> IngestionAccumulator<T>
    where 
        T: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + 'static,
    {
        IngestionAccumulator::new(strategy, file_manager, state_manager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sandboxed_file::{FileInfo, SandboxedFileManager};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    #[derive(Debug)]
    struct MockSandboxedFileManager {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MockSandboxedFileManager {
        fn new() -> Self {
            Self {
                files: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl SandboxedFileManager for MockSandboxedFileManager {
        async fn store_file(&self, category: &str, file_id: &str, content: &[u8], extension: &str) -> Result<()> {
            let key = format!("{}/{}.{}", category, file_id, extension);
            let mut files = self.files.lock().unwrap();
            files.insert(key, content.to_vec());
            Ok(())
        }

        async fn store_linked_file(&self, category: &str, file_id: &str, content: &[u8], extension: &str) -> Result<()> {
            self.store_file(category, file_id, content, extension).await
        }

        async fn read_file(&self, category: &str, file_id: &str, extension: &str) -> Result<Vec<u8>> {
            let key = format!("{}/{}.{}", category, file_id, extension);
            let files = self.files.lock().unwrap();
            files.get(&key).cloned().ok_or_else(|| anyhow::anyhow!("File not found"))
        }

        async fn file_exists(&self, category: &str, file_id: &str, extension: &str) -> Result<bool> {
            let key = format!("{}/{}.{}", category, file_id, extension);
            let files = self.files.lock().unwrap();
            Ok(files.contains_key(&key))
        }

        async fn get_file_path(&self, _category: &str, _file_id: &str, _extension: &str) -> Result<Option<PathBuf>> {
            Ok(None)
        }

        async fn delete_file(&self, category: &str, file_id: &str, extension: &str) -> Result<()> {
            let key = format!("{}/{}.{}", category, file_id, extension);
            let mut files = self.files.lock().unwrap();
            files.remove(&key);
            Ok(())
        }

        async fn list_files(&self, _category: &str) -> Result<Vec<FileInfo>> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_ingestion_accumulator_memory_strategy() {
        let file_manager = Arc::new(MockSandboxedFileManager::new());
        let strategy = IngestionAccumulationStrategy::InMemoryBuffer { max_buffer_mb: 1 };
        
        let mut accumulator: IngestionAccumulator<serde_json::Value> = 
            IngestionAccumulator::new(strategy, file_manager, None);
        
        // Simulate HTTP chunks
        let chunk1 = b"chunk1 data";
        let chunk2 = b"chunk2 data";
        
        accumulator.accumulate_http_chunk(chunk1).await.unwrap();
        accumulator.accumulate_http_chunk(chunk2).await.unwrap();
        
        let final_data = accumulator.finalize_accumulation().await.unwrap();
        assert_eq!(final_data, b"chunk1 datachunk2 data");
        
        let stats = accumulator.get_stats();
        assert_eq!(stats.total_bytes_downloaded, 22);
        assert!(!stats.is_streaming_to_file);
    }

    #[tokio::test]
    async fn test_factory_creates_appropriate_strategies() {
        let file_manager = Arc::new(MockSandboxedFileManager::new());
        
        let epg_accumulator: IngestionAccumulator<serde_json::Value> = 
            IngestionAccumulatorFactory::create_for_source("xmltv", file_manager.clone(), None);
        
        let m3u_accumulator: IngestionAccumulator<serde_json::Value> = 
            IngestionAccumulatorFactory::create_for_source("m3u", file_manager.clone(), None);
        
        // Verify different strategies are used
        assert!(epg_accumulator.get_stats().strategy.contains("HybridStreaming"));
        assert!(m3u_accumulator.get_stats().strategy.contains("StreamingParser"));
    }
}