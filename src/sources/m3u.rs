//! M3U source handler implementation
//!
//! This module provides the concrete implementation for handling M3U playlist sources.
//! It supports standard M3U and M3U8 playlists with EXTINF metadata and custom field mapping.

use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{StreamSource, StreamSourceType, Channel};
use super::traits::*;

/// M3U source handler
///
/// This handler implements the full source handler interface for M3U playlist sources.
/// It supports both standard M3U files and extended M3U8 playlists with metadata.
///
/// # Features
/// - HTTP/HTTPS playlist fetching
/// - EXTINF metadata parsing
/// - Custom field mapping support
/// - Progress reporting during ingestion
/// - Health checking with response time metrics
/// - URL validation and connectivity testing
pub struct M3uSourceHandler {
    client: Client,
}

impl M3uSourceHandler {
    /// Create a new M3U source handler
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("M3U-Proxy/1.0")
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client }
    }

    /// Parse M3U content into channels
    async fn parse_m3u_content(&self, content: &str, source: &StreamSource) -> AppResult<Vec<Channel>> {
        let mut channels = Vec::new();
        let mut current_channel: Option<PartialChannel> = None;

        debug!("Starting M3U parsing for source: {}", source.name);

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();
            
            if line.is_empty() || line.starts_with('#') && !line.starts_with("#EXTINF") {
                continue;
            }

            if line.starts_with("#EXTINF") {
                // Parse EXTINF line: #EXTINF:duration,title
                current_channel = self.parse_extinf_line(line, source)?;
            } else if !line.starts_with('#') {
                // This should be a stream URL
                if let Some(mut channel) = current_channel.take() {
                    channel.url = line.to_string();
                    
                    let complete_channel = self.complete_channel(channel, source, line_num + 1)?;
                    channels.push(complete_channel);
                } else {
                    warn!("Found stream URL without EXTINF metadata at line {}: {}", line_num + 1, line);
                    // Create a basic channel without metadata
                    let channel = self.create_basic_channel(line, source, line_num + 1)?;
                    channels.push(channel);
                }
            }
        }

        info!("Parsed {} channels from M3U source: {}", channels.len(), source.name);
        Ok(channels)
    }

    /// Parse an EXTINF line to extract channel metadata
    fn parse_extinf_line(&self, line: &str, source: &StreamSource) -> AppResult<Option<PartialChannel>> {
        // Format: #EXTINF:duration,title
        // Extended: #EXTINF:duration tvg-id="id" tvg-logo="logo" group-title="group",title
        
        let extinf_content = line.strip_prefix("#EXTINF:").unwrap_or(line);
        
        // Find the comma that separates duration from title/metadata
        let comma_pos = extinf_content.rfind(',')
            .ok_or_else(|| AppError::validation("Invalid EXTINF format: missing comma"))?;
        
        let (duration_and_attrs, title) = extinf_content.split_at(comma_pos);
        let title = title.trim_start_matches(',').trim();
        
        // Parse duration (first part before any attributes)
        let duration_str = duration_and_attrs.split_whitespace().next().unwrap_or("0");
        let _duration: f64 = duration_str.parse().unwrap_or(0.0);
        
        // Parse attributes (tvg-id, tvg-logo, group-title, etc.)
        let attributes = self.parse_extinf_attributes(duration_and_attrs);
        
        let mut channel = PartialChannel {
            name: title.to_string(),
            url: String::new(), // Will be set when we find the URL line
            group_title: attributes.get("group-title").cloned(),
            tvg_id: attributes.get("tvg-id").cloned(),
            tvg_logo: attributes.get("tvg-logo").cloned(),
            tvg_name: attributes.get("tvg-name").cloned(),
            attributes,
        };

        // Apply custom field mapping if configured
        if let Some(field_map_json) = &source.field_map {
            if let Ok(field_map) = serde_json::from_str::<HashMap<String, String>>(field_map_json) {
                channel = self.apply_field_mapping(channel, &field_map);
            }
        }

        Ok(Some(channel))
    }

    /// Parse attributes from EXTINF line (tvg-id="value" format)
    fn parse_extinf_attributes(&self, attrs_part: &str) -> HashMap<String, String> {
        let mut attributes = HashMap::new();
        
        // Simple regex-free parsing for key="value" pairs
        let mut chars = attrs_part.chars().peekable();
        let mut current_key = String::new();
        let mut current_value = String::new();
        let mut in_quotes = false;
        let mut in_key = false;
        let mut in_value = false;
        
        while let Some(ch) = chars.next() {
            match ch {
                ' ' | '\t' if !in_quotes => {
                    if in_value {
                        // End of unquoted value
                        if !current_key.is_empty() && !current_value.is_empty() {
                            attributes.insert(current_key.clone(), current_value.clone());
                        }
                        current_key.clear();
                        current_value.clear();
                        in_key = false;
                        in_value = false;
                    }
                    in_key = true;
                }
                '=' if !in_quotes => {
                    in_key = false;
                    in_value = true;
                    // Check if next char is quote
                    if chars.peek() == Some(&'"') {
                        chars.next(); // consume the quote
                        in_quotes = true;
                    }
                }
                '"' if in_value => {
                    in_quotes = false;
                    // End of quoted value
                    if !current_key.is_empty() {
                        attributes.insert(current_key.clone(), current_value.clone());
                    }
                    current_key.clear();
                    current_value.clear();
                    in_value = false;
                }
                _ => {
                    if in_key {
                        current_key.push(ch);
                    } else if in_value {
                        current_value.push(ch);
                    }
                }
            }
        }
        
        // Handle final unquoted value
        if in_value && !current_key.is_empty() && !current_value.is_empty() {
            attributes.insert(current_key, current_value);
        }
        
        attributes
    }

    /// Apply custom field mapping to channel
    fn apply_field_mapping(&self, mut channel: PartialChannel, field_map: &HashMap<String, String>) -> PartialChannel {
        for (source_field, target_field) in field_map {
            if let Some(value) = channel.attributes.get(source_field) {
                match target_field.as_str() {
                    "name" => channel.name = value.clone(),
                    "group_title" => channel.group_title = Some(value.clone()),
                    "tvg_id" => channel.tvg_id = Some(value.clone()),
                    "tvg_logo" => channel.tvg_logo = Some(value.clone()),
                    "tvg_name" => channel.tvg_name = Some(value.clone()),
                    _ => {
                        // Custom field mapping
                        channel.attributes.insert(target_field.clone(), value.clone());
                    }
                }
            }
        }
        channel
    }

    /// Complete a partial channel by filling in required fields
    fn complete_channel(&self, partial: PartialChannel, source: &StreamSource, line_number: usize) -> AppResult<Channel> {
        let now = Utc::now();
        
        Ok(Channel {
            id: Uuid::new_v4(),
            source_id: source.id,
            tvg_id: partial.tvg_id,
            tvg_name: partial.tvg_name,
            tvg_logo: partial.tvg_logo,
            tvg_shift: None,
            group_title: partial.group_title,
            channel_name: partial.name,
            stream_url: partial.url,
            created_at: now,
            updated_at: now,
        })
    }

    /// Create a basic channel without EXTINF metadata
    fn create_basic_channel(&self, url: &str, source: &StreamSource, line_number: usize) -> AppResult<Channel> {
        let now = Utc::now();
        
        // Try to extract a name from the URL
        let name = url
            .split('/')
            .last()
            .unwrap_or("Unnamed Channel")
            .split('?')
            .next()
            .unwrap_or("Unnamed Channel")
            .to_string();
        
        Ok(Channel {
            id: Uuid::new_v4(),
            source_id: source.id,
            tvg_id: None,
            tvg_name: None,
            tvg_logo: None,
            tvg_shift: None,
            group_title: None,
            channel_name: name,
            stream_url: url.to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Validate M3U URL format and accessibility
    async fn validate_m3u_url(&self, url: &str) -> AppResult<SourceValidationResult> {
        let mut result = SourceValidationResult::success();

        // Basic URL format validation
        if !url.starts_with("http://") && !url.starts_with("https://") {
            result.errors.push("M3U URL must use HTTP or HTTPS protocol".to_string());
            result.is_valid = false;
        }

        // Check for typical M3U file extensions
        if !url.ends_with(".m3u") && !url.ends_with(".m3u8") && !url.contains("playlist") {
            result = result.with_warning("URL doesn't have typical M3U extension (.m3u, .m3u8) or 'playlist' in the path");
        }

        // Test connectivity
        match self.client.head(url).send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    result.errors.push(format!("HTTP error: {}", response.status()));
                    result.is_valid = false;
                } else {
                    result = result.with_context("http_status", response.status().to_string());
                    
                    // Check content type if available
                    if let Some(content_type) = response.headers().get("content-type") {
                        let content_type_str = content_type.to_str().unwrap_or("");
                        result = result.with_context("content_type", content_type_str.to_string());
                        
                        if !content_type_str.contains("text") && !content_type_str.contains("application") {
                            result = result.with_warning("Content-Type doesn't appear to be text-based");
                        }
                    }
                }
            }
            Err(e) => {
                result.errors.push(format!("Connection failed: {}", e));
                result.is_valid = false;
            }
        }

        Ok(result)
    }

    /// Estimate channel count by doing a partial download and analysis
    async fn estimate_channels(&self, source: &StreamSource) -> AppResult<Option<u32>> {
        // Download first 64KB to estimate channel count
        let response = self.client.get(&source.url)
            .header("Range", "bytes=0-65535")
            .send()
            .await
            .map_err(|e| AppError::from(e))?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let partial_content = response.text().await
            .map_err(|e| AppError::from(e))?;

        // Count EXTINF lines as rough estimate
        let extinf_count = partial_content.lines()
            .filter(|line| line.starts_with("#EXTINF"))
            .count();

        // If we have a meaningful sample, extrapolate
        if extinf_count > 10 {
            // Rough extrapolation based on content size
            let sample_size = partial_content.len() as f64;
            let extinf_density = extinf_count as f64 / sample_size;
            
            // Estimate total file size (this is very rough)
            let estimated_total = sample_size * 4.0; // Assume sample is 1/4 of total
            let estimated_channels = (estimated_total * extinf_density) as u32;
            
            Ok(Some(estimated_channels.max(extinf_count as u32)))
        } else {
            Ok(Some(extinf_count as u32))
        }
    }
}

/// Partial channel structure used during parsing
struct PartialChannel {
    name: String,
    url: String,
    group_title: Option<String>,
    tvg_id: Option<String>,
    tvg_logo: Option<String>,
    tvg_name: Option<String>,
    attributes: HashMap<String, String>,
}

#[async_trait]
impl SourceHandler for M3uSourceHandler {
    fn source_type(&self) -> StreamSourceType {
        StreamSourceType::M3u
    }

    async fn validate_source(&self, source: &StreamSource) -> AppResult<SourceValidationResult> {
        debug!("Validating M3U source: {}", source.name);
        
        let url_validation = self.validate_m3u_url(&source.url).await?;
        let mut result = url_validation;

        // Additional M3U-specific validations
        if source.username.is_some() || source.password.is_some() {
            result = result.with_warning("M3U sources typically don't require authentication credentials");
        }

        // Validate field mapping if present
        if let Some(field_map_json) = &source.field_map {
            match serde_json::from_str::<HashMap<String, String>>(field_map_json) {
                Ok(field_map) => {
                    result = result.with_context("field_map_entries", field_map.len().to_string());
                    
                    // Validate known field mappings
                    for (source_field, target_field) in &field_map {
                        if target_field.is_empty() {
                            result = result.with_warning(format!("Empty target field mapping for '{}'", source_field));
                        }
                    }
                }
                Err(e) => {
                    result.errors.push(format!("Invalid field mapping JSON: {}", e));
                    result.is_valid = false;
                }
            }
        }

        Ok(result)
    }

    async fn get_capabilities(&self, _source: &StreamSource) -> AppResult<SourceCapabilities> {
        Ok(SourceCapabilities::m3u_basic())
    }

    async fn test_connectivity(&self, source: &StreamSource) -> AppResult<bool> {
        match self.client.head(&source.url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    async fn get_source_info(&self, source: &StreamSource) -> AppResult<HashMap<String, String>> {
        let mut info = HashMap::new();
        
        match self.client.head(&source.url).send().await {
            Ok(response) => {
                info.insert("status".to_string(), response.status().to_string());
                
                if let Some(content_length) = response.headers().get("content-length") {
                    info.insert("content_length".to_string(), content_length.to_str().unwrap_or("unknown").to_string());
                }
                
                if let Some(last_modified) = response.headers().get("last-modified") {
                    info.insert("last_modified".to_string(), last_modified.to_str().unwrap_or("unknown").to_string());
                }
                
                if let Some(server) = response.headers().get("server") {
                    info.insert("server".to_string(), server.to_str().unwrap_or("unknown").to_string());
                }
            }
            Err(e) => {
                info.insert("error".to_string(), e.to_string());
            }
        }
        
        info.insert("source_type".to_string(), "M3U".to_string());
        Ok(info)
    }
}

#[async_trait]
impl ChannelIngestor for M3uSourceHandler {
    async fn ingest_channels(&self, source: &StreamSource) -> AppResult<Vec<Channel>> {
        self.ingest_channels_with_progress(source, None).await
    }

    async fn ingest_channels_with_progress(
        &self,
        source: &StreamSource,
        progress_callback: Option<&ProgressCallback>,
    ) -> AppResult<Vec<Channel>> {
        info!("Starting M3U channel ingestion for source: {}", source.name);

        // Report initial progress
        if let Some(callback) = progress_callback {
            callback(IngestionProgress::starting("Connecting to M3U source"));
        }

        // Download the M3U content
        let response = self.client.get(&source.url).send().await
            .map_err(|e| AppError::source_error(format!("Failed to fetch M3U: {}", e)))?;

        if !response.status().is_success() {
            return Err(AppError::source_error(format!("HTTP error: {}", response.status())));
        }

        if let Some(callback) = progress_callback {
            callback(IngestionProgress::starting("Downloading playlist").update_step("Downloading playlist", Some(25.0)));
        }

        let content = response.text().await
            .map_err(|e| AppError::source_error(format!("Failed to read M3U content: {}", e)))?;

        if let Some(callback) = progress_callback {
            callback(IngestionProgress::starting("Parsing channels").update_step("Parsing channels", Some(50.0)));
        }

        // Parse the content
        let channels = self.parse_m3u_content(&content, source).await?;

        if let Some(callback) = progress_callback {
            callback(IngestionProgress::starting("Ingestion complete").update_step("Ingestion complete", Some(100.0)));
        }

        info!("Successfully ingested {} channels from M3U source: {}", channels.len(), source.name);
        Ok(channels)
    }

    async fn estimate_channel_count(&self, source: &StreamSource) -> AppResult<Option<u32>> {
        self.estimate_channels(source).await
    }
}

#[async_trait]
impl HealthChecker for M3uSourceHandler {
    async fn check_health(&self, source: &StreamSource) -> AppResult<SourceHealthStatus> {
        let start_time = std::time::Instant::now();
        let checked_at = Utc::now();

        match self.client.head(&source.url).send().await {
            Ok(response) => {
                let response_time_ms = start_time.elapsed().as_millis() as u64;
                let is_healthy = response.status().is_success();

                Ok(SourceHealthStatus {
                    is_healthy,
                    response_time_ms: Some(response_time_ms),
                    last_success: if is_healthy { Some(checked_at) } else { None },
                    error_message: if !is_healthy { 
                        Some(format!("HTTP error: {}", response.status())) 
                    } else { 
                        None 
                    },
                    checked_at,
                })
            }
            Err(e) => {
                Ok(SourceHealthStatus {
                    is_healthy: false,
                    response_time_ms: Some(start_time.elapsed().as_millis() as u64),
                    last_success: None,
                    error_message: Some(e.to_string()),
                    checked_at,
                })
            }
        }
    }

    async fn get_health_metrics(&self, source: &StreamSource) -> AppResult<SourceHealthMetrics> {
        let status = self.check_health(source).await?;
        let mut metrics = HashMap::new();

        // Try to get additional metrics
        if let Ok(estimated_channels) = self.estimate_channel_count(source).await {
            if let Some(count) = estimated_channels {
                metrics.insert("estimated_channels".to_string(), count.to_string());
            }
        }

        if let Ok(source_info) = self.get_source_info(source).await {
            for (key, value) in source_info {
                metrics.insert(key, value);
            }
        }

        Ok(SourceHealthMetrics {
            status,
            channel_count: None, // Would require full ingestion to get accurate count
            server_version: metrics.get("server").cloned(),
            uptime: None,
            metrics,
        })
    }
}

#[async_trait]
impl StreamUrlGenerator for M3uSourceHandler {
    async fn generate_stream_url(
        &self,
        _source: &StreamSource,
        channel_id: &str,
    ) -> AppResult<String> {
        // For M3U sources, the channel_id should be the direct stream URL
        // This is a pass-through since M3U channels already contain direct URLs
        Ok(channel_id.to_string())
    }

    async fn generate_stream_urls(
        &self,
        source: &StreamSource,
        channel_ids: &[String],
    ) -> AppResult<HashMap<String, String>> {
        let mut urls = HashMap::new();
        
        for channel_id in channel_ids {
            let url = self.generate_stream_url(source, channel_id).await?;
            urls.insert(channel_id.clone(), url);
        }
        
        Ok(urls)
    }

    async fn validate_stream_url(
        &self,
        _source: &StreamSource,
        url: &str,
    ) -> AppResult<bool> {
        match self.client.head(url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}

impl FullSourceHandler for M3uSourceHandler {}

impl Default for M3uSourceHandler {
    fn default() -> Self {
        Self::new()
    }
}