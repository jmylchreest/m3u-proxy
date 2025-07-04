use crate::models::{EpgChannel, EpgProgram};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DataMappingRule {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub source_type: DataMappingSourceType,
    pub scope: DataMappingRuleScope,
    pub sort_order: i32,
    pub is_active: bool,
    pub expression: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "data_mapping_source_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum DataMappingSourceType {
    Stream,
    Epg,
}

impl std::fmt::Display for DataMappingSourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataMappingSourceType::Stream => write!(f, "stream"),
            DataMappingSourceType::Epg => write!(f, "epg"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "data_mapping_rule_scope", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum DataMappingRuleScope {
    /// Rule applies to individual channels/items
    Individual,
    /// Rule applies to all streams within a source (stream-wide)
    StreamWide,
    /// Rule applies to all EPG data within a source (epg-wide)
    EpgWide,
}

pub struct StreamMappingFields;

impl StreamMappingFields {
    pub fn available_fields() -> Vec<&'static str> {
        vec![
            "tvg_id",
            "tvg_name",
            "tvg_logo",
            "tvg_shift",
            "tvg_channo",
            "group_title",
            "channel_name",
        ]
    }

    pub fn is_valid_field(field: &str) -> bool {
        Self::available_fields().contains(&field)
    }
}

pub struct EpgMappingFields;

impl EpgMappingFields {
    pub fn available_fields() -> Vec<&'static str> {
        vec![
            "channel_id",
            "channel_name",
            "channel_logo",
            "channel_group",
            "language",
        ]
    }

    pub fn is_valid_field(field: &str) -> bool {
        Self::available_fields().contains(&field)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingFieldInfo {
    pub field_name: String,
    pub display_name: String,
    pub field_type: String,
    pub nullable: bool,
    pub source_type: DataMappingSourceType,
}

impl DataMappingFieldInfo {
    pub fn available_for_source_type(
        source_type: &DataMappingSourceType,
    ) -> Vec<DataMappingFieldInfo> {
        match source_type {
            DataMappingSourceType::Stream => vec![
                DataMappingFieldInfo {
                    field_name: "tvg_id".to_string(),
                    display_name: "TVG ID".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: DataMappingSourceType::Stream,
                },
                DataMappingFieldInfo {
                    field_name: "tvg_name".to_string(),
                    display_name: "TVG Name".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: DataMappingSourceType::Stream,
                },
                DataMappingFieldInfo {
                    field_name: "tvg_logo".to_string(),
                    display_name: "TVG Logo".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: DataMappingSourceType::Stream,
                },
                DataMappingFieldInfo {
                    field_name: "tvg_shift".to_string(),
                    display_name: "TVG Shift".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: DataMappingSourceType::Stream,
                },
                DataMappingFieldInfo {
                    field_name: "tvg_channo".to_string(),
                    display_name: "TVG Channel Number".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: DataMappingSourceType::Stream,
                },
                DataMappingFieldInfo {
                    field_name: "group_title".to_string(),
                    display_name: "Group Title".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: DataMappingSourceType::Stream,
                },
                DataMappingFieldInfo {
                    field_name: "channel_name".to_string(),
                    display_name: "Channel Name".to_string(),
                    field_type: "string".to_string(),
                    nullable: false,
                    source_type: DataMappingSourceType::Stream,
                },
            ],
            DataMappingSourceType::Epg => vec![
                DataMappingFieldInfo {
                    field_name: "channel_id".to_string(),
                    display_name: "Channel ID".to_string(),
                    field_type: "string".to_string(),
                    nullable: false,
                    source_type: DataMappingSourceType::Epg,
                },
                DataMappingFieldInfo {
                    field_name: "channel_name".to_string(),
                    display_name: "Channel Name".to_string(),
                    field_type: "string".to_string(),
                    nullable: false,
                    source_type: DataMappingSourceType::Epg,
                },
                DataMappingFieldInfo {
                    field_name: "channel_logo".to_string(),
                    display_name: "Channel Logo".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: DataMappingSourceType::Epg,
                },
                DataMappingFieldInfo {
                    field_name: "channel_group".to_string(),
                    display_name: "Channel Group".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: DataMappingSourceType::Epg,
                },
                DataMappingFieldInfo {
                    field_name: "language".to_string(),
                    display_name: "Language".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: DataMappingSourceType::Epg,
                },
            ],
        }
    }

    pub fn is_valid_field_for_source_type(
        field: &str,
        source_type: &DataMappingSourceType,
    ) -> bool {
        Self::available_for_source_type(source_type)
            .iter()
            .any(|f| f.field_name == field)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappedChannel {
    #[serde(flatten)]
    pub original: crate::models::Channel,
    pub mapped_tvg_id: Option<String>,
    pub mapped_tvg_name: Option<String>,
    pub mapped_tvg_logo: Option<String>,
    pub mapped_tvg_shift: Option<String>,
    pub mapped_group_title: Option<String>,
    pub mapped_channel_name: String,
    pub applied_rules: Vec<String>,
    pub is_removed: bool,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub capture_group_values: HashMap<String, HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappedEpgChannel {
    #[serde(flatten)]
    pub original: EpgChannel,
    pub mapped_channel_id: String,
    pub mapped_channel_name: String,
    pub mapped_channel_logo: Option<String>,
    pub mapped_channel_group: Option<String>,
    pub mapped_language: Option<String>,
    pub applied_rules: Vec<String>,
    pub clone_group_id: Option<String>,
    pub is_primary_clone: bool,
    pub timeshift_offset: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappedEpgProgram {
    #[serde(flatten)]
    pub original: EpgProgram,
    pub mapped_channel_id: String,
    pub mapped_channel_name: String,
    pub mapped_program_title: String,
    pub mapped_program_description: Option<String>,
    pub mapped_program_category: Option<String>,
    pub mapped_start_time: DateTime<Utc>,
    pub mapped_end_time: DateTime<Utc>,
    pub applied_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingRuleCreateRequest {
    pub name: String,
    pub description: Option<String>,
    pub source_type: DataMappingSourceType,
    pub expression: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingRuleUpdateRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub source_type: Option<DataMappingSourceType>,
    pub expression: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingTestRequest {
    pub source_id: Uuid,
    pub source_type: DataMappingSourceType,
    pub expression: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingTestResult {
    pub is_valid: bool,
    pub error: Option<String>,
    pub matching_channels: Vec<DataMappingTestChannel>,
    pub total_channels: i32,
    pub matched_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingTestChannel {
    pub channel_name: String,
    pub group_title: Option<String>,
    pub original_values: serde_json::Value,
    pub mapped_values: serde_json::Value,
    pub applied_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingTestEpgChannel {
    pub channel_name: String,
    pub channel_group: Option<String>,
    pub original_values: serde_json::Value,
    pub mapped_values: serde_json::Value,
    pub applied_actions: Vec<String>,
    pub clone_group_info: Option<CloneGroupInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneGroupInfo {
    pub clone_group_id: String,
    pub is_primary: bool,
    pub clone_count: i32,
    pub timeshift_offset: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgDataMappingResult {
    pub mapped_channels: Vec<MappedEpgChannel>,
    pub mapped_programs: Vec<MappedEpgProgram>,
    pub clone_groups: std::collections::HashMap<String, Vec<MappedEpgChannel>>,
    pub total_mutations: i32,
    pub channels_affected: i32,
    pub programs_affected: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingPreviewRequest {
    pub source_type: DataMappingSourceType,
    pub source_id: Option<Uuid>,
    pub limit: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingPreviewResponse {
    pub source_type: DataMappingSourceType,
    pub stream_preview: Option<StreamDataMappingPreview>,
    pub epg_preview: Option<EpgDataMappingPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamDataMappingPreview {
    pub total_channels: i32,
    pub affected_channels: i32,
    pub preview_channels: Vec<MappedChannel>,
    pub applied_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgDataMappingPreview {
    pub total_channels: i32,
    pub total_programs: i32,
    pub affected_channels: i32,
    pub affected_programs: i32,
    pub preview_channels: Vec<MappedEpgChannel>,
    pub clone_groups: std::collections::HashMap<String, Vec<MappedEpgChannel>>,
    pub applied_rules: Vec<String>,
}

impl DataMappingRuleCreateRequest {
    pub fn from_expression(
        name: String,
        description: Option<String>,
        source_type: DataMappingSourceType,
        expression: String,
    ) -> Self {
        Self {
            name,
            description,
            source_type,
            expression: Some(expression),
        }
    }

    pub fn validate_expression(&self) -> Result<(), String> {
        if let Some(expression) = &self.expression {
            if expression.trim().is_empty() {
                return Err("Expression cannot be empty".to_string());
            }
            // Add more expression validation logic here as needed
            Ok(())
        } else {
            Err("Expression is required".to_string())
        }
    }
}


impl DataMappingRuleUpdateRequest {
    pub fn validate_expression(&self) -> Result<(), String> {
        if let Some(expression) = &self.expression {
            if expression.trim().is_empty() {
                return Err("Expression cannot be empty".to_string());
            }
            // Add more expression validation logic here as needed
        }
        Ok(())
    }
}
