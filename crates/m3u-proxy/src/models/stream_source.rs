//! Stream source model implementations

use crate::models::{StreamSource, StreamSourceType};
use anyhow::Result;

impl StreamSource {
    /// Check if source needs authentication
    pub fn needs_authentication(&self) -> bool {
        matches!(self.source_type, StreamSourceType::Xtream)
    }

    /// Build the full stream URL for Xtream Codes sources
    pub fn build_stream_url(&self) -> Result<String> {
        match self.source_type {
            StreamSourceType::M3u => Ok(self.url.clone()),
            StreamSourceType::Xtream => {
                if let (Some(username), Some(password)) = (&self.username, &self.password) {
                    Ok(format!(
                        "{}/get.php?username={}&password={}&type=m3u_plus&output=ts",
                        self.url.trim_end_matches('/'),
                        username,
                        password
                    ))
                } else {
                    Err(anyhow::anyhow!(
                        "Xtream Codes stream source '{}' requires username and password",
                        self.name
                    ))
                }
            }
        }
    }
}

impl std::str::FromStr for StreamSourceType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "m3u" => Ok(StreamSourceType::M3u),
            "xtream" => Ok(StreamSourceType::Xtream),
            _ => Err(anyhow::anyhow!("Invalid stream source type: {}", s)),
        }
    }
}

impl std::fmt::Display for StreamSourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamSourceType::M3u => write!(f, "m3u"),
            StreamSourceType::Xtream => write!(f, "xtream"),
        }
    }
}
