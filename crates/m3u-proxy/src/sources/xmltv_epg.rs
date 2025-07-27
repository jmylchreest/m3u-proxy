//! XMLTV EPG Source Handler
//!
//! This module provides the implementation for XMLTV/M3U EPG sources.
//! It handles the ingestion of EPG data from XMLTV format files and URLs.
//!
//! # Features
//!
//! - XMLTV format parsing using the xmltv crate
//! - Robust HTTP fetching with timeout and error handling
//! - Progress reporting during ingestion
//! - Timezone detection and normalization
//! - Channel and program validation

use async_trait::async_trait;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tracing::{debug, info};

use crate::errors::{AppError, AppResult};
use crate::models::{EpgSource, EpgSourceType, EpgProgram};
use crate::sources::traits::{
    EpgSourceHandler, EpgProgramIngestor, FullEpgSourceHandler, SourceValidationResult,
    EpgSourceCapabilities, EpgIngestionProgress, EpgProgressCallback, EpgSourceHandlerSummary,
};
use crate::utils::time::{detect_timezone_from_xmltv, log_timezone_detection};
use crate::utils::url::UrlUtils;
use crate::utils::{CompressionFormat, DecompressionService};

/// XMLTV EPG source handler implementation
pub struct XmltvEpgHandler {
    client: Client,
}

impl XmltvEpgHandler {
    /// Create a new XMLTV EPG handler
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Fetch XMLTV content from URL with automatic decompression support
    async fn fetch_xmltv_content(&self, url: &str) -> AppResult<String> {
        debug!("Fetching XMLTV content from: {}", url);
        
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| AppError::source_error(format!("Failed to fetch XMLTV: {}", e)))?;

        if !response.status().is_success() {
            return Err(AppError::source_error(format!(
                "HTTP error fetching XMLTV: {} {}",
                response.status(),
                response.status().canonical_reason().unwrap_or("Unknown")
            )));
        }

        // Get raw bytes instead of text to detect compression
        let bytes = response
            .bytes()
            .await
            .map_err(|e| AppError::source_error(format!("Failed to read XMLTV response: {}", e)))?;

        debug!("Fetched {} bytes of raw XMLTV content", bytes.len());

        // Detect compression format and decompress if needed
        let compression_format = DecompressionService::detect_compression_format(&bytes);
        debug!("Detected compression format: {:?}", compression_format);

        let decompressed_bytes = match compression_format {
            CompressionFormat::Uncompressed => {
                debug!("Content is uncompressed, using as-is");
                bytes.to_vec()
            }
            _ => {
                debug!("Content is compressed, decompressing...");
                DecompressionService::decompress(bytes)
                    .map_err(|e| AppError::source_error(format!("Failed to decompress XMLTV content: {}", e)))?
            }
        };

        // Convert decompressed bytes to UTF-8 string
        let content = String::from_utf8(decompressed_bytes)
            .map_err(|e| AppError::source_error(format!("Failed to decode XMLTV content as UTF-8: {}", e)))?;

        debug!("Successfully processed {} bytes of XMLTV content (compression: {:?})", 
               content.len(), compression_format);
        Ok(content)
    }

    /// Parse XMLTV content and extract programs only (programs-only mode)
    async fn parse_xmltv_content(
        &self,
        source: &EpgSource,
        content: &str,
        progress_callback: Option<&EpgProgressCallback>,
    ) -> AppResult<Vec<EpgProgram>> {
        if let Some(callback) = progress_callback {
            callback(EpgIngestionProgress::starting("Parsing XMLTV content"));
        }

        // Parse using our custom quick-xml parser
        let xmltv_programs = crate::utils::xmltv_parser::parse_xmltv_programs(content)?;

        // Detect timezone
        let detected_tz = detect_timezone_from_xmltv(content);
        if let Some(tz) = &detected_tz {
            log_timezone_detection(&source.name, Some(tz), tz);
        } else {
            log_timezone_detection(&source.name, None, "UTC");
        }

        // Skip channel processing - programs-only approach for database-first generation

        if let Some(callback) = progress_callback {
            callback(EpgIngestionProgress::starting("Converting programs")
                .update_step("Converting programs", Some(50.0)));
        }

        // Convert from SimpleXmltvProgram to EpgProgram
        let mut epg_programs = Vec::new();
        let mut seen_programs = HashSet::new();
        let mut duplicate_program_count = 0;
        
        for xmltv_program in xmltv_programs {
            // Create deduplication key: channel_id + start_time + program_title
            let program_title = xmltv_program.title.as_ref()
                .map(|t| t.as_str())
                .unwrap_or("Unknown Program");
            let dedup_key = format!("{}|{}|{}", 
                xmltv_program.channel,
                xmltv_program.start,
                program_title
            );
            
            // Skip duplicate programs
            if seen_programs.contains(&dedup_key) {
                duplicate_program_count += 1;
                debug!("Skipping duplicate program '{}' on channel '{}' at {}", 
                       program_title, xmltv_program.channel, xmltv_program.start);
                continue;
            }
            seen_programs.insert(dedup_key);
            // Parse start and stop times
            let start_time = chrono::DateTime::parse_from_str(&xmltv_program.start, "%Y%m%d%H%M%S %z")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(&xmltv_program.start, "%Y%m%d%H%M%S")
                    .map(|dt| dt.and_utc().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap())))
                .map_err(|e| AppError::source_error(format!("Failed to parse start time '{}': {}", xmltv_program.start, e)))?;

            let end_time = if let Some(ref stop) = xmltv_program.stop {
                chrono::DateTime::parse_from_str(stop, "%Y%m%d%H%M%S %z")
                    .or_else(|_| chrono::NaiveDateTime::parse_from_str(stop, "%Y%m%d%H%M%S")
                        .map(|dt| dt.and_utc().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap())))
                    .map_err(|e| AppError::source_error(format!("Failed to parse stop time '{}': {}", stop, e)))?
            } else {
                // If no stop time, estimate 30 minutes duration
                start_time + chrono::Duration::minutes(30)
            };

            // Channel name will be resolved during generation stage from M3U channels
            let channel_name = String::new();

            let epg_program = EpgProgram {
                id: uuid::Uuid::new_v4(),
                source_id: source.id,
                channel_id: xmltv_program.channel,
                channel_name,
                program_title: xmltv_program.title.unwrap_or_else(|| "Unknown Program".to_string()),
                program_description: xmltv_program.description,
                program_category: xmltv_program.category,
                start_time: start_time.with_timezone(&chrono::Utc),
                end_time: end_time.with_timezone(&chrono::Utc),
                episode_num: None,
                season_num: None,
                rating: None,
                language: xmltv_program.language,
                subtitles: None,
                aspect_ratio: None,
                program_icon: xmltv_program.icon,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            epg_programs.push(epg_program);
        }
        
        // Clean up deduplication set to free memory
        drop(seen_programs);
        
        if duplicate_program_count > 0 {
            info!(
                "Removed {} duplicate program entries from XMLTV feed for source '{}'", 
                duplicate_program_count, source.name
            );
        }

        if let Some(callback) = progress_callback {
            callback(EpgIngestionProgress::starting("Parsing complete")
                .update_step("Parsing complete", Some(100.0)));
        }

        info!(
            "Parsed XMLTV EPG for source '{}': {} programs",
            source.name,
            epg_programs.len()
        );

        Ok(epg_programs)
    }
}

impl Default for XmltvEpgHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EpgSourceHandler for XmltvEpgHandler {
    fn epg_source_type(&self) -> EpgSourceType {
        EpgSourceType::Xmltv
    }

    async fn validate_epg_source(&self, source: &EpgSource) -> AppResult<SourceValidationResult> {
        let mut validation = SourceValidationResult::success();
        
        // Validate EPG source type
        if source.source_type != EpgSourceType::Xmltv {
            return Ok(SourceValidationResult::failure(vec![
                format!("Expected XMLTV source type, got {:?}", source.source_type),
            ]));
        }

        // Validate URL
        if source.url.is_empty() {
            validation.errors.push("URL is required for XMLTV sources".to_string());
            validation.is_valid = false;
        } else {
            // Check URL format
            match UrlUtils::parse_and_validate(&source.url) {
                Ok(_) => {
                    validation = validation.with_context("url_format", "valid");
                }
                Err(e) => {
                    validation.errors.push(format!("Invalid URL format: {}", e));
                    validation.is_valid = false;
                }
            }
        }

        // XMLTV sources don't require authentication
        if source.username.is_some() || source.password.is_some() {
            validation = validation.with_warning("XMLTV sources typically don't require authentication");
        }

        Ok(validation)
    }

    async fn get_epg_capabilities(&self, _source: &EpgSource) -> AppResult<EpgSourceCapabilities> {
        Ok(EpgSourceCapabilities::xmltv_basic())
    }

    async fn test_epg_connectivity(&self, source: &EpgSource) -> AppResult<bool> {
        match self.client.head(&source.url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    async fn get_epg_source_info(&self, source: &EpgSource) -> AppResult<HashMap<String, String>> {
        let mut info = HashMap::new();
        info.insert("source_type".to_string(), "xmltv".to_string());
        info.insert("url".to_string(), source.url.clone());
        
        // Try to get some basic info from the source
        if let Ok(response) = self.client.head(&source.url).send().await {
            if let Some(content_type) = response.headers().get("content-type") {
                if let Ok(ct_str) = content_type.to_str() {
                    info.insert("content_type".to_string(), ct_str.to_string());
                }
            }
            if let Some(content_length) = response.headers().get("content-length") {
                if let Ok(cl_str) = content_length.to_str() {
                    info.insert("content_length".to_string(), cl_str.to_string());
                }
            }
        }

        Ok(info)
    }
}

#[async_trait]
impl EpgProgramIngestor for XmltvEpgHandler {
    async fn ingest_epg_programs(&self, source: &EpgSource) -> AppResult<Vec<EpgProgram>> {
        self.ingest_epg_programs_with_progress(source, None).await
    }

    async fn ingest_epg_programs_with_progress(
        &self,
        source: &EpgSource,
        progress_callback: Option<&EpgProgressCallback>,
    ) -> AppResult<Vec<EpgProgram>> {
        if let Some(callback) = progress_callback {
            callback(EpgIngestionProgress::starting("Fetching XMLTV content"));
        }

        // Fetch content
        let content = self.fetch_xmltv_content(&source.url).await?;

        if let Some(callback) = progress_callback {
            callback(EpgIngestionProgress::starting("Fetching complete")
                .update_step("Parsing XMLTV", Some(25.0)));
        }

        // Parse content
        self.parse_xmltv_content(source, &content, progress_callback).await
    }

    async fn estimate_program_count(&self, source: &EpgSource) -> AppResult<Option<u32>> {
        // For XMLTV, we can't easily estimate without downloading and parsing
        // In a future optimization, we could do a partial parse or use content-length
        debug!("Program count estimation not available for XMLTV source: {}", source.name);
        Ok(None)
    }

    async fn ingest_epg_programs_with_universal_progress(
        &self,
        source: &EpgSource,
        progress_callback: Option<&crate::sources::traits::UniversalCallback>,
    ) -> AppResult<Vec<EpgProgram>> {
        use crate::services::progress_service::{UniversalProgress, OperationType, UniversalState};
        use uuid::Uuid;

        info!("Starting XMLTV EPG ingestion with universal progress for source: {}", source.name);
        let source_id = Uuid::new_v4(); // Generate operation ID

        if let Some(callback) = progress_callback {
            let progress = UniversalProgress::new(
                source_id,
                OperationType::EpgIngestion,
                format!("XMLTV EPG Ingestion: {}", source.name)
            )
            .set_state(UniversalState::Connecting)
            .update_step("Connecting to XMLTV source".to_string());
            callback(progress);
        }

        if let Some(callback) = progress_callback {
            let progress = UniversalProgress::new(
                source_id,
                OperationType::EpgIngestion,
                format!("XMLTV EPG Ingestion: {}", source.name)
            )
            .set_state(UniversalState::Downloading)
            .update_step("Fetching XMLTV content".to_string())
            .update_percentage(10.0);
            callback(progress);
        }

        // Fetch content
        let content = match self.fetch_xmltv_content(&source.url).await {
            Ok(content) => content,
            Err(e) => {
                if let Some(callback) = progress_callback {
                    let progress = UniversalProgress::new(
                        source_id,
                        OperationType::EpgIngestion,
                        format!("XMLTV EPG Ingestion: {}", source.name)
                    )
                    .set_error(format!("Failed to fetch XMLTV content: {}", e));
                    callback(progress);
                }
                return Err(e);
            }
        };

        if let Some(callback) = progress_callback {
            let progress = UniversalProgress::new(
                source_id,
                OperationType::EpgIngestion,
                format!("XMLTV EPG Ingestion: {}", source.name)
            )
            .set_state(UniversalState::Processing)
            .update_step("Parsing XMLTV content".to_string())
            .update_percentage(25.0);
            callback(progress);
        }

        // Parse using our custom quick-xml parser
        let xmltv_programs = match crate::utils::xmltv_parser::parse_xmltv_programs(&content) {
            Ok(programs) => programs,
            Err(e) => {
                if let Some(callback) = progress_callback {
                    let progress = UniversalProgress::new(
                        source_id,
                        OperationType::EpgIngestion,
                        format!("XMLTV EPG Ingestion: {}", source.name)
                    )
                    .set_error(format!("Failed to parse XMLTV: {}", e));
                    callback(progress);
                }
                return Err(e);
            }
        };

        // Detect timezone
        let detected_tz = detect_timezone_from_xmltv(&content);
        if let Some(tz) = &detected_tz {
            log_timezone_detection(&source.name, Some(tz), tz);
        } else {
            log_timezone_detection(&source.name, None, "UTC");
        }

        // Skip channel processing - programs-only approach for database-first generation

        if let Some(callback) = progress_callback {
            let progress = UniversalProgress::new(
                source_id,
                OperationType::EpgIngestion,
                format!("XMLTV EPG Ingestion: {}", source.name)
            )
            .set_state(UniversalState::Processing)
            .update_step("Converting programs".to_string())
            .update_percentage(75.0);
            callback(progress);
        }

        // Convert from SimpleXmltvProgram to EpgProgram
        let mut epg_programs = Vec::new();
        let mut seen_programs = HashSet::new();
        let mut duplicate_program_count = 0;
        
        for xmltv_program in xmltv_programs {
            // Create deduplication key: channel_id + start_time + program_title
            let program_title = xmltv_program.title.as_ref()
                .map(|t| t.as_str())
                .unwrap_or("Unknown Program");
            let dedup_key = format!("{}|{}|{}", 
                xmltv_program.channel,
                xmltv_program.start,
                program_title
            );
            
            // Skip duplicate programs
            if seen_programs.contains(&dedup_key) {
                duplicate_program_count += 1;
                debug!("Skipping duplicate program '{}' on channel '{}' at {}", 
                       program_title, xmltv_program.channel, xmltv_program.start);
                continue;
            }
            seen_programs.insert(dedup_key);
            // Parse start and stop times
            let start_time = chrono::DateTime::parse_from_str(&xmltv_program.start, "%Y%m%d%H%M%S %z")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(&xmltv_program.start, "%Y%m%d%H%M%S")
                    .map(|dt| dt.and_utc().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap())))
                .map_err(|e| AppError::source_error(format!("Failed to parse start time '{}': {}", xmltv_program.start, e)))?;

            let end_time = if let Some(stop) = xmltv_program.stop {
                chrono::DateTime::parse_from_str(&stop, "%Y%m%d%H%M%S %z")
                    .or_else(|_| chrono::NaiveDateTime::parse_from_str(&stop, "%Y%m%d%H%M%S")
                        .map(|dt| dt.and_utc().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap())))
                    .map_err(|e| AppError::source_error(format!("Failed to parse stop time '{}': {}", stop, e)))?
            } else {
                start_time + chrono::Duration::minutes(30)
            };

            // Channel name will be resolved during generation stage from M3U channels
            let channel_name = String::new();

            let epg_program = EpgProgram {
                id: uuid::Uuid::new_v4(),
                source_id: source.id,
                channel_id: xmltv_program.channel,
                channel_name,
                program_title: xmltv_program.title.unwrap_or_else(|| "Unknown Program".to_string()),
                program_description: xmltv_program.description,
                program_category: xmltv_program.category,
                start_time: start_time.with_timezone(&chrono::Utc),
                end_time: end_time.with_timezone(&chrono::Utc),
                episode_num: None,
                season_num: None,
                rating: None,
                language: xmltv_program.language,
                subtitles: None,
                aspect_ratio: None,
                program_icon: xmltv_program.icon,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            epg_programs.push(epg_program);
        }
        
        // Clean up deduplication set to free memory
        drop(seen_programs);
        
        if duplicate_program_count > 0 {
            info!(
                "Removed {} duplicate program entries from XMLTV feed for source '{}'", 
                duplicate_program_count, source.name
            );
        }

        if let Some(callback) = progress_callback {
            let progress = UniversalProgress::new(
                source_id,
                OperationType::EpgIngestion,
                format!("XMLTV EPG Ingestion: {}", source.name)
            )
            .set_state(UniversalState::Completed)
            .update_step("EPG ingestion complete".to_string())
            .update_percentage(100.0)
            .update_items(epg_programs.len(), Some(epg_programs.len()));
            callback(progress);
        }

        info!(
            "Parsed XMLTV EPG for source '{}': {} programs",
            source.name,
            epg_programs.len()
        );

        Ok(epg_programs)
    }
}

impl FullEpgSourceHandler for XmltvEpgHandler {
    fn get_epg_handler_summary(&self) -> EpgSourceHandlerSummary {
        EpgSourceHandlerSummary {
            epg_source_type: EpgSourceType::Xmltv,
            supports_program_ingestion: true,
            supports_authentication: false,
        }
    }
}