//! Proxy Regeneration Queue Service
//!
//! This service manages automatic regeneration of stream proxies when their
//! associated sources (stream or EPG) are updated. It implements smart queuing
//! to prevent duplicate regenerations and provides configurable delays.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{sleep, Duration, Instant};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::{
    config::Config,
    database::Database,
    data_mapping::DataMappingService,
    logo_assets::LogoAssetService,
    proxy::ProxyService,
    utils::sqlite::SqliteRowExt,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueEntry {
    pub id: Uuid,
    pub proxy_id: Uuid,
    pub trigger_source_id: Option<Uuid>,
    pub trigger_source_type: Option<String>,
    pub status: QueueStatus,
    pub scheduled_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum QueueStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

/// Configuration for the regeneration queue
#[derive(Debug, Clone)]
pub struct RegenerationConfig {
    /// Delay in seconds after source updates before triggering regeneration
    pub delay_seconds: u64,
    /// Maximum concurrent regenerations
    pub max_concurrent: usize,
    /// Cleanup completed entries older than this (in hours)
    pub cleanup_after_hours: u64,
}

impl Default for RegenerationConfig {
    fn default() -> Self {
        Self {
            delay_seconds: 15,
            max_concurrent: 2,
            cleanup_after_hours: 24,
        }
    }
}

/// Service for managing proxy regeneration queue
#[derive(Clone)]
pub struct ProxyRegenerationService {
    pool: SqlitePool,
    config: RegenerationConfig,
    running_tasks: Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
    is_processing: Arc<RwLock<bool>>,
}

impl ProxyRegenerationService {
    pub fn new(pool: SqlitePool, config: Option<RegenerationConfig>) -> Self {
        Self {
            pool,
            config: config.unwrap_or_default(),
            running_tasks: Arc::new(Mutex::new(HashMap::new())),
            is_processing: Arc::new(RwLock::new(false)),
        }
    }

    /// Queue a proxy for regeneration due to source update
    pub async fn queue_proxy_regeneration(
        &self,
        proxy_id: Uuid,
        trigger_source_id: Uuid,
        trigger_source_type: &str,
    ) -> Result<(), sqlx::Error> {
        let queue_id = Uuid::new_v4();
        let scheduled_at = Utc::now() + chrono::Duration::seconds(self.config.delay_seconds as i64);
        
        let proxy_id_str = proxy_id.to_string();
        let queue_id_str = queue_id.to_string();
        let trigger_source_id_str = trigger_source_id.to_string();
        let scheduled_at_str = scheduled_at.to_rfc3339();
        let created_at_str = Utc::now().to_rfc3339();

        // Use INSERT OR REPLACE to handle deduplication
        // If a queue entry already exists for this proxy, update it with the new schedule time
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO proxy_regeneration_queue 
            (id, proxy_id, trigger_source_id, trigger_source_type, status, scheduled_at, created_at)
            VALUES (?, ?, ?, ?, 'pending', ?, ?)
            "#
        )
        .bind(queue_id_str)
        .bind(proxy_id_str)
        .bind(trigger_source_id_str)
        .bind(trigger_source_type)
        .bind(scheduled_at_str)
        .bind(created_at_str)
        .execute(&self.pool)
        .await?;

        info!(
            "Queued proxy {} for regeneration (trigger: {} {}, scheduled: {})",
            proxy_id, trigger_source_type, trigger_source_id, scheduled_at
        );

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
                r#"
                SELECT DISTINCT sp.id
                FROM stream_proxies sp
                JOIN proxy_sources ps ON sp.id = ps.proxy_id
                WHERE ps.source_id = ? 
                AND sp.is_active = TRUE 
                AND sp.auto_regenerate = TRUE
                "#
            }
            "epg" => {
                r#"
                SELECT DISTINCT sp.id
                FROM stream_proxies sp
                JOIN proxy_epg_sources pes ON sp.id = pes.proxy_id
                WHERE pes.epg_source_id = ? 
                AND sp.is_active = TRUE 
                AND sp.auto_regenerate = TRUE
                "#
            }
            _ => return Ok(vec![]),
        };

        let rows = sqlx::query(query)
            .bind(source_id_str)
            .fetch_all(&self.pool)
            .await?;

        let proxy_ids = rows
            .into_iter()
            .filter_map(|row| row.get_uuid("id").ok())
            .collect();

        Ok(proxy_ids)
    }

    /// Start the background processor
    pub async fn start_processor(
        &self,
        database: Database,
        data_mapping_service: DataMappingService,
        logo_asset_service: LogoAssetService,
        config: Config,
    ) {
        let pool = self.pool.clone();
        let regeneration_config = self.config.clone();
        let is_processing = self.is_processing.clone();
        let running_tasks = self.running_tasks.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            
            loop {
                interval.tick().await;
                
                // Check if any processing is already happening
                {
                    let processing = is_processing.read().await;
                    if *processing {
                        continue;
                    }
                }

                // Get ready queue entries
                match Self::get_ready_queue_entries(&pool).await {
                    Ok(entries) if !entries.is_empty() => {
                        info!("Processing {} queued proxy regenerations", entries.len());
                        
                        // Set processing flag
                        {
                            let mut processing = is_processing.write().await;
                            *processing = true;
                        }

                        // Process entries
                        for entry in entries {
                            if running_tasks.lock().await.len() >= regeneration_config.max_concurrent {
                                // Wait for a slot
                                while running_tasks.lock().await.len() >= regeneration_config.max_concurrent {
                                    sleep(Duration::from_millis(100)).await;
                                }
                            }

                            let proxy_id = entry.proxy_id;
                            let task = Self::spawn_regeneration_task(
                                entry,
                                pool.clone(),
                                database.clone(),
                                data_mapping_service.clone(),
                                logo_asset_service.clone(),
                                config.clone(),
                            );

                            running_tasks.lock().await.insert(proxy_id, task);
                        }

                        // Clear processing flag
                        {
                            let mut processing = is_processing.write().await;
                            *processing = false;
                        }
                    }
                    Ok(_) => {
                        // No entries to process
                    }
                    Err(e) => {
                        error!("Failed to get queue entries: {}", e);
                    }
                }

                // Cleanup completed tasks
                Self::cleanup_completed_tasks(&running_tasks).await;
                
                // Cleanup old queue entries
                if let Err(e) = Self::cleanup_old_entries(&pool, regeneration_config.cleanup_after_hours).await {
                    error!("Failed to cleanup old queue entries: {}", e);
                }
            }
        });
    }

    async fn get_ready_queue_entries(pool: &SqlitePool) -> Result<Vec<QueueEntry>, sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        
        let rows = sqlx::query(
            r#"
            SELECT id, proxy_id, trigger_source_id, trigger_source_type, status,
                   scheduled_at, created_at, started_at, completed_at, error_message
            FROM proxy_regeneration_queue
            WHERE status = 'pending' AND scheduled_at <= ?
            ORDER BY scheduled_at ASC
            "#
        )
        .bind(now)
        .fetch_all(pool)
        .await?;

        let entries = rows
            .into_iter()
            .filter_map(|row| {
                Some(QueueEntry {
                    id: row.get_uuid("id").ok()?,
                    proxy_id: row.get_uuid("proxy_id").ok()?,
                    trigger_source_id: row.get::<Option<String>, _>("trigger_source_id")
                        .and_then(|s| s.parse().ok()),
                    trigger_source_type: row.get("trigger_source_type"),
                    status: match row.get::<String, _>("status").as_str() {
                        "pending" => QueueStatus::Pending,
                        "processing" => QueueStatus::Processing,
                        "completed" => QueueStatus::Completed,
                        "failed" => QueueStatus::Failed,
                        _ => QueueStatus::Pending,
                    },
                    scheduled_at: row.get_datetime("scheduled_at"),
                    created_at: row.get_datetime("created_at"),
                    started_at: row.get_datetime_opt("started_at"),
                    completed_at: row.get_datetime_opt("completed_at"),
                    error_message: row.get("error_message"),
                })
            })
            .collect();

        Ok(entries)
    }

    fn spawn_regeneration_task(
        entry: QueueEntry,
        pool: SqlitePool,
        database: Database,
        data_mapping_service: DataMappingService,
        logo_asset_service: LogoAssetService,
        config: Config,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let start_time = Instant::now();
            
            // Mark as processing
            if let Err(e) = Self::update_queue_status(&pool, entry.id, QueueStatus::Processing, None).await {
                error!("Failed to update queue status to processing: {}", e);
                return;
            }

            // Get the proxy
            let proxy = match database.get_stream_proxy(entry.proxy_id).await {
                Ok(Some(proxy)) => proxy,
                Ok(None) => {
                    warn!("Proxy {} not found, removing from queue", entry.proxy_id);
                    let _ = Self::update_queue_status(&pool, entry.id, QueueStatus::Failed, 
                        Some("Proxy not found".to_string())).await;
                    return;
                }
                Err(e) => {
                    error!("Failed to get proxy {}: {}", entry.proxy_id, e);
                    let _ = Self::update_queue_status(&pool, entry.id, QueueStatus::Failed, 
                        Some(format!("Failed to get proxy: {}", e))).await;
                    return;
                }
            };

            // Use the new dependency injection architecture
            let proxy_service = ProxyService::new(config.storage.clone());
            
            // Create config resolver
            use crate::repositories::{StreamProxyRepository, StreamSourceRepository, FilterRepository};
            let proxy_repo = StreamProxyRepository::new(database.pool());
            let stream_source_repo = StreamSourceRepository::new(database.pool());
            let filter_repo = FilterRepository::new(database.pool());
            
            let config_resolver = crate::proxy::config_resolver::ProxyConfigResolver::new(
                proxy_repo,
                stream_source_repo,
                filter_repo,
                database.clone(),
            );

            // Resolve proxy configuration upfront (single database query)
            match config_resolver.resolve_config(entry.proxy_id).await {
                Ok(resolved_config) => {
                    // Validate configuration
                    if let Err(e) = config_resolver.validate_config(&resolved_config) {
                        error!("Invalid proxy configuration for {}: {}", entry.proxy_id, e);
                        let _ = Self::update_queue_status(&pool, entry.id, QueueStatus::Failed, 
                            Some(format!("Invalid configuration: {}", e))).await;
                        return;
                    }

                    // Create production output destination
                    // TODO: Get actual proxy_output_file_manager from config
                    let output = crate::models::GenerationOutput::InMemory; // Fallback for now
                    
                    // Generate using dependency injection
                    match proxy_service.generate_proxy_with_config(
                        resolved_config,
                        output,
                        &database,
                        &data_mapping_service,
                        &logo_asset_service,
                        &config.web.base_url,
                        config.data_mapping_engine.clone(),
                    ).await {
                        Ok(generation) => {
                            // Save the M3U file using the proxy ULID for proper file management
                            match proxy_service.save_m3u_file_with_manager(&proxy.ulid, &generation.m3u_content, None).await {
                                Ok(_) => {
                                    let duration = start_time.elapsed();
                                    info!(
                                        "Successfully auto-regenerated proxy '{}' with {} channels using dependency injection in {:?}",
                                        proxy.name, generation.channel_count, duration
                                    );
                                    let _ = Self::update_queue_status(&pool, entry.id, QueueStatus::Completed, None).await;
                                }
                                Err(e) => {
                                    error!("Failed to save regenerated M3U for proxy {}: {}", entry.proxy_id, e);
                                    let _ = Self::update_queue_status(&pool, entry.id, QueueStatus::Failed, 
                                        Some(format!("Failed to save M3U: {}", e))).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to regenerate proxy {} using dependency injection: {}", entry.proxy_id, e);
                            let _ = Self::update_queue_status(&pool, entry.id, QueueStatus::Failed, 
                                Some(format!("Generation failed: {}", e))).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to resolve proxy configuration for {}: {}", entry.proxy_id, e);
                    let _ = Self::update_queue_status(&pool, entry.id, QueueStatus::Failed, 
                        Some(format!("Config resolution failed: {}", e))).await;
                }
            }
        })
    }

    async fn update_queue_status(
        pool: &SqlitePool,
        entry_id: Uuid,
        status: QueueStatus,
        error_message: Option<String>,
    ) -> Result<(), sqlx::Error> {
        let entry_id_str = entry_id.to_string();
        let status_str = match status {
            QueueStatus::Pending => "pending",
            QueueStatus::Processing => "processing",
            QueueStatus::Completed => "completed",
            QueueStatus::Failed => "failed",
        };

        let now = Utc::now().to_rfc3339();

        match status {
            QueueStatus::Processing => {
                sqlx::query(
                    "UPDATE proxy_regeneration_queue SET status = ?, started_at = ? WHERE id = ?"
                )
                .bind(status_str)
                .bind(now)
                .bind(entry_id_str)
                .execute(pool)
                .await?;
            }
            QueueStatus::Completed | QueueStatus::Failed => {
                sqlx::query(
                    "UPDATE proxy_regeneration_queue SET status = ?, completed_at = ?, error_message = ? WHERE id = ?"
                )
                .bind(status_str)
                .bind(now)
                .bind(error_message)
                .bind(entry_id_str)
                .execute(pool)
                .await?;
            }
            _ => {
                sqlx::query("UPDATE proxy_regeneration_queue SET status = ? WHERE id = ?")
                    .bind(status_str)
                    .bind(entry_id_str)
                    .execute(pool)
                    .await?;
            }
        }

        Ok(())
    }

    async fn cleanup_completed_tasks(
        running_tasks: &Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
    ) {
        let mut tasks = running_tasks.lock().await;
        let mut completed = Vec::new();

        for (proxy_id, task) in tasks.iter() {
            if task.is_finished() {
                completed.push(*proxy_id);
            }
        }

        for proxy_id in completed {
            if let Some(task) = tasks.remove(&proxy_id) {
                let _ = task.await;
                debug!("Cleaned up completed regeneration task for proxy {}", proxy_id);
            }
        }
    }

    async fn cleanup_old_entries(
        pool: &SqlitePool,
        cleanup_after_hours: u64,
    ) -> Result<(), sqlx::Error> {
        let cutoff = Utc::now() - chrono::Duration::hours(cleanup_after_hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let result = sqlx::query(
            "DELETE FROM proxy_regeneration_queue WHERE status IN ('completed', 'failed') AND completed_at < ?"
        )
        .bind(cutoff_str)
        .execute(pool)
        .await?;

        if result.rows_affected() > 0 {
            debug!("Cleaned up {} old queue entries", result.rows_affected());
        }

        Ok(())
    }

    /// Get current queue status for monitoring
    pub async fn get_queue_status(&self) -> Result<HashMap<String, usize>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT status, COUNT(*) as count FROM proxy_regeneration_queue GROUP BY status"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut status = HashMap::new();
        for row in rows {
            let status_name: String = row.get("status");
            let count: i64 = row.get("count");
            status.insert(status_name, count as usize);
        }

        Ok(status)
    }
}