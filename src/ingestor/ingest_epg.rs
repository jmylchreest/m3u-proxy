use crate::database::Database;
use crate::models::*;
use crate::utils::normalize_url_scheme;
use crate::utils::time::{
    detect_timezone_from_xmltv, log_timezone_detection, parse_time_offset, validate_timezone,
};
use anyhow::{anyhow, Result};
use chrono::{DateTime, TimeZone, Utc};
#[cfg(test)]
use chrono::{Datelike, Timelike};
use regex::Regex;
use reqwest::Client;
use std::collections::HashMap;
use tracing::{debug, info, warn};
use uuid::Uuid;

pub struct EpgIngestor {
    client: Client,
    database: Option<Database>,
    state_manager: Option<crate::ingestor::state_manager::IngestionStateManager>,
}

impl EpgIngestor {
    pub fn new(database: Database) -> Self {
        Self {
            client: Client::new(),
            database: Some(database),
            state_manager: None,
        }
    }

    pub fn new_with_state_manager(
        database: Database,
        state_manager: crate::ingestor::state_manager::IngestionStateManager,
    ) -> Self {
        Self {
            client: Client::new(),
            database: Some(database),
            state_manager: Some(state_manager),
        }
    }

    pub fn new_without_database() -> Self {
        Self {
            client: Client::new(),
            database: None,
            state_manager: None,
        }
    }

    #[allow(dead_code)]
    pub fn get_state_manager(
        &self,
    ) -> Option<&crate::ingestor::state_manager::IngestionStateManager> {
        self.state_manager.as_ref()
    }

    pub async fn ingest_epg_source_with_trigger(
        &self,
        source: &EpgSource,
        trigger: crate::ingestor::state_manager::ProcessingTrigger,
    ) -> Result<(Vec<EpgChannel>, Vec<EpgProgram>, Option<String>)> {
        // Check if we can start processing this source
        if let Some(state_manager) = &self.state_manager {
            if !state_manager.try_start_processing(source.id, trigger).await {
                return Err(anyhow!(
                    "EPG source '{}' is already being processed or in backoff period",
                    source.name
                ));
            }
        }

        let result = self.ingest_epg_source(source).await;
        let success = result.is_ok();

        // Always finish processing to update failure state
        if let Some(state_manager) = &self.state_manager {
            state_manager.finish_processing(source.id, success).await;
        }

        result
    }

    pub async fn ingest_epg_source(
        &self,
        source: &EpgSource,
    ) -> Result<(Vec<EpgChannel>, Vec<EpgProgram>, Option<String>)> {
        info!(
            "Starting EPG ingestion for source: {} ({})",
            source.name, source.id
        );

        // Start progress tracking if state manager is available
        if let Some(state_manager) = &self.state_manager {
            state_manager.start_ingestion(source.id).await;
            state_manager
                .update_progress(
                    source.id,
                    crate::models::IngestionState::Connecting,
                    crate::models::ProgressInfo {
                        current_step: format!("Starting EPG ingestion for '{}'", source.name),
                        total_bytes: None,
                        downloaded_bytes: None,
                        channels_parsed: None,
                        channels_saved: None,
                        percentage: Some(5.0),
                    },
                )
                .await;
        }

        let result = match source.source_type {
            EpgSourceType::Xmltv => self.ingest_xmltv_source(source).await,
            EpgSourceType::Xtream => self.ingest_xtream_source(source).await,
        };

        // If we detected a timezone and it's different from the current one, update the source
        if let (Some(ref database), Ok((_, _, Some(ref detected_tz)))) = (&self.database, &result) {
            if validate_timezone(detected_tz).is_ok()
                && detected_tz != &source.timezone
                && !source.timezone_detected
            {
                info!(
                    "Auto-detected timezone '{}' for EPG source '{}', updating database",
                    detected_tz, source.name
                );

                let update_request = EpgSourceUpdateRequest {
                    name: source.name.clone(),
                    source_type: source.source_type.clone(),
                    url: source.url.clone(),
                    update_cron: source.update_cron.clone(),
                    username: source.username.clone(),
                    password: source.password.clone(),
                    timezone: Some(detected_tz.clone()),
                    time_offset: Some(source.time_offset.clone()),
                    is_active: source.is_active,
                };

                if let Err(e) = database.update_epg_source(source.id, &update_request).await {
                    warn!(
                        "Failed to update detected timezone for EPG source '{}': {}",
                        source.name, e
                    );
                }
            }
        }

        // Update progress based on result
        if let Some(state_manager) = &self.state_manager {
            match &result {
                Ok((channels, programs, _)) => {
                    state_manager
                        .complete_ingestion(source.id, channels.len() + programs.len())
                        .await;
                }
                Err(e) => {
                    state_manager.set_error(source.id, e.to_string()).await;
                }
            }
        }

        result
    }

    async fn ingest_xmltv_source(
        &self,
        source: &EpgSource,
    ) -> Result<(Vec<EpgChannel>, Vec<EpgProgram>, Option<String>)> {
        info!("Fetching XMLTV data from: {}", source.url);

        // Update progress
        if let Some(state_manager) = &self.state_manager {
            state_manager
                .update_progress(
                    source.id,
                    crate::models::IngestionState::Downloading,
                    crate::models::ProgressInfo {
                        current_step: "Downloading XMLTV data".to_string(),
                        total_bytes: None,
                        downloaded_bytes: None,
                        channels_parsed: None,
                        channels_saved: None,
                        percentage: Some(15.0),
                    },
                )
                .await;
        }

        // Download XMLTV content
        let response = self.client.get(&source.url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch XMLTV data: HTTP {}",
                response.status()
            ));
        }

        let content = response.text().await?;
        info!("Downloaded XMLTV content ({} bytes)", content.len());

        // Update progress
        if let Some(state_manager) = &self.state_manager {
            state_manager
                .update_progress(
                    source.id,
                    crate::models::IngestionState::Parsing,
                    crate::models::ProgressInfo {
                        current_step: "Parsing XMLTV content".to_string(),
                        total_bytes: Some(content.len() as u64),
                        downloaded_bytes: Some(content.len() as u64),
                        channels_parsed: None,
                        channels_saved: None,
                        percentage: Some(30.0),
                    },
                )
                .await;
        }

        // Parse the XMLTV content
        self.parse_xmltv_content(&content, source).await
    }

    async fn ingest_xtream_source(
        &self,
        source: &EpgSource,
    ) -> Result<(Vec<EpgChannel>, Vec<EpgProgram>, Option<String>)> {
        let username = source
            .username
            .as_ref()
            .ok_or_else(|| anyhow!("Username required for Xtream Codes EPG"))?;
        let password = source
            .password
            .as_ref()
            .ok_or_else(|| anyhow!("Password required for Xtream Codes EPG"))?;

        info!("Fetching EPG data from Xtream Codes: {}", source.url);

        // Update progress
        if let Some(state_manager) = &self.state_manager {
            state_manager
                .update_progress(
                    source.id,
                    crate::models::IngestionState::Downloading,
                    crate::models::ProgressInfo {
                        current_step: "Downloading Xtream EPG data".to_string(),
                        total_bytes: None,
                        downloaded_bytes: None,
                        channels_parsed: None,
                        channels_saved: None,
                        percentage: Some(15.0),
                    },
                )
                .await;
        }

        // First, get the EPG data endpoint with proper URL scheme
        let normalized_base_url = normalize_url_scheme(&source.url);
        let epg_url = format!(
            "{}/xmltv.php?username={}&password={}",
            normalized_base_url, username, password
        );

        info!("Fetching Xtream EPG from: {}", epg_url);

        let response = self.client.get(&epg_url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch Xtream EPG data: HTTP {}",
                response.status()
            ));
        }

        let content = response.text().await?;
        info!("Downloaded Xtream EPG content ({} bytes)", content.len());

        // Validate that we got XMLTV content
        if !content.contains("<?xml") || !content.contains("<tv") {
            return Err(anyhow!(
                "Downloaded content does not appear to be valid XMLTV data (missing XML declaration or TV root)"
            ));
        }

        // Update progress before parsing
        if let Some(state_manager) = &self.state_manager {
            state_manager
                .update_progress(
                    source.id,
                    crate::models::IngestionState::Parsing,
                    crate::models::ProgressInfo {
                        current_step: "Parsing Xtream XMLTV content".to_string(),
                        total_bytes: Some(content.len() as u64),
                        downloaded_bytes: Some(content.len() as u64),
                        channels_parsed: None,
                        channels_saved: None,
                        percentage: Some(30.0),
                    },
                )
                .await;
        }

        // Parse the XMLTV content from Xtream
        info!("Starting XMLTV parsing for EPG source '{}'", source.name);
        self.parse_xmltv_content(&content, source).await
    }

    async fn parse_xmltv_content(
        &self,
        content: &str,
        source: &EpgSource,
    ) -> Result<(Vec<EpgChannel>, Vec<EpgProgram>, Option<String>)> {
        // Check for cancellation at the start
        if let Some(state_manager) = &self.state_manager {
            if let Some(mut cancel_rx) = state_manager.get_cancellation_receiver(source.id).await {
                if cancel_rx.try_recv().is_ok() {
                    return Err(anyhow!("EPG parsing cancelled before starting"));
                }
            }
        }

        // Detect timezone from XMLTV content
        let detected_timezone = detect_timezone_from_xmltv(content);

        // Use detected timezone if found and not already detected, otherwise use configured
        let timezone_to_use = if let Some(ref detected_tz) = detected_timezone {
            log_timezone_detection(&source.name, Some(detected_tz), detected_tz);
            detected_tz
        } else {
            log_timezone_detection(&source.name, None, &source.timezone);
            &source.timezone
        };

        // Parse the timezone
        let tz = self.parse_timezone(timezone_to_use)?;

        // Parse time offset in seconds
        let time_offset_seconds = parse_time_offset(&source.time_offset)
            .map_err(|e| anyhow!("Invalid time offset in source '{}': {}", source.name, e))?;

        if time_offset_seconds != 0 {
            info!(
                "Applying time offset '{}' to EPG times for source '{}'",
                source.time_offset, source.name
            );
        }

        // Simple XML parsing - this is a basic implementation
        // In a production system, you'd want to use a proper XML parser like roxmltree or quick-xml
        let mut channels = Vec::new();
        let mut programs = Vec::new();
        let mut channel_map = std::collections::HashMap::new(); // Track channel_id duplicates

        // Parse channels with error handling and cancellation checks
        info!("Extracting channel sections from XMLTV content");
        match self.extract_xml_sections(content, "channel") {
            Some(channels_data) if !channels_data.is_empty() => {
                info!("Found {} channel sections to parse", channels_data.len());
                for (i, channel_xml) in channels_data.iter().enumerate() {
                    // Check for cancellation every 50 channels
                    if i % 50 == 0 {
                        if let Some(state_manager) = &self.state_manager {
                            if let Some(mut cancel_rx) =
                                state_manager.get_cancellation_receiver(source.id).await
                            {
                                if cancel_rx.try_recv().is_ok() {
                                    return Err(anyhow!(
                                        "EPG parsing cancelled during channel processing at {}/{}",
                                        i,
                                        channels_data.len()
                                    ));
                                }
                            }
                        }
                    }

                    if i % 100 == 0 && i > 0 {
                        info!("Processed {}/{} channels", i, channels_data.len());
                        // Update progress for channels
                        if let Some(state_manager) = &self.state_manager {
                            let percentage = 40.0 + (i as f64 / channels_data.len() as f64) * 20.0;
                            state_manager
                                .update_progress(
                                    source.id,
                                    crate::models::IngestionState::Parsing,
                                    crate::models::ProgressInfo {
                                        current_step: format!(
                                            "Parsing channels ({}/{})",
                                            i,
                                            channels_data.len()
                                        ),
                                        total_bytes: Some(content.len() as u64),
                                        downloaded_bytes: Some(content.len() as u64),
                                        channels_parsed: Some(i),
                                        channels_saved: None,
                                        percentage: Some(percentage),
                                    },
                                )
                                .await;
                        }
                    }
                    if let Some(channel) = self.parse_channel_xml(&channel_xml, source.id) {
                        // Check for duplicate channel_id
                        if let Some(existing_channel) = channel_map.get(&channel.channel_id) {
                            // Found duplicate - check if data is identical
                            if self.are_channels_identical(existing_channel, &channel) {
                                debug!(
                                    "Duplicate identical channel found: {} for source '{}'",
                                    channel.channel_id, source.name
                                );
                                // Skip this duplicate silently
                                continue;
                            } else {
                                debug!(
                                    "Duplicate conflicting channel found: {} for source '{}' - sending to DLQ",
                                    channel.channel_id, source.name
                                );
                                // Handle conflicting duplicate
                                if let Err(e) = self
                                    .handle_channel_conflict(source, existing_channel, &channel)
                                    .await
                                {
                                    warn!("Failed to save channel conflict to DLQ: {}", e);
                                }
                                continue;
                            }
                        }

                        // No duplicate, add to map and channels list
                        channel_map.insert(channel.channel_id.clone(), channel.clone());
                        channels.push(channel);
                    }
                }
            }
            Some(_) => {
                warn!("Found empty channel sections list");
            }
            None => {
                warn!("Failed to extract channel sections from XMLTV content");
            }
        }

        info!("Parsed {} channels", channels.len());

        // Check for cancellation before starting programs
        if let Some(state_manager) = &self.state_manager {
            if let Some(mut cancel_rx) = state_manager.get_cancellation_receiver(source.id).await {
                if cancel_rx.try_recv().is_ok() {
                    return Err(anyhow!("EPG parsing cancelled before program processing"));
                }
            }
        }

        // Parse programmes with error handling and cancellation checks
        info!("Extracting programme sections from XMLTV content");
        match self.extract_xml_sections(content, "programme") {
            Some(programmes_data) if !programmes_data.is_empty() => {
                info!(
                    "Found {} programme sections to parse",
                    programmes_data.len()
                );
                for (i, programme_xml) in programmes_data.iter().enumerate() {
                    // Check for cancellation every 500 programs
                    if i % 500 == 0 {
                        if let Some(state_manager) = &self.state_manager {
                            if let Some(mut cancel_rx) =
                                state_manager.get_cancellation_receiver(source.id).await
                            {
                                if cancel_rx.try_recv().is_ok() {
                                    return Err(anyhow!(
                                        "EPG parsing cancelled during program processing at {}/{}",
                                        i,
                                        programmes_data.len()
                                    ));
                                }
                            }
                        }
                    }

                    if i % 1000 == 0 && i > 0 {
                        info!("Processed {}/{} programmes", i, programmes_data.len());
                        // Update progress for programmes
                        if let Some(state_manager) = &self.state_manager {
                            let percentage =
                                60.0 + (i as f64 / programmes_data.len() as f64) * 30.0;
                            state_manager
                                .update_progress(
                                    source.id,
                                    crate::models::IngestionState::Parsing,
                                    crate::models::ProgressInfo {
                                        current_step: format!(
                                            "Parsing programmes ({}/{})",
                                            i,
                                            programmes_data.len()
                                        ),
                                        total_bytes: Some(content.len() as u64),
                                        downloaded_bytes: Some(content.len() as u64),
                                        channels_parsed: Some(channels.len()),
                                        channels_saved: None,
                                        percentage: Some(percentage),
                                    },
                                )
                                .await;
                        }
                    }
                    if let Some(program) =
                        self.parse_programme_xml(&programme_xml, source, &tz, time_offset_seconds)
                    {
                        programs.push(program);
                    }
                }
            }
            Some(_) => {
                warn!("Found empty programme sections list");
            }
            None => {
                warn!("Failed to extract programme sections from XMLTV content");
            }
        }

        info!("Parsed {} programs", programs.len());

        // Final cancellation check
        if let Some(state_manager) = &self.state_manager {
            if let Some(mut cancel_rx) = state_manager.get_cancellation_receiver(source.id).await {
                if cancel_rx.try_recv().is_ok() {
                    return Err(anyhow!("EPG parsing cancelled after completion"));
                }
            }
        }

        info!(
            "Completed XMLTV parsing for EPG source '{}': {} channels, {} programs",
            source.name,
            channels.len(),
            programs.len()
        );
        Ok((channels, programs, detected_timezone))
    }

    fn parse_timezone(&self, timezone_str: &str) -> Result<chrono_tz::Tz> {
        match timezone_str {
            "UTC" => Ok(chrono_tz::UTC),
            "America/New_York" => Ok(chrono_tz::America::New_York),
            "America/Chicago" => Ok(chrono_tz::America::Chicago),
            "America/Denver" => Ok(chrono_tz::America::Denver),
            "America/Los_Angeles" => Ok(chrono_tz::America::Los_Angeles),
            "Europe/London" => Ok(chrono_tz::Europe::London),
            "Europe/Paris" => Ok(chrono_tz::Europe::Paris),
            "Europe/Berlin" => Ok(chrono_tz::Europe::Berlin),
            "Asia/Tokyo" => Ok(chrono_tz::Asia::Tokyo),
            "Australia/Sydney" => Ok(chrono_tz::Australia::Sydney),
            _ => {
                warn!("Unknown timezone: {}, using UTC", timezone_str);
                Ok(chrono_tz::UTC)
            }
        }
    }

    fn extract_xml_sections(&self, content: &str, tag: &str) -> Option<Vec<String>> {
        info!("Extracting XML sections for tag: {} (no limit)", tag);

        // Use a more efficient regex pattern for large files
        let pattern = format!(r"<{}\s+[^>]*>.*?</{}>", tag, tag);
        let re = match Regex::new(&pattern) {
            Ok(regex) => regex,
            Err(e) => {
                warn!("Failed to compile regex for tag '{}': {}", tag, e);
                return None;
            }
        };

        let start_time = std::time::Instant::now();
        let mut sections = Vec::new();

        for match_result in re.find_iter(content) {
            sections.push(match_result.as_str().to_string());

            // Progress logging every 1000 sections
            if sections.len() % 1000 == 0 {
                info!("Extracted {} {} sections so far", sections.len(), tag);
            }
        }

        let duration = start_time.elapsed();

        info!(
            "Extracted {} {} sections in {:?}",
            sections.len(),
            tag,
            duration
        );

        if sections.is_empty() {
            warn!("No {} sections found in content", tag);
        }

        Some(sections)
    }

    fn parse_channel_xml(&self, xml: &str, source_id: Uuid) -> Option<EpgChannel> {
        // Extract channel ID
        let id_re = Regex::new(r#"id="([^"]+)""#).ok()?;
        let channel_id = id_re.captures(xml)?.get(1)?.as_str().to_string();

        // Extract display name
        let name_re = Regex::new(r"<display-name[^>]*>([^<]+)</display-name>").ok()?;
        let channel_name = name_re
            .captures(xml)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| channel_id.clone());

        // Extract icon/logo
        let icon_re = Regex::new(r#"<icon\s+src="([^"]+)""#).ok()?;
        let channel_logo = icon_re
            .captures(xml)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string());

        Some(EpgChannel {
            id: Uuid::new_v4(),
            source_id,
            channel_id,
            channel_name,
            channel_logo,
            channel_group: None, // XMLTV doesn't typically have groups
            language: None,      // Could be extracted from lang attribute if present
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    }

    fn parse_programme_xml(
        &self,
        xml: &str,
        source: &EpgSource,
        tz: &chrono_tz::Tz,
        time_offset_seconds: i32,
    ) -> Option<EpgProgram> {
        // Extract channel ID
        let channel_re = Regex::new(r#"channel="([^"]+)""#).ok()?;
        let channel_id = channel_re.captures(xml)?.get(1)?.as_str().to_string();

        // Extract start and stop times
        let start_re = Regex::new(r#"start="([^"]+)""#).ok()?;
        let stop_re = Regex::new(r#"stop="([^"]+)""#).ok()?;

        let start_str = start_re.captures(xml)?.get(1)?.as_str();
        let stop_str = stop_re.captures(xml)?.get(1)?.as_str();

        let start_time = self.parse_xmltv_datetime(start_str, tz)?;
        let end_time = self.parse_xmltv_datetime(stop_str, tz)?;

        // Apply time offset using utility function
        let start_time = crate::utils::time::apply_time_offset(start_time, time_offset_seconds);
        let end_time = crate::utils::time::apply_time_offset(end_time, time_offset_seconds);

        // Extract title
        let title_re = Regex::new(r"<title[^>]*>([^<]+)</title>").ok()?;
        let program_title = title_re
            .captures(xml)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "Unknown Program".to_string());

        // Extract description
        let desc_re = Regex::new(r"<desc[^>]*>([^<]+)</desc>").ok()?;
        let program_description = desc_re
            .captures(xml)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string());

        // Extract category
        let category_re = Regex::new(r"<category[^>]*>([^<]+)</category>").ok()?;
        let program_category = category_re
            .captures(xml)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string());

        // Extract episode info
        let episode_re = Regex::new(r"<episode-num[^>]*>([^<]+)</episode-num>").ok()?;
        let episode_info = episode_re
            .captures(xml)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string());

        let (season_num, episode_num) = self.parse_episode_info(&episode_info.unwrap_or_default());

        Some(EpgProgram {
            id: Uuid::new_v4(),
            source_id: source.id,
            channel_id: channel_id.clone(),
            channel_name: channel_id, // We'll update this with actual channel name later
            program_title,
            program_description,
            program_category,
            start_time,
            end_time,
            episode_num,
            season_num,
            rating: None,       // Could be extracted if present
            language: None,     // Could be extracted if present
            subtitles: None,    // Could be extracted if present
            aspect_ratio: None, // Could be extracted if present
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    }

    fn parse_xmltv_datetime(
        &self,
        datetime_str: &str,
        tz: &chrono_tz::Tz,
    ) -> Option<DateTime<Utc>> {
        // XMLTV datetime format: YYYYMMDDHHMMSS [timezone]
        // Example: 20231215120000 +0000

        // Remove timezone suffix if present
        let clean_datetime = datetime_str
            .split_whitespace()
            .next()
            .unwrap_or(datetime_str);

        // Parse the datetime string
        if clean_datetime.len() >= 14 {
            let year: i32 = clean_datetime[0..4].parse().ok()?;
            let month: u32 = clean_datetime[4..6].parse().ok()?;
            let day: u32 = clean_datetime[6..8].parse().ok()?;
            let hour: u32 = clean_datetime[8..10].parse().ok()?;
            let minute: u32 = clean_datetime[10..12].parse().ok()?;
            let second: u32 = clean_datetime[12..14].parse().ok()?;

            // Create naive datetime and convert to timezone, then to UTC
            let naive_dt = chrono::NaiveDate::from_ymd_opt(year, month, day)?
                .and_hms_opt(hour, minute, second)?;

            let local_dt = tz.from_local_datetime(&naive_dt).single()?;
            Some(local_dt.with_timezone(&Utc))
        } else {
            None
        }
    }

    fn parse_episode_info(&self, episode_info: &str) -> (Option<String>, Option<String>) {
        // Try to parse various episode number formats
        // Examples: "S01E05", "1.5", "1/10", etc.

        if episode_info.is_empty() {
            return (None, None);
        }

        // Try S##E## format
        if let Ok(se_re) = Regex::new(r"S(\d+)E(\d+)") {
            if let Some(caps) = se_re.captures(episode_info) {
                let season = caps
                    .get(1)
                    .and_then(|m| m.as_str().parse::<u32>().ok())
                    .map(|s| s.to_string());
                let episode = caps
                    .get(2)
                    .and_then(|m| m.as_str().parse::<u32>().ok())
                    .map(|e| e.to_string());
                return (season, episode);
            }
        }

        // Try ##.## format
        if episode_info.contains('.') {
            let parts: Vec<&str> = episode_info.split('.').collect();
            if parts.len() == 2 {
                let season = parts[0].parse::<u32>().ok().map(|s| s.to_string());
                let episode = parts[1].parse::<u32>().ok().map(|e| e.to_string());
                return (season, episode);
            }
        }

        // Try ##/## format
        if episode_info.contains('/') {
            let parts: Vec<&str> = episode_info.split('/').collect();
            if parts.len() == 2 {
                let episode = parts[0].parse::<u32>().ok().map(|e| e.to_string());
                return (None, episode);
            }
        }

        // Just try to parse as episode number
        if let Ok(episode_num) = episode_info.parse::<u32>() {
            return (None, Some(episode_num.to_string()));
        }

        (None, None)
    }

    pub fn update_channel_names(&self, channels: &[EpgChannel], programs: &mut [EpgProgram]) {
        // Create a mapping of channel ID to channel name
        let channel_map: HashMap<String, String> = channels
            .iter()
            .map(|ch| (ch.channel_id.clone(), ch.channel_name.clone()))
            .collect();

        // Update program channel names
        for program in programs.iter_mut() {
            if let Some(channel_name) = channel_map.get(&program.channel_id) {
                program.channel_name = channel_name.clone();
            }
        }
    }

    // Helper methods for duplicate channel handling
    fn are_channels_identical(&self, channel1: &EpgChannel, channel2: &EpgChannel) -> bool {
        channel1.channel_name == channel2.channel_name
            && channel1.channel_logo == channel2.channel_logo
            && channel1.channel_group == channel2.channel_group
            && channel1.language == channel2.language
    }

    async fn handle_channel_conflict(
        &self,
        source: &EpgSource,
        existing_channel: &EpgChannel,
        conflicting_channel: &EpgChannel,
    ) -> Result<()> {
        // Only save to DLQ if we have a database connection
        if let Some(database) = &self.database {
            // Check if DLQ entry already exists
            let exists = database
                .check_epg_dlq_exists(source.id, &conflicting_channel.channel_id)
                .await?;

            if exists {
                // Just increment occurrence count
                database
                    .increment_epg_dlq_occurrence(source.id, &conflicting_channel.channel_id)
                    .await?;
            } else {
                // Create new DLQ entry
                let dlq_entry = crate::models::EpgDlq {
                    id: uuid::Uuid::new_v4(),
                    source_id: source.id,
                    original_channel_id: conflicting_channel.channel_id.clone(),
                    conflict_type: crate::models::EpgConflictType::DuplicateConflicting,
                    channel_data: serde_json::to_string(conflicting_channel)
                        .unwrap_or_else(|_| "Failed to serialize".to_string()),
                    program_data: None, // Programs are handled separately
                    conflict_details: format!(
                        "Channel '{}' has conflicting data: existing name '{}' vs new name '{}', existing group '{}' vs new group '{}'",
                        conflicting_channel.channel_id,
                        existing_channel.channel_name,
                        conflicting_channel.channel_name,
                        existing_channel.channel_group.as_deref().unwrap_or("None"),
                        conflicting_channel.channel_group.as_deref().unwrap_or("None")
                    ),
                    first_seen_at: chrono::Utc::now(),
                    last_seen_at: chrono::Utc::now(),
                    occurrence_count: 1,
                    resolved: false,
                    resolution_notes: None,
                };

                database.save_epg_dlq_entry(&dlq_entry).await?;
            }
        }

        Ok(())
    }

    /// Save EPG data to database with cancellation support
    pub async fn save_epg_data_with_cancellation(
        &self,
        source_id: Uuid,
        channels: Vec<EpgChannel>,
        programs: Vec<EpgProgram>,
    ) -> Result<(usize, usize)> {
        if let Some(database) = &self.database {
            // Get cancellation receiver if state manager is available
            let cancellation_rx = if let Some(state_manager) = &self.state_manager {
                state_manager.get_cancellation_receiver(source_id).await
            } else {
                None
            };

            // Use the cancellation-aware database method
            database
                .update_epg_source_data_with_cancellation(
                    source_id,
                    channels,
                    programs,
                    cancellation_rx,
                )
                .await
        } else {
            Err(anyhow!("No database connection available"))
        }
    }
}

impl Default for EpgIngestor {
    fn default() -> Self {
        Self::new_without_database()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_episode_info() {
        let ingestor = EpgIngestor::new_without_database();

        // Test S##E## format
        let (season, episode) = ingestor.parse_episode_info("S01E05");
        assert_eq!(season, Some("1".to_string()));
        assert_eq!(episode, Some("5".to_string()));

        // Test ##.## format
        let (season, episode) = ingestor.parse_episode_info("2.10");
        assert_eq!(season, Some("2".to_string()));
        assert_eq!(episode, Some("10".to_string()));

        // Test ##/## format
        let (season, episode) = ingestor.parse_episode_info("15/20");
        assert_eq!(season, None);
        assert_eq!(episode, Some("15".to_string()));

        // Test single number
        let (season, episode) = ingestor.parse_episode_info("42");
        assert_eq!(season, None);
        assert_eq!(episode, Some("42".to_string()));

        // Test empty
        let (season, episode) = ingestor.parse_episode_info("");
        assert_eq!(season, None);
        assert_eq!(episode, None);
    }

    #[test]
    fn test_parse_xmltv_datetime() {
        let ingestor = EpgIngestor::new_without_database();
        let tz = chrono_tz::UTC;

        // Test valid datetime
        let dt = ingestor.parse_xmltv_datetime("20231215120000", &tz);
        assert!(dt.is_some());

        let dt = dt.unwrap();
        assert_eq!(dt.year(), 2023);
        assert_eq!(dt.month(), 12);
        assert_eq!(dt.day(), 15);
        assert_eq!(dt.hour(), 12);
        assert_eq!(dt.minute(), 0);
        assert_eq!(dt.second(), 0);
    }
}
