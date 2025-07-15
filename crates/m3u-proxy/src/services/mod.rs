//! Service layer for business logic
//!
//! This module provides the service layer that contains the business logic
//! for the M3U Proxy application. Services act as the orchestration layer
//! between the web controllers and the repository layer.
//!
//! # Architecture
//!
//! The service layer provides:
//! - Business logic orchestration
//! - Transaction management
//! - Input validation and transformation
//! - Error handling and logging
//! - Cross-cutting concerns (caching, metrics, etc.)
//!
//! # Design Principles
//!
//! - **Single Responsibility**: Each service handles one business domain
//! - **Dependency Injection**: Services depend on repository traits, not concrete implementations
//! - **Error Handling**: Services convert repository errors to domain-specific errors
//! - **Validation**: Input validation happens at the service layer
//! - **Logging**: Business operations are logged with appropriate context
//!
//! # Usage
//!
//! ```rust
//! use crate::services::{StreamSourceService, ChannelService};
//! use crate::repositories::{StreamSourceRepository, ChannelRepository};
//!
//! async fn example() {
//!     let stream_repo = StreamSourceRepository::new(pool);
//!     let channel_repo = ChannelRepository::new(pool);
//!     
//!     let stream_service = StreamSourceService::new(stream_repo);
//!     let channel_service = ChannelService::new(channel_repo);
//!     
//!     // Use services for business operations
//!     let source = stream_service.create_source(request).await?;
//! }
//! ```

pub mod traits;
pub mod stream_source;
pub mod stream_proxy;
pub mod channel;
pub mod filter;
pub mod data_mapping;
pub mod proxy_regeneration;
pub mod file_categories;
pub mod sandboxed_file_trait;
pub mod sandboxed_file;
pub mod logo_cache_scanner;
pub mod metrics_housekeeper;

// Re-export main traits and services
pub use traits::*;
pub use stream_source::StreamSourceService;
pub use stream_proxy::StreamProxyService;
pub use channel::ChannelService;
pub use filter::FilterService;
pub use data_mapping::DataMappingService;
pub use proxy_regeneration::ProxyRegenerationService;
pub use metrics_housekeeper::MetricsHousekeeper;
