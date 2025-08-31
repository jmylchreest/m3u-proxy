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
//! use m3u_proxy::services::stream_source_service::StreamSourceService;
//!
//! async fn example() -> Result<(), Box<dyn std::error::Error>> {
//!     // Services provide high-level business operations
//!     // Actual usage would require proper initialization with dependencies
//!     Ok(())
//! }
//! ```

pub mod circuit_breaker_manager;
pub mod circuit_breaker_pool;
pub mod cyclic_buffer;
pub mod connection_limiter;
pub mod embedded_font;
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
pub mod url_linking_service;

// Re-export main traits and services
pub use circuit_breaker_manager::CircuitBreakerManager;
pub use circuit_breaker_pool::{CircuitBreakerPool, PoolStats};
pub use cyclic_buffer::{CyclicBuffer, CyclicBufferConfig, BufferClient, CyclicBufferStats};
pub use connection_limiter::{ConnectionLimiter, ConnectionLimitsConfig, LimitExceededError, ConnectionHandle};
pub use embedded_font::EmbeddedFontManager;
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
pub use stream_proxy::{StreamProxyService, StreamProxyServiceBuilder};
pub use stream_source_service::StreamSourceService as StreamSourceBusinessService;
pub use traits::*;
pub use url_linking_service::UrlLinkingService;
