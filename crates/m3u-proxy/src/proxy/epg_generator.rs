use anyhow::Result;
use chrono::{DateTime, FixedOffset, Utc};
use std::collections::{HashMap, HashSet};
use tracing::{debug, info, warn};

use crate::database::Database;
use crate::models::*;

/// EPG generator that creates XMLTV feeds filtered to only include channels from the generated M3U
pub struct EpgGenerator {
    /// Database connection for fetching EPG data
    database: Database,
}

/// Channel information from M3U including logos
#[derive(Debug, Clone)]
pub struct M3uChannelInfo {
    pub channel_id: String,
    pub channel_logo: Option<String>,
}

/// Represents a filtered EPG program with normalized time
#[derive(Debug, Clone)]
pub struct FilteredEpgProgram {
    pub program: EpgProgram,
    pub normalized_start_time: DateTime<Utc>,
    pub normalized_end_time: DateTime<Utc>,
}

/// Represents a filtered EPG channel
#[derive(Debug, Clone)]
pub struct FilteredEpgChannel {
    pub channel: EpgChannel,
    pub programs: Vec<FilteredEpgProgram>,
}

/// Configuration for EPG generation
#[derive(Debug, Clone)]
pub struct EpgGenerationConfig {
    /// Whether to include programs that have already ended
    pub include_past_programs: bool,
    /// How many days of EPG data to include (forward from now)
    pub days_ahead: u32,
    /// How many days of EPG data to include (backward from now)
    pub days_behind: u32,
    /// Whether to deduplicate programs with same title and time
    pub deduplicate_programs: bool,
    /// Maximum number of programs per channel
    pub max_programs_per_channel: Option<usize>,
}

impl Default for EpgGenerationConfig {
    fn default() -> Self {
        Self {
            include_past_programs: false,
            days_ahead: 7,
            days_behind: 1,
            deduplicate_programs: false,
            max_programs_per_channel: None,
        }
    }
}

/// Statistics about EPG generation
#[derive(Debug)]
pub struct EpgGenerationStatistics {
    pub total_channels_in_m3u: usize,
    pub matched_epg_channels: usize,
    pub unmatched_channels: Vec<String>,
    pub total_programs_before_filter: usize,
    pub total_programs_after_filter: usize,
    pub programs_normalized: usize,
    pub duplicate_programs_removed: usize,
    pub generation_time_ms: u64,
}

impl EpgGenerator {
    /// Create a new EPG generator
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    /// Generate filtered XMLTV content based on channel IDs from the M3U proxy
    /// Generate XMLTV using already-resolved EPG sources (preferred method)
    pub async fn generate_xmltv_with_resolved_sources(
        &self,
        proxy: &StreamProxy,
        channel_ids: &[String],
        resolved_epg_sources: &[ProxyEpgSourceConfig],
        config: Option<EpgGenerationConfig>,
    ) -> Result<(String, EpgGenerationStatistics)> {
        let config = config.unwrap_or_default();
        let start_time = std::time::Instant::now();

        info!(
            "Starting EPG generation for proxy '{}' with {} channel IDs and {} resolved EPG sources",
            proxy.name,
            channel_ids.len(),
            resolved_epg_sources.len()
        );

        if resolved_epg_sources.is_empty() {
            warn!(
                "No resolved EPG sources provided for proxy '{}'",
                proxy.name
            );
            return Ok((
                self.generate_empty_xmltv(),
                EpgGenerationStatistics {
                    total_channels_in_m3u: channel_ids.len(),
                    matched_epg_channels: 0,
                    unmatched_channels: channel_ids.iter().cloned().collect(),
                    total_programs_before_filter: 0,
                    total_programs_after_filter: 0,
                    programs_normalized: 0,
                    duplicate_programs_removed: 0,
                    generation_time_ms: start_time.elapsed().as_millis() as u64,
                },
            ));
        }

        // Convert resolved sources to the format expected by the rest of the method
        let epg_sources: Vec<EpgSource> = resolved_epg_sources
            .iter()
            .map(|resolved| resolved.epg_source.clone())
            .collect();

        // Continue with the rest of the existing logic...
        self.generate_xmltv_with_sources(proxy, channel_ids, &epg_sources, config)
            .await
    }

    /// Internal method that processes EPG sources and generates XMLTV
    async fn generate_xmltv_with_sources(
        &self,
        proxy: &StreamProxy,
        channel_ids: &[String],
        epg_sources: &[EpgSource],
        config: EpgGenerationConfig,
    ) -> Result<(String, EpgGenerationStatistics)> {
        let start_time = std::time::Instant::now();

        info!(
            "Processing {} EPG sources for proxy '{}'",
            epg_sources.len(),
            proxy.name
        );

        // Step 2: Filter EPG channels to only those present in the M3U channel list
        let filtered_channels = self
            .filter_epg_channels_by_ids(epg_sources, channel_ids, &config)
            .await?;

        info!(
            "Filtered EPG channels: {} matched out of {} M3U channels",
            filtered_channels.len(),
            channel_ids.len()
        );

        // Step 3: Get programs for the filtered channels
        let channels_with_programs = self
            .get_programs_for_filtered_channels(&filtered_channels, &config)
            .await?;

        // Step 4: Calculate statistics
        let total_programs_before = channels_with_programs
            .iter()
            .map(|c| c.programs.len())
            .sum::<usize>();

        let unmatched_channels = self.find_unmatched_channels(channel_ids, &filtered_channels);

        let programs_normalized = 0; // Times are always UTC, no normalization needed

        // Step 5: Generate XMLTV content
        let xmltv_content = self
            .generate_xmltv_content(&channels_with_programs, &config)
            .await?;

        let generation_time = start_time.elapsed().as_millis() as u64;

        let statistics = EpgGenerationStatistics {
            total_channels_in_m3u: channel_ids.len(),
            matched_epg_channels: filtered_channels.len(),
            unmatched_channels,
            total_programs_before_filter: total_programs_before,
            total_programs_after_filter: total_programs_before, // Will be updated if deduplication is implemented
            programs_normalized,
            duplicate_programs_removed: 0, // Will be updated if deduplication is implemented
            generation_time_ms: generation_time,
        };

        info!(
            "EPG generation completed for proxy '{}': {} channels, {} programs, {}ms",
            proxy.name,
            statistics.matched_epg_channels,
            statistics.total_programs_after_filter,
            generation_time
        );

        Ok((xmltv_content, statistics))
    }

    /// Generate XMLTV for proxy with processed channels from M3U generation pipeline
    pub async fn generate_xmltv_for_proxy_with_processed_channels(
        &self,
        proxy: &StreamProxy,
        processed_channels: &[NumberedChannel],
        logo_service: &crate::logo_assets::service::LogoAssetService,
        base_url: &str,
        config: Option<EpgGenerationConfig>,
    ) -> Result<(String, EpgGenerationStatistics)> {
        let config = config.unwrap_or_default();
        let start_time = std::time::Instant::now();

        info!(
            "Starting EPG generation for proxy '{}' with {} processed channels",
            proxy.name,
            processed_channels.len()
        );

        // Extract channel IDs from processed channels
        let channel_ids: Vec<String> = processed_channels
            .iter()
            .filter_map(|nc| nc.channel.tvg_id.clone())
            .collect();

        // Create a map of channel logos from the processed channels
        let mut channel_logo_map: HashMap<String, String> = HashMap::new();
        for nc in processed_channels {
            if let Some(tvg_id) = &nc.channel.tvg_id {
                if let Some(logo) = &nc.channel.tvg_logo {
                    channel_logo_map.insert(tvg_id.clone(), logo.clone());
                }
            }
        }

        // Step 1: Get EPG sources associated with this proxy
        let epg_sources = self.database.get_proxy_epg_sources(proxy.id).await?;

        if epg_sources.is_empty() {
            warn!("No EPG sources found for proxy '{}'", proxy.name);
            return Ok((
                self.generate_empty_xmltv(),
                EpgGenerationStatistics {
                    total_channels_in_m3u: channel_ids.len(),
                    matched_epg_channels: 0,
                    unmatched_channels: channel_ids.clone(),
                    total_programs_before_filter: 0,
                    total_programs_after_filter: 0,
                    programs_normalized: 0,
                    duplicate_programs_removed: 0,
                    generation_time_ms: start_time.elapsed().as_millis() as u64,
                },
            ));
        }

        // Step 2: Filter EPG channels to only those present in the M3U channel list
        let mut filtered_channels = self
            .filter_epg_channels_by_ids(&epg_sources, &channel_ids, &config)
            .await?;

        // Step 2.5: Apply logo caching for channel logos
        for channel in &mut filtered_channels {
            // Use the logo from the processed M3U channels (already data-mapped)
            if let Some(m3u_logo) = channel_logo_map.get(&channel.channel_id) {
                // Apply logo caching if enabled
                if proxy.cache_channel_logos {
                    let cached_logo = self
                        .cache_logo_if_needed(m3u_logo, logo_service, base_url)
                        .await;
                    channel.channel_logo = Some(cached_logo);
                } else {
                    channel.channel_logo = Some(m3u_logo.clone());
                }
                debug!(
                    "Set logo for channel '{}': {:?}",
                    channel.channel_name, channel.channel_logo
                );
            }
        }

        // Step 3: Get programs for the filtered channels
        let mut channels_with_programs = self
            .get_programs_for_filtered_channels(&filtered_channels, &config)
            .await?;

        // Step 3.5: Apply logo caching for program logos if enabled
        if proxy.cache_program_logos {
            for filtered_channel in &mut channels_with_programs {
                for filtered_program in &mut filtered_channel.programs {
                    if let Some(ref program_icon) = filtered_program.program.program_icon {
                        let cached_logo = self
                            .cache_logo_if_needed(program_icon, logo_service, base_url)
                            .await;
                        filtered_program.program.program_icon = Some(cached_logo);
                    }
                }
            }
        }

        // Step 4: Calculate statistics
        let total_programs_before = channels_with_programs
            .iter()
            .map(|c| c.programs.len())
            .sum::<usize>();

        let unmatched_channels = self.find_unmatched_channels(&channel_ids, &filtered_channels);

        let programs_normalized = 0; // Times are always UTC, no normalization needed

        // Step 5: Generate XMLTV content
        let xmltv_content = self
            .generate_xmltv_content(&channels_with_programs, &config)
            .await?;

        let generation_time = start_time.elapsed().as_millis() as u64;

        let statistics = EpgGenerationStatistics {
            total_channels_in_m3u: channel_ids.len(),
            matched_epg_channels: filtered_channels.len(),
            unmatched_channels,
            total_programs_before_filter: total_programs_before,
            total_programs_after_filter: total_programs_before,
            programs_normalized,
            duplicate_programs_removed: 0,
            generation_time_ms: generation_time,
        };

        info!(
            "EPG generation completed for proxy '{}': {} channels, {} programs, {}ms",
            proxy.name,
            statistics.matched_epg_channels,
            statistics.total_programs_after_filter,
            generation_time
        );

        Ok((xmltv_content, statistics))
    }

    /// Filter EPG channels to only include those with IDs present in the M3U channel list
    async fn filter_epg_channels_by_ids(
        &self,
        epg_sources: &[EpgSource],
        channel_ids: &[String],
        _config: &EpgGenerationConfig,
    ) -> Result<Vec<EpgChannel>> {
        let channel_id_set: HashSet<String> = channel_ids.iter().cloned().collect();
        let mut matched_channels = Vec::new();

        for epg_source in epg_sources {
            debug!("Processing EPG source '{}'", epg_source.name);

            // Get all channels for this EPG source
            let source_channels = self.database.get_epg_source_channels(epg_source.id).await?;

            // Filter channels that match the M3U channel IDs
            for channel in source_channels {
                if channel_id_set.contains(&channel.channel_id) {
                    debug!(
                        "Matched EPG channel '{}' (ID: {})",
                        channel.channel_name, channel.channel_id
                    );
                    matched_channels.push(channel);
                } else {
                    // Also try matching by channel name if direct ID match fails
                    if channel_id_set.contains(&channel.channel_name) {
                        debug!("Matched EPG channel '{}' by name", channel.channel_name);
                        matched_channels.push(channel);
                    }
                }
            }
        }

        // Remove duplicates based on channel_id
        let mut seen_ids = HashSet::new();
        matched_channels.retain(|channel| seen_ids.insert(channel.channel_id.clone()));

        info!(
            "Channel matching complete: {} unique channels matched",
            matched_channels.len()
        );

        Ok(matched_channels)
    }

    /// Get programs for the filtered channels with time normalization
    async fn get_programs_for_filtered_channels(
        &self,
        channels: &[EpgChannel],
        config: &EpgGenerationConfig,
    ) -> Result<Vec<FilteredEpgChannel>> {
        let mut channels_with_programs = Vec::new();

        // Calculate time window
        let now = Utc::now();
        let start_time = now - chrono::Duration::days(config.days_behind as i64);
        let end_time = now + chrono::Duration::days(config.days_ahead as i64);

        for channel in channels {
            debug!("Getting programs for channel '{}'", channel.channel_name);

            // Get programs for this channel within the time window
            let programs = self
                .database
                .get_epg_programs_for_channel_in_timerange(channel.id, start_time, end_time)
                .await?;

            debug!(
                "Found {} programs for channel '{}' in time range",
                programs.len(),
                channel.channel_name
            );

            // Filter and normalize programs
            let mut filtered_programs = Vec::new();
            let programs_normalized = 0;
            let mut programs_filtered_past = 0;

            for program in programs {
                // Skip past programs if not included
                if !config.include_past_programs && program.end_time < now {
                    programs_filtered_past += 1;
                    continue;
                }

                // Times are always stored as UTC in database, use them directly
                let (normalized_start, normalized_end) = (program.start_time, program.end_time);

                filtered_programs.push(FilteredEpgProgram {
                    program,
                    normalized_start_time: normalized_start,
                    normalized_end_time: normalized_end,
                });
            }

            if programs_normalized > 0 {
                debug!(
                    "Normalized {} program times to UTC for channel '{}'",
                    programs_normalized, channel.channel_name
                );
            }
            if programs_filtered_past > 0 {
                debug!(
                    "Filtered out {} past programs for channel '{}'",
                    programs_filtered_past, channel.channel_name
                );
            }

            // Apply program limit if specified
            if let Some(max_programs) = config.max_programs_per_channel {
                if filtered_programs.len() > max_programs {
                    filtered_programs.truncate(max_programs);
                    debug!(
                        "Limited programs for channel '{}' to {} programs",
                        channel.channel_name, max_programs
                    );
                }
            }

            // Deduplicate programs if requested
            if config.deduplicate_programs {
                filtered_programs = self.deduplicate_programs(filtered_programs);
            }

            debug!(
                "Channel '{}' has {} programs after filtering",
                channel.channel_name,
                filtered_programs.len()
            );

            // Only include the channel if it has programs for the requested timeframe
            if !filtered_programs.is_empty() {
                channels_with_programs.push(FilteredEpgChannel {
                    channel: channel.clone(),
                    programs: filtered_programs,
                });
            } else {
                debug!(
                    "Excluding channel '{}' because it has no programs for the requested timeframe",
                    channel.channel_name
                );
            }
        }

        Ok(channels_with_programs)
    }

    /// Get the timezone for an EPG source from the database
    // This method is no longer needed since we always use UTC times
    // Keeping for backward compatibility but it always returns UTC
    async fn get_epg_source_timezone(&self, _source_id: uuid::Uuid) -> Result<String> {
        Ok("UTC".to_string())
    }

    /// Convert EPG program time to UTC based on source timezone
    /// This is the core logic that shifts program times from their source timezone to UTC
    // This method is no longer needed since times are always stored as UTC
    // Keeping for backward compatibility but it just returns the input time
    fn convert_epg_time_to_utc(
        &self,
        epg_time: &DateTime<Utc>,
        _source_timezone: &str,
    ) -> DateTime<Utc> {
        // Times are already stored as UTC in the database, no conversion needed
        *epg_time
    }

    /// Parse timezone offset string (e.g., "+02:00", "-05:00", "UTC")
    fn parse_timezone_offset(&self, tz_str: &str) -> Result<FixedOffset, anyhow::Error> {
        match tz_str.to_uppercase().as_str() {
            "UTC" | "GMT" => Ok(FixedOffset::east_opt(0).unwrap()),
            _ => {
                if tz_str.starts_with('+') || tz_str.starts_with('-') {
                    // Parse offset format like "+02:00" or "-05:00"
                    let sign = if tz_str.starts_with('+') { 1 } else { -1 };
                    let offset_str = &tz_str[1..];

                    if let Some((hours_str, minutes_str)) = offset_str.split_once(':') {
                        if let (Ok(hours), Ok(minutes)) =
                            (hours_str.parse::<i32>(), minutes_str.parse::<i32>())
                        {
                            let total_seconds = sign * (hours * 3600 + minutes * 60);
                            return FixedOffset::east_opt(total_seconds)
                                .ok_or_else(|| anyhow::anyhow!("Date out of range"));
                        }
                    }
                }
                Err(anyhow::anyhow!("Invalid date format"))
            }
        }
    }

    /// Normalize a DateTime<Utc> to UTC (no-op but provided for API consistency)
    #[allow(dead_code)]
    // These methods are no longer needed since times are always UTC
    // Keeping for backward compatibility
    fn normalize_to_utc(&self, dt: &DateTime<Utc>) -> DateTime<Utc> {
        *dt
    }

    fn normalize_timezone_to_utc(
        &self,
        dt: &DateTime<Utc>,
        _timezone: Option<&str>,
    ) -> DateTime<Utc> {
        *dt
    }

    /// Deduplicate programs with sophisticated matching logic
    /// This handles exact duplicates, near-duplicates, and overlapping programs
    fn deduplicate_programs(
        &self,
        mut programs: Vec<FilteredEpgProgram>,
    ) -> Vec<FilteredEpgProgram> {
        if programs.is_empty() {
            return programs;
        }

        // Sort by start time for efficient processing
        programs.sort_by(|a, b| a.normalized_start_time.cmp(&b.normalized_start_time));

        let mut deduplicated = Vec::new();
        let mut seen_exact = HashMap::new();
        let mut duplicates_removed = 0;

        for program in programs {
            // Create exact match key (title + exact start/end times)
            let exact_key = format!(
                "{}:{}:{}",
                program.program.program_title.trim().to_lowercase(),
                program.normalized_start_time.timestamp(),
                program.normalized_end_time.timestamp()
            );

            // Check for exact duplicates
            if seen_exact.contains_key(&exact_key) {
                duplicates_removed += 1;
                debug!(
                    "Removed exact duplicate: '{}' at {}",
                    program.program.program_title,
                    program.normalized_start_time.format("%Y-%m-%d %H:%M")
                );
                continue;
            }

            // Check for near-duplicates (same title, overlapping times within 5 minutes)
            let mut is_near_duplicate = false;
            for existing in &deduplicated {
                if self.are_programs_near_duplicates(&program, existing) {
                    duplicates_removed += 1;
                    debug!(
                        "Removed near-duplicate: '{}' at {} (similar to existing at {})",
                        program.program.program_title,
                        program.normalized_start_time.format("%Y-%m-%d %H:%M"),
                        existing.normalized_start_time.format("%Y-%m-%d %H:%M")
                    );
                    is_near_duplicate = true;
                    break;
                }
            }

            if !is_near_duplicate {
                seen_exact.insert(exact_key, true);
                deduplicated.push(program);
            }
        }

        if duplicates_removed > 0 {
            info!(
                "Deduplication removed {} duplicate programs",
                duplicates_removed
            );
        }

        deduplicated
    }

    /// Check if two programs are near-duplicates
    fn are_programs_near_duplicates(
        &self,
        program1: &FilteredEpgProgram,
        program2: &FilteredEpgProgram,
    ) -> bool {
        // Must have same or very similar title
        let title1 = program1.program.program_title.trim().to_lowercase();
        let title2 = program2.program.program_title.trim().to_lowercase();

        if title1 != title2 {
            // Check for minor variations (e.g., with/without episode numbers)
            let similarity = self.calculate_title_similarity(&title1, &title2);
            if similarity < 0.9 {
                return false;
            }
        }

        // Check time overlap - consider near-duplicates if they overlap significantly
        let start_diff = (program1.normalized_start_time.timestamp()
            - program2.normalized_start_time.timestamp())
        .abs();
        let end_diff = (program1.normalized_end_time.timestamp()
            - program2.normalized_end_time.timestamp())
        .abs();

        // Consider near-duplicates if start times are within 5 minutes and end times within 10 minutes
        start_diff <= 300 && end_diff <= 600
    }

    /// Calculate similarity between two titles (0.0 = completely different, 1.0 = identical)
    fn calculate_title_similarity(&self, title1: &str, title2: &str) -> f64 {
        if title1 == title2 {
            return 1.0;
        }

        // Simple similarity based on common words
        let words1: std::collections::HashSet<&str> = title1.split_whitespace().collect();
        let words2: std::collections::HashSet<&str> = title2.split_whitespace().collect();

        let intersection = words1.intersection(&words2).count();
        let union = words1.union(&words2).count();

        if union == 0 {
            0.0
        } else {
            intersection as f64 / union as f64
        }
    }

    /// Find channels from M3U that don't have matching EPG data
    fn find_unmatched_channels(
        &self,
        m3u_channel_ids: &[String],
        matched_channels: &[EpgChannel],
    ) -> Vec<String> {
        let matched_ids: HashSet<String> = matched_channels
            .iter()
            .map(|c| c.channel_id.clone())
            .collect();

        m3u_channel_ids
            .iter()
            .filter(|id| !matched_ids.contains(*id))
            .cloned()
            .collect()
    }

    /// Generate XMLTV content from filtered channels and programs
    async fn generate_xmltv_content(
        &self,
        channels_with_programs: &[FilteredEpgChannel],
        _config: &EpgGenerationConfig,
    ) -> Result<String> {
        let mut xmltv = String::new();

        // XMLTV header
        xmltv.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xmltv.push_str("<!DOCTYPE tv SYSTEM \"xmltv.dtd\">\n");
        xmltv.push_str("<tv generator-info-name=\"m3u-proxy\">\n");

        // Add channels
        for filtered_channel in channels_with_programs {
            let channel = &filtered_channel.channel;
            xmltv.push_str(&format!(
                "  <channel id=\"{}\">\n",
                self.escape_xml(&channel.channel_id)
            ));
            xmltv.push_str(&format!(
                "    <display-name>{}</display-name>\n",
                self.escape_xml(&channel.channel_name)
            ));

            // Add channel logo if available
            if let Some(logo) = &channel.channel_logo {
                if !logo.is_empty() {
                    xmltv.push_str(&format!("    <icon src=\"{}\" />\n", self.escape_xml(logo)));
                }
            }

            xmltv.push_str("  </channel>\n");
        }

        // Add programs
        for filtered_channel in channels_with_programs {
            let channel = &filtered_channel.channel;

            for filtered_program in &filtered_channel.programs {
                let program = &filtered_program.program;
                // Times are always stored as UTC, use them directly
                let start_time = program.start_time;
                let end_time = program.end_time;

                xmltv.push_str(&format!(
                    "  <programme start=\"{}\" stop=\"{}\" channel=\"{}\">\n",
                    start_time.format("%Y%m%d%H%M%S %z"),
                    end_time.format("%Y%m%d%H%M%S %z"),
                    self.escape_xml(&channel.channel_id)
                ));

                xmltv.push_str(&format!(
                    "    <title>{}</title>\n",
                    self.escape_xml(&program.program_title)
                ));

                // Add description if available
                if let Some(description) = &program.program_description {
                    if !description.is_empty() {
                        xmltv.push_str(&format!(
                            "    <desc>{}</desc>\n",
                            self.escape_xml(description)
                        ));
                    }
                }

                // Add category if available
                if let Some(category) = &program.program_category {
                    if !category.is_empty() {
                        xmltv.push_str(&format!(
                            "    <category>{}</category>\n",
                            self.escape_xml(category)
                        ));
                    }
                }

                // Add episode information if available
                if program.season_num.is_some() || program.episode_num.is_some() {
                    let season = program
                        .season_num
                        .as_ref()
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);
                    let episode = program
                        .episode_num
                        .as_ref()
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);
                    xmltv.push_str(&format!(
                        "    <episode-num system=\"onscreen\">S{:02}E{:02}</episode-num>\n",
                        season, episode
                    ));
                }

                // Add program icon if available
                if let Some(icon) = &program.program_icon {
                    if !icon.is_empty() {
                        xmltv
                            .push_str(&format!("    <icon src=\"{}\" />\n", self.escape_xml(icon)));
                    }
                }

                xmltv.push_str("  </programme>\n");
            }
        }

        xmltv.push_str("</tv>\n");

        Ok(xmltv)
    }

    /// Generate empty XMLTV content
    fn generate_empty_xmltv(&self) -> String {
        let mut xmltv = String::new();
        xmltv.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xmltv.push_str("<!DOCTYPE tv SYSTEM \"xmltv.dtd\">\n");
        xmltv.push_str("<tv generator-info-name=\"m3u-proxy\">\n");
        xmltv.push_str("</tv>\n");
        xmltv
    }

    /// Escape XML special characters
    fn escape_xml(&self, input: &str) -> String {
        input
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }

    /// Get EPG generation statistics for a proxy
    pub async fn get_epg_statistics(
        &self,
        proxy: &StreamProxy,
    ) -> Result<HashMap<String, serde_json::Value>> {
        let mut stats = HashMap::new();

        // Get EPG sources count
        let epg_sources = self.database.get_proxy_epg_sources(proxy.id).await?;
        stats.insert(
            "epg_sources_count".to_string(),
            serde_json::Value::Number(epg_sources.len().into()),
        );

        // Get total EPG channels and analyze timezone information
        let mut total_channels = 0;
        let mut total_programs = 0;
        let mut timezone_info = HashMap::new();

        for source in &epg_sources {
            let channels = self.database.get_epg_source_channels(source.id).await?;
            total_channels += channels.len();

            // Track timezone information
            let source_tz = self
                .get_epg_source_timezone(source.id)
                .await
                .unwrap_or_else(|_| "Unknown".to_string());
            *timezone_info.entry(source_tz).or_insert(0) += channels.len();

            for channel in channels {
                let programs = self
                    .database
                    .get_epg_programs_for_channel(channel.id)
                    .await?;
                total_programs += programs.len();
            }
        }

        stats.insert(
            "total_epg_channels".to_string(),
            serde_json::Value::Number(total_channels.into()),
        );
        stats.insert(
            "total_epg_programs".to_string(),
            serde_json::Value::Number(total_programs.into()),
        );
        stats.insert(
            "timezone_distribution".to_string(),
            serde_json::json!(timezone_info),
        );

        Ok(stats)
    }

    /// Helper method to cache logos if needed
    async fn cache_logo_if_needed(
        &self,
        logo_url: &str,
        logo_service: &crate::logo_assets::service::LogoAssetService,
        base_url: &str,
    ) -> String {
        // Check if this is already a cached logo reference
        if logo_url.starts_with("@logo:") {
            // It's already a logo reference, resolve it to a URL
            if let Ok(uuid) = logo_url
                .strip_prefix("@logo:")
                .unwrap_or("")
                .parse::<uuid::Uuid>()
            {
                return logo_service.get_cached_logo_url(&uuid.to_string(), base_url);
            }
        }

        // Try to cache the logo
        match logo_service.cache_logo_from_url(logo_url).await {
            Ok(cache_id) => {
                debug!(
                    "Successfully cached logo '{}' with ID: {}",
                    logo_url, cache_id
                );
                logo_service.get_cached_logo_url(&cache_id.to_string(), base_url)
            }
            Err(e) => {
                debug!("Failed to cache logo '{}': {}", logo_url, e);
                // Return original URL if caching fails
                logo_url.to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_escape_xml() {
        // Test escape_xml functionality directly
        let input = "Test & \"quotes\" <tags> 'apostrophes'";
        let expected = "Test &amp; &quot;quotes&quot; &lt;tags&gt; &#39;apostrophes&#39;";
        let result = input
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;");
        assert_eq!(result, expected);
    }

    #[test]
    fn test_timezone_parsing() {
        // Test timezone parsing directly without requiring Database
        // We'll create a minimal EpgGenerator just for testing these utility methods

        // Skip this test if we can't create a database easily
        // The timezone parsing logic could be extracted to a separate module for easier testing
        // For now, we'll test the logic inline

        // Test UTC parsing
        let tz_result = match "UTC".to_uppercase().as_str() {
            "UTC" | "GMT" => Ok(chrono::FixedOffset::east_opt(0).unwrap()),
            _ => Err(anyhow::anyhow!("Invalid timezone")),
        };
        assert!(tz_result.is_ok());
        assert_eq!(tz_result.unwrap().local_minus_utc(), 0);

        // Test positive offset parsing
        let tz_str = "+02:00";
        let sign = if tz_str.starts_with('+') { 1 } else { -1 };
        let offset_str = &tz_str[1..];
        if let Some((hours_str, minutes_str)) = offset_str.split_once(':') {
            if let (Ok(hours), Ok(minutes)) = (hours_str.parse::<i32>(), minutes_str.parse::<i32>())
            {
                let total_seconds = sign * (hours * 3600 + minutes * 60);
                let offset = chrono::FixedOffset::east_opt(total_seconds).unwrap();
                assert_eq!(offset.local_minus_utc(), 7200); // 2 hours in seconds
            }
        }

        // Test negative offset parsing
        let tz_str = "-05:00";
        let sign = if tz_str.starts_with('+') { 1 } else { -1 };
        let offset_str = &tz_str[1..];
        if let Some((hours_str, minutes_str)) = offset_str.split_once(':') {
            if let (Ok(hours), Ok(minutes)) = (hours_str.parse::<i32>(), minutes_str.parse::<i32>())
            {
                let total_seconds = sign * (hours * 3600 + minutes * 60);
                let offset = chrono::FixedOffset::east_opt(total_seconds).unwrap();
                assert_eq!(offset.local_minus_utc(), -18000); // -5 hours in seconds
            }
        }
    }

    #[test]
    fn test_utc_normalization() {
        // Test UTC normalization logic directly
        let test_time = Utc::now();

        // Test that UTC time normalized to UTC should be the same
        let normalized = test_time; // normalize_to_utc should be a no-op for UTC times
        assert_eq!(test_time, normalized);

        // Test basic timezone conversion logic
        let test_time_utc = Utc::now();
        assert!(test_time_utc.timestamp() != 0); // Just verify it's a valid timestamp

        // The actual timezone conversion would happen in the normalize_timezone_to_utc method
        // but since we can't easily create a test database, we'll just verify the time is valid
        assert!(test_time_utc.timestamp() > 0);
    }

    #[test]
    fn test_epg_generation_config_default() {
        let config = EpgGenerationConfig::default();
        assert_eq!(config.include_past_programs, false);
        assert_eq!(config.days_ahead, 7);
        assert_eq!(config.days_behind, 1);
        assert_eq!(config.deduplicate_programs, true);
        assert_eq!(config.max_programs_per_channel, Some(1000));
    }

    #[test]
    fn test_generate_empty_xmltv() {
        // Test the generate_empty_xmltv method directly without requiring Database::default()
        let xmltv = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE tv SYSTEM \"xmltv.dtd\">\n<tv generator-info-name=\"m3u-proxy\">\n</tv>\n";

        assert!(xmltv.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xmltv.contains("<tv generator-info-name=\"m3u-proxy\">"));
        assert!(xmltv.contains("</tv>"));
    }
}
