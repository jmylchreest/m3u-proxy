use anyhow::Result;
use chrono::{DateTime, Utc};
use cron::Schedule;
use std::str::FromStr;
use tokio::time::{interval, Duration};

use super::{IngestionStateManager, IngestorService};
use crate::models::*;

pub struct SchedulerService {
    ingestor: IngestorService,
}

impl SchedulerService {
    pub fn new(state_manager: IngestionStateManager) -> Self {
        Self {
            ingestor: IngestorService::new(state_manager),
        }
    }

    #[allow(dead_code)]
    pub async fn start(&self) -> Result<()> {
        let mut interval = interval(Duration::from_secs(60)); // Check every minute

        loop {
            interval.tick().await;
            // TODO: Get sources from database and check if they need updating
            // self.check_and_update_sources().await?;
        }
    }

    #[allow(dead_code)]
    async fn check_and_update_sources(&self) -> Result<()> {
        // TODO: Implement source checking and updating logic
        // 1. Get all active sources from database
        // 2. For each source, check if it's time to update based on cron schedule
        // 3. If yes, trigger ingestion
        Ok(())
    }

    #[allow(dead_code)]
    fn should_update(&self, source: &StreamSource, now: DateTime<Utc>) -> Result<bool> {
        let schedule = Schedule::from_str(&source.update_cron)?;

        if let Some(last_ingested) = source.last_ingested_at {
            // Find the next scheduled time after the last ingestion
            if let Some(next_time) = schedule.after(&last_ingested).next() {
                return Ok(now >= next_time);
            }
        }

        // If never ingested, check if it's time for the first run
        Ok(schedule.upcoming(Utc).next().is_some())
    }
}
