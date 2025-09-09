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
use tracing::{debug, info, warn};
use uuid::Uuid;
use serde::{Deserialize, Serialize};

use crate::ingestor::IngestionStateManager;

/// Universal progress callback type for reporting operation progress
pub type UniversalProgressCallback = Box<dyn Fn(UniversalProgress) + Send + Sync>;

/// Progress context for compatibility with old API
#[derive(Debug, Clone)]
pub struct ProgressContext {
    pub resource_type: String,
    pub operation_type: OperationType,
    pub operation_id: Uuid,
    pub owner_id: Uuid,
    pub owner_type: String,
    pub operation_name: String,
}

impl ProgressContext {
    pub fn new(resource_type: String, operation_type: OperationType) -> Self {
        Self {
            resource_type,
            operation_type,
            operation_id: Uuid::new_v4(),
            owner_id: Uuid::new_v4(),
            owner_type: "default".to_string(),
            operation_name: "Default Operation".to_string(),
        }
    }
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

/// Stage information for centralized progress management
#[derive(Debug, Clone)]
pub struct ProgressStage {
    pub id: String,
    pub name: String,
    pub progress_percentage: f64,
    pub is_completed: bool,
    pub stage_description: Option<String>,
}

impl ProgressStage {
    /// Check if this stage is complete
    pub fn is_complete(&self) -> bool {
        self.is_completed
    }
}

/// Lightweight updater for individual pipeline stages
/// Type alias for backward compatibility
pub type StageUpdater = ProgressStageUpdater;

#[derive(Clone)]
pub struct ProgressStageUpdater {
    stage_id: String,
    manager: Arc<ProgressManager>,
}

/// Progress stage information for API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressStageInfo {
    pub id: String,
    pub name: String,
    pub percentage: f64,
    pub stage_description: Option<String>,
}


impl ProgressStageUpdater {
    /// Update this stage's progress and recalculate overall progress
    pub async fn update_progress(&self, stage_percentage: f64, stage_description: &str) {
        self.manager.update_stage_progress(&self.stage_id, stage_percentage, stage_description).await;
    }
    
    /// Mark this stage as completed and move to next stage
    pub async fn complete_stage(&self) {
        self.manager.complete_stage(&self.stage_id).await;
    }
    
    /// Mark the entire operation as completed
    pub async fn complete_operation(&self) {
        self.manager.complete_operation().await;
    }
    
    /// Mark the entire operation as failed with an error message
    pub async fn fail_operation(&self, error_message: &str) {
        self.manager.fail(error_message).await;
    }
    
    /// Set this stage as the current active stage
    pub async fn set_as_current_stage(&self) {
        self.manager.set_current_stage(&self.stage_id).await;
    }
    
    /// Update items processed for this stage
    pub async fn update_items(&self, processed: usize, total: Option<usize>, stage_description: &str) {
        let percentage = if let Some(total) = total {
            if total > 0 { (processed as f64 / total as f64) * 100.0 } else { 0.0 }
        } else {
            0.0
        };
        self.update_progress(percentage, stage_description).await;
    }
    
    /// Update progress for preprocessing/setup work (database fetches, initialization)
    /// Sets progress to 5% to indicate setup is complete and main processing can begin
    pub async fn complete_preprocessing(&self, stage_description: &str) {
        self.update_progress(5.0, stage_description).await;
    }
    
    /// Check if cancellation has been requested for this operation
    /// Returns true if the operation should be cancelled
    pub async fn is_cancellation_requested(&self) -> bool {
        self.manager.is_cancellation_requested().await
    }
    
    /// Update progress for channel/program processing with 5-95% range calculation
    /// Formula: 5% + ((channels_processed + programs_processed) / (total_channels + total_programs)) * 90%
    /// Use this after preprocessing is complete (5%) for the main processing work
    pub async fn update_channel_program_progress(
        &self,
        channels_processed: usize,
        programs_processed: usize,
        total_channels: usize,
        total_programs: usize,
        stage_description: &str,
    ) {
        let total_items = total_channels + total_programs;
        let processed_items = channels_processed + programs_processed;
        
        let percentage = if total_items > 0 {
            // 5% preprocessing + up to 90% for processing = 5-95% range
            5.0 + ((processed_items as f64 / total_items as f64) * 90.0)
        } else {
            5.0 // Stay at 5% if no items to process
        };
        
        self.update_progress(percentage.clamp(5.0, 95.0), stage_description).await;
    }
    
    /// Get stage identifier
    pub fn stage_id(&self) -> &str {
        &self.stage_id
    }
}

/// Individual stage information for unified progress tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageInfo {
    pub id: String,
    pub name: String,
    pub percentage: f64,
    pub state: UniversalState,
    pub stage_step: String,
}

/// Progress update event types for SSE broadcasting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniversalProgress {
    pub id: Uuid,
    pub operation_id: Uuid, // Added for consistency with API
    pub operation_name: String,
    pub operation_type: OperationType,
    pub owner_type: String,
    pub owner_id: Uuid,
    pub state: UniversalState,
    pub current_stage: String,
    pub overall_percentage: f64,
    pub stages: Vec<StageInfo>,
    pub started_at: DateTime<Utc>,
    pub last_update: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    
    // Additional fields for API compatibility
    pub current_stage_name: String,
    pub stage_progress_percentage: f64,
    pub total_stages: usize,
    pub completed_stages: usize,
    pub stage_metadata: std::collections::HashMap<String, String>,
    pub error_message: Option<String>,
    
    /// API compatibility field - computed from state but stored for serialization
    pub is_complete: bool,
}

impl UniversalProgress {
    /// Check if the operation is complete (finished, failed, or cancelled)
    pub fn is_complete(&self) -> bool {
        matches!(self.state, UniversalState::Completed | UniversalState::Error | UniversalState::Cancelled)
    }

    /// Create a new UniversalProgress instance with unified structure
    pub fn new(
        id: Uuid,
        owner_id: Uuid,
        resource_type: String,
        operation_type: OperationType,
        operation_name: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            operation_id: id, // Use same ID for now
            operation_name,
            operation_type,
            owner_type: resource_type,
            owner_id,
            state: UniversalState::Preparing,
            current_stage: "initializing".to_string(),
            overall_percentage: 0.0,
            stages: Vec::new(),
            started_at: now,
            last_update: now,
            completed_at: None,
            
            // Initialize additional fields for API compatibility
            current_stage_name: "Initializing".to_string(),
            stage_progress_percentage: 0.0,
            total_stages: 0,
            completed_stages: 0,
            stage_metadata: std::collections::HashMap::new(),
            error_message: None,
            is_complete: false, // New operations start incomplete
        }
    }

    
    /// Update a specific stage's progress and recalculate overall percentage
    pub fn update_stage(&mut self, stage_id: impl AsRef<str>, percentage: f64, stage_step: impl AsRef<str>) -> &mut Self {
        let stage_id_str = stage_id.as_ref();
        let stage_step_str = stage_step.as_ref();
        
        if let Some(stage) = self.stages.iter_mut().find(|s| s.id == stage_id_str) {
            stage.percentage = percentage.clamp(0.0, 100.0);
            stage.stage_step = stage_step_str.to_string();
            stage.state = if percentage >= 100.0 { 
                UniversalState::Completed 
            } else { 
                UniversalState::Processing 
            };
        }
        
        // Note: current_stage is now managed explicitly by the orchestrator
        // Don't automatically change current_stage based on which stage reports progress
        
        // Recalculate overall percentage
        self.recalculate_overall_percentage();
        self.last_update = Utc::now();
        self
    }
    
    /// Mark a stage as completed
    pub fn complete_stage(&mut self, stage_id: &str) -> &mut Self {
        if let Some(stage) = self.stages.iter_mut().find(|s| s.id == stage_id) {
            stage.percentage = 100.0;
            stage.state = UniversalState::Completed;
            stage.stage_step = "Completed".to_string();
        }
        
        self.recalculate_overall_percentage();
        
        // Check if all stages are complete
        if self.stages.iter().all(|s| s.state == UniversalState::Completed) {
            self.state = UniversalState::Completed;
            self.completed_at = Some(Utc::now());
            self.overall_percentage = 100.0;
        }
        
        self.last_update = Utc::now();
        self
    }
    
    /// Update stage step message for backwards compatibility
    pub fn update_stage_step(&mut self, stage_step: String) -> &mut Self {
        if let Some(current_stage) = self.stages.first_mut() {
            current_stage.stage_step = stage_step;
        }
        self.last_update = Utc::now();
        self
    }
    
    /// Update items progress for backwards compatibility
    pub fn update_items(&mut self, processed: usize, total: Option<usize>, stage_step: String) -> &mut Self {
        if let Some(current_stage) = self.stages.first_mut() {
            if let Some(total_count) = total
                && total_count > 0 {
                    current_stage.percentage = (processed as f64 / total_count as f64 * 100.0).clamp(0.0, 100.0);
                }
            current_stage.stage_step = stage_step;
        }
        self.last_update = Utc::now();
        self
    }
    
    /// Update overall percentage for backwards compatibility
    pub fn update_percentage(&mut self, percentage: f64) -> &mut Self {
        self.overall_percentage = percentage.clamp(0.0, 100.0);
        self.last_update = Utc::now();
        self
    }
    
    /// Set error state for backwards compatibility
    pub fn set_error(&mut self, error_message: impl AsRef<str>) -> &mut Self {
        self.state = UniversalState::Error;
        self.error_message = Some(error_message.as_ref().to_string());
        self.completed_at = Some(Utc::now());
        self.is_complete = true;
        self.last_update = Utc::now();
        self
    }
    
    /// Set the overall operation state
    pub fn set_state(&mut self, state: UniversalState) -> &mut Self {
        if state == UniversalState::Completed {
            self.completed_at = Some(Utc::now());
            self.overall_percentage = 100.0;
        }
        self.is_complete = matches!(state, UniversalState::Completed | UniversalState::Error | UniversalState::Cancelled);
        self.state = state;
        self.last_update = Utc::now();
        self
    }
    
    /// Initialize stages for backwards compatibility
    pub fn init_stages(mut self, stage_names: Vec<String>) -> Self {
        self.total_stages = stage_names.len();
        self.stages = stage_names.into_iter().enumerate().map(|(index, name)| {
            StageInfo {
                id: name.clone(),
                name: name.clone(),
                state: if index == 0 { UniversalState::Processing } else { UniversalState::Idle },
                percentage: 0.0,
                stage_step: String::new(),
            }
        }).collect();
        
        // Set current stage info
        if let Some(first_stage) = self.stages.first() {
            self.current_stage = first_stage.id.clone();
            self.current_stage_name = first_stage.name.clone();
        }
        
        self
    }

    /// Recalculate overall percentage based on stage progress
    fn recalculate_overall_percentage(&mut self) {
        if self.stages.is_empty() {
            return;
        }
        
        let stage_weight = 100.0 / self.stages.len() as f64;
        let total_progress: f64 = self.stages.iter()
            .map(|stage| stage.percentage * stage_weight / 100.0)
            .sum();
            
        self.overall_percentage = total_progress.clamp(0.0, 100.0);
    }

}


/// Centralized progress manager for handling staged operations
pub struct ProgressManager {
    progress: Arc<RwLock<UniversalProgress>>,
    stages: Arc<RwLock<Vec<ProgressStage>>>,
    broadcast_tx: broadcast::Sender<UniversalProgress>,
    current_stage_index: Arc<RwLock<usize>>,
    progress_service_storage: Option<Arc<RwLock<HashMap<Uuid, UniversalProgress>>>>,
}

impl ProgressManager {
    /// Create a new progress manager
    pub fn new(
        id: Uuid,
        operation_name: String,
        owner_id: Option<Uuid>,
        broadcast_tx: broadcast::Sender<UniversalProgress>,
        progress_service_storage: Option<Arc<RwLock<HashMap<Uuid, UniversalProgress>>>>,
    ) -> Arc<Self> {
        Self::new_with_type(
            id,
            operation_name,
            owner_id,
            "unknown".to_string(),
            OperationType::Custom("generic".to_string()),
            broadcast_tx,
            progress_service_storage,
        )
    }
    
    /// Create a new progress manager with specific operation type
    pub fn new_with_type(
        id: Uuid,
        operation_name: String,
        owner_id: Option<Uuid>,
        owner_type: String,
        operation_type: OperationType,
        broadcast_tx: broadcast::Sender<UniversalProgress>,
        progress_service_storage: Option<Arc<RwLock<HashMap<Uuid, UniversalProgress>>>>,
    ) -> Arc<Self> {
        let now = Utc::now();
        let progress = UniversalProgress {
            id,
            operation_id: id,
            operation_name: operation_name.clone(),
            operation_type,
            owner_type,
            owner_id: owner_id.unwrap_or(id),
            state: UniversalState::Idle,
            current_stage: "initializing".to_string(),
            overall_percentage: 0.0,
            stages: Vec::new(),
            started_at: now,
            last_update: now,
            completed_at: None,
            current_stage_name: "Initializing".to_string(),
            stage_progress_percentage: 0.0,
            total_stages: 0,
            completed_stages: 0,
            stage_metadata: std::collections::HashMap::new(),
            is_complete: false,
            error_message: None,
        };

        Arc::new(Self {
            progress: Arc::new(RwLock::new(progress)),
            stages: Arc::new(RwLock::new(Vec::new())),
            broadcast_tx,
            current_stage_index: Arc::new(RwLock::new(0)),
            progress_service_storage,
        })
    }
    
    /// Add a new stage to the operation
    pub async fn add_stage(&self, stage_id: &str, stage_name: &str) -> Arc<ProgressManager> {
        let is_first_stage = {
            let mut stages = self.stages.write().await;
            
            // Check for ID collision
            if stages.iter().any(|s| s.id == stage_id) {
                panic!("Stage ID collision: '{stage_id}' already exists");
            }
            
            let is_first = stages.is_empty();
            
            stages.push(ProgressStage {
                id: stage_id.to_string(),
                name: stage_name.to_string(),
                progress_percentage: 0.0,
                is_completed: false,
                stage_description: None,
            });
            
            is_first
        }; // Drop the write lock before calling recalculate_and_broadcast
        
        // If this is the first stage, set it as current
        if is_first_stage {
            let mut progress = self.progress.write().await;
            progress.current_stage = stage_id.to_string();
            progress.current_stage_name = stage_name.to_string();
            drop(progress);
        }
        
        // Recalculate overall progress with new stage count
        self.recalculate_and_broadcast().await;
        
        // Return Arc to self for method chaining
        Arc::new(ProgressManager {
            progress: self.progress.clone(),
            stages: self.stages.clone(),
            broadcast_tx: self.broadcast_tx.clone(),
            current_stage_index: self.current_stage_index.clone(),
            progress_service_storage: self.progress_service_storage.clone(),
        })
    }
    
    /// Get a stage updater for a specific stage
    pub async fn get_stage_updater(&self, stage_id: &str) -> Option<ProgressStageUpdater> {
        let stages = self.stages.read().await;
        if stages.iter().any(|s| s.id == stage_id) {
            Some(ProgressStageUpdater {
                stage_id: stage_id.to_string(),
                manager: Arc::new(ProgressManager {
                    progress: self.progress.clone(),
                    stages: self.stages.clone(),
                    broadcast_tx: self.broadcast_tx.clone(),
                    current_stage_index: self.current_stage_index.clone(),
                    progress_service_storage: self.progress_service_storage.clone(),
                }),
            })
        } else {
            None
        }
    }
    
    /// Update progress for a specific stage
    pub async fn update_stage_progress(&self, stage_id: &str, stage_percentage: f64, stage_description: &str) {
        let mut stages = self.stages.write().await;
        if let Some(stage) = stages.iter_mut().find(|s| s.id == stage_id) {
            stage.progress_percentage = stage_percentage.clamp(0.0, 100.0);
            stage.stage_description = Some(stage_description.to_string());
        }
        drop(stages);
        
        self.recalculate_and_broadcast().await;
    }
    
    /// Mark a stage as completed
    pub async fn complete_stage(&self, stage_id: &str) {
        let mut stages = self.stages.write().await;
        let mut next_stage_id: Option<String> = None;
        
        // Mark stage as completed
        if let Some(stage) = stages.iter_mut().find(|s| s.id == stage_id) {
            stage.progress_percentage = 100.0;
            stage.is_completed = true;
            
            // Find the next incomplete stage after completing this one
            for stage in stages.iter() {
                if !stage.is_complete() {
                    next_stage_id = Some(stage.id.clone());
                    break;
                }
            }
        }
        
        drop(stages);
        
        // Automatically advance to next stage if there is one
        if let Some(next_id) = next_stage_id {
            let mut progress = self.progress.write().await;
            progress.current_stage = next_id.clone();
            
            // Update current_stage_name if we can find the stage
            let stages = self.stages.read().await;
            if let Some(stage) = stages.iter().find(|s| s.id == next_id) {
                progress.current_stage_name = stage.name.clone();
            }
            drop(stages);
            drop(progress);
        }
        
        self.recalculate_and_broadcast().await;
    }
    
    /// Set the current active stage
    pub async fn set_current_stage(&self, stage_id: &str) {
        let mut progress = self.progress.write().await;
        progress.current_stage = stage_id.to_string();
        
        // Update current_stage_name if we can find the stage
        let stages = self.stages.read().await;
        if let Some(stage) = stages.iter().find(|s| s.id == stage_id) {
            progress.current_stage_name = stage.name.clone();
        }
        drop(stages);
        
        progress.last_update = Utc::now();
        let progress_copy = progress.clone();
        drop(progress);
        
        // Store in service storage if available
        if let Some(storage) = &self.progress_service_storage {
            let mut storage = storage.write().await;
            storage.insert(progress_copy.id, progress_copy.clone());
        }
        
        let _ = self.broadcast_tx.send(progress_copy);
    }
    
    /// Complete the entire operation
    pub async fn complete_operation(&self) {
        let mut progress = self.progress.write().await;
        progress.overall_percentage = 100.0;
        progress.state = UniversalState::Completed;
        progress.completed_at = Some(Utc::now());
        progress.is_complete = true;
        progress.last_update = Utc::now();
        // Stage info is managed through stages vector
        
        let progress_copy = progress.clone();
        let _owner_id = progress.owner_id;
        drop(progress);
        
        // Store in service storage if available and remove from active operations
        if let Some(storage) = &self.progress_service_storage {
            let mut storage = storage.write().await;
            storage.insert(progress_copy.id, progress_copy.clone());
            
            // If this is a shared storage from ProgressService, also remove from active operations
            // We need to access the parent ProgressService to remove from active_operations
            // For now, we'll let the service handle cleanup via cleanup_completed()
        }
        
        let _ = self.broadcast_tx.send(progress_copy);
    }
    
    /// Get current progress state
    pub async fn get_progress(&self) -> UniversalProgress {
        self.progress.read().await.clone()
    }
    
    /// Get current progress state with forced recalculation and broadcast
    /// Only use this when you need to ensure SSE clients receive an update
    pub async fn get_progress_and_broadcast(&self) -> UniversalProgress {
        self.recalculate_and_broadcast().await;
        self.progress.read().await.clone()
    }
    
    /// Recalculate overall progress based on stage weights and broadcast update
    async fn recalculate_and_broadcast(&self) {
        let stages = self.stages.read().await;
        
        // Allow initial broadcast even with empty stages to ensure progress appears in SSE
        let stage_count = stages.len();
        
        // Calculate overall progress based on stage weights
        let mut total_progress = 0.0;
        let stage_weight = if stage_count > 0 { 100.0 / stage_count as f64 } else { 0.0 };
        let mut current_stage_info: Option<ProgressStageInfo> = None;
        
        for stage in stages.iter() {
            // Each completed stage contributes its full weight
            if stage.is_complete() {
                total_progress += stage_weight;
            } else {
                // In-progress stage contributes partial weight
                total_progress += (stage.progress_percentage / 100.0) * stage_weight;
                
                // Set current stage info to the first non-completed stage
                if current_stage_info.is_none() {
                    current_stage_info = Some(ProgressStageInfo {
                        id: stage.id.clone(),
                        name: stage.name.clone(),
                        percentage: stage.progress_percentage,
                        stage_description: stage.stage_description.clone(),
                    });
                }
            }
        }
        
        // Update progress state with stages data
        let mut progress = self.progress.write().await;
        progress.last_update = Utc::now();
        
        // Update overall state based on stage count
        if stage_count > 0 {
            // If we have stages, we should be in processing state
            if progress.state == UniversalState::Idle {
                progress.set_state(UniversalState::Processing);
            }
            
            // Note: current_stage is now managed explicitly by the orchestrator
            // Don't automatically change it based on stage completion status
        }
        // CRITICAL BUG FIX: Do not automatically mark operations as completed just because they have no stages.
        // Operations should only be completed when explicitly marked as complete via complete_operation().
        // This was causing progress to show as "completed" immediately upon creation.
        
        // Set percentage AFTER state to override set_state's automatic 100% setting
        progress.overall_percentage = total_progress.clamp(0.0, 100.0);
        
        // CRITICAL FIX: Populate stages field from ProgressManager stages
        let stages_guard = self.stages.read().await;
        progress.stages = stages_guard.iter().map(|stage| {
            let stage_state = if stage.is_complete() {
                UniversalState::Completed
            } else {
                // CRITICAL FIX: Map stage description to appropriate state for database operation detection
                let description = stage.stage_description.as_deref().unwrap_or("");
                if description.contains("database") || description.contains("Inserting") || 
                   description.contains("Saving") || description.contains("programs") {
                    UniversalState::Saving
                } else {
                    UniversalState::Processing
                }
            };
            
            StageInfo {
                id: stage.id.clone(),
                name: stage.name.clone(),
                percentage: stage.progress_percentage,
                state: stage_state,
                stage_step: stage.stage_description.clone().unwrap_or_default(),
            }
        }).collect();
        progress.total_stages = stages_guard.len();
        progress.completed_stages = stages_guard.iter().filter(|s| s.is_complete()).count();
        drop(stages_guard);
        
        let progress_copy = progress.clone();
        drop(progress);
        
        // Store in service storage if available
        if let Some(storage) = &self.progress_service_storage {
            let mut storage = storage.write().await;
            storage.insert(progress_copy.id, progress_copy.clone());
        }
        
        // Broadcast update
        let _ = self.broadcast_tx.send(progress_copy);
    }
    
    /// Complete the operation and broadcast completion event
    pub async fn complete(&self) {
        let mut progress = self.progress.write().await;
        progress.set_state(UniversalState::Completed);
        let progress_copy = progress.clone();
        drop(progress);
        
        // Store in service storage if available
        if let Some(storage) = &self.progress_service_storage {
            let mut storage = storage.write().await;
            storage.insert(progress_copy.id, progress_copy.clone());
        }
        
        // Broadcast completion event
        let _ = self.broadcast_tx.send(progress_copy);
    }
    
    /// Fail the operation and broadcast failure event
    pub async fn fail(&self, error_message: &str) {
        let mut progress = self.progress.write().await;
        progress.set_error(error_message);
        let progress_copy = progress.clone();
        drop(progress);
        
        // Store in service storage if available
        if let Some(storage) = &self.progress_service_storage {
            let mut storage = storage.write().await;
            storage.insert(progress_copy.id, progress_copy.clone());
        }
        
        // Broadcast failure event
        let _ = self.broadcast_tx.send(progress_copy);
    }
    
    /// Create a pipeline callback that bridges the old callback system to the new ProgressManager
    pub fn create_pipeline_callback(&self) -> Arc<UniversalProgressCallback> {
        let progress_manager = Arc::new(ProgressManager {
            progress: self.progress.clone(),
            stages: self.stages.clone(),
            broadcast_tx: self.broadcast_tx.clone(),
            current_stage_index: self.current_stage_index.clone(),
            progress_service_storage: self.progress_service_storage.clone(),
        });
        
        // Add a mutex to prevent race conditions in stage creation
        let stage_creation_mutex = Arc::new(tokio::sync::Mutex::new(()));
        
        Arc::new(Box::new(move |universal_progress: UniversalProgress| {
            let manager = progress_manager.clone();
            let mutex = stage_creation_mutex.clone();
            tokio::spawn(async move {
                // CRITICAL FIX: Lock to prevent concurrent stage creation
                let _lock = mutex.lock().await;
                // Update existing stages (no more auto-adding to prevent race conditions)
                if !universal_progress.current_stage.is_empty() && universal_progress.current_stage != "initializing" {
                    let stage_id = &universal_progress.current_stage;
                    
                    // Only update if stage exists (no auto-creation)
                    debug!("Callback bridge updating existing stage '{}'", stage_id);
                    if let Some(stage_updater) = manager.get_stage_updater(stage_id).await {
                        stage_updater.update_progress(
                            universal_progress.overall_percentage,
                            &universal_progress.current_stage,
                        ).await;
                        debug!("Successfully updated stage: {}", stage_id);
                    } else {
                        debug!("Stage '{}' does not exist in ProgressManager - skipping update", stage_id);
                    }
                }
            });
        }))
    }

    /// Check if cancellation has been requested for this operation
    pub async fn is_cancellation_requested(&self) -> bool {
        // Since ProgressManager doesn't have direct access to IngestionStateManager,
        // we'll check the progress state to see if it's been cancelled
        let progress = self.progress.read().await;
        matches!(progress.state, UniversalState::Cancelled)
    }
}

/// Main progress service for managing all operations
pub struct ProgressService {
    storage: Arc<RwLock<HashMap<Uuid, UniversalProgress>>>,
    broadcast_tx: broadcast::Sender<UniversalProgress>,
    _broadcast_rx: broadcast::Receiver<UniversalProgress>,
    active_operations: Arc<RwLock<HashSet<Uuid>>>,
    ingestion_state_manager: Arc<IngestionStateManager>,
}

impl ProgressService {
    pub fn new(ingestion_state_manager: Arc<IngestionStateManager>) -> Self {
        let (broadcast_tx, broadcast_rx) = broadcast::channel(1000);
        let active_operations = Arc::new(RwLock::new(HashSet::new()));
        
        // Start background cleanup task
        let cleanup_active_operations = active_operations.clone();
        let mut cleanup_rx: broadcast::Receiver<UniversalProgress> = broadcast_tx.subscribe();
        tokio::spawn(async move {
            while let Ok(progress) = cleanup_rx.recv().await {
                // Clean up active_operations when operations complete or fail
                if progress.is_complete() {
                    let mut active = cleanup_active_operations.write().await;
                    if active.remove(&progress.owner_id) {
                        debug!("Cleaned up active operation for owner {} (state: {:?})", progress.owner_id, progress.state);
                    } else {
                        debug!("Attempted to clean up already removed operation for owner {} (state: {:?})", progress.owner_id, progress.state);
                    }
                }
            }
        });
        
        Self {
            storage: Arc::new(RwLock::new(HashMap::new())),
            broadcast_tx,
            _broadcast_rx: broadcast_rx,
            active_operations,
            ingestion_state_manager,
        }
    }

    /// Create a new staged progress manager for operations with multiple stages
    pub async fn create_staged_progress_manager(
        &self,
        owner_id: Uuid,
        resource_type: String,
        operation_type: OperationType,
        operation_name: String,
    ) -> Result<Arc<ProgressManager>, anyhow::Error> {
        // Check if operation already in progress
        if self.is_operation_in_progress(owner_id).await {
            let error_msg = format!("Cannot create staged progress manager for owner {owner_id}: Operation already in progress");
            warn!("{}", error_msg);
            return Err(anyhow::anyhow!(error_msg));
        }
        let id = Uuid::new_v4();
        
        // Add to active operations using owner_id, not operation id
        {
            let mut active = self.active_operations.write().await;
            active.insert(owner_id);
        }
        
        let progress_manager = ProgressManager::new_with_type(
            id,
            operation_name,
            Some(owner_id),
            resource_type,
            operation_type,
            self.broadcast_tx.clone(),
            Some(self.storage.clone()),
        );
        
        // Store initial progress immediately so SSE can see it
        progress_manager.recalculate_and_broadcast().await;
        
        Ok(progress_manager)
    }

    /// Check if an operation is already in progress for a given owner
    pub async fn is_operation_in_progress(&self, owner_id: Uuid) -> bool {
        // First check active_operations set (for newly created operations)
        {
            let active = self.active_operations.read().await;
            if active.contains(&owner_id) {
                return true;
            }
        }
        
        // Also check storage for operations that might be persisted
        let storage = self.storage.read().await;
        let active_operations: Vec<_> = storage.values()
            .filter(|progress| progress.owner_id == owner_id && !progress.is_complete())
            .collect();
        
        if !active_operations.is_empty() {
            warn!("Found {} active operations for owner {}: {:?}", 
                active_operations.len(), 
                owner_id, 
                active_operations.iter().map(|p| (p.id, &p.operation_name, &p.state)).collect::<Vec<_>>()
            );
        }
        
        !active_operations.is_empty()
    }
    
    /// Remove an operation from the active set (internal method)
    pub async fn remove_from_active(&self, owner_id: Uuid) {
        let mut active = self.active_operations.write().await;
        active.remove(&owner_id);
    }

    /// Get all active progress states
    pub async fn get_all_progress(&self) -> Vec<UniversalProgress> {
        let storage = self.storage.read().await;
        storage.values().cloned().collect()
    }

    /// Get progress for a specific operation
    pub async fn get_progress(&self, id: Uuid) -> Option<UniversalProgress> {
        let storage = self.storage.read().await;
        storage.get(&id).cloned()
    }

    /// Subscribe to progress updates
    pub fn subscribe(&self) -> broadcast::Receiver<UniversalProgress> {
        self.broadcast_tx.subscribe()
    }

    /// Clean up completed operations
    pub async fn cleanup_completed(&self) {
        let mut storage = self.storage.write().await;
        let mut active = self.active_operations.write().await;
        
        let completed_ids: Vec<Uuid> = storage
            .iter()
            .filter(|(_, progress)| progress.is_complete())
            .map(|(id, _)| *id)
            .collect();
        
        for id in completed_ids {
            storage.remove(&id);
            active.remove(&id);
        }
    }
    
    /// Force cleanup of stuck operations for a specific owner
    pub async fn force_cleanup_owner_operations(&self, owner_id: Uuid) {
        let mut storage = self.storage.write().await;
        let mut active = self.active_operations.write().await;
        
        // Find all operations for this owner
        let owner_operation_ids: Vec<Uuid> = storage
            .iter()
            .filter(|(_, progress)| progress.owner_id == owner_id)
            .map(|(id, _)| *id)
            .collect();
        
        if !owner_operation_ids.is_empty() {
            warn!("Force cleaning up {} stuck operations for owner {}", owner_operation_ids.len(), owner_id);
            for id in owner_operation_ids {
                storage.remove(&id);
            }
        }
        
        // Also remove from active operations
        active.remove(&owner_id);
    }

    /// Get the ingestion state manager (for compatibility)
    pub fn get_ingestion_state_manager(&self) -> Arc<IngestionStateManager> {
        self.ingestion_state_manager.clone()
    }

    /// Start operation with specific ID (for compatibility)
    pub async fn start_operation_with_id(
        &self,
        operation_id: Uuid,
        owner_id: Uuid,
        _resource_type: String,
        _operation_type: OperationType,
        operation_name: String,
    ) -> Result<UniversalProgressCallback, Box<dyn std::error::Error>> {
        // Check if operation already in progress
        if self.is_operation_in_progress(owner_id).await {
            return Err("Operation already in progress for this owner".into());
        }

        let _now = Utc::now();
        let progress = UniversalProgress::new(
            operation_id,
            owner_id,
            _resource_type,
            _operation_type,
            operation_name,
        );

        // Store initial progress
        {
            let mut storage = self.storage.write().await;
            storage.insert(operation_id, progress.clone());
        }

        // Add to active operations
        {
            let mut active = self.active_operations.write().await;
            active.insert(operation_id);
        }

        // Create callback that updates this specific operation
        let storage_clone = self.storage.clone();
        let broadcast_tx = self.broadcast_tx.clone();
        
        Ok(Box::new(move |updated_progress: UniversalProgress| {
            let storage = storage_clone.clone();
            let tx = broadcast_tx.clone();
            
            tokio::spawn(async move {
                // Update storage
                {
                    let mut storage = storage.write().await;
                    storage.insert(updated_progress.id, updated_progress.clone());
                }
                
                // Broadcast update
                let _ = tx.send(updated_progress);
            });
        }))
    }

    
    
    /// Start operation (backwards compatibility)
    pub async fn start_operation(
        &self,
        owner_id: Uuid,
        owner_type: String,
        operation_type: OperationType,
        operation_name: String,
    ) -> Result<UniversalProgressCallback, Box<dyn std::error::Error>> {
        self.start_operation_with_id(Uuid::new_v4(), owner_id, owner_type, operation_type, operation_name).await
    }
    
    /// Complete operation (backwards compatibility)
    pub async fn complete_operation(&self, operation_id: Uuid) {
        let mut storage = self.storage.write().await;
        let mut owner_id_to_remove: Option<Uuid> = None;
        
        if let Some(progress) = storage.get_mut(&operation_id) {
            progress.set_state(UniversalState::Completed);
            owner_id_to_remove = Some(progress.owner_id);
        }
        drop(storage);
        
        // Remove from active operations set
        if let Some(owner_id) = owner_id_to_remove {
            let mut active = self.active_operations.write().await;
            active.remove(&owner_id);
        }
        
        self.broadcast_progress().await;
    }
    
    /// Create progress callback (backwards compatibility)
    pub async fn create_progress_callback(&self) -> UniversalProgressCallback {
        // Return a no-op callback for now
        Box::new(|_progress: UniversalProgress| {
            // No-op implementation
        })
    }
    
    /// Start operation with context (backwards compatibility)
    pub async fn start_operation_with_context(
        &self,
        owner_id: Uuid,
        owner_type: String,
        operation_type: OperationType,
        operation_name: String,
        _context: ProgressContext,
    ) -> Result<UniversalProgressCallback, Box<dyn std::error::Error>> {
        self.start_operation(owner_id, owner_type, operation_type, operation_name).await
    }
    
    /// Fail operation (backwards compatibility)
    pub async fn fail_operation(&self, operation_id: Uuid, error_message: &str) {
        let mut storage = self.storage.write().await;
        if let Some(progress) = storage.get_mut(&operation_id) {
            progress.set_error(error_message);
        }
        drop(storage);
        
        self.broadcast_progress().await;
    }
    
    /// Check if there are any active database operations in progress
    /// This is used by the scheduler to determine if it's safe to shut down
    pub async fn has_active_database_operations(&self) -> bool {
        // First clean up completed operations to avoid false positives
        self.cleanup_completed().await;
        
        let storage = self.storage.read().await;
        
        for progress in storage.values() {
            // Skip completed operations
            if progress.is_complete() {
                continue;
            }
            
            // Check if this is an ingestion operation with database activity
            match progress.operation_type {
                OperationType::StreamIngestion | OperationType::EpgIngestion => {
                    // Check if currently in a database-critical stage
                    for stage in &progress.stages {
                        if matches!(stage.state, UniversalState::Saving | UniversalState::Processing) 
                            && stage.percentage < 100.0 {
                            tracing::info!(
                                "Found active database operation during shutdown check: {} - stage '{}' at {}% ({}) - overall_complete: {}",
                                progress.operation_name,
                                stage.name,
                                stage.percentage,
                                stage.stage_step,
                                progress.is_complete()
                            );
                            return true;
                        }
                    }
                }
                _ => {
                    // Non-ingestion operations don't block shutdown
                    continue;
                }
            }
        }
        
        false
    }

    /// Broadcast progress update (backwards compatibility)
    async fn broadcast_progress(&self) {
        // Since we're using a different broadcast system, this can be a no-op for now
        // The new ProgressManager handles broadcasting
    }
    
    /// Clean up stale progress entries (for operations that didn't complete properly)
    pub async fn cleanup_stale_operations(&self, max_age_minutes: i64) {
        let cutoff_time = chrono::Utc::now() - chrono::Duration::minutes(max_age_minutes);
        let mut storage = self.storage.write().await;
        let mut to_remove = Vec::new();
        
        for (id, progress) in storage.iter() {
            if !progress.is_complete() && progress.last_update < cutoff_time {
                warn!("Cleaning up stale operation: {} ({}) - last update was {} minutes ago", 
                    id, progress.operation_name,
                    (chrono::Utc::now() - progress.last_update).num_minutes()
                );
                to_remove.push(*id);
            }
        }
        
        for id in to_remove {
            storage.remove(&id);
        }
        
        if !storage.is_empty() {
            info!("Cleaned up stale operations, {} active operations remaining", storage.len());
        }
    }
    
    /// Force clear all operations for a specific owner (emergency cleanup)
    pub async fn force_clear_owner_operations(&self, owner_id: Uuid) {
        let mut storage = self.storage.write().await;
        let mut to_remove = Vec::new();
        
        for (id, progress) in storage.iter() {
            if progress.owner_id == owner_id {
                warn!("Force clearing operation: {} ({}) for owner {}", 
                    id, progress.operation_name, owner_id
                );
                to_remove.push(*id);
            }
        }
        
        for id in to_remove {
            storage.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_single_stage_progress() {
        let (tx, _rx) = broadcast::channel(100);
        let progress_manager = ProgressManager::new(
            Uuid::new_v4(),
            "test_operation".to_string(),
            None,
            tx,
            None,
        );

        // Add a single stage
        let progress_manager = progress_manager.add_stage("stage1", "Test Stage").await;
        
        // Verify initial state
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.overall_percentage, 0.0);
        assert!(!progress.stages.is_empty());
        assert_eq!(progress.stages[0].id, "stage1");
        assert_eq!(progress.stages[0].percentage, 0.0);

        // Get stage updater and update to 50%
        let updater = progress_manager.get_stage_updater("stage1").await.unwrap();
        updater.update_progress(50.0, "Half way done").await;
        
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.overall_percentage, 50.0);
        assert_eq!(progress.stages[0].percentage, 50.0);
        assert_eq!(progress.stages[0].stage_step, "Half way done");

        // Complete the stage
        updater.complete_stage().await;
        
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.overall_percentage, 100.0);
        assert_eq!(progress.stages[0].percentage, 100.0);

        // Complete the entire operation
        progress_manager.complete_operation().await;
        
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.overall_percentage, 100.0);
        assert!(progress.is_complete());
        assert_eq!(progress.state, UniversalState::Completed);
    }

    #[tokio::test]
    async fn test_multi_stage_progress_calculation() {
        let (tx, _rx) = broadcast::channel(100);
        let progress_manager = ProgressManager::new(
            Uuid::new_v4(),
            "multi_stage_test".to_string(),
            None,
            tx,
            None,
        );

        // Add 4 stages (25% each)
        let progress_manager = progress_manager
            .add_stage("stage1", "Stage 1").await
            .add_stage("stage2", "Stage 2").await
            .add_stage("stage3", "Stage 3").await
            .add_stage("stage4", "Stage 4").await;

        // Verify initial state - all stages at 0%
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.overall_percentage, 0.0);

        // Stage 1 at 10% should give overall 2.5% (10% of 25%)
        let updater1 = progress_manager.get_stage_updater("stage1").await.unwrap();
        updater1.update_progress(10.0, "Stage 1 starting").await;
        
        let progress = progress_manager.get_progress().await;
        assert!((progress.overall_percentage - 2.5).abs() < 0.01);

        // Complete stage 1 (should give 25% overall)
        updater1.complete_stage().await;
        
        let progress = progress_manager.get_progress().await;
        assert!((progress.overall_percentage - 25.0).abs() < 0.01);

        // Stage 2 at 50% should give overall 37.5% (25% + 12.5%)
        let updater2 = progress_manager.get_stage_updater("stage2").await.unwrap();
        updater2.update_progress(50.0, "Stage 2 half done").await;
        
        let progress = progress_manager.get_progress().await;
        assert!((progress.overall_percentage - 37.5).abs() < 0.01);
        assert_eq!(progress.current_stage, "stage2");

        // Complete stages 2 and 3
        updater2.complete_stage().await;
        let updater3 = progress_manager.get_stage_updater("stage3").await.unwrap();
        updater3.complete_stage().await;
        
        let progress = progress_manager.get_progress().await;
        assert!((progress.overall_percentage - 75.0).abs() < 0.01);
        assert_eq!(progress.current_stage, "stage4");

        // Complete final stage
        let updater4 = progress_manager.get_stage_updater("stage4").await.unwrap();
        updater4.complete_stage().await;
        
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.overall_percentage, 100.0);
    }

    #[tokio::test]
    async fn test_update_items_helper() {
        let (tx, _rx) = broadcast::channel(100);
        let progress_manager = ProgressManager::new(
            Uuid::new_v4(),
            "items_test".to_string(),
            None,
            tx,
            None,
        );

        let progress_manager = progress_manager.add_stage("processing", "Processing Items").await;
        let updater = progress_manager.get_stage_updater("processing").await.unwrap();

        // Process 25 out of 100 items (should be 25%)
        updater.update_items(25, Some(100), "Processing data").await;
        
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.overall_percentage, 25.0);
        assert_eq!(progress.stages[0].percentage, 25.0);
        assert_eq!(progress.stages[0].stage_step, "Processing data");

        // Process 100 out of 100 items (should be 100%)
        updater.update_items(100, Some(100), "Processing complete").await;
        
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.overall_percentage, 100.0);
        assert_eq!(progress.stages[0].percentage, 100.0);
    }

    #[tokio::test]
    async fn test_progress_service_blocking() {
        let ingestion_manager = Arc::new(IngestionStateManager::new());
        let service = ProgressService::new(ingestion_manager);
        let owner_id = Uuid::new_v4();

        // No operations in progress initially
        assert!(!service.is_operation_in_progress(owner_id).await);

        // Start an operation
        let progress_manager = service.create_staged_progress_manager(owner_id, "test_resource".to_string(), OperationType::Pipeline, "test_op".to_string()).await.unwrap();
        let _progress_manager = progress_manager.add_stage("stage1", "Stage 1").await;

        // Should detect operation in progress
        assert!(service.is_operation_in_progress(owner_id).await);

        // Complete the operation
        progress_manager.complete_operation().await;

        // Operation should now be complete (but still in storage)
        let all_progress = service.get_all_progress().await;
        assert_eq!(all_progress.len(), 1);
        assert!(all_progress[0].is_complete());

        // Cleanup should remove completed operations
        service.cleanup_completed().await;
        let all_progress = service.get_all_progress().await;
        assert_eq!(all_progress.len(), 0);
    }

    #[tokio::test]
    async fn test_stage_progression_order() {
        let (tx, _rx) = broadcast::channel(100);
        let progress_manager = ProgressManager::new(
            Uuid::new_v4(),
            "stage_order_test".to_string(),
            None,
            tx,
            None,
        );

        // Add stages in order
        let progress_manager = progress_manager
            .add_stage("data_mapping", "Data Mapping").await
            .add_stage("filtering", "Filtering").await
            .add_stage("generation", "Generation").await;

        // Initially should show first stage
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.current_stage, "data_mapping");

        // Complete first stage, should move to second
        let updater1 = progress_manager.get_stage_updater("data_mapping").await.unwrap();
        updater1.complete_stage().await;
        
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.current_stage, "filtering");

        // Complete second stage, should move to third
        let updater2 = progress_manager.get_stage_updater("filtering").await.unwrap();
        updater2.complete_stage().await;
        
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.current_stage, "generation");

        // Complete final stage
        let updater3 = progress_manager.get_stage_updater("generation").await.unwrap();
        updater3.complete_stage().await;
        
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.overall_percentage, 100.0);
    }

    #[tokio::test]
    async fn test_concurrent_stage_updates() {
        let (tx, _rx) = broadcast::channel(100);
        let progress_manager = ProgressManager::new(
            Uuid::new_v4(),
            "concurrent_test".to_string(),
            None,
            tx,
            None,
        );

        let progress_manager = progress_manager
            .add_stage("stage1", "Stage 1").await
            .add_stage("stage2", "Stage 2").await;

        let updater1 = progress_manager.get_stage_updater("stage1").await.unwrap();
        let updater2 = progress_manager.get_stage_updater("stage2").await.unwrap();

        // Simulate concurrent updates
        let handle1 = {
            let updater = updater1.clone();
            tokio::spawn(async move {
                for i in 0..=10 {
                    updater.update_progress(i as f64 * 10.0, &format!("Stage 1 at {i}")).await;
                    sleep(Duration::from_millis(1)).await;
                }
                updater.complete_stage().await;
            })
        };

        let handle2 = {
            let updater = updater2.clone();
            tokio::spawn(async move {
                sleep(Duration::from_millis(5)).await; // Start slightly later
                for i in 0..=10 {
                    updater.update_progress(i as f64 * 10.0, &format!("Stage 2 at {i}")).await;
                    sleep(Duration::from_millis(1)).await;
                }
                updater.complete_stage().await;
            })
        };

        // Wait for both stages to complete
        let _ = tokio::join!(handle1, handle2);

        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.overall_percentage, 100.0);
    }

    #[tokio::test]
    async fn test_edge_cases() {
        let (tx, _rx) = broadcast::channel(100);
        let progress_manager = ProgressManager::new(
            Uuid::new_v4(),
            "edge_cases_test".to_string(),
            None,
            tx,
            None,
        );

        // Test with no stages - should be Idle until work begins
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.overall_percentage, 0.0);
        assert_eq!(progress.state, UniversalState::Idle);

        // Add stage and test clamping
        let progress_manager = progress_manager.add_stage("test_stage", "Test Stage").await;
        let updater = progress_manager.get_stage_updater("test_stage").await.unwrap();

        // Test negative percentage (should clamp to 0)
        updater.update_progress(-10.0, "Negative test").await;
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.stages[0].percentage, 0.0);

        // Test > 100% (should clamp to 100)
        updater.update_progress(150.0, "Over 100 test").await;
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.stages[0].percentage, 100.0);

        // Test invalid stage ID
        let invalid_updater = progress_manager.get_stage_updater("nonexistent").await;
        assert!(invalid_updater.is_none());

        // Test update_items with 0 total
        updater.update_items(5, Some(0), "Zero total test").await;
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.stages[0].percentage, 0.0);

        // Test update_items with None total
        updater.update_items(5, None, "None total test").await;
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.stages[0].percentage, 0.0);
    }

    #[tokio::test]
    async fn test_channel_program_progress_calculation() {
        let (tx, _rx) = broadcast::channel(100);
        let progress_manager = ProgressManager::new(
            Uuid::new_v4(),
            "channel_program_test".to_string(),
            None,
            tx,
            None,
        );

        let progress_manager = progress_manager.add_stage("processing", "Processing Data").await;
        let updater = progress_manager.get_stage_updater("processing").await.unwrap();

        // Stage starts at 0%
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.stages[0].percentage, 0.0);

        // Complete preprocessing (database fetch, setup) - sets to 5%
        updater.complete_preprocessing("Database fetch complete, starting processing").await;
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.stages[0].percentage, 5.0);

        // Test with 1000 channels, 500 programs (1500 total)
        // Process 150 channels, 75 programs (225 total = 15% of 1500)
        // Expected: 5% + (15% * 90%) = 5% + 13.5% = 18.5%
        updater.update_channel_program_progress(150, 75, 1000, 500, "Processing 15%").await;
        let progress = progress_manager.get_progress().await;
        assert!((progress.stages[0].percentage - 18.5).abs() < 0.01);

        // Process 500 channels, 250 programs (750 total = 50% of 1500)
        // Expected: 5% + (50% * 90%) = 5% + 45% = 50%
        updater.update_channel_program_progress(500, 250, 1000, 500, "Processing 50%").await;
        let progress = progress_manager.get_progress().await;
        assert!((progress.stages[0].percentage - 50.0).abs() < 0.01);

        // Process all items (1000 channels, 500 programs = 100% of 1500)
        // Expected: 5% + (100% * 90%) = 5% + 90% = 95%
        updater.update_channel_program_progress(1000, 500, 1000, 500, "Processing complete").await;
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.stages[0].percentage, 95.0);

        // Complete the stage (should go to 100%)
        updater.complete_stage().await;
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.stages[0].percentage, 100.0);
    }

    #[tokio::test]
    async fn test_preprocessing_workflow() {
        let (tx, _rx) = broadcast::channel(100);
        let progress_manager = ProgressManager::new(
            Uuid::new_v4(),
            "preprocessing_test".to_string(),
            None,
            tx,
            None,
        );

        let progress_manager = progress_manager.add_stage("data_mapping", "Data Mapping").await;
        let updater = progress_manager.get_stage_updater("data_mapping").await.unwrap();

        // Stage starts at 0%
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.stages[0].percentage, 0.0);

        // Complete preprocessing (database queries, setup)
        updater.complete_preprocessing("Loaded 1000 channels and 500 programs from database").await;
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.stages[0].percentage, 5.0);

        // Now do main processing work
        updater.update_channel_program_progress(500, 250, 1000, 500, "Mapping 50% complete").await;
        let progress = progress_manager.get_progress().await;
        assert!((progress.stages[0].percentage - 50.0).abs() < 0.01);

        // Complete the stage (100%)
        updater.complete_stage().await;
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.stages[0].percentage, 100.0);
    }

    #[tokio::test]
    async fn test_edge_cases_channel_program_progress() {
        let (tx, _rx) = broadcast::channel(100);
        let progress_manager = ProgressManager::new(
            Uuid::new_v4(),
            "edge_cases_test".to_string(),
            None,
            tx,
            None,
        );

        let progress_manager = progress_manager.add_stage("processing", "Processing Data").await;
        let updater = progress_manager.get_stage_updater("processing").await.unwrap();

        // Complete preprocessing first
        updater.complete_preprocessing("Setup complete").await;

        // Test with no channels or programs (0 total) - should stay at 5%
        updater.update_channel_program_progress(0, 0, 0, 0, "No data to process").await;
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.stages[0].percentage, 5.0);

        // Test with only channels (no programs)
        updater.update_channel_program_progress(50, 0, 100, 0, "Channels only").await;
        let progress = progress_manager.get_progress().await;
        assert!((progress.stages[0].percentage - 50.0).abs() < 0.01); // 5% + (50% * 90%) = 50%

        // Test with only programs (no channels)
        updater.update_channel_program_progress(0, 25, 0, 100, "Programs only").await;
        let progress = progress_manager.get_progress().await;
        assert!((progress.stages[0].percentage - 27.5).abs() < 0.01); // 5% + (25% * 90%) = 27.5%

        // Test boundary: exactly at 95% should not exceed
        updater.update_channel_program_progress(1000, 1000, 1000, 1000, "At boundary").await;
        let progress = progress_manager.get_progress().await;
        assert_eq!(progress.stages[0].percentage, 95.0);
    }
}