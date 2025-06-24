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
    pub channel_similarity: Option<ChannelSimilarityConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: Option<u32>,
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
pub struct ChannelSimilarityConfig {
    /// Regex patterns to remove when comparing channels (cloned channel patterns)
    pub clone_patterns: Vec<String>,
    /// Regex patterns that indicate timeshift channels (e.g., r"\+(\d+)" for +1, +24, etc.)
    /// Hours shift will be extracted from the first capture group
    pub timeshift_patterns: Vec<String>,
    /// Minimum confidence threshold for considering channels as clones (0.0-1.0)
    /// Channels above this threshold should share the same tvg-id/channel id
    pub clone_confidence_threshold: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database: DatabaseConfig {
                url: "sqlite://./m3u-proxy.db".to_string(),
                max_connections: Some(10),
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
            channel_similarity: Some(ChannelSimilarityConfig {
                clone_patterns: vec![
                    r"(?i)\b4K\b".to_string(),
                    r"(?i)\bHD\b".to_string(),
                    r"(?i)\bSD\b".to_string(),
                    r"(?i)\bHEVC\b".to_string(),
                    r"(?i)\b720P?\b".to_string(),
                    r"(?i)\b1080P?\b".to_string(),
                    r"(?i)\bUHD\b".to_string(),
                    r"\[|\]|\(|\)".to_string(),
                    r"(?i)\(SAT\)".to_string(),
                    r"(?i)\(CABLE\)".to_string(),
                    r"(?i)\(IPTV\)".to_string(),
                ],
                timeshift_patterns: vec![r"(?i)\+(\d+)".to_string(), r"(?i)\+(\d+)H".to_string()],
                clone_confidence_threshold: 0.90,
            }),
        }
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
