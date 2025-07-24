//! EPG source model implementations with timezone detection and time offset support

use crate::models::{EpgSource, EpgSourceType};
use crate::utils::time::{detect_timezone_from_xmltv, log_timezone_detection, parse_time_offset};
use anyhow::Result;
use tracing::warn;

impl EpgSource {
    /// Detect timezone from EPG content (simplified after migration 004)
    /// Note: Timezone handling was simplified - all times are normalized to UTC
    #[allow(dead_code)]
    pub async fn detect_timezone_from_content(
        &self,
        epg_content: &str,
    ) -> Result<Option<String>> {
        let detected_tz = match self.source_type {
            EpgSourceType::Xmltv => detect_timezone_from_xmltv(epg_content),
            EpgSourceType::Xtream => {
                // Xtream Codes EPG usually doesn't contain timezone info in the content
                None
            }
        };

        if let Some(detected) = detected_tz {
            if crate::utils::time::validate_timezone(&detected).is_ok() {
                log_timezone_detection(&self.name, Some(&detected), &detected);
                return Ok(Some(detected));
            } else {
                warn!(
                    "EPG source '{}': Detected invalid timezone '{}', ignoring",
                    self.name, detected
                );
            }
        } else {
            log_timezone_detection(&self.name, None, "UTC");
        }

        Ok(None)
    }

    /// Get parsed time offset in seconds
    #[allow(dead_code)]
    pub fn get_time_offset_seconds(&self) -> Result<i32> {
        parse_time_offset(&self.time_offset)
            .map_err(|e| anyhow::anyhow!("Invalid time offset in source '{}': {}", self.name, e))
    }

    /// Check if source needs authentication (Xtream Codes)
    #[allow(dead_code)]
    pub fn needs_authentication(&self) -> bool {
        matches!(self.source_type, EpgSourceType::Xtream)
    }

    /// Build the full EPG URL for Xtream Codes sources
    #[allow(dead_code)]
    pub fn build_epg_url(&self) -> Result<String> {
        match self.source_type {
            EpgSourceType::Xmltv => {
                // Ensure XMLTV URLs have proper scheme
                let url = if self.url.starts_with("http://") || self.url.starts_with("https://") {
                    self.url.clone()
                } else {
                    format!("https://{}", self.url)
                };
                Ok(url)
            },
            EpgSourceType::Xtream => {
                if let (Some(username), Some(password)) = (&self.username, &self.password) {
                    // Ensure Xtream URLs have proper scheme
                    let base_url = if self.url.starts_with("http://") || self.url.starts_with("https://") {
                        self.url.clone()
                    } else {
                        format!("https://{}", self.url)
                    };
                    
                    Ok(format!(
                        "{}/xmltv.php?username={}&password={}",
                        base_url.trim_end_matches('/'),
                        username,
                        password
                    ))
                } else {
                    Err(anyhow::anyhow!(
                        "Xtream Codes EPG source '{}' requires username and password",
                        self.name
                    ))
                }
            }
        }
    }
}

impl std::fmt::Display for EpgSourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EpgSourceType::Xmltv => write!(f, "xmltv"),
            EpgSourceType::Xtream => write!(f, "xtream"),
        }
    }
}

impl std::str::FromStr for EpgSourceType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "xmltv" => Ok(EpgSourceType::Xmltv),
            "xtream" => Ok(EpgSourceType::Xtream),
            _ => Err(anyhow::anyhow!("Invalid EPG source type: {}", s)),
        }
    }
}
