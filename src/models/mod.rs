use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

pub mod channel;
pub mod filter;
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

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
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

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Filter {
    pub id: Uuid,
    pub name: String,
    pub pattern: String,
    pub starting_channel_number: i32,
    pub is_inverse: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
