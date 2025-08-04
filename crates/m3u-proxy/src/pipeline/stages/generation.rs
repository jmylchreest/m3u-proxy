use anyhow::Result;
use sandboxed_file_manager::SandboxedManager;
use sqlx::SqlitePool;
use std::collections::{HashMap, BTreeSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::logo_assets::service::LogoAssetService;
use crate::models::{NumberedChannel, ChannelNumberAssignmentType};
use crate::pipeline::engines::rule_processor::EpgProgram;
use crate::pipeline::models::{PipelineArtifact, ArtifactType, ContentType, ProcessingStage};
use crate::pipeline::traits::{PipelineStage, ProgressAware};
use crate::pipeline::error::PipelineError;
use crate::services::progress_service::ProgressManager;


/// Optimized channel info for M3U generation (removed unused fields)
#[derive(Debug)]
struct ChannelInfo {
    stream_display_names: BTreeSet<String>,     // From M3U channels (by tvg_id)
    logo_url: Option<String>,                   // Logo from first detected channel
}

/// Generation stage - streams to temporary files in pipeline storage
/// Files will be atomically published by the publish_content stage
pub struct GenerationStage {
    #[allow(dead_code)]
    db_pool: SqlitePool,
    pipeline_file_manager: SandboxedManager,  // Pipeline temporary storage
    pipeline_execution_prefix: String,
    proxy_id: Uuid,
    #[allow(dead_code)]
    base_url: String,
    progress_manager: Option<Arc<ProgressManager>>,
}

impl GenerationStage {
    pub async fn new(
        db_pool: SqlitePool,
        pipeline_file_manager: SandboxedManager,  // Pipeline temporary storage
        pipeline_execution_prefix: String,
        proxy_id: Uuid,
        base_url: String,
        progress_manager: Option<Arc<ProgressManager>>,
    ) -> Result<Self> {
        Ok(Self {
            db_pool,
            pipeline_file_manager,
            pipeline_execution_prefix,
            proxy_id,
            base_url,
            progress_manager,
        })
    }
    
    /// Helper method for reporting progress
    async fn report_progress(&self, percentage: f64, message: &str) {
        if let Some(pm) = &self.progress_manager {
            if let Some(updater) = pm.get_stage_updater("generation").await {
                updater.update_progress(percentage, message).await;
            }
        }
    }
    
    /// Set the progress manager for this stage
    pub fn set_progress_manager(&mut self, progress_manager: Arc<ProgressManager>) {
        self.progress_manager = Some(progress_manager);
    }
    
    /// Generate temporary M3U and XMLTV files for atomic publishing
    pub async fn process_channels_and_programs(
        &self,
        numbered_channels: Vec<NumberedChannel>,
        epg_programs: Vec<EpgProgram>,
        _cache_channel_logos: bool,
        _logo_service: &LogoAssetService,
    ) -> Result<Vec<PipelineArtifact>> {
        let process_start = Instant::now();
        
        info!(
            "Generation stage: proxy_id={} channels={} programs={} streaming_to_temp_files=true",
            self.proxy_id, numbered_channels.len(), epg_programs.len()
        );

        // Build M3U channel map (no EPG channel data needed)
        let channel_map_start = std::time::Instant::now();
        let channel_map = self.build_m3u_channel_map(&numbered_channels).await?;
        let channel_map_duration = channel_map_start.elapsed();
        debug!(
            "M3U channel map built: duration={} unique_channels={}",
            crate::utils::human_format::format_duration_precise(channel_map_duration),
            channel_map.len()
        );
        
        // Generate temporary M3U file
        let m3u_gen_start = std::time::Instant::now();
        let temp_m3u_file = format!("{}_temp.m3u8", self.pipeline_execution_prefix);
        let m3u_bytes = self.generate_m3u_streaming(&numbered_channels, &temp_m3u_file).await?;
        let m3u_gen_duration = m3u_gen_start.elapsed();
        info!(
            "M3U generation completed: duration={} file={} size={}KB channels_written={}",
            crate::utils::human_format::format_duration_precise(m3u_gen_duration),
            temp_m3u_file, m3u_bytes / 1024, numbered_channels.len()
        );
        
        // Generate temporary XMLTV file with M3U channel filtering
        let xmltv_gen_start = std::time::Instant::now();
        let temp_xmltv_file = format!("{}_temp.xmltv", self.pipeline_execution_prefix);
        let xmltv_bytes = self.generate_xmltv_streaming(&channel_map, &epg_programs, &temp_xmltv_file).await?;
        let xmltv_gen_duration = xmltv_gen_start.elapsed();
        info!(
            "XMLTV generation completed: duration={} file={} size={}KB filtered_by_m3u_channels=true",
            crate::utils::human_format::format_duration_precise(xmltv_gen_duration),
            temp_xmltv_file, xmltv_bytes / 1024
        );
        
        let total_duration = process_start.elapsed();
        
        // Create pipeline artifacts for publish_content stage
        let m3u_artifact = PipelineArtifact::new(
            ArtifactType::new(ContentType::M3uPlaylist, ProcessingStage::Generated),
            temp_m3u_file.clone(),
            "generation".to_string(),
        )
        .with_record_count(numbered_channels.len())
        .with_file_size(m3u_bytes)
        .with_metadata("proxy_id".to_string(), self.proxy_id.to_string().into())
        .with_metadata("target_filename".to_string(), format!("{}.m3u8", self.proxy_id).into());

        let xmltv_artifact = PipelineArtifact::new(
            ArtifactType::new(ContentType::XmltvGuide, ProcessingStage::Generated),
            temp_xmltv_file.clone(),
            "generation".to_string(),
        )
        .with_record_count(epg_programs.len())
        .with_file_size(xmltv_bytes)
        .with_metadata("proxy_id".to_string(), self.proxy_id.to_string().into())
        .with_metadata("target_filename".to_string(), format!("{}.xmltv", self.proxy_id).into());
        
        info!(
            "Generation stage completed: total_duration={} channel_map_duration={} m3u_duration={} xmltv_duration={} channels_processed={} programs_processed={} artifacts_created={} m3u_size={}KB xmltv_size={}KB",
            crate::utils::human_format::format_duration_precise(total_duration),
            crate::utils::human_format::format_duration_precise(channel_map_duration),
            crate::utils::human_format::format_duration_precise(m3u_gen_duration),
            crate::utils::human_format::format_duration_precise(xmltv_gen_duration),
            numbered_channels.len(),
            epg_programs.len(),
            2, // m3u + xmltv artifacts
            m3u_bytes / 1024,
            xmltv_bytes / 1024
        );

        // Explicit memory cleanup - force drop of large data structures
        drop(numbered_channels);
        drop(epg_programs);
        drop(channel_map);

        info!("Generation stage completed, memory cleanup performed");
        Ok(vec![m3u_artifact, xmltv_artifact])
    }

    /// Build M3U channel map (stream channels only - database-first approach)
    async fn build_m3u_channel_map(
        &self,
        numbered_channels: &[NumberedChannel],
    ) -> Result<HashMap<String, ChannelInfo>> {
        let build_start = Instant::now();
        let mut channel_map = HashMap::new();
        
        // Collect stream channel info from M3U (source of truth)
        for numbered_channel in numbered_channels {
            if let Some(ref tvg_id) = numbered_channel.channel.tvg_id {
                let entry = channel_map.entry(tvg_id.clone()).or_insert_with(|| ChannelInfo {
                    stream_display_names: BTreeSet::new(),
                    logo_url: None,
                });
                
                // Add stream display names (channel_name and tvg_name)
                entry.stream_display_names.insert(numbered_channel.channel.channel_name.clone());
                if let Some(ref tvg_name) = numbered_channel.channel.tvg_name {
                    if !tvg_name.is_empty() {
                        entry.stream_display_names.insert(tvg_name.clone());
                    }
                }
                
                // Set logo from first detected channel
                if entry.logo_url.is_none() {
                    entry.logo_url = numbered_channel.channel.tvg_logo.clone();
                }
            }
        }
        
        debug!(
            "M3U channel map built: stream_channels={} mapped_channels={} duration={} (database_first_mode=true)",
            numbered_channels.len(),
            channel_map.len(),
            crate::utils::human_format::format_duration_precise(build_start.elapsed())
        );
        
        Ok(channel_map)
    }
    
    /// Generate M3U content streaming to temporary file
    async fn generate_m3u_streaming(
        &self,
        numbered_channels: &[NumberedChannel],
        temp_file_path: &str,
    ) -> Result<u64> {
        let m3u_start = Instant::now();
        
        // Create file writer
        let file = self.pipeline_file_manager.create(temp_file_path).await
            .map_err(|e| anyhow::anyhow!("Failed to create temp M3U file: {}", e))?;
        let mut writer = tokio::io::BufWriter::new(file);
        
        // Write M3U header
        writer.write_all(b"#EXTM3U\n").await?;
        
        let mut bytes_written = 7u64; // "#EXTM3U\n"
        let mut channels_written = 0;
        
        for numbered_channel in numbered_channels {
            let channel = &numbered_channel.channel;
            // Build EXTINF line with conditional attributes
            let mut extinf_line = format!("#EXTINF:-1");
            
            // Add tvg-id if present
            if let Some(ref tvg_id) = channel.tvg_id {
                if !tvg_id.is_empty() {
                    extinf_line.push_str(&format!(" tvg-id=\"{}\"", tvg_id));
                }
            }
            
            // Add tvg-name if present
            if let Some(ref tvg_name) = channel.tvg_name {
                if !tvg_name.is_empty() {
                    extinf_line.push_str(&format!(" tvg-name=\"{}\"", tvg_name));
                }
            }
            
            // Add tvg-logo if present
            if let Some(ref tvg_logo) = channel.tvg_logo {
                if !tvg_logo.is_empty() {
                    extinf_line.push_str(&format!(" tvg-logo=\"{}\"", tvg_logo));
                }
            }
            
            // Add group-title if present
            if let Some(ref group_title) = channel.group_title {
                if !group_title.is_empty() {
                    extinf_line.push_str(&format!(" group-title=\"{}\"", group_title));
                }
            }
            
            // Add tvg-chno if present
            if let Some(ref tvg_chno) = channel.tvg_chno {
                if !tvg_chno.is_empty() {
                    extinf_line.push_str(&format!(" tvg-chno=\"{}\"", tvg_chno));
                }
            }
            
            // Add channel name and newline
            extinf_line.push_str(&format!(",{}\n", channel.channel_name));
            
            // Write EXTINF line
            writer.write_all(extinf_line.as_bytes()).await?;
            bytes_written += extinf_line.len() as u64;
            
            // Write proxy stream URL instead of original URL
            // This allows the proxy to capture metrics and implement relays
            let proxy_stream_url = format!(
                "{}/stream/{}/{}",
                self.base_url.trim_end_matches('/'),
                crate::utils::uuid_parser::uuid_to_base64(&self.proxy_id),
                crate::utils::uuid_parser::uuid_to_base64(&channel.id)
            );
            let stream_line = format!("{}\n", proxy_stream_url);
            writer.write_all(stream_line.as_bytes()).await?;
            bytes_written += stream_line.len() as u64;
            
            channels_written += 1;
        }
        
        writer.flush().await?;
        drop(writer);
        
        // Free M3U streaming memory
        debug!("Freed M3U streaming writer and buffers");
        
        info!(
            "M3U streaming completed: channels={} bytes={} duration={}",
            channels_written,
            bytes_written,
            crate::utils::human_format::format_duration_precise(m3u_start.elapsed())
        );
        
        Ok(bytes_written)
    }
    
    /// Generate XMLTV content using proper serialization to temporary file
    async fn generate_xmltv_streaming(
        &self,
        channel_map: &HashMap<String, ChannelInfo>,
        epg_programs: &[EpgProgram],
        temp_file_path: &str,
    ) -> Result<u64> {
        let xmltv_start = Instant::now();
        
        let mut programs_written = 0;
        let mut programs_filtered = 0;
        
        // For now, let's use the manual approach but with proper XML escaping until we fix the xmltv structures
        // TODO: Fix xmltv structure and use proper serialization
        
        // Create file writer
        let file = self.pipeline_file_manager.create(temp_file_path).await
            .map_err(|e| anyhow::anyhow!("Failed to create temp XMLTV file: {}", e))?;
        let mut writer = tokio::io::BufWriter::new(file);
        
        let mut bytes_written = 0u64;
        
        // Write XMLTV header
        let header = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE tv SYSTEM "xmltv.dtd">
<tv generator-info-name="m3u-proxy">
"#;
        writer.write_all(header.as_bytes()).await?;
        bytes_written += header.len() as u64;
        
        // Write channel definitions (only M3U channels - database-first approach)
        for (channel_id, channel_info) in channel_map {
            // Use stream display names (M3U channels are source of truth)
            let display_name = if !channel_info.stream_display_names.is_empty() {
                channel_info.stream_display_names.iter().next().unwrap().clone()
            } else {
                channel_id.clone()
            };
            
            let mut channel_line = format!("  <channel id=\"{}\">\n", quick_xml::escape::escape(channel_id));
            channel_line.push_str(&format!("    <display-name>{}</display-name>\n", quick_xml::escape::escape(&display_name)));
            
            // Add logo if present
            if let Some(ref logo_url) = channel_info.logo_url {
                if !logo_url.is_empty() {
                    channel_line.push_str(&format!("    <icon src=\"{}\"/>\n", quick_xml::escape::escape(logo_url)));
                }
            }
            
            channel_line.push_str("  </channel>\n");
            
            writer.write_all(channel_line.as_bytes()).await?;
            bytes_written += channel_line.len() as u64;
        }
        
        // Write program data (only for M3U channels that exist in channel_map)
        for program in epg_programs {
            // CRITICAL: Only include programs for channels that exist in M3U
            if !channel_map.contains_key(&program.channel_id) {
                programs_filtered += 1;
                continue;
            }
            
            let start_time = program.start_time.format("%Y%m%d%H%M%S %z");
            let stop_time = program.end_time.format("%Y%m%d%H%M%S %z");
            
            let mut program_line = format!(
                "  <programme start=\"{}\" stop=\"{}\" channel=\"{}\">\n",
                start_time, stop_time, quick_xml::escape::escape(&program.channel_id)
            );
            
            program_line.push_str(&format!("    <title>{}</title>\n", quick_xml::escape::escape(&program.title)));
            
            if let Some(ref description) = program.description {
                if !description.is_empty() {
                    program_line.push_str(&format!("    <desc>{}</desc>\n", quick_xml::escape::escape(description)));
                }
            }
            
            program_line.push_str("  </programme>\n");
            
            writer.write_all(program_line.as_bytes()).await?;
            bytes_written += program_line.len() as u64;
            programs_written += 1;
        }
        
        // Write XMLTV footer
        let footer = "</tv>\n";
        writer.write_all(footer.as_bytes()).await?;
        bytes_written += footer.len() as u64;
        
        writer.flush().await?;
        drop(writer);
        
        debug!(
            "Program filtering completed: total_programs={} programs_written={} programs_filtered_out={}",
            epg_programs.len(), programs_written, programs_filtered
        );
        
        // Free XMLTV streaming memory
        debug!("Freed XMLTV streaming writer and buffers");
        
        info!(
            "XMLTV streaming completed: channels={} programs={} programs_filtered={} bytes={} duration={} (database_first=true using_quick_xml_escape=true)",
            channel_map.len(),
            programs_written,
            programs_filtered,
            bytes_written,
            crate::utils::human_format::format_duration_precise(xmltv_start.elapsed())
        );
        
        Ok(bytes_written)
    }
    
    // Removed fetch_epg_display_names - no longer needed in database-first approach
    // EPG channel data is not stored, only programs are ingested

    

    
    /// Load artifacts from input for pipeline execution
    async fn load_artifacts_from_input(&self, input_artifacts: Vec<PipelineArtifact>) -> Result<(Vec<NumberedChannel>, Vec<EpgProgram>), PipelineError> {
        let mut numbered_channels = Vec::new();
        let mut epg_programs = Vec::new();
        
        for artifact in input_artifacts {
            match artifact.artifact_type.content {
                ContentType::Channels => {
                    // Read channels from JSONL file and convert to NumberedChannel
                    let content = self.pipeline_file_manager.read_to_string(&artifact.file_path).await
                        .map_err(|e| PipelineError::stage_error("generation", format!("Failed to read channels file {}: {}", artifact.file_path, e)))?;
                    
                    // Parse JSONL format (one JSON object per line)
                    for (line_num, line) in content.lines().enumerate() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        
                        match serde_json::from_str::<crate::models::Channel>(line) {
                            Ok(channel) => {
                                let numbered_channel = NumberedChannel {
                                    channel,
                                    assigned_number: 0, // Will be assigned by numbering stage
                                    assignment_type: ChannelNumberAssignmentType::Sequential,
                                };
                                numbered_channels.push(numbered_channel);
                            }
                            Err(e) => {
                                warn!("Failed to parse channel at line {}: {} - Error: {}", line_num + 1, line, e);
                            }
                        }
                    }
                }
                ContentType::EpgPrograms => {
                    // Read EPG programs from JSONL file
                    let content = self.pipeline_file_manager.read_to_string(&artifact.file_path).await
                        .map_err(|e| PipelineError::stage_error("generation", format!("Failed to read EPG programs file {}: {}", artifact.file_path, e)))?;
                    
                    for (line_num, line) in content.lines().enumerate() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        
                        match serde_json::from_str::<EpgProgram>(line) {
                            Ok(program) => epg_programs.push(program),
                            Err(e) => {
                                warn!("Failed to parse EPG program at line {}: {} - Error: {}", line_num + 1, line, e);
                            }
                        }
                    }
                }
                _ => {
                    debug!("Skipping artifact of type {:?} in generation stage", artifact.artifact_type.content);
                }
            }
        }
        
        info!(
            "Loaded artifacts: {} numbered channels, {} EPG programs",
            numbered_channels.len(),
            epg_programs.len()
        );
        
        Ok((numbered_channels, epg_programs))
    }

    /// Clean up temporary files and resources
    pub fn cleanup(self) -> Result<()> {
        // The SandboxedManager will clean up its temporary files when dropped
        debug!("Generation stage cleanup completed for execution: {}", self.pipeline_execution_prefix);
        Ok(())
    }
}

impl ProgressAware for GenerationStage {
    fn get_progress_manager(&self) -> Option<&Arc<ProgressManager>> {
        self.progress_manager.as_ref()
    }
}

#[async_trait::async_trait]
impl PipelineStage for GenerationStage {
    async fn execute(&mut self, input: Vec<PipelineArtifact>) -> Result<Vec<PipelineArtifact>, PipelineError> {
        info!("Generation stage starting with {} input artifacts", input.len());
        
        self.report_progress(10.0, "Loading generation data").await;
        
        // Load data from input artifacts
        let (numbered_channels, epg_programs) = self.load_artifacts_from_input(input).await?;
        
        self.report_progress(50.0, "Generating M3U and XMLTV files").await;
        
        // Generate the files (for now, create a dummy logo service)
        let logo_storage = crate::logo_assets::LogoAssetStorage::new(
            std::path::PathBuf::from("/tmp/logos_uploaded"),
            std::path::PathBuf::from("/tmp/logos_cached")
        );
        let logo_service = crate::logo_assets::service::LogoAssetService::new(
            self.db_pool.clone(),
            logo_storage
        );
        let artifacts = self.process_channels_and_programs(numbered_channels, epg_programs, false, &logo_service).await
            .map_err(|e| PipelineError::stage_error("generation", format!("Generation failed: {}", e)))?;
        
        self.report_progress(100.0, "Generation completed").await;
        
        Ok(artifacts)
    }
    
    fn stage_id(&self) -> &'static str {
        "generation"
    }
    
    fn stage_name(&self) -> &'static str {
        "Generation"
    }
    
    async fn cleanup(&mut self) -> Result<(), PipelineError> {
        Ok(())
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}