use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub database: DatabaseConfig,
    pub web: WebConfig,
    pub storage: StorageConfig,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub m3u_path: PathBuf,
    pub logo_path: PathBuf,
    pub proxy_versions_to_keep: u32,
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
            },
            storage: StorageConfig {
                m3u_path: PathBuf::from("./data/m3u"),
                logo_path: PathBuf::from("./data/logos"),
                proxy_versions_to_keep: 3,
            },
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_file = std::env::var("CONFIG_FILE").unwrap_or_else(|_| "config.toml".to_string());
        
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