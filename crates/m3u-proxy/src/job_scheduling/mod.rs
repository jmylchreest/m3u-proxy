//! Job scheduling subsystem for m3u-proxy
//!
//! This module provides a unified job queue system that handles:
//! - Stream source ingestion scheduling
//! - EPG source ingestion scheduling  
//! - Proxy regeneration scheduling
//! - Maintenance task scheduling
//!
//! The system is built around four main components:
//! - `JobQueue`: Thread-safe job storage with deduplication
//! - `JobScheduler`: Cron-based job scheduling service
//! - `JobQueueRunner`: Job execution coordination service
//! - `JobExecutor`: Actual work execution service

pub mod api;
pub mod job_executor;
pub mod job_queue;
pub mod job_queue_runner;
pub mod job_scheduler;
pub mod types;

pub use api::JobSchedulingAPI;
pub use job_executor::JobExecutor;
pub use job_queue::JobQueue;
pub use job_queue_runner::JobQueueRunner;
pub use job_scheduler::JobScheduler;
pub use types::*;