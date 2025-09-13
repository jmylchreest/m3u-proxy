//! External API for job scheduling system
//! This module contains deprecated legacy APIs during migration

#![allow(deprecated)] // Allow deprecated warnings during migration

use super::job_scheduler::{JobScheduler, SourceType};
use super::types::JobPriority;
use anyhow::Result;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

/// External API for interacting with the job scheduling system
/// This provides a clean interface for other parts of the application
#[derive(Clone)]
pub struct JobSchedulingAPI {
    job_scheduler: Arc<JobScheduler>,
}

impl JobSchedulingAPI {
    /// Create a new job scheduling API
    pub fn new(job_scheduler: Arc<JobScheduler>) -> Self {
        Self { job_scheduler }
    }

    /// Trigger immediate refresh of a stream source
    pub async fn trigger_stream_source_refresh(&self, source_id: Uuid) -> Result<()> {
        info!(
            "API: Triggering immediate stream source refresh for {}",
            source_id
        );
        self.job_scheduler
            .trigger_source_refresh(source_id, SourceType::Stream)
            .await
    }

    /// Trigger immediate refresh of an EPG source
    pub async fn trigger_epg_source_refresh(&self, source_id: Uuid) -> Result<()> {
        info!(
            "API: Triggering immediate EPG source refresh for {}",
            source_id
        );
        self.job_scheduler
            .trigger_source_refresh(source_id, SourceType::EPG)
            .await
    }

    /// Schedule proxy regeneration jobs (called after ingestion completes)
    pub async fn schedule_proxy_regenerations(&self, proxy_ids: Vec<Uuid>) -> Result<()> {
        if !proxy_ids.is_empty() {
            info!(
                "API: Scheduling regeneration for {} proxies",
                proxy_ids.len()
            );
            self.job_scheduler
                .schedule_proxy_regenerations(proxy_ids)
                .await?;
        }
        Ok(())
    }

    /// Schedule a maintenance operation
    pub async fn schedule_maintenance(
        &self,
        operation: String,
        priority: JobPriority,
    ) -> Result<()> {
        info!("API: Scheduling maintenance operation: {}", operation);
        self.job_scheduler
            .schedule_maintenance(operation, priority)
            .await
    }

    /// Get current queue statistics
    pub async fn get_queue_stats(&self) -> crate::job_scheduling::job_queue::JobQueueStats {
        self.job_scheduler.get_queue_stats().await
    }

    /// Health check endpoint for the scheduling system
    pub async fn health_check(&self) -> SchedulingHealthStatus {
        let stats = self.get_queue_stats().await;

        SchedulingHealthStatus {
            is_healthy: true,
            pending_jobs: stats.pending_jobs,
            running_jobs: stats.running_jobs,
            total_tracked_keys: stats.total_tracked_keys,
        }
    }

    /// Compatibility method for old scheduler event system
    /// This allows existing code to continue working while we transition
    #[deprecated(note = "Use specific trigger methods instead")]
    pub async fn send_scheduler_event(&self, event: LegacySchedulerEvent) -> Result<()> {
        match event {
            LegacySchedulerEvent::SourceCreated(source_id, source_type) => {
                // For newly created sources, we don't immediately trigger refresh
                // The scheduler will pick them up on the next cycle
                info!(
                    "API: Source created {} ({:?}) - will be picked up by scheduler",
                    source_id, source_type
                );
                Ok(())
            }
            LegacySchedulerEvent::SourceUpdated(source_id, source_type) => {
                // Source updates don't automatically trigger refresh
                info!(
                    "API: Source updated {} ({:?}) - no immediate action",
                    source_id, source_type
                );
                Ok(())
            }
            LegacySchedulerEvent::ManualRefreshTriggered(source_id, source_type) => {
                match source_type {
                    LegacySourceType::Stream => self.trigger_stream_source_refresh(source_id).await,
                    LegacySourceType::EPG => self.trigger_epg_source_refresh(source_id).await,
                }
            }
            LegacySchedulerEvent::CacheInvalidation => {
                // No-op in new system (no cache to invalidate)
                info!("API: Cache invalidation ignored - no cache in new system");
                Ok(())
            }
        }
    }
}

/// Health status of the scheduling system
#[derive(Debug, Clone, serde::Serialize)]
pub struct SchedulingHealthStatus {
    pub is_healthy: bool,
    pub pending_jobs: usize,
    pub running_jobs: usize,
    pub total_tracked_keys: usize,
}

/// Legacy scheduler events for compatibility
#[deprecated(note = "Use JobSchedulingAPI methods directly")]
#[derive(Debug, Clone)]
pub enum LegacySchedulerEvent {
    SourceCreated(Uuid, LegacySourceType),
    SourceUpdated(Uuid, LegacySourceType),
    ManualRefreshTriggered(Uuid, LegacySourceType),
    CacheInvalidation,
}

/// Legacy source type for compatibility
#[deprecated(note = "Use job_scheduler::SourceType")]
#[derive(Debug, Clone, Copy)]
pub enum LegacySourceType {
    Stream,
    EPG,
}
