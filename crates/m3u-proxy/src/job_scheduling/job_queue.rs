//! Job queue implementation with deduplication and priority ordering

use super::types::{JobSchedulingError, ScheduledJob};
use chrono::{DateTime, Utc};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Thread-safe job queue with deduplication and priority ordering
#[derive(Debug)]
pub struct JobQueue {
    /// Pending jobs ordered by priority and time (min-heap using Reverse)
    pending: Arc<RwLock<BinaryHeap<Reverse<ScheduledJob>>>>,
    /// Currently running jobs (job_id -> job_key mapping)
    running: Arc<RwLock<HashMap<Uuid, String>>>,
    /// Active job keys for deduplication (both pending and running)
    job_keys: Arc<RwLock<HashSet<String>>>,
}

impl JobQueue {
    /// Create a new empty job queue
    pub fn new() -> Self {
        Self {
            pending: Arc::new(RwLock::new(BinaryHeap::new())),
            running: Arc::new(RwLock::new(HashMap::new())),
            job_keys: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Enqueue a job if it doesn't already exist
    /// Returns Ok(true) if job was enqueued, Ok(false) if duplicate was skipped
    pub async fn enqueue(&self, job: ScheduledJob) -> Result<bool, JobSchedulingError> {
        let job_key = job.job_key();
        let mut job_keys = self.job_keys.write().await;

        // Check for duplicate job
        if job_keys.contains(&job_key) {
            debug!("Skipping duplicate job for key: {}", job_key);
            return Ok(false);
        }

        // Add to tracking and pending queue
        job_keys.insert(job_key.clone());
        drop(job_keys);

        let mut pending = self.pending.write().await;
        pending.push(Reverse(job.clone()));

        info!(
            "Enqueued job {} (type: {:?}, priority: {:?}, scheduled: {})",
            job_key,
            job.job_type,
            job.priority,
            job.scheduled_time.format("%Y-%m-%d %H:%M:%S UTC")
        );

        Ok(true)
    }

    /// Get ready jobs up to the specified limit
    pub async fn get_ready_jobs(&self, now: DateTime<Utc>, limit: usize) -> Vec<ScheduledJob> {
        let mut pending = self.pending.write().await;
        let mut ready_jobs = Vec::new();

        // Extract ready jobs from the heap
        let mut remaining_jobs = BinaryHeap::new();

        while let Some(Reverse(job)) = pending.pop() {
            if job.is_ready(now) && ready_jobs.len() < limit {
                ready_jobs.push(job);
            } else {
                remaining_jobs.push(Reverse(job));
            }
        }

        // Put back jobs that weren't ready or exceeded limit
        *pending = remaining_jobs;

        if !ready_jobs.is_empty() {
            debug!("Retrieved {} ready jobs from queue", ready_jobs.len());
        }

        ready_jobs
    }

    /// Get jobs that can be executed considering both time readiness and concurrency limits
    pub async fn get_executable_jobs(
        &self,
        now: DateTime<Utc>,
        available_slots: usize,
        current_type_counts: &std::collections::HashMap<
            super::job_queue_runner::JobTypeCategory,
            usize,
        >,
        type_limits: &std::collections::HashMap<super::job_queue_runner::JobTypeCategory, usize>,
    ) -> Vec<ScheduledJob> {
        let mut pending = self.pending.write().await;
        let mut executable_jobs = Vec::new();
        let mut remaining_jobs = BinaryHeap::new();
        let mut local_type_counts = current_type_counts.clone();

        // Extract jobs from the heap and determine which can be executed
        while let Some(Reverse(job)) = pending.pop() {
            if job.is_ready(now) && executable_jobs.len() < available_slots {
                let job_category = super::job_queue_runner::JobTypeCategory::from(&job.job_type);
                let current_count = local_type_counts.get(&job_category).unwrap_or(&0);
                let type_limit = type_limits.get(&job_category).unwrap_or(&1);

                if current_count < type_limit {
                    // Can execute this job
                    executable_jobs.push(job);
                    *local_type_counts.entry(job_category).or_insert(0) += 1;
                } else {
                    // At concurrency limit for this type, put it back
                    remaining_jobs.push(Reverse(job));
                }
            } else {
                remaining_jobs.push(Reverse(job));
            }
        }

        // Put back jobs that couldn't be executed
        *pending = remaining_jobs;

        if !executable_jobs.is_empty() {
            debug!(
                "Retrieved {} executable jobs from queue",
                executable_jobs.len()
            );
        }

        executable_jobs
    }

    /// Mark a job as running
    pub async fn mark_running(&self, job_id: Uuid, job_key: String) {
        let mut running = self.running.write().await;
        running.insert(job_id, job_key.clone());

        debug!("Marked job {} as running", job_key);
    }

    /// Mark a job as completed and remove from tracking
    pub async fn mark_completed(&self, job_id: Uuid) {
        let mut running = self.running.write().await;

        if let Some(job_key) = running.remove(&job_id) {
            drop(running);

            let mut job_keys = self.job_keys.write().await;
            job_keys.remove(&job_key);

            debug!("Job {} completed and removed from tracking", job_key);
        } else {
            warn!("Attempted to mark unknown job {} as completed", job_id);
        }
    }

    /// Get the number of currently running jobs
    pub async fn running_count(&self) -> usize {
        self.running.read().await.len()
    }

    /// Get the number of pending jobs
    pub async fn pending_count(&self) -> usize {
        self.pending.read().await.len()
    }

    /// Get queue statistics
    pub async fn stats(&self) -> JobQueueStats {
        let pending = self.pending.read().await;
        let running = self.running.read().await;

        JobQueueStats {
            pending_jobs: pending.len(),
            running_jobs: running.len(),
            total_tracked_keys: self.job_keys.read().await.len(),
        }
    }

    /// Check if a specific job key is already tracked (pending or running)
    pub async fn contains_job_key(&self, job_key: &str) -> bool {
        self.job_keys.read().await.contains(job_key)
    }

    /// Get all running job keys for debugging
    pub async fn get_running_job_keys(&self) -> Vec<String> {
        self.running.read().await.values().cloned().collect()
    }

    /// Clear all jobs (for testing)
    #[cfg(test)]
    pub async fn clear(&self) {
        let mut pending = self.pending.write().await;
        let mut running = self.running.write().await;
        let mut job_keys = self.job_keys.write().await;

        pending.clear();
        running.clear();
        job_keys.clear();
    }
}

impl Default for JobQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the job queue state
#[derive(Debug, Clone)]
pub struct JobQueueStats {
    /// Number of jobs waiting to be executed
    pub pending_jobs: usize,
    /// Number of jobs currently being executed
    pub running_jobs: usize,
    /// Total number of tracked job keys (should equal pending + running)
    pub total_tracked_keys: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job_scheduling::types::{JobPriority, JobType};
    use chrono::Duration;
    use tokio;

    #[tokio::test]
    async fn test_job_queue_enqueue_and_deduplication() {
        let queue = JobQueue::new();
        let source_id = Uuid::new_v4();

        let job1 = ScheduledJob::new(JobType::StreamIngestion(source_id), JobPriority::Normal);

        let job2 = ScheduledJob::new(
            JobType::StreamIngestion(source_id), // Same source - should deduplicate
            JobPriority::High,
        );

        // First job should enqueue successfully
        let result1 = queue.enqueue(job1).await.unwrap();
        assert!(result1);

        // Second job should be deduplicated
        let result2 = queue.enqueue(job2).await.unwrap();
        assert!(!result2);

        let stats = queue.stats().await;
        assert_eq!(stats.pending_jobs, 1);
        assert_eq!(stats.total_tracked_keys, 1);
    }

    #[tokio::test]
    async fn test_job_queue_priority_ordering() {
        let queue = JobQueue::new();
        let now = Utc::now();

        // Add jobs in reverse priority order
        let maintenance_job = ScheduledJob::new_scheduled(
            JobType::Maintenance("test".to_string()),
            JobPriority::Maintenance,
            now,
        );

        let critical_job = ScheduledJob::new_scheduled(
            JobType::StreamIngestion(Uuid::new_v4()),
            JobPriority::Critical,
            now,
        );

        let normal_job = ScheduledJob::new_scheduled(
            JobType::EpgIngestion(Uuid::new_v4()),
            JobPriority::Normal,
            now,
        );

        queue.enqueue(maintenance_job).await.unwrap();
        queue.enqueue(critical_job.clone()).await.unwrap();
        queue.enqueue(normal_job).await.unwrap();

        // Should get jobs in priority order
        let ready_jobs = queue.get_ready_jobs(now, 10).await;
        assert_eq!(ready_jobs.len(), 3);

        // First job should be critical priority
        assert_eq!(ready_jobs[0].priority, JobPriority::Critical);
        assert_eq!(ready_jobs[0].id, critical_job.id);
    }

    #[tokio::test]
    async fn test_job_queue_ready_jobs_filtering() {
        let queue = JobQueue::new();
        let now = Utc::now();

        let ready_job = ScheduledJob::new_scheduled(
            JobType::StreamIngestion(Uuid::new_v4()),
            JobPriority::Normal,
            now - Duration::minutes(1), // Ready now
        );

        let future_job = ScheduledJob::new_scheduled(
            JobType::EpgIngestion(Uuid::new_v4()),
            JobPriority::Normal,
            now + Duration::minutes(10), // Not ready yet
        );

        queue.enqueue(ready_job.clone()).await.unwrap();
        queue.enqueue(future_job).await.unwrap();

        let ready_jobs = queue.get_ready_jobs(now, 10).await;
        assert_eq!(ready_jobs.len(), 1);
        assert_eq!(ready_jobs[0].id, ready_job.id);

        // Future job should still be in pending
        let stats = queue.stats().await;
        assert_eq!(stats.pending_jobs, 1);
    }

    #[tokio::test]
    async fn test_job_queue_running_lifecycle() {
        let queue = JobQueue::new();
        let job = ScheduledJob::new(
            JobType::StreamIngestion(Uuid::new_v4()),
            JobPriority::Normal,
        );
        let job_key = job.job_key();
        let job_id = job.id;

        // Enqueue and get the job
        queue.enqueue(job).await.unwrap();
        let ready_jobs = queue.get_ready_jobs(Utc::now(), 1).await;
        assert_eq!(ready_jobs.len(), 1);

        // Mark as running
        queue.mark_running(job_id, job_key.clone()).await;
        assert_eq!(queue.running_count().await, 1);

        // Job key should still be tracked (prevents duplicates)
        assert!(queue.contains_job_key(&job_key).await);

        // Mark as completed
        queue.mark_completed(job_id).await;
        assert_eq!(queue.running_count().await, 0);

        // Job key should no longer be tracked
        assert!(!queue.contains_job_key(&job_key).await);
    }

    #[tokio::test]
    async fn test_job_queue_limit_ready_jobs() {
        let queue = JobQueue::new();
        let now = Utc::now();

        // Add 5 ready jobs
        for _i in 0..5 {
            let job = ScheduledJob::new_scheduled(
                JobType::StreamIngestion(Uuid::new_v4()),
                JobPriority::Normal,
                now,
            );
            queue.enqueue(job).await.unwrap();
        }

        // Request only 3 jobs
        let ready_jobs = queue.get_ready_jobs(now, 3).await;
        assert_eq!(ready_jobs.len(), 3);

        // 2 jobs should remain pending
        let stats = queue.stats().await;
        assert_eq!(stats.pending_jobs, 2);
    }
}
