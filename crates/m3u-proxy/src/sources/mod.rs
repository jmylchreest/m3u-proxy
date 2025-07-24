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
//! use crate::sources::{SourceHandlerFactory, SourceHandler};
//! use crate::models::{StreamSource, StreamSourceType};
//!
//! async fn example() -> Result<(), Box<dyn std::error::Error>> {
//!     let source = StreamSource { /* ... */ };
//!     
//!     // Get appropriate handler for source type
//!     let handler = SourceHandlerFactory::create_handler(&source.source_type)?;
//!     
//!     // Validate source configuration
//!     let validation = handler.validate_source(&source).await?;
//!     if !validation.is_valid {
//!         return Err("Invalid source configuration".into());
//!     }
//!     
//!     // Check capabilities
//!     let capabilities = handler.get_capabilities(&source).await?;
//!     if capabilities.supports_streaming {
//!         // Perform ingestion
//!         let channels = handler.ingest_channels(&source).await?;
//!         println!("Ingested {} channels", channels.len());
//!     }
//!     
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