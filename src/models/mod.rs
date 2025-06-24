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
    pub starting_channel_number: i32,
    pub is_inverse: bool,
    pub logical_operator: LogicalOperator,
    pub condition_tree: Option<String>, // JSON tree structure for nested conditions
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterCondition {
    pub id: Uuid,
    pub filter_id: Uuid,
    pub field_name: String,
    pub operator: FilterOperator,
    pub value: String,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
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
    pub conditions: Vec<FilterConditionRequest>,
    pub logical_operator: LogicalOperator,
    pub starting_channel_number: i32,
    pub is_inverse: bool,
    pub condition_tree: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterUpdateRequest {
    pub name: String,
    pub conditions: Vec<FilterConditionRequest>,
    pub logical_operator: LogicalOperator,
    pub starting_channel_number: i32,
    pub is_inverse: bool,
    pub condition_tree: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConditionRequest {
    pub field_name: String,
    pub operator: FilterOperator,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterWithUsage {
    #[serde(flatten)]
    pub filter: Filter,
    pub conditions: Vec<FilterCondition>,
    pub usage_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterTestRequest {
    pub source_id: Uuid,
    pub conditions: Vec<FilterConditionRequest>,
    pub logical_operator: LogicalOperator,
    pub is_inverse: bool,
    pub condition_tree: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterTestResult {
    pub is_valid: bool,
    pub error: Option<String>,
    pub matching_channels: Vec<FilterTestChannel>,
    pub total_channels: usize,
    pub matched_count: usize,
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
#[sqlx(type_name = "text", rename_all = "lowercase")]
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

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum LogicalOperator {
    #[serde(rename = "and")]
    And,
    #[serde(rename = "or")]
    Or,
    #[serde(rename = "all")]
    All,
    #[serde(rename = "any")]
    Any,
}

impl LogicalOperator {
    /// Checks if this is an AND-like operator (And or All)
    pub fn is_and_like(&self) -> bool {
        matches!(self, LogicalOperator::And | LogicalOperator::All)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterGroup {
    pub conditions: Vec<FilterConditionRequest>,
    pub groups: Vec<FilterGroup>,
    pub logical_operator: LogicalOperator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedFilter {
    pub root_group: FilterGroup,
}

// New tree-based condition structures for nested expressions
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

impl Filter {
    /// Check if this filter uses the new tree-based condition structure
    pub fn uses_condition_tree(&self) -> bool {
        self.condition_tree.is_some()
    }

    /// Parse the condition tree from JSON if present
    pub fn get_condition_tree(&self) -> Option<ConditionTree> {
        self.condition_tree
            .as_ref()
            .and_then(|json| serde_json::from_str(json).ok())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterFieldInfo {
    pub name: String,
    pub display_name: String,
    pub field_type: String,
    pub nullable: bool,
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpgDlq {
    pub id: Uuid,
    pub source_id: Uuid,
    pub original_channel_id: String,
    pub conflict_type: EpgConflictType,
    pub channel_data: String,         // JSON blob
    pub program_data: Option<String>, // JSON blob
    pub conflict_details: String,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub occurrence_count: i32,
    pub resolved: bool,
    pub resolution_notes: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum EpgConflictType {
    #[serde(rename = "duplicate_identical")]
    DuplicateIdentical,
    #[serde(rename = "duplicate_conflicting")]
    DuplicateConflicting,
}

impl std::fmt::Display for EpgConflictType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EpgConflictType::DuplicateIdentical => write!(f, "duplicate_identical"),
            EpgConflictType::DuplicateConflicting => write!(f, "duplicate_conflicting"),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpgDlqStatistics {
    pub total_conflicts: usize,
    pub by_source: std::collections::HashMap<String, usize>,
    pub by_conflict_type: std::collections::HashMap<String, usize>,
    pub common_patterns: Vec<EpgDlqPattern>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpgDlqPattern {
    pub pattern: String,
    pub count: usize,
    pub examples: Vec<String>,
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
