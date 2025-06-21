use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DataMappingRule {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub sort_order: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
    pub label_key: Option<String>,
    pub label_value: Option<String>,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum DataMappingActionType {
    #[serde(rename = "set_value")]
    SetValue,
    #[serde(rename = "set_default_if_empty")]
    SetDefaultIfEmpty,
    #[serde(rename = "set_logo")]
    SetLogo,
    #[serde(rename = "set_label")]
    SetLabel,
    #[serde(rename = "transform_value")]
    TransformValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappedChannel {
    #[serde(flatten)]
    pub original: crate::models::Channel,
    pub mapped_tvg_id: Option<String>,
    pub mapped_tvg_name: Option<String>, 
    pub mapped_tvg_logo: Option<String>,
    pub mapped_group_title: Option<String>,
    pub mapped_channel_name: String,
    pub labels: std::collections::HashMap<String, String>,
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
    pub conditions: Vec<DataMappingConditionRequest>,
    pub actions: Vec<DataMappingActionRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingRuleUpdateRequest {
    pub name: String,
    pub description: Option<String>,
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
    pub label_key: Option<String>,
    pub label_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingTestRequest {
    pub source_id: Uuid,
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