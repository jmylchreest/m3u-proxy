/// Configuration default values
/// 
/// This module contains all the default values for configuration options,
/// making them easily changeable in one central location.
// Database defaults
pub const DEFAULT_DATABASE_URL: &str = "sqlite://./m3u-proxy.db";
pub const DEFAULT_MAX_CONNECTIONS: u32 = 10;
pub const DEFAULT_EPG_CHANNELS_BATCH_SIZE: usize = 3600;
pub const DEFAULT_EPG_PROGRAMS_BATCH_SIZE: usize = 1800;
pub const DEFAULT_STREAM_CHANNELS_BATCH_SIZE: usize = 500;

// Web server defaults
pub const DEFAULT_HOST: &str = "0.0.0.0";
pub const DEFAULT_PORT: u16 = 8080;
// Note: base_url is the ONLY truly mandatory field with no default

// Storage defaults
pub const DEFAULT_M3U_PATH: &str = "./data/m3u";
pub const DEFAULT_UPLOADED_LOGO_PATH: &str = "./data/logos/uploaded";
pub const DEFAULT_CACHED_LOGO_PATH: &str = "./data/logos/cached";
pub const DEFAULT_TEMP_PATH: &str = "./data/temp";
pub const DEFAULT_PIPELINE_PATH: &str = "./data/pipeline";
pub const DEFAULT_PROXY_VERSIONS_TO_KEEP: u32 = 3;
pub const DEFAULT_CLEAN_ORPHAN_LOGOS: bool = true;

// Ingestion defaults
pub const DEFAULT_PROGRESS_UPDATE_INTERVAL: usize = 1000;
pub const DEFAULT_RUN_MISSED_IMMEDIATELY: bool = true;


// Data mapping engine defaults
pub const DEFAULT_PRECHECK_SPECIAL_CHARS: &str = "+-@#$%&*=<>!~`€£{}[].";
pub const DEFAULT_MINIMUM_LITERAL_LENGTH: usize = 2;

// Proxy generation defaults
pub const DEFAULT_MAX_MEMORY_MB: usize = 512;
pub const DEFAULT_BATCH_SIZE: usize = 1000;
pub const DEFAULT_ENABLE_PARALLEL_PROCESSING: bool = true;
pub const DEFAULT_MEMORY_CHECK_INTERVAL: usize = 100;
pub const DEFAULT_WARNING_THRESHOLD: f64 = 0.8;
pub const DEFAULT_ENABLE_MEMORY_TRACKING: bool = true;
pub const DEFAULT_ENABLE_PROFILING: bool = false;

// EPG generation defaults
pub const DEFAULT_INCLUDE_PAST_PROGRAMS: bool = false;
pub const DEFAULT_DAYS_AHEAD: u32 = 7;
pub const DEFAULT_DAYS_BEHIND: u32 = 1;
pub const DEFAULT_DEDUPLICATE_PROGRAMS: bool = true;
pub const DEFAULT_MAX_PROGRAMS_PER_CHANNEL: usize = 1000;

// Metrics defaults
pub const DEFAULT_RAW_LOG_RETENTION: &str = "7d";
pub const DEFAULT_HOURLY_STATS_RETENTION: &str = "30d";
pub const DEFAULT_DAILY_STATS_RETENTION: &str = "365d";
pub const DEFAULT_SESSION_TIMEOUT: &str = "15s";
pub const DEFAULT_HOUSEKEEPER_INTERVAL: &str = "1m";

// Relay system defaults
pub const DEFAULT_FFMPEG_COMMAND: &str = "ffmpeg";
pub const DEFAULT_MAX_BUFFER_SIZE: usize = 50 * 1024 * 1024; // 50MB
pub const DEFAULT_MAX_CHUNKS: usize = 1000;
pub const DEFAULT_CHUNK_TIMEOUT_SECONDS: u64 = 60;
pub const DEFAULT_CLIENT_TIMEOUT_SECONDS: u64 = 30;
pub const DEFAULT_CLEANUP_INTERVAL_SECONDS: u64 = 5;
pub const DEFAULT_ENABLE_FILE_SPILL: bool = false;
pub const DEFAULT_MAX_FILE_SPILL_SIZE: usize = 500 * 1024 * 1024; // 500MB