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
use crate::database::Database;
use crate::ingestor::state_manager::ProcessingTrigger;
use crate::models::*;

pub type CacheInvalidationSender = broadcast::Sender<()>;
pub type CacheInvalidationReceiver = broadcast::Receiver<()>;

pub fn create_cache_invalidation_channel() -> (CacheInvalidationSender, CacheInvalidationReceiver) {
    broadcast::channel(100)
}

#[derive(Clone)]
enum SchedulableSource {
    Stream(StreamSource),
    Epg(EpgSource),
}

impl SchedulableSource {
    fn id(&self) -> Uuid {
        match self {
            SchedulableSource::Stream(s) => s.id,
            SchedulableSource::Epg(s) => s.id,
        }
    }

    fn name(&self) -> &str {
        match self {
            SchedulableSource::Stream(s) => &s.name,
            SchedulableSource::Epg(s) => &s.name,
        }
    }

    fn update_cron(&self) -> &str {
        match self {
            SchedulableSource::Stream(s) => &s.update_cron,
            SchedulableSource::Epg(s) => &s.update_cron,
        }
    }

    fn last_ingested_at(&self) -> Option<DateTime<Utc>> {
        match self {
            SchedulableSource::Stream(s) => s.last_ingested_at,
            SchedulableSource::Epg(s) => s.last_ingested_at,
        }
    }

    fn source_type(&self) -> &str {
        match self {
            SchedulableSource::Stream(_) => "Stream",
            SchedulableSource::Epg(_) => "EPG",
        }
    }
}

#[derive(Clone)]
struct CachedSource {
    source: SchedulableSource,
    schedule: Option<Schedule>,
    last_checked: Option<DateTime<Utc>>,
}

pub struct SchedulerService {
    ingestor: IngestorService,
    database: Database,
    run_missed_immediately: bool,
    cached_sources: Arc<RwLock<HashMap<Uuid, CachedSource>>>,
    last_cache_refresh: Arc<RwLock<DateTime<Utc>>>,
    cache_invalidation_rx: Option<CacheInvalidationReceiver>,
}

impl SchedulerService {
    pub fn new(
        state_manager: IngestionStateManager,
        database: Database,
        run_missed_immediately: bool,
        cache_invalidation_rx: Option<CacheInvalidationReceiver>,
    ) -> Self {
        let ingestor = IngestorService::new(state_manager.clone());
        Self {
            ingestor,
            database,
            run_missed_immediately,
            cached_sources: Arc::new(RwLock::new(HashMap::new())),
            last_cache_refresh: Arc::new(RwLock::new(Utc::now())),
            cache_invalidation_rx,
        }
    }

    pub async fn start(mut self) -> Result<()> {
        info!("Starting scheduler service");

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

        // Load both stream and EPG sources
        let stream_sources = self.database.list_stream_sources().await?;
        let epg_sources = self.database.list_epg_sources().await?;
        let now = Utc::now();

        let mut cache = self.cached_sources.write().await;
        cache.clear();

        // Process stream sources
        for source in stream_sources {
            if !source.is_active {
                continue;
            }

            let schedule = match Schedule::from_str(&source.update_cron) {
                Ok(s) => Some(s),
                Err(e) => {
                    warn!(
                        "Stream source '{}' has invalid cron expression '{}': {}",
                        source.name, source.update_cron, e
                    );
                    None
                }
            };

            cache.insert(
                source.id,
                CachedSource {
                    source: SchedulableSource::Stream(source),
                    schedule,
                    last_checked: Some(now),
                },
            );
        }

        // Process EPG sources
        for source in epg_sources {
            if !source.is_active {
                continue;
            }

            let schedule = match Schedule::from_str(&source.update_cron) {
                Ok(s) => Some(s),
                Err(e) => {
                    warn!(
                        "EPG source '{}' has invalid cron expression '{}': {}",
                        source.name, source.update_cron, e
                    );
                    None
                }
            };

            cache.insert(
                source.id,
                CachedSource {
                    source: SchedulableSource::Epg(source),
                    schedule,
                    last_checked: Some(now),
                },
            );
        }

        *self.last_cache_refresh.write().await = now;
        info!(
            "Cached {} active sources with schedules (stream + EPG)",
            cache.len()
        );
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
                        "Source '{}' (ID: {}, Type: {}) - Next scheduled update: {} (cron: {})",
                        source.name(),
                        source.id(),
                        source.source_type(),
                        next_time.format("%Y-%m-%d %H:%M:%S UTC"),
                        source.update_cron()
                    );

                    // Check if we missed a scheduled run
                    if self.run_missed_immediately && source.last_ingested_at().is_some() {
                        if let Some(last_ingested) = source.last_ingested_at() {
                            if let Some(should_have_run) = schedule.after(&last_ingested).next() {
                                if now >= should_have_run
                                    && now.signed_duration_since(should_have_run).num_seconds() > 5
                                {
                                    info!(
                                        "Source '{}' missed scheduled run at {} - will run immediately",
                                        source.name(), should_have_run.format("%Y-%m-%d %H:%M:%S UTC")
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
                            source.name(),
                            e
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

    async fn process_source_update(&self, source: SchedulableSource) {
        match &source {
            SchedulableSource::Stream(stream_source) => {
                self.process_stream_source_update(stream_source).await;

                // If this stream source has a linked EPG source, refresh it too
                if let Ok(Some(linked_epg)) = self
                    .database
                    .find_linked_epg_by_stream_id(stream_source.id)
                    .await
                {
                    info!(
                        "Refreshing linked EPG source '{}' after stream source '{}' update",
                        linked_epg.name, stream_source.name
                    );
                    self.process_epg_source_update(&linked_epg).await;
                }
            }
            SchedulableSource::Epg(epg_source) => {
                self.process_epg_source_update(epg_source).await;

                // If this EPG source has a linked stream source, refresh it too
                if let Ok(Some(linked_stream)) = self
                    .database
                    .find_linked_stream_by_epg_id(epg_source.id)
                    .await
                {
                    info!(
                        "Refreshing linked stream source '{}' after EPG source '{}' update",
                        linked_stream.name, epg_source.name
                    );
                    self.process_stream_source_update(&linked_stream).await;
                }
            }
        }
    }

    async fn process_stream_source_update(&self, source: &StreamSource) {
        // Use the IngestorService orchestrator to ensure identical behavior
        let success = match self
            .ingestor
            .refresh_stream_source(self.database.clone(), source, ProcessingTrigger::Scheduler)
            .await
        {
            Ok(_channel_count) => {
                // Update cached source data - the timestamp was already updated by the shared function
                {
                    let mut cache = self.cached_sources.write().await;
                    if let Some(cached_source) = cache.get_mut(&source.id) {
                        if let SchedulableSource::Stream(ref mut stream_src) =
                            &mut cached_source.source
                        {
                            // Refresh the cached timestamp from database
                            if let Ok(Some(updated_source)) =
                                self.database.get_stream_source(source.id).await
                            {
                                stream_src.last_ingested_at = updated_source.last_ingested_at;
                                debug!(
                                    "Updated cached last_ingested_at for stream source '{}' to {:?}",
                                    source.name, updated_source.last_ingested_at
                                );
                            }
                        }
                    }
                }

                // Calculate next update time
                if let Ok(schedule) = Schedule::from_str(&source.update_cron) {
                    if let Some(next_time) = schedule.upcoming(Utc).next() {
                        info!(
                            "Scheduled refresh completed for stream source '{}' - Next update: {}",
                            source.name,
                            next_time.format("%Y-%m-%d %H:%M:%S UTC")
                        );
                    }
                }
                true
            }
            Err(e) => {
                error!(
                    "Scheduled stream source refresh failed for source '{}': {}",
                    source.name, e
                );
                false
            }
        };

        let _ = success;
    }

    async fn process_epg_source_update(&self, source: &EpgSource) {
        // Use the IngestorService orchestrator to ensure identical behavior
        let success = match self
            .ingestor
            .ingest_epg_source(self.database.clone(), source, ProcessingTrigger::Scheduler)
            .await
        {
            Ok((_channel_count, _program_count)) => {
                // Update cached source data - the timestamp was already updated by the shared function
                {
                    let mut cache = self.cached_sources.write().await;
                    if let Some(cached_source) = cache.get_mut(&source.id) {
                        if let SchedulableSource::Epg(ref mut epg_src) = &mut cached_source.source {
                            // Refresh the cached timestamp from database
                            if let Ok(Some(updated_source)) =
                                self.database.get_epg_source(source.id).await
                            {
                                epg_src.last_ingested_at = updated_source.last_ingested_at;
                                debug!(
                                    "Updated cached last_ingested_at for EPG source '{}' to {:?}",
                                    source.name, updated_source.last_ingested_at
                                );
                            }
                        }
                    }
                }

                // Calculate next update time
                if let Ok(schedule) = Schedule::from_str(&source.update_cron) {
                    if let Some(next_time) = schedule.upcoming(Utc).next() {
                        info!(
                            "Scheduled refresh completed for EPG source '{}' - Next update: {}",
                            source.name,
                            next_time.format("%Y-%m-%d %H:%M:%S UTC")
                        );
                    }
                }
                true
            }
            Err(e) => {
                error!(
                    "Scheduled EPG refresh failed for source '{}': {}",
                    source.name, e
                );
                false
            }
        };

        let _ = success;
    }

    fn should_update_cached(
        &self,
        source: &SchedulableSource,
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

        if let Some(last_ingested) = source.last_ingested_at() {
            // Find the next scheduled time after the last ingestion
            if let Some(next_time) = schedule.after(&last_ingested).next() {
                let should_run = now >= next_time;
                if should_run {
                    trace!(
                        "Source '{}' should update: last_ingested={}, next_time={}, now={}",
                        source.name(),
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
        if should_run_first_time && source.last_ingested_at().is_none() {
            trace!(
                "Source '{}' has never been ingested and schedule is active - should run",
                source.name()
            );
        }

        Ok(should_run_first_time && source.last_ingested_at().is_none())
    }
}
