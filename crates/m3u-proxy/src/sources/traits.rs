//! Source handler trait definitions
//!
//! This module defines the core abstractions for handling different source types.
//! The traits follow SOLID principles, particularly the Interface Segregation
//! Principle (ISP) by providing focused, single-responsibility interfaces.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{StreamSource, Channel, StreamSourceType, EpgSource, EpgProgram, EpgSourceType};
use crate::services::progress_service::UniversalProgressCallback;

/// Source validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceValidationResult {
    /// Whether the source configuration is valid
    pub is_valid: bool,
    /// Validation errors if any
    pub errors: Vec<String>,
    /// Warnings that don't prevent usage
    pub warnings: Vec<String>,
    /// Additional validation context
    pub context: HashMap<String, String>,
}

impl SourceValidationResult {
    /// Create a successful validation result
    pub fn success() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            context: HashMap::new(),
        }
    }

    /// Create a failed validation result
    pub fn failure(errors: Vec<String>) -> Self {
        Self {
            is_valid: false,
            errors,
            warnings: Vec::new(),
            context: HashMap::new(),
        }
    }

    /// Add a warning
    pub fn with_warning<S: Into<String>>(mut self, warning: S) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Add context information
    pub fn with_context<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }
}

/// Source capability information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceCapabilities {
    /// Whether the source supports live streaming
    pub supports_streaming: bool,
    /// Whether the source supports EPG data
    pub supports_epg: bool,
    /// Whether the source supports VOD content
    pub supports_vod: bool,
    /// Whether the source supports channel logos
    pub supports_logos: bool,
    /// Whether the source supports categories/groups
    pub supports_categories: bool,
    /// Whether the source requires authentication
    pub requires_authentication: bool,
    /// Maximum concurrent connections supported
    pub max_concurrent_connections: Option<u32>,
    /// Supported content formats
    pub supported_formats: Vec<String>,
    /// Additional capability metadata
    pub metadata: HashMap<String, String>,
}

impl SourceCapabilities {
    /// Create basic capabilities for M3U sources
    pub fn m3u_basic() -> Self {
        Self {
            supports_streaming: true,
            supports_epg: false,
            supports_vod: false,
            supports_logos: true,
            supports_categories: true,
            requires_authentication: false,
            max_concurrent_connections: None,
            supported_formats: vec!["m3u".to_string(), "m3u8".to_string()],
            metadata: HashMap::new(),
        }
    }

    /// Create capabilities for Xtream sources
    pub fn xtream_full() -> Self {
        Self {
            supports_streaming: true,
            supports_epg: true,
            supports_vod: true,
            supports_logos: true,
            supports_categories: true,
            requires_authentication: true,
            max_concurrent_connections: Some(1000),
            supported_formats: vec!["xtream".to_string(), "m3u8".to_string()],
            metadata: HashMap::new(),
        }
    }
}

/// Ingestion progress information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionProgress {
    /// Current processing step
    pub current_step: String,
    /// Total bytes to download (if known)
    pub total_bytes: Option<u64>,
    /// Bytes downloaded so far
    pub downloaded_bytes: Option<u64>,
    /// Channels parsed so far
    pub channels_parsed: Option<u32>,
    /// Channels successfully saved
    pub channels_saved: Option<u32>,
    /// Overall progress percentage (0-100)
    pub percentage: Option<f32>,
    /// Additional progress metadata
    pub metadata: HashMap<String, String>,
}

impl IngestionProgress {
    /// Create initial progress state
    pub fn starting<S: Into<String>>(step: S) -> Self {
        Self {
            current_step: step.into(),
            total_bytes: None,
            downloaded_bytes: None,
            channels_parsed: None,
            channels_saved: None,
            percentage: Some(0.0),
            metadata: HashMap::new(),
        }
    }

    /// Update progress with new step
    pub fn update_step<S: Into<String>>(mut self, step: S, percentage: Option<f32>) -> Self {
        self.current_step = step.into();
        self.percentage = percentage;
        self
    }
}

/// Progress callback type for reporting ingestion progress
/// DEPRECATED: Use UniversalProgressCallback instead
pub type ProgressCallback = dyn Fn(IngestionProgress) + Send + Sync;

/// Universal progress callback type (preferred)
pub type UniversalCallback = Box<UniversalProgressCallback>;

/// Core source handler trait
///
/// This trait defines the essential operations that all source handlers must implement.
/// It follows the Single Responsibility Principle by focusing solely on source-specific
/// operations.
#[async_trait]
pub trait SourceHandler: Send + Sync {
    /// Get the source type this handler supports
    fn source_type(&self) -> StreamSourceType;

    /// Validate a source configuration
    async fn validate_source(&self, source: &StreamSource) -> AppResult<SourceValidationResult>;

    /// Get capabilities for a specific source
    async fn get_capabilities(&self, source: &StreamSource) -> AppResult<SourceCapabilities>;

    /// Test connectivity to a source
    async fn test_connectivity(&self, source: &StreamSource) -> AppResult<bool>;

    /// Get source metadata (version, server info, etc.)
    async fn get_source_info(&self, source: &StreamSource) -> AppResult<HashMap<String, String>>;
}

/// Channel ingestion trait
///
/// Separated from SourceHandler to follow the Interface Segregation Principle.
/// Sources that support channel ingestion implement this trait.
#[async_trait]
pub trait ChannelIngestor: Send + Sync {
    /// Ingest channels from a source
    async fn ingest_channels(&self, source: &StreamSource) -> AppResult<Vec<Channel>>;

    /// Ingest channels with progress callback
    async fn ingest_channels_with_progress(
        &self,
        source: &StreamSource,
        progress_callback: Option<&ProgressCallback>,
    ) -> AppResult<Vec<Channel>>;

    /// Ingest channels with universal progress callback (preferred)
    async fn ingest_channels_with_universal_progress(
        &self,
        source: &StreamSource,
        progress_callback: Option<&UniversalCallback>,
    ) -> AppResult<Vec<Channel>>;

    /// Estimate the number of channels available (for progress reporting)
    async fn estimate_channel_count(&self, source: &StreamSource) -> AppResult<Option<u32>>;
}

/// Health checking trait
///
/// Sources that support health monitoring implement this trait.
#[async_trait]
pub trait HealthChecker: Send + Sync {
    /// Check if the source is currently healthy
    async fn check_health(&self, source: &StreamSource) -> AppResult<SourceHealthStatus>;

    /// Get detailed health metrics
    async fn get_health_metrics(&self, source: &StreamSource) -> AppResult<SourceHealthMetrics>;
}

/// Source health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceHealthStatus {
    /// Whether the source is healthy
    pub is_healthy: bool,
    /// Response time in milliseconds
    pub response_time_ms: Option<u64>,
    /// Last successful check timestamp
    pub last_success: Option<chrono::DateTime<chrono::Utc>>,
    /// Last error message if unhealthy
    pub error_message: Option<String>,
    /// Health check timestamp
    pub checked_at: chrono::DateTime<chrono::Utc>,
}

/// Detailed health metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceHealthMetrics {
    /// Basic health status
    pub status: SourceHealthStatus,
    /// Number of available channels
    pub channel_count: Option<u32>,
    /// Server version or identifier
    pub server_version: Option<String>,
    /// Uptime information
    pub uptime: Option<String>,
    /// Additional metrics
    pub metrics: HashMap<String, String>,
}

/// URL generation trait
///
/// Sources that support URL generation for streaming implement this trait.
#[async_trait]
pub trait StreamUrlGenerator: Send + Sync {
    /// Generate a streaming URL for a channel
    async fn generate_stream_url(
        &self,
        source: &StreamSource,
        channel_id: &str,
    ) -> AppResult<String>;

    /// Generate URLs for multiple channels at once
    async fn generate_stream_urls(
        &self,
        source: &StreamSource,
        channel_ids: &[String],
    ) -> AppResult<HashMap<String, String>>;

    /// Validate that a generated URL is accessible
    async fn validate_stream_url(
        &self,
        source: &StreamSource,
        url: &str,
    ) -> AppResult<bool>;
}

/// Authentication trait
///
/// Sources that require authentication implement this trait.
#[async_trait]
pub trait Authenticator: Send + Sync {
    /// Authenticate with the source
    async fn authenticate(&self, source: &StreamSource) -> AppResult<AuthenticationResult>;

    /// Refresh authentication if needed
    async fn refresh_authentication(&self, source: &StreamSource) -> AppResult<AuthenticationResult>;

    /// Check if current authentication is valid
    async fn is_authenticated(&self, source: &StreamSource) -> AppResult<bool>;
}

/// Authentication result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationResult {
    /// Whether authentication was successful
    pub success: bool,
    /// Authentication token or session ID if applicable
    pub token: Option<String>,
    /// Token expiration time if applicable
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Error message if authentication failed
    pub error_message: Option<String>,
    /// Additional authentication metadata
    pub metadata: HashMap<String, String>,
}

/// Composite trait for full-featured source handlers
///
/// This trait combines all the individual traits for sources that support
/// all functionality. Implementing this trait indicates a fully-featured source.
pub trait FullSourceHandler: 
    SourceHandler + 
    ChannelIngestor + 
    HealthChecker + 
    StreamUrlGenerator + 
    Send + 
    Sync 
{
    /// Get a comprehensive source summary
    fn get_handler_summary(&self) -> SourceHandlerSummary {
        SourceHandlerSummary {
            source_type: self.source_type(),
            supports_ingestion: true,
            supports_health_check: true,
            supports_url_generation: true,
            supports_authentication: false, // Default, can be overridden
        }
    }
}

/// Summary of source handler capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceHandlerSummary {
    /// Source type this handler supports
    pub source_type: StreamSourceType,
    /// Whether the handler supports channel ingestion
    pub supports_ingestion: bool,
    /// Whether the handler supports health checking
    pub supports_health_check: bool,
    /// Whether the handler supports URL generation
    pub supports_url_generation: bool,
    /// Whether the handler supports authentication
    pub supports_authentication: bool,
}

/// Error types specific to source handling
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("Source type '{0:?}' is not supported")]
    UnsupportedSourceType(StreamSourceType),
    
    #[error("Source configuration is invalid: {0}")]
    InvalidConfiguration(String),
    
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),
    
    #[error("Source is not reachable: {0}")]
    ConnectionFailed(String),
    
    #[error("Ingestion failed: {0}")]
    IngestionFailed(String),
    
    #[error("URL generation failed: {0}")]
    UrlGenerationFailed(String),
    
    #[error("Health check failed: {0}")]
    HealthCheckFailed(String),
}

// ============================================================================
// EPG-Specific Traits
// ============================================================================

/// Core EPG source handler trait
///
/// This trait defines the essential operations that all EPG source handlers must implement.
/// It follows the Single Responsibility Principle by focusing solely on EPG source-specific
/// operations.
#[async_trait]
pub trait EpgSourceHandler: Send + Sync {
    /// Get the EPG source type this handler supports
    fn epg_source_type(&self) -> EpgSourceType;

    /// Validate an EPG source configuration
    async fn validate_epg_source(&self, source: &EpgSource) -> AppResult<SourceValidationResult>;

    /// Get capabilities for a specific EPG source
    async fn get_epg_capabilities(&self, source: &EpgSource) -> AppResult<EpgSourceCapabilities>;

    /// Test connectivity to an EPG source
    async fn test_epg_connectivity(&self, source: &EpgSource) -> AppResult<bool>;

    /// Get EPG source metadata (version, server info, etc.)
    async fn get_epg_source_info(&self, source: &EpgSource) -> AppResult<HashMap<String, String>>;
}

/// EPG program ingestion trait
///
/// Separated from EpgSourceHandler to follow the Interface Segregation Principle.
/// EPG sources that support program ingestion implement this trait.
#[async_trait]
pub trait EpgProgramIngestor: Send + Sync {
    /// Ingest EPG programs from a source (programs-only mode)
    async fn ingest_epg_programs(&self, source: &EpgSource) -> AppResult<Vec<EpgProgram>>;

    /// Ingest EPG programs with progress callback (programs-only mode)
    async fn ingest_epg_programs_with_progress(
        &self,
        source: &EpgSource,
        progress_callback: Option<&EpgProgressCallback>,
    ) -> AppResult<Vec<EpgProgram>>;

    /// Ingest EPG programs with universal progress callback (preferred)
    async fn ingest_epg_programs_with_universal_progress(
        &self,
        source: &EpgSource,
        progress_callback: Option<&UniversalCallback>,
    ) -> AppResult<Vec<EpgProgram>>;

    /// Estimate the number of programs available (for progress reporting)
    async fn estimate_program_count(&self, source: &EpgSource) -> AppResult<Option<u32>>;
}

/// EPG source capability information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgSourceCapabilities {
    /// Whether the source supports live EPG data
    pub supports_live_epg: bool,
    /// Whether the source supports historical EPG data
    pub supports_historical_epg: bool,
    /// Whether the source supports channel information
    pub supports_channel_info: bool,
    /// Whether the source supports program categories
    pub supports_categories: bool,
    /// Whether the source requires authentication
    pub requires_authentication: bool,
    /// Maximum days of EPG data available
    pub max_days_available: Option<u32>,
    /// Supported EPG formats
    pub supported_formats: Vec<String>,
    /// Additional capability metadata
    pub metadata: HashMap<String, String>,
}

impl EpgSourceCapabilities {
    /// Create basic capabilities for XMLTV sources
    pub fn xmltv_basic() -> Self {
        Self {
            supports_live_epg: true,
            supports_historical_epg: true,
            supports_channel_info: true,
            supports_categories: true,
            requires_authentication: false,
            max_days_available: None,
            supported_formats: vec!["xmltv".to_string(), "xml".to_string()],
            metadata: HashMap::new(),
        }
    }

    /// Create capabilities for Xtream EPG sources
    pub fn xtream_epg() -> Self {
        Self {
            supports_live_epg: true,
            supports_historical_epg: true,
            supports_channel_info: true,
            supports_categories: true,
            requires_authentication: true,
            max_days_available: Some(7), // Typical Xtream EPG retention
            supported_formats: vec!["xmltv".to_string(), "xtream".to_string()],
            metadata: HashMap::new(),
        }
    }
}

/// EPG progress information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgIngestionProgress {
    /// Current processing step
    pub current_step: String,
    /// Total bytes to download (if known)
    pub total_bytes: Option<u64>,
    /// Bytes downloaded so far
    pub downloaded_bytes: Option<u64>,
    /// Channels parsed so far
    pub channels_parsed: Option<u32>,
    /// Programs parsed so far
    pub programs_parsed: Option<u32>,
    /// Programs successfully saved
    pub programs_saved: Option<u32>,
    /// Overall progress percentage (0-100)
    pub percentage: Option<f32>,
    /// Additional progress metadata
    pub metadata: HashMap<String, String>,
}

impl EpgIngestionProgress {
    /// Create initial progress state
    pub fn starting<S: Into<String>>(step: S) -> Self {
        Self {
            current_step: step.into(),
            total_bytes: None,
            downloaded_bytes: None,
            channels_parsed: None,
            programs_parsed: None,
            programs_saved: None,
            percentage: Some(0.0),
            metadata: HashMap::new(),
        }
    }

    /// Update progress with new step
    pub fn update_step<S: Into<String>>(mut self, step: S, percentage: Option<f32>) -> Self {
        self.current_step = step.into();
        self.percentage = percentage;
        self
    }
}

/// EPG progress callback type for reporting ingestion progress
/// DEPRECATED: Use UniversalProgressCallback instead
pub type EpgProgressCallback = dyn Fn(EpgIngestionProgress) + Send + Sync;

/// Composite trait for full-featured EPG source handlers
///
/// This trait combines all the individual EPG traits for sources that support
/// all EPG functionality. Implementing this trait indicates a fully-featured EPG source.
pub trait FullEpgSourceHandler: 
    EpgSourceHandler + 
    EpgProgramIngestor + 
    Send + 
    Sync 
{
    /// Get a comprehensive EPG source summary
    fn get_epg_handler_summary(&self) -> EpgSourceHandlerSummary {
        EpgSourceHandlerSummary {
            epg_source_type: self.epg_source_type(),
            supports_program_ingestion: true,
            supports_authentication: false, // Default, can be overridden
        }
    }
}

/// Summary of EPG source handler capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgSourceHandlerSummary {
    /// EPG source type this handler supports
    pub epg_source_type: EpgSourceType,
    /// Whether the handler supports program ingestion
    pub supports_program_ingestion: bool,
    /// Whether the handler supports authentication
    pub supports_authentication: bool,
}