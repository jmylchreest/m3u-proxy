use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

pub mod channel;
pub mod data_mapping;
pub mod epg_source;
pub mod filter;
pub mod linked_xtream;
pub mod logo_asset;
pub mod relay;
pub mod stream_proxy;
pub mod stream_source;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
#[schema(description = "Stream source configuration for M3U playlists or Xtream Codes APIs")]
pub struct StreamSource {
    pub id: Uuid,
    pub name: String,
    pub source_type: StreamSourceType,
    pub url: String,
    pub max_concurrent_streams: i32,
    #[schema(example = "0 0 0 * * * *")]
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub field_map: Option<String>, // JSON string for M3U field mapping
    /// For Xtream sources: ignore channel numbers from API and allow renumbering
    pub ignore_channel_numbers: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_ingested_at: Option<DateTime<Utc>>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, Eq, Hash, ToSchema)]
#[sqlx(type_name = "stream_source_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum StreamSourceType {
    M3u,
    Xtream,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, Eq, Hash)]
#[sqlx(type_name = "stream_proxy_mode", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum StreamProxyMode {
    Redirect,
    Proxy,
    Relay,
}

impl StreamProxyMode {
    /// Parse a string into a StreamProxyMode, defaulting to Redirect for unknown values
    pub fn from_str(s: &str) -> Self {
        match s {
            "redirect" => StreamProxyMode::Redirect,
            "proxy" => StreamProxyMode::Proxy,
            "relay" => StreamProxyMode::Relay,
            _ => StreamProxyMode::Redirect,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct StreamProxy {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_generated_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub auto_regenerate: bool,
    pub proxy_mode: StreamProxyMode,
    pub upstream_timeout: Option<i32>,
    pub buffer_size: Option<i32>,
    pub max_concurrent_streams: Option<i32>,
    pub starting_channel_number: i32,
    #[serde(default = "default_cache_channel_logos")]
    pub cache_channel_logos: bool,
    #[serde(default = "default_cache_program_logos")]
    pub cache_program_logos: bool,
    pub relay_profile_id: Option<Uuid>,
}

fn default_cache_channel_logos() -> bool {
    true
}

fn default_cache_program_logos() -> bool {
    false
}

fn default_update_linked() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProxySource {
    pub proxy_id: Uuid,
    pub source_id: Uuid,
    pub priority_order: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProxyEpgSource {
    pub proxy_id: Uuid,
    pub epg_source_id: Uuid,
    pub priority_order: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(description = "Channel filter with complex condition logic for selecting/excluding channels")]
pub struct Filter {
    pub id: Uuid,
    pub name: String,
    pub source_type: FilterSourceType,
    pub is_inverse: bool,
    pub is_system_default: bool,
    pub expression: String, // Human-readable expression like "channel_name contains \"HD\""
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, ToSchema)]
#[sqlx(type_name = "filter_source_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum FilterSourceType {
    Stream,
    Epg,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProxyFilter {
    pub proxy_id: Uuid,
    pub filter_id: Uuid,
    pub priority_order: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

// Service layer request models for proxy operations
#[derive(Debug, Clone)]
pub struct StreamProxyCreateRequest {
    pub name: String,
    pub description: Option<String>,
    pub proxy_mode: StreamProxyMode,
    pub upstream_timeout: Option<i32>,
    pub buffer_size: Option<i32>,
    pub max_concurrent_streams: Option<i32>,
    pub starting_channel_number: i32,
    pub stream_sources: Vec<ProxySourceCreateRequest>,
    pub epg_sources: Vec<ProxyEpgSourceCreateRequest>,
    pub filters: Vec<ProxyFilterCreateRequest>,
    pub is_active: bool,
    pub auto_regenerate: bool,
    pub cache_channel_logos: bool,
    pub cache_program_logos: bool,
    pub relay_profile_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct StreamProxyUpdateRequest {
    pub name: String,
    pub description: Option<String>,
    pub proxy_mode: StreamProxyMode,
    pub upstream_timeout: Option<i32>,
    pub buffer_size: Option<i32>,
    pub max_concurrent_streams: Option<i32>,
    pub starting_channel_number: i32,
    pub stream_sources: Vec<ProxySourceCreateRequest>,
    pub epg_sources: Vec<ProxyEpgSourceCreateRequest>,
    pub filters: Vec<ProxyFilterCreateRequest>,
    pub is_active: bool,
    pub auto_regenerate: bool,
    pub cache_channel_logos: bool,
    pub cache_program_logos: bool,
    pub relay_profile_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct ProxySourceCreateRequest {
    pub source_id: Uuid,
    pub priority_order: i32,
}

#[derive(Debug, Clone)]
pub struct ProxyEpgSourceCreateRequest {
    pub epg_source_id: Uuid,
    pub priority_order: i32,
}

#[derive(Debug, Clone)]
pub struct ProxyFilterCreateRequest {
    pub filter_id: Uuid,
    pub priority_order: i32,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, PartialEq)]
pub struct Channel {
    pub id: Uuid,
    pub source_id: Uuid,
    pub tvg_id: Option<String>,
    pub tvg_name: Option<String>,
    pub tvg_chno: Option<String>, // Channel number from M3U (e.g., "1", "12")
    pub tvg_logo: Option<String>,
    pub tvg_shift: Option<String>, // Timeshift offset for M3U (e.g., "+1", "+24")
    pub group_title: Option<String>,
    pub channel_name: String,
    pub stream_url: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProxyGeneration {
    pub id: Uuid,
    pub proxy_id: Uuid,
    pub version: i32,
    pub channel_count: i32,
    pub m3u_content: String,
    pub created_at: DateTime<Utc>,
    // New fields for enhanced generation tracking
    pub total_channels: usize,
    pub filtered_channels: usize,
    pub applied_filters: Vec<String>,
    // Comprehensive performance and monitoring stats
    pub stats: Option<GenerationStats>,
    // Add the processed channels for EPG generation
    pub processed_channels: Option<Vec<NumberedChannel>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct M3uFieldMap {
    pub channel_name_field: Option<String>,
    pub group_field: Option<String>,
    pub logo_field: Option<String>,
    pub tvg_id_field: Option<String>,
    pub tvg_name_field: Option<String>,
}

impl Default for M3uFieldMap {
    fn default() -> Self {
        Self {
            channel_name_field: None,
            group_field: Some("group-title".to_string()),
            logo_field: Some("tvg-logo".to_string()),
            tvg_id_field: Some("tvg-id".to_string()),
            tvg_name_field: Some("tvg-name".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSourceCreateRequest {
    pub name: String,
    pub source_type: StreamSourceType,
    pub url: String,
    pub max_concurrent_streams: i32,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub field_map: Option<String>,
    pub ignore_channel_numbers: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSourceUpdateRequest {
    pub name: String,
    pub source_type: StreamSourceType,
    pub url: String,
    pub max_concurrent_streams: i32,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub field_map: Option<String>,
    pub ignore_channel_numbers: bool,
    pub is_active: bool,
    /// Whether to update linked sources with the same URL (defaults to true)
    #[serde(default = "default_update_linked")]
    pub update_linked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshResponse {
    pub success: bool,
    pub message: String,
    pub channel_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSourceWithStats {
    #[serde(flatten)]
    pub source: StreamSource,
    pub channel_count: i64,
    pub next_scheduled_update: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionProgress {
    pub source_id: Uuid,
    pub state: IngestionState,
    pub progress: ProgressInfo,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IngestionState {
    Idle,
    Connecting,
    Downloading,
    Parsing,
    Saving,
    Processing,
    Completed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressInfo {
    pub current_step: String,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: Option<u64>,
    pub channels_parsed: Option<usize>,
    pub channels_saved: Option<usize>,
    pub programs_parsed: Option<usize>,
    pub programs_saved: Option<usize>,
    pub percentage: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelListRequest {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub filter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelListResponse {
    pub channels: Vec<Channel>,
    pub total_count: i64,
    pub page: u32,
    pub limit: u32,
    pub total_pages: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FilterCreateRequest {
    pub name: String,
    pub source_type: FilterSourceType,
    pub is_inverse: bool,
    pub expression: String, // Human-readable expression like "channel_name contains \"HD\""
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FilterUpdateRequest {
    pub name: String,
    pub source_type: FilterSourceType,
    pub is_inverse: bool,
    pub expression: String, // Human-readable expression like "channel_name contains \"HD\""
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FilterWithUsage {
    #[serde(flatten)]
    pub filter: Filter,
    pub usage_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FilterTestRequest {
    pub source_id: Uuid,
    pub source_type: FilterSourceType,
    pub filter_expression: String, // Raw text expression like "(A OR B) AND C"
    pub is_inverse: bool,
}

// New generalized models
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExpressionValidateRequest {
    #[schema(example = "channel_name contains \"HD\" OR (group_title = \"Movies\" AND stream_url starts_with \"https\")")]
    pub expression: String,
}


#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FilterTestResult {
    pub is_valid: bool,
    pub error: Option<String>,
    pub matching_channels: Vec<FilterTestChannel>,
    pub total_channels: usize,
    pub matched_count: usize,
    pub expression_tree: Option<serde_json::Value>,
}

// New generalized error category
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExpressionErrorCategory {
    Syntax,   // General syntax issues, unclosed parentheses, missing operators
    Field,    // Invalid or unknown field names
    Operator, // Invalid or unknown operators/modifiers
    Value,    // Invalid values, unparseable regex, type mismatches
}


// New generalized validation error
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExpressionValidationError {
    #[schema(example = "field")]
    pub category: ExpressionErrorCategory,
    
    #[schema(example = "unknown_field")]
    pub error_type: String,
    
    #[schema(example = "Unknown field 'channe_name'")]
    pub message: String,
    
    #[schema(example = "Field 'channe_name' is not available. Did you mean 'channel_name'?")]
    pub details: Option<String>,
    
    #[schema(example = 25)]
    pub position: Option<usize>,
    
    #[schema(example = "channe_name contains")]
    pub context: Option<String>,
    
    #[schema(example = "Available fields: channel_name, group_title, stream_url, tvg_id, tvg_name, tvg_logo")]
    pub suggestion: Option<String>,
}


// New generalized validation result
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExpressionValidateResult {
    #[schema(example = true)]
    pub is_valid: bool,
    
    #[schema(example = json!([
        {
            "category": "field",
            "error_type": "unknown_field",
            "message": "Unknown field 'channe_name'",
            "details": "Field 'channe_name' is not available. Did you mean 'channel_name'?",
            "position": 0,
            "context": "channe_name contains",
            "suggestion": "Available fields: channel_name, group_title, stream_url, tvg_id, tvg_name, tvg_logo"
        },
        {
            "category": "operator",
            "error_type": "unknown_operator",
            "message": "Unknown operator 'containz'",
            "details": "Operator 'containz' is not supported. Did you mean 'contains'?",
            "position": 13,
            "context": "channel_name containz",
            "suggestion": "Available operators: contains, starts_with, ends_with, equals, not_equals, matches_regex"
        },
        {
            "category": "value",
            "error_type": "invalid_regex",
            "message": "Invalid regular expression",
            "details": "Regex pattern '[unclosed' has unclosed bracket",
            "position": 35,
            "context": "matches_regex '[unclosed'",
            "suggestion": "Use valid regex syntax: channel_name matches_regex '^[a-zA-Z]+$'"
        },
        {
            "category": "syntax",
            "error_type": "unclosed_parentheses",
            "message": "Unclosed parentheses",
            "details": "Opening parenthesis at position 45 is never closed",
            "position": 45,
            "context": "(group_title equals",
            "suggestion": "Add closing parenthesis: (group_title equals \"value\")"
        }
    ]))]
    pub errors: Vec<ExpressionValidationError>,
    
    #[schema(example = json!({
        "type": "condition",
        "field": "channel_name", 
        "operator": "Contains",
        "value": "Sports",
        "case_sensitive": false,
        "negate": false
    }))]
    pub expression_tree: Option<serde_json::Value>,
}


#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FilterTestChannel {
    pub channel_name: String,
    pub group_title: Option<String>,
    pub matched_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterField {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum FilterOperator {
    #[serde(rename = "matches")]
    Matches, // Regex match
    #[serde(rename = "equals")]
    Equals, // Exact match
    #[serde(rename = "contains")]
    Contains, // Contains substring
    #[serde(rename = "starts_with")]
    StartsWith, // Starts with
    #[serde(rename = "ends_with")]
    EndsWith, // Ends with
    #[serde(rename = "not_matches")]
    NotMatches, // Does not match regex
    #[serde(rename = "not_equals")]
    NotEquals, // Does not equal
    #[serde(rename = "not_contains")]
    NotContains, // Does not contain
    #[serde(rename = "not_starts_with")]
    NotStartsWith, // Does not start with
    #[serde(rename = "not_ends_with")]
    NotEndsWith, // Does not end with
    #[serde(rename = "greater_than")]
    GreaterThan, // Greater than (numeric/datetime comparison)
    #[serde(rename = "less_than")]
    LessThan, // Less than (numeric/datetime comparison)
    #[serde(rename = "greater_than_or_equal")]
    GreaterThanOrEqual, // Greater than or equal (numeric/datetime comparison)
    #[serde(rename = "less_than_or_equal")]
    LessThanOrEqual, // Less than or equal (numeric/datetime comparison)
}

impl std::fmt::Display for FilterOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterOperator::Matches => write!(f, "matches"),
            FilterOperator::Equals => write!(f, "equals"),
            FilterOperator::Contains => write!(f, "contains"),
            FilterOperator::StartsWith => write!(f, "starts_with"),
            FilterOperator::EndsWith => write!(f, "ends_with"),
            FilterOperator::NotMatches => write!(f, "not_matches"),
            FilterOperator::NotEquals => write!(f, "not_equals"),
            FilterOperator::NotContains => write!(f, "not_contains"),
            FilterOperator::NotStartsWith => write!(f, "not_starts_with"),
            FilterOperator::NotEndsWith => write!(f, "not_ends_with"),
            FilterOperator::GreaterThan => write!(f, "greater_than"),
            FilterOperator::LessThan => write!(f, "less_than"),
            FilterOperator::GreaterThanOrEqual => write!(f, "greater_than_or_equal"),
            FilterOperator::LessThanOrEqual => write!(f, "less_than_or_equal"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum LogicalOperator {
    #[serde(rename = "and")]
    And,
    #[serde(rename = "or")]
    Or,
}

impl std::fmt::Display for LogicalOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogicalOperator::And => write!(f, "and"),
            LogicalOperator::Or => write!(f, "or"),
        }
    }
}

impl LogicalOperator {
    /// Checks if this is an AND-like operator
    pub fn is_and_like(&self) -> bool {
        matches!(self, LogicalOperator::And)
    }
}

// Tree-based condition structures for nested expressions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ConditionNode {
    #[serde(rename = "condition")]
    Condition {
        field: String,
        operator: FilterOperator,
        value: String,
        #[serde(default)]
        case_sensitive: bool,
        #[serde(default)]
        negate: bool,
    },
    #[serde(rename = "group")]
    Group {
        operator: LogicalOperator,
        children: Vec<ConditionNode>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionTree {
    pub root: ConditionNode,
}

// Extended expression support for action syntax
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ExtendedExpression {
    #[serde(rename = "condition_only")]
    ConditionOnly(ConditionTree),
    #[serde(rename = "condition_with_actions")]
    ConditionWithActions {
        condition: ConditionTree,
        actions: Vec<Action>,
    },
    #[serde(rename = "conditional_action_groups")]
    ConditionalActionGroups(Vec<ConditionalActionGroup>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionalActionGroup {
    pub conditions: ConditionTree,
    pub actions: Vec<Action>,
    pub logical_operator: Option<LogicalOperator>, // AND/OR with next group
}

// Individual action within a SET clause
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub field: String,
    pub operator: ActionOperator,
    pub value: ActionValue,
}

/// Action operators for data mapping rules
///
/// These operators define how values are modified in data mapping rules:
///
/// # Examples
///
/// ```
/// // Basic assignment
/// SET tvg-name="Sports Channel"
///
/// // Delete field (removes from output)
/// DELETE tvg-channo
///
/// // Set to null (explicit null value)
/// SET tvg-logo=null
///
/// // Conditional assignment (only if field is empty)
/// SET tvg-id?="auto-generated-id"
///
/// // Append to existing value
/// SET group-title+=" HD"
///
/// // Remove substring
/// SET channel-name-="[HD]"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum ActionOperator {
    /// Set field to new value (overwrites existing value)
    /// Syntax: `SET field="value"` or `field="value"`
    #[serde(rename = "set")]
    Set,

    /// Set field only if current value is empty or null
    /// Syntax: `SET field?="value"` or `field?="value"`
    #[serde(rename = "set_if_empty")]
    SetIfEmpty,

    /// Append value to existing field (with space separator)
    /// Syntax: `SET field+="value"` or `field+="value"`
    #[serde(rename = "append")]
    Append,

    /// Remove substring from existing field
    /// Syntax: `SET field-="substring"` or `field-="substring"`
    #[serde(rename = "remove")]
    Remove,

    /// Delete field entirely (removes from output)
    /// Syntax: `DELETE field`
    #[serde(rename = "delete")]
    Delete,
}

/// Values that can be assigned in data mapping actions
///
/// # Examples
///
/// ```
/// // Literal string value
/// ActionValue::Literal("Sports HD".to_string())
///
/// // Explicit null value (clears field)
/// ActionValue::Null
///
/// // Future: Function call
/// ActionValue::Function(FunctionCall { name: "upper".to_string(), arguments: vec![] })
///
/// // Future: Variable reference
/// ActionValue::Variable(VariableRef { field_name: "tvg_id".to_string() })
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ActionValue {
    /// Literal string value
    #[serde(rename = "literal")]
    Literal(String),

    /// Null/empty value (clears the field)
    #[serde(rename = "null")]
    Null,

    /// Function call (future feature)
    #[serde(rename = "function")]
    Function(FunctionCall),

    /// Variable reference (future feature)
    #[serde(rename = "variable")]
    Variable(VariableRef),
}

// Future: Function call support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: Vec<ActionValue>,
}

// Future: Variable reference support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableRef {
    pub field_name: String,
}

impl Action {
    /// Validate the action for correctness
    ///
    /// # Validation Rules
    ///
    /// - DELETE operator cannot be used with any value
    /// - Required fields (like channel_name) cannot be deleted
    /// - SET operator requires a value
    /// - Field names must be valid for the source type
    pub fn validate(
        &self,
        source_type: &data_mapping::DataMappingSourceType,
    ) -> Result<(), String> {
        use crate::models::data_mapping::{
            DataMappingSourceType, EpgMappingFields, StreamMappingFields,
        };

        // Validate field name
        let valid_fields = match source_type {
            DataMappingSourceType::Stream => StreamMappingFields::available_fields(),
            DataMappingSourceType::Epg => EpgMappingFields::available_fields(),
        };

        if !valid_fields.contains(&self.field.as_str()) {
            return Err(format!(
                "Invalid field '{}' for source type {}",
                self.field, source_type
            ));
        }

        // Validate operator-value combinations
        match (&self.operator, &self.value) {
            (ActionOperator::Delete, _) => {
                // Cannot delete required fields
                if matches!(self.field.as_str(), "channel_name") {
                    return Err("Cannot delete required field 'channel_name'".to_string());
                }
                Ok(())
            }
            (ActionOperator::Set, ActionValue::Null) => Ok(()),
            (ActionOperator::Set, ActionValue::Literal(_)) => Ok(()),
            (ActionOperator::SetIfEmpty, ActionValue::Literal(_)) => Ok(()),
            (ActionOperator::Append, ActionValue::Literal(_)) => Ok(()),
            (ActionOperator::Remove, ActionValue::Literal(_)) => Ok(()),
            _ => Err(format!(
                "Invalid combination of operator {:?} with value {:?}",
                self.operator, self.value
            )),
        }
    }

    /// Get documentation for this action
    pub fn get_documentation(&self) -> String {
        match &self.operator {
            ActionOperator::Set => format!("Set {} to new value", self.field),
            ActionOperator::SetIfEmpty => format!("Set {} only if currently empty", self.field),
            ActionOperator::Append => format!("Append to {} with space separator", self.field),
            ActionOperator::Remove => format!("Remove substring from {}", self.field),
            ActionOperator::Delete => format!("Delete {} field entirely", self.field),
        }
    }
}

impl Filter {
    /// Check if this filter uses the new tree-based condition structure
    pub fn uses_condition_tree(&self) -> bool {
        true // Always uses condition_tree now
    }

    /// Parse the condition tree from JSON
    pub fn get_condition_tree(&self) -> Option<ConditionTree> {
        if self.expression.trim().is_empty() {
            return None;
        }
        
        // Parse expression to condition tree
        let parser = crate::expression_parser::ExpressionParser::new();
        match parser.parse(&self.expression) {
            Ok(tree) => {
                if self.name.contains("Adult") {
                    tracing::debug!(
                        "Successfully parsed condition tree for filter '{}'",
                        self.name
                    );
                }
                Some(tree)
            }
            Err(e) => {
                tracing::error!(
                    "Failed to parse expression for filter '{}': {}",
                    self.name,
                    e
                );
                tracing::error!("Raw expression: {}", self.expression);
                None
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FilterFieldInfo {
    pub name: String,
    pub display_name: String,
    pub field_type: String,
    pub nullable: bool,
    pub source_type: FilterSourceType,
}

impl FilterFieldInfo {
    /// Get available fields for a specific filter source type
    pub fn available_for_source_type(source_type: &FilterSourceType) -> Vec<FilterFieldInfo> {
        match source_type {
            FilterSourceType::Stream => vec![
                FilterFieldInfo {
                    name: "channel_name".to_string(),
                    display_name: "Channel Name".to_string(),
                    field_type: "string".to_string(),
                    nullable: false,
                    source_type: FilterSourceType::Stream,
                },
                FilterFieldInfo {
                    name: "tvg_id".to_string(),
                    display_name: "TVG ID".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: FilterSourceType::Stream,
                },
                FilterFieldInfo {
                    name: "tvg_name".to_string(),
                    display_name: "TVG Name".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: FilterSourceType::Stream,
                },
                FilterFieldInfo {
                    name: "tvg_logo".to_string(),
                    display_name: "TVG Logo".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: FilterSourceType::Stream,
                },
                FilterFieldInfo {
                    name: "group_title".to_string(),
                    display_name: "Group Title".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: FilterSourceType::Stream,
                },
                FilterFieldInfo {
                    name: "stream_url".to_string(),
                    display_name: "Stream URL".to_string(),
                    field_type: "string".to_string(),
                    nullable: false,
                    source_type: FilterSourceType::Stream,
                },
            ],
            FilterSourceType::Epg => vec![
                FilterFieldInfo {
                    name: "channel_id".to_string(),
                    display_name: "EPG Channel ID".to_string(),
                    field_type: "string".to_string(),
                    nullable: false,
                    source_type: FilterSourceType::Epg,
                },
                FilterFieldInfo {
                    name: "channel_name".to_string(),
                    display_name: "EPG Channel Name".to_string(),
                    field_type: "string".to_string(),
                    nullable: false,
                    source_type: FilterSourceType::Epg,
                },
                FilterFieldInfo {
                    name: "channel_logo".to_string(),
                    display_name: "EPG Channel Logo".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: FilterSourceType::Epg,
                },
                FilterFieldInfo {
                    name: "channel_group".to_string(),
                    display_name: "EPG Channel Group".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: FilterSourceType::Epg,
                },
                FilterFieldInfo {
                    name: "language".to_string(),
                    display_name: "EPG Language".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: FilterSourceType::Epg,
                },
                FilterFieldInfo {
                    name: "program_title".to_string(),
                    display_name: "Program Title".to_string(),
                    field_type: "string".to_string(),
                    nullable: false,
                    source_type: FilterSourceType::Epg,
                },
                FilterFieldInfo {
                    name: "program_category".to_string(),
                    display_name: "Program Category".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: FilterSourceType::Epg,
                },
                FilterFieldInfo {
                    name: "program_description".to_string(),
                    display_name: "Program Description".to_string(),
                    field_type: "string".to_string(),
                    nullable: true,
                    source_type: FilterSourceType::Epg,
                },
            ],
        }
    }

    /// Validate if a field is valid for a filter source type
    pub fn is_valid_field_for_source_type(
        field_name: &str,
        source_type: &FilterSourceType,
    ) -> bool {
        Self::available_for_source_type(source_type)
            .iter()
            .any(|field| field.name == field_name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyFilterWithDetails {
    #[serde(flatten)]
    pub proxy_filter: ProxyFilter,
    pub filter: Filter,
}

// Channel numbering data structures
use std::collections::{BTreeMap, HashSet};

/// State tracking for sophisticated channel numbering algorithm
#[derive(Debug, Clone)]
pub struct ChannelNumberingState {
    pub reserved_numbers: HashSet<i32>,
    pub explicit_assignments: BTreeMap<i32, Vec<Channel>>, // channo -> channels
    pub sequential_channels: Vec<Channel>,
}

/// Channel with assigned number and assignment type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumberedChannel {
    pub channel: Channel,
    pub assigned_number: i32,
    pub assignment_type: ChannelNumberAssignmentType,
}

/// How a channel number was assigned
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChannelNumberAssignmentType {
    /// Had explicit tvg-channo from data mapping
    Explicit,
    /// Had tvg-channo but incremented due to conflict
    ExplicitIncremented,
    /// Assigned sequentially
    Sequential,
}

impl NumberedChannel {
    /// Get a description of how this channel number was assigned
    pub fn assignment_description(&self) -> String {
        match self.assignment_type {
            ChannelNumberAssignmentType::Explicit => format!(
                "Explicit assignment (data mapping specified {})",
                self.assigned_number
            ),
            ChannelNumberAssignmentType::ExplicitIncremented => {
                format!("Incremented due to conflict (original + offset)")
            }
            ChannelNumberAssignmentType::Sequential => format!("Sequential assignment"),
        }
    }
}

/// Timing information for proxy generation steps
#[derive(Debug, Clone)]
pub struct GenerationTiming {
    pub total_duration_ms: u128,
    pub source_loading_ms: u128,
    pub data_mapping_ms: u128,
    pub filter_application_ms: u128,
    pub channel_numbering_ms: u128,
    pub m3u_generation_ms: u128,
    pub file_writing_ms: u128,
    pub database_save_ms: u128,
}

impl GenerationTiming {
    pub fn new() -> Self {
        Self {
            total_duration_ms: 0,
            source_loading_ms: 0,
            data_mapping_ms: 0,
            filter_application_ms: 0,
            channel_numbering_ms: 0,
            m3u_generation_ms: 0,
            file_writing_ms: 0,
            database_save_ms: 0,
        }
    }

    /// Log comprehensive timing statistics
    pub fn log_statistics(&self, proxy_name: &str, channel_count: usize) {
        use tracing::info;
        info!(
            "Generation completed for '{}': {} channels in {}ms | Steps: source_load={}ms data_map={}ms filter={}ms numbering={}ms m3u_gen={}ms file_write={}ms db_save={}ms",
            proxy_name,
            channel_count,
            self.total_duration_ms,
            self.source_loading_ms,
            self.data_mapping_ms,
            self.filter_application_ms,
            self.channel_numbering_ms,
            self.m3u_generation_ms,
            self.file_writing_ms,
            self.database_save_ms
        );
    }
}

// EPG Source Models
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct EpgSource {
    pub id: Uuid,
    pub name: String,
    pub source_type: EpgSourceType,
    pub url: String,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub original_timezone: Option<String>,
    pub time_offset: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_ingested_at: Option<DateTime<Utc>>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, ToSchema)]
#[sqlx(type_name = "epg_source_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum EpgSourceType {
    Xmltv,
    Xtream,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct EpgProgram {
    pub id: Uuid,
    pub source_id: Uuid,
    pub channel_id: String,
    pub channel_name: String,
    pub program_title: String,
    pub program_description: Option<String>,
    pub program_category: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub episode_num: Option<String>,
    pub season_num: Option<String>,
    pub rating: Option<String>,
    pub language: Option<String>,
    pub subtitles: Option<String>,
    pub aspect_ratio: Option<String>,
    pub program_icon: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgSourceCreateRequest {
    pub name: String,
    pub source_type: EpgSourceType,
    pub url: String,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub timezone: Option<String>,
    pub time_offset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgSourceUpdateRequest {
    pub name: String,
    pub source_type: EpgSourceType,
    pub url: String,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub timezone: Option<String>,
    pub time_offset: Option<String>,
    pub is_active: bool,
    /// Whether to update linked sources with the same URL (defaults to true)
    #[serde(default = "default_update_linked")]
    pub update_linked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgSourceWithStats {
    #[serde(flatten)]
    pub source: EpgSource,
    pub channel_count: i64,
    pub program_count: i64,
    pub next_scheduled_update: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgViewerRequest {
    pub start_time: String, // ISO 8601 string for URL query parsing
    pub end_time: String,   // ISO 8601 string for URL query parsing
    pub channel_filter: Option<String>,
    pub source_ids: Option<Vec<Uuid>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgViewerRequestParsed {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub channel_filter: Option<String>,
    pub source_ids: Option<Vec<Uuid>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgViewerResponse {
    pub programs: Vec<EpgProgram>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgRefreshResponse {
    pub success: bool,
    pub message: String,
    pub channel_count: usize,
    pub program_count: usize,
}



// Unified Source API Models
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "source_kind")]
pub enum UnifiedSource {
    #[serde(rename = "stream")]
    Stream {
        #[serde(flatten)]
        source: StreamSource,
        // Stream-specific fields
        max_concurrent_streams: i32,
        field_map: Option<String>,
    },
    #[serde(rename = "epg")]
    Epg {
        #[serde(flatten)]
        source: EpgSourceBase,
        // EPG-specific fields
        original_timezone: Option<String>,
        time_offset: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgSourceBase {
    pub id: Uuid,
    pub name: String,
    pub source_type: EpgSourceType,
    pub url: String,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_ingested_at: Option<DateTime<Utc>>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "source_kind")]
pub enum UnifiedSourceWithStats {
    #[serde(rename = "stream")]
    Stream {
        #[serde(flatten)]
        source: StreamSource,
        // Stream-specific fields
        max_concurrent_streams: i32,
        field_map: Option<String>,
        // Stream stats
        channel_count: i64,
        next_scheduled_update: Option<DateTime<Utc>>,
    },
    #[serde(rename = "epg")]
    Epg {
        #[serde(flatten)]
        source: EpgSourceBase,
        // EPG-specific fields
        original_timezone: Option<String>,
        time_offset: String,
        // EPG stats
        channel_count: i64,
        program_count: i64,
        next_scheduled_update: Option<DateTime<Utc>>,
    },
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayStatus {
    pub config_id: Uuid,
    pub proxy_id: Uuid,
    pub is_running: bool,
    pub pid: Option<u32>,
    pub port: Option<u16>,
    pub started_at: Option<DateTime<Utc>>,
    pub client_count: i32,
    pub bytes_served: u64,
    pub error_message: Option<String>,
}

impl UnifiedSourceWithStats {
    pub fn from_stream(stream_with_stats: StreamSourceWithStats) -> Self {
        Self::Stream {
            max_concurrent_streams: stream_with_stats.source.max_concurrent_streams,
            field_map: stream_with_stats.source.field_map.clone(),
            source: StreamSource {
                id: stream_with_stats.source.id,
                name: stream_with_stats.source.name,
                source_type: stream_with_stats.source.source_type,
                url: stream_with_stats.source.url,
                max_concurrent_streams: stream_with_stats.source.max_concurrent_streams,
                update_cron: stream_with_stats.source.update_cron,
                username: stream_with_stats.source.username,
                password: stream_with_stats.source.password,
                field_map: stream_with_stats.source.field_map,
                ignore_channel_numbers: stream_with_stats.source.ignore_channel_numbers,
                created_at: stream_with_stats.source.created_at,
                updated_at: stream_with_stats.source.updated_at,
                last_ingested_at: stream_with_stats.source.last_ingested_at,
                is_active: stream_with_stats.source.is_active,
            },
            channel_count: stream_with_stats.channel_count,
            next_scheduled_update: stream_with_stats.next_scheduled_update,
        }
    }

    pub fn from_epg(epg_with_stats: EpgSourceWithStats) -> Self {
        Self::Epg {
            original_timezone: epg_with_stats.source.original_timezone.clone(),
            time_offset: epg_with_stats.source.time_offset.clone(),
            source: EpgSourceBase {
                id: epg_with_stats.source.id,
                name: epg_with_stats.source.name,
                source_type: epg_with_stats.source.source_type,
                url: epg_with_stats.source.url,
                update_cron: epg_with_stats.source.update_cron,
                username: epg_with_stats.source.username,
                password: epg_with_stats.source.password,
                created_at: epg_with_stats.source.created_at,
                updated_at: epg_with_stats.source.updated_at,
                last_ingested_at: epg_with_stats.source.last_ingested_at,
                is_active: epg_with_stats.source.is_active,
            },
            channel_count: epg_with_stats.channel_count,
            program_count: epg_with_stats.program_count,
            next_scheduled_update: epg_with_stats.next_scheduled_update,
        }
    }

    pub fn get_id(&self) -> Uuid {
        match self {
            Self::Stream { source, .. } => source.id,
            Self::Epg { source, .. } => source.id,
        }
    }

    pub fn get_name(&self) -> &str {
        match self {
            Self::Stream { source, .. } => &source.name,
            Self::Epg { source, .. } => &source.name,
        }
    }

    pub fn is_stream(&self) -> bool {
        matches!(self, Self::Stream { .. })
    }

    pub fn is_epg(&self) -> bool {
        matches!(self, Self::Epg { .. })
    }
}

// New dependency injection structures for generator refactor

/// Complete proxy configuration resolved from database
/// This eliminates the need for database queries during generation
#[derive(Debug, Clone)]
pub struct ResolvedProxyConfig {
    pub proxy: StreamProxy,
    pub sources: Vec<ProxySourceConfig>,
    pub filters: Vec<ProxyFilterConfig>,
    pub epg_sources: Vec<ProxyEpgSourceConfig>,
}

/// Stream source configuration with priority
#[derive(Debug, Clone)]
pub struct ProxySourceConfig {
    pub source: StreamSource,
    pub priority_order: i32,
}

/// Filter configuration with metadata
#[derive(Debug, Clone)]
pub struct ProxyFilterConfig {
    pub filter: Filter,
    pub priority_order: i32,
    pub is_active: bool,
}

/// EPG source configuration with priority
#[derive(Debug, Clone)]
pub struct ProxyEpgSourceConfig {
    pub epg_source: EpgSource,
    pub priority_order: i32,
}

/// Output destination abstraction for generation
#[derive(Clone, Debug)]
pub enum GenerationOutput {
    /// Preview mode - writes to preview file manager
    Preview {
        file_manager: sandboxed_file_manager::SandboxedManager,
        proxy_name: String,
    },
    /// Production mode - writes to proxy output file manager
    Production {
        file_manager: sandboxed_file_manager::SandboxedManager,
        update_database: bool,
    },
    /// In-memory mode - returns content only (for testing)
    InMemory,
}

impl GenerationOutput {
    pub fn is_preview(&self) -> bool {
        matches!(self, Self::Preview { .. })
    }

    pub fn is_production(&self) -> bool {
        matches!(self, Self::Production { .. })
    }

    pub fn should_update_database(&self) -> bool {
        match self {
            Self::Production {
                update_database, ..
            } => *update_database,
            _ => false,
        }
    }
}

/// Comprehensive generation statistics for performance monitoring and UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationStats {
    /// Overall timing
    pub total_duration_ms: u64,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,

    /// Stage-wise performance breakdown
    pub stage_timings: std::collections::HashMap<String, u64>, // stage_name -> duration_ms
    pub stage_memory_usage: std::collections::HashMap<String, u64>, // stage_name -> peak_memory_bytes

    /// Channel processing metrics
    pub total_channels_processed: usize,
    pub channels_per_second: f64,
    pub average_channel_processing_ms: f64,

    /// Source processing metrics
    pub sources_processed: usize,
    pub channels_by_source: std::collections::HashMap<String, usize>, // source_name -> channel_count
    pub source_processing_times: std::collections::HashMap<String, u64>, // source_name -> duration_ms

    /// Filter application metrics
    pub filters_applied: Vec<String>,
    pub filter_processing_times: std::collections::HashMap<String, u64>, // filter_name -> duration_ms
    pub channels_before_filtering: usize,
    pub channels_after_filtering: usize,
    pub channels_filtered_out: usize,

    /// Memory metrics
    pub peak_memory_usage_mb: Option<f64>,
    pub average_memory_usage_mb: Option<f64>,
    pub memory_efficiency: Option<f64>, // channels_per_mb
    pub gc_collections: Option<usize>,

    /// Data mapping metrics
    pub data_mapping_duration_ms: u64,
    pub channels_mapped: usize,
    pub mapping_transformations_applied: usize,

    /// Channel numbering metrics
    pub channel_numbering_duration_ms: u64,
    pub numbering_strategy: String,
    pub number_conflicts_resolved: usize,

    /// M3U generation metrics
    pub m3u_generation_duration_ms: u64,
    pub m3u_size_bytes: usize,
    pub m3u_lines_generated: usize,

    /// Error and warning tracking
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub recoverable_errors: usize,

    /// Pipeline-specific metrics
    pub pipeline_type: String, // "standard", "chunked", "adaptive", etc.
    pub memory_pressure_events: usize,
    pub spill_to_disk_events: usize,
    pub temp_files_created: usize,
}

impl GenerationStats {
    pub fn new(pipeline_type: String) -> Self {
        let now = Utc::now();
        Self {
            total_duration_ms: 0,
            started_at: now,
            completed_at: now,
            stage_timings: std::collections::HashMap::new(),
            stage_memory_usage: std::collections::HashMap::new(),
            total_channels_processed: 0,
            channels_per_second: 0.0,
            average_channel_processing_ms: 0.0,
            sources_processed: 0,
            channels_by_source: std::collections::HashMap::new(),
            source_processing_times: std::collections::HashMap::new(),
            filters_applied: Vec::new(),
            filter_processing_times: std::collections::HashMap::new(),
            channels_before_filtering: 0,
            channels_after_filtering: 0,
            channels_filtered_out: 0,
            peak_memory_usage_mb: None,
            average_memory_usage_mb: None,
            memory_efficiency: None,
            gc_collections: None,
            data_mapping_duration_ms: 0,
            channels_mapped: 0,
            mapping_transformations_applied: 0,
            channel_numbering_duration_ms: 0,
            numbering_strategy: "sequential".to_string(),
            number_conflicts_resolved: 0,
            m3u_generation_duration_ms: 0,
            m3u_size_bytes: 0,
            m3u_lines_generated: 0,
            warnings: Vec::new(),
            errors: Vec::new(),
            recoverable_errors: 0,
            pipeline_type,
            memory_pressure_events: 0,
            spill_to_disk_events: 0,
            temp_files_created: 0,
        }
    }

    /// Add timing for a stage
    pub fn add_stage_timing(&mut self, stage: &str, duration_ms: u64) {
        self.stage_timings.insert(stage.to_string(), duration_ms);
    }

    /// Add memory usage for a stage
    pub fn add_stage_memory(&mut self, stage: &str, memory_bytes: u64) {
        self.stage_memory_usage
            .insert(stage.to_string(), memory_bytes);
    }

    /// Finalize stats and calculate derived metrics
    pub fn finalize(&mut self) {
        self.completed_at = Utc::now();

        // Only calculate total_duration_ms if it hasn't been set manually
        if self.total_duration_ms == 0 {
            self.total_duration_ms =
                (self.completed_at - self.started_at).num_milliseconds() as u64;
        }

        // Calculate channels per second
        if self.total_duration_ms > 0 {
            self.channels_per_second =
                (self.total_channels_processed as f64) / (self.total_duration_ms as f64 / 1000.0);
        }

        // Calculate average channel processing time
        if self.total_channels_processed > 0 {
            self.average_channel_processing_ms =
                (self.total_duration_ms as f64) / (self.total_channels_processed as f64);
        }

        // Calculate channels filtered out
        self.channels_filtered_out = self
            .channels_before_filtering
            .saturating_sub(self.channels_after_filtering);

        // Calculate memory efficiency
        if let Some(peak_memory) = self.peak_memory_usage_mb {
            if peak_memory > 0.0 {
                self.memory_efficiency = Some(self.total_channels_processed as f64 / peak_memory);
            }
        }
    }

    /// Generate a concise summary string for logging with tree-style stage breakdown
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();

        // Top-level summary line
        lines.push(format!(
            "Generation completed in {}ms: {} channels ({} sources, {} filters) | {:.1} ch/s | Peak: {:.1}MB | Pipeline: {}",
            self.total_duration_ms,
            self.total_channels_processed,
            self.sources_processed,
            self.filters_applied.len(),
            self.channels_per_second,
            self.peak_memory_usage_mb.unwrap_or(0.0),
            self.pipeline_type
        ));

        // Stage-by-stage breakdown with tree-style formatting
        if !self.stage_timings.is_empty() {
            // Define stage order for consistent reporting
            let stage_order = [
                ("source_loading", "Source Loading"),
                ("data_mapping", "Data Mapping"),
                ("filtering", "Filtering"),
                ("channel_numbering", "Channel Numbering"),
                ("m3u_generation", "M3U Generation"),
            ];

            // Collect stages that exist in order
            let mut existing_stages = Vec::new();
            for (stage_key, stage_name) in &stage_order {
                if self.stage_timings.contains_key(*stage_key) {
                    existing_stages.push((stage_key.to_string(), stage_name.to_string()));
                }
            }

            // Add any additional stages not in the standard order
            for stage in self.stage_timings.keys() {
                if !stage_order.iter().any(|(key, _)| *key == stage) {
                    let stage_display = stage
                        .replace("_", " ")
                        .split(' ')
                        .map(|word| {
                            format!(
                                "{}{}",
                                word.chars().next().unwrap().to_uppercase(),
                                &word[1..]
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    existing_stages.push((stage.clone(), stage_display));
                }
            }

            // Generate tree-style output for each stage with k=v pairs
            for (i, (stage_key, stage_name)) in existing_stages.iter().enumerate() {
                let is_last = i == existing_stages.len() - 1;
                let tree_char = if is_last { "└─" } else { "├─" };

                if let Some(&duration) = self.stage_timings.get(stage_key) {
                    let percentage = if self.total_duration_ms > 0 {
                        (duration as f64 / self.total_duration_ms as f64) * 100.0
                    } else {
                        0.0
                    };

                    // Extract strategy from stage name (e.g., "source_loading_inmemory" -> "inmemory")
                    let strategy = if stage_key.ends_with("_inmemory") {
                        "inmemory"
                    } else if stage_key.ends_with("_chunked") {
                        "chunked"
                    } else if stage_key.ends_with("_filespill") {
                        "filespill"
                    } else {
                        "standard"
                    };

                    let mut kv_pairs = vec![
                        format!("execution_time={}ms", duration),
                        format!("total_time_pc={:.1}", percentage),
                        format!("strategy={}", strategy),
                    ];

                    // Add memory info if available
                    if let Some(&memory_bytes) = self.stage_memory_usage.get(stage_key) {
                        let memory_mb = memory_bytes / (1024 * 1024);
                        kv_pairs.push(format!("peak_memory={}MB", memory_mb));
                    }

                    lines.push(format!(
                        "{} {}: {}",
                        tree_char,
                        stage_name,
                        kv_pairs.join(" ")
                    ));
                }
            }
        }

        lines.join("\n")
    }

    /// Generate detailed performance breakdown for debugging
    pub fn detailed_summary(&self) -> String {
        let mut summary = Vec::new();

        summary.push(format!("=== Generation Performance Summary ==="));
        summary.push(format!("Total Duration: {}ms", self.total_duration_ms));
        summary.push(format!(
            "Channels Processed: {} ({:.1} ch/s)",
            self.total_channels_processed, self.channels_per_second
        ));
        summary.push(format!(
            "Sources: {} | Filters: {}",
            self.sources_processed,
            self.filters_applied.len()
        ));

        if !self.stage_timings.is_empty() {
            summary.push(format!(""));
            summary.push(format!("Stage Timings:"));
            for (stage, duration) in &self.stage_timings {
                summary.push(format!("  {}: {}ms", stage, duration));
            }
        }

        if let Some(peak_memory) = self.peak_memory_usage_mb {
            summary.push(format!(""));
            summary.push(format!(
                "Memory: Peak {:.1}MB | Efficiency: {:.1} ch/MB",
                peak_memory,
                self.memory_efficiency.unwrap_or(0.0)
            ));
        }

        if !self.warnings.is_empty() || !self.errors.is_empty() {
            summary.push(format!(""));
            summary.push(format!(
                "Issues: {} warnings, {} errors",
                self.warnings.len(),
                self.errors.len()
            ));
        }

        summary.join("\n")
    }
}
