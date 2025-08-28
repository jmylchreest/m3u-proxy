use crate::models::EpgProgram;
use chrono::{DateTime, Utc};
use sea_orm_migration::sea_query::StringLen;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataMappingRule {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub source_type: DataMappingSourceType,
    pub sort_order: i32,
    pub is_active: bool,
    pub expression: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema, sea_orm::DeriveActiveEnum, strum::EnumIter)]
#[serde(rename_all = "lowercase")]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum DataMappingSourceType {
    #[sea_orm(string_value = "stream")]
    Stream,
    #[sea_orm(string_value = "epg")]
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



pub struct StreamMappingFields;

impl StreamMappingFields {
    pub fn available_fields() -> Vec<&'static str> {
        vec![
            "tvg_id",
            "tvg_name",
            "tvg_logo",
            "tvg_shift",
            "tvg_chno",
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
                    field_name: "tvg_chno".to_string(),
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataMappingRuleCreateRequest {
    pub name: String,
    pub description: Option<String>,
    pub source_type: DataMappingSourceType,
    pub expression: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataMappingRuleUpdateRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub source_type: Option<DataMappingSourceType>,
    pub expression: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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



#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataMappingPreviewRequest {
    pub source_type: DataMappingSourceType,
    pub source_id: Option<Uuid>,
    pub limit: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingPreviewResponse {
    pub source_type: DataMappingSourceType,
    pub stream_preview: Option<StreamDataMappingPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamDataMappingPreview {
    pub total_channels: i32,
    pub affected_channels: i32,
    pub preview_channels: Vec<MappedChannel>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingStats {
    pub total_channels: usize,
    pub channels_modified: usize,
    pub total_rules_processed: usize,
    pub processing_time_ms: u128,
    pub rule_performance: HashMap<String, u128>,
}

impl DataMappingStats {
    /// Get the number of rules with performance data
    pub fn len(&self) -> usize {
        self.rule_performance.len()
    }

    /// Check if performance data is empty
    pub fn is_empty(&self) -> bool {
        self.rule_performance.is_empty()
    }

    /// Iterate over rule performance data
    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, String, u128> {
        self.rule_performance.iter()
    }
}
