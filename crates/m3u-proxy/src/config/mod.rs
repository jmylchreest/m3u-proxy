use anyhow::Result;
use figment::{Figment, providers::{Toml, Env, Format}};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod defaults;
pub mod duration_serde;
pub mod file_categories;

use defaults::*;






#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub database: DatabaseConfig,
    pub web: WebConfig,
    pub storage: StorageConfig,
    pub ingestion: IngestionConfig,
    pub data_mapping_engine: Option<DataMappingEngineConfig>,
    pub relay: Option<RelayConfig>,
    pub security: Option<SecurityConfig>,
    pub features: Option<FeaturesConfig>,
    pub circuitbreaker: Option<CircuitBreakerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeaturesConfig {
    /// Simple boolean flags for enabling/disabling features
    /// Example: debug-frontend = true, experimental-ui = false
    #[serde(default)]
    pub flags: std::collections::HashMap<String, bool>,
    
    /// Per-feature configuration settings
    /// Example: config.debug-frontend.level = "verbose", config.video-player.buffer-size = 1024
    #[serde(default)]
    pub config: std::collections::HashMap<String, std::collections::HashMap<String, serde_json::Value>>,
}

impl FeaturesConfig {
    /// Check if a feature flag is enabled (defaults to false if not found)
    pub fn is_feature_enabled(&self, feature_name: &str) -> bool {
        self.flags.get(feature_name).copied().unwrap_or(false)
    }
    
    /// Get configuration for a specific feature (returns empty map if not found)
    pub fn get_feature_config(&self, feature_name: &str) -> &std::collections::HashMap<String, serde_json::Value> {
        use std::sync::LazyLock;
        static EMPTY_CONFIG: LazyLock<std::collections::HashMap<String, serde_json::Value>> = LazyLock::new(std::collections::HashMap::new);
        self.config.get(feature_name).unwrap_or(&EMPTY_CONFIG)
    }
    
    /// Get a specific config value for a feature (returns None if not found)
    pub fn get_feature_config_value(&self, feature_name: &str, config_key: &str) -> Option<&serde_json::Value> {
        self.get_feature_config(feature_name).get(config_key)
    }
    
    /// Get a config value as a string (returns None if not found or not a string)
    pub fn get_config_string(&self, feature_name: &str, config_key: &str) -> Option<String> {
        self.get_feature_config_value(feature_name, config_key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
    
    /// Get a config value as a number (returns None if not found or not a number)
    pub fn get_config_number(&self, feature_name: &str, config_key: &str) -> Option<f64> {
        self.get_feature_config_value(feature_name, config_key)
            .and_then(|v| v.as_f64())
    }
    
    /// Get a config value as a boolean (returns None if not found or not a boolean)
    pub fn get_config_bool(&self, feature_name: &str, config_key: &str) -> Option<bool> {
        self.get_feature_config_value(feature_name, config_key)
            .and_then(|v| v.as_bool())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: Option<u32>,
    pub batch_sizes: Option<DatabaseBatchConfig>,
    
    /// SQLite-specific configuration
    #[serde(default)]
    pub sqlite: SqliteConfig,
    
    /// PostgreSQL-specific configuration  
    #[serde(default)]
    pub postgresql: PostgreSqlConfig,
    
    /// MySQL-specific configuration
    #[serde(default)]
    pub mysql: MySqlConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteConfig {
    #[serde(default = "default_busy_timeout")]
    pub busy_timeout: String,
    #[serde(default = "default_cache_size")]
    pub cache_size: String,
    #[serde(default = "default_wal_autocheckpoint")]
    pub wal_autocheckpoint: u32,
    #[serde(default = "default_journal_mode")]
    pub journal_mode: String,
    #[serde(default = "default_synchronous")]
    pub synchronous: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgreSqlConfig {
    #[serde(default = "default_statement_timeout")]
    pub statement_timeout: Option<String>,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout: Option<String>,
    #[serde(default = "default_max_lifetime")]
    pub max_lifetime: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MySqlConfig {
    #[serde(default = "default_wait_timeout")]
    pub wait_timeout: Option<u32>,
    #[serde(default = "default_interactive_timeout")]
    pub interactive_timeout: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseBatchConfig {
    /// Maximum number of EPG programs to insert in a single batch
    /// Each program has 12 fields, so batch_size * 12 must be <= SQLite variable limit
    pub epg_programs: Option<usize>,
    /// Maximum number of stream channels to process in a single chunk
    pub stream_channels: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_request_timeout")]
    pub request_timeout: String,
    #[serde(default = "default_max_request_size")]
    pub max_request_size: String,
    #[serde(default = "default_enable_request_logging")]
    pub enable_request_logging: bool,
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageConfig {
    #[serde(default = "default_m3u_path")]
    pub m3u_path: PathBuf,
    #[serde(default = "default_m3u_retention")]
    pub m3u_retention: String,
    #[serde(default = "default_m3u_cleanup_interval")]
    pub m3u_cleanup_interval: String,
    
    #[serde(default = "default_uploaded_logo_path")]
    pub uploaded_logo_path: PathBuf,
    
    #[serde(default = "default_cached_logo_path")]
    pub cached_logo_path: PathBuf,
    #[serde(default = "default_cached_logo_retention")]
    pub cached_logo_retention: String,
    #[serde(default = "default_cached_logo_cleanup_interval")]
    pub cached_logo_cleanup_interval: String,
    
    
    #[serde(default = "default_temp_path")]
    pub temp_path: Option<String>,
    #[serde(default = "default_temp_retention")]
    pub temp_retention: String,
    #[serde(default = "default_temp_cleanup_interval")]
    pub temp_cleanup_interval: String,
    
    #[serde(default = "default_pipeline_path")]
    pub pipeline_path: PathBuf,
    #[serde(default = "default_pipeline_retention")]
    pub pipeline_retention: String,
    #[serde(default = "default_pipeline_cleanup_interval")]
    pub pipeline_cleanup_interval: String,

}


// Web defaults
fn default_host() -> String {
    DEFAULT_HOST.to_string()
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_request_timeout() -> String {
    "30s".to_string()
}

fn default_max_request_size() -> String {
    "10MB".to_string()
}

fn default_base_url() -> String {
    "http://localhost:8080".to_string()
}

fn default_enable_request_logging() -> bool {
    false
}

fn default_user_agent() -> String {
    format!("{}/{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

fn default_busy_timeout() -> String {
    "5000".to_string()
}

fn default_cache_size() -> String {
    "-64000".to_string()
}

fn default_wal_autocheckpoint() -> u32 {
    1000
}

fn default_journal_mode() -> String {
    "WAL".to_string()
}

fn default_synchronous() -> String {
    "NORMAL".to_string()
}

fn default_statement_timeout() -> Option<String> {
    Some("30s".to_string())
}

fn default_idle_timeout() -> Option<String> {
    Some("10m".to_string())
}

fn default_max_lifetime() -> Option<String> {
    Some("30m".to_string())
}

fn default_wait_timeout() -> Option<u32> {
    Some(28800)
}

fn default_interactive_timeout() -> Option<u32> {
    Some(28800)
}






fn default_analyzeduration() -> String {
    "10s".to_string()
}

fn default_probesize() -> String {
    "10MB".to_string()
}


// Storage defaults
fn default_m3u_path() -> PathBuf {
    PathBuf::from(DEFAULT_M3U_PATH)
}

fn default_uploaded_logo_path() -> PathBuf {
    PathBuf::from(DEFAULT_UPLOADED_LOGO_PATH)
}

fn default_cached_logo_path() -> PathBuf {
    PathBuf::from(DEFAULT_CACHED_LOGO_PATH)
}


fn default_temp_path() -> Option<String> {
    Some(DEFAULT_TEMP_PATH.to_string())
}

fn default_pipeline_path() -> PathBuf {
    PathBuf::from(DEFAULT_PIPELINE_PATH)
}


// Storage retention defaults
fn default_m3u_retention() -> String {
    "30d".to_string()
}

fn default_m3u_cleanup_interval() -> String {
    "4h".to_string()
}

fn default_cached_logo_retention() -> String {
    "90d".to_string()
}

fn default_cached_logo_cleanup_interval() -> String {
    "12h".to_string()
}

fn default_temp_retention() -> String {
    "5m".to_string()
}

fn default_temp_cleanup_interval() -> String {
    "1m".to_string()
}

fn default_pipeline_retention() -> String {
    "10m".to_string()  // Slightly longer than temp for pipeline execution
}

fn default_pipeline_cleanup_interval() -> String {
    "2m".to_string()   // Less frequent than temp cleanup
}

// Ingestion defaults
fn default_progress_update_interval() -> usize {
    DEFAULT_PROGRESS_UPDATE_INTERVAL
}

fn default_run_missed_immediately() -> bool {
    DEFAULT_RUN_MISSED_IMMEDIATELY
}

fn default_use_new_source_handlers() -> bool {
    true // Default to new source handlers
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionConfig {
    #[serde(default = "default_progress_update_interval")]
    pub progress_update_interval: usize,
    #[serde(default = "default_run_missed_immediately")]
    pub run_missed_immediately: bool,
    /// Whether to use new source handlers instead of legacy ingestors
    #[serde(default = "default_use_new_source_handlers")]
    pub use_new_source_handlers: bool,
}

impl Default for IngestionConfig {
    fn default() -> Self {
        Self {
            progress_update_interval: default_progress_update_interval(),
            run_missed_immediately: default_run_missed_immediately(),
            use_new_source_handlers: default_use_new_source_handlers(),
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingEngineConfig {
    /// Special characters used for regex precheck filtering
    /// These characters are considered significant enough to use as first-pass filters
    /// Default: "+-@#$%&*=<>!~`€£{}[]"
    pub precheck_special_chars: Option<String>,
    /// Minimum length required for literal strings in regex precheck
    /// Set to 0 to disable literal string precheck entirely
    /// Default: 2
    pub minimum_literal_length: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Maximum allowed regex quantifier limit to prevent ReDoS attacks
    /// Default: 100
    #[serde(default = "default_max_quantifier_limit")]
    pub max_quantifier_limit: usize,
}

fn default_max_quantifier_limit() -> usize {
    100
}

/// Circuit breaker configuration with support for named profiles
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema, Default)]
pub struct CircuitBreakerConfig {
    /// Global circuit breaker settings that apply to all profiles unless overridden
    #[serde(default)]
    pub global: CircuitBreakerProfileConfig,
    
    /// Named circuit breaker profiles for different services
    /// Example: rssafe, database, http_client
    #[serde(default)]
    pub profiles: std::collections::HashMap<String, CircuitBreakerProfileConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CircuitBreakerProfileConfig {
    /// Circuit breaker implementation type: "rssafe" or "noop"
    #[serde(default = "default_circuit_breaker_type")]
    pub implementation_type: String,
    
    /// Number of consecutive failures before opening the circuit
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
    
    /// Timeout duration for individual operations (e.g., "5s", "30s")
    #[serde(default = "default_operation_timeout")]
    pub operation_timeout: String,
    
    /// How long to wait before attempting to close the circuit (e.g., "30s", "1m")
    #[serde(default = "default_reset_timeout")]
    pub reset_timeout: String,
    
    /// Number of consecutive successes needed to close circuit from half-open state
    #[serde(default = "default_success_threshold")]
    pub success_threshold: u32,
    
    /// HTTP status codes that should NOT trigger circuit breaker failures
    /// Supports wildcards: "2xx" matches 200-299, "404" matches exact, etc.
    /// Default includes all 2xx responses and common expected errors
    #[serde(default = "default_acceptable_status_codes")]
    pub acceptable_status_codes: Vec<String>,
}

fn default_circuit_breaker_type() -> String {
    "simple".to_string()
}

fn default_failure_threshold() -> u32 {
    3
}

fn default_operation_timeout() -> String {
    "5s".to_string()
}

fn default_reset_timeout() -> String {
    "30s".to_string()
}

fn default_success_threshold() -> u32 {
    2
}

fn default_acceptable_status_codes() -> Vec<String> {
    vec!["2xx".to_string(), "3xx".to_string()]
}

impl Default for CircuitBreakerProfileConfig {
    fn default() -> Self {
        Self {
            implementation_type: default_circuit_breaker_type(),
            failure_threshold: default_failure_threshold(),
            operation_timeout: default_operation_timeout(),
            reset_timeout: default_reset_timeout(),
            success_threshold: default_success_threshold(),
            acceptable_status_codes: default_acceptable_status_codes(),
        }
    }
}



impl DatabaseBatchConfig {
    /// SQLite variable limit (32,766 in 3.32.0+, 999 in older versions)
    const SQLITE_MAX_VARIABLES: usize = 32766;
    
    /// PostgreSQL variable limit (65,535 parameters per query)
    const POSTGRES_MAX_VARIABLES: usize = 65535;
    
    /// MySQL variable limit (65,535 placeholders per prepared statement)
    const MYSQL_MAX_VARIABLES: usize = 65535;

    /// Number of fields per EPG program record
    const EPG_PROGRAM_FIELDS: usize = 12;

    /// Number of fields per stream channel record  
    /// (id, source_id, tvg_id, tvg_name, tvg_chno, channel_name, tvg_logo, tvg_shift, group_title, stream_url, created_at, updated_at)
    const STREAM_CHANNEL_FIELDS: usize = 12;

    /// Validate batch sizes to ensure they don't exceed SQLite limits
    pub fn validate(&self) -> Result<(), String> {
        if let Some(epg_programs) = self.epg_programs {
            let variables = epg_programs * Self::EPG_PROGRAM_FIELDS;
            if variables > Self::SQLITE_MAX_VARIABLES {
                return Err(format!(
                    "EPG program batch size {} would require {} variables, exceeding SQLite limit of {}",
                    epg_programs,
                    variables,
                    Self::SQLITE_MAX_VARIABLES
                ));
            }
        }

        if let Some(stream_channels) = self.stream_channels {
            let variables = stream_channels * Self::STREAM_CHANNEL_FIELDS;
            if variables > Self::SQLITE_MAX_VARIABLES {
                return Err(format!(
                    "Stream channel batch size {} would require {} variables, exceeding SQLite limit of {}",
                    stream_channels,
                    variables,
                    Self::SQLITE_MAX_VARIABLES
                ));
            }
        }

        Ok(())
    }

    /// Get safe batch size for EPG programs based on database backend
    pub fn safe_epg_program_batch_size(&self, backend: sea_orm::DatabaseBackend) -> usize {
        let default_size = match backend {
            sea_orm::DatabaseBackend::Sqlite => 2000,    // Conservative for SQLite
            sea_orm::DatabaseBackend::Postgres => 4000,  // More aggressive for PostgreSQL
            sea_orm::DatabaseBackend::MySql => 3000,     // Moderate for MySQL
        };
        
        let configured = self.epg_programs.unwrap_or(default_size);
        let max_safe = match backend {
            sea_orm::DatabaseBackend::Sqlite => Self::SQLITE_MAX_VARIABLES / Self::EPG_PROGRAM_FIELDS,
            sea_orm::DatabaseBackend::Postgres => Self::POSTGRES_MAX_VARIABLES / Self::EPG_PROGRAM_FIELDS,
            sea_orm::DatabaseBackend::MySql => Self::MYSQL_MAX_VARIABLES / Self::EPG_PROGRAM_FIELDS,
        };
        
        configured.min(max_safe)
    }

    /// Get safe batch size for EPG programs (legacy SQLite-only method for compatibility)
    pub fn safe_epg_program_batch_size_legacy(&self) -> usize {
        self.safe_epg_program_batch_size(sea_orm::DatabaseBackend::Sqlite)
    }

    /// Get safe batch size for stream channels based on database backend
    pub fn safe_stream_channel_batch_size(&self, backend: sea_orm::DatabaseBackend) -> usize {
        let default_size = match backend {
            sea_orm::DatabaseBackend::Sqlite => 2000,    // Conservative for SQLite
            sea_orm::DatabaseBackend::Postgres => 4000,  // More aggressive for PostgreSQL
            sea_orm::DatabaseBackend::MySql => 3000,     // Moderate for MySQL
        };
        
        let configured = self.stream_channels.unwrap_or(default_size);
        let max_safe = match backend {
            sea_orm::DatabaseBackend::Sqlite => Self::SQLITE_MAX_VARIABLES / Self::STREAM_CHANNEL_FIELDS,
            sea_orm::DatabaseBackend::Postgres => Self::POSTGRES_MAX_VARIABLES / Self::STREAM_CHANNEL_FIELDS,
            sea_orm::DatabaseBackend::MySql => Self::MYSQL_MAX_VARIABLES / Self::STREAM_CHANNEL_FIELDS,
        };
        
        configured.min(max_safe)
    }
}

impl Default for DatabaseBatchConfig {
    fn default() -> Self {
        Self {
            // Database-agnostic default - will be adjusted per backend in safe_epg_program_batch_size()
            // This default will be overridden based on actual database backend used
            epg_programs: None, // Use backend-specific defaults
            // Stream channels: conservative default for all databases
            stream_channels: Some(1000),
        }
    }
}


impl Default for SqliteConfig {
    fn default() -> Self {
        Self {
            busy_timeout: default_busy_timeout(),
            cache_size: default_cache_size(),
            wal_autocheckpoint: default_wal_autocheckpoint(),
            journal_mode: default_journal_mode(),
            synchronous: default_synchronous(),
        }
    }
}

impl Default for PostgreSqlConfig {
    fn default() -> Self {
        Self {
            statement_timeout: default_statement_timeout(),
            idle_timeout: default_idle_timeout(),
            max_lifetime: default_max_lifetime(),
        }
    }
}

impl Default for MySqlConfig {
    fn default() -> Self {
        Self {
            wait_timeout: default_wait_timeout(),
            interactive_timeout: default_interactive_timeout(),
        }
    }
}

impl Default for DataMappingEngineConfig {
    fn default() -> Self {
        Self {
            precheck_special_chars: Some("+-@#$%&*=<>!~`€£{}[].".to_string()),
            minimum_literal_length: Some(2),
        }
    }
}


/// Relay system configuration for FFmpeg-based stream processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    /// FFmpeg command to use for relay operations
    /// Can be a full path (/usr/bin/ffmpeg) or command name (ffmpeg)
    /// The system will search $PATH if not a full path
    #[serde(default = "default_ffmpeg_command")]
    pub ffmpeg_command: String,
    
    /// FFprobe command to use for stream probing operations
    /// Can be a full path (/usr/bin/ffprobe) or command name (ffprobe)
    /// The system will search $PATH if not a full path
    #[serde(default = "default_ffprobe_command")]
    pub ffprobe_command: String,
    
    /// Stream analysis duration for FFmpeg
    #[serde(default = "default_analyzeduration")]
    pub analyzeduration: String,
    
    /// Stream probe size for FFmpeg
    #[serde(default = "default_probesize")]
    pub probesize: String,
    
    /// Cyclic buffer configuration for in-memory stream buffering
    #[serde(default)]
    pub buffer: BufferConfig,
}

fn default_ffmpeg_command() -> String {
    "ffmpeg".to_string()
}

fn default_ffprobe_command() -> String {
    "ffprobe".to_string()
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            ffmpeg_command: default_ffmpeg_command(),
            ffprobe_command: default_ffprobe_command(),
            analyzeduration: default_analyzeduration(),
            probesize: default_probesize(),
            buffer: BufferConfig::default(),
        }
    }
}

/// Configuration for relay cyclic buffer system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferConfig {
    /// Maximum buffer size in bytes (default: 50MB)
    #[serde(default = "default_max_buffer_size")]
    pub max_buffer_size: usize,
    
    /// Maximum number of chunks to keep in memory (default: 1000)
    #[serde(default = "default_max_chunks")]
    pub max_chunks: usize,
    
    /// How long to keep chunks in memory in seconds (default: 60)
    #[serde(default = "default_chunk_timeout_seconds")]
    pub chunk_timeout_seconds: u64,
    
    /// How long to wait for slow clients in seconds (default: 30)
    #[serde(default = "default_client_timeout_seconds")]
    pub client_timeout_seconds: u64,
    
    /// How often to cleanup old chunks in seconds (default: 5)
    #[serde(default = "default_cleanup_interval_seconds")]
    pub cleanup_interval_seconds: u64,
    
    /// Enable file spill to disk when buffer is full (default: false)
    #[serde(default = "default_enable_file_spill")]
    pub enable_file_spill: bool,
    
    /// Maximum file spill size in bytes (default: 500MB)
    #[serde(default = "default_max_file_spill_size")]
    pub max_file_spill_size: usize,
}

fn default_max_buffer_size() -> usize { 50 * 1024 * 1024 }  // 50MB
fn default_max_chunks() -> usize { 1000 }
fn default_chunk_timeout_seconds() -> u64 { 60 }
fn default_client_timeout_seconds() -> u64 { 30 }
fn default_cleanup_interval_seconds() -> u64 { 5 }
fn default_enable_file_spill() -> bool { false }
fn default_max_file_spill_size() -> usize { 500 * 1024 * 1024 }  // 500MB

impl Default for BufferConfig {
    fn default() -> Self {
        Self {
            max_buffer_size: default_max_buffer_size(),
            max_chunks: default_max_chunks(),
            chunk_timeout_seconds: default_chunk_timeout_seconds(),
            client_timeout_seconds: default_client_timeout_seconds(),
            cleanup_interval_seconds: default_cleanup_interval_seconds(),
            enable_file_spill: default_enable_file_spill(),
            max_file_spill_size: default_max_file_spill_size(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database: DatabaseConfig {
                url: "sqlite:./data/m3u-proxy.db".to_string(),
                max_connections: Some(10),
                batch_sizes: Some(DatabaseBatchConfig::default()),
                sqlite: SqliteConfig::default(),
                postgresql: PostgreSqlConfig::default(),
                mysql: MySqlConfig::default(),
            },
            web: WebConfig {
                host: "0.0.0.0".to_string(),
                port: 8080,
                base_url: "http://localhost:8080".to_string(),
                request_timeout: default_request_timeout(),
                max_request_size: default_max_request_size(),
                enable_request_logging: default_enable_request_logging(),
                user_agent: default_user_agent(),
            },
            storage: StorageConfig {
                m3u_path: PathBuf::from("./data/m3u"),
                m3u_retention: "30d".to_string(),
                m3u_cleanup_interval: "4h".to_string(),
                uploaded_logo_path: PathBuf::from("./data/logos/uploaded"),
                cached_logo_path: PathBuf::from("./data/logos/cached"),
                cached_logo_retention: "90d".to_string(),
                cached_logo_cleanup_interval: "12h".to_string(),
                temp_path: Some("./data/temp".to_string()),
                temp_retention: "5m".to_string(),
                temp_cleanup_interval: "1m".to_string(),
                pipeline_path: PathBuf::from("./data/pipeline"),
                pipeline_retention: "10m".to_string(),
                pipeline_cleanup_interval: "2m".to_string(),
            },
            ingestion: IngestionConfig {
                progress_update_interval: 1000,
                run_missed_immediately: true,
                use_new_source_handlers: default_use_new_source_handlers(),
            },
            data_mapping_engine: Some(DataMappingEngineConfig::default()),
            relay: Some(RelayConfig::default()),
            security: Some(SecurityConfig {
                max_quantifier_limit: default_max_quantifier_limit(),
            }),
            features: Some(FeaturesConfig::default()),
            circuitbreaker: Some(CircuitBreakerConfig::default()),
        }
    }
}


impl Config {
    pub fn load() -> Result<Self> {
        let config_file =
            std::env::var("CONFIG_FILE").unwrap_or_else(|_| "config.toml".to_string());
        Self::load_from_file(&config_file)
    }

    pub fn load_from_file(config_file: &str) -> Result<Self> {
        // Check if config file exists
        if !std::path::Path::new(config_file).exists() {
            tracing::warn!("Config file '{}' not found, using default configuration values", config_file);
            
            // Start with default config and merge environment variables
            let default_config = Self::default();
            let config: Config = Figment::new()
                .merge(figment::providers::Serialized::defaults(default_config))
                .merge(Env::prefixed("M3U_PROXY_").split("__"))
                .extract()?;
                
            return Ok(config);
        }
        
        // Load config with figment (TOML file + environment variables)
        let config: Config = Figment::new()
            .merge(Toml::file(config_file))
            .merge(Env::prefixed("M3U_PROXY_").split("__"))
            .extract()?;
            
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_batch_config_validation() {
        // Test valid configuration (within SQLite limits)
        let valid_config = DatabaseBatchConfig {
            epg_programs: Some(1800), // 1800 * 18 = 32,400 variables (within 32,766 limit)
            stream_channels: Some(500),
        };

        assert!(valid_config.validate().is_ok());
    }

    #[test]
    fn test_safe_batch_sizes() {
        let config = DatabaseBatchConfig {
            epg_programs: Some(3000), // Too large, should be capped
            stream_channels: Some(1000),
        };

        // Should return safe sizes within SQLite limits
        let safe_programs = config.safe_epg_program_batch_size(sea_orm::DatabaseBackend::Sqlite);

        assert!(safe_programs * 12 <= 32766); // Use actual EPG_PROGRAM_FIELDS constant

        // Should cap to maximum safe values  
        assert_eq!(safe_programs, 32766 / 12); // 2730 (using actual EPG_PROGRAM_FIELDS constant)
    }

    #[test]
    fn test_default_batch_config() {
        let default_config = DatabaseBatchConfig::default();

        // Default values should be valid
        assert!(default_config.validate().is_ok());

        // Default values should be within safe limits
        assert_eq!(default_config.epg_programs, None); // Uses backend-specific defaults
        assert_eq!(default_config.stream_channels, Some(1000));
    }
}
