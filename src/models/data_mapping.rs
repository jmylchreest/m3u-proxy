use crate::models::{EpgChannel, EpgProgram};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
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

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DataMappingCondition {
    pub id: Uuid,
    pub rule_id: Uuid,
    pub field_name: String,
    pub operator: crate::models::FilterOperator,
    pub value: String,
    pub logical_operator: Option<crate::models::LogicalOperator>,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DataMappingAction {
    pub id: Uuid,
    pub rule_id: Uuid,
    pub action_type: DataMappingActionType,
    pub target_field: String,
    pub value: Option<String>,
    pub logo_asset_id: Option<Uuid>,
    pub timeshift_minutes: Option<i32>, // For timeshift EPG action
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum DataMappingActionType {
    // Shared actions - work on both Stream and EPG sources (but with different fields)
    #[serde(rename = "set_value")]
    SetValue,
    #[serde(rename = "set_default_if_empty")]
    SetDefaultIfEmpty,
    #[serde(rename = "set_logo")]
    SetLogo,
    // Stream-only actions
    #[serde(rename = "timeshift_epg")]
    TimeshiftEpg,
    #[serde(rename = "deduplicate_stream_urls")]
    DeduplicateStreamUrls,
    #[serde(rename = "remove_channel")]
    RemoveChannel,
}

impl DataMappingActionType {
    /// Returns true if this action type is valid for the given source type
    pub fn is_valid_for_source_type(&self, source_type: &DataMappingSourceType) -> bool {
        match (self, source_type) {
            // Shared actions - valid for both source types
            (DataMappingActionType::SetValue, _) => true,
            (DataMappingActionType::SetDefaultIfEmpty, _) => true,
            (DataMappingActionType::SetLogo, _) => true,
            // Stream-only actions
            (DataMappingActionType::TimeshiftEpg, DataMappingSourceType::Stream) => true,
            (DataMappingActionType::DeduplicateStreamUrls, DataMappingSourceType::Stream) => true,
            (DataMappingActionType::RemoveChannel, DataMappingSourceType::Stream) => true,
            // Cross-type restrictions
            (DataMappingActionType::TimeshiftEpg, DataMappingSourceType::Epg) => false,
            (DataMappingActionType::DeduplicateStreamUrls, DataMappingSourceType::Epg) => false,
            (DataMappingActionType::RemoveChannel, DataMappingSourceType::Epg) => false,
        }
    }

    /// Get available action types for a specific source type
    pub fn available_for_source_type(
        source_type: &DataMappingSourceType,
    ) -> Vec<DataMappingActionType> {
        match source_type {
            DataMappingSourceType::Stream => vec![
                DataMappingActionType::SetValue,
                DataMappingActionType::SetDefaultIfEmpty,
                DataMappingActionType::SetLogo,
                DataMappingActionType::DeduplicateStreamUrls, // Deduplicates channels with same stream URL
                DataMappingActionType::RemoveChannel, // Removes channel from output entirely
            ],
            DataMappingSourceType::Epg => vec![
                DataMappingActionType::SetValue,
                DataMappingActionType::SetDefaultIfEmpty,
                DataMappingActionType::SetLogo,
            ],
        }
    }
}

/// Available fields for Stream source data mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamMappingFields;

impl StreamMappingFields {
    pub fn available_fields() -> Vec<&'static str> {
        vec![
            "channel_name",
            "tvg_id",
            "tvg_name",
            "tvg_logo",
            "tvg_shift", // For timeshift channels
            "group_title",
            "stream_url",
        ]
    }

    pub fn is_valid_field(field_name: &str) -> bool {
        Self::available_fields().contains(&field_name)
    }
}

/// Available fields for EPG source data mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    pub fn is_valid_field(field_name: &str) -> bool {
        Self::available_fields().contains(&field_name)
    }
}

/// Field validation for data mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingFieldInfo {
    pub field_name: String,
    pub display_name: String,
    pub field_type: String,
    pub nullable: bool,
    pub source_type: DataMappingSourceType,
}

impl DataMappingFieldInfo {
    /// Get available fields for a specific source type
    pub fn available_for_source_type(
        source_type: &DataMappingSourceType,
    ) -> Vec<DataMappingFieldInfo> {
        match source_type {
            DataMappingSourceType::Stream => vec![
                DataMappingFieldInfo {
                    field_name: "channel_name".to_string(),
                    display_name: "Channel Name".to_string(),
                    field_type: "string".to_string(),
                    nullable: false,
                    source_type: DataMappingSourceType::Stream,
                },
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
                    field_name: "group_title".to_string(),
                    display_name: "Group Title".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: DataMappingSourceType::Stream,
                },
                DataMappingFieldInfo {
                    field_name: "stream_url".to_string(),
                    display_name: "Stream URL".to_string(),
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

    /// Validate if a field is valid for a source type
    pub fn is_valid_field_for_source_type(
        field_name: &str,
        source_type: &DataMappingSourceType,
    ) -> bool {
        match source_type {
            DataMappingSourceType::Stream => StreamMappingFields::is_valid_field(field_name),
            DataMappingSourceType::Epg => EpgMappingFields::is_valid_field(field_name),
        }
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
    pub applied_rules: Vec<Uuid>,
    pub is_removed: bool, // True if channel should be removed from output
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
    pub applied_rules: Vec<Uuid>,
    pub clone_group_id: Option<String>, // For identifying cloned channels
    pub is_primary_clone: bool,         // True for the primary channel in a clone group
    pub timeshift_offset: Option<i32>,  // Timeshift offset in minutes
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
    pub applied_rules: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingRuleWithDetails {
    #[serde(flatten)]
    pub rule: DataMappingRule,
    pub conditions: Vec<DataMappingCondition>,
    pub actions: Vec<DataMappingAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingRuleCreateRequest {
    pub name: String,
    pub description: Option<String>,
    pub source_type: DataMappingSourceType,
    pub conditions: Vec<DataMappingConditionRequest>,
    pub actions: Vec<DataMappingActionRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingRuleUpdateRequest {
    pub name: String,
    pub description: Option<String>,
    pub source_type: DataMappingSourceType,
    pub conditions: Vec<DataMappingConditionRequest>,
    pub actions: Vec<DataMappingActionRequest>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingConditionRequest {
    pub field_name: String,
    pub operator: crate::models::FilterOperator,
    pub value: String,
    pub logical_operator: Option<crate::models::LogicalOperator>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingActionRequest {
    pub action_type: DataMappingActionType,
    pub target_field: String,
    pub value: Option<String>,
    pub logo_asset_id: Option<Uuid>,
    pub timeshift_minutes: Option<i32>, // For timeshift EPG action
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingTestRequest {
    pub source_id: Uuid,
    pub source_type: DataMappingSourceType,
    pub conditions: Vec<DataMappingConditionRequest>,
    pub actions: Vec<DataMappingActionRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingTestResult {
    pub is_valid: bool,
    pub error: Option<String>,
    pub matching_channels: Vec<DataMappingTestChannel>,
    pub total_channels: usize,
    pub matched_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingTestChannel {
    pub channel_name: String,
    pub group_title: Option<String>,
    pub original_values: std::collections::HashMap<String, Option<String>>,
    pub mapped_values: std::collections::HashMap<String, Option<String>>,
    pub applied_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingTestEpgChannel {
    pub channel_name: String,
    pub channel_group: Option<String>,
    pub original_values: std::collections::HashMap<String, Option<String>>,
    pub mapped_values: std::collections::HashMap<String, Option<String>>,
    pub applied_actions: Vec<String>,
    pub clone_group_info: Option<CloneGroupInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneGroupInfo {
    pub clone_group_id: String,
    pub is_primary: bool,
    pub clone_count: usize,
    pub timeshift_offset: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgDataMappingResult {
    pub mapped_channels: Vec<MappedEpgChannel>,
    pub mapped_programs: Vec<MappedEpgProgram>,
    pub clone_groups: std::collections::HashMap<String, Vec<Uuid>>, // clone_group_id -> channel IDs
    pub total_mutations: usize,
    pub channels_affected: usize,
    pub programs_affected: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingPreviewRequest {
    pub source_type: DataMappingSourceType,
    pub source_id: Option<Uuid>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingPreviewResponse {
    pub source_type: DataMappingSourceType,
    pub stream_preview: Option<StreamDataMappingPreview>,
    pub epg_preview: Option<EpgDataMappingPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamDataMappingPreview {
    pub total_channels: usize,
    pub affected_channels: usize,
    pub preview_channels: Vec<DataMappingTestChannel>,
    pub applied_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgDataMappingPreview {
    pub total_channels: usize,
    pub total_programs: usize,
    pub affected_channels: usize,
    pub affected_programs: usize,
    pub preview_channels: Vec<DataMappingTestEpgChannel>,
    pub clone_groups: std::collections::HashMap<String, CloneGroupInfo>,
    pub applied_rules: Vec<String>,
}
