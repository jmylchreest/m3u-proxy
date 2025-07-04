//! EPG source model implementations with timezone detection and time offset support

use crate::models::{EpgSource, EpgSourceType};
use crate::utils::time::{detect_timezone_from_xmltv, log_timezone_detection, parse_time_offset};
use anyhow::Result;
use tracing::{info, warn};

impl EpgSource {
    /// Detect and update timezone from EPG content
    #[allow(dead_code)]
    pub async fn detect_and_update_timezone(
        &mut self,
        _pool: &sqlx::SqlitePool,
        epg_content: &str,
    ) -> Result<bool> {
        let detected_tz = match self.source_type {
            EpgSourceType::Xmltv => detect_timezone_from_xmltv(epg_content),
            EpgSourceType::Xtream => {
                // Xtream Codes EPG usually doesn't contain timezone info in the content
                // We might need to make additional API calls or use server location
                None
            }
        };

        let mut timezone_updated = false;

        if let Some(detected) = detected_tz {
            if crate::utils::time::validate_timezone(&detected).is_ok() {
                // Only update if we haven't manually set the timezone
                if self.timezone == "UTC" && !self.timezone_detected {
                    log_timezone_detection(&self.name, Some(&detected), &detected);

                    self.timezone = detected;
                    self.timezone_detected = true;
                    timezone_updated = true;

                    info!(
                        "Updated EPG source '{}' timezone to '{}'",
                        self.name, self.timezone
                    );
                } else {
                    log_timezone_detection(&self.name, Some(&detected), &self.timezone);
                }
            } else {
                warn!(
                    "EPG source '{}': Detected invalid timezone '{}', keeping '{}'",
                    self.name, detected, self.timezone
                );
            }
        } else {
            log_timezone_detection(&self.name, None, &self.timezone);
        }

        Ok(timezone_updated)
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
            EpgSourceType::Xmltv => Ok(self.url.clone()),
            EpgSourceType::Xtream => {
                if let (Some(username), Some(password)) = (&self.username, &self.password) {
                    Ok(format!(
                        "{}/xmltv.php?username={}&password={}",
                        self.url.trim_end_matches('/'),
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
