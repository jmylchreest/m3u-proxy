//! Source Type Handlers
//!
//! This module provides polymorphic source handling abstractions that implement
//! the Open/Closed Principle. Each source type (M3U, Xtream) has its own handler
//! that implements common traits for ingestion, validation, and capability detection.
//!
//! # Architecture
//!
//! The source handler system follows these patterns:
//! - **Strategy Pattern**: Different algorithms for each source type
//! - **Factory Pattern**: Source handler creation based on type
//! - **Capability Pattern**: Dynamic feature detection per source
//! - **Polymorphism**: Common interface across all source types
//!
//! # Usage
//!
//! ```rust
//! use m3u_proxy::sources::factory::SourceHandlerFactory;
//! use m3u_proxy::sources::traits::SourceHandler;
//! use m3u_proxy::models::{StreamSource, StreamSourceType};
//! use uuid::Uuid;
//! use chrono::Utc;
//!
//! async fn example() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create example source
//!     let source = StreamSource {
//!         id: Uuid::new_v4(),
//!         name: "Example".to_string(),
//!         source_type: StreamSourceType::M3u,
//!         url: "http://example.com/playlist.m3u".to_string(),
//!         max_concurrent_streams: 10,
//!         update_cron: "0 */6 * * *".to_string(),
//!         username: None,
//!         password: None,
//!         field_map: None,
//!         ignore_channel_numbers: false,
//!         is_active: true,
//!         created_at: Utc::now(),
//!         updated_at: Utc::now(),
//!         last_ingested_at: None,
//!     };
//!     
//!     // Get appropriate handler for source type
//!     let handler = SourceHandlerFactory::create_handler(&source.source_type)?;
//!     
//!     // Example operations would be done here in actual usage
//!     Ok(())
//! }
//! ```

pub mod traits;
pub mod m3u;
pub mod xtream;
pub mod xmltv_epg;
pub mod xtream_epg;
pub mod factory;

pub use traits::*;
pub use factory::SourceHandlerFactory;