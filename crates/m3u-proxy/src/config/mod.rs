use anyhow::Result;
use figment::{Figment, providers::{Toml, Env, Format}};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

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

fn default_busy_timeout() -> String {
    "5000".to_string()
}

fn default_cache_size() -> String {
    "-64000".to_string()
}

fn default_wal_autocheckpoint() -> u32 {
    1000
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
        // Create default config file if it doesn't exist
        if !std::path::Path::new(&config_file).exists() {
            let default_config = Self::default();
            let contents = toml::to_string_pretty(&default_config)?;
            std::fs::write(&config_file, contents)?;
            info!("Created default config file: {}", config_file);
        }
        
        // Load config with figment (TOML file + environment variables)
        let config: Config = Figment::new()
            .merge(Toml::file(&config_file))
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
