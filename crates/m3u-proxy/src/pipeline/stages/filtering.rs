//! Filtering stage for pipeline processing
//!
//! This module provides filtering capabilities for channel and EPG data using
//! configurable filter rules with extensible design and time function support.

use crate::database::repositories::filter::FilterSeaOrmRepository;
use crate::database::repositories::stream_proxy::StreamProxySeaOrmRepository;
use crate::models::{Channel, FilterSourceType};
use crate::pipeline::engines::{
    ChannelFilteringEngine, EpgFilterProcessor, FilterEngineResult, FilteringEngine,
    RegexEvaluator, StreamFilterProcessor,
};
use crate::pipeline::error::PipelineError;
use crate::pipeline::models::{ArtifactType, ContentType, PipelineArtifact};
use crate::pipeline::traits::{PipelineStage, ProgressAware};
use crate::services::progress_service::ProgressManager;
use crate::utils::regex_preprocessor::{RegexPreprocessor, RegexPreprocessorConfig};
use sandboxed_file_manager::SandboxedManager;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, trace, warn};

#[derive(Debug, Serialize, Deserialize)]
struct FilterRule {
    id: String,
    name: String,
    source_type: FilterSourceType,
    expression: String,
    is_inverse: bool,
    is_system_default: bool,
    priority_order: i32,
}

pub struct FilteringStage {
    proxy_repository: StreamProxySeaOrmRepository,
    filter_repository: FilterSeaOrmRepository,
    file_manager: SandboxedManager,

    regex_preprocessor: RegexPreprocessor,
    proxy_id: Option<uuid::Uuid>,
    progress_manager: Option<Arc<ProgressManager>>,
}

impl FilteringStage {
    pub async fn new(
        db_connection: Arc<DatabaseConnection>,
        file_manager: SandboxedManager,
        _pipeline_execution_prefix: String,
        proxy_id: Option<uuid::Uuid>,
        progress_manager: Option<Arc<ProgressManager>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let regex_preprocessor = RegexPreprocessor::new(RegexPreprocessorConfig::default());

        // Create repositories using the Arc<DatabaseConnection>
        let proxy_repository = StreamProxySeaOrmRepository::new(db_connection.clone());
        let filter_repository = FilterSeaOrmRepository::new(db_connection);

        Ok(Self {
            proxy_repository,
            filter_repository,
            file_manager,
            regex_preprocessor,
            proxy_id,
            progress_manager,
        })
    }

    /// Helper method for reporting progress
    async fn report_progress(&self, percentage: f64, message: &str) {
        if let Some(pm) = &self.progress_manager
            && let Some(updater) = pm.get_stage_updater("filtering").await
        {
            updater.update_progress(percentage, message).await;
        }
    }

    /// Set the progress manager for this stage
    pub fn set_progress_manager(&mut self, progress_manager: Arc<ProgressManager>) {
        self.progress_manager = Some(progress_manager);
    }

    pub async fn process(
        &mut self,
        input_artifacts: Vec<PipelineArtifact>,
    ) -> Result<Vec<PipelineArtifact>, Box<dyn std::error::Error>> {
        let stage_start = Instant::now();
        info!(
            "Starting filtering stage input_artifacts={}",
            input_artifacts.len()
        );

        let mut output_artifacts = Vec::new();
        let mut stage_filter_stats = std::collections::HashMap::new(); // filter_id -> (filter_name, included_count, excluded_count, total_time)
        let mut total_input_records = 0;
        let mut total_output_records = 0;

        let total_artifacts = input_artifacts.len();
        for (artifact_index, artifact) in input_artifacts.into_iter().enumerate() {
            match artifact.artifact_type.content {
                ContentType::Channels => {
                    let channel_progress =
                        10.0 + (artifact_index as f64 / total_artifacts as f64 * 40.0); // 10-50% for channels
                    self.report_progress(
                        channel_progress,
                        &format!(
                            "Filtering channels artifact {}/{}",
                            artifact_index + 1,
                            total_artifacts
                        ),
                    )
                    .await;
                    let (filtered_artifact, filter_stats, input_count, output_count) =
                        self.process_channel_artifact(artifact).await?;
                    output_artifacts.push(filtered_artifact);

                    // Aggregate filter statistics
                    total_input_records += input_count;
                    total_output_records += output_count;
                    for (filter_id, (filter_name, included, excluded, duration, priority)) in
                        filter_stats
                    {
                        let entry = stage_filter_stats.entry(filter_id).or_insert((
                            filter_name.clone(),
                            0,
                            0,
                            std::time::Duration::ZERO,
                            priority,
                        ));
                        entry.1 += included;
                        entry.2 += excluded;
                        entry.3 += duration;
                    }
                }
                ContentType::EpgPrograms => {
                    let epg_progress =
                        50.0 + (artifact_index as f64 / total_artifacts as f64 * 45.0); // 50-95% for EPG programs
                    self.report_progress(
                        epg_progress,
                        &format!(
                            "Filtering EPG programs artifact {}/{}",
                            artifact_index + 1,
                            total_artifacts
                        ),
                    )
                    .await;
                    let filtered_artifact = self.process_epg_artifact(artifact).await?;
                    output_artifacts.push(filtered_artifact);
                }
                _ => {
                    // Pass through other content types unchanged
                    let generic_progress =
                        10.0 + (artifact_index as f64 / total_artifacts as f64 * 85.0); // 10-95% for other types
                    self.report_progress(
                        generic_progress,
                        &format!(
                            "Processing artifact {}/{}",
                            artifact_index + 1,
                            total_artifacts
                        ),
                    )
                    .await;
                    output_artifacts.push(artifact);
                }
            }
        }

        let stage_duration = stage_start.elapsed();
        self.report_progress(
            95.0,
            &format!("Finalizing: {total_output_records} records filtered"),
        )
        .await;
        info!(
            "Completed filtering stage duration={} input_records={} output_records={} output_artifacts={}",
            crate::utils::human_format::format_duration_precise(stage_duration),
            total_input_records,
            total_output_records,
            output_artifacts.len()
        );

        // Log filter performance summary similar to data mapping
        if !stage_filter_stats.is_empty() {
            info!("Filter performance summary:");
            for (filter_id, (filter_name, included_count, excluded_count, total_time, priority)) in
                stage_filter_stats
            {
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
                info!(
                    "  Filter performance: filter_id={} filter_name={} priority={} included_count={} excluded_count={} included_percent={:.3}% excluded_percent={:.3}% avg_duration={}",
                    filter_id,
                    filter_name,
                    priority,
                    included_count,
                    excluded_count,
                    included_percent,
                    excluded_percent,
                    crate::utils::human_format::format_duration_precise(avg_time)
                );
            }
        }

        Ok(output_artifacts)
    }

    async fn process_channel_artifact(
        &mut self,
        artifact: PipelineArtifact,
    ) -> Result<
        (
            PipelineArtifact,
            std::collections::HashMap<String, (String, usize, usize, std::time::Duration, i32)>,
            usize,
            usize,
        ),
        Box<dyn std::error::Error>,
    > {
        let process_start = Instant::now();
        info!(
            "Processing channel artifact file_path={}",
            artifact.file_path
        );

        // Load channel filter rules from database
        let filter_rules = self.load_filter_rules(FilterSourceType::Stream).await?;
        // Channel filter rules loaded

        if filter_rules.is_empty() {
            info!("No channel filter rules found, passing through unchanged");
            let channels = self.read_channels_from_artifact(&artifact).await?;
            let input_count = channels.len();
            info!(
                "Passthrough: read {} channels, creating filtered artifact",
                input_count
            );

            // Create filtered artifact even when no filters are applied
            let filtered_file_path = artifact
                .file_path
                .replace("_mapping_channels.jsonl", "_filtered_channels.jsonl");

            // Write channels to filtered file
            let output_artifact = self
                .write_channels_to_artifact(channels, &filtered_file_path)
                .await?;

            info!(
                "Pipeline temp file written full_path={}",
                self.file_manager
                    .get_full_path(&filtered_file_path)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| filtered_file_path.clone())
            );

            return Ok((
                output_artifact,
                std::collections::HashMap::new(),
                input_count,
                input_count,
            ));
        }

        info!("Loaded channel filter rules count={}", filter_rules.len());

        // Read channels from input artifact
        let channels = self.read_channels_from_artifact(&artifact).await?;
        debug!("Read channels from input artifact count={}", channels.len());

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
                &rule.expression,
                regex_evaluator,
            )?;
            filtering_engine.add_filter_processor(Box::new(processor));
        }

        // Process channels through filtering engine with progress updates
        self.report_progress(
            30.0,
            &format!(
                "Applying {} filters to {} channels",
                filter_rules.len(),
                channels.len()
            ),
        )
        .await;
        let filter_result = filtering_engine.process_records(&channels)?;

        // Log filtering results
        self.log_filtering_results(&filter_result, "channels");

        // Send progress update after filtering
        self.report_progress(
            45.0,
            &format!(
                "Filtered {} channels to {} results",
                filter_result.total_input, filter_result.total_filtered
            ),
        )
        .await;

        // Write filtered channels to new artifact
        let filtered_file_path = artifact
            .file_path
            .replace("_mapping_channels.jsonl", "_filtered_channels.jsonl");
        let output_artifact = self
            .write_channels_to_artifact(filter_result.filtered_records, &filtered_file_path)
            .await?;

        info!(
            "Completed channel filtering duration={} input_channels={} output_channels={}",
            crate::utils::human_format::format_duration_precise(process_start.elapsed()),
            filter_result.total_input,
            filter_result.total_filtered
        );

        info!(
            "Pipeline temp file written full_path={}",
            self.file_manager
                .get_full_path(&filtered_file_path)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| filtered_file_path.clone())
        );

        // Convert filter stats to include filter names and priorities
        let mut filter_stats_with_names = std::collections::HashMap::new();
        for (filter_id, (included, excluded, duration)) in filter_result.filter_stats {
            let filter_name = filter_name_map
                .get(&filter_id)
                .cloned()
                .unwrap_or_else(|| filter_id.clone());
            let priority = filter_priority_map.get(&filter_id).copied().unwrap_or(999);
            filter_stats_with_names.insert(
                filter_id,
                (filter_name, included, excluded, duration, priority),
            );
        }

        Ok((
            output_artifact,
            filter_stats_with_names,
            filter_result.total_input,
            filter_result.total_filtered,
        ))
    }

    async fn process_epg_artifact(
        &mut self,
        artifact: PipelineArtifact,
    ) -> Result<PipelineArtifact, Box<dyn std::error::Error>> {
        let process_start = Instant::now();
        info!("Processing EPG artifact file_path={}", artifact.file_path);

        // Load EPG filter rules from database
        let filter_rules = self.load_filter_rules(FilterSourceType::Epg).await?;
        if filter_rules.is_empty() {
            info!("No EPG filter rules found, passing through unchanged");

            // Simply rename the file from mapping to filtered stage
            let filtered_file_path = artifact
                .file_path
                .replace("_mapping_programs.jsonl", "_filtered_programs.jsonl");

            // Copy content to new file path
            let content = self.file_manager.read(&artifact.file_path).await?;
            self.file_manager
                .write(&filtered_file_path, &content)
                .await?;

            // Create new artifact with updated path and metadata
            let output_artifact = PipelineArtifact::new(
                ArtifactType::filtered_epg(),
                filtered_file_path.clone(),
                "filtering".to_string(),
            )
            .with_record_count(artifact.record_count.unwrap_or(0))
            .with_file_size(content.len() as u64);

            info!(
                "Completed EPG filtering duration={} mode=passthrough input_programs={} output_programs={}",
                crate::utils::human_format::format_duration_precise(process_start.elapsed()),
                artifact.record_count.unwrap_or(0),
                artifact.record_count.unwrap_or(0)
            );

            info!(
                "Pipeline temp file written full_path={}",
                self.file_manager
                    .get_full_path(&filtered_file_path)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| filtered_file_path.clone())
            );

            return Ok(output_artifact);
        }

        // EPG filter rules loaded

        // Read & deduplicate EPG programs from input artifact
        self.report_progress(60.0, "Reading EPG programs for filtering")
            .await;
        let programs = self.read_epg_programs_from_artifact(&artifact).await?;
        let total_input = programs.len();
        debug!(
            "Read EPG programs from input artifact count={}",
            total_input
        );

        // Build filtering engine (reuse unified expression framework)
        let mut epg_engine: FilteringEngine<crate::pipeline::engines::rule_processor::EpgProgram> =
            FilteringEngine::new();
        let mut filter_name_map = std::collections::HashMap::new();
        let mut filter_priority_map = std::collections::HashMap::new();

        for rule in &filter_rules {
            filter_name_map.insert(rule.id.clone(), rule.name.clone());
            filter_priority_map.insert(rule.id.clone(), rule.priority_order);
            let regex_evaluator = RegexEvaluator::new(self.regex_preprocessor.clone());
            let processor = EpgFilterProcessor::new(
                rule.id.clone(),
                rule.name.clone(),
                rule.is_inverse,
                &rule.expression,
                regex_evaluator,
            )?;
            epg_engine.add_filter_processor(Box::new(processor));
        }

        self.report_progress(
            75.0,
            &format!(
                "Applying {} EPG filters to {} programs",
                filter_rules.len(),
                total_input
            ),
        )
        .await;

        // Evaluate programs
        let mut included = Vec::with_capacity(total_input);
        let mut excluded_count = 0usize;

        for program in programs {
            match epg_engine.should_include(&program) {
                Ok(should) => {
                    if should {
                        included.push(program);
                    } else {
                        excluded_count += 1;
                    }
                }
                Err(e) => {
                    warn!(
                        "EPG filter evaluation error program_id={} err={}",
                        program.id, e
                    );
                    excluded_count += 1;
                }
            }
        }

        let total_output = included.len();
        self.report_progress(
            85.0,
            &format!("Filtered EPG programs to {} results", total_output),
        )
        .await;

        // Write filtered programs to new artifact path
        let filtered_file_path = artifact
            .file_path
            .replace("_mapping_programs.jsonl", "_filtered_programs.jsonl");

        let output_artifact = self
            .write_epg_programs_to_artifact(included, &filtered_file_path)
            .await?
            .with_metadata(
                "epg_filters_applied".to_string(),
                serde_json::Value::Number(serde_json::Number::from(filter_rules.len() as u64)),
            )
            .with_metadata(
                "epg_programs_excluded".to_string(),
                serde_json::Value::Number(serde_json::Number::from(excluded_count as u64)),
            )
            .with_metadata(
                "epg_programs_included".to_string(),
                serde_json::Value::Number(serde_json::Number::from(total_output as u64)),
            )
            .with_metadata(
                "epg_programs_input".to_string(),
                serde_json::Value::Number(serde_json::Number::from(total_input as u64)),
            );

        info!(
            "[EPG_FILTER] duration={} input_programs={} included={} excluded={} filters={} ",
            crate::utils::human_format::format_duration_precise(process_start.elapsed()),
            total_input,
            total_output,
            excluded_count,
            filter_rules.len()
        );

        info!(
            "Pipeline temp file written full_path={}",
            self.file_manager
                .get_full_path(&filtered_file_path)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| filtered_file_path.clone())
        );

        Ok(output_artifact)
    }

    async fn load_filter_rules(
        &self,
        source_type: FilterSourceType,
    ) -> Result<Vec<FilterRule>, Box<dyn std::error::Error>> {
        if let Some(proxy_id) = self.proxy_id {
            // Use SeaORM repository to get proxy filters with proper relationships
            let proxy_filters = self.proxy_repository.get_proxy_filters(proxy_id).await?;

            let mut rules = Vec::new();
            for proxy_filter in proxy_filters {
                // Get the actual filter details using the filter repository
                if let Some(filter) = self
                    .filter_repository
                    .find_by_id(proxy_filter.filter_id)
                    .await?
                {
                    // Only include filters that match the requested source type and are active
                    let filter_source_type = match filter.source_type {
                        crate::models::FilterSourceType::Stream => FilterSourceType::Stream,
                        crate::models::FilterSourceType::Epg => FilterSourceType::Epg,
                    };

                    if filter_source_type == source_type && proxy_filter.is_active {
                        rules.push(FilterRule {
                            id: filter.id.to_string(),
                            name: filter.name,
                            source_type: filter_source_type,
                            expression: filter.expression,
                            is_inverse: filter.is_inverse,
                            is_system_default: filter.is_system_default,
                            priority_order: proxy_filter.priority_order,
                        });
                    }
                }
            }

            // Sort by priority order
            rules.sort_by_key(|r| r.priority_order);

            Ok(rules)
        } else {
            // No proxy_id provided - return empty list
            warn!("No proxy_id provided to FilteringStage, returning empty filter rules");
            Ok(Vec::new())
        }
    }

    async fn read_channels_from_artifact(
        &self,
        artifact: &PipelineArtifact,
    ) -> Result<Vec<Channel>, Box<dyn std::error::Error>> {
        let content = self
            .file_manager
            .read_to_string(&artifact.file_path)
            .await?;

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
            info!(
                "Input deduplication: total_channels_read={} unique_channels={} deduplicated_channels={}",
                total_channels_read,
                channels.len(),
                deduplicated_channels
            );
        } else {
            info!(
                "Input channels: total_channels_read={} unique_channels={} deduplicated_channels=0",
                total_channels_read,
                channels.len()
            );
        }

        Ok(channels)
    }

    async fn read_epg_programs_from_artifact(
        &self,
        artifact: &PipelineArtifact,
    ) -> Result<Vec<crate::pipeline::engines::rule_processor::EpgProgram>, Box<dyn std::error::Error>>
    {
        let content = self
            .file_manager
            .read_to_string(&artifact.file_path)
            .await?;

        let mut programs = Vec::new();
        let mut seen_program_keys = std::collections::HashSet::new();
        let mut total_programs_read = 0;
        let mut deduplicated_programs = 0;

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let program: crate::pipeline::engines::rule_processor::EpgProgram =
                serde_json::from_str(line)?;
            total_programs_read += 1;

            // Create composite key for deduplication: channel_id + title + start_time
            let dedup_key = format!(
                "{}|{}|{}",
                program.channel_id,
                program.title,
                program.start_time.timestamp()
            );

            if seen_program_keys.insert(dedup_key.clone()) {
                // New unique program
                programs.push(program);
            } else {
                // Duplicate program found
                deduplicated_programs += 1;
                trace!(
                    "Duplicate EPG program found: channel_id={} title={} start_time={}",
                    program.channel_id, program.title, program.start_time
                );
            }
        }

        if deduplicated_programs > 0 {
            info!(
                "Input EPG deduplication: total_programs_read={} unique_programs={} deduplicated_programs={}",
                total_programs_read,
                programs.len(),
                deduplicated_programs
            );
        } else {
            info!(
                "Input EPG programs: total_programs_read={} unique_programs={} deduplicated_programs=0",
                total_programs_read,
                programs.len()
            );
        }

        Ok(programs)
    }

    async fn write_channels_to_artifact(
        &mut self,
        channels: Vec<Channel>,
        file_path: &str,
    ) -> Result<PipelineArtifact, Box<dyn std::error::Error>> {
        // Write channels as JSONL
        let mut content = String::new();
        for channel in &channels {
            content.push_str(&serde_json::to_string(channel)?);
            content.push('\n');
        }

        self.file_manager
            .write(file_path, content.as_bytes())
            .await?;

        Ok(PipelineArtifact::new(
            ArtifactType::filtered_channels(),
            file_path.to_string(),
            "filtering".to_string(),
        )
        .with_record_count(channels.len())
        .with_file_size(content.len() as u64))
    }

    async fn write_epg_programs_to_artifact(
        &mut self,
        programs: Vec<crate::pipeline::engines::rule_processor::EpgProgram>,
        file_path: &str,
    ) -> Result<PipelineArtifact, Box<dyn std::error::Error>> {
        // Write EPG programs as JSONL
        let mut content = String::new();
        for program in &programs {
            content.push_str(&serde_json::to_string(program)?);
            content.push('\n');
        }

        self.file_manager
            .write(file_path, content.as_bytes())
            .await?;

        Ok(PipelineArtifact::new(
            ArtifactType::filtered_epg(),
            file_path.to_string(),
            "filtering".to_string(),
        )
        .with_record_count(programs.len())
        .with_file_size(content.len() as u64))
    }

    fn log_filtering_results<T>(&self, result: &FilterEngineResult<T>, content_type: &str) {
        info!(
            "{} filtering results input_records={} output_records={} duration={}",
            content_type,
            result.total_input,
            result.total_filtered,
            crate::utils::human_format::format_duration_precise(result.execution_time)
        );

        for (filter_id, (included_count, excluded_count, filter_time)) in &result.filter_stats {
            trace!(
                "Filter filter_id={} included={} excluded={} duration={}",
                filter_id,
                included_count,
                excluded_count,
                crate::utils::human_format::format_duration_precise(*filter_time)
            );
        }
    }

    pub fn cleanup(self) -> Result<(), Box<dyn std::error::Error>> {
        // Clear any cached state
        trace!("Cleaning up filtering stage");
        Ok(())
    }
}

impl ProgressAware for FilteringStage {
    fn get_progress_manager(&self) -> Option<&Arc<ProgressManager>> {
        self.progress_manager.as_ref()
    }
}

#[async_trait::async_trait]
impl PipelineStage for FilteringStage {
    async fn execute(
        &mut self,
        input: Vec<PipelineArtifact>,
    ) -> Result<Vec<PipelineArtifact>, PipelineError> {
        self.report_progress(25.0, "Initializing filters").await;
        let result = self.process(input).await.map_err(|e| {
            PipelineError::stage_error("filtering", format!("Filtering failed: {e}"))
        })?;
        self.report_progress(100.0, "Filtering completed").await;
        Ok(result)
    }

    fn stage_id(&self) -> &'static str {
        "filtering"
    }

    fn stage_name(&self) -> &'static str {
        "Filtering"
    }

    async fn cleanup(&mut self) -> Result<(), PipelineError> {
        trace!("Cleaning up filtering stage");
        Ok(())
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
