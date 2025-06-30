use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

pub mod channel;
pub mod data_mapping;
pub mod epg_source;
pub mod filter;
pub mod linked_xtream;
pub mod logo_asset;
pub mod stream_proxy;
pub mod stream_source;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct StreamSource {
    pub id: Uuid,
    pub name: String,
    pub source_type: StreamSourceType,
    pub url: String,
    pub max_concurrent_streams: i32,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub field_map: Option<String>, // JSON string for M3U field mapping
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_ingested_at: Option<DateTime<Utc>>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "stream_source_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum StreamSourceType {
    M3u,
    Xtream,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct StreamProxy {
    pub id: Uuid,
    pub ulid: String, // ULID for public identification
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_generated_at: Option<DateTime<Utc>>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProxySource {
    pub proxy_id: Uuid,
    pub source_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    pub id: Uuid,
    pub name: String,
    pub source_type: FilterSourceType,
    pub starting_channel_number: i32,
    pub is_inverse: bool,
    pub condition_tree: String, // JSON tree structure for complex nested conditions
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
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
    pub sort_order: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Channel {
    pub id: Uuid,
    pub source_id: Uuid,
    pub tvg_id: Option<String>,
    pub tvg_name: Option<String>,
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
    pub is_active: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterCreateRequest {
    pub name: String,
    pub source_type: FilterSourceType,
    pub starting_channel_number: i32,
    pub is_inverse: bool,
    pub filter_expression: String, // Raw text expression like "(A OR B) AND C"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterUpdateRequest {
    pub name: String,
    pub source_type: FilterSourceType,
    pub starting_channel_number: i32,
    pub is_inverse: bool,
    pub filter_expression: String, // Raw text expression like "(A OR B) AND C"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterWithUsage {
    #[serde(flatten)]
    pub filter: Filter,
    pub usage_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterTestRequest {
    pub source_id: Uuid,
    pub source_type: FilterSourceType,
    pub filter_expression: String, // Raw text expression like "(A OR B) AND C"
    pub is_inverse: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterTestResult {
    pub is_valid: bool,
    pub error: Option<String>,
    pub matching_channels: Vec<FilterTestChannel>,
    pub total_channels: usize,
    pub matched_count: usize,
    pub expression_tree: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

// Assignment operators for actions
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum ActionOperator {
    #[serde(rename = "set")]
    Set, // = (overwrite)

    #[serde(rename = "set_if_empty")]
    SetIfEmpty, // ?= (conditional set - only if current value is empty/null)

    #[serde(rename = "append")]
    Append, // += (append with space)

    #[serde(rename = "remove")]
    Remove, // -= (remove substring)
}

// Values that can be assigned in actions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ActionValue {
    #[serde(rename = "literal")]
    Literal(String),

    // Future: Function call support
    #[serde(rename = "function")]
    Function(FunctionCall),

    // Future: Variable reference support
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

impl Filter {
    /// Check if this filter uses the new tree-based condition structure
    pub fn uses_condition_tree(&self) -> bool {
        true // Always uses condition_tree now
    }

    /// Parse the condition tree from JSON
    pub fn get_condition_tree(&self) -> Option<ConditionTree> {
        serde_json::from_str(&self.condition_tree).ok()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

// EPG Source Models
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct EpgSource {
    pub id: Uuid,
    pub name: String,
    pub source_type: EpgSourceType,
    pub url: String,
    pub update_cron: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub timezone: String,
    pub timezone_detected: bool,
    pub time_offset: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_ingested_at: Option<DateTime<Utc>>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct EpgChannel {
    pub id: Uuid,
    pub source_id: Uuid,
    pub channel_id: String,
    pub channel_name: String,
    pub channel_logo: Option<String>,
    pub channel_group: Option<String>,
    pub language: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ChannelEpgMapping {
    pub id: Uuid,
    pub stream_channel_id: Uuid,
    pub epg_channel_id: Uuid,
    pub mapping_type: EpgMappingType,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "epg_mapping_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum EpgMappingType {
    Manual,
    AutoName,
    AutoTvgId,
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
    pub channels: Vec<EpgChannelWithPrograms>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgChannelWithPrograms {
    #[serde(flatten)]
    pub channel: EpgChannel,
    pub programs: Vec<EpgProgram>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgRefreshResponse {
    pub success: bool,
    pub message: String,
    pub channel_count: usize,
    pub program_count: usize,
}

// Xtream Codes Integration Models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XtreamCodesCreateRequest {
    pub name: String,
    pub url: String,
    pub username: String,
    pub password: String,
    pub max_concurrent_streams: i32,
    pub update_cron: String,
    pub timezone: Option<String>,
    pub time_offset: Option<String>,
    pub create_stream_source: bool,
    pub create_epg_source: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XtreamCodesCreateResponse {
    pub success: bool,
    pub message: String,
    pub stream_source: Option<StreamSource>,
    pub epg_source: Option<EpgSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XtreamCodesUpdateRequest {
    pub name: String,
    pub url: String,
    pub username: String,
    pub password: String,
    pub max_concurrent_streams: i32,
    pub update_cron: String,
    pub timezone: String,
    pub time_offset: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedXtreamSources {
    pub stream_source: Option<StreamSource>,
    pub epg_source: Option<EpgSource>,
    pub link_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
        timezone: String,
        timezone_detected: bool,
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
        timezone: String,
        timezone_detected: bool,
        time_offset: String,
        // EPG stats
        channel_count: i64,
        program_count: i64,
        next_scheduled_update: Option<DateTime<Utc>>,
    },
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
            timezone: epg_with_stats.source.timezone.clone(),
            timezone_detected: epg_with_stats.source.timezone_detected,
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
