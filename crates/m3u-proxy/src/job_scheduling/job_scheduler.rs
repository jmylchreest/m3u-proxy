//! Job scheduler service for cron-based job scheduling

use super::job_queue::JobQueue;
use super::types::{JobPriority, JobType, ScheduledJob};
use crate::database::Database;
use crate::database::repositories::{EpgSourceSeaOrmRepository, StreamSourceSeaOrmRepository};
use crate::models::{EpgSource, StreamSource};
use anyhow::Result;
use chrono::{DateTime, Utc};
use cron::Schedule;
use std::str::FromStr;
use std::sync::Arc;
use tokio::time::{Duration, interval};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Service responsible for evaluating cron schedules and enqueuing jobs
pub struct JobScheduler {
    job_queue: Arc<JobQueue>,
    stream_source_repo: StreamSourceSeaOrmRepository,
    epg_source_repo: EpgSourceSeaOrmRepository,
}

impl JobScheduler {
    /// Create a new job scheduler
    pub fn new(job_queue: Arc<JobQueue>, database: Database) -> Self {
        let connection = database.connection().clone();
        Self {
            job_queue,
            stream_source_repo: StreamSourceSeaOrmRepository::new(connection.clone()),
            epg_source_repo: EpgSourceSeaOrmRepository::new(connection),
        }
    }

    /// Run the job scheduler service
    pub async fn run(&self, cancellation_token: tokio_util::sync::CancellationToken) -> Result<()> {
        info!("Starting job scheduler service");
        let mut schedule_check = interval(Duration::from_secs(60)); // Check every minute

        // Skip the first immediate tick to avoid scheduling jobs right at startup
        schedule_check.tick().await;

        loop {
            tokio::select! {
                _ = schedule_check.tick() => {
                    if let Err(e) = self.schedule_due_jobs().await {
                        error!("Error scheduling due jobs: {}", e);
                    }
                }
                _ = cancellation_token.cancelled() => {
                    info!("Job scheduler received cancellation signal, shutting down");
                    break;
                }
            }
        }

        info!("Job scheduler service stopped");
        Ok(())
    }

    /// Check all sources and schedule jobs for those that are due
    async fn schedule_due_jobs(&self) -> Result<()> {
        let now = Utc::now();
        debug!(
            "Checking for jobs due at {}",
            now.format("%Y-%m-%d %H:%M:%S UTC")
        );

        // Process stream sources
        match self.stream_source_repo.find_all().await {
            Ok(sources) => {
                for source in sources {
                    if let Err(e) = self.check_and_schedule_stream_source(&source, now).await {
                        warn!("Failed to schedule stream source {}: {}", source.name, e);
                    }
                }
            }
            Err(e) => error!("Failed to fetch stream sources: {}", e),
        }

        // Process EPG sources
        match self.epg_source_repo.find_all().await {
            Ok(sources) => {
                for source in sources {
                    if let Err(e) = self.check_and_schedule_epg_source(&source, now).await {
                        warn!("Failed to schedule EPG source {}: {}", source.name, e);
                    }
                }
            }
            Err(e) => error!("Failed to fetch EPG sources: {}", e),
        }

        Ok(())
    }

    /// Check if a stream source needs scheduling and enqueue it
    async fn check_and_schedule_stream_source(
        &self,
        source: &StreamSource,
        now: DateTime<Utc>,
    ) -> Result<()> {
        debug!(
            "Evaluating stream source '{}' (active: {}, cron: '{}')",
            source.name, source.is_active, source.update_cron
        );

        if !source.is_active {
            debug!("Skipping inactive stream source '{}'", source.name);
            return Ok(()); // Skip inactive sources
        }

        if self.should_schedule_source(&source.update_cron, source.last_ingested_at, now)? {
            let job = ScheduledJob::new(JobType::StreamIngestion(source.id), JobPriority::Normal);

            match self.job_queue.enqueue(job).await {
                Ok(true) => {
                    info!(
                        "Scheduled stream source '{}' ({}) for ingestion",
                        source.name, source.id
                    );
                }
                Ok(false) => {
                    debug!(
                        "Stream source '{}' ({}) already scheduled, skipping",
                        source.name, source.id
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to enqueue stream source '{}' ({}): {}",
                        source.name, source.id, e
                    );
                }
            }
        }

        Ok(())
    }

    /// Check if an EPG source needs scheduling and enqueue it
    async fn check_and_schedule_epg_source(
        &self,
        source: &EpgSource,
        now: DateTime<Utc>,
    ) -> Result<()> {
        if !source.is_active {
            return Ok(()); // Skip inactive sources
        }

        if self.should_schedule_source(&source.update_cron, source.last_ingested_at, now)? {
            let job = ScheduledJob::new(JobType::EpgIngestion(source.id), JobPriority::Normal);

            match self.job_queue.enqueue(job).await {
                Ok(true) => {
                    info!(
                        "Scheduled EPG source '{}' ({}) for ingestion",
                        source.name, source.id
                    );
                }
                Ok(false) => {
                    debug!(
                        "EPG source '{}' ({}) already scheduled, skipping",
                        source.name, source.id
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to enqueue EPG source '{}' ({}): {}",
                        source.name, source.id, e
                    );
                }
            }
        }

        Ok(())
    }

    /// Determine if a source should be scheduled based on its cron expression
    fn should_schedule_source(
        &self,
        cron_expression: &str,
        last_ingested_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        let schedule = Schedule::from_str(cron_expression)
            .map_err(|e| anyhow::anyhow!("Invalid cron expression '{}': {}", cron_expression, e))?;

        match last_ingested_at {
            Some(last_ingested) => {
                // Find the next scheduled time after the last ingestion
                if let Some(next_time) = schedule.after(&last_ingested).next() {
                    let should_run = now >= next_time;
                    debug!(
                        "Evaluating source schedule: last_ingested={}, next_scheduled={}, now={}, should_run={}",
                        last_ingested.format("%Y-%m-%d %H:%M:%S UTC"),
                        next_time.format("%Y-%m-%d %H:%M:%S UTC"),
                        now.format("%Y-%m-%d %H:%M:%S UTC"),
                        should_run
                    );
                    Ok(should_run)
                } else {
                    // No future schedules
                    debug!("Source has no future schedules - should not run");
                    Ok(false)
                }
            }
            None => {
                // Never ingested - check if there's a valid upcoming schedule
                let has_schedule = schedule.upcoming(Utc).next().is_some();
                debug!(
                    "Source never ingested, has_schedule={} - should run now",
                    has_schedule
                );
                Ok(has_schedule)
            }
        }
    }

    /// Schedule proxy regeneration jobs (called after ingestion completes)
    pub async fn schedule_proxy_regenerations(&self, proxy_ids: Vec<Uuid>) -> Result<()> {
        let count = proxy_ids.len();

        // Schedule proxy regenerations 60 seconds from now to allow ingestion to settle
        let scheduled_time = Utc::now() + chrono::Duration::seconds(60);

        for proxy_id in proxy_ids {
            let job = ScheduledJob::new_scheduled(
                JobType::ProxyRegeneration(proxy_id),
                JobPriority::Low, // Lower priority than ingestion jobs
                scheduled_time,
            );

            match self.job_queue.enqueue(job).await {
                Ok(true) => {
                    debug!(
                        "Scheduled proxy regeneration for {} at {}",
                        proxy_id,
                        scheduled_time.format("%Y-%m-%d %H:%M:%S UTC")
                    );
                }
                Ok(false) => {
                    debug!("Proxy {} regeneration already scheduled", proxy_id);
                }
                Err(e) => {
                    warn!(
                        "Failed to schedule proxy regeneration for {}: {}",
                        proxy_id, e
                    );
                }
            }
        }

        if count > 0 {
            info!(
                "Scheduled {} proxy regenerations for {}",
                count,
                scheduled_time.format("%Y-%m-%d %H:%M:%S UTC")
            );
        }

        Ok(())
    }

    /// Trigger immediate source refresh (API method)
    pub async fn trigger_source_refresh(
        &self,
        source_id: Uuid,
        source_type: SourceType,
    ) -> Result<()> {
        let job_type = match source_type {
            SourceType::Stream => JobType::StreamIngestion(source_id),
            SourceType::EPG => JobType::EpgIngestion(source_id),
        };

        let job = ScheduledJob::new(job_type, JobPriority::High); // High priority for manual triggers

        match self.job_queue.enqueue(job).await {
            Ok(true) => {
                info!(
                    "Triggered immediate refresh for {:?} source {}",
                    source_type, source_id
                );
                Ok(())
            }
            Ok(false) => {
                info!("Source {} already scheduled for refresh", source_id);
                Ok(())
            }
            Err(e) => {
                error!("Failed to trigger source refresh: {}", e);
                Err(e.into())
            }
        }
    }

    /// Schedule a maintenance job
    pub async fn schedule_maintenance(
        &self,
        operation: String,
        priority: JobPriority,
    ) -> Result<()> {
        let job = ScheduledJob::new(JobType::Maintenance(operation.clone()), priority);

        match self.job_queue.enqueue(job).await {
            Ok(true) => {
                info!("Scheduled maintenance job: {}", operation);
                Ok(())
            }
            Ok(false) => {
                debug!("Maintenance job '{}' already scheduled", operation);
                Ok(())
            }
            Err(e) => {
                error!("Failed to schedule maintenance job '{}': {}", operation, e);
                Err(e.into())
            }
        }
    }

    /// Get queue statistics
    pub async fn get_queue_stats(&self) -> crate::job_scheduling::job_queue::JobQueueStats {
        self.job_queue.stats().await
    }
}

/// Source type for API calls
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    Stream,
    EPG,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    // Test imports will be added when integration tests are implemented

    // Helper function to test cron logic without needing full JobScheduler setup
    fn test_cron_scheduling(
        cron_expression: &str,
        last_ingested_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        use cron::Schedule;
        use std::str::FromStr;

        let schedule = Schedule::from_str(cron_expression)
            .map_err(|e| anyhow::anyhow!("Invalid cron expression '{}': {}", cron_expression, e))?;

        match last_ingested_at {
            Some(last_ingested) => {
                // Find the next scheduled time after the last ingestion
                if let Some(next_time) = schedule.after(&last_ingested).next() {
                    Ok(now >= next_time)
                } else {
                    Ok(false)
                }
            }
            None => {
                // Never ingested, should always run immediately
                Ok(true)
            }
        }
    }

    #[test]
    fn test_should_schedule_source_never_ingested() {
        let now = Utc::now();

        // Valid cron expression, never ingested - should schedule
        let result = test_cron_scheduling("0 0/15 * * * * *", None, now);
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Invalid cron expression
        let result = test_cron_scheduling("invalid", None, now);
        assert!(result.is_err());
    }

    #[test]
    fn test_should_schedule_source_with_last_ingestion() {
        let now = Utc::now();

        // Last ingested 20 minutes ago, 15-minute cron should trigger
        let last_ingested = now - Duration::minutes(20);
        let result = test_cron_scheduling("0 0/15 * * * * *", Some(last_ingested), now);
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Last ingested 5 minutes ago, 15-minute cron should not trigger
        let last_ingested = now - Duration::minutes(5);
        let result = test_cron_scheduling("0 0/15 * * * * *", Some(last_ingested), now);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    // TODO: Implement integration tests with proper mocking for:
    // - schedule_proxy_regenerations
    // - cron-based scheduling
    // - repository error handling
}
