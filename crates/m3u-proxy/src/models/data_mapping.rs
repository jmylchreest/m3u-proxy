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

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    ToSchema,
    sea_orm::DeriveActiveEnum,
    strum::EnumIter,
)]
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
            // Core EPG fields
            "channel_id",
            "channel_name",
            "channel_logo",
            "channel_group",
            "title",        // Program title
            "description",  // Program description
            "program_icon", // Program icon/thumbnail
            // Extended XMLTV metadata fields
            "program_category", // <category> - program category
            "subtitles",        // <sub-title> - episode subtitle
            "episode_num",      // Episode number for <episode-num>
            "season_num",       // Season number for <episode-num>
            "language",         // <language> - program language
            "rating",           // <rating> - content rating
            "aspect_ratio",     // Video aspect ratio metadata
        ]
    }

    pub fn is_valid_field(field: &str) -> bool {
        Self::available_fields().contains(&field)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMappingFieldInfo {
    pub field_name: String,
    pub canonical_name: String,
    pub display_name: String,
    pub field_type: String,
    pub nullable: bool,
    pub source_type: DataMappingSourceType,
    pub read_only: bool,
    pub aliases: Vec<String>,
}

impl DataMappingFieldInfo {
    pub fn available_for_source_type(
        source_type: &DataMappingSourceType,
    ) -> Vec<DataMappingFieldInfo> {
        use crate::field_registry::{FieldDataType, FieldRegistry, SourceKind, StageKind};
        let registry = FieldRegistry::global();
        let source_kind = match source_type {
            DataMappingSourceType::Stream => SourceKind::Stream,
            DataMappingSourceType::Epg => SourceKind::Epg,
        };
        registry
            .descriptors_for(source_kind, StageKind::DataMapping)
            .into_iter()
            .map(|d| DataMappingFieldInfo {
                field_name: d.name.to_string(),
                canonical_name: d.name.to_string(),
                display_name: d.display_name.to_string(),
                field_type: match d.data_type {
                    FieldDataType::Url => "url",
                    FieldDataType::Integer => "integer",
                    FieldDataType::DateTime => "datetime",
                    FieldDataType::Duration => "duration",
                    FieldDataType::String => "string",
                }
                .to_string(),
                nullable: d.nullable,
                source_type: source_type.clone(),
                read_only: d.read_only,
                aliases: d.aliases.iter().map(|a| a.to_string()).collect(),
            })
            .collect()
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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
    /// Source type to preview (stream or epg)
    pub source_type: DataMappingSourceType,
    /// Optional single source filter (legacy – superseded by source_ids in POST body elsewhere)
    pub source_id: Option<Uuid>,
    /// Maximum number of sample changes to return
    pub limit: Option<i32>,
}

/// Typed request for ad-hoc expression preview (POST /data-mapping/preview).
///
/// This is the strongly-typed counterpart to the older ad-hoc preview payload
/// that lived only in the web layer. It allows clients (and OpenAPI) to
/// understand the shape of an expression-based preview request that:
///   - Targets one source kind (stream or epg)
///   - Optionally restricts to a list of specific source IDs (empty => all)
///   - Supplies a single expression to be parsed, canonicalized (aliases -> canonical),
///     validated and then applied virtually
///   - Supports an optional sample limit (server may cap to a safe maximum)
///
/// NOTE:
///   expression is required for expression previews (unlike rule previews
///   where expression may be absent because existing stored rules are used).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataMappingExpressionPreviewRequest {
    /// Source type to preview (stream or epg)
    pub source_type: DataMappingSourceType,
    /// Specific sources to include (empty = all active/available sources of this type)
    pub source_ids: Vec<Uuid>,
    /// The raw user-entered expression (aliases allowed; will be canonicalized internally)
    pub expression: String,
    /// Maximum number of modified (or matched) sample records to return (server caps hard maximum)
    pub limit: Option<i32>,
    /// Whether to include sample detail payloads (modified & matched-but-unmodified).
    /// Defaults to true if omitted. Set false for a fast counts-only preview.
    pub include_samples: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataMappingPreviewResponse {
    /// Source type evaluated
    pub source_type: DataMappingSourceType,
    /// Unified summary totals (independent of stream/epg specifics).
    ///
    /// The summary always reflects:
    ///   - total_records: total candidates considered
    ///   - condition_matches: records whose condition evaluated true (even if unchanged)
    ///   - modified_records: records with at least one field modification
    ///   - canonical_expression: the fully canonicalized expression string
    ///     (all aliases resolved to canonical registry field names) when
    ///     the preview originated from an ad-hoc expression submission.
    ///     For previews of stored rules this may be None if no re-canonicalization
    ///     was necessary/recorded.
    pub summary: DataMappingPreviewSummary,
    /// Stream preview details (present when source_type == stream).
    /// Provides stream-specific counts plus sampled channel transformations.
    pub stream: Option<StreamDataMappingPreview>,
    /// EPG preview details (present when source_type == epg).
    /// Provides EPG program–specific counts plus sampled program transformations.
    pub epg: Option<EpgDataMappingPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StreamDataMappingPreview {
    /// Total channels processed
    pub total_channels: i32,
    /// Number of channels where the condition matched (even if no modification)
    pub condition_matches: i32,
    /// Number of channels with at least one modification
    pub affected_channels: i32,
    /// Sample channel modifications as raw JSON objects (legacy structure). Empty when not requested.
    pub preview_channels: Vec<serde_json::Value>,
    /// Typed sample channel modifications (preferred new field). Present only when samples requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_channels_typed: Option<Vec<MappedChannel>>,
    /// Condition-matched but unmodified channel samples (typed). Allows UI to explain zero-mod scenarios.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_but_unmodified_channels: Option<Vec<MappedChannel>>,
    /// Names or identifiers of rules applied to produce changes
    pub applied_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EpgDataMappingPreview {
    /// Total programs processed
    pub total_programs: i32,
    /// Number of programs whose condition matched (even if not modified)
    pub condition_matches: i32,
    /// Number of programs actually modified
    pub affected_programs: i32,
    /// Sample program modifications as raw JSON objects (legacy structure). Empty when not requested.
    pub preview_programs: Vec<serde_json::Value>,
    /// Typed sample program modifications (preferred new field). Present only when samples requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_programs_typed: Option<Vec<MappedEpgProgram>>,
    /// Condition-matched but unmodified program samples (typed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_but_unmodified_programs: Option<Vec<MappedEpgProgram>>,
    /// Applied rule names / identifiers
    pub applied_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataMappingPreviewSummary {
    /// Total records processed (channels or programs)
    pub total_records: i32,
    /// Total condition matches (channels or programs where a condition evaluated true)
    pub condition_matches: i32,
    /// Total modified records
    pub modified_records: i32,
    /// Original submitted expression (with aliases as entered) if available
    pub raw_expression: Option<String>,
    /// Expression string after canonicalization (aliases -> canonical) if performed
    pub canonical_expression: Option<String>,
    /// Total records actually scanned/evaluated (may differ from total_records if future caps or early exits are introduced)
    #[serde(default)]
    pub scanned_records: Option<i32>,
    /// Whether the scan was truncated due to a server-side cap (future-proof; always false currently)
    #[serde(default)]
    pub truncated: Option<bool>,
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
        if let Some(expression) = &self.expression
            && expression.trim().is_empty()
        {
            return Err("Expression cannot be empty".to_string());
        }
        // Add more expression validation logic here as needed
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
