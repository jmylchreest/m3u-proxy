//! Proxy Regeneration Service
//!
//! This service manages automatic regeneration of stream proxies when their
//! associated sources (stream or EPG) are updated. It uses pure in-memory state
//! with Tokio timers for delayed execution and deduplication.

// Serde imports removed - no longer needed after cleaning up legacy structs
use sqlx::{SqlitePool, Row};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use crate::config::Config;
use crate::repositories::stream_proxy::StreamProxyRepository;
use crate::services::progress_service::{ProgressManager, ProgressService, OperationType};
use crate::ingestor::IngestionStateManager;

// Removed scheduler dependency - now uses pure in-memory Tokio timers


/// Regeneration request for the sequential queue
#[derive(Clone)]
pub struct RegenerationRequest {
    pub proxy_id: Uuid,
    pub is_manual: bool,
    pub requested_at: chrono::DateTime<chrono::Utc>,
    pub progress_manager: Option<Arc<ProgressManager>>,
}

/// Configuration for the regeneration service
#[derive(Debug, Clone)]
pub struct RegenerationConfig {
    /// Delay in seconds after source updates before triggering regeneration
    pub delay_seconds: u64,
    /// Maximum concurrent regenerations (kept for compatibility, but queue is now sequential)
    pub max_concurrent: usize,
    /// Coordination window in seconds to wait for related source completions
    pub coordination_window_seconds: u64,
}

impl Default for RegenerationConfig {
    fn default() -> Self {
        Self {
            delay_seconds: 15,
            max_concurrent: 2,
            coordination_window_seconds: 30,
        }
    }
}

/// Service for managing proxy regeneration with priority queue system
#[derive(Clone)]
pub struct ProxyRegenerationService {
    pool: SqlitePool,
    proxy_repository: StreamProxyRepository,
    config: RegenerationConfig,
    app_config: Config,
    /// Active delayed regeneration timers
    pending_regenerations: Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
    /// Currently running regeneration tasks
    active_regenerations: Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
    /// Track which proxies are queued to prevent duplicates
    queued_proxies: Arc<Mutex<HashSet<Uuid>>>,
    /// In-memory priority queue: manual requests get priority
    manual_queue_sender: mpsc::UnboundedSender<RegenerationRequest>,
    auto_queue_sender: mpsc::UnboundedSender<RegenerationRequest>,
    /// Progress service for creating progress managers
    progress_service: Arc<ProgressService>,
    /// Ingestion state manager to check for active/pending operations
    ingestion_state_manager: Arc<IngestionStateManager>,
    temp_file_manager: sandboxed_file_manager::SandboxedManager,
}

impl ProxyRegenerationService {
    pub fn new(
        pool: SqlitePool,
        app_config: Config,
        config: Option<RegenerationConfig>,
        temp_file_manager: sandboxed_file_manager::SandboxedManager,
        _system: std::sync::Arc<tokio::sync::RwLock<sysinfo::System>>,
        progress_service: Arc<ProgressService>,
        ingestion_state_manager: Arc<IngestionStateManager>,
    ) -> Self {
        // Create priority queues: manual gets processed before automatic
        let (manual_queue_sender, manual_queue_receiver) = mpsc::unbounded_channel::<RegenerationRequest>();
        let (auto_queue_sender, auto_queue_receiver) = mpsc::unbounded_channel::<RegenerationRequest>();
        
        let service = Self {
            pool: pool.clone(),
            proxy_repository: StreamProxyRepository::new(pool.clone()),
            config: config.unwrap_or_default(),
            app_config,
            pending_regenerations: Arc::new(Mutex::new(HashMap::new())),
            active_regenerations: Arc::new(Mutex::new(HashMap::new())),
            queued_proxies: Arc::new(Mutex::new(HashSet::new())),
            manual_queue_sender,
            auto_queue_sender,
            progress_service: progress_service.clone(),
            ingestion_state_manager: ingestion_state_manager.clone(),
            temp_file_manager: temp_file_manager.clone(),
        };
        
        // Start the priority queue processor in the background
        service.start_priority_queue_processor(
            manual_queue_receiver, 
            auto_queue_receiver, 
            pool, 
            temp_file_manager, 
            progress_service, 
            ingestion_state_manager
        );
        
        service
    }
    
    /// Start the priority queue processor: manual jobs before automatic jobs
    fn start_priority_queue_processor(
        &self,
        mut manual_queue_receiver: mpsc::UnboundedReceiver<RegenerationRequest>,
        mut auto_queue_receiver: mpsc::UnboundedReceiver<RegenerationRequest>,
        pool: SqlitePool,
        temp_file_manager: sandboxed_file_manager::SandboxedManager,
        _progress_service: Arc<ProgressService>,
        ingestion_state_manager: Arc<IngestionStateManager>,
    ) {
        let active_regenerations = self.active_regenerations.clone();
        let queued_proxies = self.queued_proxies.clone();
        let app_config = self.app_config.clone();
        let queue_fill_delay = self.config.delay_seconds;
        
        tokio::spawn(async move {
            info!("Starting priority proxy regeneration queue processor (manual->auto priority)");
            
            loop {
                // Step 1: Process all manual jobs first (higher priority)
                let mut processed_manual = false;
                while let Ok(request) = manual_queue_receiver.try_recv() {
                    Self::process_regeneration_request(
                        request,
                        &pool,
                        &temp_file_manager,
                        &ingestion_state_manager,
                        &active_regenerations,
                        &queued_proxies,
                        &app_config,
                    ).await;
                    processed_manual = true;
                }
                
                // Step 2: If no manual jobs, check for automatic jobs
                if !processed_manual {
                    match auto_queue_receiver.try_recv() {
                        Ok(request) => {
                            // Check if a manual request for the same proxy exists in the manual queue
                            let mut manual_has_proxy = false;
                            while let Ok(manual_request) = manual_queue_receiver.try_recv() {
                                if manual_request.proxy_id == request.proxy_id {
                                    manual_has_proxy = true;
                                    // Process the manual request immediately (higher priority)
                                    Self::process_regeneration_request(
                                        manual_request,
                                        &pool,
                                        &temp_file_manager,
                                        &ingestion_state_manager,
                                        &active_regenerations,
                                        &queued_proxies,
                                        &app_config,
                                    ).await;
                                    break;
                                } else {
                                    // Put other manual requests back in queue
                                    // Since this is try_recv, we can't put it back, so process it
                                    Self::process_regeneration_request(
                                        manual_request,
                                        &pool,
                                        &temp_file_manager,
                                        &ingestion_state_manager,
                                        &active_regenerations,
                                        &queued_proxies,
                                        &app_config,
                                    ).await;
                                }
                            }
                            
                            // If manual request exists for same proxy, skip auto request
                            if manual_has_proxy {
                                debug!("Skipping auto regeneration for proxy {} - manual request takes priority", request.proxy_id);
                                continue;
                            }
                            
                            // Got an auto request - wait for queue to fill before processing
                            info!("Auto regeneration request received for proxy {} - waiting {}s for queue to fill", 
                                  request.proxy_id, queue_fill_delay);
                            
                            tokio::time::sleep(Duration::from_secs(queue_fill_delay)).await;
                            
                            // Process this request
                            Self::process_regeneration_request(
                                request,
                                &pool,
                                &temp_file_manager,
                                &ingestion_state_manager,
                                &active_regenerations,
                                &queued_proxies,
                                &app_config,
                            ).await;
                            
                            // Process any other auto requests that arrived during the delay
                            while let Ok(additional_request) = auto_queue_receiver.try_recv() {
                                Self::process_regeneration_request(
                                    additional_request,
                                    &pool,
                                    &temp_file_manager,
                                    &ingestion_state_manager,
                                    &active_regenerations,
                                    &queued_proxies,
                                    &app_config,
                                ).await;
                            }
                        }
                        Err(mpsc::error::TryRecvError::Empty) => {
                            // No work available, wait a bit and check again
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            info!("Auto queue disconnected, checking for manual work...");
                            // Check if manual queue is also disconnected
                            if manual_queue_receiver.recv().await.is_none() {
                                break;
                            }
                        }
                    }
                }
            }
            
            info!("Priority proxy regeneration queue processor terminated");
        });
    }
    
    /// Process a single regeneration request with all safety checks
    async fn process_regeneration_request(
        request: RegenerationRequest,
        pool: &SqlitePool,
        temp_file_manager: &sandboxed_file_manager::SandboxedManager,
        ingestion_state_manager: &Arc<IngestionStateManager>,
        active_regenerations: &Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
        queued_proxies: &Arc<Mutex<HashSet<Uuid>>>,
        app_config: &Config,
    ) {
        let proxy_id = request.proxy_id;
        
        // Remove from queued set since we're about to process it
        {
            let mut queued = queued_proxies.lock().await;
            queued.remove(&proxy_id);
        }
        
        info!("Processing regeneration request for proxy {} (manual: {}, requested: {})", 
              proxy_id, request.is_manual, request.requested_at);
        
        // CRITICAL: Always check ingestion status before processing (ingestion has priority)
        let has_ingestion = match ingestion_state_manager.has_active_ingestions().await {
            Ok(active) => {
                active
            },
            Err(e) => {
                error!("Failed to check ingestion status: {}", e);
                return;
            }
        };
        
        if has_ingestion {
            let message = if request.is_manual {
                "Manual regeneration blocked: ingestion is in progress. This prevents resource conflicts and ensures data consistency."
            } else {
                "Automatic regeneration blocked: ingestion is in progress."
            };
            warn!("{} Skipping proxy {}", message, proxy_id);
            return;
        }
        
        // Execute the regeneration
        match Self::execute_single_proxy_regeneration(
            pool.clone(),
            temp_file_manager.clone(),
            proxy_id,
            request.progress_manager.clone(),
            active_regenerations.clone(),
            app_config.clone(),
        ).await {
            Ok(()) => {
                debug!("Successfully completed regeneration for proxy {}", proxy_id);
            }
            Err(e) => {
                error!("Failed to regenerate proxy {}: {}", proxy_id, e);
            }
        }
    }

    /// Queue a proxy for regeneration due to source update (with delay)
    pub async fn queue_proxy_regeneration(
        &self,
        proxy_id: Uuid,
        trigger_source_id: Uuid,
        trigger_source_type: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // CRITICAL FIX: Check if proxy is already actively regenerating
        {
            let active = self.active_regenerations.lock().await;
            if active.contains_key(&proxy_id) {
                let message = format!(
                    "Proxy {proxy_id} is already actively regenerating, ignoring new {trigger_source_type} trigger from source {trigger_source_id}"
                );
                debug!("{}", message);
                return Ok(()); // Silently ignore to avoid error spam
            }
        }
        
        let mut pending = self.pending_regenerations.lock().await;
        
        // Cancel existing timer for this proxy if any (deduplication)
        if let Some(existing_handle) = pending.remove(&proxy_id) {
            existing_handle.abort();
            debug!("Cancelled existing regeneration timer for proxy {}", proxy_id);
        }

        // Start progress tracking using ProgressManager
        let operation_name = Self::create_human_readable_operation_name(
            &self.pool, proxy_id, trigger_source_type, trigger_source_id
        ).await;
        let trigger_source_type_owned = trigger_source_type.to_string();
        
        // Initialize progress tracking using ProgressService
        let progress_manager = match self.progress_service.create_staged_progress_manager(
            proxy_id,
            "proxy".to_string(),
            OperationType::ProxyRegeneration,
            operation_name.clone(),
        ).await {
            Ok(mgr) => Some(mgr),
            Err(e) => {
                // If ProgressManager creation fails, it means there's already an active operation
                // We should not proceed with a duplicate regeneration
                warn!("Failed to create progress manager for proxy {}: {} - skipping duplicate regeneration", proxy_id, e);
                return Ok(());
            }
        };

        // Create delayed regeneration task
        let _pool = self.pool.clone();
        let _temp_file_manager = self.temp_file_manager.clone();
        let delay_seconds = self.config.delay_seconds;
        let _pending_clone = self.pending_regenerations.clone();
        let _active_clone = self.active_regenerations.clone();
        let service_clone = self.clone();
        
        let handle = tokio::spawn(async move {
            // Update progress: waiting for delay  
            if let Some(ref progress_mgr) = progress_manager {
                let stage_manager = progress_mgr.add_stage("delay", "Waiting").await;
                if let Some(updater) = stage_manager.get_stage_updater("delay").await {
                    updater.update_progress(10.0, &format!("Waiting {delay_seconds}s before regeneration")).await;
                }
            }
            info!("Proxy {} regeneration: waiting {}s before starting", proxy_id, delay_seconds);
            
            // Wait for the delay
            sleep(Duration::from_secs(delay_seconds)).await;
            
            info!("Delay completed for proxy {} - queueing for sequential processing (triggered by {} source {})", 
                  proxy_id, trigger_source_type_owned, trigger_source_id);
            
            // After delay, queue the regeneration request for sequential processing
            let request = RegenerationRequest {
                proxy_id,
                is_manual: false,
                requested_at: chrono::Utc::now(),
                progress_manager,
            };
            
            // Check if already queued to prevent duplicates
            {
                let mut queued = service_clone.queued_proxies.lock().await;
                if queued.contains(&proxy_id) {
                    debug!("Proxy {} already queued for regeneration, skipping duplicate", proxy_id);
                    return;
                }
                queued.insert(proxy_id);
            }
            
            if let Err(e) = service_clone.auto_queue_sender.send(request) {
                // Remove from queued set if send failed
                service_clone.queued_proxies.lock().await.remove(&proxy_id);
                error!("Failed to queue automatic regeneration for proxy {} after delay: {}", proxy_id, e);
            }
        });

        pending.insert(proxy_id, handle);

        info!(
            "Queued proxy {} for regeneration (trigger: {} {}, scheduled in {}s)",
            proxy_id, trigger_source_type, trigger_source_id, self.config.delay_seconds
        );

        Ok(())
    }

    /// Queue a manual proxy regeneration (sequential processing)
    pub async fn queue_manual_regeneration(
        &self,
        proxy_id: Uuid,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Check if proxy is already in the queue or actively regenerating
        {
            let active = self.active_regenerations.lock().await;
            if active.contains_key(&proxy_id) {
                let message = format!(
                    "Proxy {proxy_id} is already actively regenerating, rejecting duplicate manual regeneration request"
                );
                warn!("{}", message);
                return Err(message.into()); // Return error for manual requests so user gets feedback
            }
        }
        
        // Cancel any pending delayed regeneration since manual takes priority
        {
            let mut pending = self.pending_regenerations.lock().await;
            if let Some(existing_handle) = pending.remove(&proxy_id) {
                existing_handle.abort();
                debug!("Cancelled pending auto regeneration for manual trigger: {}", proxy_id);
            }
        }
        
        // Remove from auto queue if it exists there (manual takes priority)
        {
            let mut queued = self.queued_proxies.lock().await;
            if queued.contains(&proxy_id) {
                queued.remove(&proxy_id);
                debug!("Removed proxy {} from auto queue - manual regeneration takes priority", proxy_id);
            }
        }

        // Create progress manager for this manual regeneration
        let operation_name = Self::create_human_readable_manual_operation_name(&self.pool, proxy_id).await;
        let progress_manager = self.progress_service.create_staged_progress_manager(
            proxy_id,
            "proxy".to_string(),
            OperationType::ProxyRegeneration,
            operation_name,
        ).await?;

        // Check if already queued to prevent duplicates (after removing from auto queue)
        {
            let mut queued = self.queued_proxies.lock().await;
            if queued.contains(&proxy_id) {
                let message = format!("Proxy {proxy_id} is already queued for regeneration, rejecting duplicate manual request");
                warn!("{}", message);
                return Err(message.into());
            }
            queued.insert(proxy_id);
        }
        
        // Add request to the manual priority queue
        let request = RegenerationRequest {
            proxy_id,
            is_manual: true,
            requested_at: chrono::Utc::now(),
            progress_manager: Some(progress_manager),
        };

        if let Err(e) = self.manual_queue_sender.send(request) {
            // Remove from queued set if send failed
            self.queued_proxies.lock().await.remove(&proxy_id);
            let message = format!("Failed to queue manual regeneration for proxy {proxy_id}: {e}");
            error!("{}", message);
            return Err(message.into());
        }

        info!("Queued manual regeneration for proxy {} - priority processing", proxy_id);
        Ok(())
    }
    
    /// Execute a single proxy regeneration (used by the queue processor)
    async fn execute_single_proxy_regeneration(
        pool: SqlitePool,
        temp_file_manager: sandboxed_file_manager::SandboxedManager,
        proxy_id: Uuid,
        progress_manager: Option<Arc<ProgressManager>>,
        active_regenerations: Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
        app_config: Config,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create and track the regeneration task
        let handle = tokio::spawn(async move {
            debug!("Starting regeneration execution for proxy {}", proxy_id);
            
            // Create a new service instance for this regeneration
            // We can't use self here since this is a static method called from the queue processor
            match Self::regenerate_single_proxy_internal(
                pool,
                temp_file_manager,
                proxy_id,
                progress_manager,
                app_config,
            ).await {
                Ok(()) => {
                    debug!("Successfully completed regeneration for proxy {}", proxy_id);
                }
                Err(e) => {
                    error!("Failed to regenerate proxy {}: {}", proxy_id, e);
                }
            }
        });
        
        // Track as active
        {
            let mut active = active_regenerations.lock().await;
            active.insert(proxy_id, handle);
        }
        
        // Wait for completion and cleanup
        match active_regenerations.lock().await.remove(&proxy_id) {
            Some(handle) => {
                handle.await.map_err(|e| format!("Task join error: {e}"))?;
            }
            None => return Err("Failed to find regeneration task".into()),
        }
        
        Ok(())
    }
    
    /// Internal static method for regenerating a single proxy (used by queue processor)
    async fn regenerate_single_proxy_internal(
        pool: SqlitePool,
        temp_file_manager: sandboxed_file_manager::SandboxedManager,
        proxy_id: Uuid,
        progress_manager: Option<Arc<ProgressManager>>,
        app_config: Config,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::pipeline::PipelineOrchestratorFactory;
        
        let storage_config = app_config.storage.clone();

        // Create factory and orchestrator
        let factory = PipelineOrchestratorFactory::from_components(
            pool.clone(),
            app_config.clone(),
            storage_config,
            temp_file_manager,
        ).await?;

        info!("Regenerating proxy {} using pipeline factory (sequential queue)", proxy_id);
        
        // Use provided progress manager
        if let Some(ref _pm) = progress_manager {
            info!("Using provided ProgressManager for proxy {} regeneration", proxy_id);
        }
        
        // Create and execute the regeneration pipeline
        let mut orchestrator = factory.create_for_proxy(proxy_id).await?;
        orchestrator.set_progress_manager(progress_manager.clone());
        
        match orchestrator.execute_pipeline().await {
            Ok(result) => {
                match result.status {
                    crate::pipeline::models::PipelineStatus::Completed => {
                        info!("Successfully regenerated proxy {}", proxy_id);
                        factory.unregister_orchestrator(proxy_id).await;
                        
                        // CRITICAL FIX: Update the proxy's last_generated_at timestamp
                        let stream_proxy_repo = crate::repositories::StreamProxyRepository::new(pool.clone());
                        let update_time = chrono::Utc::now();
                        if let Err(e) = stream_proxy_repo.update_last_generated(proxy_id).await {
                            warn!("Failed to update last_generated_at for proxy {}: {}", proxy_id, e);
                        } else {
                            info!("Updated last_generated_at timestamp for proxy {} to {}", proxy_id, update_time.to_rfc3339());
                        }
                        
                        if let Some(pm) = &progress_manager {
                            pm.complete().await;
                        }
                        Ok(())
                    }
                    crate::pipeline::models::PipelineStatus::Failed => {
                        let error_msg = result.error_message.unwrap_or_else(|| "Unknown error".to_string());
                        error!("Pipeline failed for proxy {}: {}", proxy_id, error_msg);
                        factory.unregister_orchestrator(proxy_id).await;
                        if let Some(pm) = &progress_manager {
                            pm.fail(&error_msg).await;
                        }
                        Err(format!("Pipeline execution failed: {error_msg}").into())
                    }
                    _ => {
                        let warning_msg = format!("Pipeline completed with unexpected status: {:?}", result.status);
                        warn!("{} for proxy {}", warning_msg, proxy_id);
                        factory.unregister_orchestrator(proxy_id).await;
                        if let Some(pm) = &progress_manager {
                            pm.fail(&warning_msg).await;
                        }
                        Err(warning_msg.into())
                    }
                }
            }
            Err(e) => {
                error!("Failed to execute pipeline for proxy {}: {}", proxy_id, e);
                factory.unregister_orchestrator(proxy_id).await;
                if let Some(pm) = &progress_manager {
                    pm.fail(&format!("Pipeline execution failed: {e}")).await;
                }
                Err(Box::new(e))
            }
        }
    }


    /// Execute the actual proxy regeneration using the new pipeline
    #[allow(dead_code)]
    async fn execute_regeneration(
        &self,
        pool: SqlitePool, 
        temp_file_manager: sandboxed_file_manager::SandboxedManager,
        proxy_id: Option<Uuid>,
        progress_manager: Option<Arc<ProgressManager>>,
        is_manual_trigger: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // CRITICAL FIX: Check for active ingestions for ALL triggers to prevent resource conflicts
        if self.has_active_ingestions().await? {
            let message = if is_manual_trigger {
                "Manual regeneration blocked: ingestion is in progress. This prevents resource conflicts and ensures data consistency."
            } else {
                "Automatic regeneration blocked: ingestion is in progress."
            };
            warn!("{}", message);
            return Err(message.into());
        }

        if let Some(id) = proxy_id {
            info!("Starting regeneration for proxy {}", id);
            if let Some(ref progress_mgr) = progress_manager {
                let updater = progress_mgr.get_stage_updater("proxy_regeneration").await
                    .or(progress_mgr.get_stage_updater("manual_regeneration").await);
                if let Some(updater) = updater {
                    updater.update_progress(50.0, "Starting pipeline execution").await;
                }
            }
            
            self.regenerate_single_proxy(pool, temp_file_manager, id, progress_manager.clone()).await?;
        } else {
            return Err("Bulk regeneration has been removed - use individual proxy regeneration instead".into());
        }
        
        Ok(())
    }

    /// Regenerate a single proxy using the new pipeline
    #[allow(dead_code)]
    async fn regenerate_single_proxy(
        &self,
        pool: SqlitePool,
        temp_file_manager: sandboxed_file_manager::SandboxedManager,
        proxy_id: Uuid,
        existing_progress_manager: Option<Arc<ProgressManager>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::pipeline::PipelineOrchestratorFactory;

        // Use the injected app configuration
        let storage_config = self.app_config.storage.clone();

        // Create factory and orchestrator
        let factory = PipelineOrchestratorFactory::from_components(
            pool,
            self.app_config.clone(),
            storage_config,
            temp_file_manager,
        ).await?;

        info!("Regenerating proxy {} using new pipeline factory", proxy_id);
        
        // Use existing ProgressManager if provided, otherwise create a new one (for manual regeneration)
        let progress_manager = match existing_progress_manager {
            Some(mgr) => {
                info!("Using existing ProgressManager for proxy {} regeneration", proxy_id);
                mgr
            },
            None => {
                // Only create new ProgressManager for manual regeneration paths
                let operation_name = Self::create_human_readable_manual_operation_name(&self.pool, proxy_id).await;
                let result = self.progress_service.create_staged_progress_manager(
                    proxy_id,
                    "proxy".to_string(),
                    OperationType::ProxyRegeneration,
                    operation_name,
                ).await;
                
                match result {
                    Ok(mgr) => {
                        info!("Created new ProgressManager for manual proxy {} regeneration", proxy_id);
                        mgr
                    },
                    Err(e) => {
                        let error_msg = e.to_string();
                        error!("Failed to create ProgressManager for proxy {}: {} - aborting regeneration", proxy_id, error_msg);
                        if error_msg.contains("Operation already in progress") {
                            warn!("Progress operation already in progress for proxy {} - use cleanup API to resolve", proxy_id);
                        }
                        return Err(anyhow::anyhow!("Cannot start regeneration - ProgressManager creation failed: {}", error_msg).into());
                    }
                }
            }
        };

        // Create and execute pipeline
        let mut orchestrator = factory.create_for_proxy(proxy_id).await?;
        
        // Set the progress manager on the orchestrator
        orchestrator.set_progress_manager(Some(progress_manager.clone()));
        
        // Store factory reference for cleanup
        let factory_for_cleanup = factory.clone();
        
        let updater = progress_manager.get_stage_updater("proxy_regeneration").await
            .or(progress_manager.get_stage_updater("manual_regeneration").await);
        if let Some(updater) = updater {
            updater.update_progress(75.0, "Executing pipeline stages").await;
        }
        
        let execution_result = orchestrator.execute_pipeline().await?;

        match execution_result.status {
            crate::pipeline::models::PipelineStatus::Completed => {
                info!("Successfully completed regeneration for proxy {}", proxy_id);
                
                // CRITICAL FIX: Unregister orchestrator after successful completion
                factory_for_cleanup.unregister_orchestrator(proxy_id).await;
                
                // Update the proxy's last_generated_at timestamp
                let stream_proxy_repo = crate::repositories::StreamProxyRepository::new(self.pool.clone());
                let update_time = chrono::Utc::now();
                if let Err(e) = stream_proxy_repo.update_last_generated(proxy_id).await {
                    warn!("Failed to update last_generated_at for proxy {}: {}", proxy_id, e);
                } else {
                    info!("Updated last_generated_at timestamp for proxy {} to {}", proxy_id, update_time.to_rfc3339());
                }
                
                let updater = progress_manager.get_stage_updater("proxy_regeneration").await
                    .or(progress_manager.get_stage_updater("manual_regeneration").await);
                if let Some(updater) = updater {
                    updater.update_progress(100.0, "Pipeline execution completed successfully").await;
                    updater.complete_stage().await;
                }
                // Complete the overall operation
                progress_manager.complete().await;
            }
            crate::pipeline::models::PipelineStatus::Failed => {
                let error_msg = execution_result.error_message
                    .unwrap_or_else(|| "Unknown error".to_string());
                error!("Pipeline failed for proxy {}: {}", proxy_id, error_msg);
                
                // CRITICAL FIX: Unregister orchestrator after failure
                factory_for_cleanup.unregister_orchestrator(proxy_id).await;
                
                let updater = progress_manager.get_stage_updater("proxy_regeneration").await
                    .or(progress_manager.get_stage_updater("manual_regeneration").await);
                if let Some(updater) = updater {
                    updater.update_progress(0.0, &format!("Pipeline execution failed: {error_msg}")).await;
                }
                // Fail the overall operation
                progress_manager.fail(&error_msg).await;
                
                return Err(format!("Pipeline execution failed for proxy {proxy_id}").into());
            }
            _ => {
                warn!("Pipeline completed with status: {:?} for proxy {}", 
                    execution_result.status, proxy_id);
                
                // CRITICAL FIX: Unregister orchestrator for any completion status
                factory_for_cleanup.unregister_orchestrator(proxy_id).await;
                
                // Complete the operation with a warning status
                let warning_msg = format!("Pipeline completed with unexpected status: {:?}", execution_result.status);
                progress_manager.fail(&warning_msg).await;
            }
        }

        Ok(())
    }


    /// Find all proxies that use a specific source and have auto_regenerate enabled
    pub async fn find_affected_proxies(
        &self,
        source_id: Uuid,
        source_type: &str,
    ) -> Result<Vec<Uuid>, sqlx::Error> {
        match source_type {
            "stream" => {
                self.proxy_repository.find_proxies_by_stream_source(source_id)
                    .await
                    .map_err(|_e| sqlx::Error::RowNotFound)
            }
            "epg" => {
                self.proxy_repository.find_proxies_by_epg_source(source_id)
                    .await
                    .map_err(|_e| sqlx::Error::RowNotFound)
            }
            _ => Err(sqlx::Error::TypeNotFound { type_name: format!("Invalid source_type: {source_type}") }),
        }
    }

    /// Get all sources (stream and EPG) associated with a specific proxy
    async fn get_proxy_sources(&self, proxy_id: Uuid) -> Result<HashSet<(Uuid, String)>, sqlx::Error> {
        let mut sources = HashSet::new();
        
        // Get stream sources using repository
        match self.proxy_repository.get_stream_source_ids(proxy_id).await {
            Ok(stream_ids) => {
                for source_id in stream_ids {
                    sources.insert((source_id, "stream".to_string()));
                }
            }
            Err(_) => {
                return Err(sqlx::Error::RowNotFound);
            }
        }
        
        // Get EPG sources using repository
        match self.proxy_repository.get_epg_source_ids(proxy_id).await {
            Ok(epg_ids) => {
                for source_id in epg_ids {
                    sources.insert((source_id, "epg".to_string()));
                }
            }
            Err(_) => {
                return Err(sqlx::Error::RowNotFound);
            }
        }
        
        Ok(sources)
    }

    /// Queue regeneration for all affected proxies - simply schedules proxy regeneration jobs
    pub async fn queue_affected_proxies_coordinated(&self, source_id: Uuid, source_type: &str) {
        // COORDINATION FIX: Check if we have too many active regenerations already
        let active_count = self.get_active_regeneration_count().await;
        if active_count >= self.config.max_concurrent {
            debug!(
                "Scheduler skipping {} source {} proxy regenerations: {} active regenerations at max capacity ({})",
                source_type, source_id, active_count, self.config.max_concurrent
            );
            return;
        }

        match self.find_affected_proxies(source_id, source_type).await {
            Ok(proxy_ids) => {
                if proxy_ids.is_empty() {
                    debug!("No proxies affected by {} source {} update", source_type, source_id);
                    return;
                }

                info!(
                    "Found {} proxies affected by {} source {} update, scheduling regeneration jobs",
                    proxy_ids.len(), source_type, source_id
                );

                // Schedule regeneration for each affected proxy (with duplicate prevention)
                for proxy_id in proxy_ids {
                    self.schedule_proxy_regeneration(proxy_id, source_id, source_type).await;
                }
            }
            Err(e) => {
                error!("Failed to find affected proxies for {} source {}: {}", source_type, source_id, e);
            }
        }
    }

    /// Schedule a proxy regeneration (with duplicate prevention)
    async fn schedule_proxy_regeneration(&self, proxy_id: Uuid, trigger_source_id: Uuid, trigger_source_type: &str) {
        // Check if this proxy already has a pending or active regeneration
        {
            let pending = self.pending_regenerations.lock().await;
            if pending.contains_key(&proxy_id) {
                return;
            }
        }
        
        {
            let active = self.active_regenerations.lock().await;
            if active.contains_key(&proxy_id) {
                return;
            }
        }

        info!(
            "Scheduling regeneration for proxy {} (triggered by {} source {})",
            proxy_id, trigger_source_type, trigger_source_id
        );

        // Start the coordination process immediately (no delay for coordination)
        self.coordinate_proxy_regeneration(proxy_id, trigger_source_id, trigger_source_type).await;
    }

    /// Simple coordination: wait for all ingestions to complete, then trigger regeneration
    async fn coordinate_proxy_regeneration(&self, proxy_id: Uuid, completed_source_id: Uuid, completed_source_type: &str) {
        // Skip if already actively regenerating
        if self.has_active_regeneration(proxy_id).await {
            debug!("Proxy {} already actively regenerating, skipping coordination", proxy_id);
            return;
        }

        // Get all sources associated with this proxy
        let all_sources = match self.get_proxy_sources(proxy_id).await {
            Ok(sources) => sources,
            Err(e) => {
                error!("Failed to get sources for proxy {}: {}", proxy_id, e);
                return;
            }
        };

        debug!(
            "Proxy {} has {} total sources: {:?}", 
            proxy_id, all_sources.len(), all_sources
        );

        // Check for any active or soon-to-be-executed ingestions
        if let Some(blocking_sources) = self.check_for_blocking_ingestions(&all_sources).await {
            debug!(
                "Proxy {} coordination: {} ingestions are active or pending: {:?} - will retry in 30s", 
                proxy_id, blocking_sources.len(), blocking_sources
            );
            
            // Schedule a retry using the existing delay mechanism
            if let Err(e) = self.queue_proxy_regeneration_with_delay(proxy_id, completed_source_id, completed_source_type, 30).await {
                error!("Failed to schedule delayed regeneration for proxy {}: {}", proxy_id, e);
            }
            return;
        }

        // All ingestions are idle - trigger regeneration immediately
        info!(
            "Proxy {} coordination: all {} sources are idle, triggering regeneration",
            proxy_id, all_sources.len()
        );

        if let Err(e) = self.queue_proxy_regeneration(proxy_id, completed_source_id, completed_source_type).await {
            error!("Failed to queue regeneration for proxy {}: {}", proxy_id, e);
        }
    }

    /// Queue proxy regeneration with custom delay (for coordination retries)
    async fn queue_proxy_regeneration_with_delay(
        &self,
        proxy_id: Uuid,
        trigger_source_id: Uuid,
        trigger_source_type: &str,
        delay_seconds: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Similar to queue_proxy_regeneration but with custom delay and coordination retry
        
        // Cancel existing timer for this proxy if any (deduplication)
        {
            let mut pending = self.pending_regenerations.lock().await;
            if let Some(existing_handle) = pending.remove(&proxy_id) {
                existing_handle.abort();
                debug!("Cancelled existing regeneration timer for proxy {} (coordination retry)", proxy_id);
            }
        }

        // Create delayed regeneration task that retries coordination
        let service_clone = self.clone();
        let trigger_source_type_owned = trigger_source_type.to_string();
        
        let handle = tokio::spawn(async move {
            sleep(Duration::from_secs(delay_seconds)).await;
            
            info!("Retrying coordination for proxy {} after {}s delay", proxy_id, delay_seconds);
            
            // Check one more time if we should proceed, then trigger regeneration directly
            let all_sources = match service_clone.get_proxy_sources(proxy_id).await {
                Ok(sources) => sources,
                Err(e) => {
                    error!("Failed to get sources for delayed proxy regeneration {}: {}", proxy_id, e);
                    return;
                }
            };

            // Final check for blocking ingestions before proceeding
            if let Some(blocking_sources) = service_clone.check_for_blocking_ingestions(&all_sources).await {
                debug!(
                    "Delayed coordination for proxy {}: {} ingestions still active: {:?} - giving up for now", 
                    proxy_id, blocking_sources.len(), blocking_sources
                );
                
                // Don't schedule another retry to avoid the Send trait issue
                // The next ingestion completion will trigger coordination again
                return;
            }

            // All clear - trigger regeneration immediately
            info!(
                "Delayed coordination for proxy {}: all sources are now idle, triggering regeneration",
                proxy_id
            );

            if let Err(e) = service_clone.queue_proxy_regeneration(proxy_id, trigger_source_id, &trigger_source_type_owned).await {
                error!("Failed to queue regeneration for proxy {} after delay: {}", proxy_id, e);
            }
        });

        // Track as pending
        {
            let mut pending = self.pending_regenerations.lock().await;
            pending.insert(proxy_id, handle);
        }

        debug!(
            "Scheduled coordination retry for proxy {} in {}s (triggered by {} source {})",
            proxy_id, delay_seconds, trigger_source_type, trigger_source_id
        );

        Ok(())
    }

    /// Check if any sources have active or soon-to-be-executed ingestions
    async fn check_for_blocking_ingestions(&self, sources: &HashSet<(Uuid, String)>) -> Option<Vec<(Uuid, String)>> {
        let mut blocking_sources = Vec::new();
        let now = chrono::Utc::now();
        
        for (source_id, source_type) in sources {
            // Check if source is actively being processed
            if let Some(processing_info) = self.ingestion_state_manager.get_processing_info(*source_id).await {
                // Active if no next_retry_after (currently processing) OR retry is soon (within 30s)
                let is_active = processing_info.next_retry_after.is_none();
                let is_soon = processing_info.next_retry_after
                    .map(|retry_time| retry_time <= now + chrono::Duration::seconds(30))
                    .unwrap_or(false);
                
                if is_active || is_soon {
                    blocking_sources.push((*source_id, source_type.clone()));
                    if is_active {
                        debug!("Source {} ({}) is actively processing", source_id, source_type);
                    } else {
                        debug!("Source {} ({}) will retry within 30s", source_id, source_type);
                    }
                }
            }
        }
        
        if blocking_sources.is_empty() {
            None
        } else {
            Some(blocking_sources)
        }
    }

    /// Queue regeneration for all affected proxies after source update (legacy method)
    pub async fn queue_affected_proxies(&self, source_id: Uuid, source_type: &str) {
        match self.find_affected_proxies(source_id, source_type).await {
            Ok(proxy_ids) => {
                if proxy_ids.is_empty() {
                    debug!("No proxies affected by {} source {} update", source_type, source_id);
                    return;
                }

                info!(
                    "Found {} proxies affected by {} source {} update, queueing for regeneration",
                    proxy_ids.len(), source_type, source_id
                );

                for proxy_id in proxy_ids {
                    if let Err(e) = self.queue_proxy_regeneration(proxy_id, source_id, source_type).await {
                        error!("Failed to queue proxy {} for regeneration: {}", proxy_id, e);
                    }
                }
            }
            Err(e) => {
                error!("Failed to find affected proxies for {} source {}: {}", source_type, source_id, e);
            }
        }
    }

    /// Monitor for completed source ingestions (EPG and stream) and queue affected proxies
    /// Only triggers regeneration for proxies where source data is newer than proxy data
    pub async fn monitor_source_completions(&self) {
        let pool = &self.pool;
        let monitor_time = chrono::Utc::now();
        debug!("Starting monitor_source_completions at {}", monitor_time.to_rfc3339());
        
        // Find sources that have been updated more recently than any proxy that uses them
        match sqlx::query(
            r#"
            WITH outdated_proxies AS (
                -- Find stream sources newer than their associated proxies
                SELECT DISTINCT
                    ss.id as source_id,
                    'stream' as source_type,
                    ss.name as source_name,
                    ss.last_ingested_at,
                    p.id as proxy_id,
                    p.name as proxy_name,
                    p.updated_at as proxy_updated_at
                FROM stream_sources ss
                JOIN proxy_sources ps ON ps.source_id = ss.id
                JOIN stream_proxies p ON p.id = ps.proxy_id
                WHERE ss.is_active = 1 
                AND p.is_active = 1
                AND ss.last_ingested_at IS NOT NULL
                AND (p.last_generated_at IS NULL OR datetime(ss.last_ingested_at) > datetime(p.last_generated_at))
                
                UNION ALL
                
                -- Find EPG sources newer than their associated proxies
                SELECT DISTINCT
                    es.id as source_id,
                    'epg' as source_type,
                    es.name as source_name,
                    es.last_ingested_at,
                    p.id as proxy_id,
                    p.name as proxy_name,
                    p.updated_at as proxy_updated_at
                FROM epg_sources es
                JOIN proxy_epg_sources pes ON pes.epg_source_id = es.id
                JOIN stream_proxies p ON p.id = pes.proxy_id
                WHERE es.is_active = 1 
                AND p.is_active = 1
                AND es.last_ingested_at IS NOT NULL
                AND (p.last_generated_at IS NULL OR datetime(es.last_ingested_at) > datetime(p.last_generated_at))
            )
            SELECT 
                source_id,
                source_type, 
                source_name,
                COUNT(DISTINCT proxy_id) as affected_proxy_count,
                MAX(last_ingested_at) as latest_ingestion
            FROM outdated_proxies
            GROUP BY source_id, source_type, source_name
            ORDER BY latest_ingestion DESC
            "#
        )
        .fetch_all(pool)
        .await
        {
            Ok(rows) => {
                for row in rows {
                    let source_id_str: String = row.get("source_id");
                    let source_type: String = row.get("source_type");
                    
                    if let Ok(source_id) = source_id_str.parse::<Uuid>() {
                        // COORDINATION FIX: Use coordinated method for background monitoring
                        self.queue_affected_proxies_coordinated(source_id, &source_type).await;
                    }
                }
            }
            Err(e) => {
                error!("Failed to monitor source completions: {}", e);
            }
        }
    }


    /// Get queue status summary for API compatibility
    pub async fn get_queue_status(&self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let pending_count = self.pending_regenerations.lock().await.len();
        let active_count = self.active_regenerations.lock().await.len();
        let queued_count = self.queued_proxies.lock().await.len();
        
        // Check ingestion status for additional context
        let has_ingestion = self.ingestion_state_manager.has_active_ingestions().await.unwrap_or(false);
        
        Ok(serde_json::json!({
            "pending_delays": pending_count,
            "active_regenerations": active_count,
            "queued_for_processing": queued_count,
            "total_tracked": pending_count + active_count + queued_count,
            "ingestion_blocking": has_ingestion,
            "status": "running"
        }))
    }

    /// Start the background processor for monitoring source completions
    pub fn start_processor(&self) {
        let service = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60)); // Check every minute
            loop {
                interval.tick().await;
                service.monitor_source_completions().await;
            }
        });
        info!("Started proxy regeneration background processor");
    }

    /// Cancel all pending regenerations (useful for shutdown)
    pub async fn cancel_all_pending(&self) {
        let mut pending = self.pending_regenerations.lock().await;
        let mut active = self.active_regenerations.lock().await;
        
        for (proxy_id, handle) in pending.drain() {
            handle.abort();
            debug!("Cancelled pending regeneration for proxy {}", proxy_id);
        }
        
        for (proxy_id, handle) in active.drain() {
            handle.abort();
            debug!("Cancelled active regeneration for proxy {}", proxy_id);
        }
        
        // Progress entries are managed by ProgressService and will be cleaned up automatically
        
        info!("Cancelled all pending and active regenerations");
    }

    /// Check if there are any active regenerations for a specific proxy
    pub async fn has_active_regeneration(&self, proxy_id: Uuid) -> bool {
        // Check service-level active regenerations
        let active = self.active_regenerations.lock().await;
        active.contains_key(&proxy_id)
    }

    /// Check if there are any active regenerations (any proxy)
    pub async fn has_any_active_regenerations(&self) -> bool {
        let active = self.active_regenerations.lock().await;
        !active.is_empty()
    }

    /// Get count of active regenerations
    pub async fn get_active_regeneration_count(&self) -> usize {
        let active = self.active_regenerations.lock().await;
        active.len()
    }

    /// Check if there are any active ingestions in progress
    #[allow(dead_code)]
    async fn has_active_ingestions(&self) -> Result<bool, Box<dyn std::error::Error>> {
        // Use the injected ingestion state manager to check for active ingestions
        self.ingestion_state_manager.has_active_ingestions().await
            .map_err(|e| format!("Failed to check ingestion status: {e}").into())
    }

    /// Create human-readable operation name for progress tracking
    async fn create_human_readable_operation_name(
        pool: &SqlitePool,
        proxy_id: Uuid,
        trigger_source_type: &str,
        trigger_source_id: Uuid,
    ) -> String {
        use crate::repositories::{StreamProxyRepository, StreamSourceRepository, EpgSourceRepository, traits::Repository};

        // Get proxy name using repository
        let proxy_repo = StreamProxyRepository::new(pool.clone());
        let proxy_name = match proxy_repo.find_by_id(proxy_id).await {
            Ok(Some(proxy)) => format!("'{}'", proxy.name),
            _ => proxy_id.to_string(),
        };

        // Get source name based on type using repositories
        let source_name = match trigger_source_type {
            "stream" => {
                let stream_repo = StreamSourceRepository::new(pool.clone());
                match stream_repo.find_by_id(trigger_source_id).await {
                    Ok(Some(source)) => format!("'{}'", source.name),
                    _ => trigger_source_id.to_string(),
                }
            }
            "epg" => {
                let epg_repo = EpgSourceRepository::new(pool.clone());
                match epg_repo.find_by_id(trigger_source_id).await {
                    Ok(Some(source)) => format!("'{}'", source.name),
                    _ => trigger_source_id.to_string(),
                }
            }
            _ => trigger_source_id.to_string(),
        };

        let source_type_display = match trigger_source_type {
            "stream" => "Stream Source",
            "epg" => "EPG Source", 
            _ => "Source",
        };

        format!(
            "Regenerating Proxy {proxy_name} (triggered by {source_type_display}: {source_name})"
        )
    }

    /// Create human-readable operation name for manual regenerations
    async fn create_human_readable_manual_operation_name(
        pool: &SqlitePool,
        proxy_id: Uuid,
    ) -> String {
        use crate::repositories::{StreamProxyRepository, traits::Repository};

        // Get proxy name using repository
        let proxy_repo = StreamProxyRepository::new(pool.clone());
        let proxy_name = match proxy_repo.find_by_id(proxy_id).await {
            Ok(Some(proxy)) => format!("'{}'", proxy.name),
            _ => proxy_id.to_string(),
        };

        format!("Manual Regeneration: Proxy {proxy_name}")
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use sysinfo::SystemExt;

    #[tokio::test]
    async fn test_deduplication() {
        // Create mock services for testing
        let ingestion_state_manager = Arc::new(IngestionStateManager::new());
        let progress_service = Arc::new(ProgressService::new(ingestion_state_manager.clone()));
        
        let service = ProxyRegenerationService::new(
            sqlx::SqlitePool::connect(":memory:").await.unwrap(),
            Config::default(),
            Some(RegenerationConfig { delay_seconds: 1, max_concurrent: 1, coordination_window_seconds: 5 }),
            sandboxed_file_manager::SandboxedManager::builder()
                .base_directory(std::env::temp_dir().join("m3u_proxy_test"))
                .build().await.unwrap(),
            Arc::new(tokio::sync::RwLock::new(sysinfo::System::new())),
            progress_service,
            ingestion_state_manager,
        );

        let proxy_id = Uuid::new_v4();
        let source_id = Uuid::new_v4();

        // Queue the same regeneration twice
        service.queue_proxy_regeneration(proxy_id, source_id, "stream").await.unwrap();
        service.queue_proxy_regeneration(proxy_id, source_id, "stream").await.unwrap();

        // Should only have one pending regeneration
        let pending_count = service.pending_regenerations.lock().await.len();
        assert_eq!(pending_count, 1);
    }

    #[tokio::test] 
    async fn test_manual_regeneration() {
        // Create mock services for testing
        let ingestion_state_manager = Arc::new(IngestionStateManager::new());
        let progress_service = Arc::new(ProgressService::new(ingestion_state_manager.clone()));
        
        let service = ProxyRegenerationService::new(
            sqlx::SqlitePool::connect(":memory:").await.unwrap(),
            Config::default(),
            None,
            sandboxed_file_manager::SandboxedManager::builder()
                .base_directory(std::env::temp_dir().join("m3u_proxy_test2"))
                .build().await.unwrap(),
            Arc::new(tokio::sync::RwLock::new(sysinfo::System::new())),
            progress_service,
            ingestion_state_manager,
        );

        let proxy_id = Uuid::new_v4();
        
        let result = service.queue_manual_regeneration(proxy_id).await;
        assert!(result.is_ok());
        
        // Should have one queued regeneration (not active, since no worker is running)
        let queued_count = service.queued_proxies.lock().await.len();
        assert_eq!(queued_count, 1);
    }
}