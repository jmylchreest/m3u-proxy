//! Proxy Regeneration Service
//!
//! This service manages automatic regeneration of stream proxies when their
//! associated sources (stream or EPG) are updated. It uses pure in-memory state
//! with Tokio timers for delayed execution and deduplication.

use chrono::Utc;
// Serde imports removed - no longer needed after cleaning up legacy structs
use sqlx::{SqlitePool, Row};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use crate::config::Config;
use crate::utils::uuid_parser::parse_uuid_flexible;
use crate::services::progress_service::{ProgressManager, ProgressService, OperationType};
use crate::ingestor::IngestionStateManager;

// Removed scheduler dependency - now uses pure in-memory Tokio timers


/// Configuration for the regeneration service
#[derive(Debug, Clone)]
pub struct RegenerationConfig {
    /// Delay in seconds after source updates before triggering regeneration
    pub delay_seconds: u64,
    /// Maximum concurrent regenerations
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

/// Service for managing proxy regeneration with ProgressManager tracking
#[derive(Clone)]
pub struct ProxyRegenerationService {
    pool: SqlitePool,
    config: RegenerationConfig,
    app_config: Config,
    /// Active delayed regeneration timers
    pending_regenerations: Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
    /// Currently running regeneration tasks
    active_regenerations: Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
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
        Self {
            pool,
            config: config.unwrap_or_default(),
            app_config,
            pending_regenerations: Arc::new(Mutex::new(HashMap::new())),
            active_regenerations: Arc::new(Mutex::new(HashMap::new())),
            progress_service,
            ingestion_state_manager,
            temp_file_manager,
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
                    "Proxy {} is already actively regenerating, ignoring new {} trigger from source {}",
                    proxy_id, trigger_source_type, trigger_source_id
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

        let _scheduled_at = Utc::now() + chrono::Duration::seconds(self.config.delay_seconds as i64);
        
        // Start progress tracking using ProgressManager
        let operation_name = format!(
            "Regenerate Proxy {} (triggered by {} source {})",
            proxy_id, trigger_source_type, trigger_source_id
        );
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
        let pool = self.pool.clone();
        let temp_file_manager = self.temp_file_manager.clone();
        let delay_seconds = self.config.delay_seconds;
        let pending_clone = self.pending_regenerations.clone();
        let active_clone = self.active_regenerations.clone();
        let service_clone = self.clone();
        
        let handle = tokio::spawn(async move {
            // Update progress: waiting for delay  
            if let Some(ref progress_mgr) = progress_manager {
                let stage_manager = progress_mgr.add_stage("delay", "Waiting").await;
                if let Some(updater) = stage_manager.get_stage_updater("delay").await {
                    updater.update_progress(10.0, &format!("Waiting {}s before regeneration", delay_seconds)).await;
                }
            }
            info!("Proxy {} regeneration: waiting {}s before starting", proxy_id, delay_seconds);
            
            // Wait for the delay
            sleep(Duration::from_secs(delay_seconds)).await;
            
            // ATOMIC TRANSITION: Remove from pending and add to active in single operation
            let regen_handle = {
                let service = service_clone.clone();
                let active_cleanup = active_clone.clone(); // Clone for inner task cleanup
                tokio::spawn(async move {
                    // Add proxy_regeneration stage to the progress manager
                    if let Some(ref progress_mgr) = progress_manager {
                        progress_mgr.add_stage("proxy_regeneration", "Proxy Regeneration").await;
                    }
                    
                    // Update progress: starting regeneration
                    if let Some(ref progress_mgr) = progress_manager {
                        if let Some(updater) = progress_mgr.get_stage_updater("proxy_regeneration").await {
                            updater.update_progress(25.0, "Starting proxy regeneration").await;
                        }
                    }
                    info!("Starting proxy {} regeneration (triggered by {} source {})", proxy_id, trigger_source_type_owned, trigger_source_id);
                    
                    let error_msg = {
                        let result = service.execute_regeneration(
                            pool, 
                            temp_file_manager, 
                            Some(proxy_id), 
                            progress_manager.clone(),
                            false // Automatic trigger - check for active ingestions
                        ).await;
                        
                        // Extract error message if any
                        match result {
                            Ok(_) => None,
                            Err(e) => Some(e.to_string()),
                        }
                    }; // result is dropped here
                    
                    // Complete or fail the operation
                    if let Some(error_msg) = error_msg {
                        if let Some(ref progress_mgr) = progress_manager {
                            if let Some(updater) = progress_mgr.get_stage_updater("proxy_regeneration").await {
                                updater.update_progress(0.0, &format!("Failed: {}", error_msg)).await;
                            }
                            // Fail the overall operation
                            progress_mgr.fail(&error_msg).await;
                        }
                        error!("Proxy {} regeneration failed: {}", proxy_id, error_msg);
                    } else {
                        if let Some(ref progress_mgr) = progress_manager {
                            if let Some(updater) = progress_mgr.get_stage_updater("proxy_regeneration").await {
                                updater.update_progress(100.0, "Regeneration completed successfully").await;
                                updater.complete_stage().await;
                            }
                            // Complete the overall operation
                            progress_mgr.complete().await;
                        }
                        info!("Proxy {} regeneration completed successfully", proxy_id);
                    }
                    
                    // CRITICAL FIX: Clean up active regeneration tracking when task completes
                    {
                        let mut active_guard = active_cleanup.lock().await;
                        active_guard.remove(&proxy_id);
                        debug!("Cleaned up active regeneration tracking for proxy {}", proxy_id);
                    }
                })
            };

            // CRITICAL FIX: Atomic pending->active transition
            {
                let mut pending_guard = pending_clone.lock().await;
                let mut active_guard = active_clone.lock().await;
                
                // Remove from pending and add to active atomically
                pending_guard.remove(&proxy_id);
                active_guard.insert(proxy_id, regen_handle);
                
                // Drop guards to release both locks simultaneously
            }
        });

        pending.insert(proxy_id, handle);

        info!(
            "Queued proxy {} for regeneration (trigger: {} {}, scheduled in {}s)",
            proxy_id, trigger_source_type, trigger_source_id, self.config.delay_seconds
        );

        Ok(())
    }

    /// Queue a manual proxy regeneration (immediate processing)
    pub async fn queue_manual_regeneration(
        &self,
        proxy_id: Uuid,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // CRITICAL FIX: Check if proxy is already actively regenerating
        {
            let active = self.active_regenerations.lock().await;
            if active.contains_key(&proxy_id) {
                let message = format!(
                    "Proxy {} is already actively regenerating, rejecting manual regeneration request",
                    proxy_id
                );
                warn!("{}", message);
                return Err(message.into()); // Return error for manual requests so user gets feedback
            }
        }
        
        // Cancel any pending delayed regeneration
        {
            let mut pending = self.pending_regenerations.lock().await;
            if let Some(existing_handle) = pending.remove(&proxy_id) {
                existing_handle.abort();
                debug!("Cancelled pending regeneration for manual trigger: {}", proxy_id);
            }
        }

        // Start progress tracking for manual regeneration
        let _operation_name = format!("Manual Regeneration: Proxy {}", proxy_id);
        
        // Progress tracking will be handled in regenerate_single_proxy

        // Start immediate regeneration
        let pool = self.pool.clone();
        let temp_file_manager = self.temp_file_manager.clone();
        let active_clone = self.active_regenerations.clone();
        // Progress tracking will be handled in execute_regeneration
        let service_clone = self.clone();
        
        let handle = tokio::spawn(async move {
            // Update progress: starting manual regeneration
            info!("Starting manual regeneration for proxy {}", proxy_id);
            
            let service = service_clone.clone();
            let error_msg = {
                let result = service.execute_regeneration(
                    pool, 
                    temp_file_manager, 
                    Some(proxy_id), 
                    None, // Progress tracking handled in regenerate_single_proxy
                    true // Manual trigger - allow override of active ingestions
                ).await;
                
                // Extract error message if any
                match result {
                    Ok(_) => None,
                    Err(e) => Some(e.to_string()),
                }
            }; // result is dropped here
            
            // Complete or fail the operation
            if let Some(error_msg) = error_msg {
                error!("Manual regeneration failed for proxy {}: {}", proxy_id, error_msg);
            } else {
                info!("Manual regeneration completed successfully for proxy {}", proxy_id);
            }
            
            // Cleanup after completion
            {
                let mut active_guard = active_clone.lock().await;
                active_guard.remove(&proxy_id);
            }
        });

        // Track as active
        {
            let mut active = self.active_regenerations.lock().await;
            active.insert(proxy_id, handle);
        }

        info!("Started manual regeneration for proxy {}", proxy_id);
        Ok(())
    }

    /// Queue manual regeneration for all active proxies (immediate processing)
    pub async fn queue_manual_regeneration_all(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Start progress tracking for manual regeneration of all proxies
        let placeholder_proxy_id = Uuid::new_v4(); // Placeholder for tracking all proxies
        let _operation_name = "Manual Regeneration: All Active Proxies".to_string();
        
        // Initialize progress tracking if available
        // Progress tracking will be handled in execute_regeneration

        // Start immediate regeneration for all proxies
        let pool = self.pool.clone();
        let temp_file_manager = self.temp_file_manager.clone();
        let active_clone = self.active_regenerations.clone();
        // Progress tracking will be handled in execute_regeneration
        let service_clone = self.clone();
        
        let handle = tokio::spawn(async move {
            info!("Starting bulk regeneration for all active proxies");
            
            let service = service_clone.clone();
            let error_msg = {
                let result = service.execute_regeneration(
                    pool, 
                    temp_file_manager, 
                    None, 
                    None, // No progress manager for bulk operations for now
                    true // Manual trigger - allow override of active ingestions
                ).await;
                
                // Extract error message if any
                match result {
                    Ok(_) => None,
                    Err(e) => Some(e.to_string()),
                }
            }; // result is dropped here
            
            // Complete or fail the operation
            if let Some(error_msg) = error_msg {
                error!("Bulk regeneration failed: {}", error_msg);
            } else {
                info!("Bulk regeneration completed successfully");
            }
            
            // Cleanup after completion
            {
                let mut active_guard = active_clone.lock().await;
                active_guard.remove(&placeholder_proxy_id);
            }
        });

        // Track as active
        {
            let mut active = self.active_regenerations.lock().await;
            active.insert(placeholder_proxy_id, handle);
        }

        info!("Started manual regeneration for all active proxies");
        Ok(())
    }

    /// Execute the actual proxy regeneration using the new pipeline
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

        match proxy_id {
            Some(id) => {
                info!("Starting regeneration for proxy {}", id);
                if let Some(ref progress_mgr) = progress_manager {
                    let updater = progress_mgr.get_stage_updater("proxy_regeneration").await
                        .or(progress_mgr.get_stage_updater("manual_regeneration").await);
                    if let Some(updater) = updater {
                        updater.update_progress(50.0, "Starting pipeline execution").await;
                    }
                }
                
                self.regenerate_single_proxy(pool, temp_file_manager, id, progress_manager.clone()).await?;
            }
            None => {
                info!("Starting regeneration for all active proxies");
                self.regenerate_all_proxies(pool, temp_file_manager, progress_manager).await?;
            }
        }
        
        Ok(())
    }

    /// Regenerate a single proxy using the new pipeline
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
                let result = self.progress_service.create_staged_progress_manager(
                    proxy_id,
                    "proxy".to_string(),
                    OperationType::ProxyRegeneration,
                    format!("Manual Regenerate Proxy {}", proxy_id),
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
                    updater.update_progress(0.0, &format!("Pipeline execution failed: {}", error_msg)).await;
                }
                // Fail the overall operation
                progress_manager.fail(&error_msg).await;
                
                return Err(format!("Pipeline execution failed for proxy {}", proxy_id).into());
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

    /// Regenerate all active proxies using the new pipeline
    async fn regenerate_all_proxies(
        &self,
        pool: SqlitePool,
        temp_file_manager: sandboxed_file_manager::SandboxedManager,
        progress_manager: Option<Arc<ProgressManager>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Get all active proxies directly from database
        let active_proxies = sqlx::query("SELECT id, name FROM stream_proxies WHERE is_active = 1")
            .fetch_all(&pool)
            .await?;

        if active_proxies.is_empty() {
            info!("No active proxies found for regeneration");
            return Ok(());
        }

        info!("Found {} active proxies for regeneration", active_proxies.len());
        let total_proxies = active_proxies.len();
        let _placeholder_id = Uuid::new_v4(); // Use for progress tracking of batch operation

        let mut success_count = 0;
        let mut failure_count = 0;

        // Regenerate each proxy
        for (index, proxy) in active_proxies.iter().enumerate() {
            let proxy_id_str: String = proxy.get("id");
            let proxy_name: String = proxy.get("name");
            let proxy_id = parse_uuid_flexible(&proxy_id_str)?;
            
            if let Some(ref progress_mgr) = progress_manager {
                if let Some(updater) = progress_mgr.get_stage_updater("bulk_regeneration").await {
                    let percentage = (index as f64 / total_proxies as f64) * 100.0;
                    updater.update_progress(percentage, &format!("Regenerating proxy: {} ({}/{})", proxy_name, index + 1, total_proxies)).await;
                }
            }
            
            info!("Regenerating proxy: {} ({})", proxy_name, proxy_id);
            
            match self.regenerate_single_proxy(pool.clone(), temp_file_manager.clone(), proxy_id, None).await {
                Ok(()) => {
                    success_count += 1;
                    info!("Successfully regenerated proxy: {}", proxy_name);
                }
                Err(e) => {
                    failure_count += 1;
                    error!("Failed to regenerate proxy {}: {}", proxy_name, e);
                }
            }
        }

        info!("Regeneration complete: {} succeeded, {} failed", success_count, failure_count);
        
        if let Some(ref progress_mgr) = progress_manager {
            if let Some(updater) = progress_mgr.get_stage_updater("bulk_regeneration").await {
                if failure_count > 0 {
                    updater.update_progress(0.0, &format!(
                        "Completed with {} failures: {} succeeded, {} failed", 
                        failure_count, success_count, failure_count
                    )).await;
                } else {
                    updater.update_progress(100.0, &format!("All {} proxies regenerated successfully", success_count)).await;
                    updater.complete_stage().await;
                }
            }
        }

        if failure_count > 0 {
            return Err(format!("Failed to regenerate {} out of {} proxies", failure_count, success_count + failure_count).into());
        }

        Ok(())
    }

    /// Find all proxies that use a specific source and have auto_regenerate enabled
    pub async fn find_affected_proxies(
        &self,
        source_id: Uuid,
        source_type: &str,
    ) -> Result<Vec<Uuid>, sqlx::Error> {
        let source_id_str = source_id.to_string();

        let query = match source_type {
            "stream" => {
                "SELECT DISTINCT sp.id as proxy_id
                 FROM stream_proxies sp
                 JOIN proxy_sources ps ON sp.id = ps.proxy_id
                 WHERE ps.source_id = ? AND sp.is_active = 1 AND sp.auto_regenerate = 1"
            }
            "epg" => {
                "SELECT DISTINCT sp.id as proxy_id
                 FROM stream_proxies sp
                 JOIN proxy_epg_sources pes ON sp.id = pes.proxy_id  
                 WHERE pes.epg_source_id = ? AND sp.is_active = 1 AND sp.auto_regenerate = 1"
            }
            _ => return Err(sqlx::Error::TypeNotFound { type_name: format!("Invalid source_type: {}", source_type) }),
        };

        let rows = sqlx::query(query)
            .bind(&source_id_str)
            .fetch_all(&self.pool)
            .await?;

        let proxy_ids = rows
            .into_iter()
            .filter_map(|row| {
                let proxy_id_str: String = row.get("proxy_id");
                proxy_id_str.parse::<Uuid>().ok()
            })
            .collect();

        Ok(proxy_ids)
    }

    /// Get all sources (stream and EPG) associated with a specific proxy
    async fn get_proxy_sources(&self, proxy_id: Uuid) -> Result<HashSet<(Uuid, String)>, sqlx::Error> {
        let proxy_id_str = proxy_id.to_string();
        let mut sources = HashSet::new();
        
        // Get stream sources
        let stream_rows = sqlx::query(
            "SELECT source_id FROM proxy_sources WHERE proxy_id = ?"
        )
        .bind(&proxy_id_str)
        .fetch_all(&self.pool)
        .await?;
        
        for row in stream_rows {
            let source_id_str: String = row.get("source_id");
            if let Ok(source_id) = source_id_str.parse::<Uuid>() {
                sources.insert((source_id, "stream".to_string()));
            }
        }
        
        // Get EPG sources
        let epg_rows = sqlx::query(
            "SELECT epg_source_id FROM proxy_epg_sources WHERE proxy_id = ?"
        )
        .bind(&proxy_id_str)
        .fetch_all(&self.pool)
        .await?;
        
        for row in epg_rows {
            let source_id_str: String = row.get("epg_source_id");
            if let Ok(source_id) = source_id_str.parse::<Uuid>() {
                sources.insert((source_id, "epg".to_string()));
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
                debug!("Proxy {} already has a pending regeneration, skipping duplicate", proxy_id);
                return;
            }
        }
        
        {
            let active = self.active_regenerations.lock().await;
            if active.contains_key(&proxy_id) {
                debug!("Proxy {} already has an active regeneration, skipping duplicate", proxy_id);
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

    /// Monitor for completed EPG ingestions and queue affected proxies
    pub async fn monitor_epg_completions(&self) {
        // This would be called by a background task to check for recently completed EPG sources
        // For now, we'll implement a simple approach that could be enhanced later
        let pool = &self.pool;
        
        // Look for EPG sources that completed ingestion in the last minute
        match sqlx::query(
            r#"
            SELECT id, name, updated_at 
            FROM epg_sources 
            WHERE is_active = 1 
            AND datetime(updated_at) > datetime('now', '-1 minute')
            ORDER BY updated_at DESC
            "#
        )
        .fetch_all(pool)
        .await
        {
            Ok(rows) => {
                for row in rows {
                    let id: String = row.get("id");
                    let name: String = row.get("name");
                    if let Ok(source_id) = id.parse::<Uuid>() {
                        debug!("Found recently updated EPG source: {} ({})", name, source_id);
                        // COORDINATION FIX: Use coordinated method for background monitoring
                        self.queue_affected_proxies_coordinated(source_id, "epg").await;
                    }
                }
            }
            Err(e) => {
                error!("Failed to monitor EPG completions: {}", e);
            }
        }
    }


    /// Get queue status summary for API compatibility
    pub async fn get_queue_status(&self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let pending_count = self.pending_regenerations.lock().await.len();
        let active_count = self.active_regenerations.lock().await.len();
        
        // For compatibility, return basic status without progress service integration
        Ok(serde_json::json!({
            "pending": pending_count,
            "active": active_count,
            "total_tracked": pending_count + active_count,
            "status": "running"
        }))
    }

    /// Start the background processor for monitoring EPG completions
    pub fn start_processor(&self) {
        let service = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60)); // Check every minute
            loop {
                interval.tick().await;
                service.monitor_epg_completions().await;
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
    /// For now, always return false since we don't have access to ingestion state
    /// TODO: Integrate with proper ingestion state tracking if needed
    async fn has_active_ingestions(&self) -> Result<bool, Box<dyn std::error::Error>> {
        // Simplified implementation - could be extended to check actual ingestion state
        Ok(false)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_deduplication() {
        let service = ProxyRegenerationService::new(
            sqlx::SqlitePool::connect(":memory:").await.unwrap(),
            Config::default(),
            Some(RegenerationConfig { delay_seconds: 1, max_concurrent: 1 }),
            sandboxed_file_manager::SandboxedManager::new("/tmp").unwrap(),
            Arc::new(tokio::sync::RwLock::new(sysinfo::System::new())),
            None, // No progress manager for test
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
        let service = ProxyRegenerationService::new(
            sqlx::SqlitePool::connect(":memory:").await.unwrap(),
            Config::default(),
            None,
            sandboxed_file_manager::SandboxedManager::new("/tmp").unwrap(),
            Arc::new(tokio::sync::RwLock::new(sysinfo::System::new())),
            None, // No progress manager for test
        );

        let proxy_id = Uuid::new_v4();
        
        let result = service.queue_manual_regeneration(proxy_id).await;
        assert!(result.is_ok());
        
        // Should have one active regeneration
        let active_count = service.active_regenerations.lock().await.len();
        assert_eq!(active_count, 1);
    }
}