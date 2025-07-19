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

pub mod channel;
pub mod cyclic_buffer;
pub mod data_mapping;
pub mod epg_source_service;
pub mod error_fallback;
pub mod ffmpeg_wrapper;
pub mod file_categories;
pub mod filter;
pub mod logo_cache_scanner;
pub mod metrics_housekeeper;
pub mod proxy_regeneration;
pub mod relay_manager;
pub mod sandboxed_file;
pub mod sandboxed_file_trait;
pub mod source_linking_service;
pub mod stream_proxy;
pub mod stream_prober;
pub mod stream_source;
pub mod stream_source_service;
pub mod traits;

// Re-export main traits and services
pub use channel::ChannelService;
pub use cyclic_buffer::{CyclicBuffer, CyclicBufferConfig, BufferClient, CyclicBufferStats};
pub use data_mapping::DataMappingService;
pub use epg_source_service::EpgSourceService;
pub use error_fallback::{ErrorFallbackGenerator, StreamHealthMonitor};
pub use ffmpeg_wrapper::FFmpegProcessWrapper;
pub use filter::FilterService;
pub use metrics_housekeeper::MetricsHousekeeper;
pub use proxy_regeneration::ProxyRegenerationService;
pub use relay_manager::RelayManager;
pub use source_linking_service::SourceLinkingService;
pub use stream_prober::{StreamProber, ProbeResult, StreamMappingStrategy};
pub use stream_proxy::StreamProxyService;
pub use stream_source::StreamSourceService;
pub use stream_source_service::StreamSourceService as StreamSourceBusinessService;
pub use traits::*;
