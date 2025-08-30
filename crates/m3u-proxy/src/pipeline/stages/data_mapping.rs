use crate::models::{Channel, data_mapping::DataMappingRule};
use crate::pipeline::models::{PipelineArtifact, ArtifactType};
use crate::pipeline::traits::{PipelineStage, ProgressAware};
use crate::pipeline::error::PipelineError;
use crate::pipeline::services::{HelperPostProcessor, HelperProcessorError};
use crate::services::progress_service::ProgressManager;
// Import helper traits implementation (this ensures the trait implementations are available)
#[allow(unused_imports)]
use crate::pipeline::services::helper_traits;
use tracing::{info, error, debug, trace, warn};
use crate::pipeline::engines::{DataMappingEngine, ChannelDataMappingEngine, StreamRuleProcessor, EpgProgram, EngineResult};
use crate::pipeline::engines::rule_processor::RegexEvaluator;
use crate::utils::human_format::format_duration_precise;
use crate::utils::regex_preprocessor::RegexPreprocessor;
use sandboxed_file_manager::SandboxedManager;
use serde_json;
use sea_orm::{EntityTrait, QueryFilter, ColumnTrait, QueryOrder, PaginatorTrait};
use crate::entities::{stream_sources, channels, data_mapping_rules, epg_programs, prelude::*};
use std::sync::Arc;
use std::time::Instant;
use std::collections::HashMap;

// Configurable batch sizes for memory optimization
const CHANNEL_PROCESSING_BATCH_SIZE: usize = 100;  // Process channels in batches for detailed logging
const EPG_PROGRAMS_BATCH_SIZE: usize = 1000;       // Process EPG programs in batches to avoid loading all into memory
const EPG_PROGRESS_LOG_INTERVAL: usize = 10000;    // Log EPG progress every N programs
const CHANNEL_PROGRESS_INTERVAL: usize = 1000;     // Report progress every N channels

pub struct DataMappingStage {
    db_connection: std::sync::Arc<sea_orm::DatabaseConnection>,
    file_manager: SandboxedManager,
    pipeline_execution_prefix: String,
    regex_preprocessor: RegexPreprocessor,
    helper_processor: Option<HelperPostProcessor>,
    progress_manager: Option<Arc<ProgressManager>>,
}

impl DataMappingStage {
    pub async fn new(
        db_connection: std::sync::Arc<sea_orm::DatabaseConnection>, 
        pipeline_execution_prefix: String, 
        shared_file_manager: SandboxedManager,
        progress_manager: Option<Arc<ProgressManager>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Use the shared file manager instead of creating our own
        // Initialize regex preprocessor for optimization
        let regex_preprocessor = RegexPreprocessor::new(Default::default());
        
        Ok(Self {
            db_connection,
            file_manager: shared_file_manager,
            pipeline_execution_prefix,
            regex_preprocessor,
            helper_processor: None,
            progress_manager,
        })
    }
    
    /// Set the progress manager for this stage (used when set after construction)
    pub fn set_progress_manager(&mut self, progress_manager: Arc<ProgressManager>) {
        self.progress_manager = Some(progress_manager);
    }
    
    /// Configure the helper post-processor for this stage
    pub fn with_helper_processor(mut self, helper_processor: HelperPostProcessor) -> Self {
        self.helper_processor = Some(helper_processor);
        self
    }
    
    pub async fn process_channels(&mut self) -> Result<PipelineArtifact, Box<dyn std::error::Error>> {
        let stage_start = Instant::now();
        info!("Starting channel processing in data mapping stage");
        
        // Query all stream sources using SeaORM
        let stream_sources = StreamSources::find()
            .filter(stream_sources::Column::IsActive.eq(true))
            .all(&*self.db_connection)
            .await?;
        
        info!("Found {} active stream sources", stream_sources.len());
        
        // Debug: Log all found stream sources
        for source in &stream_sources {
            debug!("Active stream source found: {} ({})", source.name, source.id);
        }
        
        let output_file_path = format!("{}_mapping_channels.jsonl", self.pipeline_execution_prefix);
        
        // Track overall statistics
        let mut total_channels_processed = 0;
        let mut total_channels_modified = 0;
        let mut rule_stats: HashMap<String, (String, usize, usize, std::time::Duration)> = HashMap::new(); // (rule_name, applied_count, processed_count, total_time)
        let mut output_file_created = false; // Track if output file has been created across all sources
        
        for source in stream_sources {
            let source_id = source.id;
            let source_name = source.name.clone();
            
            info!("Processing source: {} ({})", source_name, source_id);
            
            // Check if we have any rules for this source type using SeaORM
            info!("Querying data mapping rules for stream sources");
            let rule_models = match DataMappingRules::find()
                .filter(data_mapping_rules::Column::SourceType.eq("stream"))
                .filter(data_mapping_rules::Column::IsActive.eq(true))
                .order_by_asc(data_mapping_rules::Column::SortOrder)
                .all(&*self.db_connection)
                .await {
                    Ok(models) => {
                        info!("Successfully fetched {} data mapping rule models", models.len());
                        models
                    },
                    Err(e) => {
                        error!("Failed to fetch data mapping rules: {}", e);
                        return Err(e.into());
                    }
                };
            
            let mut rules = Vec::new();
            for rule_model in rule_models {
                info!("Processing data mapping rule id={} rule_name='{}' priority={}", rule_model.id, rule_model.name, rule_model.sort_order);
                
                let rule = match self.create_data_mapping_rule_from_model(&rule_model) {
                    Ok(rule) => rule,
                    Err(e) => {
                        error!("Failed to create DataMappingRule from model: {}", e);
                        return Err(anyhow::anyhow!("DataMappingRule parsing error: {}", e).into());
                    }
                };
                rules.push(rule);
            }
            
            info!("Found {} data mapping rules for stream sources", rules.len());
            
            // Process channels through data mapping rules with enhanced logging
            let source_start = Instant::now();
            info!("Processing channels for source: {}", source_id);
            debug!("Binding source_id as string: '{}'", source_id.to_string());
            
            // Set up output file for batch writes
            let output_file_path = format!("{}_mapping_channels.jsonl", self.pipeline_execution_prefix);
            let mut batch_content = String::new();
            // Use configurable batch size constants
            const BATCH_SIZE: usize = CHANNEL_PROCESSING_BATCH_SIZE;
            
            // Set up data mapping engine if we have rules
            let mut engine = if !rules.is_empty() {
                let mut engine = ChannelDataMappingEngine::new(source_id);
                
                // Build rule name mapping for statistics
                let mut rule_name_map: HashMap<String, String> = HashMap::new();
                
                // Add rule processors to the engine
                for rule in rules {
                    if let Some(expression) = rule.expression {
                        trace!("Adding rule processor: {} ({})", rule.name, rule.id);
                        let rule_id_str = rule.id.to_string();
                        rule_name_map.insert(rule_id_str.clone(), rule.name.clone());
                        
                        let regex_evaluator = RegexEvaluator::new(self.regex_preprocessor.clone());
                        let processor = StreamRuleProcessor::new(
                            rule_id_str,
                            rule.name.clone(),
                            expression,
                            regex_evaluator,
                        );
                        engine.add_rule_processor(processor);
                    }
                }
                
                Some((engine, rule_name_map))
            } else {
                None
            };
            
            // Get total channel count for this source for progress percentage calculation using SeaORM
            let total_channels = Channels::find()
                .filter(channels::Column::SourceId.eq(source_id.clone()))
                .count(&*self.db_connection)
                .await
                .unwrap_or(0);
            
            info!("Found {} channels for source {}", total_channels, source_name);
            
            // Get all channels for this source using SeaORM (we'll process in batches)
            let channel_models = Channels::find()
                .filter(channels::Column::SourceId.eq(source_id))
                .all(&*self.db_connection)
                .await?;
            
            let mut channel_count = 0;
            let mut source_modified_count = 0;
            let mut current_batch = Vec::new();
            
            for channel_model in channel_models {
                match self.create_channel_from_model(&channel_model) {
                    Ok(channel) => {
                        channel_count += 1;
                        current_batch.push(channel);
                                
                                
                                // Process batch when it reaches BATCH_SIZE
                                if current_batch.len() >= BATCH_SIZE {
                                    if !current_batch.is_empty() {
                                        let batch_result = if let Some((ref mut engine, ref rule_name_map)) = engine {
                                            // Process batch through engine
                                            let result = engine.process_records(std::mem::take(&mut current_batch))?;
                                            
                                            // Log detailed rule results
                                            self.log_rule_results(&result, channel_count);
                                            
                                            // Update statistics
                                            source_modified_count += result.total_modified;
                                            for (rule_id, results) in &result.rule_results {
                                                let applied_count = results.iter().filter(|r| r.rule_applied).count();
                                                let total_time: std::time::Duration = results.iter().map(|r| r.execution_time).sum();
                                                let rule_name = rule_name_map.get(rule_id).cloned().unwrap_or_else(|| rule_id.clone());
                                                
                                                let entry = rule_stats.entry(rule_id.clone()).or_insert((rule_name.clone(), 0, 0, std::time::Duration::ZERO));
                                                entry.1 += applied_count;
                                                entry.2 += results.len();
                                                entry.3 += total_time;
                                            }
                                            
                                            result
                                        } else {
                                            // No rules, return channels as-is
                                            let batch_data = std::mem::take(&mut current_batch);
                                            let batch_len = batch_data.len();
                                            EngineResult {
                                                processed_records: batch_data,
                                                total_processed: batch_len,
                                                total_modified: 0,
                                                rule_results: HashMap::new(),
                                                execution_time: std::time::Duration::ZERO,
                                            }
                                        };
                                        
                                        // Post-process channels with helper processor if configured
                                        let mut final_channels = Vec::new();
                                        let mut helper_modifications = 0;
                                        
                                        for processed_channel in batch_result.processed_records {
                                            if let Some(ref helper_processor) = self.helper_processor {
                                                if helper_processor.record_needs_processing(&processed_channel) {
                                                    let channel_name = processed_channel.channel_name.clone();
                                                    match helper_processor.process_record(processed_channel).await {
                                                        Ok((processed_record, modifications)) => {
                                                            if !modifications.is_empty() {
                                                                helper_modifications += 1;
                                                                trace!("Applied {} helper modifications to channel {}", 
                                                                    modifications.len(), processed_record.channel_name);
                                                            }
                                                            final_channels.push(processed_record);
                                                        }
                                                        Err(HelperProcessorError::CriticalDatabaseError(ref db_error)) => {
                                                            // Critical database error - halt the entire pipeline
                                                            error!("Critical database error during helper processing for channel {}: {}", 
                                                                channel_name, db_error);
                                                            error!("Halting pipeline execution due to critical database error");
                                                            return Err(format!("Critical database error in helper processing: {db_error}").into());
                                                        }
                                                        Err(e) => {
                                                            // Other errors - log and continue without this channel 
                                                            warn!("Helper processing failed for channel {}: {}", channel_name, e);
                                                            warn!("Skipping channel {} due to helper processing error", channel_name);
                                                        }
                                                    }
                                                } else {
                                                    final_channels.push(processed_channel);
                                                }
                                            } else {
                                                final_channels.push(processed_channel);
                                            }
                                        }
                                        
                                        // Update modified count if helpers made changes
                                        if helper_modifications > 0 {
                                            source_modified_count += helper_modifications;
                                            trace!("Helper processor applied {} modifications to batch", helper_modifications);
                                        }
                                        
                                        // Write processed channels to file
                                        for processed_channel in final_channels {
                                            let json_line = serde_json::to_string(&processed_channel)?;
                                            batch_content.push_str(&json_line);
                                            batch_content.push('\n');
                                        }
                                        
                                        // Write to file if we have content (using batch processing approach)
                                        if !batch_content.is_empty() {
                                            if !output_file_created {
                                                // First write - create file
                                                self.file_manager.write(&output_file_path, batch_content.as_bytes()).await?;
                                                output_file_created = true;
                                            } else {
                                                // Subsequent writes - append
                                                let existing_content = self.file_manager.read(&output_file_path).await?;
                                                let mut combined_content = String::from_utf8(existing_content)?;
                                                combined_content.push_str(&batch_content);
                                                self.file_manager.write(&output_file_path, combined_content.as_bytes()).await?;
                                            }
                                            batch_content.clear();
                                        }
                                    }
                                    
                                    // Update progress every CHANNEL_PROGRESS_INTERVAL channels
                                    if channel_count % CHANNEL_PROGRESS_INTERVAL == 0 {
                                        let completion_percentage = if total_channels > 0 {
                                            (channel_count as f64 / total_channels as f64 * 100.0).round() as u32
                                        } else {
                                            0
                                        };
                                        
                                        // Broadcast progress update via SSE
                                        let progress_message = format!("Processing {source_name}: {channel_count}/{total_channels} channels ({source_modified_count} modified)");
                                        self.report_progress(10.0 + (completion_percentage as f64 * 0.4), &progress_message).await;
                                    }
                                }
                    },
                    Err(e) => {
                        error!("Failed to create channel from model {}: {}", channel_count + 1, e);
                        return Err(e.into());
                    }
                }
            }
            
            // Process any remaining channels in the final batch
            if !current_batch.is_empty() {
                let batch_result = if let Some((ref mut engine, ref rule_name_map)) = engine {
                    let result = engine.process_records(current_batch)?;
                    self.log_rule_results(&result, channel_count);
                    source_modified_count += result.total_modified;
                    
                    // Update final statistics
                    for (rule_id, results) in &result.rule_results {
                        let applied_count = results.iter().filter(|r| r.rule_applied).count();
                        let total_time: std::time::Duration = results.iter().map(|r| r.execution_time).sum();
                        let rule_name = rule_name_map.get(rule_id).cloned().unwrap_or_else(|| rule_id.clone());
                        
                        let entry = rule_stats.entry(rule_id.clone()).or_insert((rule_name.clone(), 0, 0, std::time::Duration::ZERO));
                        entry.1 += applied_count;
                        entry.2 += results.len();
                        entry.3 += total_time;
                    }
                    
                    result
                } else {
                    let batch_len = current_batch.len();
                    EngineResult {
                        processed_records: current_batch,
                        total_processed: batch_len,
                        total_modified: 0,
                        rule_results: HashMap::new(),
                        execution_time: std::time::Duration::ZERO,
                    }
                };
                
                // Post-process final batch with helper processor if configured
                let mut final_channels = Vec::new();
                let mut helper_modifications = 0;
                
                for processed_channel in batch_result.processed_records {
                    if let Some(ref helper_processor) = self.helper_processor {
                        if helper_processor.record_needs_processing(&processed_channel) {
                            let channel_name = processed_channel.channel_name.clone();
                            match helper_processor.process_record(processed_channel).await {
                                Ok((processed_record, modifications)) => {
                                    if !modifications.is_empty() {
                                        helper_modifications += 1;
                                        trace!("Applied {} helper modifications to channel {}", 
                                            modifications.len(), processed_record.channel_name);
                                    }
                                    final_channels.push(processed_record);
                                }
                                Err(HelperProcessorError::CriticalDatabaseError(ref db_error)) => {
                                    // Critical database error - halt the entire pipeline
                                    error!("Critical database error during helper processing for channel {}: {}", 
                                        channel_name, db_error);
                                    error!("Halting pipeline execution due to critical database error");
                                    return Err(format!("Critical database error in helper processing: {db_error}").into());
                                }
                                Err(e) => {
                                    // Other errors - log and continue without this channel
                                    warn!("Helper processing failed for channel {}: {}", channel_name, e);
                                    warn!("Skipping channel {} due to helper processing error", channel_name);
                                }
                            }
                        } else {
                            final_channels.push(processed_channel);
                        }
                    } else {
                        final_channels.push(processed_channel);
                    }
                }
                
                // Update modified count if helpers made changes
                if helper_modifications > 0 {
                    source_modified_count += helper_modifications;
                    trace!("Helper processor applied {} modifications to final batch", helper_modifications);
                }
                
                // Write final batch
                for processed_channel in final_channels {
                    let json_line = serde_json::to_string(&processed_channel)?;
                    batch_content.push_str(&json_line);
                    batch_content.push('\n');
                }
                
                if !batch_content.is_empty() {
                    if !output_file_created {
                        // First write across all sources - create file
                        self.file_manager.write(&output_file_path, batch_content.as_bytes()).await?;
                        output_file_created = true;
                    } else {
                        // Subsequent writes - append to existing file
                        let existing_content = self.file_manager.read(&output_file_path).await?;
                        let mut combined_content = String::from_utf8(existing_content)?;
                        combined_content.push_str(&batch_content);
                        self.file_manager.write(&output_file_path, combined_content.as_bytes()).await?;
                    }
                }
            }
            
            let source_duration = source_start.elapsed();
            let final_completion_percentage = if total_channels > 0 {
                (channel_count as f64 / total_channels as f64 * 100.0).round() as u32
            } else {
                100
            };
            info!("Source processing completed: source={} total_channels={} modified_channels={} completion_percentage={:.0}% duration={}", 
                  source_name, channel_count, source_modified_count, final_completion_percentage, format_duration_precise(source_duration));
            
            // Update totals
            total_channels_processed += channel_count;
            total_channels_modified += source_modified_count;
            
            // Clean up engine if we created one
            if let Some((engine, _)) = engine {
                engine.destroy();
            }
            
            if channel_count == 0 {
                continue;
            }
        }
        
        // Log overall performance summary
        let stage_duration = stage_start.elapsed();
        let overall_completion_percentage = 100u32; // Stage completed fully
        info!("Data mapping stage completed: total_channels_processed={} total_channels_modified={} completion_percentage={:.0}% stage_duration={}", 
              total_channels_processed, total_channels_modified, overall_completion_percentage, format_duration_precise(stage_duration));
        
        // Log per-rule performance statistics
        if !rule_stats.is_empty() {
            info!("Rule performance summary:");
            for (rule_id, (rule_name, applied_count, processed_count, total_time)) in rule_stats {
                let avg_time = if processed_count > 0 { 
                    total_time / processed_count as u32 
                } else { 
                    std::time::Duration::ZERO 
                };
                let affected_percent = if processed_count > 0 { 
                    applied_count as f64 * 100.0 / processed_count as f64
                } else { 
                    0.0 
                };
                info!("  Rule performance: rule_id={} rule_name='{}' applied_count={} processed_count={} affected_percent={:.3}% avg_duration={}", 
                      rule_id, rule_name, applied_count, processed_count, affected_percent, format_duration_precise(avg_time));
            }
        }
        
        // Get the relative path for the temp file
        let full_path = self.file_manager.get_full_path(&output_file_path)?;
        info!("Pipeline temp file written: full_path={}", full_path.display());
        
        // Get file size
        let file_size_bytes = if let Ok(content) = self.file_manager.read(&output_file_path).await {
            Some(content.len() as u64)
        } else {
            None
        };
        
        // Create pipeline artifact for mapped channels
        let artifact = PipelineArtifact::new(
            ArtifactType::mapped_channels(),
            output_file_path,
            "data_mapping".to_string(),
        )
        .with_record_count(total_channels_processed)
        .with_metadata("total_modified".to_string(), serde_json::Value::Number(serde_json::Number::from(total_channels_modified)))
        .with_metadata("stage_duration_ms".to_string(), serde_json::Value::Number(serde_json::Number::from(stage_duration.as_millis() as u64)));
        
        let artifact = if let Some(size) = file_size_bytes {
            artifact.with_file_size(size)
        } else {
            artifact
        };
        
        // Memory cleanup is handled automatically when variables go out of scope
        
        info!("Data mapping channels stage completed, memory cleanup performed");
        Ok(artifact)
    }
    
    pub async fn process_programs(&mut self) -> Result<PipelineArtifact, Box<dyn std::error::Error>> {
        let stage_start = Instant::now();
        info!("Starting EPG programs processing in data mapping stage");
        
        let output_file_path = format!("{}_mapping_programs.jsonl", self.pipeline_execution_prefix);
        
        // Check if we have any EPG data mapping rules using SeaORM
        let epg_rules = DataMappingRules::find()
            .filter(data_mapping_rules::Column::SourceType.eq("epg"))
            .filter(data_mapping_rules::Column::IsActive.eq(true))
            .order_by_asc(data_mapping_rules::Column::SortOrder)
            .all(&*self.db_connection)
            .await?;
        
        let program_count = if epg_rules.is_empty() {
            info!("No EPG data mapping rules found, processing EPG programs for helpers only");
            
            // Serialize all EPG programs from database
            let programs = self.serialize_epg_programs_from_database().await?;
            let count = programs.len();
            
            // Process programs through helper processor if available
            let processed_programs = if let Some(ref helper_processor) = self.helper_processor {
                let mut processed = Vec::new();
                let mut helper_modifications = 0;
                
                for program in programs {
                    if helper_processor.record_needs_processing(&program) {
                        match helper_processor.process_record(program.clone()).await {
                            Ok((processed_program, modifications)) => {
                                if !modifications.is_empty() {
                                    helper_modifications += 1;
                                    trace!("Applied {} helper modifications to EPG program {}", 
                                        modifications.len(), processed_program.title);
                                }
                                processed.push(processed_program);
                            }
                            Err(e) => {
                                warn!("Helper processing failed for EPG program: {}", e);
                                processed.push(program); // Keep original on error
                            }
                        }
                    } else {
                        processed.push(program);
                    }
                }
                
                if helper_modifications > 0 {
                    info!("Applied helper processing to {} EPG programs", helper_modifications);
                }
                
                processed
            } else {
                programs
            };
            
            // Write processed programs to file
            self.write_programs_to_file(&processed_programs, &output_file_path).await?;
            
            count
        } else {
            info!("Found {} EPG data mapping rules, implementing rule processing...", epg_rules.len());
            
            // TODO: Implement actual EPG data mapping rules processing
            // For now, serialize with helper processing until EPG rule engine is ready
            warn!("EPG data mapping rules found but processing not yet implemented, applying helper processing only");
            
            let programs = self.serialize_epg_programs_from_database().await?;
            let count = programs.len();
            
            // Process programs through helper processor if available
            let processed_programs = if let Some(ref helper_processor) = self.helper_processor {
                let mut processed = Vec::new();
                let mut helper_modifications = 0;
                
                for program in programs {
                    if helper_processor.record_needs_processing(&program) {
                        match helper_processor.process_record(program.clone()).await {
                            Ok((processed_program, modifications)) => {
                                if !modifications.is_empty() {
                                    helper_modifications += 1;
                                    trace!("Applied {} helper modifications to EPG program {}", 
                                        modifications.len(), processed_program.title);
                                }
                                processed.push(processed_program);
                            }
                            Err(e) => {
                                warn!("Helper processing failed for EPG program: {}", e);
                                processed.push(program); // Keep original on error
                            }
                        }
                    } else {
                        processed.push(program);
                    }
                }
                
                if helper_modifications > 0 {
                    info!("Applied helper processing to {} EPG programs", helper_modifications);
                }
                
                processed
            } else {
                programs
            };
            
            // Write processed programs to file
            self.write_programs_to_file(&processed_programs, &output_file_path).await?;
            
            count
        };
        
        let stage_duration = stage_start.elapsed();
        info!("EPG programs processing completed: total_programs={} stage_duration={}", 
              program_count, format_duration_precise(stage_duration));
        
        // Get file size
        let file_size_bytes = if let Ok(content) = self.file_manager.read(&output_file_path).await {
            Some(content.len() as u64)
        } else {
            None
        };
        
        // Get the relative path for the temp file
        let full_path = self.file_manager.get_full_path(&output_file_path)?;
        info!("Pipeline temp file written: full_path={}", full_path.display());
        
        // Create pipeline artifact for mapped EPG programs
        let artifact = PipelineArtifact::new(
            ArtifactType::mapped_epg(),
            output_file_path,
            "data_mapping".to_string(),
        )
        .with_record_count(program_count)
        .with_metadata("stage_duration_ms".to_string(), serde_json::Value::Number(serde_json::Number::from(stage_duration.as_millis() as u64)))
        .with_metadata("epg_rules_found".to_string(), serde_json::Value::Number(serde_json::Number::from(epg_rules.len())));
        
        let artifact = if let Some(size) = file_size_bytes {
            artifact.with_file_size(size)
        } else {
            artifact
        };
        
        // Memory cleanup is handled automatically when variables go out of scope
        
        info!("Data mapping programs stage completed, memory cleanup performed");
        Ok(artifact)
    }
    
    async fn serialize_epg_programs_from_database(&self) -> Result<Vec<EpgProgram>, Box<dyn std::error::Error>> {
        info!("Serializing EPG programs from database using SeaORM approach");
        
        // Get all EPG programs using SeaORM (ordered by start_time)
        let epg_models = EpgPrograms::find()
            .order_by_asc(epg_programs::Column::StartTime)
            .all(&*self.db_connection)
            .await?;
        
        let mut all_programs = Vec::new();
        let mut batch_programs = Vec::new();
        const BATCH_SIZE: usize = EPG_PROGRAMS_BATCH_SIZE;
        let mut processed_count = 0;
        
        for epg_model in epg_models {
            let program = self.create_epg_program_from_model(&epg_model)?;
            batch_programs.push(program);
            processed_count += 1;
            
            // Process batch when full
            if batch_programs.len() >= BATCH_SIZE {
                // Move batch to final collection and clear batch memory
                all_programs.append(&mut batch_programs);
                
                // Log progress periodically
                if processed_count % EPG_PROGRESS_LOG_INTERVAL == 0 {
                    debug!("Processed {} EPG programs so far", processed_count);
                    
                    // Calculate progress in 50-95% range based on processed count
                    // Note: We don't know total count in advance, so use a reasonable estimate
                    // For progress reporting, assume we're making steady progress through the range
                    let estimated_progress = if processed_count < 10000 {
                        50.0 + (processed_count as f64 / 10000.0) * 20.0  // 50-70% for first 10k
                    } else if processed_count < 50000 {
                        70.0 + ((processed_count - 10000) as f64 / 40000.0) * 15.0  // 70-85% for 10k-50k
                    } else {
                        85.0 + ((processed_count - 50000) as f64 / 50000.0) * 10.0  // 85-95% for 50k+
                    }.min(95.0);
                    
                    let progress_message = format!("Processing EPG programs: {processed_count} processed");
                    self.report_progress(estimated_progress, &progress_message).await;
                }
            }
        }
        
        // Process remaining programs in final batch
        if !batch_programs.is_empty() {
            all_programs.append(&mut batch_programs);
        }
        
        info!("Serialized {} EPG programs from database using batch processing approach", all_programs.len());
        Ok(all_programs)
    }
    
    async fn write_programs_to_file(&self, programs: &[EpgProgram], file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut content = String::new();
        for program in programs {
            let json_line = serde_json::to_string(program)?;
            content.push_str(&json_line);
            content.push('\n');
        }
        
        self.file_manager.write(file_path, content.as_bytes()).await?;
        Ok(())
    }

    /// Create Channel from SeaORM model
    fn create_channel_from_model(&self, model: &crate::entities::channels::Model) -> Result<Channel, anyhow::Error> {
        
        let channel = Channel {
            id: model.id,
            source_id: model.source_id,
            tvg_id: model.tvg_id.clone(),
            tvg_name: model.tvg_name.clone(),
            tvg_chno: model.tvg_chno.clone(),
            tvg_logo: model.tvg_logo.clone(),
            tvg_shift: model.tvg_shift.clone(),
            group_title: model.group_title.clone(),
            channel_name: model.channel_name.clone(),
            stream_url: model.stream_url.clone(),
            created_at: model.created_at,
            updated_at: model.updated_at,
            video_codec: None,
            audio_codec: None,
            resolution: None,
            probe_method: None,
            last_probed_at: None,
        };
        
        
        Ok(channel)
    }

    /// Create DataMappingRule from SeaORM model
    fn create_data_mapping_rule_from_model(&self, model: &crate::entities::data_mapping_rules::Model) -> Result<DataMappingRule, anyhow::Error> {
        let rule = DataMappingRule {
            id: model.id,
            name: model.name.clone(),
            description: model.description.clone(),
            source_type: model.source_type.clone(),
            sort_order: model.sort_order,
            is_active: model.is_active,
            expression: model.expression.clone(),
            created_at: model.created_at,
            updated_at: model.updated_at,
        };
        Ok(rule)
    }

    /// Create EpgProgram from SeaORM model
    fn create_epg_program_from_model(&self, model: &crate::entities::epg_programs::Model) -> Result<EpgProgram, anyhow::Error> {
        let program = EpgProgram {
            id: model.id.to_string(),
            channel_id: model.channel_id.clone(),
            title: model.program_title.clone(),
            description: model.program_description.clone(),
            program_icon: model.program_icon.clone(),
            start_time: model.start_time,
            end_time: model.end_time,
        };
        Ok(program)
    }

    fn log_rule_results<T>(&self, result: &EngineResult<T>, processed_count: usize) {
        if !result.rule_results.is_empty() {
            trace!("Engine batch result: batch_end_channel={} batch_processed={} batch_modified={} batch_duration={}", 
                   processed_count, result.total_processed, result.total_modified, format_duration_precise(result.execution_time));
            
            for (rule_id, rule_results) in &result.rule_results {
                let applied_count = rule_results.iter().filter(|r| r.rule_applied).count();
                if applied_count > 0 {
                    trace!("Rule batch result: rule_id={} applied_count={} batch_records={}", rule_id, applied_count, rule_results.len());
                    
                    // Log individual field modifications at trace level
                    for (i, rule_result) in rule_results.iter().enumerate() {
                        if rule_result.rule_applied && !rule_result.field_modifications.is_empty() {
                            trace!("  Record modification: record_index={} field_count={} duration={}", 
                                   i + 1, rule_result.field_modifications.len(), format_duration_precise(rule_result.execution_time));
                            for modification in &rule_result.field_modifications {
                                trace!("    Field change: field={} action={} old_value={:?} new_value={:?}", 
                                       modification.field_name, 
                                       format!("{:?}", modification.modification_type).to_lowercase(),
                                       modification.old_value, 
                                       modification.new_value);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn cleanup(self) -> Result<(), Box<dyn std::error::Error>> {
        // The file manager will cleanup temp files when dropped
        Ok(())
    }
    
    /// Helper method for reporting progress
    async fn report_progress(&self, percentage: f64, message: &str) {
        if let Some(pm) = &self.progress_manager {
            if let Some(updater) = pm.get_stage_updater("data_mapping").await {
                debug!("Data mapping progress: {:.0}% - {}", percentage, message);
                updater.update_progress(percentage, message).await;
            } else {
                debug!("No stage updater found for 'data_mapping'");
            }
        } else {
            debug!("No progress manager set on DataMappingStage");
        }
    }
}

impl ProgressAware for DataMappingStage {
    fn get_progress_manager(&self) -> Option<&Arc<ProgressManager>> {
        self.progress_manager.as_ref()
    }
}

#[async_trait::async_trait]
impl PipelineStage for DataMappingStage {
    async fn execute(&mut self, _input: Vec<PipelineArtifact>) -> Result<Vec<PipelineArtifact>, PipelineError> {
        self.report_progress(5.0, "Starting data mapping").await;
        
        let mut artifacts = Vec::new();
        
        // Process channels
        self.report_progress(10.0, "Processing channels").await;
        let channel_artifact = self.process_channels().await
            .map_err(|e| PipelineError::stage_error("data_mapping", format!("Channel processing failed: {e}")))?;
        artifacts.push(channel_artifact);
        
        // Process EPG programs
        self.report_progress(50.0, "Processing EPG programs").await;
        let program_artifact = self.process_programs().await
            .map_err(|e| PipelineError::stage_error("data_mapping", format!("EPG processing failed: {e}")))?;
        artifacts.push(program_artifact);
        
        self.report_progress(100.0, "Data mapping completed").await;
        Ok(artifacts)
    }
    
    fn stage_id(&self) -> &'static str {
        "data_mapping"
    }
    
    fn stage_name(&self) -> &'static str {
        "Data Mapping"
    }
    
    async fn cleanup(&mut self) -> Result<(), PipelineError> {
        Ok(())
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}