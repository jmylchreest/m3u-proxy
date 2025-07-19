//! EPG Ingestor - Robust XMLTV parsing and ingestion
//!
//! This module handles ingestion of EPG (Electronic Program Guide) data from XMLTV sources.
//! It uses the xmltv crate for reliable parsing and handles deduplication through the
//! data mapping system rather than a DLQ (Dead Letter Queue) approach.
//!
//! Key features:
//! - Robust XMLTV parsing using the xmltv crate with quick-xml
//! - Cancellation support for long-running operations
//! - Progress tracking and reporting
//! - Channel deduplication during ingestion (same source)
//! - Cross-source deduplication handled by data mapping during proxy generation

use crate::database::Database;
use crate::models::*;
use crate::utils::time::{detect_timezone_from_xmltv, log_timezone_detection, parse_time_offset};
use crate::utils::url::UrlUtils;
use anyhow::{Result, anyhow};
use chrono::{DateTime, TimeZone, Utc};
#[cfg(test)]
use chrono::{Datelike, Timelike};
use quick_xml::de::from_str;
use regex::Regex;
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use xmltv::{Channel, Programme, Tv};

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

    /// Shared EPG refresh function used by both manual refresh and scheduler
    /// This ensures identical behavior and eliminates code duplication
    pub async fn refresh_epg_source(
        database: Database,
        state_manager: crate::ingestor::state_manager::IngestionStateManager,
        source: &EpgSource,
        trigger: crate::ingestor::state_manager::ProcessingTrigger,
    ) -> Result<(usize, usize), Box<dyn std::error::Error + Send + Sync>> {
        use tracing::{error, info};

        let start_time = std::time::Instant::now();
        let source_id = source.id;
        let source_name = source.name.clone();

        info!(
            "Starting EPG refresh for source '{}' ({}) - trigger: {:?}",
            source_name, source_id, trigger
        );

        let ingestor = EpgIngestor::new_with_state_manager(database.clone(), state_manager.clone());

        match ingestor
            .ingest_epg_source_with_trigger(source, trigger.clone())
            .await
        {
            Ok((channels, mut programs, _detected_timezone)) => {
                // Update channel names in programs
                ingestor.update_channel_names(&channels, &mut programs);

                // Note: Timezone detection was simplified in migration 004
                // All times are normalized to UTC during ingestion

                // Save to database using cancellation-aware method
                match ingestor
                    .save_epg_data_with_cancellation(source_id, channels, programs)
                    .await
                {
                    Ok((channel_count, program_count)) => {
                        // Update last ingested timestamp
                        if let Err(e) = database.update_epg_source_last_ingested(source_id).await {
                            error!(
                                "Failed to update last_ingested_at for EPG source '{}': {}",
                                source_name, e
                            );
                        }

                        // Ensure state manager is updated to completed status
                        state_manager
                            .complete_ingestion_with_programs(
                                source_id,
                                channel_count,
                                Some(program_count),
                            )
                            .await;

                        let duration = start_time.elapsed();
                        info!(
                            "EPG refresh completed source={} channels={} programs={} trigger={:?} duration={}",
                            source_name, channel_count, program_count, trigger, 
                            crate::utils::format_duration(duration.as_millis() as u64)
                        );

                        Ok((channel_count, program_count))
                    }
                    Err(e) => {
                        // Ensure state manager is updated to error status
                        state_manager.set_error(source_id, e.to_string()).await;

                        error!(
                            "Failed to save EPG data for source '{}': {}",
                            source_name, e
                        );
                        Err(e.into())
                    }
                }
            }
            Err(e) => {
                // Ensure state manager is updated to error status for ingestion failure
                state_manager.set_error(source_id, e.to_string()).await;

                error!("Failed to refresh EPG source '{}': {}", source_name, e);
                Err(e.into())
            }
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
                        programs_parsed: None,
                        programs_saved: None,
                        percentage: Some(5.0),
                    },
                )
                .await;
        }

        let result = match source.source_type {
            EpgSourceType::Xmltv => self.ingest_xmltv_source(source).await,
            EpgSourceType::Xtream => self.ingest_xtream_source(source).await,
        };

        // Note: Timezone auto-detection was simplified in migration 004
        // All times are now normalized to UTC during processing

        // Update progress based on result
        if let Some(state_manager) = &self.state_manager {
            match &result {
                Ok((channels, programs, _)) => {
                    state_manager
                        .complete_ingestion_with_programs(
                            source.id,
                            channels.len(),
                            Some(programs.len()),
                        )
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
        info!(
            "Fetching XMLTV data from: {}",
            crate::utils::url::UrlUtils::obfuscate_credentials(&source.url)
        );

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
                        programs_parsed: None,
                        programs_saved: None,
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
                        programs_parsed: None,
                        programs_saved: None,
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

        info!(
            "Fetching EPG data from Xtream Codes: {}",
            crate::utils::url::UrlUtils::obfuscate_credentials(&source.url)
        );

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
                        programs_parsed: None,
                        programs_saved: None,
                        percentage: Some(15.0),
                    },
                )
                .await;
        }

        // First, get the EPG data endpoint with proper URL scheme
        let normalized_base_url = UrlUtils::normalize_scheme(&source.url);
        let epg_url = format!(
            "{}/xmltv.php?username={}&password={}",
            normalized_base_url, username, password
        );

        info!(
            "Fetching Xtream EPG from: {}",
            UrlUtils::obfuscate_credentials(&epg_url)
        );

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
                        programs_parsed: None,
                        programs_saved: None,
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
        // Use detected timezone if available, otherwise use original_timezone or default to UTC
        let timezone_to_use = if let Some(ref detected_tz) = detected_timezone {
            log_timezone_detection(&source.name, Some(detected_tz), detected_tz);
            detected_tz
        } else if let Some(ref original_tz) = source.original_timezone {
            log_timezone_detection(&source.name, None, original_tz);
            original_tz
        } else {
            log_timezone_detection(&source.name, None, "UTC");
            "UTC"
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

        // Use the proper XMLTV library for robust parsing
        info!("Starting XMLTV parsing using xmltv library with quick-xml");

        // Parse the entire XMLTV document
        let xmltv_data: Tv = from_str(content)
            .map_err(|e| anyhow!("Failed to parse XMLTV content with xmltv library: {}", e))?;

        let mut channels = Vec::new();
        let mut programs = Vec::new();
        let mut seen_channel_ids = std::collections::HashSet::new();

        // Process channels from the parsed XMLTV data
        info!(
            "Processing {} channels from XMLTV data",
            xmltv_data.channels.len()
        );

        for (i, xmltv_channel) in xmltv_data.channels.iter().enumerate() {
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
                                xmltv_data.channels.len()
                            ));
                        }
                    }
                }
            }

            if i % 100 == 0 && i > 0 {
                debug!("Processed {}/{} channels", i, xmltv_data.channels.len());
                // Update progress for channels
                if let Some(state_manager) = &self.state_manager {
                    let percentage = 40.0 + (i as f64 / xmltv_data.channels.len() as f64) * 20.0;
                    state_manager
                        .update_progress(
                            source.id,
                            crate::models::IngestionState::Parsing,
                            crate::models::ProgressInfo {
                                current_step: format!(
                                    "Parsing channels ({}/{})",
                                    i,
                                    xmltv_data.channels.len()
                                ),
                                total_bytes: Some(content.len() as u64),
                                downloaded_bytes: Some(content.len() as u64),
                                channels_parsed: Some(i),
                                channels_saved: None,
                                programs_parsed: None,
                                programs_saved: None,
                                percentage: Some(percentage),
                            },
                        )
                        .await;
                }
            }

            // Skip duplicate channel_ids within the same source to respect database constraints
            // Cross-source duplicates will be handled by data mapping during proxy generation
            if seen_channel_ids.contains(&xmltv_channel.id) {
                continue;
            }
            seen_channel_ids.insert(xmltv_channel.id.clone());

            let channel = self.convert_xmltv_channel(xmltv_channel, source.id);
            channels.push(channel);
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

        // Process programs from the parsed XMLTV data
        info!(
            "Processing {} programs from XMLTV data",
            xmltv_data.programmes.len()
        );

        for (i, xmltv_program) in xmltv_data.programmes.iter().enumerate() {
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
                                xmltv_data.programmes.len()
                            ));
                        }
                    }
                }
            }

            if i % 1000 == 0 && i > 0 {
                debug!("Processed {}/{} programmes", i, xmltv_data.programmes.len());
                // Update progress for programmes
                if let Some(state_manager) = &self.state_manager {
                    let percentage = 60.0 + (i as f64 / xmltv_data.programmes.len() as f64) * 30.0;
                    state_manager
                        .update_progress(
                            source.id,
                            crate::models::IngestionState::Parsing,
                            crate::models::ProgressInfo {
                                current_step: format!(
                                    "Parsing programmes ({}/{})",
                                    i,
                                    xmltv_data.programmes.len()
                                ),
                                total_bytes: Some(content.len() as u64),
                                downloaded_bytes: Some(content.len() as u64),
                                channels_parsed: Some(channels.len()),
                                channels_saved: None,
                                programs_parsed: Some(i),
                                programs_saved: None,
                                percentage: Some(percentage),
                            },
                        )
                        .await;
                }
            }

            if let Some(program) =
                self.convert_xmltv_program(xmltv_program, source, &tz, time_offset_seconds)
            {
                programs.push(program);
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

    fn convert_xmltv_channel(&self, xmltv_channel: &Channel, source_id: Uuid) -> EpgChannel {
        let channel_name = xmltv_channel
            .display_names
            .first()
            .map(|name| name.name.clone())
            .unwrap_or_else(|| xmltv_channel.id.clone());

        let channel_logo = xmltv_channel.icons.first().cloned();

        EpgChannel {
            id: Uuid::new_v4(),
            source_id,
            channel_id: xmltv_channel.id.clone(),
            channel_name,
            channel_logo,
            channel_group: None, // XMLTV doesn't typically have groups
            language: None,      // Could be extracted from lang attribute if present
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn convert_xmltv_program(
        &self,
        xmltv_program: &Programme,
        source: &EpgSource,
        tz: &chrono_tz::Tz,
        time_offset_seconds: i32,
    ) -> Option<EpgProgram> {
        // Convert start and stop times
        let start_time = self.parse_xmltv_datetime(&xmltv_program.start, tz)?;
        let end_time = xmltv_program
            .stop
            .as_ref()
            .and_then(|stop| self.parse_xmltv_datetime(stop, tz))?;

        // Apply time offset using utility function
        let start_time = crate::utils::time::apply_time_offset(start_time, time_offset_seconds);
        let end_time = crate::utils::time::apply_time_offset(end_time, time_offset_seconds);

        // Extract title
        let program_title = xmltv_program
            .titles
            .first()
            .map(|title| title.value.clone())
            .unwrap_or_else(|| "Unknown Program".to_string());

        // Extract description
        let program_description = xmltv_program
            .descriptions
            .first()
            .map(|desc| desc.value.clone());

        // Extract category
        let program_category = xmltv_program
            .categories
            .first()
            .map(|category| category.name.clone());

        // Extract episode info
        let episode_info = xmltv_program
            .episode_num
            .first()
            .map(|ep| ep.value.clone())
            .unwrap_or_default();

        let (season_num, episode_num) = self.parse_episode_info(&episode_info);

        // Extract rating
        let rating = xmltv_program
            .ratings
            .first()
            .map(|rating| rating.value.clone());

        // Extract language
        let language = xmltv_program
            .language
            .as_ref()
            .map(|lang| lang.value.clone());

        // Extract program icon if available
        let program_icon = xmltv_program.icons.first().map(|icon| icon.src.clone());

        Some(EpgProgram {
            id: Uuid::new_v4(),
            source_id: source.id,
            channel_id: xmltv_program.channel.clone(),
            channel_name: xmltv_program.channel.clone(), // We'll update this with actual channel name later
            program_title,
            program_description,
            program_category,
            start_time,
            end_time,
            episode_num,
            season_num,
            rating,
            language,
            subtitles: None,    // Could be extracted if present
            aspect_ratio: None, // Could be extracted if present
            program_icon,
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

    /// Save EPG data to database with cancellation support
    pub async fn save_epg_data_with_cancellation(
        &self,
        source_id: Uuid,
        channels: Vec<EpgChannel>,
        programs: Vec<EpgProgram>,
    ) -> Result<(usize, usize)> {
        if let Some(database) = &self.database {
            // Set progress to saving state
            if let Some(state_manager) = &self.state_manager {
                use crate::models::IngestionState;
                use crate::models::ProgressInfo;

                state_manager
                    .update_progress(
                        source_id,
                        IngestionState::Saving,
                        ProgressInfo {
                            current_step: format!(
                                "Saving {} channels and {} programs to database",
                                channels.len(),
                                programs.len()
                            ),
                            total_bytes: None,
                            downloaded_bytes: None,
                            channels_parsed: Some(channels.len()),
                            channels_saved: None,
                            programs_parsed: Some(programs.len()),
                            programs_saved: None,
                            percentage: None,
                        },
                    )
                    .await;
            }

            // Get cancellation receiver if state manager is available
            let cancellation_rx = if let Some(state_manager) = &self.state_manager {
                state_manager.get_cancellation_receiver(source_id).await
            } else {
                None
            };

            // Create progress callback that updates state manager
            let _total_programs = programs.len();
            let channels_count = channels.len();
            let state_manager_clone = self.state_manager.clone();
            let progress_callback = move |programs_saved: usize, total: usize| {
                if let Some(ref state_manager) = state_manager_clone {
                    let percentage = if total > 0 {
                        90.0 + (programs_saved as f64 / total as f64) * 10.0
                    } else {
                        95.0
                    };

                    // Use spawn to avoid blocking the database operation
                    let state_manager_inner = state_manager.clone();
                    tokio::spawn(async move {
                        let _ = state_manager_inner
                            .update_progress(
                                source_id,
                                crate::models::IngestionState::Saving,
                                crate::models::ProgressInfo {
                                    current_step: format!(
                                        "Saved {}/{} programs to database",
                                        programs_saved, total
                                    ),
                                    total_bytes: None,
                                    downloaded_bytes: None,
                                    channels_parsed: Some(channels_count),
                                    channels_saved: Some(channels_count),
                                    programs_parsed: Some(total),
                                    programs_saved: Some(programs_saved),
                                    percentage: Some(percentage),
                                },
                            )
                            .await;
                    });
                }
            };

            // Use the cancellation-aware database method with progress updates
            // Add timeout to prevent hanging (30 minutes max for EPG operations)
            let database_operation = database
                .update_epg_source_data_with_cancellation_and_progress(
                    source_id,
                    channels,
                    programs,
                    cancellation_rx,
                    Some(progress_callback),
                );

            let result = match timeout(Duration::from_secs(1800), database_operation).await {
                Ok(db_result) => db_result,
                Err(_) => {
                    error!(
                        "EPG database operation timed out after 30 minutes for source {}",
                        source_id
                    );
                    Err(anyhow!("Database operation timed out"))
                }
            };

            // Always update state manager, whether successful or not
            if let Some(state_manager) = &self.state_manager {
                match &result {
                    Ok((channels_saved, programs_saved)) => {
                        // Mark ingestion as completed on success
                        state_manager
                            .complete_ingestion_with_programs(
                                source_id,
                                *channels_saved,
                                Some(*programs_saved),
                            )
                            .await;
                    }
                    Err(e) => {
                        // Mark ingestion as failed on error
                        state_manager.set_error(source_id, e.to_string()).await;
                    }
                }
            }

            result
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
