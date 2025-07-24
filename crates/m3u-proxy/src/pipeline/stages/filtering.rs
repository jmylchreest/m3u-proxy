//! Filtering stage for pipeline processing
//!
//! This module provides filtering capabilities for channel and EPG data using
//! configurable filter rules with extensible design and time function support.

use crate::pipeline::engines::{
    ChannelFilteringEngine, StreamFilterProcessor,
    RegexEvaluator, FilterEngineResult
};
use crate::pipeline::models::{PipelineArtifact, ArtifactType, ContentType};
use crate::models::{Channel, FilterSourceType};
use crate::utils::regex_preprocessor::{RegexPreprocessor, RegexPreprocessorConfig};
use sandboxed_file_manager::SandboxedManager;
use sqlx::SqlitePool;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{info, warn, trace};

#[derive(Debug, Serialize, Deserialize)]
struct FilterRule {
    id: String,
    name: String,
    source_type: FilterSourceType,
    condition_tree: String,
    is_inverse: bool,
    is_system_default: bool,
    priority_order: i32,
}

pub struct FilteringStage {
    db_pool: SqlitePool,
    file_manager: SandboxedManager,
    #[allow(dead_code)]
    pipeline_execution_prefix: String,
    regex_preprocessor: RegexPreprocessor,
    proxy_id: Option<uuid::Uuid>,
}

impl FilteringStage {
    pub async fn new(db_pool: SqlitePool, file_manager: SandboxedManager, pipeline_execution_prefix: String, proxy_id: Option<uuid::Uuid>) -> Result<Self, Box<dyn std::error::Error>> {
        let regex_preprocessor = RegexPreprocessor::new(RegexPreprocessorConfig::default());
        
        Ok(Self {
            db_pool,
            file_manager,
            pipeline_execution_prefix,
            regex_preprocessor,
            proxy_id,
        })
    }
    
    pub async fn process(&mut self, input_artifacts: Vec<PipelineArtifact>) -> Result<Vec<PipelineArtifact>, Box<dyn std::error::Error>> {
        let stage_start = Instant::now();
        info!("Starting filtering stage input_artifacts={}", input_artifacts.len());
        
        let mut output_artifacts = Vec::new();
        let mut stage_filter_stats = std::collections::HashMap::new(); // filter_id -> (filter_name, included_count, excluded_count, total_time)
        let mut total_input_records = 0;
        let mut total_output_records = 0;
        
        for artifact in input_artifacts {
            match artifact.artifact_type.content {
                ContentType::Channels => {
                    let (filtered_artifact, filter_stats, input_count, output_count) = self.process_channel_artifact(artifact).await?;
                    output_artifacts.push(filtered_artifact);
                    
                    // Aggregate filter statistics
                    total_input_records += input_count;
                    total_output_records += output_count;
                    for (filter_id, (filter_name, included, excluded, duration, priority)) in filter_stats {
                        let entry = stage_filter_stats.entry(filter_id).or_insert((filter_name.clone(), 0, 0, std::time::Duration::ZERO, priority));
                        entry.1 += included;
                        entry.2 += excluded;
                        entry.3 += duration;
                    }
                }
                ContentType::EpgPrograms => {
                    let filtered_artifact = self.process_epg_artifact(artifact).await?;
                    output_artifacts.push(filtered_artifact);
                }
                _ => {
                    // Pass through other content types unchanged
                    output_artifacts.push(artifact);
                }
            }
        }
        
        let stage_duration = stage_start.elapsed();
        info!("Completed filtering stage duration={} input_records={} output_records={} output_artifacts={}", 
             crate::utils::human_format::format_duration_precise(stage_duration), 
             total_input_records, total_output_records, output_artifacts.len());
        
        // Log filter performance summary similar to data mapping
        if !stage_filter_stats.is_empty() {
            info!("Filter performance summary:");
            for (filter_id, (filter_name, included_count, excluded_count, total_time, priority)) in stage_filter_stats {
                let total_processed = included_count + excluded_count;
                let avg_time = if total_processed > 0 { 
                    total_time / total_processed as u32 
                } else { 
                    std::time::Duration::ZERO 
                };
                let included_percent = if total_processed > 0 { 
                    included_count as f64 * 100.0 / total_processed as f64
                } else { 
                    0.0 
                };
                let excluded_percent = if total_processed > 0 { 
                    excluded_count as f64 * 100.0 / total_processed as f64
                } else { 
                    0.0 
                };
                info!("  Filter performance: filter_id={} filter_name={} priority={} included_count={} excluded_count={} included_percent={:.3}% excluded_percent={:.3}% avg_duration={}", 
                      filter_id, filter_name, priority, included_count, excluded_count, included_percent, excluded_percent, crate::utils::human_format::format_duration_precise(avg_time));
            }
        }
        
        Ok(output_artifacts)
    }
    
    async fn process_channel_artifact(&mut self, artifact: PipelineArtifact) -> Result<(PipelineArtifact, std::collections::HashMap<String, (String, usize, usize, std::time::Duration, i32)>, usize, usize), Box<dyn std::error::Error>> {
        let process_start = Instant::now();
        info!("Processing channel artifact file_path={}", artifact.file_path);
        
        // Load channel filter rules from database
        let filter_rules = self.load_filter_rules(FilterSourceType::Stream).await?;
        info!("Loaded channel filter rules count={} for proxy_id={:?}", filter_rules.len(), self.proxy_id);
        
        if filter_rules.is_empty() {
            info!("No channel filter rules found, passing through unchanged");
            let channels = self.read_channels_from_artifact(&artifact).await?;
            let input_count = channels.len();
            info!("Passthrough: read {} channels, creating filtered artifact", input_count);
            
            // Create filtered artifact even when no filters are applied
            let filtered_file_path = artifact.file_path
                .replace("_mapping_channels.jsonl", "_filtered_channels.jsonl");
            
            // Write channels to filtered file
            let output_artifact = self.write_channels_to_artifact(channels, &filtered_file_path).await?;
            
            info!("Pipeline temp file written full_path={}", 
                 self.file_manager.get_full_path(&filtered_file_path).map(|p| p.display().to_string()).unwrap_or_else(|_| filtered_file_path.clone()));
            
            return Ok((output_artifact, std::collections::HashMap::new(), input_count, input_count));
        }
        
        info!("Loaded channel filter rules count={}", filter_rules.len());
        
        // Read channels from input artifact
        let channels = self.read_channels_from_artifact(&artifact).await?;
        info!("Read channels from input artifact count={}", channels.len());
        
        // Create filtering engine and add processors
        let mut filtering_engine = ChannelFilteringEngine::new();
        let mut filter_name_map = std::collections::HashMap::new();
        let mut filter_priority_map = std::collections::HashMap::new();
        for rule in &filter_rules {
            filter_name_map.insert(rule.id.clone(), rule.name.clone());
            filter_priority_map.insert(rule.id.clone(), rule.priority_order);
            let regex_evaluator = RegexEvaluator::new(self.regex_preprocessor.clone());
            let processor = StreamFilterProcessor::new(
                rule.id.clone(),
                rule.name.clone(),
                rule.is_inverse,
                &rule.condition_tree,
                regex_evaluator,
            )?;
            filtering_engine.add_filter_processor(Box::new(processor));
        }
        
        // Process channels through filtering engine
        let filter_result = filtering_engine.process_records(&channels)?;
        
        // Log filtering results
        self.log_filtering_results(&filter_result, "channels");
        
        // Write filtered channels to new artifact
        let filtered_file_path = artifact.file_path
            .replace("_mapping_channels.jsonl", "_filtered_channels.jsonl");
        let output_artifact = self.write_channels_to_artifact(
            filter_result.filtered_records,
            &filtered_file_path
        ).await?;
        
        info!("Completed channel filtering duration={} input_channels={} output_channels={}", 
             crate::utils::human_format::format_duration_precise(process_start.elapsed()), 
             filter_result.total_input, 
             filter_result.total_filtered);
        
        info!("Pipeline temp file written full_path={}", 
             self.file_manager.get_full_path(&filtered_file_path).map(|p| p.display().to_string()).unwrap_or_else(|_| filtered_file_path.clone()));
        
        // Convert filter stats to include filter names and priorities
        let mut filter_stats_with_names = std::collections::HashMap::new();
        for (filter_id, (included, excluded, duration)) in filter_result.filter_stats {
            let filter_name = filter_name_map.get(&filter_id).cloned().unwrap_or_else(|| filter_id.clone());
            let priority = filter_priority_map.get(&filter_id).copied().unwrap_or(999);
            filter_stats_with_names.insert(filter_id, (filter_name, included, excluded, duration, priority));
        }
        
        Ok((output_artifact, filter_stats_with_names, filter_result.total_input, filter_result.total_filtered))
    }
    
    async fn process_epg_artifact(&mut self, artifact: PipelineArtifact) -> Result<PipelineArtifact, Box<dyn std::error::Error>> {
        let process_start = Instant::now();
        info!("Processing EPG artifact file_path={}", artifact.file_path);
        
        // Load EPG filter rules from database
        let filter_rules = self.load_filter_rules(FilterSourceType::Epg).await?;
        if filter_rules.is_empty() {
            info!("No EPG filter rules found, passing through unchanged");
            
            // Simply rename the file from mapping to filtered stage
            let filtered_file_path = artifact.file_path
                .replace("_mapping_programs.jsonl", "_filtered_programs.jsonl");
            
            // Copy content to new file path
            let content = self.file_manager.read(&artifact.file_path).await?;
            self.file_manager.write(&filtered_file_path, &content).await?;
            
            // Create new artifact with updated path and metadata
            let output_artifact = PipelineArtifact::new(
                ArtifactType::filtered_epg(),
                filtered_file_path.clone(),
                "filtering".to_string(),
            )
            .with_record_count(artifact.record_count.unwrap_or(0))
            .with_file_size(content.len() as u64);
            
            info!("Completed EPG filtering duration={} mode=passthrough input_programs={} output_programs={}", 
                 crate::utils::human_format::format_duration_precise(process_start.elapsed()),
                 artifact.record_count.unwrap_or(0),
                 artifact.record_count.unwrap_or(0));
            
            info!("Pipeline temp file written full_path={}", 
                 self.file_manager.get_full_path(&filtered_file_path).map(|p| p.display().to_string()).unwrap_or_else(|_| filtered_file_path.clone()));
            
            return Ok(output_artifact);
        }
        
        info!("Loaded EPG filter rules count={}", filter_rules.len());
        
        // For now, apply deduplication and pass through EPG artifacts since EpgFilterProcessor is a placeholder
        // TODO: Implement actual EPG filtering when EpgFilterProcessor is complete
        warn!("EPG filtering not yet implemented, applying deduplication and passing through");
        
        // Read EPG programs from input artifact with deduplication
        let programs = self.read_epg_programs_from_artifact(&artifact).await?;
        info!("Read EPG programs from input artifact count={}", programs.len());
        let programs_count = programs.len();
        
        // Update file path to include stage name
        let filtered_file_path = artifact.file_path
            .replace("_mapping_programs.jsonl", "_filtered_programs.jsonl");
        
        // Write deduplicated programs to new artifact
        let output_artifact = self.write_epg_programs_to_artifact(programs, &filtered_file_path).await?;
        
        info!("Completed EPG filtering duration={} mode=dedup-passthrough input_programs={} output_programs={}", 
             crate::utils::human_format::format_duration_precise(process_start.elapsed()),
             artifact.record_count.unwrap_or(0), 
             programs_count);
        
        info!("Pipeline temp file written full_path={}", 
             self.file_manager.get_full_path(&filtered_file_path).map(|p| p.display().to_string()).unwrap_or_else(|_| filtered_file_path.clone()));
        
        // Memory cleanup is handled automatically when variables go out of scope
        
        info!("EPG filtering stage completed, memory cleanup performed");
        Ok(output_artifact)
    }
    
    async fn load_filter_rules(&self, source_type: FilterSourceType) -> Result<Vec<FilterRule>, Box<dyn std::error::Error>> {
        if let Some(proxy_id) = self.proxy_id {
            // Load proxy-specific filters with priority ordering
            let query = r#"
                SELECT pf.priority_order, f.id, f.name, f.source_type, f.condition_tree, f.is_inverse, f.is_system_default
                FROM proxy_filters pf
                JOIN filters f ON pf.filter_id = f.id
                WHERE pf.proxy_id = ? AND pf.is_active = 1 AND f.source_type = ?
                ORDER BY pf.priority_order ASC
            "#;
            
            let source_type_str = match source_type {
                FilterSourceType::Stream => "stream",
                FilterSourceType::Epg => "epg",
            };
            
            let rows = sqlx::query_as::<_, (i32, String, String, String, String, bool, bool)>(query)
                .bind(proxy_id.to_string())
                .bind(source_type_str)
                .fetch_all(&self.db_pool)
                .await?;
            
            let mut rules = Vec::new();
            for (priority_order, id, name, _source_type, condition_tree, is_inverse, is_system_default) in rows {
                rules.push(FilterRule {
                    id,
                    name,
                    source_type: source_type.clone(),
                    condition_tree,
                    is_inverse,
                    is_system_default,
                    priority_order,
                });
            }
            
            Ok(rules)
        } else {
            // No proxy_id provided - return empty list
            warn!("No proxy_id provided to FilteringStage, returning empty filter rules");
            Ok(Vec::new())
        }
    }
    
    async fn read_channels_from_artifact(&self, artifact: &PipelineArtifact) -> Result<Vec<Channel>, Box<dyn std::error::Error>> {
        let content = self.file_manager.read_to_string(&artifact.file_path).await?;
        
        let mut channels = Vec::new();
        let mut seen_stream_urls = std::collections::HashSet::new();
        let mut total_channels_read = 0;
        let mut deduplicated_channels = 0;
        
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let channel: Channel = serde_json::from_str(line)?;
            total_channels_read += 1;
            
            // Track deduplication by stream_url
            if seen_stream_urls.insert(channel.stream_url.clone()) {
                // New unique stream URL
                channels.push(channel);
            } else {
                // Duplicate stream URL found
                deduplicated_channels += 1;
                trace!("Duplicate stream_url found: {}", channel.stream_url);
            }
        }
        
        if deduplicated_channels > 0 {
            info!("Input deduplication: total_channels_read={} unique_channels={} deduplicated_channels={}", 
                  total_channels_read, channels.len(), deduplicated_channels);
        } else {
            info!("Input channels: total_channels_read={} unique_channels={} deduplicated_channels=0", 
                  total_channels_read, channels.len());
        }
        
        Ok(channels)
    }
    
    async fn read_epg_programs_from_artifact(&self, artifact: &PipelineArtifact) -> Result<Vec<crate::pipeline::engines::rule_processor::EpgProgram>, Box<dyn std::error::Error>> {
        let content = self.file_manager.read_to_string(&artifact.file_path).await?;
        
        let mut programs = Vec::new();
        let mut seen_program_keys = std::collections::HashSet::new();
        let mut total_programs_read = 0;
        let mut deduplicated_programs = 0;
        
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let program: crate::pipeline::engines::rule_processor::EpgProgram = serde_json::from_str(line)?;
            total_programs_read += 1;
            
            // Create composite key for deduplication: channel_id + title + start_time
            let dedup_key = format!("{}|{}|{}", program.channel_id, program.title, program.start_time.timestamp());
            
            if seen_program_keys.insert(dedup_key.clone()) {
                // New unique program
                programs.push(program);
            } else {
                // Duplicate program found
                deduplicated_programs += 1;
                trace!("Duplicate EPG program found: channel_id={} title={} start_time={}", 
                       program.channel_id, program.title, program.start_time);
            }
        }
        
        if deduplicated_programs > 0 {
            info!("Input EPG deduplication: total_programs_read={} unique_programs={} deduplicated_programs={}", 
                  total_programs_read, programs.len(), deduplicated_programs);
        } else {
            info!("Input EPG programs: total_programs_read={} unique_programs={} deduplicated_programs=0", 
                  total_programs_read, programs.len());
        }
        
        Ok(programs)
    }
    
    async fn write_channels_to_artifact(&mut self, channels: Vec<Channel>, file_path: &str) -> Result<PipelineArtifact, Box<dyn std::error::Error>> {
        // Write channels as JSONL
        let mut content = String::new();
        for channel in &channels {
            content.push_str(&serde_json::to_string(channel)?);
            content.push('\n');
        }
        
        self.file_manager.write(file_path, content.as_bytes()).await?;
        
        Ok(PipelineArtifact::new(
            ArtifactType::filtered_channels(),
            file_path.to_string(),
            "filtering".to_string(),
        )
        .with_record_count(channels.len())
        .with_file_size(content.len() as u64))
    }
    
    async fn write_epg_programs_to_artifact(&mut self, programs: Vec<crate::pipeline::engines::rule_processor::EpgProgram>, file_path: &str) -> Result<PipelineArtifact, Box<dyn std::error::Error>> {
        // Write EPG programs as JSONL
        let mut content = String::new();
        for program in &programs {
            content.push_str(&serde_json::to_string(program)?);
            content.push('\n');
        }
        
        self.file_manager.write(file_path, content.as_bytes()).await?;
        
        Ok(PipelineArtifact::new(
            ArtifactType::filtered_epg(),
            file_path.to_string(),
            "filtering".to_string(),
        )
        .with_record_count(programs.len())
        .with_file_size(content.len() as u64))
    }
    
    fn log_filtering_results<T>(&self, result: &FilterEngineResult<T>, content_type: &str) {
        info!("{} filtering results input_records={} output_records={} duration={}", 
             content_type, result.total_input, result.total_filtered, 
             crate::utils::human_format::format_duration_precise(result.execution_time));
        
        for (filter_id, (included_count, excluded_count, filter_time)) in &result.filter_stats {
            trace!("Filter filter_id={} included={} excluded={} duration={}", 
                  filter_id, included_count, excluded_count, 
                  crate::utils::human_format::format_duration_precise(*filter_time));
        }
    }
    
    pub fn cleanup(self) -> Result<(), Box<dyn std::error::Error>> {
        // Clear any cached state
        trace!("Cleaning up filtering stage");
        Ok(())
    }
}