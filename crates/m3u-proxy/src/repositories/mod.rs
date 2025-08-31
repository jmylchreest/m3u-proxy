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
//! use m3u_proxy::database::repositories::StreamSourceSeaOrmRepository;
//! use m3u_proxy::models::{StreamSource, StreamSourceCreateRequest, StreamSourceType};
//! use sea_orm::DatabaseConnection;
//! use std::sync::Arc;
//! use uuid::Uuid;
//!
//! async fn example() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create a mock database connection for the example
//!     # let connection = Arc::new(sea_orm::Database::connect("sqlite::memory:").await?);
//!     let repo = StreamSourceSeaOrmRepository::new(connection);
//!     
//!     let create_request = StreamSourceCreateRequest {
//!         name: "Example Source".to_string(),
//!         source_type: StreamSourceType::M3u,
//!         url: "http://example.com/playlist.m3u".to_string(),
//!         max_concurrent_streams: 10,
//!         update_cron: "0 0 */6 * * * *".to_string(),
//!         username: None,
//!         password: None,
//!         field_map: None,
//!         ignore_channel_numbers: false,
//!     };
//!     
//!     let source = repo.create(create_request).await?;
//!     let found_source = repo.find_by_id(&source.id).await?;
//!     // ... use source
//!     Ok(())
//! }
//! ```

pub mod traits;
pub mod retry_wrapper;

// Re-export main traits and types
pub use traits::*;
