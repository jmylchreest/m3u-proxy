use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use tracing::{debug, error, info};
use uuid::Uuid;

use super::SourceIngestor;
use crate::models::*;

pub struct M3uIngestor {
    client: reqwest::Client,
}

impl M3uIngestor {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl SourceIngestor for M3uIngestor {
    async fn ingest(
        &self,
        source: &StreamSource,
        state_manager: &crate::ingestor::IngestionStateManager,
    ) -> Result<Vec<Channel>> {
        info!(
            "Starting M3U ingestion for source '{}' ({})",
            source.name, source.id
        );

        // Update state to connecting
        state_manager
            .update_progress(
                source.id,
                crate::models::IngestionState::Connecting,
                crate::models::ProgressInfo {
                    current_step: "Connecting to M3U source".to_string(),
                    total_bytes: None,
                    downloaded_bytes: None,
                    channels_parsed: None,
                    channels_saved: None,
                    percentage: Some(10.0),
                },
            )
            .await;

        info!("Connecting to M3U source: {}", source.url);

        // Start download
        let response = self.client.get(&source.url).send().await.map_err(|e| {
            error!("Failed to connect to M3U source '{}': {}", source.name, e);
            e
        })?;

        let total_size = response.content_length();
        info!(
            "Connected to M3U source '{}', content length: {:?} bytes",
            source.name, total_size
        );

        state_manager
            .update_progress(
                source.id,
                crate::models::IngestionState::Downloading,
                crate::models::ProgressInfo {
                    current_step: "Downloading M3U playlist".to_string(),
                    total_bytes: total_size,
                    downloaded_bytes: Some(0),
                    channels_parsed: None,
                    channels_saved: None,
                    percentage: Some(20.0),
                },
            )
            .await;

        // Download with progress tracking
        let mut content = String::new();
        let mut downloaded = 0u64;
        let mut last_logged_percentage = 0.0;

        use futures::StreamExt;
        let mut stream = response.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| {
                error!(
                    "Error downloading chunk from M3U source '{}': {}",
                    source.name, e
                );
                e
            })?;

            content.push_str(&String::from_utf8_lossy(&chunk));
            downloaded += chunk.len() as u64;

            if let Some(total) = total_size {
                let percentage = 20.0 + (downloaded as f64 / total as f64) * 30.0; // 20% to 50%

                // Log progress every 10%
                if percentage - last_logged_percentage >= 10.0 {
                    debug!(
                        "Download progress for '{}': {:.1}% ({} / {} bytes)",
                        source.name, percentage, downloaded, total
                    );
                    last_logged_percentage = percentage;
                }

                state_manager
                    .update_progress(
                        source.id,
                        crate::models::IngestionState::Downloading,
                        crate::models::ProgressInfo {
                            current_step: format!("Downloaded {} / {} bytes", downloaded, total),
                            total_bytes: Some(total),
                            downloaded_bytes: Some(downloaded),
                            channels_parsed: None,
                            channels_saved: None,
                            percentage: Some(percentage),
                        },
                    )
                    .await;
            } else {
                // Log periodic updates for unknown size
                if downloaded % 100_000 == 0 && downloaded > 0 {
                    debug!("Downloaded {} bytes from '{}'", downloaded, source.name);
                }
            }
        }

        info!(
            "Download completed for '{}': {} bytes",
            source.name, downloaded
        );

        // Start parsing
        info!("Starting M3U parsing for source '{}'", source.name);
        state_manager
            .update_progress(
                source.id,
                crate::models::IngestionState::Parsing,
                crate::models::ProgressInfo {
                    current_step: "Parsing M3U playlist".to_string(),
                    total_bytes: total_size,
                    downloaded_bytes: Some(downloaded),
                    channels_parsed: Some(0),
                    channels_saved: None,
                    percentage: Some(60.0),
                },
            )
            .await;

        self.parse_m3u(&content, source, state_manager).await
    }
}

impl M3uIngestor {
    async fn parse_m3u(
        &self,
        content: &str,
        source: &StreamSource,
        state_manager: &crate::ingestor::IngestionStateManager,
    ) -> Result<Vec<Channel>> {
        let mut channels = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        info!(
            "Parsing M3U content with {} lines for source '{}'",
            total_lines, source.name
        );

        let mut i = 0;
        let mut last_logged_count = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            if line.starts_with("#EXTINF:") {
                if let Some(channel) = self.parse_extinf_line(line, lines.get(i + 1), source)? {
                    channels.push(channel);

                    // Log progress every 100 channels
                    if channels.len() % 100 == 0 && channels.len() > last_logged_count {
                        debug!(
                            "Parsed {} channels from M3U source '{}'",
                            channels.len(),
                            source.name
                        );
                        last_logged_count = channels.len();

                        // Update progress
                        let percentage = 60.0 + (i as f64 / total_lines as f64) * 20.0; // 60% to 80%
                        state_manager
                            .update_progress(
                                source.id,
                                crate::models::IngestionState::Parsing,
                                crate::models::ProgressInfo {
                                    current_step: format!("Parsed {} channels", channels.len()),
                                    total_bytes: None,
                                    downloaded_bytes: None,
                                    channels_parsed: Some(channels.len()),
                                    channels_saved: None,
                                    percentage: Some(percentage),
                                },
                            )
                            .await;
                    }
                }
                i += 2; // Skip the URL line
            } else {
                i += 1;
            }
        }

        info!(
            "M3U parsing completed for source '{}': {} channels parsed",
            source.name,
            channels.len()
        );

        // Final parsing update
        state_manager
            .update_progress(
                source.id,
                crate::models::IngestionState::Parsing,
                crate::models::ProgressInfo {
                    current_step: format!("Parsing completed - {} channels found", channels.len()),
                    total_bytes: None,
                    downloaded_bytes: None,
                    channels_parsed: Some(channels.len()),
                    channels_saved: None,
                    percentage: Some(80.0),
                },
            )
            .await;

        Ok(channels)
    }

    fn parse_extinf_line(
        &self,
        extinf_line: &str,
        url_line: Option<&&str>,
        source: &StreamSource,
    ) -> Result<Option<Channel>> {
        let url = match url_line {
            Some(url) if !url.trim().is_empty() && !url.trim().starts_with('#') => {
                url.trim().to_string()
            }
            _ => return Ok(None),
        };

        // Parse EXTINF line: #EXTINF:-1 tvg-id="..." tvg-name="..." tvg-logo="..." group-title="...",Channel Name
        let (attributes_part, channel_name) = if let Some(comma_pos) = extinf_line.rfind(',') {
            (
                &extinf_line[8..comma_pos], // Skip "#EXTINF:"
                extinf_line[comma_pos + 1..].trim().to_string(),
            )
        } else {
            return Ok(None);
        };

        let mut tvg_id = None;
        let mut tvg_name = None;
        let mut tvg_logo = None;
        let mut group_title = None;

        // Simple attribute parser - could be improved with proper regex
        for attr in self.parse_attributes(attributes_part) {
            match attr.0.as_str() {
                "tvg-id" => tvg_id = Some(attr.1),
                "tvg-name" => tvg_name = Some(attr.1),
                "tvg-logo" => tvg_logo = Some(attr.1),
                "group-title" => group_title = Some(attr.1),
                _ => {}
            }
        }

        Ok(Some(Channel {
            id: Uuid::new_v4(),
            source_id: source.id,
            tvg_id,
            tvg_name,
            tvg_logo,
            group_title,
            channel_name,
            stream_url: url,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }))
    }

    fn parse_attributes(&self, attributes: &str) -> Vec<(String, String)> {
        let mut attrs = Vec::new();
        let mut current_key = String::new();
        let mut current_value = String::new();
        let mut in_quotes = false;
        let mut in_value = false;
        let mut escape_next = false;

        for ch in attributes.chars() {
            if escape_next {
                if in_value {
                    current_value.push(ch);
                } else {
                    current_key.push(ch);
                }
                escape_next = false;
                continue;
            }

            match ch {
                '\\' => escape_next = true,
                '"' => {
                    if in_value {
                        in_quotes = !in_quotes;
                    }
                }
                '=' if !in_quotes && !in_value => {
                    in_value = true;
                }
                ' ' | '\t' if !in_quotes => {
                    if in_value && !current_value.is_empty() {
                        attrs.push((
                            current_key.trim().to_string(),
                            current_value.trim_matches('"').to_string(),
                        ));
                        current_key.clear();
                        current_value.clear();
                        in_value = false;
                    }
                }
                _ => {
                    if in_value {
                        current_value.push(ch);
                    } else {
                        current_key.push(ch);
                    }
                }
            }
        }

        // Handle last attribute
        if in_value && !current_value.is_empty() {
            attrs.push((
                current_key.trim().to_string(),
                current_value.trim_matches('"').to_string(),
            ));
        }

        attrs
    }
}
