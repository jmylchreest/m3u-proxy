//! Repository pattern implementation for data access
//!
//! This module provides a clean abstraction layer over the database,
//! implementing the Repository pattern to separate business logic from
//! data access concerns.
//!
//! # Architecture
//!
//! The repository pattern provides:
//! - Clear separation between business logic and data access
//! - Testability through trait-based interfaces
//! - Consistency in database operations
//! - Centralized query optimization
//!
//! # Usage
//!
//! ```rust
//! use m3u_proxy::repositories::traits::Repository;
//! use m3u_proxy::repositories::StreamSourceRepository;
//! use m3u_proxy::models::StreamSource;
//! use uuid::Uuid;
//!
//! async fn example(repo: impl Repository<StreamSource, Uuid>) -> Result<(), Box<dyn std::error::Error>> {
//!     let id = Uuid::new_v4();
//!     let source = repo.find_by_id(id).await?;
//!     // ... use source
//!     Ok(())
//! }
//! ```

pub mod traits;
pub mod retry_wrapper;
pub mod stream_source;
pub mod stream_proxy;
pub mod channel;
pub mod filter;
pub mod epg_source;
pub mod epg_program;
pub mod url_linking;
pub mod relay;
pub mod health;

// Re-export main traits and types
pub use traits::*;
pub use stream_source::StreamSourceRepository;
pub use stream_proxy::StreamProxyRepository;
pub use channel::ChannelRepository;
pub use filter::FilterRepository;
pub use epg_source::{EpgSourceRepository, EpgSourceWithStats};
pub use epg_program::{EpgProgramRepository, EpgProgramQuery};
pub use url_linking::UrlLinkingRepository;
pub use relay::RelayRepository;
pub use health::{HealthRepository, ScheduledSource, ChannelInfo};