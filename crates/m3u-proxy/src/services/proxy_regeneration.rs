//! Proxy Regeneration Service
//!
//! This service manages automatic regeneration of stream proxies when their
//! associated sources (stream or EPG) are updated. It uses pure in-memory state
//! with Tokio timers for delayed execution and deduplication.

use chrono::Utc;
// Serde imports removed - no longer needed after cleaning up legacy structs
use sqlx::{SqlitePool, Row};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use crate::config::Config;
use crate::utils::uuid_parser::parse_uuid_flexible;
use crate::services::progress_service::{ProgressService, OperationType, UniversalProgress, UniversalState};

// Removed scheduler dependency - now uses pure in-memory Tokio timers

/// Configuration for the regeneration service
#[derive(Debug, Clone)]
pub struct RegenerationConfig {
    /// Delay in seconds after source updates before triggering regeneration
    pub delay_seconds: u64,
    /// Maximum concurrent regenerations
    pub max_concurrent: usize,
}

impl Default for RegenerationConfig {
    fn default() -> Self {
        Self {
            delay_seconds: 15,
            max_concurrent: 2,
        }
    }
}

/// Service for managing proxy regeneration with UniversalProgress tracking
#[derive(Clone)]
pub struct ProxyRegenerationService {
    pool: SqlitePool,
    config: RegenerationConfig,
    app_config: Config,
    /// Active delayed regeneration timers
    pending_regenerations: Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
    /// Currently running regeneration tasks
    active_regenerations: Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
    /// Universal progress service for tracking
    progress_service: Arc<ProgressService>,
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
    ) -> Self {
        Self {
            pool,
            config: config.unwrap_or_default(),
            app_config,
            pending_regenerations: Arc::new(Mutex::new(HashMap::new())),
            active_regenerations: Arc::new(Mutex::new(HashMap::new())),
            progress_service,
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
        
        // Start progress tracking using UniversalProgress
        let operation_name = format!(
            "Regenerate Proxy {} (triggered by {} source {})",
            proxy_id, trigger_source_type, trigger_source_id
        );
        let trigger_source_type_owned = trigger_source_type.to_string();
        let callback = Arc::new(self.progress_service.start_operation(
            proxy_id,
            OperationType::ProxyRegeneration,
            operation_name,
        ).await);

        // Create delayed regeneration task
        let pool = self.pool.clone();
        let temp_file_manager = self.temp_file_manager.clone();
        let delay_seconds = self.config.delay_seconds;
        let pending_clone = self.pending_regenerations.clone();
        let active_clone = self.active_regenerations.clone();
        let progress_service = self.progress_service.clone();
        let service_clone = self.clone();
        
        let callback_for_spawn = Arc::clone(&callback);
        let handle = tokio::spawn(async move {
            // Update progress: waiting for delay
            let delay_progress = UniversalProgress::new(
                proxy_id,
                OperationType::ProxyRegeneration,
                format!("Regenerate Proxy {} (triggered by {} source {})", proxy_id, trigger_source_type_owned, trigger_source_id),
            )
            .set_state(UniversalState::Preparing)
            .update_step(format!("Waiting {}s before regeneration", delay_seconds));
            callback_for_spawn(delay_progress);
            
            // Wait for the delay
            sleep(Duration::from_secs(delay_seconds)).await;
            
            // ATOMIC TRANSITION: Remove from pending and add to active in single operation
            let regen_handle = {
                let service = service_clone.clone();
                tokio::spawn(async move {
                    // Update progress: starting regeneration
                    let start_progress = UniversalProgress::new(
                        proxy_id,
                        OperationType::ProxyRegeneration,
                        format!("Regenerate Proxy {} (triggered by {} source {})", proxy_id, trigger_source_type_owned, trigger_source_id),
                    )
                    .set_state(UniversalState::Processing)
                    .update_step("Starting proxy regeneration".to_string());
                    callback_for_spawn(start_progress);
                    
                    let error_msg = {
                        let result = service.execute_regeneration(
                            pool, 
                            temp_file_manager, 
                            Some(proxy_id), 
                            Some(Arc::clone(&callback_for_spawn)), // Pass the progress callback
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
                        progress_service.fail_operation(proxy_id, error_msg).await;
                    } else {
                        progress_service.complete_operation(proxy_id).await;
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
        let operation_name = format!("Manual Regeneration: Proxy {}", proxy_id);
        let callback = Arc::new(self.progress_service.start_operation(
            proxy_id,
            OperationType::ProxyRegeneration,
            operation_name,
        ).await);

        // Start immediate regeneration
        let pool = self.pool.clone();
        let temp_file_manager = self.temp_file_manager.clone();
        let active_clone = self.active_regenerations.clone();
        let progress_service = self.progress_service.clone();
        let service_clone = self.clone();
        
        let handle = tokio::spawn(async move {
            // Update progress: starting manual regeneration
            let start_progress = UniversalProgress::new(
                proxy_id,
                OperationType::ProxyRegeneration,
                format!("Manual Regeneration: Proxy {}", proxy_id),
            )
            .set_state(UniversalState::Processing)
            .update_step("Starting manual regeneration".to_string());
            callback(start_progress);
            
            let service = service_clone.clone();
            let error_msg = {
                let result = service.execute_regeneration(
                    pool, 
                    temp_file_manager, 
                    Some(proxy_id), 
                    Some(Arc::clone(&callback)), // Pass the progress callback for manual trigger
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
                progress_service.fail_operation(proxy_id, error_msg).await;
            } else {
                progress_service.complete_operation(proxy_id).await;
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
        let operation_name = "Manual Regeneration: All Active Proxies".to_string();
        let callback = Arc::new(self.progress_service.start_operation(
            placeholder_proxy_id,
            OperationType::ProxyRegeneration,
            operation_name,
        ).await);

        // Start immediate regeneration for all proxies
        let pool = self.pool.clone();
        let temp_file_manager = self.temp_file_manager.clone();
        let active_clone = self.active_regenerations.clone();
        let progress_service = self.progress_service.clone();
        let service_clone = self.clone();
        
        let handle = tokio::spawn(async move {
            // Update progress: starting batch regeneration
            let start_progress = UniversalProgress::new(
                placeholder_proxy_id,
                OperationType::ProxyRegeneration,
                "Manual Regeneration: All Active Proxies".to_string(),
            )
            .set_state(UniversalState::Processing)
            .update_step("Starting regeneration for all active proxies".to_string());
            callback(start_progress);
            
            let service = service_clone.clone();
            let error_msg = {
                let result = service.execute_regeneration(
                    pool, 
                    temp_file_manager, 
                    None, 
                    Some(Arc::clone(&callback)), // Pass the progress callback for bulk regeneration
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
                progress_service.fail_operation(placeholder_proxy_id, error_msg).await;
            } else {
                progress_service.complete_operation(placeholder_proxy_id).await;
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
        progress_callback: Option<Arc<crate::services::progress_service::UniversalProgressCallback>>,
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
                if let Some(ref callback) = progress_callback {
                    let progress = UniversalProgress::new(
                        id,
                        OperationType::ProxyRegeneration,
                        format!("Regenerate Proxy {}", id),
                    )
                    .set_state(UniversalState::Processing)
                    .update_step("Starting pipeline execution".to_string());
                    callback(progress);
                }
                
                self.regenerate_single_proxy(pool, temp_file_manager, id, progress_callback).await?;
            }
            None => {
                info!("Starting regeneration for all active proxies");
                self.regenerate_all_proxies(pool, temp_file_manager, progress_callback).await?;
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
        progress_callback: Option<Arc<crate::services::progress_service::UniversalProgressCallback>>,
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
        
        if let Some(ref callback) = progress_callback {
            let progress = UniversalProgress::new(
                proxy_id,
                OperationType::ProxyRegeneration,
                format!("Regenerate Proxy {}", proxy_id),
            )
            .set_state(UniversalState::Processing)
            .update_step("Creating pipeline orchestrator".to_string());
            callback(progress);
        }

        // Create and execute pipeline
        let mut orchestrator = factory.create_for_proxy(proxy_id).await?;
        
        // Store factory reference for cleanup
        let factory_for_cleanup = factory.clone();
        
        if let Some(ref callback) = progress_callback {
            let progress = UniversalProgress::new(
                proxy_id,
                OperationType::ProxyRegeneration,
                format!("Regenerate Proxy {}", proxy_id),
            )
            .set_state(UniversalState::Processing)
            .update_step("Executing pipeline stages".to_string())
            .update_percentage(50.0);
            callback(progress);
        }
        
        let execution_result = orchestrator.execute_pipeline(&progress_callback).await?;

        match execution_result.status {
            crate::pipeline::models::PipelineStatus::Completed => {
                info!("Successfully completed regeneration for proxy {}", proxy_id);
                
                // CRITICAL FIX: Unregister orchestrator after successful completion
                factory_for_cleanup.unregister_orchestrator(proxy_id).await;
                
                if let Some(ref callback) = progress_callback {
                    let progress = UniversalProgress::new(
                        proxy_id,
                        OperationType::ProxyRegeneration,
                        format!("Regenerate Proxy {}", proxy_id),
                    )
                    .set_state(UniversalState::Completed)
                    .update_step("Pipeline execution completed successfully".to_string())
                    .update_percentage(100.0);
                    callback(progress);
                }
            }
            crate::pipeline::models::PipelineStatus::Failed => {
                let error_msg = execution_result.error_message
                    .unwrap_or_else(|| "Unknown error".to_string());
                error!("Pipeline failed for proxy {}: {}", proxy_id, error_msg);
                
                // CRITICAL FIX: Unregister orchestrator after failure
                factory_for_cleanup.unregister_orchestrator(proxy_id).await;
                
                if let Some(ref callback) = progress_callback {
                    let progress = UniversalProgress::new(
                        proxy_id,
                        OperationType::ProxyRegeneration,
                        format!("Regenerate Proxy {}", proxy_id),
                    )
                    .set_error(format!("Pipeline execution failed: {}", error_msg));
                    callback(progress);
                }
                
                return Err(format!("Pipeline execution failed for proxy {}", proxy_id).into());
            }
            _ => {
                warn!("Pipeline completed with status: {:?} for proxy {}", 
                    execution_result.status, proxy_id);
                
                // CRITICAL FIX: Unregister orchestrator for any completion status
                factory_for_cleanup.unregister_orchestrator(proxy_id).await;
            }
        }

        Ok(())
    }

    /// Regenerate all active proxies using the new pipeline
    async fn regenerate_all_proxies(
        &self,
        pool: SqlitePool,
        temp_file_manager: sandboxed_file_manager::SandboxedManager,
        progress_callback: Option<Arc<crate::services::progress_service::UniversalProgressCallback>>,
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
        let placeholder_id = Uuid::new_v4(); // Use for progress tracking of batch operation

        let mut success_count = 0;
        let mut failure_count = 0;

        // Regenerate each proxy
        for (index, proxy) in active_proxies.iter().enumerate() {
            let proxy_id_str: String = proxy.get("id");
            let proxy_name: String = proxy.get("name");
            let proxy_id = parse_uuid_flexible(&proxy_id_str)?;
            
            if let Some(ref callback) = progress_callback {
                let progress = UniversalProgress::new(
                    placeholder_id,
                    OperationType::ProxyRegeneration,
                    "Manual Regeneration: All Active Proxies".to_string(),
                )
                .set_state(UniversalState::Processing)
                .update_step(format!("Regenerating proxy: {} ({}/{})", proxy_name, index + 1, total_proxies))
                .update_percentage((index as f64 / total_proxies as f64) * 100.0);
                callback(progress);
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
        
        if let Some(ref callback) = progress_callback {
            if failure_count > 0 {
                let progress = UniversalProgress::new(
                    placeholder_id,
                    OperationType::ProxyRegeneration,
                    "Manual Regeneration: All Active Proxies".to_string(),
                )
                .set_error(format!(
                    "Completed with {} failures: {} succeeded, {} failed", 
                    failure_count, success_count, failure_count
                ));
                callback(progress);
            } else {
                let progress = UniversalProgress::new(
                    placeholder_id,
                    OperationType::ProxyRegeneration,
                    "Manual Regeneration: All Active Proxies".to_string(),
                )
                .set_state(UniversalState::Completed)
                .update_step(format!("All {} proxies regenerated successfully", success_count))
                .update_percentage(100.0);
                callback(progress);
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

    /// Queue regeneration for all affected proxies after source update (scheduler-coordinated)
    /// This version includes additional coordination to prevent conflicts with manual regenerations
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

                // COORDINATION FIX: Filter out proxies that are already actively regenerating
                let mut available_proxies = Vec::new();
                let mut skipped_count = 0;
                
                for proxy_id in proxy_ids {
                    if self.has_active_regeneration(proxy_id).await {
                        skipped_count += 1;
                        debug!("Skipping proxy {} - already actively regenerating", proxy_id);
                    } else {
                        available_proxies.push(proxy_id);
                    }
                }

                if available_proxies.is_empty() {
                    debug!(
                        "All {} proxies affected by {} source {} are already actively regenerating, skipping",
                        skipped_count, source_type, source_id
                    );
                    return;
                }

                info!(
                    "Found {} available proxies affected by {} source {} update, queueing for regeneration (skipped {} already active)",
                    available_proxies.len(), source_type, source_id, skipped_count
                );

                for proxy_id in available_proxies {
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
        
        // Get count from ProgressService
        let progress_operations = self.progress_service.get_progress_by_type(
            OperationType::ProxyRegeneration
        ).await;
        let total_entries = progress_operations.len();
        
        Ok(serde_json::json!({
            "pending": pending_count,
            "active": active_count,
            "total_tracked": total_entries,
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
    /// This checks both local active regenerations AND the universal progress service
    pub async fn has_active_regeneration(&self, proxy_id: Uuid) -> bool {
        // Check service-level active regenerations
        let active = self.active_regenerations.lock().await;
        if active.contains_key(&proxy_id) {
            return true;
        }
        drop(active); // Release lock early
        
        // RACE CONDITION FIX: Also check the universal progress service for any ProxyRegeneration
        // operations for this proxy. This covers both API requests and background regenerations.
        let progress_operations = self.progress_service.get_progress_by_type(
            crate::services::progress_service::OperationType::ProxyRegeneration
        ).await;
        
        for (_operation_id, progress) in progress_operations {
            // Check if this progress operation is for our proxy
            if let Some(proxy_id_value) = progress.metadata.get("proxy_id") {
                if let Some(progress_proxy_id_str) = proxy_id_value.as_str() {
                    if let Ok(progress_proxy_id) = progress_proxy_id_str.parse::<Uuid>() {
                        if progress_proxy_id == proxy_id {
                            // Found an active regeneration for this proxy
                            return true;
                        }
                    }
                }
            }
        }
        
        false
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
    async fn has_active_ingestions(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let all_progress = self.progress_service.get_ingestion_state_manager().get_all_progress().await;
        
        for (source_id, progress) in all_progress {
            match progress.state {
                crate::models::IngestionState::Connecting | 
                crate::models::IngestionState::Downloading | 
                crate::models::IngestionState::Processing | 
                crate::models::IngestionState::Parsing | 
                crate::models::IngestionState::Saving => {
                    info!("Active ingestion found for source {}: state={:?}", source_id, progress.state);
                    return Ok(true); // Active ingestion found
                }
                _ => continue,
            }
        }
        Ok(false)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_deduplication() {
        let state_manager = crate::ingestor::IngestionStateManager::new();
        let progress_service = Arc::new(ProgressService::new(state_manager));
        
        let service = ProxyRegenerationService::new(
            sqlx::SqlitePool::connect(":memory:").await.unwrap(),
            Config::default(),
            Some(RegenerationConfig { delay_seconds: 1, max_concurrent: 1 }),
            sandboxed_file_manager::SandboxedManager::new("/tmp").unwrap(),
            Arc::new(tokio::sync::RwLock::new(sysinfo::System::new())),
            progress_service,
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
        let state_manager = crate::ingestor::IngestionStateManager::new();
        let progress_service = Arc::new(ProgressService::new(state_manager));
        
        let service = ProxyRegenerationService::new(
            sqlx::SqlitePool::connect(":memory:").await.unwrap(),
            Config::default(),
            None,
            sandboxed_file_manager::SandboxedManager::new("/tmp").unwrap(),
            Arc::new(tokio::sync::RwLock::new(sysinfo::System::new())),
            progress_service,
        );

        let proxy_id = Uuid::new_v4();
        
        let result = service.queue_manual_regeneration(proxy_id).await;
        assert!(result.is_ok());
        
        // Should have one active regeneration
        let active_count = service.active_regenerations.lock().await.len();
        assert_eq!(active_count, 1);
    }
}