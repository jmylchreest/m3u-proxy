use anyhow::Result;
use chrono::{DateTime, Utc};
use cron::Schedule;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast, mpsc};
use tokio::time::{sleep_until, Instant};
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

use super::IngestorService;
use crate::database::Database;
use crate::models::*;
use crate::services::{ProxyRegenerationService, StreamSourceBusinessService, EpgSourceService};

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
    grace_period_until: Option<DateTime<Utc>>, // Prevent immediate scheduling after creation/manual refresh
}

#[derive(Debug, Clone)]
pub enum SchedulerEvent {
    SourceCreated(Uuid),
    SourceUpdated(Uuid),
    SourceDeleted(Uuid),
    ManualRefreshTriggered(Uuid),
    BackoffExpired(Uuid),
    CacheInvalidation,
    Shutdown,
}

pub struct SchedulerService {
    ingestor: IngestorService,
    stream_source_service: Arc<StreamSourceBusinessService>,
    epg_source_service: Arc<EpgSourceService>,
    database: Database,
    run_missed_immediately: bool,
    cached_sources: Arc<RwLock<HashMap<Uuid, CachedSource>>>,
    last_cache_refresh: Arc<RwLock<DateTime<Utc>>>,
    cache_invalidation_rx: Option<CacheInvalidationReceiver>,
    proxy_regeneration_service: Option<ProxyRegenerationService>,
    event_tx: mpsc::UnboundedSender<SchedulerEvent>,
    event_rx: Option<mpsc::UnboundedReceiver<SchedulerEvent>>,
    active_backoff_timers: Arc<RwLock<HashSet<Uuid>>>, // Track sources with active backoff timers
}

impl SchedulerService {
    pub fn new(
        progress_service: Arc<crate::services::progress_service::ProgressService>,
        database: Database,
        stream_source_service: Arc<StreamSourceBusinessService>,
        epg_source_service: Arc<EpgSourceService>,
        run_missed_immediately: bool,
        cache_invalidation_rx: Option<CacheInvalidationReceiver>,
        proxy_regeneration_service: Option<ProxyRegenerationService>,
    ) -> Self {
        let ingestor = IngestorService::new(progress_service);
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            ingestor,
            stream_source_service,
            epg_source_service,
            database,
            run_missed_immediately,
            cached_sources: Arc::new(RwLock::new(HashMap::new())),
            last_cache_refresh: Arc::new(RwLock::new(Utc::now())),
            cache_invalidation_rx,
            proxy_regeneration_service,
            event_tx,
            event_rx: Some(event_rx),
            active_backoff_timers: Arc::new(RwLock::new(HashSet::new())),
        }
    }
    
    // Public method to get event sender for external components
    pub fn get_event_sender(&self) -> mpsc::UnboundedSender<SchedulerEvent> {
        self.event_tx.clone()
    }

    pub async fn start(mut self) -> Result<()> {
        info!("Starting event-driven scheduler service");

        // Load initial cache from database
        if let Err(e) = self.refresh_cache().await {
            error!("Failed to load initial cache: {}", e);
            return Err(e);
        }

        // Log next execution times for all sources at startup and process missed runs
        let missed_sources = self.log_startup_schedule().await?;
        
        // Process any missed sources immediately
        if !missed_sources.is_empty() {
            info!("Processing {} missed source(s) immediately", missed_sources.len());
            for source in missed_sources {
                info!("Starting immediate ingestion for missed {} source: {}", source.source_type(), source.name());
                self.process_source_update(source).await;
            }
        }

        // Take the event receiver
        let mut event_rx = self.event_rx.take().expect("Event receiver should be available");

        loop {
            let next_wake_time = self.calculate_next_wake_time().await;
            trace!("Next scheduler wake time: {:?}", next_wake_time);

            tokio::select! {
                // Wake up for scheduled events based on intelligent calculation
                _ = sleep_until(next_wake_time) => {
                    trace!("Scheduler wake-up for scheduled events");
                    if let Err(e) = self.process_scheduled_events().await {
                        error!("Error processing scheduled events: {}", e);
                    }
                }
                
                // React to immediate events
                Some(event) = event_rx.recv() => {
                    trace!("Received scheduler event: {:?}", event);
                    if let Err(e) = self.handle_event(event).await {
                        error!("Error handling scheduler event: {}", e);
                    }
                }
                
                // Legacy cache invalidation support
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
                    grace_period_until: None,
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
                    grace_period_until: None,
                },
            );
        }

        *self.last_cache_refresh.write().await = now;
        debug!(
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

    async fn log_startup_schedule(&self) -> Result<Vec<SchedulableSource>> {
        let cache = self.cached_sources.read().await;
        let now = Utc::now();
        let mut missed_sources = Vec::new();

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
                                        "Source '{}' (ID: {}) missed scheduled run at {} - will run immediately",
                                        source.name(),
                                        source.id(),
                                        should_have_run.format("%Y-%m-%d %H:%M:%S UTC")
                                    );
                                    // Add to missed sources list for immediate processing
                                    missed_sources.push(source.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(missed_sources)
    }

    async fn check_and_update_sources_from_cache(&self) -> Result<()> {
        let now = Utc::now();
        let mut cache = self.cached_sources.write().await;

        // Collect sources that need updating to avoid holding the write lock during processing
        let mut sources_to_update = Vec::new();

        for (_source_id, cached_source) in cache.iter_mut() {
            let source = &cached_source.source;

            // Check if source is actively being processed or in backoff period
            if let Some(processing_info) = self
                .ingestor
                .get_state_manager()
                .get_processing_info(source.id())
                .await
            {
                // If next_retry_after is None, the source is actively being processed
                if processing_info.next_retry_after.is_none() {
                    trace!(
                        "Source '{}' is actively being processed (started at {}) - skipping scheduler run",
                        source.name(),
                        processing_info.started_at.format("%Y-%m-%d %H:%M:%S UTC")
                    );
                    continue; // Skip this source - actively processing
                }

                // Check if we're in backoff period
                if let Some(retry_after) = processing_info.next_retry_after {
                    if now < retry_after {
                        trace!(
                            "Source '{}' is still in backoff period until {}",
                            source.name(),
                            retry_after.format("%Y-%m-%d %H:%M:%S UTC")
                        );
                        continue; // Skip this source - still in backoff
                    } else {
                        trace!(
                            "Source '{}' backoff period has expired (was until {}), checking schedule",
                            source.name(),
                            retry_after.format("%Y-%m-%d %H:%M:%S UTC")
                        );
                    }
                }
            }

            if let Some(schedule) = &cached_source.schedule {
                match self.should_update_cached(
                    source,
                    schedule,
                    now,
                    &mut cached_source.last_checked,
                ) {
                    Ok(should_update) => {
                        if should_update {
                            trace!("Source '{}' will be processed", source.name());
                            sources_to_update.push(source.clone());
                        } else {
                            trace!("Source '{}' should not update (throttled)", source.name());
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
                // BUT only if it hasn't been updated recently (prevent cascading loops)
                if let Ok(Some(linked_epg)) = self
                    .database
                    .find_linked_epg_by_stream_id(stream_source.id)
                    .await
                {
                    // Check if linked EPG was updated recently (within last 5 minutes)
                    // Use scheduler cache to get more accurate timing
                    let should_refresh_linked = {
                        let cache = self.cached_sources.read().await;
                        if let Some(cached_epg) = cache.get(&linked_epg.id) {
                            if let Some(last_ingested) = cached_epg.source.last_ingested_at() {
                                let time_since_last = chrono::Utc::now().signed_duration_since(last_ingested);
                                time_since_last.num_minutes() >= 5
                            } else {
                                true // Never ingested, safe to refresh
                            }
                        } else {
                            true // Not in cache, safe to refresh
                        }
                    };
                    
                    if should_refresh_linked {
                        info!(
                            "Refreshing linked EPG source '{}' after stream source '{}' update",
                            linked_epg.name, stream_source.name
                        );
                        self.process_epg_source_update(&linked_epg).await;
                    } else {
                        info!(
                            "Skipping linked EPG source '{}' refresh - updated recently (within 5 minutes)",
                            linked_epg.name
                        );
                    }
                }
            }
            SchedulableSource::Epg(epg_source) => {
                self.process_epg_source_update(epg_source).await;

                // If this EPG source has a linked stream source, refresh it too
                // BUT only if it hasn't been updated recently (prevent cascading loops)
                if let Ok(Some(linked_stream)) = self
                    .database
                    .find_linked_stream_by_epg_id(epg_source.id)
                    .await
                {
                    // Check if linked stream was updated recently (within last 5 minutes)
                    // Use scheduler cache to get more accurate timing
                    let should_refresh_linked = {
                        let cache = self.cached_sources.read().await;
                        if let Some(cached_stream) = cache.get(&linked_stream.id) {
                            if let Some(last_ingested) = cached_stream.source.last_ingested_at() {
                                let time_since_last = chrono::Utc::now().signed_duration_since(last_ingested);
                                time_since_last.num_minutes() >= 5
                            } else {
                                true // Never ingested, safe to refresh
                            }
                        } else {
                            true // Not in cache, safe to refresh
                        }
                    };
                    
                    if should_refresh_linked {
                        info!(
                            "Refreshing linked stream source '{}' after EPG source '{}' update",
                            linked_stream.name, epg_source.name
                        );
                        self.process_stream_source_update(&linked_stream).await;
                    } else {
                        info!(
                            "Skipping linked stream source '{}' refresh - updated recently (within 5 minutes)",
                            linked_stream.name
                        );
                    }
                }
            }
        }
    }

    async fn process_stream_source_update(&self, source: &StreamSource) {
        // Create progress manager for this ingestion operation
        let progress_manager = match self.ingestor.progress_service.create_staged_progress_manager(
            source.id, // Use source ID as owner
            "stream_source".to_string(),
            crate::services::progress_service::OperationType::StreamIngestion,
            format!("Ingest Stream Source: {}", source.name),
        ).await {
            Ok(manager) => {
                // Add ingestion stage
                let manager_with_stage: std::sync::Arc<crate::services::progress_service::ProgressManager> = manager.add_stage("stream_ingestion", "Stream Ingestion").await;
                Some(manager_with_stage)
            },
            Err(e) => {
                warn!("Failed to create progress manager for stream source ingestion {}: {} - continuing without progress", source.name, e);
                None
            }
        };

        // Get progress updater if we have a progress manager
        let progress_updater = if let Some(ref manager) = progress_manager {
            manager.get_stage_updater("stream_ingestion").await
        } else {
            None
        };

        // Use StreamSourceService with progress tracking
        let success = match self
            .stream_source_service
            .refresh_with_progress_updater(source, progress_updater.as_ref())
            .await
        {
            Ok(_channel_count) => {
                // Update cached source data - the timestamp was already updated by the shared function
                {
                    let mut cache = self.cached_sources.write().await;
                    if let Some(cached_source) = cache.get_mut(&source.id) {
                        if let SchedulableSource::Stream(stream_src) = &mut cached_source.source {
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

        // Complete the progress operation
        if let Some(manager) = progress_manager {
            if success {
                manager.complete().await;
                info!("Completed stream source ingestion progress for {}", source.name);
            } else {
                manager.fail(&format!("Stream source ingestion failed for {}", source.name)).await;
                warn!("Failed stream source ingestion progress for {}", source.name);
            }
        }

        // Trigger proxy auto-regeneration after successful stream source update
        if success {
            if let Some(ref proxy_service) = self.proxy_regeneration_service {
                // COORDINATION FIX: Use coordinated method to prevent scheduler-manual conflicts
                proxy_service.queue_affected_proxies_coordinated(source.id, "stream").await;
            }
        }
    }

    async fn process_epg_source_update(&self, source: &EpgSource) {
        // Create progress manager for this ingestion operation
        let progress_manager = match self.ingestor.progress_service.create_staged_progress_manager(
            source.id, // Use source ID as owner
            "epg_source".to_string(),
            crate::services::progress_service::OperationType::EpgIngestion,
            format!("Ingest EPG Source: {}", source.name),
        ).await {
            Ok(manager) => {
                // Add ingestion stage
                let manager_with_stage: std::sync::Arc<crate::services::progress_service::ProgressManager> = manager.add_stage("epg_ingestion", "EPG Ingestion").await;
                Some(manager_with_stage)
            },
            Err(e) => {
                warn!("Failed to create progress manager for EPG source ingestion {}: {} - continuing without progress", source.name, e);
                None
            }
        };

        // Get progress updater if we have a progress manager
        let progress_updater = if let Some(ref manager) = progress_manager {
            manager.get_stage_updater("epg_ingestion").await
        } else {
            None
        };

        // Use EpgSourceService with progress tracking
        let success = match self
            .epg_source_service
            .ingest_programs_with_progress_updater(source, progress_updater.as_ref())
            .await
        {
            Ok(_program_count) => {
                // Update cached source data - the timestamp was already updated by the shared function
                {
                    let mut cache = self.cached_sources.write().await;
                    if let Some(cached_source) = cache.get_mut(&source.id) {
                        if let SchedulableSource::Epg(epg_src) = &mut cached_source.source {
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

        // Complete the progress operation
        if let Some(manager) = progress_manager {
            if success {
                manager.complete().await;
                info!("Completed EPG source ingestion progress for {}", source.name);
            } else {
                manager.fail(&format!("EPG source ingestion failed for {}", source.name)).await;
                warn!("Failed EPG source ingestion progress for {}", source.name);
            }
        }

        // Trigger proxy auto-regeneration after successful EPG source update
        if success {
            if let Some(ref proxy_service) = self.proxy_regeneration_service {
                // COORDINATION FIX: Use coordinated method to prevent scheduler-manual conflicts
                proxy_service.queue_affected_proxies_coordinated(source.id, "epg").await;
            }
        }
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
            // Grace period: don't re-trigger immediately after recent ingestion
            // This prevents immediate re-runs when an ingestion just completed
            let time_since_last_ingestion = now.signed_duration_since(last_ingested);
            if time_since_last_ingestion.num_minutes() < 5 {
                trace!(
                    "Source '{}' was ingested recently ({} minutes ago) - skipping to prevent immediate re-trigger",
                    source.name(),
                    time_since_last_ingestion.num_minutes()
                );
                return Ok(false);
            }

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

    // New event-driven scheduler methods

    /// Calculate the next time the scheduler should wake up based on upcoming schedules and backoffs
    async fn calculate_next_wake_time(&self) -> Instant {
        let now = Utc::now();
        let cache = self.cached_sources.read().await;
        
        let mut upcoming_times = Vec::new();
        
        for (source_id, cached_source) in cache.iter() {
            // Skip sources in grace period
            if let Some(grace_until) = cached_source.grace_period_until {
                if now < grace_until {
                    trace!("Source '{}' in grace period until {}", cached_source.source.name(), grace_until);
                    continue;
                }
            }
            
            // Check if source is in backoff period
            if let Some(processing_info) = self.ingestor.get_state_manager().get_processing_info(*source_id).await {
                if let Some(retry_after) = processing_info.next_retry_after {
                    if retry_after > now {
                        // Source in backoff - add backoff expiry time
                        upcoming_times.push(retry_after);
                        continue;
                    } else {
                        // Backoff expired - check if we have a timer active to avoid spam
                        let timers = self.active_backoff_timers.read().await;
                        if timers.contains(source_id) {
                            continue; // Timer already scheduled for this source
                        }
                        
                        // Add immediate processing
                        upcoming_times.push(now);
                        continue;
                    }
                }
                
                // Source actively processing - skip
                if processing_info.next_retry_after.is_none() {
                    continue;
                }
            }
            
            // Check cron schedule
            if let Some(schedule) = &cached_source.schedule {
                if let Some(next_time) = schedule.upcoming(Utc).next() {
                    // Apply grace period check
                    if let Some(last_ingested) = cached_source.source.last_ingested_at() {
                        let time_since_last = now.signed_duration_since(last_ingested);
                        if time_since_last.num_minutes() < 5 {
                            // Still in grace period - skip this schedule
                            continue;
                        }
                    }
                    
                    upcoming_times.push(next_time);
                }
            }
        }
        
        // Find the earliest upcoming time
        let next_wake = upcoming_times.into_iter().min()
            .unwrap_or_else(|| now + chrono::Duration::minutes(5)); // Default to 5 minutes if no schedules
        
        // Convert to tokio Instant with reasonable bounds
        let sleep_duration = next_wake.signed_duration_since(now)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(60))
            .max(std::time::Duration::from_secs(1))     // Minimum 1 second
            .min(std::time::Duration::from_secs(300));  // Maximum 5 minutes
        
        Instant::now() + sleep_duration
    }

    /// Process events that are scheduled (cron-based or backoff expiry)
    async fn process_scheduled_events(&mut self) -> Result<()> {
        
        // Use the existing check_and_update_sources_from_cache logic but with better filtering
        if let Err(e) = self.check_and_update_sources_from_cache().await {
            error!("Error checking scheduled sources: {}", e);
        }
        
        // Check if we need to refresh cache (every 5 minutes)
        if self.should_refresh_cache().await {
            debug!("Refreshing scheduler cache (periodic refresh)");
            if let Err(e) = self.refresh_cache().await {
                error!("Failed to refresh cache: {}", e);
            }
        }
        
        Ok(())
    }

    /// Handle immediate events (CRUD operations, manual triggers, etc.)
    async fn handle_event(&mut self, event: SchedulerEvent) -> Result<()> {
        match event {
            SchedulerEvent::SourceCreated(source_id) => {
                info!("Handling source creation event for {}", source_id);
                // Refresh cache to include new source
                self.refresh_cache().await?;
                
                // Set grace period to prevent immediate scheduling
                self.set_source_grace_period(source_id, chrono::Duration::minutes(5)).await;
            }
            
            SchedulerEvent::SourceUpdated(source_id) => {
                info!("Handling source update event for {}", source_id);
                // Refresh specific source in cache
                self.refresh_single_source_in_cache(source_id).await?;
            }
            
            SchedulerEvent::SourceDeleted(source_id) => {
                info!("Handling source deletion event for {}", source_id);
                // Remove from cache
                let mut cache = self.cached_sources.write().await;
                cache.remove(&source_id);
                
                // Clean up any active timers
                let mut timers = self.active_backoff_timers.write().await;
                timers.remove(&source_id);
            }
            
            SchedulerEvent::ManualRefreshTriggered(source_id) => {
                info!("Handling manual refresh trigger for {}", source_id);
                // Set grace period to prevent immediate re-scheduling after manual run
                self.set_source_grace_period(source_id, chrono::Duration::minutes(5)).await;
                
                // Clear any backoff timers since manual refresh overrides backoff
                let mut timers = self.active_backoff_timers.write().await;
                timers.remove(&source_id);
            }
            
            SchedulerEvent::BackoffExpired(source_id) => {
                trace!("Handling backoff expiry for {}", source_id);
                // Remove from active timers
                let mut timers = self.active_backoff_timers.write().await;
                timers.remove(&source_id);
                
                // The source will be picked up in the next scheduled check
            }
            
            SchedulerEvent::CacheInvalidation => {
                info!("Handling cache invalidation event");
                self.refresh_cache().await?;
            }
            
            SchedulerEvent::Shutdown => {
                info!("Handling shutdown event");
                return Err(anyhow::anyhow!("Scheduler shutdown requested"));
            }
        }
        
        Ok(())
    }

    /// Set a grace period for a source to prevent immediate scheduling
    async fn set_source_grace_period(&self, source_id: Uuid, duration: chrono::Duration) {
        let grace_until = Utc::now() + duration;
        let mut cache = self.cached_sources.write().await;
        
        if let Some(cached_source) = cache.get_mut(&source_id) {
            cached_source.grace_period_until = Some(grace_until);
            info!("Set grace period for source '{}' until {}", cached_source.source.name(), grace_until);
        }
    }

    /// Refresh a single source in the cache
    async fn refresh_single_source_in_cache(&self, source_id: Uuid) -> Result<()> {
        // Fetch updated source from database
        if let Ok(Some(stream_source)) = self.database.get_stream_source(source_id).await {
            let schedulable_source = SchedulableSource::Stream(stream_source);
            let source_name = schedulable_source.name().to_string(); // Store name before move
            
            // Parse schedule if available
            let schedule = Schedule::from_str(schedulable_source.update_cron()).ok();
            
            // Update cache
            let mut cache = self.cached_sources.write().await;
            if let Some(cached_source) = cache.get_mut(&source_id) {
                cached_source.source = schedulable_source;
                cached_source.schedule = schedule;
                // Preserve grace period and last_checked
            } else {
                // Source not in cache - add it
                cache.insert(source_id, CachedSource {
                    source: schedulable_source,
                    schedule,
                    last_checked: None,
                    grace_period_until: None,
                });
            }
            
            info!("Refreshed source '{}' in cache", source_name);
        } else if let Ok(Some(epg_source)) = self.database.get_epg_source(source_id).await {
            let schedulable_source = SchedulableSource::Epg(epg_source);
            let source_name = schedulable_source.name().to_string(); // Store name before move
            
            // Parse schedule if available
            let schedule = Schedule::from_str(schedulable_source.update_cron()).ok();
            
            // Update cache
            let mut cache = self.cached_sources.write().await;
            if let Some(cached_source) = cache.get_mut(&source_id) {
                cached_source.source = schedulable_source;
                cached_source.schedule = schedule;
                // Preserve grace period and last_checked
            } else {
                // Source not in cache - add it
                cache.insert(source_id, CachedSource {
                    source: schedulable_source,
                    schedule,
                    last_checked: None,
                    grace_period_until: None,
                });
            }
            
            info!("Refreshed EPG source '{}' in cache", source_name);
        } else {
            warn!("Source {} not found in database during cache refresh", source_id);
        }
        
        Ok(())
    }

    /// Schedule a backoff expiry timer
    pub async fn schedule_backoff_expiry(&self, source_id: Uuid, retry_after: DateTime<Utc>) {
        let now = Utc::now();
        
        if retry_after <= now {
            // Already expired - send immediate event
            let _ = self.event_tx.send(SchedulerEvent::BackoffExpired(source_id));
            return;
        }
        
        // Add to active timers to prevent duplicate scheduling
        {
            let mut timers = self.active_backoff_timers.write().await;
            timers.insert(source_id);
        }
        
        let delay = retry_after.signed_duration_since(now)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(60));
        
        let event_tx = self.event_tx.clone();
        let timers = self.active_backoff_timers.clone();
        
        // Spawn timer task
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            
            // Remove from active timers
            {
                let mut active_timers = timers.write().await;
                active_timers.remove(&source_id);
            }
            
            // Send expiry event
            let _ = event_tx.send(SchedulerEvent::BackoffExpired(source_id));
        });
        
        trace!("Scheduled backoff expiry timer for source {} at {}", source_id, retry_after);
    }

    /// Get scheduler health information for the health endpoint
    pub async fn get_health_info(&self) -> crate::web::responses::SchedulerHealth {
        let cached_sources = self.cached_sources.read().await;
        let last_cache_refresh = *self.last_cache_refresh.read().await;
        
        // Count sources by type
        let mut stream_sources = 0u32;
        let mut epg_sources = 0u32;
        let mut next_scheduled_times = Vec::new();
        
        for cached_source in cached_sources.values() {
            match &cached_source.source {
                SchedulableSource::Stream(_) => stream_sources += 1,
                SchedulableSource::Epg(_) => epg_sources += 1,
            }
            
            // Get next scheduled time if schedule is available
            if let Some(ref schedule) = cached_source.schedule {
                if let Some(next_time) = schedule.upcoming(Utc).next() {
                    next_scheduled_times.push(crate::web::responses::NextScheduledTime {
                        source_id: cached_source.source.id(),
                        source_name: cached_source.source.name().to_string(),
                        source_type: cached_source.source.source_type().to_string(),
                        next_run: next_time,
                        cron_expression: cached_source.source.update_cron().to_string(),
                    });
                }
            }
        }
        
        // Sort by next run time
        next_scheduled_times.sort_by(|a, b| a.next_run.cmp(&b.next_run));
        
        // Get active ingestions count - simplified for now
        // TODO: Get actual active ingestion count from progress service or state manager
        let active_ingestions = 0u32;
        
        crate::web::responses::SchedulerHealth {
            status: "running".to_string(),
            sources_scheduled: crate::web::responses::ScheduledSourceCounts {
                stream_sources,
                epg_sources,
            },
            next_scheduled_times,
            last_cache_refresh,
            active_ingestions,
        }
    }
}
