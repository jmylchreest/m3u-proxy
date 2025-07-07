use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct LogoAsset {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub file_name: String,
    pub file_path: String,
    pub file_size: i64,
    pub mime_type: String,
    pub asset_type: LogoAssetType,
    pub source_url: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub parent_asset_id: Option<Uuid>,
    pub format_type: LogoFormatType,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum LogoAssetType {
    #[serde(rename = "uploaded")]
    Uploaded,
    #[serde(rename = "cached")]
    Cached,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum LogoFormatType {
    #[serde(rename = "original")]
    Original,
    #[serde(rename = "png_conversion")]
    PngConversion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoAssetCreateRequest {
    pub name: String,
    pub description: Option<String>,
    pub asset_type: LogoAssetType,
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoAssetUpdateRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoAssetUploadResponse {
    pub id: Uuid,
    pub name: String,
    pub file_name: String,
    pub file_size: i64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoAssetListRequest {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub search: Option<String>,
    pub asset_type: Option<LogoAssetType>,
    pub include_cached: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoAssetListResponse {
    pub assets: Vec<LogoAssetWithUrl>,
    pub total_count: i64,
    pub page: u32,
    pub limit: u32,
    pub total_pages: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoAssetWithUrl {
    #[serde(flatten)]
    pub asset: LogoAsset,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoAssetWithLinked {
    #[serde(flatten)]
    pub asset: LogoAsset,
    pub url: String,
    pub linked_assets: Vec<LogoAssetWithUrl>,
    pub available_formats: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoAssetSearchRequest {
    pub query: Option<String>,
    pub limit: Option<u32>,
    pub include_cached: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoAssetSearchResult {
    pub assets: Vec<LogoAssetWithUrl>,
    pub total_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoCacheStats {
    pub total_cached_logos: i64,
    pub total_uploaded_logos: i64,
    pub total_storage_used: i64,
    pub total_linked_assets: i64,
    pub cache_hit_rate: Option<f64>,
    /// Filesystem-based cached logos (not in database)
    pub filesystem_cached_logos: i64,
    /// Storage used by filesystem-based cached logos
    pub filesystem_cached_storage: i64,
}
