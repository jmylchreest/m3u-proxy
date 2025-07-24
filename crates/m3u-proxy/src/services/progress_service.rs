//! Universal Progress Service
//!
//! This service provides a unified interface for tracking and reporting progress
//! across all operations in the system, including:
//! - Source ingestion (M3U, Xtream, EPG)  
//! - Proxy regeneration
//! - Pipeline processing
//! - Background tasks
//! - Any long-running operations

use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;
use serde::{Deserialize, Serialize};

use crate::models::*;
use crate::ingestor::IngestionStateManager;

/// Universal progress callback type that works with any operation
pub type UniversalProgressCallback = Box<dyn Fn(UniversalProgress) + Send + Sync>;

/// Operation types that can report progress
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    /// Stream source ingestion (M3U, Xtream)
    StreamIngestion,
    /// EPG source ingestion
    EpgIngestion,
    /// Proxy regeneration
    ProxyRegeneration,
    /// Pipeline processing
    Pipeline,
    /// Data mapping operations
    DataMapping,
    /// Logo caching operations
    LogoCaching,
    /// Channel filtering
    Filtering,
    /// Background maintenance tasks
    Maintenance,
    /// Database operations
    Database,
    /// Custom operation type
    Custom(String),
}

/// Universal progress state that works for all operation types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UniversalState {
    /// Operation is idle/not started
    Idle,
    /// Preparing to start (authentication, validation, etc.)
    Preparing,
    /// Actively connecting to external resource
    Connecting,
    /// Downloading/fetching data
    Downloading,
    /// Processing/parsing data
    Processing,
    /// Saving results to database
    Saving,
    /// Cleaning up resources
    Cleanup,
    /// Operation completed successfully
    Completed,
    /// Operation failed with error
    Error,
    /// Operation was cancelled
    Cancelled,
}

/// Universal progress information for any operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniversalProgress {
    pub operation_id: Uuid,
    pub operation_type: OperationType,
    pub operation_name: String, // Human-readable name like "Ingest Xtream Source: MyProvider"
    pub state: UniversalState,
    pub current_step: String,
    pub progress_percentage: Option<f64>,
    pub items_processed: Option<usize>,
    pub items_total: Option<usize>,
    pub bytes_processed: Option<u64>,
    pub bytes_total: Option<u64>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>, // Operation-specific data
}

impl UniversalProgress {
    pub fn new(
        operation_id: Uuid,
        operation_type: OperationType,
        operation_name: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            operation_id,
            operation_type,
            operation_name,
            state: UniversalState::Idle,
            current_step: "Starting...".to_string(),
            progress_percentage: None,
            items_processed: None,
            items_total: None,
            bytes_processed: None,
            bytes_total: None,
            started_at: now,
            updated_at: now,
            completed_at: None,
            error_message: None,
            metadata: HashMap::new(),
        }
    }
    
    pub fn update_step(mut self, step: String) -> Self {
        self.current_step = step;
        self.updated_at = Utc::now();
        self
    }
    
    pub fn update_percentage(mut self, percentage: f64) -> Self {
        self.progress_percentage = Some(percentage);
        self.updated_at = Utc::now();
        self
    }
    
    pub fn update_items(mut self, processed: usize, total: Option<usize>) -> Self {
        self.items_processed = Some(processed);
        if let Some(t) = total {
            self.items_total = Some(t);
            self.progress_percentage = Some((processed as f64 / t as f64) * 100.0);
        }
        self.updated_at = Utc::now();
        self
    }
    
    pub fn update_bytes(mut self, processed: u64, total: Option<u64>) -> Self {
        self.bytes_processed = Some(processed);
        if let Some(t) = total {
            self.bytes_total = Some(t);
            self.progress_percentage = Some((processed as f64 / t as f64) * 100.0);
        }
        self.updated_at = Utc::now();
        self
    }
    
    pub fn set_state(mut self, state: UniversalState) -> Self {
        let is_complete = matches!(state, UniversalState::Completed | UniversalState::Error | UniversalState::Cancelled);
        self.state = state;
        if is_complete {
            self.completed_at = Some(Utc::now());
        }
        self.updated_at = Utc::now();
        self
    }
    
    pub fn set_error(mut self, error: String) -> Self {
        self.error_message = Some(error);
        self.state = UniversalState::Error;
        self.completed_at = Some(Utc::now());
        self.updated_at = Utc::now();
        self
    }
    
    pub fn add_metadata(mut self, key: String, value: serde_json::Value) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Maximum number of operations to keep in memory
const MAX_OPERATIONS: usize = 50;

/// Universal progress service that handles all operation types
#[derive(Clone)]
pub struct ProgressService {
    /// Active operations progress
    progress: Arc<RwLock<HashMap<Uuid, UniversalProgress>>>,
    /// Broadcast channel for real-time progress updates
    broadcast_tx: broadcast::Sender<UniversalProgress>,
    /// Legacy ingestion state manager for backward compatibility
    ingestion_state_manager: IngestionStateManager,
}

impl ProgressService {
    pub fn new(ingestion_state_manager: IngestionStateManager) -> Self {
        let (broadcast_tx, _) = broadcast::channel(1000);
        Self {
            progress: Arc::new(RwLock::new(HashMap::new())),
            broadcast_tx,
            ingestion_state_manager,
        }
    }
    
    /// Subscribe to real-time progress updates
    pub fn subscribe(&self) -> broadcast::Receiver<UniversalProgress> {
        let receiver = self.broadcast_tx.subscribe();
        let count = self.broadcast_tx.receiver_count();
        tracing::debug!("New SSE subscriber created, total subscriber count: {}", count);
        receiver
    }
    
    /// Get current receiver count for debugging
    pub fn get_receiver_count(&self) -> usize {
        self.broadcast_tx.receiver_count()
    }
    
    /// Start tracking a new operation
    pub async fn start_operation(
        &self,
        operation_id: Uuid,
        operation_type: OperationType,
        operation_name: String,
    ) -> UniversalProgressCallback {
        let initial_progress = UniversalProgress::new(operation_id, operation_type, operation_name);
        
        // Store initial progress
        {
            let mut progress_map = self.progress.write().await;
            progress_map.insert(operation_id, initial_progress.clone());
        }
        
        // Clean up old operations to maintain size limit
        self.cleanup_by_size().await;
        
        // Broadcast initial state
        let _ = self.broadcast_tx.send(initial_progress);
        
        // Return callback for progress updates
        let progress = self.progress.clone();
        let broadcast_tx = self.broadcast_tx.clone();
        let ingestion_manager = self.ingestion_state_manager.clone();
        let service_clone = self.clone();
        
        Box::new(move |updated_progress: UniversalProgress| {
            // Clone data for async execution
            let progress_clone = progress.clone();
            let broadcast_clone = broadcast_tx.clone();
            let ingestion_clone = ingestion_manager.clone();
            let service_clone = service_clone.clone();
            let updated = updated_progress.clone();
            
            // Immediately broadcast the update synchronously
            // Broadcast channels automatically drop oldest messages when full, so this is safe
            match broadcast_clone.send(updated.clone()) {
                Ok(receiver_count) => {
                    tracing::trace!("Successfully broadcast progress update for operation {} ({}) to {} receivers", 
                                  updated.operation_id, updated.current_step, receiver_count);
                }
                Err(_) => {
                    // This is normal when no SSE clients are connected - don't log as warning
                    tracing::debug!("No active SSE subscribers for progress update: {} ({})", 
                                  updated.operation_id, updated.current_step);
                }
            }
            
            // Then spawn background task for async operations (storage and legacy sync)
            tokio::spawn(async move {
                tracing::debug!("Progress callback async task started for operation {} ({})", 
                              updated.operation_id, updated.current_step);
                
                // Store updated progress
                {
                    let mut progress_map = progress_clone.write().await;
                    progress_map.insert(updated.operation_id, updated.clone());
                    tracing::debug!("Progress stored in map for operation {} - total operations: {}", 
                                  updated.operation_id, progress_map.len());
                }
                
                // Clean up old operations to maintain size limit
                service_clone.cleanup_by_size().await;
                
                // Update legacy ingestion state manager if this is an ingestion operation
                if matches!(updated.operation_type, OperationType::StreamIngestion | OperationType::EpgIngestion) {
                    let legacy_state = convert_universal_to_legacy_state(&updated.state);
                    let legacy_progress = convert_universal_to_legacy_progress(&updated);
                    
                    ingestion_clone.update_progress(
                        updated.operation_id,
                        legacy_state,
                        legacy_progress,
                    ).await;
                }
            });
        })
    }
    
    /// Get progress for a specific operation
    pub async fn get_progress(&self, operation_id: Uuid) -> Option<UniversalProgress> {
        let progress_map = self.progress.read().await;
        progress_map.get(&operation_id).cloned()
    }
    
    /// Get all active operations
    pub async fn get_all_progress(&self) -> HashMap<Uuid, UniversalProgress> {
        let progress_map = self.progress.read().await;
        progress_map.clone()
    }
    
    /// Get operations by type
    pub async fn get_progress_by_type(&self, operation_type: OperationType) -> HashMap<Uuid, UniversalProgress> {
        let progress_map = self.progress.read().await;
        progress_map
            .iter()
            .filter(|(_, progress)| progress.operation_type == operation_type)
            .map(|(id, progress)| (*id, progress.clone()))
            .collect()
    }
    
    /// Complete an operation
    pub async fn complete_operation(&self, operation_id: Uuid) {
        {
            let mut progress_map = self.progress.write().await;
            if let Some(mut progress) = progress_map.get(&operation_id).cloned() {
                progress = progress.set_state(UniversalState::Completed);
                progress_map.insert(operation_id, progress.clone());
                let _ = self.broadcast_tx.send(progress);
            }
        }
        
        // Clean up old operations to maintain size limit
        self.cleanup_by_size().await;
    }
    
    /// Mark operation as failed
    pub async fn fail_operation(&self, operation_id: Uuid, error: String) {
        {
            let mut progress_map = self.progress.write().await;
            if let Some(mut progress) = progress_map.get(&operation_id).cloned() {
                progress = progress.set_error(error);
                progress_map.insert(operation_id, progress.clone());
                let _ = self.broadcast_tx.send(progress);
            }
        }
        
        // Clean up old operations to maintain size limit
        self.cleanup_by_size().await;
    }
    
    /// Cancel an operation
    pub async fn cancel_operation(&self, operation_id: Uuid) {
        {
            let mut progress_map = self.progress.write().await;
            if let Some(mut progress) = progress_map.get(&operation_id).cloned() {
                progress = progress.set_state(UniversalState::Cancelled);
                progress_map.insert(operation_id, progress.clone());
                let _ = self.broadcast_tx.send(progress);
            }
        }
        
        // Clean up old operations to maintain size limit
        self.cleanup_by_size().await;
    }
    
    /// Clean up completed operations older than specified duration
    pub async fn cleanup_old_operations(&self, max_age: chrono::Duration) {
        let mut progress_map = self.progress.write().await;
        let cutoff = Utc::now() - max_age;
        
        progress_map.retain(|_, progress| {
            match progress.state {
                UniversalState::Completed | UniversalState::Error | UniversalState::Cancelled => {
                    progress.completed_at.map_or(true, |completed_at| completed_at > cutoff)
                }
                _ => true, // Keep active operations
            }
        });
    }
    
    /// Clean up operations to limit total count, keeping the most recent operations
    async fn cleanup_by_size(&self) {
        let mut progress_map = self.progress.write().await;
        
        if progress_map.len() <= MAX_OPERATIONS {
            return;
        }
        
        // First, always keep active operations
        let (active_ops, completed_ops): (Vec<_>, Vec<_>) = progress_map
            .iter()
            .partition(|(_, progress)| !matches!(
                progress.state,
                UniversalState::Completed | UniversalState::Error | UniversalState::Cancelled
            ));
        
        // If we have too many active operations alone, keep all of them (they're important)
        if active_ops.len() >= MAX_OPERATIONS {
            tracing::warn!(
                "Have {} active operations, which exceeds MAX_OPERATIONS ({}). Keeping all active operations.",
                active_ops.len(),
                MAX_OPERATIONS
            );
            return;
        }
        
        // Calculate how many completed operations we can keep
        let max_completed = MAX_OPERATIONS - active_ops.len();
        
        if completed_ops.len() <= max_completed {
            return; // No cleanup needed
        }
        
        // Sort completed operations by updated_at (most recent first)
        let mut completed_sorted: Vec<_> = completed_ops.into_iter().collect();
        completed_sorted.sort_by(|a, b| b.1.updated_at.cmp(&a.1.updated_at));
        
        // Keep only the most recent completed operations
        let to_keep: HashSet<Uuid> = active_ops
            .into_iter()
            .map(|(id, _)| *id)
            .chain(
                completed_sorted
                    .into_iter()
                    .take(max_completed)
                    .map(|(id, _)| *id)
            )
            .collect();
        
        let original_count = progress_map.len();
        progress_map.retain(|id, _| to_keep.contains(id));
        let new_count = progress_map.len();
        
        if original_count != new_count {
            tracing::debug!(
                "Cleaned up {} old operations (kept {} most recent out of {})",
                original_count - new_count,
                new_count,
                original_count
            );
        }
    }
    
    /// Get legacy ingestion state manager for backward compatibility
    pub fn get_ingestion_state_manager(&self) -> &IngestionStateManager {
        &self.ingestion_state_manager
    }
    
    
}

/// Convert universal state to legacy ingestion state
fn convert_universal_to_legacy_state(state: &UniversalState) -> IngestionState {
    match state {
        UniversalState::Idle => IngestionState::Idle,
        UniversalState::Preparing => IngestionState::Connecting,
        UniversalState::Connecting => IngestionState::Connecting,
        UniversalState::Downloading => IngestionState::Downloading,
        UniversalState::Processing => IngestionState::Processing,
        UniversalState::Saving => IngestionState::Saving,
        UniversalState::Cleanup => IngestionState::Processing,
        UniversalState::Completed => IngestionState::Completed,
        UniversalState::Error => IngestionState::Error,
        UniversalState::Cancelled => IngestionState::Error,
    }
}

/// Convert universal progress to legacy progress info
fn convert_universal_to_legacy_progress(progress: &UniversalProgress) -> ProgressInfo {
    ProgressInfo {
        current_step: progress.current_step.clone(),
        total_bytes: progress.bytes_total,
        downloaded_bytes: progress.bytes_processed,
        channels_parsed: progress.items_processed,
        channels_saved: None, // Not tracked in universal progress yet
        programs_parsed: None, // Not tracked in universal progress yet
        programs_saved: None,  // Not tracked in universal progress yet
        percentage: progress.progress_percentage,
    }
}


impl Default for ProgressService {
    fn default() -> Self {
        Self::new(IngestionStateManager::new())
    }
}