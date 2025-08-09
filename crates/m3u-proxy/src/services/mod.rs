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
//! use crate::services::StreamSourceService;
//!
//! async fn example() {
//!     let stream_service = StreamSourceService::new(database, epg_service, cache_tx);
//!
//!     // Use services for business operations
//!     let source = stream_service.create_with_auto_epg(request).await?;
//! }
//! ```

pub mod cyclic_buffer;
pub mod epg_source_service;
pub mod error_fallback;
pub mod ffmpeg_command_builder;
pub mod ffmpeg_wrapper;
pub mod file_categories;
pub mod logo_cache_scanner;
pub mod progress_service;
pub mod proxy_regeneration;
pub mod relay_config_resolver;
pub mod relay_manager;
pub mod sandboxed_file;
pub mod sandboxed_file_trait;
pub mod source_linking_service;
pub mod stream_proxy;
pub mod stream_prober;
pub mod stream_source_service;
pub mod traits;

// Re-export main traits and services
pub use cyclic_buffer::{CyclicBuffer, CyclicBufferConfig, BufferClient, CyclicBufferStats};
pub use epg_source_service::EpgSourceService;
pub use error_fallback::{ErrorFallbackGenerator, StreamHealthMonitor};
pub use ffmpeg_command_builder::FFmpegCommandBuilder;
pub use ffmpeg_wrapper::FFmpegProcessWrapper;
pub use progress_service::{ProgressService, OperationType};
pub use proxy_regeneration::ProxyRegenerationService;
pub use relay_config_resolver::RelayConfigResolver;
pub use relay_manager::RelayManager;
pub use source_linking_service::SourceLinkingService;
pub use stream_prober::{StreamProber, ProbeResult, StreamMappingStrategy};
pub use stream_proxy::StreamProxyService;
pub use stream_source_service::StreamSourceService as StreamSourceBusinessService;
pub use traits::*;
