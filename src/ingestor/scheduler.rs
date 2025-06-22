use anyhow::Result;
use chrono::{DateTime, Utc};
use cron::Schedule;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

use super::{IngestionStateManager, IngestorService};
use crate::config::Config;
use crate::data_mapping::DataMappingService;
use crate::database::Database;
use crate::ingestor::state_manager::ProcessingTrigger;
use crate::logo_assets::LogoAssetService;
use crate::models::*;

pub type CacheInvalidationSender = broadcast::Sender<()>;
pub type CacheInvalidationReceiver = broadcast::Receiver<()>;

pub fn create_cache_invalidation_channel() -> (CacheInvalidationSender, CacheInvalidationReceiver) {
    broadcast::channel(100)
}

#[derive(Clone)]
struct CachedSource {
    source: StreamSource,
    schedule: Option<Schedule>,
    last_checked: Option<DateTime<Utc>>,
}

pub struct SchedulerService {
    ingestor: IngestorService,
    database: Database,
    data_mapping_service: DataMappingService,
    logo_asset_service: LogoAssetService,
    config: Config,
    run_missed_immediately: bool,
    cached_sources: Arc<RwLock<HashMap<Uuid, CachedSource>>>,
    last_cache_refresh: Arc<RwLock<DateTime<Utc>>>,
    cache_invalidation_rx: Option<CacheInvalidationReceiver>,
}

impl SchedulerService {
    pub fn new(
        state_manager: IngestionStateManager,
        database: Database,
        data_mapping_service: DataMappingService,
        logo_asset_service: LogoAssetService,
        config: Config,
        run_missed_immediately: bool,
        cache_invalidation_rx: Option<CacheInvalidationReceiver>,
    ) -> Self {
        let ingestor = IngestorService::new(state_manager.clone());
        Self {
            ingestor,
            database,
            data_mapping_service,
            logo_asset_service,
            config,
            run_missed_immediately,
            cached_sources: Arc::new(RwLock::new(HashMap::new())),
            last_cache_refresh: Arc::new(RwLock::new(Utc::now())),
            cache_invalidation_rx,
        }
    }

    pub async fn start(mut self) -> Result<()> {
        info!("Starting scheduler service (checking every second with cached schedules)");

        // Load initial cache from database
        if let Err(e) = self.refresh_cache().await {
            error!("Failed to load initial cache: {}", e);
            return Err(e);
        }

        // Log next execution times for all sources at startup
        self.log_startup_schedule().await?;

        let mut interval = interval(Duration::from_secs(1)); // Check every second

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    trace!("Scheduler tick - checking cached schedules");

                    // Check if we need to refresh cache (every 5 minutes)
                    if self.should_refresh_cache().await {
                        debug!("Refreshing scheduler cache (periodic refresh)");
                        if let Err(e) = self.refresh_cache().await {
                            error!("Failed to refresh cache: {}", e);
                        }
                    }

                    if let Err(e) = self.check_and_update_sources_from_cache().await {
                        error!("Error checking sources: {}", e);
                    }
                }
                _ = self.receive_cache_invalidation(), if self.cache_invalidation_rx.is_some() => {
                    debug!("Received cache invalidation signal");
                    if let Err(e) = self.refresh_cache().await {
                        error!("Failed to refresh cache after invalidation: {}", e);
                    }
                }
            }
        }
    }

    async fn receive_cache_invalidation(&mut self) {
        if let Some(rx) = &mut self.cache_invalidation_rx {
            let _ = rx.recv().await;
        } else {
            // If no receiver, return a future that never completes
            std::future::pending::<()>().await;
        }
    }

    async fn refresh_cache(&self) -> Result<()> {
        debug!("Refreshing scheduler cache from database");
        let sources = self.database.list_stream_sources().await?;
        let now = Utc::now();

        let mut cache = self.cached_sources.write().await;
        cache.clear();

        for source in sources {
            if !source.is_active {
                continue;
            }

            let schedule = match Schedule::from_str(&source.update_cron) {
                Ok(s) => Some(s),
                Err(e) => {
                    warn!(
                        "Source '{}' has invalid cron expression '{}': {}",
                        source.name, source.update_cron, e
                    );
                    None
                }
            };

            cache.insert(
                source.id,
                CachedSource {
                    source,
                    schedule,
                    last_checked: Some(now),
                },
            );
        }

        *self.last_cache_refresh.write().await = now;
        info!("Cached {} active sources with schedules", cache.len());
        Ok(())
    }

    async fn should_refresh_cache(&self) -> bool {
        let last_refresh = *self.last_cache_refresh.read().await;
        let now = Utc::now();
        now.signed_duration_since(last_refresh).num_minutes() >= 5
    }

    async fn log_startup_schedule(&self) -> Result<()> {
        let cache = self.cached_sources.read().await;
        let now = Utc::now();

        for cached_source in cache.values() {
            let source = &cached_source.source;

            if let Some(schedule) = &cached_source.schedule {
                if let Some(next_time) = schedule.upcoming(Utc).next() {
                    info!(
                        "Source '{}' (ID: {}) - Next scheduled update: {} (cron: {})",
                        source.name,
                        source.id,
                        next_time.format("%Y-%m-%d %H:%M:%S UTC"),
                        source.update_cron
                    );

                    // Check if we missed a scheduled run
                    if self.run_missed_immediately && source.last_ingested_at.is_some() {
                        if let Some(last_ingested) = source.last_ingested_at {
                            if let Some(should_have_run) = schedule.after(&last_ingested).next() {
                                if now >= should_have_run
                                    && now.signed_duration_since(should_have_run).num_seconds() > 5
                                {
                                    info!(
                                        "Source '{}' missed scheduled run at {} - will run immediately",
                                        source.name, should_have_run.format("%Y-%m-%d %H:%M:%S UTC")
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn check_and_update_sources_from_cache(&self) -> Result<()> {
        let now = Utc::now();
        let mut cache = self.cached_sources.write().await;

        // Collect sources that need updating to avoid holding the write lock during processing
        let mut sources_to_update = Vec::new();

        for (_source_id, cached_source) in cache.iter_mut() {
            let source = &cached_source.source;

            // Processing state and backoff checks are now handled by ingest_source_with_trigger()

            if let Some(schedule) = &cached_source.schedule {
                match self.should_update_cached(
                    source,
                    schedule,
                    now,
                    &mut cached_source.last_checked,
                ) {
                    Ok(should_update) => {
                        if should_update {
                            sources_to_update.push(source.clone());
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Error checking if source '{}' should update: {}",
                            source.name, e
                        );
                    }
                }
            }
        }

        // Release the write lock before processing
        drop(cache);

        // Process sources that need updating
        for source in sources_to_update {
            self.process_source_update(source).await;
        }

        Ok(())
    }

    async fn process_source_update(&self, source: StreamSource) {
        info!(
            "Triggering scheduled refresh for source '{}' (ID: {}, URL: {}, Cron: {})",
            source.name, source.id, source.url, source.update_cron
        );

        let _success = match self
            .ingestor
            .ingest_source_with_trigger(&source, ProcessingTrigger::Scheduler)
            .await
        {
            Ok(channels) => {
                info!(
                    "Ingestion completed for '{}': {} channels",
                    source.name,
                    channels.len()
                );

                // Store original channels without data mapping
                // Data mapping will be applied during proxy generation
                info!(
                    "Storing {} original channels for source '{}' (data mapping will be applied during proxy generation)",
                    channels.len(),
                    source.name
                );

                // Update the channels in database
                match self
                    .database
                    .update_source_channels(
                        source.id,
                        &channels,
                        Some(self.ingestor.get_state_manager()),
                    )
                    .await
                {
                    Ok(_) => {
                        // Update last_ingested_at timestamp AFTER successful database write
                        match self.database.update_source_last_ingested(source.id).await {
                            Ok(last_ingested_timestamp) => {
                                // Update cached source data with exact timestamp from database
                                {
                                    let mut cache = self.cached_sources.write().await;
                                    if let Some(cached_source) = cache.get_mut(&source.id) {
                                        cached_source.source.last_ingested_at =
                                            Some(last_ingested_timestamp);
                                        debug!(
                                            "Updated cached last_ingested_at for source '{}' to {}",
                                            source.name,
                                            last_ingested_timestamp.format("%Y-%m-%d %H:%M:%S UTC")
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                error!(
                                    "Failed to update last_ingested_at for source '{}': {}",
                                    source.name, e
                                );
                            }
                        }

                        // Mark ingestion as completed with final channel count
                        self.ingestor
                            .get_state_manager()
                            .complete_ingestion(source.id, channels.len())
                            .await;

                        // Calculate next update time
                        if let Ok(schedule) = Schedule::from_str(&source.update_cron) {
                            if let Some(next_time) = schedule.upcoming(Utc).next() {
                                info!(
                                    "Scheduled refresh completed for source '{}' - Next update: {}",
                                    source.name,
                                    next_time.format("%Y-%m-%d %H:%M:%S UTC")
                                );
                            }
                        }
                        true
                    }
                    Err(e) => {
                        error!(
                            "Failed to save channels to database for source '{}': {}",
                            source.name, e
                        );
                        false
                    }
                }
            }
            Err(e) => {
                error!("Failed to refresh source '{}': {}", source.name, e);
                false
            }
        };

        // Processing state, backoff, and error handling are all managed by ingest_source_with_trigger
    }

    fn should_update_cached(
        &self,
        source: &StreamSource,
        schedule: &Schedule,
        now: DateTime<Utc>,
        last_checked: &mut Option<DateTime<Utc>>,
    ) -> Result<bool> {
        // Only check if we haven't checked this source in the last second
        if let Some(last_check_time) = *last_checked {
            if now.signed_duration_since(last_check_time).num_seconds() < 1 {
                return Ok(false);
            }
        }

        *last_checked = Some(now);

        if let Some(last_ingested) = source.last_ingested_at {
            // Find the next scheduled time after the last ingestion
            if let Some(next_time) = schedule.after(&last_ingested).next() {
                let should_run = now >= next_time;
                if should_run {
                    trace!(
                        "Source '{}' should update: last_ingested={}, next_time={}, now={}",
                        source.name,
                        last_ingested.format("%Y-%m-%d %H:%M:%S UTC"),
                        next_time.format("%Y-%m-%d %H:%M:%S UTC"),
                        now.format("%Y-%m-%d %H:%M:%S UTC")
                    );
                }
                return Ok(should_run);
            }
        }

        // If never ingested, check if it's time for the first run
        let should_run_first_time = schedule.upcoming(Utc).next().is_some();
        if should_run_first_time && source.last_ingested_at.is_none() {
            trace!(
                "Source '{}' has never been ingested and schedule is active - should run",
                source.name
            );
        }

        Ok(should_run_first_time && source.last_ingested_at.is_none())
    }
}
