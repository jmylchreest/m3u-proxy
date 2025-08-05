use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

pub mod defaults;
pub mod duration_serde;
pub mod file_categories;

use defaults::*;

/// Pipeline configuration for processing optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// Maximum memory usage in MB before spilling to disk
    pub max_memory_mb: Option<usize>,
    /// Pipeline suspension duration when errors occur
    #[serde(default = "default_suspension_duration")]
    pub suspension_duration: String,
    /// Logo processing batch size
    #[serde(default = "default_logo_batch_size")]
    pub logo_batch_size: usize,
    /// Channel processing batch size
    #[serde(default = "default_channel_batch_size")]
    pub channel_batch_size: usize,
    /// EPG processing batch size
    #[serde(default = "default_epg_batch_size")]
    pub epg_batch_size: usize,
    /// Retry attempts for failed operations
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

/// Metrics configuration for stream access tracking and retention
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Retention period for raw access logs
    #[serde(default = "default_raw_log_retention")]
    pub raw_log_retention: String,
    
    /// Retention period for hourly aggregates
    #[serde(default = "default_hourly_stats_retention")]
    pub hourly_stats_retention: String,
    
    /// Retention period for daily aggregates
    #[serde(default = "default_daily_stats_retention")]
    pub daily_stats_retention: String,
    
    /// Client session timeout
    #[serde(default = "default_session_timeout")]
    pub session_timeout: String,
    
    /// Housekeeper run interval
    #[serde(default = "default_housekeeper_interval")]
    pub housekeeper_interval: String,
}

fn default_raw_log_retention() -> String { "7d".to_string() }
fn default_hourly_stats_retention() -> String { "30d".to_string() }
fn default_daily_stats_retention() -> String { "365d".to_string() }
fn default_session_timeout() -> String { "15s".to_string() }
fn default_housekeeper_interval() -> String { "1m".to_string() }

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_memory_mb: Some(512), // 512MB default limit
            suspension_duration: default_suspension_duration(),
            logo_batch_size: default_logo_batch_size(),
            channel_batch_size: default_channel_batch_size(),
            epg_batch_size: default_epg_batch_size(),
            max_retries: default_max_retries(),
        }
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            raw_log_retention: default_raw_log_retention(),
            hourly_stats_retention: default_hourly_stats_retention(),
            daily_stats_retention: default_daily_stats_retention(),
            session_timeout: default_session_timeout(),
            housekeeper_interval: default_housekeeper_interval(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub database: DatabaseConfig,
    pub web: WebConfig,
    pub storage: StorageConfig,
    pub ingestion: IngestionConfig,
    pub data_mapping_engine: Option<DataMappingEngineConfig>,
    pub proxy_generation: Option<ProxyGenerationConfig>,
    pub pipeline: Option<PipelineConfig>,
    pub metrics: Option<MetricsConfig>,
    pub relay: Option<RelayConfig>,
    pub operational: Option<OperationalConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: Option<u32>,
    pub batch_sizes: Option<DatabaseBatchConfig>,
    #[serde(default = "default_busy_timeout")]
    pub busy_timeout: String,
    #[serde(default = "default_cache_size")]
    pub cache_size: String,
    #[serde(default = "default_wal_autocheckpoint")]
    pub wal_autocheckpoint: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseBatchConfig {
    /// Maximum number of EPG programs to insert in a single batch
    /// Each program has 12 fields, so batch_size * 12 must be <= SQLite variable limit
    pub epg_programs: Option<usize>,
    /// Maximum number of stream channels to process in a single chunk
    pub stream_channels: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub base_url: String, // This is the ONLY mandatory field
    #[serde(default = "default_request_timeout")]
    pub request_timeout: String,
    #[serde(default = "default_max_request_size")]
    pub max_request_size: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    
    #[serde(default = "default_proxy_versions_to_keep")]
    pub proxy_versions_to_keep: u32,
    
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

    /// Automatically clean up orphaned logo database entries when uploaded_logo_path changes
    #[serde(default = "default_clean_orphan_logos")]
    pub clean_orphan_logos: bool,
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

fn default_busy_timeout() -> String {
    "30s".to_string()
}

fn default_cache_size() -> String {
    "64MB".to_string()
}

fn default_wal_autocheckpoint() -> u32 {
    1000
}

fn default_suspension_duration() -> String {
    "5m".to_string()
}

fn default_logo_batch_size() -> usize {
    1000
}

fn default_channel_batch_size() -> usize {
    100
}

fn default_epg_batch_size() -> usize {
    1000
}

fn default_max_retries() -> u32 {
    3
}

fn default_analyzeduration() -> String {
    "10s".to_string()
}

fn default_probesize() -> String {
    "10MB".to_string()
}

fn default_log_buffer_size() -> usize {
    200
}

fn default_similarity_threshold() -> f64 {
    0.60
}

fn default_logo_progress_interval() -> usize {
    10
}

fn default_channel_debug_interval() -> usize {
    1000
}

fn default_epg_progress_interval() -> usize {
    10000
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

fn default_proxy_versions_to_keep() -> u32 {
    DEFAULT_PROXY_VERSIONS_TO_KEEP
}

fn default_temp_path() -> Option<String> {
    Some(DEFAULT_TEMP_PATH.to_string())
}

fn default_pipeline_path() -> PathBuf {
    PathBuf::from(DEFAULT_PIPELINE_PATH)
}

fn default_clean_orphan_logos() -> bool {
    DEFAULT_CLEAN_ORPHAN_LOGOS
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
pub struct ProxyGenerationConfig {
    /// Memory configuration for proxy generation
    pub memory: ProxyMemoryConfig,
    /// EPG generation configuration
    pub epg: EpgGenerationConfig,
    /// Enable detailed memory tracking and reporting
    pub enable_memory_tracking: bool,
    /// Enable performance profiling
    pub enable_profiling: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyMemoryConfig {
    /// Maximum memory usage in MB for proxy generation
    pub max_memory_mb: Option<usize>,
    /// Batch size for processing channels
    pub batch_size: usize,
    /// Enable parallel processing (uses more memory but faster)
    pub enable_parallel_processing: bool,
    /// Memory usage check interval (in number of processed items)
    pub memory_check_interval: usize,
    /// Warning threshold as percentage of max memory (0.0-1.0)
    pub warning_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgGenerationConfig {
    /// Include programs that have already ended
    pub include_past_programs: bool,
    /// Number of days of future EPG data to include
    pub days_ahead: u32,
    /// Number of days of past EPG data to include
    pub days_behind: u32,
    /// Remove duplicate programs with same title and time
    pub deduplicate_programs: bool,
    /// Maximum number of programs per channel (None for unlimited)
    pub max_programs_per_channel: Option<usize>,
    /// Time zone for EPG normalization (when normalize_to_utc is true)
    pub source_timezone: Option<String>,
}

impl DatabaseBatchConfig {
    /// SQLite variable limit (32,766 in 3.32.0+, 999 in older versions)
    const SQLITE_MAX_VARIABLES: usize = 32766;

    /// Number of fields per EPG program record
    const EPG_PROGRAM_FIELDS: usize = 18;

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

        Ok(())
    }

    /// Get safe batch size for EPG programs (respects SQLite limits)
    pub fn safe_epg_program_batch_size(&self) -> usize {
        let configured = self.epg_programs.unwrap_or(1900);
        let max_safe = Self::SQLITE_MAX_VARIABLES / Self::EPG_PROGRAM_FIELDS;
        configured.min(max_safe)
    }
}

impl Default for DatabaseBatchConfig {
    fn default() -> Self {
        Self {
            // SQLite 3.32.0+ supports up to 32,766 variables per query
            // EPG programs: 18 fields * 1800 = 32,400 variables (safe margin)
            epg_programs: Some(1800),
            // Stream channels: reduced for better SQLite performance with large datasets
            stream_channels: Some(500),
        }
    }
}

/// Operational configuration for logging and system behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalConfig {
    /// Log buffer size for in-memory logging
    #[serde(default = "default_log_buffer_size")]
    pub log_buffer_size: usize,
    /// Similarity threshold for expression matching (0.0-1.0)
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f64,
    /// Progress reporting intervals for various operations
    pub progress_intervals: Option<ProgressIntervalConfig>,
}

/// Progress interval configuration for UI responsiveness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressIntervalConfig {
    /// Logo progress batch interval
    #[serde(default = "default_logo_progress_interval")]
    pub logo_progress_interval: usize,
    /// Channel debug interval
    #[serde(default = "default_channel_debug_interval")]
    pub channel_debug_interval: usize,
    /// EPG progress log interval
    #[serde(default = "default_epg_progress_interval")]
    pub epg_progress_interval: usize,
}

impl Default for DataMappingEngineConfig {
    fn default() -> Self {
        Self {
            precheck_special_chars: Some("+-@#$%&*=<>!~`€£{}[].".to_string()),
            minimum_literal_length: Some(2),
        }
    }
}

impl Default for OperationalConfig {
    fn default() -> Self {
        Self {
            log_buffer_size: default_log_buffer_size(),
            similarity_threshold: default_similarity_threshold(),
            progress_intervals: Some(ProgressIntervalConfig::default()),
        }
    }
}

impl Default for ProgressIntervalConfig {
    fn default() -> Self {
        Self {
            logo_progress_interval: default_logo_progress_interval(),
            channel_debug_interval: default_channel_debug_interval(),
            epg_progress_interval: default_epg_progress_interval(),
        }
    }
}

impl Default for ProxyGenerationConfig {
    fn default() -> Self {
        Self {
            memory: ProxyMemoryConfig::default(),
            epg: EpgGenerationConfig::default(),
            enable_memory_tracking: true,
            enable_profiling: false,
        }
    }
}

impl Default for ProxyMemoryConfig {
    fn default() -> Self {
        Self {
            max_memory_mb: Some(512), // 512MB default limit
            batch_size: 1000,
            enable_parallel_processing: true,
            memory_check_interval: 100,
            warning_threshold: 0.8, // Warn at 80% of limit
        }
    }
}

impl Default for EpgGenerationConfig {
    fn default() -> Self {
        Self {
            include_past_programs: false,
            days_ahead: 7,
            days_behind: 1,
            deduplicate_programs: true,
            max_programs_per_channel: Some(1000),
            source_timezone: None, // Auto-detect from EPG source
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
                url: "sqlite://./m3u-proxy.db".to_string(),
                max_connections: Some(10),
                batch_sizes: Some(DatabaseBatchConfig::default()),
                busy_timeout: default_busy_timeout(),
                cache_size: default_cache_size(),
                wal_autocheckpoint: default_wal_autocheckpoint(),
            },
            web: WebConfig {
                host: "0.0.0.0".to_string(),
                port: 8080,
                base_url: "http://localhost:8080".to_string(),
                request_timeout: default_request_timeout(),
                max_request_size: default_max_request_size(),
            },
            storage: StorageConfig {
                m3u_path: PathBuf::from("./data/m3u"),
                m3u_retention: "30d".to_string(),
                m3u_cleanup_interval: "4h".to_string(),
                uploaded_logo_path: PathBuf::from("./data/logos/uploaded"),
                cached_logo_path: PathBuf::from("./data/logos/cached"),
                cached_logo_retention: "90d".to_string(),
                cached_logo_cleanup_interval: "12h".to_string(),
                proxy_versions_to_keep: 3,
                temp_path: Some("./data/temp".to_string()),
                temp_retention: "5m".to_string(),
                temp_cleanup_interval: "1m".to_string(),
                pipeline_path: PathBuf::from("./data/pipeline"),
                pipeline_retention: "10m".to_string(),
                pipeline_cleanup_interval: "2m".to_string(),
                clean_orphan_logos: true,
            },
            ingestion: IngestionConfig {
                progress_update_interval: 1000,
                run_missed_immediately: true,
                use_new_source_handlers: default_use_new_source_handlers(),
            },
            data_mapping_engine: Some(DataMappingEngineConfig::default()),
            proxy_generation: Some(ProxyGenerationConfig::default()),
            pipeline: Some(PipelineConfig::default()),
            metrics: Some(MetricsConfig::default()),
            relay: Some(RelayConfig::default()),
            operational: Some(OperationalConfig::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_batch_config_validation() {
        // Test valid configuration
        let valid_config = DatabaseBatchConfig {
            epg_programs: Some(1900), // 1900 * 18 = 34,200 variables (exceeds 32,766, should be capped)
            stream_channels: Some(1000),
        };
        assert!(valid_config.validate().is_ok());

        // Test EPG programs exceeding limit
        let invalid_programs = DatabaseBatchConfig {
            epg_programs: Some(2000), // 2000 * 18 = 36,000 variables (exceeds 32,766)
            stream_channels: Some(1000),
        };
        assert!(invalid_programs.validate().is_err());
    }

    #[test]
    fn test_safe_batch_sizes() {
        let config = DatabaseBatchConfig {
            epg_programs: Some(3000), // Too large, should be capped
            stream_channels: Some(1000),
        };

        // Should return safe sizes within SQLite limits
        let safe_programs = config.safe_epg_program_batch_size();

        assert!(safe_programs * 18 <= 32766);

        // Should cap to maximum safe values
        assert_eq!(safe_programs, 32766 / 18); // 1820
    }

    #[test]
    fn test_default_batch_config() {
        let default_config = DatabaseBatchConfig::default();

        // Default values should be valid
        assert!(default_config.validate().is_ok());

        // Default values should be within safe limits
        assert_eq!(default_config.epg_programs, Some(1800));
        assert_eq!(default_config.stream_channels, Some(500));
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_file =
            std::env::var("CONFIG_FILE").unwrap_or_else(|_| "config.toml".to_string());
        Self::load_from_file(&config_file)
    }

    pub fn load_from_file(config_file: &str) -> Result<Self> {
        if std::path::Path::new(&config_file).exists() {
            let contents = std::fs::read_to_string(&config_file)?;
            Ok(toml::from_str(&contents)?)
        } else {
            let default_config = Self::default();
            let contents = toml::to_string_pretty(&default_config)?;
            std::fs::write(&config_file, contents)?;
            info!("Created default config file: {}", config_file);
            Ok(default_config)
        }
    }
}
