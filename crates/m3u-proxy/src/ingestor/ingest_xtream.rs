use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use std::collections::HashMap;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::SourceIngestor;
use crate::models::*;

pub struct XtreamIngestor {
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct XtreamChannel {
    #[serde(deserialize_with = "deserialize_string_or_int_option", default)]
    num: Option<i32>,
    name: String,
    #[serde(default = "default_stream_type")]
    stream_type: String,
    #[serde(deserialize_with = "deserialize_string_or_int")]
    stream_id: i32,
    #[serde(default)]
    stream_icon: Option<String>,
    #[serde(default)]
    epg_channel_id: Option<String>,
    #[serde(default)]
    added: Option<String>,
    #[serde(default)]
    category_name: Option<String>,
    #[serde(deserialize_with = "deserialize_string_or_int_option", default)]
    category_id: Option<i32>,
    #[serde(deserialize_with = "deserialize_string_or_int_option", default)]
    tv_archive: Option<i32>,
    #[serde(deserialize_with = "deserialize_string_or_int_option", default)]
    tv_archive_duration: Option<i32>,
    #[serde(default)]
    direct_source: Option<String>,
    #[serde(flatten)]
    extra_fields: HashMap<String, serde_json::Value>,
}

// Default function for stream_type when missing from JSON
fn default_stream_type() -> String {
    "live".to_string()
}

// Helper function to deserialize either string or int to i32
fn deserialize_string_or_int<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct StringOrIntVisitor;

    impl<'de> Visitor<'de> for StringOrIntVisitor {
        type Value = i32;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an integer or a string representation of an integer")
        }

        fn visit_i8<E>(self, value: i8) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value as i32)
        }

        fn visit_i16<E>(self, value: i16) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value as i32)
        }

        fn visit_i32<E>(self, value: i32) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value >= i32::MIN as i64 && value <= i32::MAX as i64 {
                Ok(value as i32)
            } else {
                Err(E::custom(format!(
                    "i64 value {} is out of range for i32",
                    value
                )))
            }
        }

        fn visit_u8<E>(self, value: u8) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value as i32)
        }

        fn visit_u16<E>(self, value: u16) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value as i32)
        }

        fn visit_u32<E>(self, value: u32) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value <= i32::MAX as u32 {
                Ok(value as i32)
            } else {
                Err(E::custom(format!(
                    "u32 value {} is out of range for i32",
                    value
                )))
            }
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value <= i32::MAX as u64 {
                Ok(value as i32)
            } else {
                Err(E::custom(format!(
                    "u64 value {} is out of range for i32",
                    value
                )))
            }
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            value.parse::<i32>().map_err(E::custom)
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            value.parse::<i32>().map_err(E::custom)
        }
    }

    deserializer.deserialize_any(StringOrIntVisitor)
}

// Helper function to deserialize either string or int to Option<i32>
fn deserialize_string_or_int_option<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct StringOrIntOptionVisitor;

    impl<'de> Visitor<'de> for StringOrIntOptionVisitor {
        type Value = Option<i32>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an integer, a string representation of an integer, or null")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserialize_string_or_int(deserializer).map(Some)
        }

        fn visit_i8<E>(self, value: i8) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value as i32))
        }

        fn visit_i16<E>(self, value: i16) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value as i32))
        }

        fn visit_i32<E>(self, value: i32) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value >= i32::MIN as i64 && value <= i32::MAX as i64 {
                Ok(Some(value as i32))
            } else {
                Err(E::custom(format!(
                    "i64 value {} is out of range for i32",
                    value
                )))
            }
        }

        fn visit_u8<E>(self, value: u8) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value as i32))
        }

        fn visit_u16<E>(self, value: u16) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value as i32))
        }

        fn visit_u32<E>(self, value: u32) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value <= i32::MAX as u32 {
                Ok(Some(value as i32))
            } else {
                Err(E::custom(format!(
                    "u32 value {} is out of range for i32",
                    value
                )))
            }
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value <= i32::MAX as u64 {
                Ok(Some(value as i32))
            } else {
                Err(E::custom(format!(
                    "u64 value {} is out of range for i32",
                    value
                )))
            }
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value.is_empty() {
                Ok(None)
            } else {
                value.parse::<i32>().map(Some).map_err(E::custom)
            }
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value.is_empty() {
                Ok(None)
            } else {
                value.parse::<i32>().map(Some).map_err(E::custom)
            }
        }
    }

    deserializer.deserialize_option(StringOrIntOptionVisitor)
}

impl XtreamIngestor {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
    
    /// Extract clean channel name from Xtream API name field
    /// The name field often contains EXTINF lines like: "#EXTINF:-1 tvg-name=\"...\" ## CHANNEL NAME ##"
    /// We need to extract just the clean channel name
    fn extract_clean_channel_name(raw_name: &str) -> String {
        // Check if this looks like an EXTINF line
        if raw_name.starts_with("#EXTINF:") {
            // Try to extract from tvg-name attribute first (most reliable)
            if let Some(tvg_name_start) = raw_name.find("tvg-name=\"") {
                let start = tvg_name_start + 10; // Skip 'tvg-name="'
                if let Some(end) = raw_name[start..].find('"') {
                    let tvg_name = &raw_name[start..start + end];
                    if !tvg_name.is_empty() {
                        debug!(
                            "Xtream channel name extracted from tvg-name: '{}' -> '{}'",
                            raw_name, tvg_name
                        );
                        return tvg_name.to_string();
                    }
                }
            }
            
            // Fallback: Look for content after the last attribute (space-separated)
            // Format: #EXTINF:-1 tvg-name="..." tvg-chno="..." CHANNEL NAME
            let mut parts = raw_name.split_whitespace();
            parts.next(); // Skip "#EXTINF:-1"
            
            // Skip all attribute=value pairs
            let remaining_parts: Vec<&str> = parts.skip_while(|part| part.contains('=')).collect();
            
            if !remaining_parts.is_empty() {
                let channel_name = remaining_parts.join(" ");
                if !channel_name.is_empty() {
                    debug!(
                        "Xtream channel name extracted from end: '{}' -> '{}'",
                        raw_name, channel_name
                    );
                    return channel_name;
                }
            }
            
            // Last fallback: try to find comma-separated content
            if let Some(comma_pos) = raw_name.rfind(',') {
                let after_comma = raw_name[comma_pos + 1..].trim();
                
                // If there's content after the comma, use that as the channel name
                if !after_comma.is_empty() {
                    debug!(
                        "Xtream channel name extracted from comma: '{}' -> '{}'",
                        raw_name, after_comma
                    );
                    return after_comma.to_string();
                }
            }
            
            warn!(
                "Could not extract clean channel name from EXTINF line: '{}'",
                raw_name
            );
        }
        
        // If not an EXTINF line or extraction failed, return as-is
        raw_name.to_string()
    }
}

#[async_trait]
impl SourceIngestor for XtreamIngestor {
    async fn ingest(
        &self,
        source: &StreamSource,
        state_manager: &crate::ingestor::IngestionStateManager,
    ) -> Result<Vec<Channel>> {
        info!(
            "Starting Xtream ingestion for source '{}' ({})",
            source.name, source.id
        );

        let username = source.username.as_ref().ok_or_else(|| {
            error!("Username required for Xtream source '{}'", source.name);
            anyhow::anyhow!("Username required for Xtream source")
        })?;
        let password = source.password.as_ref().ok_or_else(|| {
            error!("Password required for Xtream source '{}'", source.name);
            anyhow::anyhow!("Password required for Xtream source")
        })?;

        // Update state to connecting
        state_manager
            .update_progress(
                source.id,
                crate::models::IngestionState::Connecting,
                crate::models::ProgressInfo {
                    current_step: "Connecting to Xtream API".to_string(),
                    total_bytes: None,
                    downloaded_bytes: None,
                    channels_parsed: None,
                    channels_saved: None,
                    programs_parsed: None,
                    programs_saved: None,
                    percentage: Some(10.0),
                },
            )
            .await;

        let base_url = self.parse_base_url(&source.url)?;
        let channels_url = format!(
            "{}/player_api.php?username={}&password={}&action=get_live_streams",
            base_url, username, password
        );

        info!(
            "Connecting to Xtream API: {}",
            channels_url.replace(password, "***")
        );

        // Update state to downloading
        state_manager
            .update_progress(
                source.id,
                crate::models::IngestionState::Downloading,
                crate::models::ProgressInfo {
                    current_step: "Fetching channel list from Xtream API".to_string(),
                    total_bytes: None,
                    downloaded_bytes: None,
                    channels_parsed: None,
                    channels_saved: None,
                    programs_parsed: None,
                    programs_saved: None,
                    percentage: Some(30.0),
                },
            )
            .await;

        let response = self.client.get(&channels_url).send().await.map_err(|e| {
            error!(
                "Failed to connect to Xtream API for source '{}': {}",
                source.name, e
            );
            e
        })?;

        info!("Xtream API responded with status: {}", response.status());

        let response_text = response.text().await.map_err(|e| {
            error!(
                "Failed to read Xtream API response body for source '{}': {}",
                source.name, e
            );
            e
        })?;

        debug!(
            "Xtream API response for source '{}': {}",
            source.name,
            &response_text[..std::cmp::min(500, response_text.len())]
        );

        let channels: Vec<XtreamChannel> = match serde_json::from_str(&response_text) {
            Ok(channels) => channels,
            Err(e) => {
                error!(
                    "Failed to parse Xtream API JSON response for source '{}': {}",
                    source.name, e
                );

                // Try to find the problematic field around the error position
                let column = e.column();
                let start = if column >= 100 { column - 100 } else { 0 };
                let end = std::cmp::min(column + 100, response_text.len());
                let snippet = &response_text[start..end];
                error!(
                    "JSON snippet around error position {}: ...{}...",
                    column, snippet
                );

                // Save the response to a file for debugging
                if let Err(write_err) = std::fs::write("xtream_debug_response.json", &response_text)
                {
                    warn!("Failed to write debug response file: {}", write_err);
                } else {
                    info!("Saved problematic response to xtream_debug_response.json for debugging");
                }

                // Try parsing as a single object instead of array (some APIs return objects)
                if let Ok(single_channel) = serde_json::from_str::<XtreamChannel>(&response_text) {
                    warn!(
                        "API returned single channel object instead of array, converting to array"
                    );
                    vec![single_channel]
                } else {
                    // Try to parse as Value first to see the structure
                    if let Ok(json_value) =
                        serde_json::from_str::<serde_json::Value>(&response_text)
                    {
                        error!(
                            "Response structure: {}",
                            serde_json::to_string_pretty(&json_value)
                                .unwrap_or_else(|_| "Unable to pretty-print".to_string())
                        );
                    }

                    debug!("Full response body: {}", response_text);
                    return Err(anyhow::anyhow!("Failed to parse JSON: {}", e));
                }
            }
        };

        info!(
            "Received {} channels from Xtream API for source '{}'",
            channels.len(),
            source.name
        );

        // Update state to parsing
        state_manager
            .update_progress(
                source.id,
                crate::models::IngestionState::Parsing,
                crate::models::ProgressInfo {
                    current_step: format!("Processing {} channels from Xtream API", channels.len()),
                    total_bytes: None,
                    downloaded_bytes: None,
                    channels_parsed: Some(0),
                    channels_saved: None,
                    programs_parsed: None,
                    programs_saved: None,
                    percentage: Some(60.0),
                },
            )
            .await;

        let mut result = Vec::new();
        let mut processed = 0;
        let mut seen_stream_ids = std::collections::HashSet::new();
        let mut duplicate_count = 0;

        for xtream_channel in channels {
            let stream_url = format!(
                "{}/live/{}/{}/{}.ts",
                base_url, username, password, xtream_channel.stream_id
            );

            // Check for duplicate stream_ids
            if seen_stream_ids.contains(&xtream_channel.stream_id) {
                duplicate_count += 1;
                debug!(
                    "Skipping duplicate stream_id {} for channel '{}'",
                    xtream_channel.stream_id, xtream_channel.name
                );
                continue;
            }
            seen_stream_ids.insert(xtream_channel.stream_id);

            // Extract clean channel name from potentially EXTINF-formatted name
            let clean_channel_name = Self::extract_clean_channel_name(&xtream_channel.name);
            
            let channel = Channel {
                id: Uuid::new_v4(),
                source_id: source.id,
                tvg_id: xtream_channel.epg_channel_id,
                tvg_name: Some(clean_channel_name.clone()),
                tvg_logo: xtream_channel.stream_icon,
                tvg_shift: None,
                group_title: xtream_channel.category_name,
                channel_name: clean_channel_name,
                stream_url,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };

            result.push(channel);
            processed += 1;

            // Log progress every 100 channels
            if processed % 100 == 0 {
                debug!(
                    "Processed {} / {} channels from Xtream source '{}'",
                    processed,
                    result.len(),
                    source.name
                );

                let percentage = 60.0 + (processed as f64 / result.len() as f64) * 20.0; // 60% to 80%
                state_manager
                    .update_progress(
                        source.id,
                        crate::models::IngestionState::Parsing,
                        crate::models::ProgressInfo {
                            current_step: format!("Processed {} channels", processed),
                            total_bytes: None,
                            downloaded_bytes: None,
                            channels_parsed: Some(processed),
                            channels_saved: None,
                            programs_parsed: None,
                            programs_saved: None,
                            percentage: Some(percentage),
                        },
                    )
                    .await;
            }
        }

        info!(
            "Xtream processing completed for source '{}': {} channels processed, {} duplicates skipped",
            source.name,
            result.len(),
            duplicate_count
        );

        // Final parsing update
        state_manager
            .update_progress(
                source.id,
                crate::models::IngestionState::Parsing,
                crate::models::ProgressInfo {
                    current_step: format!("Processing completed - {} channels found", result.len()),
                    total_bytes: None,
                    downloaded_bytes: None,
                    channels_parsed: Some(result.len()),
                    channels_saved: None,
                    programs_parsed: None,
                    programs_saved: None,
                    percentage: Some(80.0),
                },
            )
            .await;

        Ok(result)
    }
}

impl XtreamIngestor {
    fn parse_base_url(&self, url: &str) -> Result<String> {
        // Ensure URL has a scheme if missing
        let url_to_parse = if url.starts_with("http://") || url.starts_with("https://") {
            url.to_string()
        } else {
            format!("http://{}", url)
        };

        debug!("Parsing base URL from: {}", url_to_parse);

        // Remove any trailing paths and parameters to get base URL
        let parsed = url::Url::parse(&url_to_parse)?;
        let base_url = if let Some(port) = parsed.port() {
            format!(
                "{}://{}:{}",
                parsed.scheme(),
                parsed.host_str().unwrap_or("localhost"),
                port
            )
        } else {
            format!(
                "{}://{}",
                parsed.scheme(),
                parsed.host_str().unwrap_or("localhost")
            )
        };

        debug!("Parsed base URL: {}", base_url);
        Ok(base_url)
    }
}
