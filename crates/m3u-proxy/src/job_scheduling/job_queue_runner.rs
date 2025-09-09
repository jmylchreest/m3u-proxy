//! Job queue runner service for executing scheduled jobs

use super::job_executor::JobExecutor;
use super::job_queue::JobQueue;
use super::job_scheduler::JobScheduler;
use super::types::{JobType, ScheduledJob};
use crate::config::JobSchedulingConfig;
use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::RwLock as TokioRwLock;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

/// Service responsible for executing jobs from the queue
pub struct JobQueueRunner {
    job_queue: Arc<JobQueue>,
    job_executor: Arc<JobExecutor>,
    job_scheduler: Arc<JobScheduler>, // For scheduling follow-up jobs
    max_concurrent: Arc<AtomicUsize>,
    concurrent_limits: Arc<TokioRwLock<HashMap<JobTypeCategory, usize>>>,
}

/// Category of job types for concurrency limiting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobTypeCategory {
    StreamIngestion,
    EpgIngestion, 
    ProxyRegeneration,
    Maintenance,
}

impl From<&JobType> for JobTypeCategory {
    fn from(job_type: &JobType) -> Self {
        match job_type {
            JobType::StreamIngestion(_) => JobTypeCategory::StreamIngestion,
            JobType::EpgIngestion(_) => JobTypeCategory::EpgIngestion,
            JobType::ProxyRegeneration(_) => JobTypeCategory::ProxyRegeneration,
            JobType::Maintenance(_) => JobTypeCategory::Maintenance,
        }
    }
}

impl JobQueueRunner {
    /// Create a new job queue runner with configuration
    pub fn new(
        job_queue: Arc<JobQueue>,
        job_executor: Arc<JobExecutor>,
        job_scheduler: Arc<JobScheduler>,
        config: &JobSchedulingConfig,
    ) -> Self {
        let mut concurrent_limits = HashMap::new();
        
        // Configure limits based on provided configuration
        concurrent_limits.insert(JobTypeCategory::StreamIngestion, config.stream_ingestion_limit);
        concurrent_limits.insert(JobTypeCategory::EpgIngestion, config.epg_ingestion_limit);
        concurrent_limits.insert(JobTypeCategory::ProxyRegeneration, config.proxy_regeneration_limit);
        concurrent_limits.insert(JobTypeCategory::Maintenance, config.maintenance_limit);

        Self {
            job_queue,
            job_executor,
            job_scheduler,
            max_concurrent: Arc::new(AtomicUsize::new(config.global_max_jobs)),
            concurrent_limits: Arc::new(TokioRwLock::new(concurrent_limits)),
        }
    }

    /// Run the job queue runner service
    pub async fn run(&self, cancellation_token: tokio_util::sync::CancellationToken) -> Result<()> {
        info!("Starting job queue runner service (max concurrent: {})", self.max_concurrent.load(Ordering::Relaxed));
        let mut execution_check = interval(Duration::from_secs(5)); // Check queue every 5 seconds

        loop {
            tokio::select! {
                _ = execution_check.tick() => {
                    if let Err(e) = self.process_pending_jobs().await {
                        error!("Error processing pending jobs: {}", e);
                    }
                }
                _ = cancellation_token.cancelled() => {
                    info!("Job queue runner received cancellation signal");
                    self.wait_for_running_jobs_to_complete().await;
                    break;
                }
            }
        }

        info!("Job queue runner service stopped");
        Ok(())
    }

    /// Process jobs that are ready to run
    async fn process_pending_jobs(&self) -> Result<()> {
        let now = Utc::now();
        let current_running = self.job_queue.running_count().await;

        let max_concurrent = self.max_concurrent.load(Ordering::Relaxed);
        
        // Don't exceed global concurrent limit
        if current_running >= max_concurrent {
            debug!("At maximum concurrent jobs ({}), waiting", max_concurrent);
            return Ok(());
        }

        let available_slots = max_concurrent - current_running;
        
        // Get running job keys to check per-type limits
        let running_job_keys = self.job_queue.get_running_job_keys().await;
        let type_counts = self.count_running_jobs_by_type(&running_job_keys);
        
        // Get current limits (read lock)
        let concurrent_limits = self.concurrent_limits.read().await;
        
        // Get jobs that can actually be executed based on type limits
        let jobs_to_execute = self.job_queue.get_executable_jobs(now, available_slots, &type_counts, &concurrent_limits).await;
        
        drop(concurrent_limits); // Release lock early

        if jobs_to_execute.is_empty() {
            return Ok(());
        }

        debug!("Found {} jobs ready for execution", jobs_to_execute.len());

        // Execute the jobs
        for job in jobs_to_execute {
            self.execute_job_async(job).await;
        }

        Ok(())
    }

    /// Execute a job asynchronously
    async fn execute_job_async(&self, job: ScheduledJob) {
        let job_key = job.job_key();
        let job_id = job.id;
        
        // Mark job as running
        self.job_queue.mark_running(job_id, job_key.clone()).await;
        
        info!("Starting execution of job: {} (priority: {:?})", job_key, job.priority);

        // Spawn the job execution
        let job_queue = self.job_queue.clone();
        let job_executor = self.job_executor.clone();
        let job_scheduler = self.job_scheduler.clone();
        
        tokio::spawn(async move {
            let start_time = std::time::Instant::now();
            let result = Self::execute_job(job, job_executor, job_scheduler).await;
            let duration = start_time.elapsed();

            // Always mark job as completed, regardless of success/failure
            job_queue.mark_completed(job_id).await;

            match result {
                Ok(()) => {
                    info!("Job {} completed successfully in {:?}", job_key, duration);
                }
                Err(e) => {
                    error!("Job {} failed after {:?}: {}", job_key, duration, e);
                }
            }
        });
    }

    /// Execute a single job
    async fn execute_job(
        job: ScheduledJob,
        job_executor: Arc<JobExecutor>,
        job_scheduler: Arc<JobScheduler>,
    ) -> Result<()> {
        match job.job_type {
            JobType::StreamIngestion(source_id) => {
                let affected_proxies = job_executor.execute_stream_job(source_id).await?;
                
                // Schedule proxy regenerations for affected proxies
                if !affected_proxies.is_empty() {
                    debug!("Stream ingestion affected {} proxies, scheduling regenerations", affected_proxies.len());
                    job_scheduler.schedule_proxy_regenerations(affected_proxies).await?;
                }
                
                Ok(())
            }

            JobType::EpgIngestion(source_id) => {
                let affected_proxies = job_executor.execute_epg_job(source_id).await?;
                
                // Schedule proxy regenerations for affected proxies
                if !affected_proxies.is_empty() {
                    debug!("EPG ingestion affected {} proxies, scheduling regenerations", affected_proxies.len());
                    job_scheduler.schedule_proxy_regenerations(affected_proxies).await?;
                }
                
                Ok(())
            }

            JobType::ProxyRegeneration(proxy_id) => {
                job_executor.execute_proxy_regeneration(proxy_id).await
            }

            JobType::Maintenance(operation) => {
                job_executor.execute_maintenance(&operation).await
            }
        }
    }

    /// Count currently running jobs by type category
    fn count_running_jobs_by_type(&self, running_job_keys: &[String]) -> HashMap<JobTypeCategory, usize> {
        Self::count_jobs_by_type(running_job_keys)
    }

    /// Static helper for counting jobs by type - separated for easier testing
    fn count_jobs_by_type(running_job_keys: &[String]) -> HashMap<JobTypeCategory, usize> {
        let mut counts = HashMap::new();
        
        for job_key in running_job_keys {
            let category = if job_key.starts_with("stream:") {
                JobTypeCategory::StreamIngestion
            } else if job_key.starts_with("epg:") {
                JobTypeCategory::EpgIngestion
            } else if job_key.starts_with("proxy:") {
                JobTypeCategory::ProxyRegeneration
            } else if job_key.starts_with("maintenance:") {
                JobTypeCategory::Maintenance
            } else {
                continue; // Unknown job type
            };
            
            *counts.entry(category).or_insert(0) += 1;
        }
        
        counts
    }

    /// Wait for all running jobs to complete during shutdown
    async fn wait_for_running_jobs_to_complete(&self) {
        info!("Waiting for running jobs to complete...");
        
        // Dump initial job status
        self.dump_job_status().await;
        
        let mut check_interval = interval(Duration::from_millis(500));
        let start_time = std::time::Instant::now();
        const MAX_WAIT_TIME: Duration = Duration::from_secs(30); // Maximum wait time
        
        loop {
            let running_count = self.job_queue.running_count().await;
            
            if running_count == 0 {
                info!("All jobs completed successfully");
                break;
            }
            
            if start_time.elapsed() > MAX_WAIT_TIME {
                warn!("Timeout waiting for {} jobs to complete, proceeding with shutdown", running_count);
                self.dump_job_status().await;
                break;
            }
            
            debug!("Still waiting for {} jobs to complete...", running_count);
            check_interval.tick().await;
        }
    }

    /// Dump current job status for debugging
    async fn dump_job_status(&self) {
        let stats = self.get_execution_stats().await;
        let running_keys = self.job_queue.get_running_job_keys().await;
        let pending_stats = self.job_queue.stats().await;
        
        info!("=== JOB STATUS DUMP ===");
        info!("Queue Stats - Pending: {}, Running: {}, Max Concurrent: {}", 
              stats.total_pending, stats.total_running, stats.max_concurrent);
        info!("Running jobs by type: {:?}", stats.running_by_type);
        
        if !running_keys.is_empty() {
            info!("Currently running jobs:");
            for job_key in &running_keys {
                info!("  - {}", job_key);
            }
        }
        
        if pending_stats.pending_jobs > 0 {
            info!("Pending jobs: {}", pending_stats.pending_jobs);
        }
        
        info!("=== END JOB STATUS DUMP ===");
    }

    /// Get current execution statistics
    pub async fn get_execution_stats(&self) -> ExecutionStats {
        let queue_stats = self.job_queue.stats().await;
        let running_job_keys = self.job_queue.get_running_job_keys().await;
        let type_counts = self.count_running_jobs_by_type(&running_job_keys);

        ExecutionStats {
            total_pending: queue_stats.pending_jobs,
            total_running: queue_stats.running_jobs,
            max_concurrent: self.max_concurrent.load(Ordering::Relaxed),
            running_by_type: type_counts,
        }
    }

    /// Update the global maximum concurrent jobs limit at runtime
    pub fn update_global_limit(&self, new_limit: usize) {
        let old_limit = self.max_concurrent.swap(new_limit, Ordering::Relaxed);
        info!("Updated global concurrent jobs limit from {} to {}", old_limit, new_limit);
    }

    /// Update a specific job type concurrency limit at runtime
    pub async fn update_type_limit(&self, job_type: JobTypeCategory, new_limit: usize) {
        let mut limits = self.concurrent_limits.write().await;
        let old_limit = limits.insert(job_type, new_limit);
        
        match old_limit {
            Some(old) => info!("Updated {:?} job limit from {} to {}", job_type, old, new_limit),
            None => info!("Set {:?} job limit to {}", job_type, new_limit),
        }
    }

    /// Get current concurrency configuration
    pub async fn get_concurrency_config(&self) -> JobSchedulingConfig {
        let limits = self.concurrent_limits.read().await;
        
        JobSchedulingConfig {
            global_max_jobs: self.max_concurrent.load(Ordering::Relaxed),
            stream_ingestion_limit: *limits.get(&JobTypeCategory::StreamIngestion).unwrap_or(&1),
            epg_ingestion_limit: *limits.get(&JobTypeCategory::EpgIngestion).unwrap_or(&1),
            proxy_regeneration_limit: *limits.get(&JobTypeCategory::ProxyRegeneration).unwrap_or(&1),
            maintenance_limit: *limits.get(&JobTypeCategory::Maintenance).unwrap_or(&1),
        }
    }

    /// Update multiple concurrency settings at once
    pub async fn update_concurrency_config(&self, config: &JobSchedulingConfig) {
        // Update global limit
        self.update_global_limit(config.global_max_jobs);
        
        // Update type-specific limits
        let mut limits = self.concurrent_limits.write().await;
        
        let old_stream = limits.insert(JobTypeCategory::StreamIngestion, config.stream_ingestion_limit);
        let old_epg = limits.insert(JobTypeCategory::EpgIngestion, config.epg_ingestion_limit);
        let old_proxy = limits.insert(JobTypeCategory::ProxyRegeneration, config.proxy_regeneration_limit);
        let old_maintenance = limits.insert(JobTypeCategory::Maintenance, config.maintenance_limit);
        
        info!("Updated job scheduling configuration:");
        info!("  Global max: {}", config.global_max_jobs);
        info!("  Stream ingestion: {} -> {}", old_stream.unwrap_or(1), config.stream_ingestion_limit);
        info!("  EPG ingestion: {} -> {}", old_epg.unwrap_or(1), config.epg_ingestion_limit);
        info!("  Proxy regeneration: {} -> {}", old_proxy.unwrap_or(1), config.proxy_regeneration_limit);
        info!("  Maintenance: {} -> {}", old_maintenance.unwrap_or(1), config.maintenance_limit);
    }
}

/// Statistics about job execution
#[derive(Debug, Clone)]
pub struct ExecutionStats {
    pub total_pending: usize,
    pub total_running: usize,
    pub max_concurrent: usize,
    pub running_by_type: HashMap<JobTypeCategory, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job_scheduling::types::JobType;
    use uuid::Uuid;

    #[test]
    fn test_job_type_category_mapping() {
        let stream_job = JobType::StreamIngestion(Uuid::new_v4());
        let epg_job = JobType::EpgIngestion(Uuid::new_v4());
        let proxy_job = JobType::ProxyRegeneration(Uuid::new_v4());
        let maintenance_job = JobType::Maintenance("test".to_string());

        assert_eq!(JobTypeCategory::from(&stream_job), JobTypeCategory::StreamIngestion);
        assert_eq!(JobTypeCategory::from(&epg_job), JobTypeCategory::EpgIngestion);
        assert_eq!(JobTypeCategory::from(&proxy_job), JobTypeCategory::ProxyRegeneration);
        assert_eq!(JobTypeCategory::from(&maintenance_job), JobTypeCategory::Maintenance);
    }

    #[test]
    fn test_count_running_jobs_by_type() {
        let running_keys = vec![
            "stream:123e4567-e89b-12d3-a456-426614174000".to_string(),
            "stream:123e4567-e89b-12d3-a456-426614174001".to_string(),
            "epg:123e4567-e89b-12d3-a456-426614174002".to_string(),
            "proxy:123e4567-e89b-12d3-a456-426614174003".to_string(),
        ];
        
        let counts = JobQueueRunner::count_jobs_by_type(&running_keys);
        
        assert_eq!(counts.get(&JobTypeCategory::StreamIngestion), Some(&2));
        assert_eq!(counts.get(&JobTypeCategory::EpgIngestion), Some(&1));
        assert_eq!(counts.get(&JobTypeCategory::ProxyRegeneration), Some(&1));
        assert_eq!(counts.get(&JobTypeCategory::Maintenance), None);
    }

    // Integration tests would require proper mocking of JobExecutor and JobScheduler
    // These would test the full job execution pipeline
}