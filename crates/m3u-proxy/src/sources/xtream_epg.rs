//! Xtream Codes EPG Source Handler
//!
//! This module provides the implementation for Xtream Codes EPG sources.
//! It handles the ingestion of EPG data from Xtream API endpoints.
//!
//! # Features
//!
//! - Xtream Codes authentication and API interaction
//! - XMLTV format parsing from Xtream EPG endpoints
//! - Progress reporting during ingestion
//! - Connection validation and error handling

use async_trait::async_trait;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::errors::{AppError, AppResult};
use crate::models::{EpgSource, EpgSourceType, EpgProgram};
use crate::sources::traits::{
    EpgSourceHandler, EpgProgramIngestor, FullEpgSourceHandler, SourceValidationResult,
    EpgSourceCapabilities, EpgIngestionProgress, EpgProgressCallback, EpgSourceHandlerSummary,
};
use crate::utils::url::UrlUtils;
use crate::utils::human_format::format_memory;

/// Xtream Codes EPG source handler implementation
pub struct XtreamEpgHandler {
    client: Client,
}

impl XtreamEpgHandler {
    /// Create a new Xtream EPG handler
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(60)) // Longer timeout for Xtream
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Build the Xtream EPG URL with authentication
    fn build_epg_url(&self, source: &EpgSource) -> AppResult<String> {
        source.build_epg_url()
            .map_err(|e| AppError::source_error(format!("Failed to build EPG URL: {}", e)))
    }

    /// Fetch EPG content from Xtream API with HTTPS/HTTP fallback
    async fn fetch_xtream_epg_content(&self, source: &EpgSource) -> AppResult<String> {
        let epg_url = self.build_epg_url(source)?;
        debug!("Fetching Xtream EPG content from: {}", crate::utils::url::UrlUtils::obfuscate_credentials(&epg_url));

        // Try the original URL first
        match self.client.get(&epg_url).send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    return Err(AppError::source_error(format!(
                        "HTTP error fetching Xtream EPG: {} {}",
                        response.status(),
                        response.status().canonical_reason().unwrap_or("Unknown")
                    )));
                }

                let content = response
                    .text()
                    .await
                    .map_err(|e| AppError::source_error(format!("Failed to read Xtream EPG response: {}", e)))?;

                debug!("Fetched {} of Xtream EPG content", format_memory(content.len() as f64));
                Ok(content)
            }
            Err(e) => {
                // If HTTPS failed and URL started with https://, try HTTP fallback
                if epg_url.starts_with("https://") {
                    warn!("HTTPS EPG fetch failed for '{}', trying HTTP fallback", source.name);
                    
                    let http_url = epg_url.replace("https://", "http://");
                    debug!("Fetching Xtream EPG content from HTTP fallback: {}", crate::utils::url::UrlUtils::obfuscate_credentials(&http_url));
                    
                    match self.client.get(&http_url).send().await {
                        Ok(response) => {
                            if !response.status().is_success() {
                                return Err(AppError::source_error(format!(
                                    "HTTP error fetching Xtream EPG (fallback): {} {}",
                                    response.status(),
                                    response.status().canonical_reason().unwrap_or("Unknown")
                                )));
                            }

                            let content = response
                                .text()
                                .await
                                .map_err(|e| AppError::source_error(format!("Failed to read Xtream EPG response (fallback): {}", e)))?;

                            info!("Successfully fetched {} of Xtream EPG content using HTTP fallback", format_memory(content.len() as f64));
                            Ok(content)
                        }
                        Err(fallback_e) => {
                            Err(AppError::source_error(format!(
                                "Failed to fetch Xtream EPG: HTTPS error: {}, HTTP fallback error: {}",
                                e, fallback_e
                            )))
                        }
                    }
                } else {
                    Err(AppError::source_error(format!("Failed to fetch Xtream EPG: {}", e)))
                }
            }
        }
    }

    /// Parse Xtream EPG content (which is usually XMLTV format) - programs-only mode
    async fn parse_xtream_epg_content(
        &self,
        source: &EpgSource,
        content: &str,
        progress_callback: Option<&EpgProgressCallback>,
    ) -> AppResult<Vec<EpgProgram>> {
        if let Some(callback) = progress_callback {
            callback(EpgIngestionProgress::starting("Parsing Xtream EPG content"));
        }

        // Parse using our custom quick-xml parser
        let xmltv_programs = crate::utils::xmltv_parser::parse_xmltv_programs(content)?;

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
            
            // Parse start and stop times (Xtream usually provides proper timezone info)
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
                "Removed {} duplicate program entries from Xtream EPG feed for source '{}'", 
                duplicate_program_count, source.name
            );
        }

        if let Some(callback) = progress_callback {
            callback(EpgIngestionProgress::starting("Parsing complete")
                .update_step("Parsing complete", Some(100.0)));
        }

        info!(
            "Parsed Xtream EPG for source '{}': {} programs",
            source.name,
            epg_programs.len()
        );

        Ok(epg_programs)
    }

    /// Test Xtream authentication by making a simple API call
    async fn test_xtream_auth(&self, source: &EpgSource) -> AppResult<bool> {
        use tracing::warn;
        
        if let (Some(username), Some(password)) = (&source.username, &source.password) {
            
            // Ensure URL has proper scheme (same logic as stream source handler)
            let base_url = if source.url.starts_with("http://") || source.url.starts_with("https://") {
                source.url.clone()
            } else {
                // Default to HTTPS for security
                format!("https://{}", source.url)
            };
            
            let test_url = format!(
                "{}/player_api.php?username={}&password={}&action=get_user_info",
                base_url.trim_end_matches('/'),
                username,
                password
            );
            
            // Try HTTPS first, fallback to HTTP if it fails (same as stream source handler)
            
            match self.client.get(&test_url).send().await {
                Ok(response) => {
                    
                    if !response.status().is_success() {
                        warn!("EPG auth failed with HTTP status: {}", response.status());
                        return Ok(false);
                    }
                    
                    // Parse the response to check if authentication actually succeeded
                    match response.json::<serde_json::Value>().await {
                        Ok(json) => {
                            
                            // Check if we have user_info and if the status is "Active"
                            if let Some(user_info) = json.get("user_info") {
                                if let Some(status) = user_info.get("status") {
                                    if let Some(status_str) = status.as_str() {
                                        let is_active = status_str == "Active";
                                        return Ok(is_active);
                                    }
                                }
                            }
                            // If we don't have proper user_info, it might be an error response
                            warn!("EPG auth response missing valid user_info");
                            Ok(false)
                        }
                        Err(e) => {
                            warn!("Failed to parse EPG auth response JSON: {}", e);
                            Ok(false)
                        }
                    }
                }
                Err(e) => {
                    // If HTTPS failed and URL started with https://, try HTTP fallback
                    if base_url.starts_with("https://") {
                        warn!("HTTPS EPG auth failed for '{}', trying HTTP fallback", source.name);
                        
                        let http_url = base_url.replace("https://", "http://");
                        let fallback_test_url = format!(
                            "{}/player_api.php?username={}&password={}&action=get_user_info",
                            http_url.trim_end_matches('/'),
                            username,
                            password
                        );
                        
                        match self.client.get(&fallback_test_url).send().await {
                            Ok(response) => {
                                
                                if !response.status().is_success() {
                                    warn!("EPG auth fallback failed with HTTP status: {}", response.status());
                                    return Ok(false);
                                }
                                
                                // Parse the response to check if authentication actually succeeded
                                match response.json::<serde_json::Value>().await {
                                    Ok(json) => {
                                        
                                        // Check if we have user_info and if the status is "Active"
                                        if let Some(user_info) = json.get("user_info") {
                                            if let Some(status) = user_info.get("status") {
                                                if let Some(status_str) = status.as_str() {
                                                    let is_active = status_str == "Active";
                                                    return Ok(is_active);
                                                }
                                            }
                                        }
                                        // If we don't have proper user_info, it might be an error response
                                        warn!("EPG auth fallback response missing valid user_info");
                                        Ok(false)
                                    }
                                    Err(e) => {
                                        warn!("Failed to parse EPG auth fallback response JSON: {}", e);
                                        Ok(false)
                                    }
                                }
                            }
                            Err(fallback_e) => {
                                warn!("EPG auth fallback also failed: {}", fallback_e);
                                Ok(false)
                            }
                        }
                    } else {
                        warn!("EPG auth network request failed: {}", e);
                        Ok(false)
                    }
                }
            }
        } else {
            warn!("EPG source '{}' missing credentials: username={:?}, password={:?}", 
                  source.name, source.username, source.password.as_ref().map(|p| format!("{}chars", p.len())));
            Ok(false)
        }
    }
}

impl Default for XtreamEpgHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EpgSourceHandler for XtreamEpgHandler {
    fn epg_source_type(&self) -> EpgSourceType {
        EpgSourceType::Xtream
    }

    async fn validate_epg_source(&self, source: &EpgSource) -> AppResult<SourceValidationResult> {
        let mut validation = SourceValidationResult::success();
        
        // Validate EPG source type
        if source.source_type != EpgSourceType::Xtream {
            return Ok(SourceValidationResult::failure(vec![
                format!("Expected Xtream source type, got {:?}", source.source_type),
            ]));
        }

        // Validate URL
        if source.url.is_empty() {
            validation.errors.push("URL is required for Xtream sources".to_string());
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

        // Validate authentication (required for Xtream)
        if source.username.is_none() || source.password.is_none() {
            validation.errors.push("Username and password are required for Xtream sources".to_string());
            validation.is_valid = false;
        } else if let (Some(username), Some(password)) = (&source.username, &source.password) {
            if username.is_empty() || password.is_empty() {
                validation.errors.push("Username and password cannot be empty for Xtream sources".to_string());
                validation.is_valid = false;
            } else {
                validation = validation.with_context("authentication", "provided");
            }
        }

        Ok(validation)
    }

    async fn get_epg_capabilities(&self, _source: &EpgSource) -> AppResult<EpgSourceCapabilities> {
        Ok(EpgSourceCapabilities::xtream_epg())
    }

    async fn test_epg_connectivity(&self, source: &EpgSource) -> AppResult<bool> {
        // Test both authentication and EPG endpoint
        if !self.test_xtream_auth(source).await? {
            return Ok(false);
        }

        // Try to fetch a small amount of EPG data to test the endpoint
        match self.build_epg_url(source) {
            Ok(epg_url) => {
                match self.client.head(&epg_url).send().await {
                    Ok(response) => Ok(response.status().is_success()),
                    Err(_) => Ok(false),
                }
            }
            Err(_) => Ok(false),
        }
    }

    async fn get_epg_source_info(&self, source: &EpgSource) -> AppResult<HashMap<String, String>> {
        let mut info = HashMap::new();
        info.insert("source_type".to_string(), "xtream".to_string());
        info.insert("url".to_string(), source.url.clone());
        
        if let (Some(username), Some(_)) = (&source.username, &source.password) {
            info.insert("username".to_string(), username.clone());
            info.insert("has_password".to_string(), "true".to_string());
        }

        // Try to get server info from Xtream API
        if let Ok(auth_success) = self.test_xtream_auth(source).await {
            info.insert("authentication".to_string(), auth_success.to_string());
        }

        Ok(info)
    }
}

#[async_trait]
impl EpgProgramIngestor for XtreamEpgHandler {
    async fn ingest_epg_programs(&self, source: &EpgSource) -> AppResult<Vec<EpgProgram>> {
        self.ingest_epg_programs_with_progress(source, None).await
    }

    async fn ingest_epg_programs_with_progress(
        &self,
        source: &EpgSource,
        progress_callback: Option<&EpgProgressCallback>,
    ) -> AppResult<Vec<EpgProgram>> {
        if let Some(callback) = progress_callback {
            callback(EpgIngestionProgress::starting("Authenticating with Xtream"));
        }

        // Test authentication first
        if !self.test_xtream_auth(source).await? {
            return Err(AppError::source_error(format!("Xtream authentication failed for source: {}", source.name)));
        }

        if let Some(callback) = progress_callback {
            callback(EpgIngestionProgress::starting("Authentication successful")
                .update_step("Fetching EPG content", Some(20.0)));
        }

        // Fetch EPG content
        let content = self.fetch_xtream_epg_content(source).await?;

        if let Some(callback) = progress_callback {
            callback(EpgIngestionProgress::starting("Fetching complete")
                .update_step("Parsing EPG", Some(40.0)));
        }

        // Parse content
        self.parse_xtream_epg_content(source, &content, progress_callback).await
    }

    async fn estimate_program_count(&self, source: &EpgSource) -> AppResult<Option<u32>> {
        // For Xtream, we could potentially get some info from the API
        // but for now, we'll return None like XMLTV
        debug!("Program count estimation not available for Xtream source: {}", source.name);
        Ok(None)
    }

    async fn ingest_epg_programs_with_universal_progress(
        &self,
        source: &EpgSource,
        progress_callback: Option<&crate::sources::traits::UniversalCallback>,
    ) -> AppResult<Vec<EpgProgram>> {
        use crate::services::progress_service::{UniversalProgress, OperationType, UniversalState};
        use uuid::Uuid;

        info!("Starting Xtream EPG ingestion with universal progress for source: {}", source.name);
        let source_id = Uuid::new_v4(); // Generate operation ID

        if let Some(callback) = progress_callback {
            let progress = UniversalProgress::new(
                source_id,
                OperationType::EpgIngestion,
                format!("Xtream EPG Ingestion: {}", source.name)
            )
            .set_state(UniversalState::Connecting)
            .update_step("Authenticating with Xtream".to_string());
            callback(progress);
        }

        // Test authentication first
        let auth_success = match self.test_xtream_auth(source).await {
            Ok(success) => success,
            Err(e) => {
                if let Some(callback) = progress_callback {
                    let progress = UniversalProgress::new(
                        source_id,
                        OperationType::EpgIngestion,
                        format!("Xtream EPG Ingestion: {}", source.name)
                    )
                    .set_error(format!("Authentication test failed: {}", e));
                    callback(progress);
                }
                return Err(e);
            }
        };

        if !auth_success {
            let error_msg = format!("Xtream authentication failed for source: {}", source.name);
            if let Some(callback) = progress_callback {
                let progress = UniversalProgress::new(
                    source_id,
                    OperationType::EpgIngestion,
                    format!("Xtream EPG Ingestion: {}", source.name)
                )
                .set_error(error_msg.clone());
                callback(progress);
            }
            return Err(AppError::source_error(error_msg));
        }

        if let Some(callback) = progress_callback {
            let progress = UniversalProgress::new(
                source_id,
                OperationType::EpgIngestion,
                format!("Xtream EPG Ingestion: {}", source.name)
            )
            .set_state(UniversalState::Downloading)
            .update_step("Fetching EPG content".to_string())
            .update_percentage(20.0);
            callback(progress);
        }

        // Fetch EPG content
        let content = match self.fetch_xtream_epg_content(source).await {
            Ok(content) => content,
            Err(e) => {
                if let Some(callback) = progress_callback {
                    let progress = UniversalProgress::new(
                        source_id,
                        OperationType::EpgIngestion,
                        format!("Xtream EPG Ingestion: {}", source.name)
                    )
                    .set_error(format!("Failed to fetch EPG content: {}", e));
                    callback(progress);
                }
                return Err(e);
            }
        };

        if let Some(callback) = progress_callback {
            let progress = UniversalProgress::new(
                source_id,
                OperationType::EpgIngestion,
                format!("Xtream EPG Ingestion: {}", source.name)
            )
            .set_state(UniversalState::Processing)
            .update_step("Parsing EPG content".to_string())
            .update_percentage(40.0);
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
                        format!("Xtream EPG Ingestion: {}", source.name)
                    )
                    .set_error(format!("Failed to parse Xtream EPG (XMLTV format): {}", e));
                    callback(progress);
                }
                return Err(e);
            }
        };

        if let Some(callback) = progress_callback {
            let progress = UniversalProgress::new(
                source_id,
                OperationType::EpgIngestion,
                format!("Xtream EPG Ingestion: {}", source.name)
            )
            .set_state(UniversalState::Processing)
            .update_step("Converting programs".to_string())
            .update_percentage(60.0);
            callback(progress);
        }

        // Skip channel processing - programs-only approach for database-first generation

        if let Some(callback) = progress_callback {
            let progress = UniversalProgress::new(
                source_id,
                OperationType::EpgIngestion,
                format!("Xtream EPG Ingestion: {}", source.name)
            )
            .set_state(UniversalState::Processing)
            .update_step("Converting programs".to_string())
            .update_percentage(80.0);
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
            
            // Parse start and stop times (Xtream usually provides proper timezone info)
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
                "Removed {} duplicate program entries from Xtream EPG feed for source '{}'", 
                duplicate_program_count, source.name
            );
        }

        if let Some(callback) = progress_callback {
            let progress = UniversalProgress::new(
                source_id,
                OperationType::EpgIngestion,
                format!("Xtream EPG Ingestion: {}", source.name)
            )
            .set_state(UniversalState::Completed)
            .update_step("EPG ingestion complete".to_string())
            .update_percentage(100.0)
            .update_items(epg_programs.len(), Some(epg_programs.len()));
            callback(progress);
        }

        info!(
            "Parsed Xtream EPG for source '{}': {} programs",
            source.name,
            epg_programs.len()
        );

        Ok(epg_programs)
    }
}

impl FullEpgSourceHandler for XtreamEpgHandler {
    fn get_epg_handler_summary(&self) -> EpgSourceHandlerSummary {
        EpgSourceHandlerSummary {
            epg_source_type: EpgSourceType::Xtream,
            supports_program_ingestion: true,
            supports_authentication: true,
        }
    }
}