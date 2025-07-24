//! Channel numbering stage for pipeline processing
//!
//! This stage assigns channel numbers to channels based on existing tvg-channo values
//! with priority-based conflict resolution and efficient single-pass algorithm.

use crate::pipeline::models::{PipelineArtifact, ArtifactType, ContentType, ProcessingStage};
use crate::models::Channel;
use sandboxed_file_manager::SandboxedManager;
use serde_json;
use std::collections::{HashSet, BTreeSet};
use std::time::Instant;
use tracing::{info, warn, debug};
use crate::utils::human_format::format_duration_precise;

pub struct NumberingStage {
    file_manager: SandboxedManager,
    pipeline_execution_prefix: String,
    starting_channel_number: u32,
}

impl NumberingStage {
    pub fn new(
        file_manager: SandboxedManager, 
        pipeline_execution_prefix: String,
        starting_channel_number: u32,
    ) -> Self {
        Self {
            file_manager,
            pipeline_execution_prefix,
            starting_channel_number,
        }
    }
    
    pub async fn process(&self, input_artifacts: Vec<PipelineArtifact>) -> Result<Vec<PipelineArtifact>, Box<dyn std::error::Error>> {
        let stage_start = Instant::now();
        info!("Starting numbering stage with {} input artifacts", input_artifacts.len());
        
        let mut output_artifacts = Vec::new();
        let mut total_channels_processed = 0;
        let mut total_numbers_assigned = 0;
        let mut total_channo_conflicts_resolved = 0;
        
        for artifact in input_artifacts {
            match artifact.artifact_type.content {
                ContentType::Channels => {
                    let (numbered_artifact, processed, assigned, conflicts) = self.process_channel_artifact(artifact).await?;
                    output_artifacts.push(numbered_artifact);
                    total_channels_processed += processed;
                    total_numbers_assigned += assigned;
                    total_channo_conflicts_resolved += conflicts;
                }
                ContentType::EpgPrograms => {
                    // EPG data: copy content to maintain consistent naming
                    let copied_artifact = self.copy_epg_artifact(artifact).await?;
                    output_artifacts.push(copied_artifact);
                }
                _ => {
                    // Pass through other content types unchanged
                    output_artifacts.push(artifact);
                }
            }
        }
        
        let stage_duration = stage_start.elapsed();
        info!(
            "Numbering stage completed: duration={} channels_processed={} numbers_assigned={} channo_conflicts_resolved={}",
            format_duration_precise(stage_duration), total_channels_processed, total_numbers_assigned, total_channo_conflicts_resolved
        );
        
        Ok(output_artifacts)
    }
    
    async fn process_channel_artifact(&self, artifact: PipelineArtifact) -> Result<(PipelineArtifact, usize, usize, usize), Box<dyn std::error::Error>> {
        // Read channels from input artifact
        let content = String::from_utf8(self.file_manager.read(&artifact.file_path).await?)?;
        
        let mut channels: Vec<Channel> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line))
            .collect::<Result<Vec<_>, _>>()?;
        
        let channel_count = channels.len();
        if channel_count == 0 {
            debug!("No channels to process in artifact {}", artifact.id);
            return Ok((artifact, 0, 0, 0));
        }
        
        // Apply efficient numbering algorithm
        let (assigned_count, channo_conflicts_resolved) = self.apply_numbering(&mut channels).await?;
        
        // Write numbered channels to new artifact file
        let output_filename = format!("numbered_channels_{}.jsonl", uuid::Uuid::new_v4());
        
        let output_content = channels
            .iter()
            .map(|channel| serde_json::to_string(channel))
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        
        self.file_manager.write(&output_filename, output_content.as_bytes()).await?;
        
        // Create output artifact
        let mut output_artifact = PipelineArtifact::new(
            ArtifactType::new(ContentType::Channels, ProcessingStage::Numbered),
            output_filename,
            "numbering".to_string(),
        )
        .with_record_count(channel_count)
        .with_metadata("channels_assigned_numbers".to_string(), assigned_count.into())
        .with_metadata("channo_conflicts_resolved".to_string(), channo_conflicts_resolved.into())
        .with_metadata("starting_channel_number".to_string(), self.starting_channel_number.into());
        
        // Add file size if possible
        output_artifact = output_artifact.with_file_size(output_content.len() as u64);
        
        debug!(
            "Processed channel artifact: {} channels, {} assigned numbers, {} channo conflicts resolved",
            channel_count, assigned_count, channo_conflicts_resolved
        );
        
        Ok((output_artifact, channel_count, assigned_count, channo_conflicts_resolved))
    }
    
    async fn copy_epg_artifact(&self, artifact: PipelineArtifact) -> Result<PipelineArtifact, Box<dyn std::error::Error>> {
        // For EPG files, copy the JSONL content to maintain consistency
        let output_filename = format!("{}_numbered_programs.jsonl", self.pipeline_execution_prefix);
        
        // Read from input and write to output
        let content = self.file_manager.read(&artifact.file_path).await?;
        self.file_manager.write(&output_filename, &content).await?;
        
        debug!("Copied EPG programs artifact: {} -> {}", artifact.file_path, output_filename);
        
        let mut output_artifact = PipelineArtifact::new(
            ArtifactType::new(ContentType::EpgPrograms, ProcessingStage::Numbered),
            output_filename,
            "numbering".to_string(),
        );
        
        // Preserve metadata from input artifact
        if let Some(record_count) = artifact.record_count {
            output_artifact = output_artifact.with_record_count(record_count);
        }
        
        // Add file size
        output_artifact = output_artifact.with_file_size(content.len() as u64);
        
        Ok(output_artifact)
    }
    
    /// Apply efficient single-pass numbering algorithm with comprehensive metrics
    async fn apply_numbering(&self, channels: &mut [Channel]) -> Result<(usize, usize), Box<dyn std::error::Error>> {
        let algorithm_start = Instant::now();
        let total_channels = channels.len();
        
        // Performance tracking
        let mut used_numbers = HashSet::new();
        let mut channels_needing_numbers = Vec::new();
        let mut channo_conflicts_resolved = 0;
        let mut channels_with_existing_channo = 0;
        
        // Phase-based progress logging for large datasets
        let mut progress_task = None;
        if total_channels > 1000 {
            debug!("Numbering progress: starting analysis of {} channels (0.0%), elapsed: {}", 
                   total_channels, format_duration_precise(algorithm_start.elapsed()));
            
            // Start interval-driven progress updates
            let total_for_task = total_channels;
            let start_time = algorithm_start;
            progress_task = Some(tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
                loop {
                    interval.tick().await;
                    let elapsed = start_time.elapsed();
                    debug!("Numbering progress: processing {} channels, elapsed: {}", 
                           total_for_task, format_duration_precise(elapsed));
                }
            }));
        }
        
        // First pass: collect existing tvg-channo values and handle conflicts intelligently
        let first_pass_start = Instant::now();
        
        for (idx, channel) in channels.iter().enumerate() {
            if let Some(ref channo_str) = channel.tvg_chno {
                channels_with_existing_channo += 1;
                
                // Parse channel number, ignoring leading zeros as requested
                if let Ok(mut desired_channo) = channo_str.trim_start_matches('0').parse::<u32>() {
                    if desired_channo > 0 {
                        // Try to use the desired number, or increment until we find an available one
                        let original_channo = desired_channo;
                        while used_numbers.contains(&desired_channo) {
                            desired_channo += 1;
                            channo_conflicts_resolved += 1;
                        }
                        
                        // Claim the resolved number
                        used_numbers.insert(desired_channo);
                        
                        // Update the channel's channo if it was incremented
                        if desired_channo != original_channo {
                            warn!("Channel number conflict {} found for channel '{}', reassigned to {}", 
                                  original_channo, channel.channel_name, desired_channo);
                            // We'll update the actual channel later to avoid borrowing issues
                            channels_needing_numbers.push((idx, Some(desired_channo)));
                        }
                        continue;
                    } else {
                        // Invalid number (0) - mark for sequential numbering
                        debug!("Invalid channel number 0 for channel '{}', will assign new number", 
                               channel.channel_name);
                        channels_needing_numbers.push((idx, None));
                    }
                } else {
                    // Invalid format - mark for sequential numbering
                    debug!("Invalid channel number format '{}' for channel '{}', will assign new number", 
                           channo_str, channel.channel_name);
                    channels_needing_numbers.push((idx, None));
                }
            } else {
                // No channel number - mark for sequential numbering
                channels_needing_numbers.push((idx, None));
            }
        }
        
        let first_pass_duration = first_pass_start.elapsed();
        
        // Progress update after first pass
        if total_channels > 1000 {
            debug!("Numbering progress: first pass complete, {} channels analyzed (33.3%), elapsed: {}", 
                   total_channels, format_duration_precise(algorithm_start.elapsed()));
        }
        
        // Build available number pool efficiently - start from starting_channel_number for sequential fills
        // Only count channels that need sequential assignment (not conflict-resolved ones)
        let pool_build_start = Instant::now();
        
        let sequential_assignment_needed = channels_needing_numbers.iter()
            .filter(|(_, assigned_num)| assigned_num.is_none())
            .count() as u32;
        
        let upper_bound = self.starting_channel_number + sequential_assignment_needed;
        
        // Only add numbers from starting_channel_number upward (sequential fills)
        let mut available_numbers = BTreeSet::new();
        for num in self.starting_channel_number..=upper_bound {
            if !used_numbers.contains(&num) {
                available_numbers.insert(num);
            }
        }
        
        let pool_build_duration = pool_build_start.elapsed();
        
        // Calculate efficiency metrics
        let theoretical_max_pool = if sequential_assignment_needed > 0 {
            upper_bound - self.starting_channel_number + 1
        } else {
            0
        };
        let actual_pool_size = available_numbers.len() as u32;
        let pool_efficiency = if theoretical_max_pool > 0 {
            (actual_pool_size as f64 / theoretical_max_pool as f64) * 100.0
        } else {
            100.0
        };
        
        debug!(
            "Channel numbering analysis: existing_numbers={} need_assignment={} (sequential={} conflicts_resolved={}) available_pool_size={} pool_efficiency={:.1}%",
            used_numbers.len(),
            channels_needing_numbers.len(),
            sequential_assignment_needed,
            channo_conflicts_resolved,
            available_numbers.len(),
            pool_efficiency
        );
        
        // Progress update after pool building
        if total_channels > 1000 {
            debug!("Numbering progress: pool building complete, {} available numbers (66.7%), elapsed: {}", 
                   available_numbers.len(), format_duration_precise(algorithm_start.elapsed()));
        }
        
        // Second pass: assign numbers to channels that need them
        let assignment_start = Instant::now();
        let mut assigned_count = 0;
        
        for (idx, assigned_num) in &channels_needing_numbers {
            match assigned_num {
                Some(num) => {
                    // This channel was already assigned a number due to conflict resolution
                    channels[*idx].tvg_chno = Some(num.to_string());
                    assigned_count += 1;
                }
                None => {
                    // This channel needs sequential assignment
                    if let Some(next_number) = available_numbers.pop_first() {
                        channels[*idx].tvg_chno = Some(next_number.to_string());
                        used_numbers.insert(next_number);
                        assigned_count += 1;
                    } else {
                        if let Some(task) = progress_task.as_ref() {
                            task.abort();
                        }
                        return Err(format!("Ran out of available channel numbers starting from {}", self.starting_channel_number).into());
                    }
                }
            }
        }
        
        let assignment_duration = assignment_start.elapsed();
        let total_algorithm_duration = algorithm_start.elapsed();
        
        // Final progress update
        if total_channels > 1000 {
            debug!("Numbering progress: assignment complete, {} channels processed (100.0%), elapsed: {}", 
                   total_channels, format_duration_precise(total_algorithm_duration));
        }
        
        // Abort progress logging
        if let Some(task) = progress_task {
            task.abort();
        }
        
        // Comprehensive performance metrics
        let avg_time_per_channel_duration = std::time::Duration::from_nanos(
            (total_algorithm_duration.as_nanos() / total_channels as u128) as u64
        );
        let channels_without_channo = total_channels - channels_with_existing_channo;
        
        let (min_num, max_num) = if !used_numbers.is_empty() {
            (*used_numbers.iter().min().unwrap(), *used_numbers.iter().max().unwrap())
        } else {
            (self.starting_channel_number, self.starting_channel_number)
        };
        
        info!(
            "Channel numbering performance: total={} existing_channo={} conflicts_resolved={} newly_assigned={} \
             range={}~{} duration={} (1st_pass={} pool_build={} assignment={}) avg_per_channel={}",
            total_channels, 
            channels_with_existing_channo,
            channo_conflicts_resolved, 
            assigned_count,
            min_num, 
            max_num,
            format_duration_precise(total_algorithm_duration),
            format_duration_precise(first_pass_duration),
            format_duration_precise(pool_build_duration), 
            format_duration_precise(assignment_duration),
            format_duration_precise(avg_time_per_channel_duration)
        );
        
        // Additional efficiency logging
        if channels_with_existing_channo > 0 {
            let existing_channo_percentage = (channels_with_existing_channo as f64 / total_channels as f64) * 100.0;
            info!(
                "Channel number distribution: {:.1}% had existing channo, {:.1}% needed assignment, {:.1}% conflicts resolved",
                existing_channo_percentage,
                (channels_without_channo as f64 / total_channels as f64) * 100.0,
                (channo_conflicts_resolved as f64 / total_channels as f64) * 100.0
            );
        }
        
        Ok((assigned_count, channo_conflicts_resolved))
    }
}