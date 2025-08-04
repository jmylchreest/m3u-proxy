//! Logo caching stage for pipeline processing
//!
//! This stage processes records after data mapping and filtering to automatically
//! cache remote logo URLs. It respects per-proxy configuration settings and only
//! caches external URLs while leaving proxy URLs unchanged.

use crate::models::Channel;
use crate::pipeline::models::{PipelineArtifact, ArtifactType, ContentType, ProcessingStage};
use crate::pipeline::engines::rule_processor::{FieldModification, EpgProgram};
use crate::pipeline::traits::{PipelineStage, ProgressAware};
use crate::pipeline::error::PipelineError;
use crate::logo_assets::service::LogoAssetService;
use crate::services::progress_service::ProgressManager;
use sandboxed_file_manager::SandboxedManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, trace, warn};

// Configurable batch sizes and intervals for logo caching optimization
const LOGO_CACHING_BATCH_SIZE: usize = 1000;         // Process logos in batches to reduce memory pressure
const LOGO_PROGRESS_BATCH_INTERVAL: usize = 10;      // Log progress every N batches (10 batches = 10,000 channels)

/// Configuration for logo caching behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoCachingConfig {
    pub cache_channel_logos: bool,
    pub cache_program_logos: bool,
    pub base_url: String,
}

/// Logo caching stage that processes remote URLs and caches them locally
pub struct LogoCachingStage {
    file_manager: SandboxedManager,
    pipeline_execution_prefix: String,
    logo_service: Arc<LogoAssetService>,
    config: LogoCachingConfig,
    progress_manager: Option<Arc<ProgressManager>>,
}

/// Classification of logo URLs for processing decisions
#[derive(Debug, Clone, PartialEq)]
enum LogoUrlType {
    LocalProxy,    // URLs pointing to this proxy (skip caching)
    RemoteUrl,     // External HTTP/HTTPS URLs (cache if enabled)
    Unknown,       // Other formats (skip)
}

/// Result of logo caching operations
#[derive(Debug)]
struct LogoCachingResult {
    pub processed_records: Vec<Channel>,
    pub total_processed: usize,
    pub total_cached: usize,
    pub cache_failures: usize,
    pub cache_hits: usize,
    pub total_downloaded_bytes: u64,
    pub local_proxy_urls: usize,
    pub remote_urls: usize,
    pub unknown_urls: usize,
    #[allow(dead_code)]
    pub field_modifications: Vec<FieldModification>,
}

/// Result of EPG logo caching operations
#[derive(Debug)]
struct EpgLogoCachingResult {
    pub processed_records: Vec<EpgProgram>,
    pub total_processed: usize,
    pub total_cached: usize,
    pub cache_failures: usize,
    pub cache_hits: usize,
    pub total_downloaded_bytes: u64,
    #[allow(dead_code)]
    pub local_proxy_urls: usize,
    #[allow(dead_code)]
    pub unknown_urls: usize,
    #[allow(dead_code)]
    pub field_modifications: Vec<FieldModification>,
}

impl LogoCachingStage {
    /// Generate a normalized cache ID from a logo URL using the same algorithm as LogoAssetService
    fn generate_cache_id_from_url(url: &str) -> Result<String, Box<dyn std::error::Error>> {
        use sha2::{Digest, Sha256};
        use std::collections::BTreeMap;
        use url::Url;
        
        let parsed_url = Url::parse(url).map_err(|e| {
            format!("Invalid URL '{}': {}", url, e)
        })?;

        // Start building normalized URL without scheme
        let mut normalized = String::new();

        // Add host
        if let Some(host) = parsed_url.host_str() {
            normalized.push_str(host);
        }

        // Add port if not default
        if let Some(port) = parsed_url.port() {
            let is_default_port = (parsed_url.scheme() == "http" && port == 80)
                || (parsed_url.scheme() == "https" && port == 443);
            if !is_default_port {
                normalized.push(':');
                normalized.push_str(&port.to_string());
            }
        }

        // Add path without file extension
        let path = parsed_url.path();
        if let Some(last_slash) = path.rfind('/') {
            let (dir_part, file_part) = path.split_at(last_slash + 1);
            normalized.push_str(dir_part);

            // Remove extension from filename
            if let Some(dot_pos) = file_part.rfind('.') {
                normalized.push_str(&file_part[..dot_pos]);
            } else {
                normalized.push_str(file_part);
            }
        } else {
            // No slash in path, treat whole path as filename
            if let Some(dot_pos) = path.rfind('.') {
                normalized.push_str(&path[..dot_pos]);
            } else {
                normalized.push_str(path);
            }
        }

        // Sort and add query parameters
        let mut sorted_params = BTreeMap::new();
        for (key, value) in parsed_url.query_pairs() {
            sorted_params.insert(key.to_string(), value.to_string());
        }

        if !sorted_params.is_empty() {
            normalized.push('?');
            let param_string: Vec<String> = sorted_params
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            normalized.push_str(&param_string.join("&"));
        }

        // Generate SHA256 hash
        let mut hasher = Sha256::new();
        hasher.update(normalized.as_bytes());
        let hash = hasher.finalize();

        Ok(format!("{:x}", hash))
    }
    pub async fn new(
        file_manager: SandboxedManager,
        pipeline_execution_prefix: String,
        logo_service: Arc<LogoAssetService>,
        config: LogoCachingConfig,
        progress_manager: Option<Arc<ProgressManager>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            file_manager,
            pipeline_execution_prefix,
            logo_service,
            config,
            progress_manager,
        })
    }
    
    /// Helper method for reporting progress
    async fn report_progress(&self, percentage: f64, message: &str) {
        if let Some(pm) = &self.progress_manager {
            if let Some(updater) = pm.get_stage_updater("logo_caching").await {
                updater.update_progress(percentage, message).await;
            }
        }
    }
    
    /// Set the progress manager for this stage
    pub fn set_progress_manager(&mut self, progress_manager: Arc<ProgressManager>) {
        self.progress_manager = Some(progress_manager);
    }

    /// Process channels from a pipeline artifact
    pub async fn process_channels(&mut self) -> Result<PipelineArtifact, Box<dyn std::error::Error>> {
        let stage_start = Instant::now();
        info!("Starting logo caching stage");

        // Read input artifact from filtering stage
        let input_file_path = format!("{}_filtered_channels.jsonl", self.pipeline_execution_prefix);
        
        if !self.file_manager.exists(&input_file_path).await? {
            warn!("No filtered channels file found at {}, skipping logo caching", input_file_path);
            let output_path = format!("{}_logo_cached_channels.jsonl", self.pipeline_execution_prefix);
            return Ok(PipelineArtifact::new(
                ArtifactType::logo_cached_channels(),
                output_path,
                "logo_caching".to_string(),
            ));
        }

        let channels = self.read_channels_from_file(&input_file_path).await?;
        info!("Loaded {} channels for logo caching", channels.len());

        if !self.config.cache_channel_logos {
            info!("Channel logo caching is disabled, copying channels without modification");
            let output_file_path = format!("{}_logo_cached_channels.jsonl", self.pipeline_execution_prefix);
            self.write_channels_to_file(&channels, &output_file_path).await?;
            
            return Ok(PipelineArtifact::new(
                ArtifactType::logo_cached_channels(),
                output_file_path,
                "logo_caching".to_string(),
            ));
        }

        let result = self.process_channel_logos(channels).await?;
        
        let output_file_path = format!("{}_logo_cached_channels.jsonl", self.pipeline_execution_prefix);
        self.write_channels_to_file(&result.processed_records, &output_file_path).await?;

        let stage_duration = stage_start.elapsed();
        let average_logo_size = if result.total_cached > 0 {
            result.total_downloaded_bytes / result.total_cached as u64
        } else {
            0
        };
        
        info!(
            "Logo caching stage completed: processed={} cached={} failures={} hits={} total_downloaded_bytes={} average_logo_size={} duration={:?}",
            result.total_processed, 
            result.total_cached, 
            result.cache_failures,
            result.cache_hits,
            result.total_downloaded_bytes,
            average_logo_size,
            stage_duration
        );
        
        info!(
            "URL classification: remote={} local_proxy={} unknown={} hit_rate={:.1}%",
            result.remote_urls,
            result.local_proxy_urls,
            result.unknown_urls,
            if result.remote_urls > 0 {
                (result.cache_hits as f32 / result.remote_urls as f32) * 100.0
            } else {
                0.0
            }
        );

        let output_artifact = PipelineArtifact::new(
            ArtifactType::logo_cached_channels(),
            output_file_path,
            "logo_caching".to_string(),
        ).with_record_count(result.processed_records.len());

        // Memory cleanup is handled automatically when variables go out of scope

        info!("Logo caching channels stage completed, memory cleanup performed");
        Ok(output_artifact)
    }

    /// Process all channel logos according to caching configuration using batched processing
    async fn process_channel_logos(&self, channels: Vec<Channel>) -> Result<LogoCachingResult, Box<dyn std::error::Error>> {
        let mut processed_records = Vec::new();
        let mut total_cached = 0;
        let mut cache_failures = 0;
        let mut cache_hits = 0;
        let mut total_downloaded_bytes = 0u64;
        let mut local_proxy_urls = 0;
        let mut remote_urls = 0;
        let mut unknown_urls = 0;
        let mut field_modifications = Vec::new();
        
        let total_channels = channels.len();
        let start_time = std::time::Instant::now();
        let mut processed_count = 0;

        // Process channels in batches to reduce memory pressure and improve performance
        let channel_batches: Vec<_> = channels.chunks(LOGO_CACHING_BATCH_SIZE).collect();
        
        let total_batches = channel_batches.len();
        
        for (batch_index, batch) in channel_batches.into_iter().enumerate() {
            let batch_start = std::time::Instant::now();
            let mut batch_processed = Vec::new();
            
            for mut channel in batch.iter().cloned() {
                processed_count += 1;
                
                let original_logo = channel.tvg_logo.clone();
                
                if let Some(ref logo_url) = original_logo {
                    match self.classify_logo_url(logo_url) {
                        LogoUrlType::RemoteUrl => {
                            remote_urls += 1;
                            trace!("Processing remote logo URL for channel {}: {}", channel.channel_name, logo_url);
                            
                            // Check if logo already exists (cache hit detection)
                            let cache_id = Self::generate_cache_id_from_url(logo_url)?;
                            let logo_file_path = format!("{}.png", cache_id);
                            let logo_exists = self.file_manager.exists(&logo_file_path).await.unwrap_or(false);
                            
                            if logo_exists {
                                cache_hits += 1;
                                trace!("Cache hit for channel {}: {} (existing cache_id={})", 
                                    channel.channel_name, logo_url, cache_id);
                            }
                            
                            match self.logo_service.cache_logo_from_url_with_size_tracking(logo_url).await {
                                Ok((returned_cache_id, bytes_transferred)) => {
                                    let new_url = self.logo_service.get_cached_logo_url(&returned_cache_id, &self.config.base_url);
                                    channel.tvg_logo = Some(new_url.clone());
                                    
                                    if !logo_exists {
                                        total_cached += 1;
                                        total_downloaded_bytes += bytes_transferred;
                                    }
                                    
                                    // Track the field modification
                                    field_modifications.push(FieldModification {
                                        field_name: "tvg_logo".to_string(),
                                        old_value: Some(logo_url.clone()),
                                        new_value: Some(new_url),
                                        modification_type: crate::pipeline::engines::rule_processor::ModificationType::Set,
                                    });
                                    
                                    trace!("Processed logo for channel {}: {} -> cache_id={} (hit={} bytes={})", 
                                        channel.channel_name, logo_url, returned_cache_id, logo_exists, bytes_transferred);
                                }
                                Err(e) => {
                                    cache_failures += 1;
                                    warn!("Failed to cache logo for channel {}: {} - Error: {}", 
                                        channel.channel_name, logo_url, e);
                                    // Keep original URL on cache failure
                                }
                            }
                        }
                        LogoUrlType::LocalProxy => {
                            local_proxy_urls += 1;
                            trace!("Skipping local proxy URL for channel {}: {}", channel.channel_name, logo_url);
                            // Keep as-is
                        }
                        LogoUrlType::Unknown => {
                            unknown_urls += 1;
                            trace!("Skipping unknown URL format for channel {}: {}", channel.channel_name, logo_url);
                            // Keep as-is
                        }
                    }
                }
                
                batch_processed.push(channel);
            }
            
            // Add batch to final results
            processed_records.extend(batch_processed);
            
            // Log batch completion
            let batch_duration = batch_start.elapsed();
            
            // Progress reporting: every N batches + final batch (pure batch-driven)
            let should_log_progress = (batch_index + 1) % LOGO_PROGRESS_BATCH_INTERVAL == 0;
            let is_final_batch = batch_index + 1 == total_batches;
            
            if should_log_progress || is_final_batch {
                let elapsed = start_time.elapsed();
                let progress_pct = processed_count as f32 / total_channels as f32 * 100.0;
                let estimated_remaining = if processed_count > 0 && processed_count < total_channels {
                    let avg_time_per_channel = elapsed.as_secs_f32() / processed_count as f32;
                    std::time::Duration::from_secs_f32(avg_time_per_channel * (total_channels - processed_count) as f32)
                } else {
                    std::time::Duration::ZERO
                };
                
                info!(
                    "Logo caching progress: batch {}/{} channels {}/{} ({:.1}%) cached={} failures={} hits={} downloaded_bytes={} elapsed={:?} eta={:?}",
                    batch_index + 1, total_batches, processed_count, total_channels, progress_pct, 
                    total_cached, cache_failures, cache_hits, total_downloaded_bytes, elapsed, estimated_remaining
                );
                
                // Send SSE progress update with 5-95% range
                let adjusted_progress = 5.0 + (progress_pct as f64 * 0.9); // Scale to 5-95% range
                let progress_message = format!("Caching logos: {}/{} channels ({:.1}%)", processed_count, total_channels, progress_pct);
                self.report_progress(adjusted_progress, &progress_message).await;
            } else {
                // Just log batch completion without full progress details
                debug!("Completed logo caching batch {}/{}: processed={} duration={}", 
                      batch_index + 1, total_batches, batch.len(), 
                      crate::utils::human_format::format_duration_precise(batch_duration));
            }
            
            // Batch memory is automatically cleaned up when it goes out of scope
        }

        let total_processed = processed_records.len();
        
        Ok(LogoCachingResult {
            processed_records,
            total_processed,
            total_cached,
            cache_failures,
            cache_hits,
            total_downloaded_bytes,
            local_proxy_urls,
            remote_urls,
            unknown_urls,
            field_modifications,
        })
    }

    /// Classify a logo URL to determine caching behavior
    fn classify_logo_url(&self, url: &str) -> LogoUrlType {
        if url.starts_with(&format!("{}/api/v1/logos/", self.config.base_url.trim_end_matches('/'))) {
            // This is a URL pointing to our own proxy - don't cache it
            LogoUrlType::LocalProxy
        } else if url.starts_with("http://") || url.starts_with("https://") {
            // External HTTP/HTTPS URL - cache it if enabled
            LogoUrlType::RemoteUrl
        } else {
            // Other formats (data:, file:, relative paths, etc.) - skip
            LogoUrlType::Unknown
        }
    }

    /// Read channels from JSONL file
    async fn read_channels_from_file(&self, file_path: &str) -> Result<Vec<Channel>, Box<dyn std::error::Error>> {
        let content = self.file_manager.read(file_path).await?;
        let content_str = String::from_utf8(content)?;
        
        let mut channels = Vec::new();
        for (line_num, line) in content_str.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            
            match serde_json::from_str::<Channel>(line) {
                Ok(channel) => channels.push(channel),
                Err(e) => {
                    warn!("Failed to parse channel at line {}: {} - Error: {}", line_num + 1, line, e);
                }
            }
        }
        
        Ok(channels)
    }

    /// Write channels to JSONL file
    async fn write_channels_to_file(&self, channels: &[Channel], file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut content = String::new();
        
        for channel in channels {
            let json_line = serde_json::to_string(channel)?;
            content.push_str(&json_line);
            content.push('\n');
        }
        
        self.file_manager.write(file_path, content.as_bytes()).await?;
        debug!("Wrote {} channels to {}", channels.len(), file_path);
        
        Ok(())
    }
    
    /// Generic process method that handles multiple artifact types
    pub async fn process(&mut self, input_artifacts: Vec<PipelineArtifact>) -> Result<Vec<PipelineArtifact>, Box<dyn std::error::Error>> {
        info!("Starting logo caching stage with {} input artifacts", input_artifacts.len());
        let mut output_artifacts = Vec::new();
        
        let total_artifacts = input_artifacts.len();
        for (artifact_index, artifact) in input_artifacts.into_iter().enumerate() {
            let base_progress = 5.0 + (artifact_index as f64 / total_artifacts as f64 * 90.0); // 5% to 95%
            
            let processed_artifact = match artifact.artifact_type.content {
                ContentType::Channels => {
                    self.report_progress(base_progress, &format!("Caching channel logos {}/{}", artifact_index + 1, total_artifacts)).await;
                    // Process channels through existing channel logic
                    self.process_channel_artifact(artifact).await?
                }
                ContentType::EpgPrograms => {
                    self.report_progress(base_progress, &format!("Caching EPG program logos {}/{}", artifact_index + 1, total_artifacts)).await;
                    // Process EPG programs with program_icon field
                    self.process_epg_artifact(artifact).await?
                }
                _ => {
                    // Pass through other content types unchanged
                    self.report_progress(base_progress, &format!("Processing artifact {}/{}", artifact_index + 1, total_artifacts)).await;
                    debug!("Passing through artifact of type {:?} unchanged", artifact.artifact_type.content);
                    artifact
                }
            };
            
            output_artifacts.push(processed_artifact);
        }
        
        Ok(output_artifacts)
    }
    
    /// Process a single channel artifact (wrapper around existing process_channels logic)
    async fn process_channel_artifact(&mut self, artifact: PipelineArtifact) -> Result<PipelineArtifact, Box<dyn std::error::Error>> {
        // Read channels from artifact
        let channels = self.read_channels_from_file(&artifact.file_path).await?;
        
        if !self.config.cache_channel_logos {
            info!("Channel logo caching is disabled, copying channels without modification");
            let output_file_path = format!("{}_logo_cached_channels.jsonl", self.pipeline_execution_prefix);
            self.write_channels_to_file(&channels, &output_file_path).await?;
            
            return Ok(PipelineArtifact::new(
                ArtifactType::logo_cached_channels(),
                output_file_path,
                "logo_caching".to_string(),
            ).with_record_count(channels.len()));
        }
        
        // Process channel logos
        let logo_result = self.process_channel_logos(channels).await?;
        
        // Write processed channels
        let output_file_path = format!("{}_logo_cached_channels.jsonl", self.pipeline_execution_prefix);
        self.write_channels_to_file(&logo_result.processed_records, &output_file_path).await?;
        
        // Create output artifact with comprehensive metadata
        let output_artifact = PipelineArtifact::new(
            ArtifactType::logo_cached_channels(),
            output_file_path,
            "logo_caching".to_string(),
        )
        .with_record_count(logo_result.total_processed)
        .with_metadata("total_cached".to_string(), serde_json::Value::Number(serde_json::Number::from(logo_result.total_cached)))
        .with_metadata("cache_failures".to_string(), serde_json::Value::Number(serde_json::Number::from(logo_result.cache_failures)))
        .with_metadata("cache_hits".to_string(), serde_json::Value::Number(serde_json::Number::from(logo_result.cache_hits)))
        .with_metadata("total_downloaded_bytes".to_string(), serde_json::Value::Number(serde_json::Number::from(logo_result.total_downloaded_bytes)));
        
        Ok(output_artifact)
    }
    
    /// Process EPG programs artifact with program_icon field support
    async fn process_epg_artifact(&mut self, artifact: PipelineArtifact) -> Result<PipelineArtifact, Box<dyn std::error::Error>> {
        info!("Processing EPG programs artifact: {}", artifact.file_path);
        
        if !self.config.cache_program_logos {
            info!("Program logo caching is disabled, copying programs without modification");
            let output_file_path = format!("{}_logo_cached_programs.jsonl", self.pipeline_execution_prefix);
            
            // Read programs using the proper method, then write them unchanged
            let programs = self.read_programs_from_file(&artifact.file_path).await?;
            self.write_programs_to_file(&programs, &output_file_path).await?;
            
            return Ok(PipelineArtifact::new(
                ArtifactType::new(ContentType::EpgPrograms, ProcessingStage::LogoCached),
                output_file_path,
                "logo_caching".to_string(),
            ).with_record_count(artifact.record_count.unwrap_or(0)));
        }
        
        // Read EPG programs from file
        let programs = self.read_programs_from_file(&artifact.file_path).await?;
        info!("Loaded {} EPG programs for logo caching", programs.len());
        
        // Process program logos
        let logo_result = self.process_program_logos(programs).await?;
        
        // Write processed programs
        let output_file_path = format!("{}_logo_cached_programs.jsonl", self.pipeline_execution_prefix);
        self.write_programs_to_file(&logo_result.processed_records, &output_file_path).await?;
        
        // Create output artifact with metadata
        let output_artifact = PipelineArtifact::new(
            ArtifactType::new(ContentType::EpgPrograms, ProcessingStage::LogoCached),
            output_file_path,
            "logo_caching".to_string(),
        )
        .with_record_count(logo_result.total_processed)
        .with_metadata("total_cached".to_string(), serde_json::Value::Number(serde_json::Number::from(logo_result.total_cached)))
        .with_metadata("cache_failures".to_string(), serde_json::Value::Number(serde_json::Number::from(logo_result.cache_failures)))
        .with_metadata("cache_hits".to_string(), serde_json::Value::Number(serde_json::Number::from(logo_result.cache_hits)))
        .with_metadata("total_downloaded_bytes".to_string(), serde_json::Value::Number(serde_json::Number::from(logo_result.total_downloaded_bytes)));
        
        Ok(output_artifact)
    }
    
    /// Read EPG programs from JSONL file
    async fn read_programs_from_file(&self, file_path: &str) -> Result<Vec<EpgProgram>, Box<dyn std::error::Error>> {
        let content = String::from_utf8(self.file_manager.read(file_path).await?)?;
        
        let programs: Result<Vec<_>, _> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str::<EpgProgram>(line))
            .collect();
        
        let programs = programs?;
        debug!("Read {} EPG programs from {}", programs.len(), file_path);
        
        Ok(programs)
    }
    
    /// Write EPG programs to JSONL file
    async fn write_programs_to_file(&self, programs: &[EpgProgram], file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut content = String::new();
        for program in programs {
            let json_line = serde_json::to_string(program)?;
            content.push_str(&json_line);
            content.push('\n');
        }
        
        self.file_manager.write(file_path, content.as_bytes()).await?;
        debug!("Wrote {} EPG programs to {}", programs.len(), file_path);
        
        Ok(())
    }
    
    /// Process program logos similar to channel logos
    async fn process_program_logos(&self, programs: Vec<EpgProgram>) -> Result<EpgLogoCachingResult, Box<dyn std::error::Error>> {
        let total_input = programs.len();
        let mut processed_programs = Vec::with_capacity(total_input);
        let mut total_cached = 0;
        let mut cache_failures = 0;
        let cache_hits = 0;
        let mut total_downloaded_bytes = 0;
        let mut local_proxy_urls = 0;
        let mut unknown_urls = 0;
        
        info!("Processing program logos for {} programs", total_input);
        
        for mut program in programs {
            if let Some(program_icon_url) = program.program_icon.clone() {
                let url_type = self.classify_logo_url(&program_icon_url);
                match url_type {
                    LogoUrlType::RemoteUrl => {
                        trace!("Caching remote program icon: {}", program_icon_url);
                        match self.logo_service.cache_logo_from_url_with_size_tracking(&program_icon_url).await {
                            Ok((cache_id, bytes_transferred)) => {
                                let cached_url = self.logo_service.get_cached_logo_url(&cache_id, &self.config.base_url);
                                program.program_icon = Some(cached_url);
                                total_cached += 1;
                                total_downloaded_bytes += bytes_transferred;
                                
                                trace!("Program icon downloaded and cached: {} -> {}", 
                                    program_icon_url, program.program_icon.as_ref().unwrap());
                            }
                            Err(e) => {
                                warn!("Failed to cache program icon {}: {}", program_icon_url, e);
                                cache_failures += 1;
                                // Keep original URL on cache failure
                            }
                        }
                    }
                    LogoUrlType::LocalProxy => {
                        local_proxy_urls += 1;
                        trace!("Skipping local proxy program icon: {}", program_icon_url);
                    }
                    LogoUrlType::Unknown => {
                        unknown_urls += 1;
                        trace!("Skipping unknown format program icon: {}", program_icon_url);
                    }
                }
            }
            processed_programs.push(program);
        }
        
        info!(
            "Program logo caching completed: processed={} cached={} failures={} cache_hits={} local_proxy={} unknown={} downloaded_bytes={}",
            total_input, total_cached, cache_failures, cache_hits, local_proxy_urls, unknown_urls, total_downloaded_bytes
        );
        
        let result = EpgLogoCachingResult {
            processed_records: processed_programs,
            total_processed: total_input,
            total_cached,
            cache_failures,
            cache_hits,
            total_downloaded_bytes,
            local_proxy_urls,
            unknown_urls,
            field_modifications: Vec::new(), // Could track program_icon field modifications
        };

        // Memory cleanup is handled automatically when variables go out of scope

        info!("Logo caching EPG programs stage completed, memory cleanup performed");
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;
    
    fn create_test_channel(name: &str, logo_url: Option<String>) -> Channel {
        Channel {
            id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            tvg_id: None,
            tvg_name: None,
            tvg_logo: logo_url,
            tvg_shift: None,
            group_title: None,
            channel_name: name.to_string(),
            stream_url: "http://example.com/stream".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
    
    #[test]
    #[ignore] // TODO: Requires mocking LogoAssetService for proper testing
    fn test_logo_url_classification() {
        let config = LogoCachingConfig {
            cache_channel_logos: true,
            cache_program_logos: false,
            base_url: "https://proxy.example.com".to_string(),
        };
        // Test URL classification logic directly without needing the full service
        fn classify_test_url(url: &str, base_url: &str) -> LogoUrlType {
            if url.starts_with(&format!("{}/api/v1/logos/", base_url.trim_end_matches('/'))) {
                LogoUrlType::LocalProxy
            } else if url.starts_with("http://") || url.starts_with("https://") {
                LogoUrlType::RemoteUrl
            } else {
                LogoUrlType::Unknown
            }
        }
        
        let base = &config.base_url;
        
        // Local proxy URLs should be skipped
        assert_eq!(
            classify_test_url("https://proxy.example.com/api/v1/logos/uuid123", base),
            LogoUrlType::LocalProxy
        );
        assert_eq!(
            classify_test_url("https://proxy.example.com/api/v1/logos/cached/cache123", base),
            LogoUrlType::LocalProxy
        );
        
        // Remote HTTP/HTTPS URLs should be cached
        assert_eq!(
            classify_test_url("https://external.com/logo.png", base),
            LogoUrlType::RemoteUrl
        );
        assert_eq!(
            classify_test_url("http://provider.tv/logos/channel.jpg", base),
            LogoUrlType::RemoteUrl
        );
        
        // Other formats should be skipped
        assert_eq!(
            classify_test_url("data:image/png;base64,iVBOR...", base),
            LogoUrlType::Unknown
        );
        assert_eq!(
            classify_test_url("/local/path/logo.png", base),
            LogoUrlType::Unknown
        );
        assert_eq!(
            classify_test_url("ftp://server.com/logo.png", base),
            LogoUrlType::Unknown
        );
    }
    
    #[test]
    fn test_channel_logo_processing_logic() {
        let _channels = vec![
            create_test_channel("BBC One", Some("https://external.com/bbc.png".to_string())),
            create_test_channel("ITV", Some("https://proxy.example.com/api/v1/logos/uuid123".to_string())),
            create_test_channel("Channel 4", Some("data:image/png;base64,abc123".to_string())),
            create_test_channel("Channel 5", None),
        ];
        
        // This test demonstrates the expected processing logic
        // In a real test, we'd mock LogoAssetService to verify the behavior
    }
}

impl ProgressAware for LogoCachingStage {
    fn get_progress_manager(&self) -> Option<&Arc<ProgressManager>> {
        self.progress_manager.as_ref()
    }
}

#[async_trait::async_trait]
impl PipelineStage for LogoCachingStage {
    async fn execute(&mut self, input: Vec<PipelineArtifact>) -> Result<Vec<PipelineArtifact>, PipelineError> {
        self.report_progress(5.0, "Initializing logo cache").await;
        let result = self.process(input).await
            .map_err(|e| PipelineError::stage_error("logo_caching", format!("Logo caching failed: {}", e)))?;
        self.report_progress(100.0, "Logo caching completed").await;
        Ok(result)
    }
    
    fn stage_id(&self) -> &'static str {
        "logo_caching"
    }
    
    fn stage_name(&self) -> &'static str {
        "Logo Caching"
    }
    
    async fn cleanup(&mut self) -> Result<(), PipelineError> {
        Ok(())
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}