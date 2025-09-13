use super::artifacts::ArtifactRegistry;
use crate::utils::datetime::DateTimeParser;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineExecution {
    pub id: Uuid,
    pub execution_prefix: String,
    pub proxy_id: Uuid,
    pub status: PipelineStatus,
    #[serde(serialize_with = "crate::utils::datetime::serialize_datetime")]
    #[serde(deserialize_with = "crate::utils::datetime::deserialize_datetime")]
    pub started_at: DateTime<Utc>,
    #[serde(serialize_with = "crate::utils::datetime::serialize_optional_datetime")]
    #[serde(deserialize_with = "crate::utils::datetime::deserialize_optional_datetime")]
    pub completed_at: Option<DateTime<Utc>>,
    pub stages: HashMap<String, PipelineStageExecution>,
    pub artifacts: ArtifactRegistry,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PipelineStatus {
    Initializing,
    DataMapping,
    Filtering,
    LogoCaching,
    Numbering,
    Generation,
    Publishing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStageExecution {
    pub name: String,
    pub status: StageStatus,
    #[serde(serialize_with = "crate::utils::datetime::serialize_optional_datetime")]
    #[serde(deserialize_with = "crate::utils::datetime::deserialize_optional_datetime")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "crate::utils::datetime::serialize_optional_datetime")]
    #[serde(deserialize_with = "crate::utils::datetime::deserialize_optional_datetime")]
    pub completed_at: Option<DateTime<Utc>>,
    pub output_artifacts: Vec<String>, // Artifact IDs
    pub metrics: HashMap<String, serde_json::Value>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StageStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

impl PipelineExecution {
    pub fn new(proxy_id: Uuid) -> Self {
        let execution_id = Uuid::new_v4();
        let execution_prefix = format!("pipeline_{}", execution_id.simple());

        Self {
            id: execution_id,
            execution_prefix,
            proxy_id,
            status: PipelineStatus::Initializing,
            started_at: DateTimeParser::now_utc(),
            completed_at: None,
            stages: HashMap::new(),
            artifacts: ArtifactRegistry::new(),
            error_message: None,
        }
    }

    pub fn add_stage(&mut self, stage_name: String) {
        let stage = PipelineStageExecution {
            name: stage_name.clone(),
            status: StageStatus::Pending,
            started_at: None,
            completed_at: None,
            output_artifacts: Vec::new(),
            metrics: HashMap::new(),
            error_message: None,
        };
        self.stages.insert(stage_name, stage);
    }

    pub fn start_stage(&mut self, stage_name: &str) {
        if let Some(stage) = self.stages.get_mut(stage_name) {
            stage.status = StageStatus::Running;
            stage.started_at = Some(DateTimeParser::now_utc());
        }
    }

    pub fn complete_stage(
        &mut self,
        stage_name: &str,
        output_artifacts: Vec<String>,
        metrics: HashMap<String, serde_json::Value>,
    ) {
        if let Some(stage) = self.stages.get_mut(stage_name) {
            stage.status = StageStatus::Completed;
            stage.completed_at = Some(DateTimeParser::now_utc());
            stage.output_artifacts = output_artifacts;
            stage.metrics = metrics;
        }
    }

    /// Complete a stage with pipeline artifacts
    pub fn complete_stage_with_artifacts(
        &mut self,
        stage_name: &str,
        artifacts: Vec<super::artifacts::PipelineArtifact>,
        metrics: HashMap<String, serde_json::Value>,
    ) {
        let mut artifact_ids = Vec::new();

        // Register artifacts and collect their IDs
        for artifact in artifacts {
            let artifact_id = artifact.id.clone();
            self.artifacts.register(artifact);
            artifact_ids.push(artifact_id);
        }

        // Complete the stage with artifact IDs
        self.complete_stage(stage_name, artifact_ids, metrics);
    }

    /// Get artifacts produced by a specific stage
    pub fn get_stage_artifacts(
        &self,
        stage_name: &str,
    ) -> Vec<&super::artifacts::PipelineArtifact> {
        self.artifacts.get_by_stage(stage_name)
    }

    /// Get artifacts of a specific type
    pub fn get_artifacts_by_type(
        &self,
        artifact_type: &super::artifacts::ArtifactType,
    ) -> Vec<&super::artifacts::PipelineArtifact> {
        self.artifacts.get_by_type(artifact_type)
    }

    pub fn fail_stage(&mut self, stage_name: &str, error_message: String) {
        if let Some(stage) = self.stages.get_mut(stage_name) {
            stage.status = StageStatus::Failed;
            stage.completed_at = Some(DateTimeParser::now_utc());
            stage.error_message = Some(error_message);
        }
    }

    pub fn complete(&mut self) {
        self.status = PipelineStatus::Completed;
        self.completed_at = Some(DateTimeParser::now_utc());
    }

    pub fn fail(&mut self, error_message: String) {
        self.status = PipelineStatus::Failed;
        self.completed_at = Some(DateTimeParser::now_utc());
        self.error_message = Some(error_message);
    }
}
