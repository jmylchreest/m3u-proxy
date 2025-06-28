use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub database: DatabaseConfig,
    pub web: WebConfig,
    pub storage: StorageConfig,
    pub ingestion: IngestionConfig,
    pub display: Option<DisplayConfig>,
    pub data_mapping_engine: Option<DataMappingEngineConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: Option<u32>,
    pub batch_sizes: Option<DatabaseBatchConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseBatchConfig {
    /// Maximum number of EPG channels to insert in a single batch
    /// Each channel has 9 fields, so batch_size * 9 must be <= SQLite variable limit
    pub epg_channels: Option<usize>,
    /// Maximum number of EPG programs to insert in a single batch
    /// Each program has 17 fields, so batch_size * 17 must be <= SQLite variable limit
    pub epg_programs: Option<usize>,
    /// Maximum number of stream channels to process in a single chunk
    pub stream_channels: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    pub host: String,
    pub port: u16,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub m3u_path: PathBuf,
    pub uploaded_logo_path: PathBuf,
    pub cached_logo_path: PathBuf,
    pub proxy_versions_to_keep: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionConfig {
    pub progress_update_interval: usize,
    pub run_missed_immediately: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    pub local_timezone: String,
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

    /// Number of fields per EPG channel record
    const EPG_CHANNEL_FIELDS: usize = 9;

    /// Number of fields per EPG program record
    const EPG_PROGRAM_FIELDS: usize = 17;

    /// Validate batch sizes to ensure they don't exceed SQLite limits
    pub fn validate(&self) -> Result<(), String> {
        if let Some(epg_channels) = self.epg_channels {
            let variables = epg_channels * Self::EPG_CHANNEL_FIELDS;
            if variables > Self::SQLITE_MAX_VARIABLES {
                return Err(format!(
                    "EPG channel batch size {} would require {} variables, exceeding SQLite limit of {}",
                    epg_channels, variables, Self::SQLITE_MAX_VARIABLES
                ));
            }
        }

        if let Some(epg_programs) = self.epg_programs {
            let variables = epg_programs * Self::EPG_PROGRAM_FIELDS;
            if variables > Self::SQLITE_MAX_VARIABLES {
                return Err(format!(
                    "EPG program batch size {} would require {} variables, exceeding SQLite limit of {}",
                    epg_programs, variables, Self::SQLITE_MAX_VARIABLES
                ));
            }
        }

        Ok(())
    }

    /// Get safe batch size for EPG channels (respects SQLite limits)
    pub fn safe_epg_channel_batch_size(&self) -> usize {
        let configured = self.epg_channels.unwrap_or(3600);
        let max_safe = Self::SQLITE_MAX_VARIABLES / Self::EPG_CHANNEL_FIELDS;
        configured.min(max_safe)
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
            // EPG channels: 9 fields * 3600 = 32,400 variables (safe margin)
            epg_channels: Some(3600),
            // EPG programs: 17 fields * 1900 = 32,300 variables (safe margin)
            epg_programs: Some(1900),
            // Stream channels: optimized for performance (not variable-limited)
            stream_channels: Some(1000),
        }
    }
}

impl Default for DataMappingEngineConfig {
    fn default() -> Self {
        Self {
            precheck_special_chars: Some("+-@#$%&*=<>!~`€£{}[]".to_string()),
            minimum_literal_length: Some(2),
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
            },
            web: WebConfig {
                host: "0.0.0.0".to_string(),
                port: 8080,
                base_url: "http://localhost:8080".to_string(),
            },
            storage: StorageConfig {
                m3u_path: PathBuf::from("./data/m3u"),
                uploaded_logo_path: PathBuf::from("./data/logos/uploaded"),
                cached_logo_path: PathBuf::from("./data/logos/cached"),
                proxy_versions_to_keep: 3,
            },
            ingestion: IngestionConfig {
                progress_update_interval: 1000,
                run_missed_immediately: true,
            },
            display: Some(DisplayConfig {
                local_timezone: "UTC".to_string(),
            }),
            data_mapping_engine: Some(DataMappingEngineConfig::default()),
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
            epg_channels: Some(3600), // 3600 * 9 = 32,400 variables
            epg_programs: Some(1900), // 1900 * 17 = 32,300 variables
            stream_channels: Some(1000),
        };
        assert!(valid_config.validate().is_ok());

        // Test EPG channels exceeding limit
        let invalid_channels = DatabaseBatchConfig {
            epg_channels: Some(4000), // 4000 * 9 = 36,000 variables (exceeds 32,766)
            epg_programs: Some(1900),
            stream_channels: Some(1000),
        };
        assert!(invalid_channels.validate().is_err());

        // Test EPG programs exceeding limit
        let invalid_programs = DatabaseBatchConfig {
            epg_channels: Some(3600),
            epg_programs: Some(2000), // 2000 * 17 = 34,000 variables (exceeds 32,766)
            stream_channels: Some(1000),
        };
        assert!(invalid_programs.validate().is_err());
    }

    #[test]
    fn test_safe_batch_sizes() {
        let config = DatabaseBatchConfig {
            epg_channels: Some(5000), // Too large, should be capped
            epg_programs: Some(3000), // Too large, should be capped
            stream_channels: Some(1000),
        };

        // Should return safe sizes within SQLite limits
        let safe_channels = config.safe_epg_channel_batch_size();
        let safe_programs = config.safe_epg_program_batch_size();

        assert!(safe_channels * 9 <= 32766);
        assert!(safe_programs * 17 <= 32766);

        // Should cap to maximum safe values
        assert_eq!(safe_channels, 32766 / 9); // 3640
        assert_eq!(safe_programs, 32766 / 17); // 1927
    }

    #[test]
    fn test_default_batch_config() {
        let default_config = DatabaseBatchConfig::default();

        // Default values should be valid
        assert!(default_config.validate().is_ok());

        // Default values should be within safe limits
        assert_eq!(default_config.epg_channels, Some(3600));
        assert_eq!(default_config.epg_programs, Some(1900));
        assert_eq!(default_config.stream_channels, Some(1000));
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_file =
            std::env::var("CONFIG_FILE").unwrap_or_else(|_| "config.toml".to_string());

        if std::path::Path::new(&config_file).exists() {
            let contents = std::fs::read_to_string(&config_file)?;
            Ok(toml::from_str(&contents)?)
        } else {
            let default_config = Self::default();
            let contents = toml::to_string_pretty(&default_config)?;
            std::fs::create_dir_all("./data/m3u")?;
            std::fs::create_dir_all("./data/logos")?;
            std::fs::write(&config_file, contents)?;
            Ok(default_config)
        }
    }
}
