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
//! use crate::repositories::{Repository, StreamSourceRepository};
//! use uuid::Uuid;
//!
//! async fn example(repo: impl Repository<StreamSource, Uuid>) {
//!     let source = repo.find_by_id(uuid).await?;
//!     // ... use source
//! }
//! ```

pub mod traits;
pub mod stream_source;
pub mod channel;
pub mod filter;

// Re-export main traits and types
pub use traits::*;
pub use stream_source::StreamSourceRepository;
pub use channel::ChannelRepository;
pub use filter::FilterRepository;