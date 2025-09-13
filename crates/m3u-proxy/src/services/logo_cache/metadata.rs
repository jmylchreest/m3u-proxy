//! Metadata structures for cached logos

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Metadata stored in .json files next to cached logos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedLogoMetadata {
    /// Original URL this logo was cached from
    pub original_url: Option<String>,
    /// Channel name or identifier this logo belongs to
    pub channel_name: Option<String>,
    /// Channel group or category
    pub channel_group: Option<String>,
    /// Description of the logo
    pub description: Option<String>,
    /// Tags for searching
    pub tags: Option<Vec<String>>,
    /// Image dimensions if known
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// Additional metadata fields
    pub extra_fields: Option<std::collections::HashMap<String, String>>,
    /// When this logo was first cached
    pub cached_at: DateTime<Utc>,
    /// Last time this metadata was updated
    pub updated_at: DateTime<Utc>,
}
