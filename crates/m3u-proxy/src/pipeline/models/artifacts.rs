//! Pipeline artifact tracking system
//!
//! This module defines the types and tracking system for pipeline artifacts as they flow
//! between stages. Each stage produces and consumes specific types of artifacts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Types of content that can be stored in pipeline artifacts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ContentType {
    /// Channel/stream data (M3U records)
    Channels,
    /// EPG/program data (XMLTV records)
    EpgPrograms,
    /// Generated M3U playlist files
    M3uPlaylist,
    /// Generated XMLTV guide files
    XmltvGuide,
    /// Generated proxy files (M3U playlists) - legacy
    ProxyFiles,
    /// Generated EPG files (XMLTV) - legacy
    EpgFiles,
}

/// Processing stages that artifacts can be in
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ProcessingStage {
    /// Raw data loaded from sources
    Raw,
    /// Data after mapping rules applied
    Mapped,
    /// Data after filtering rules applied
    Filtered,
    /// Data after logo caching applied
    LogoCached,
    /// Data after numbering rules applied
    Numbered,
    /// Final generated output in temporary files
    Generated,
    /// Files published to final location atomically
    Published,
}

/// Type of pipeline artifact combining content type and processing stage
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ArtifactType {
    pub content: ContentType,
    pub stage: ProcessingStage,
}

impl ArtifactType {
    pub fn new(content: ContentType, stage: ProcessingStage) -> Self {
        Self { content, stage }
    }

    /// Channel data after data mapping
    pub fn mapped_channels() -> Self {
        Self::new(ContentType::Channels, ProcessingStage::Mapped)
    }

    /// EPG data after data mapping
    pub fn mapped_epg() -> Self {
        Self::new(ContentType::EpgPrograms, ProcessingStage::Mapped)
    }

    /// Channel data after filtering
    pub fn filtered_channels() -> Self {
        Self::new(ContentType::Channels, ProcessingStage::Filtered)
    }

    /// EPG data after filtering
    pub fn filtered_epg() -> Self {
        Self::new(ContentType::EpgPrograms, ProcessingStage::Filtered)
    }

    /// Channel data after logo caching
    pub fn logo_cached_channels() -> Self {
        Self::new(ContentType::Channels, ProcessingStage::LogoCached)
    }

    /// Channel data after numbering
    pub fn numbered_channels() -> Self {
        Self::new(ContentType::Channels, ProcessingStage::Numbered)
    }

    /// Generated M3U playlist files
    pub fn generated_m3u() -> Self {
        Self::new(ContentType::M3uPlaylist, ProcessingStage::Generated)
    }

    /// Generated XMLTV guide files
    pub fn generated_xmltv() -> Self {
        Self::new(ContentType::XmltvGuide, ProcessingStage::Generated)
    }

    /// Published M3U playlist files
    pub fn published_m3u() -> Self {
        Self::new(ContentType::M3uPlaylist, ProcessingStage::Published)
    }

    /// Published XMLTV guide files
    pub fn published_xmltv() -> Self {
        Self::new(ContentType::XmltvGuide, ProcessingStage::Published)
    }

    /// Final generated proxy files - legacy
    pub fn generated_proxies() -> Self {
        Self::new(ContentType::ProxyFiles, ProcessingStage::Generated)
    }

    /// Final generated EPG files - legacy
    pub fn generated_epg() -> Self {
        Self::new(ContentType::EpgFiles, ProcessingStage::Generated)
    }
}

impl std::fmt::Display for ArtifactType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}_{:?}", self.content, self.stage)
    }
}

/// A pipeline artifact representing a file created during pipeline execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineArtifact {
    /// Unique identifier for this artifact
    pub id: String,
    /// Type of artifact (content + processing stage)
    pub artifact_type: ArtifactType,
    /// Relative path to the file within the pipeline temp directory
    pub file_path: String,
    /// Name of the pipeline stage that created this artifact
    pub created_by_stage: String,
    /// Number of records in this artifact (if applicable)
    pub record_count: Option<usize>,
    /// Size of the file in bytes
    pub file_size: Option<u64>,
    /// When this artifact was created
    #[serde(serialize_with = "crate::utils::datetime::serialize_datetime")]
    #[serde(deserialize_with = "crate::utils::datetime::deserialize_datetime")]
    pub created_at: DateTime<Utc>,
    /// Additional metadata about this artifact
    pub metadata: HashMap<String, serde_json::Value>,
}

impl PipelineArtifact {
    pub fn new(artifact_type: ArtifactType, file_path: String, created_by_stage: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            artifact_type,
            file_path,
            created_by_stage,
            record_count: None,
            file_size: None,
            created_at: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    pub fn with_record_count(mut self, count: usize) -> Self {
        self.record_count = Some(count);
        self
    }

    pub fn with_file_size(mut self, size: u64) -> Self {
        self.file_size = Some(size);
        self
    }

    pub fn with_metadata(mut self, key: String, value: serde_json::Value) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Registry for tracking pipeline artifacts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRegistry {
    /// All artifacts created during pipeline execution
    pub artifacts: HashMap<String, PipelineArtifact>,
    /// Index by artifact type for quick lookup
    pub by_type: HashMap<ArtifactType, Vec<String>>,
    /// Index by creating stage
    pub by_stage: HashMap<String, Vec<String>>,
}

impl ArtifactRegistry {
    pub fn new() -> Self {
        Self {
            artifacts: HashMap::new(),
            by_type: HashMap::new(),
            by_stage: HashMap::new(),
        }
    }

    /// Register a new artifact
    pub fn register(&mut self, artifact: PipelineArtifact) {
        let artifact_id = artifact.id.clone();
        let artifact_type = artifact.artifact_type.clone();
        let stage = artifact.created_by_stage.clone();

        // Add to main registry
        self.artifacts.insert(artifact_id.clone(), artifact);

        // Add to type index
        self.by_type
            .entry(artifact_type)
            .or_default()
            .push(artifact_id.clone());

        // Add to stage index
        self.by_stage.entry(stage).or_default().push(artifact_id);
    }

    /// Get all artifacts of a specific type
    pub fn get_by_type(&self, artifact_type: &ArtifactType) -> Vec<&PipelineArtifact> {
        self.by_type
            .get(artifact_type)
            .map(|ids| ids.iter().filter_map(|id| self.artifacts.get(id)).collect())
            .unwrap_or_default()
    }

    /// Get all artifacts created by a specific stage
    pub fn get_by_stage(&self, stage: &str) -> Vec<&PipelineArtifact> {
        self.by_stage
            .get(stage)
            .map(|ids| ids.iter().filter_map(|id| self.artifacts.get(id)).collect())
            .unwrap_or_default()
    }

    /// Get the most recent artifact of a specific type
    pub fn get_latest_by_type(&self, artifact_type: &ArtifactType) -> Option<&PipelineArtifact> {
        self.get_by_type(artifact_type)
            .into_iter()
            .max_by_key(|artifact| artifact.created_at)
    }

    /// Get all artifact types currently registered
    pub fn get_available_types(&self) -> Vec<&ArtifactType> {
        self.by_type.keys().collect()
    }

    /// Get summary statistics
    pub fn get_summary(&self) -> ArtifactSummary {
        let total_artifacts = self.artifacts.len();
        let total_records: usize = self.artifacts.values().filter_map(|a| a.record_count).sum();
        let total_size_bytes: u64 = self.artifacts.values().filter_map(|a| a.file_size).sum();

        let by_type: HashMap<ArtifactType, usize> = self
            .by_type
            .iter()
            .map(|(t, ids)| (t.clone(), ids.len()))
            .collect();

        ArtifactSummary {
            total_artifacts,
            total_records,
            total_size_bytes,
            by_type,
        }
    }

    /// Clear all artifacts to free memory (called after pipeline completion)
    pub fn clear(&mut self) {
        self.artifacts.clear();
        self.by_type.clear();
        self.by_stage.clear();
    }

    /// Remove old artifacts to prevent memory accumulation
    pub fn cleanup_old_artifacts(&mut self, max_age_minutes: i64) {
        let cutoff_time = Utc::now() - chrono::Duration::minutes(max_age_minutes);

        // Find artifacts older than cutoff
        let old_artifact_ids: Vec<String> = self
            .artifacts
            .iter()
            .filter(|(_, artifact)| artifact.created_at < cutoff_time)
            .map(|(id, _)| id.clone())
            .collect();

        // Remove from main registry
        for artifact_id in &old_artifact_ids {
            if let Some(artifact) = self.artifacts.remove(artifact_id) {
                // Remove from type index
                if let Some(type_list) = self.by_type.get_mut(&artifact.artifact_type) {
                    type_list.retain(|id| id != artifact_id);
                    if type_list.is_empty() {
                        self.by_type.remove(&artifact.artifact_type);
                    }
                }

                // Remove from stage index
                if let Some(stage_list) = self.by_stage.get_mut(&artifact.created_by_stage) {
                    stage_list.retain(|id| id != artifact_id);
                    if stage_list.is_empty() {
                        self.by_stage.remove(&artifact.created_by_stage);
                    }
                }
            }
        }

        if !old_artifact_ids.is_empty() {
            tracing::debug!(
                "Cleaned up {} old artifacts from registry",
                old_artifact_ids.len()
            );
        }
    }
}

impl Default for ArtifactRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary statistics for pipeline artifacts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactSummary {
    pub total_artifacts: usize,
    pub total_records: usize,
    pub total_size_bytes: u64,
    pub by_type: HashMap<ArtifactType, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_type_creation() {
        let mapped_channels = ArtifactType::mapped_channels();
        assert_eq!(mapped_channels.content, ContentType::Channels);
        assert_eq!(mapped_channels.stage, ProcessingStage::Mapped);
    }

    #[test]
    fn test_artifact_registry() {
        let mut registry = ArtifactRegistry::new();

        let artifact = PipelineArtifact::new(
            ArtifactType::mapped_channels(),
            "mapped_channels.jsonl".to_string(),
            "data_mapping".to_string(),
        )
        .with_record_count(1000);

        registry.register(artifact);

        let artifacts = registry.get_by_type(&ArtifactType::mapped_channels());
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].record_count, Some(1000));
    }
}
