//! Job scheduling type definitions

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use uuid::Uuid;

/// Priority levels for job execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JobPriority {
    /// System startup, recovery operations
    Critical = 0,
    /// Manual user triggers
    High = 1,
    /// Regular scheduled refreshes
    Normal = 2,
    /// Proxy regeneration (runs after ingestion jobs)
    Low = 3,
    /// Background maintenance tasks
    Maintenance = 4,
}

impl PartialOrd for JobPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JobPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

/// Type of job to be executed
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JobType {
    /// Stream source ingestion job
    StreamIngestion(Uuid),
    /// EPG source ingestion job
    EpgIngestion(Uuid),
    /// Proxy regeneration job
    ProxyRegeneration(Uuid),
    /// Maintenance job with operation name
    Maintenance(String),
}

impl JobType {
    /// Generate a unique key for deduplication
    /// Jobs with the same key will be deduplicated
    pub fn job_key(&self) -> String {
        match self {
            JobType::StreamIngestion(source_id) => format!("stream:{source_id}"),
            JobType::EpgIngestion(source_id) => format!("epg:{source_id}"),
            JobType::ProxyRegeneration(proxy_id) => format!("proxy:{proxy_id}"),
            JobType::Maintenance(operation) => format!("maintenance:{operation}"),
        }
    }

    /// Get the resource ID this job operates on
    pub fn resource_id(&self) -> Option<Uuid> {
        match self {
            JobType::StreamIngestion(id) 
            | JobType::EpgIngestion(id) 
            | JobType::ProxyRegeneration(id) => Some(*id),
            JobType::Maintenance(_) => None,
        }
    }
}

/// A scheduled job ready for execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJob {
    /// Unique job instance identifier
    pub id: Uuid,
    /// Type of job to execute
    pub job_type: JobType,
    /// When this job should be executed
    pub scheduled_time: DateTime<Utc>,
    /// Priority level for execution ordering
    pub priority: JobPriority,
}

impl ScheduledJob {
    /// Create a new scheduled job
    pub fn new(job_type: JobType, priority: JobPriority) -> Self {
        Self {
            id: Uuid::new_v4(),
            job_type,
            scheduled_time: Utc::now(),
            priority,
        }
    }

    /// Create a new scheduled job with specific time
    pub fn new_scheduled(job_type: JobType, priority: JobPriority, scheduled_time: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            job_type,
            scheduled_time,
            priority,
        }
    }

    /// Get the deduplication key for this job
    pub fn job_key(&self) -> String {
        self.job_type.job_key()
    }

    /// Check if this job is ready to run
    pub fn is_ready(&self, now: DateTime<Utc>) -> bool {
        self.scheduled_time <= now
    }
}

impl PartialEq for ScheduledJob {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ScheduledJob {}

impl PartialOrd for ScheduledJob {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledJob {
    /// Jobs are ordered by priority first, then by scheduled time
    /// Critical (0) < Normal (2), and earlier times come first
    fn cmp(&self, other: &Self) -> Ordering {
        // First compare by priority (Critical = 0 should be "less than" Normal = 2)
        match self.priority.cmp(&other.priority) {
            Ordering::Equal => {
                // If priorities are equal, order by scheduled time (earlier time first)
                self.scheduled_time.cmp(&other.scheduled_time)
            }
            priority_order => priority_order,
        }
    }
}

/// Errors that can occur in the job scheduling system
#[derive(Debug, thiserror::Error)]
pub enum JobSchedulingError {
    /// Job already exists in the queue
    #[error("Job with key '{key}' already exists in queue")]
    DuplicateJob { key: String },
    
    /// Job queue is full
    #[error("Job queue is full, cannot enqueue more jobs")]
    QueueFull,
    
    /// Invalid job configuration
    #[error("Invalid job configuration: {reason}")]
    InvalidJob { reason: String },
    
    /// Database operation failed
    #[error("Database operation failed: {source}")]
    DatabaseError { 
        #[from]
        source: anyhow::Error 
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_job_priority_ordering() {
        assert!(JobPriority::Critical < JobPriority::High);
        assert!(JobPriority::High < JobPriority::Normal);
        assert!(JobPriority::Normal < JobPriority::Low);
        assert!(JobPriority::Low < JobPriority::Maintenance);
    }

    #[test]
    fn test_job_type_key_generation() {
        let stream_id = Uuid::new_v4();
        let epg_id = Uuid::new_v4();
        let proxy_id = Uuid::new_v4();

        let stream_job = JobType::StreamIngestion(stream_id);
        let epg_job = JobType::EpgIngestion(epg_id);
        let proxy_job = JobType::ProxyRegeneration(proxy_id);
        let maintenance_job = JobType::Maintenance("cleanup".to_string());

        assert_eq!(stream_job.job_key(), format!("stream:{stream_id}"));
        assert_eq!(epg_job.job_key(), format!("epg:{epg_id}"));
        assert_eq!(proxy_job.job_key(), format!("proxy:{proxy_id}"));
        assert_eq!(maintenance_job.job_key(), "maintenance:cleanup");
    }

    #[test]
    fn test_scheduled_job_ordering() {
        let now = Utc::now();
        
        // Job with higher priority (lower enum value) should come first
        let critical_job = ScheduledJob::new_scheduled(
            JobType::Maintenance("test".to_string()),
            JobPriority::Critical,
            now + Duration::hours(1),
        );
        
        let normal_job = ScheduledJob::new_scheduled(
            JobType::Maintenance("test2".to_string()),
            JobPriority::Normal,
            now,
        );
        
        assert!(critical_job < normal_job);
        
        // Jobs with same priority should be ordered by time
        let earlier_job = ScheduledJob::new_scheduled(
            JobType::Maintenance("earlier".to_string()),
            JobPriority::Normal,
            now,
        );
        
        let later_job = ScheduledJob::new_scheduled(
            JobType::Maintenance("later".to_string()),
            JobPriority::Normal,
            now + Duration::minutes(10),
        );
        
        assert!(earlier_job < later_job);
    }

    #[test]
    fn test_job_is_ready() {
        let now = Utc::now();
        
        let ready_job = ScheduledJob::new_scheduled(
            JobType::Maintenance("ready".to_string()),
            JobPriority::Normal,
            now - Duration::minutes(1),
        );
        
        let future_job = ScheduledJob::new_scheduled(
            JobType::Maintenance("future".to_string()),
            JobPriority::Normal,
            now + Duration::minutes(1),
        );
        
        assert!(ready_job.is_ready(now));
        assert!(!future_job.is_ready(now));
    }
}