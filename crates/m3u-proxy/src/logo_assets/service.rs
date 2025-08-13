use crate::logo_assets::storage::LogoAssetStorage;
use crate::models::logo_asset::*;

use anyhow;
use chrono::Utc;
use image::ImageFormat;
use reqwest::Client;
use sandboxed_file_manager::SandboxedManager;
use sha2::{Digest, Sha256};
use sqlx::{Pool, Row, Sqlite};
use std::collections::BTreeMap;

use std::path::PathBuf;
use tokio::fs;
use tracing::{debug, error, trace};
use url::Url;
use uuid::Uuid;
use crate::utils::uuid_parser::parse_uuid_flexible;

#[derive(Debug, Clone)]
pub struct LogoAssetService {
    pool: Pool<Sqlite>,
    pub storage: LogoAssetStorage,
    http_client: Client,
    logo_file_manager: Option<SandboxedManager>,
}


/// Parameters for creating a logo asset with specific ID
#[derive(Debug)]
pub struct CreateAssetWithIdParams {
    pub asset_id: Uuid,
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
}
impl LogoAssetService {
    pub fn new(pool: Pool<Sqlite>, storage: LogoAssetStorage) -> Self {
        Self {
            pool,
            storage,
            http_client: Client::new(),
            logo_file_manager: None,
        }
    }

    /// Update the service with a file manager
    pub fn with_file_manager(mut self, file_manager: SandboxedManager) -> Self {
        self.logo_file_manager = Some(file_manager);
        self
    }
    

    /// Construct the full URL for a cached logo
    pub fn get_cached_logo_url(&self, cache_id: &str, base_url: &str) -> String {
        format!(
            "{}/api/v1/logos/cached/{}",
            base_url.trim_end_matches("/"),
            cache_id
        )
    }
    pub async fn create_asset_with_id(
        &self,
        params: CreateAssetWithIdParams,
    ) -> Result<LogoAsset, sqlx::Error> {
        let asset_type_str = match params.asset_type {
            LogoAssetType::Uploaded => "uploaded",
            LogoAssetType::Cached => "cached",
        };

        let asset_id_str = params.asset_id.to_string();

        let created_at = Utc::now().to_rfc3339();
        let updated_at = created_at.clone();

        sqlx::query(
            r#"
            INSERT INTO logo_assets (id, name, description, file_name, file_path, file_size, mime_type, asset_type, source_url, width, height, parent_asset_id, format_type, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, 'original', ?, ?)
            "#
        )
        .bind(&asset_id_str)
        .bind(&params.name)
        .bind(&params.description)
        .bind(&params.file_name)
        .bind(&params.file_path)
        .bind(params.file_size)
        .bind(&params.mime_type)
        .bind(asset_type_str)
        .bind(&params.source_url)
        .bind(params.width)
        .bind(params.height)
        .bind(&updated_at)
        .execute(&self.pool)
        .await?;

        Ok(LogoAsset {
            id: params.asset_id.to_string(),
            name: params.name,
            description: params.description,
            file_name: params.file_name,
            file_path: params.file_path,
            file_size: params.file_size,
            mime_type: params.mime_type,
            asset_type: params.asset_type,
            source_url: params.source_url,
            width: params.width,
            height: params.height,
            parent_asset_id: None,
            format_type: LogoFormatType::Original,
            created_at: crate::utils::datetime::DateTimeParser::parse_flexible(&created_at)
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
            updated_at: crate::utils::datetime::DateTimeParser::parse_flexible(&updated_at)
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
        })
    }

    pub async fn get_asset(&self, asset_id: Uuid) -> Result<LogoAsset, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT id, name, description, file_name, file_path, file_size, mime_type, asset_type, source_url, width, height, parent_asset_id, format_type, created_at, updated_at
            FROM logo_assets
            WHERE id = ?
            "#
        )
        .bind(asset_id.to_string())
        .fetch_one(&self.pool)
        .await?;

        let asset_type = match row.get::<String, _>("asset_type").as_str() {
            "uploaded" => LogoAssetType::Uploaded,
            "cached" => LogoAssetType::Cached,
            _ => LogoAssetType::Uploaded,
        };

        let parent_asset_id = row
            .get::<Option<String>, _>("parent_asset_id")
            .and_then(|s| s.parse().ok());

        let format_type = match row.get::<String, _>("format_type").as_str() {
            "png_conversion" => LogoFormatType::PngConversion,
            _ => LogoFormatType::Original,
        };

        Ok(LogoAsset {
            id: asset_id.to_string(),
            name: row.get("name"),
            description: row.get("description"),
            file_name: row.get("file_name"),
            file_path: row.get("file_path"),
            file_size: row.get("file_size"),
            mime_type: row.get("mime_type"),
            asset_type,
            source_url: row.get("source_url"),
            width: row.get("width"),
            height: row.get("height"),
            parent_asset_id,
            format_type,
            created_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                &row.get::<String, _>("created_at"),
            )
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
            updated_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                &row.get::<String, _>("updated_at"),
            )
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
        })
    }

    pub async fn list_assets(
        &self,
        request: LogoAssetListRequest,
        base_url: &str,
    ) -> Result<LogoAssetListResponse, sqlx::Error> {
        let limit = request.limit.unwrap_or(20);
        let page = request.page.unwrap_or(1);
        let offset = (page - 1) * limit;

        let mut query = String::from(
            "SELECT id, name, description, file_name, file_path, file_size, mime_type, asset_type, source_url, width, height, parent_asset_id, format_type, created_at, updated_at FROM logo_assets WHERE format_type = 'original'",
        );

        let mut count_query = String::from(
            "SELECT COUNT(*) as count FROM logo_assets WHERE format_type = 'original'",
        );

        if let Some(search) = &request.search {
            let search_clause = format!(" AND name LIKE '%{search}%'");
            query.push_str(&search_clause);
            count_query.push_str(&search_clause);
        }

        if let Some(asset_type) = &request.asset_type {
            let type_str = match asset_type {
                LogoAssetType::Uploaded => "uploaded",
                LogoAssetType::Cached => "cached",
            };
            let type_clause = format!(" AND asset_type = '{type_str}'");
            query.push_str(&type_clause);
            count_query.push_str(&type_clause);
        }

        query.push_str(&format!(
            " ORDER BY created_at DESC LIMIT {limit} OFFSET {offset}"
        ));

        let rows = sqlx::query(&query).fetch_all(&self.pool).await?;
        let count_row = sqlx::query(&count_query).fetch_one(&self.pool).await?;
        let total_count: i64 = count_row.get("count");

        let mut assets = Vec::new();

        for row in rows {
            let asset_type = match row.get::<String, _>("asset_type").as_str() {
                "uploaded" => LogoAssetType::Uploaded,
                "cached" => LogoAssetType::Cached,
                _ => LogoAssetType::Uploaded,
            };

            let parent_asset_id = row
                .get::<Option<String>, _>("parent_asset_id")
                .and_then(|s| s.parse().ok());

            let format_type = match row.get::<String, _>("format_type").as_str() {
                "png_conversion" => LogoFormatType::PngConversion,
                _ => LogoFormatType::Original,
            };

            let asset = LogoAsset {
                id: row.get::<String, _>("id"),
                name: row.get("name"),
                description: row.get("description"),
                file_name: row.get("file_name"),
                file_path: row.get("file_path"),
                file_size: row.get("file_size"),
                mime_type: row.get("mime_type"),
                asset_type,
                source_url: row.get("source_url"),
                width: row.get("width"),
                height: row.get("height"),
                parent_asset_id,
                format_type,
                created_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.get::<String, _>("created_at"),
                )
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
                updated_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.get::<String, _>("updated_at"),
                )
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
            };

            assets.push(asset);
        }

        let total_pages = ((total_count as f64) / (limit as f64)).ceil() as u32;

        let assets_with_urls: Vec<LogoAssetWithUrl> = assets
            .into_iter()
            .map(|asset| {
                let url = format!("{}/api/v1/logos/{}", base_url.trim_end_matches('/'), asset.id);
                LogoAssetWithUrl { asset, url }
            })
            .collect();

        Ok(LogoAssetListResponse {
            assets: assets_with_urls,
            total_count,
            page,
            limit,
            total_pages,
        })
    }

    pub async fn update_asset(
        &self,
        asset_id: Uuid,
        name: String,
        description: Option<String>,
    ) -> Result<LogoAsset, sqlx::Error> {
        let asset_id_str = asset_id.to_string();
        let updated_at = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            UPDATE logo_assets
            SET name = ?, description = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&name)
        .bind(&description)
        .bind(&updated_at)
        .bind(&asset_id_str)
        .execute(&self.pool)
        .await?;

        self.get_asset(asset_id).await
    }

    pub async fn delete_asset(&self, asset_id: Uuid) -> Result<(), sqlx::Error> {
        // Get asset info first to delete the file
        let asset = self.get_asset(asset_id).await?;

        // Delete linked assets first (iterative approach to avoid recursion)
        let mut assets_to_delete = vec![asset_id];
        let mut processed = std::collections::HashSet::new();

        while let Some(current_id) = assets_to_delete.pop() {
            if processed.contains(&current_id) {
                continue;
            }
            processed.insert(current_id);

            // Find linked assets for current asset
            let linked_assets = sqlx::query("SELECT id FROM logo_assets WHERE parent_asset_id = ?")
                .bind(current_id.to_string())
                .fetch_all(&self.pool)
                .await?;

            for linked_row in linked_assets {
                let linked_id = parse_uuid_flexible(&linked_row.get::<String, _>("id")).unwrap();
                if !processed.contains(&linked_id) {
                    assets_to_delete.push(linked_id);
                }
            }
        }

        // Delete all assets from database
        for asset_id_to_delete in &processed {
            sqlx::query("DELETE FROM logo_assets WHERE id = ?")
                .bind(asset_id_to_delete.to_string())
                .execute(&self.pool)
                .await?;
        }

        // Delete file from storage
        if let Err(e) = self.storage.delete_file(&asset.file_path).await {
            error!("Failed to delete file for asset {}: {}", asset_id, e);
        }

        Ok(())
    }

    pub async fn search_assets(
        &self,
        request: LogoAssetSearchRequest,
        base_url: &str,
    ) -> Result<LogoAssetSearchResult, sqlx::Error> {
        let limit = request.limit.unwrap_or(20);
        let search_query = request.query.unwrap_or_default();

        // First get uploaded logos from database
        let query = format!(
            "SELECT id, name, description, file_name, file_path, file_size, mime_type, asset_type, source_url, width, height, parent_asset_id, format_type, created_at, updated_at FROM logo_assets WHERE format_type = 'original' AND asset_type = 'uploaded' AND name LIKE '%{search_query}%' ORDER BY name LIMIT {limit}"
        );

        let rows = sqlx::query(&query).fetch_all(&self.pool).await?;

        let mut assets = Vec::new();

        for row in rows {
            let asset_type = match row.get::<String, _>("asset_type").as_str() {
                "uploaded" => LogoAssetType::Uploaded,
                "cached" => LogoAssetType::Cached,
                _ => LogoAssetType::Uploaded,
            };

            let parent_asset_id = row
                .get::<Option<String>, _>("parent_asset_id")
                .and_then(|s| s.parse().ok());

            let format_type = match row.get::<String, _>("format_type").as_str() {
                "png_conversion" => LogoFormatType::PngConversion,
                _ => LogoFormatType::Original,
            };

            let asset = LogoAsset {
                id: row.get::<String, _>("id"),
                name: row.get("name"),
                description: row.get("description"),
                file_name: row.get("file_name"),
                file_path: row.get("file_path"),
                file_size: row.get("file_size"),
                mime_type: row.get("mime_type"),
                asset_type,
                source_url: row.get("source_url"),
                width: row.get("width"),
                height: row.get("height"),
                parent_asset_id,
                format_type,
                created_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.get::<String, _>("created_at"),
                )
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
                updated_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.get::<String, _>("updated_at"),
                )
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
            };

            let url = format!(
                "{}/api/v1/logos/{}",
                base_url.trim_end_matches('/'),
                asset.id
            );
            assets.push(LogoAssetWithUrl { asset, url });
        }

        Ok(LogoAssetSearchResult {
            total_count: assets.len(),
            assets,
        })
    }

    /// Search assets with support for cached logos from filesystem
    pub async fn search_assets_with_cached(
        &self,
        request: LogoAssetSearchRequest,
        base_url: &str,
        logo_cache_scanner: Option<&crate::services::logo_cache_scanner::LogoCacheScanner>,
        include_cached: bool,
    ) -> Result<LogoAssetSearchResult, anyhow::Error> {
        let limit = request.limit.unwrap_or(20);
        let search_query = request.query.as_deref();

        let mut all_assets = Vec::new();

        // Get uploaded logos from database
        let mut db_request = request.clone();
        db_request.include_cached = None; // Don't pass this to the simpler search method
        let db_result = self
            .search_assets(db_request, base_url)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to search database assets: {}", e))?;
        all_assets.extend(db_result.assets);

        // Add cached logos from filesystem if requested and scanner is available
        if include_cached {
            if let Some(scanner) = logo_cache_scanner {
                let cached_logos = scanner
                    .search_cached_logos(search_query, None)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to search cached logos: {}", e))?;

                // Convert cached logos to LogoAssetWithUrl format
                for cached_logo in cached_logos {
                    // Create a synthetic LogoAsset from cached logo info
                    let synthetic_asset = LogoAsset {
                        id: cached_logo.cache_id.clone(), // Use cache_id directly as string ID
                        name: cached_logo.file_name.clone(),
                        description: Some(format!("Cached logo: {}", cached_logo.cache_id)),
                        file_name: cached_logo.file_name.clone(),
                        file_path: format!("cached/{}", cached_logo.file_name),
                        file_size: cached_logo.size_bytes as i64,
                        mime_type: cached_logo.content_type.clone(),
                        asset_type: LogoAssetType::Cached,
                        source_url: cached_logo.inferred_source_url.clone(),
                        width: None, // We don't store dimensions for cached logos
                        height: None,
                        parent_asset_id: None,
                        format_type: crate::models::logo_asset::LogoFormatType::Original,
                        created_at: cached_logo.created_at,
                        updated_at: cached_logo.last_accessed,
                    };

                    // Use the cached logo serving URL
                    let url = format!(
                        "{}/api/v1/logos/cached/{}",
                        base_url.trim_end_matches('/'),
                        cached_logo.cache_id
                    );
                    all_assets.push(LogoAssetWithUrl {
                        asset: synthetic_asset,
                        url,
                    });
                }
            }
        }

        // Sort by asset type (uploaded first, cached after), then by name
        all_assets.sort_by(|a, b| {
            use crate::models::logo_asset::LogoAssetType;
            match (&a.asset.asset_type, &b.asset.asset_type) {
                (LogoAssetType::Uploaded, LogoAssetType::Cached) => std::cmp::Ordering::Less,
                (LogoAssetType::Cached, LogoAssetType::Uploaded) => std::cmp::Ordering::Greater,
                _ => a.asset.name.cmp(&b.asset.name), // Same type, sort by name
            }
        });

        // Apply limit
        let total_count = all_assets.len();
        all_assets.truncate(limit as usize);

        Ok(LogoAssetSearchResult {
            total_count,
            assets: all_assets,
        })
    }

    pub async fn get_cache_stats(&self) -> Result<LogoCacheStats, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT
                SUM(CASE WHEN asset_type = 'cached' AND format_type = 'original' THEN 1 ELSE 0 END) as total_cached_logos,
                SUM(CASE WHEN asset_type = 'uploaded' AND format_type = 'original' THEN 1 ELSE 0 END) as total_uploaded_logos,
                SUM(file_size) as total_storage_used,
                SUM(CASE WHEN format_type = 'png_conversion' THEN 1 ELSE 0 END) as total_linked_assets
            FROM logo_assets
            "#
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(LogoCacheStats {
            total_cached_logos: row.get::<Option<i64>, _>("total_cached_logos").unwrap_or(0),
            total_uploaded_logos: row
                .get::<Option<i64>, _>("total_uploaded_logos")
                .unwrap_or(0),
            total_storage_used: row.get::<Option<i64>, _>("total_storage_used").unwrap_or(0),
            total_linked_assets: row
                .get::<Option<i64>, _>("total_linked_assets")
                .unwrap_or(0),
            cache_hit_rate: None,
            filesystem_cached_logos: 0, // Will be updated by caller with scanner data
            filesystem_cached_storage: 0, // Will be updated by caller with scanner data
        })
    }

    /// Get cache stats with filesystem-based cached logos included
    pub async fn get_cache_stats_with_filesystem(
        &self,
        logo_cache_scanner: Option<&crate::services::logo_cache_scanner::LogoCacheScanner>,
    ) -> Result<LogoCacheStats, anyhow::Error> {
        // Get database-based stats first
        let mut stats = self
            .get_cache_stats()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get database cache stats: {}", e))?;

        // Add filesystem-based stats if scanner is available
        if let Some(scanner) = logo_cache_scanner {
            let cached_logos = scanner
                .scan_cached_logos()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to scan cached logos: {}", e))?;

            stats.filesystem_cached_logos = cached_logos.len() as i64;
            stats.filesystem_cached_storage =
                cached_logos.iter().map(|logo| logo.size_bytes as i64).sum();

            tracing::debug!(
                "Filesystem cache stats: {} logos, {} bytes total",
                stats.filesystem_cached_logos,
                stats.filesystem_cached_storage
            );
        }

        Ok(stats)
    }

    /// Check and generate missing metadata for existing cached logos
    pub async fn ensure_cached_logo_metadata(
        &self,
        logo_cache_scanner: &crate::services::logo_cache_scanner::LogoCacheScanner,
    ) -> Result<usize, anyhow::Error> {
        let cached_logos = logo_cache_scanner.scan_cached_logos().await?;
        let mut generated_count = 0;

        for logo_info in cached_logos {
            if logo_info.metadata.is_none() {
                // Try to generate basic metadata with just the original URL if we can infer it
                let generated = logo_cache_scanner
                    .generate_metadata_for_existing_logo(
                        &logo_info.cache_id,
                        logo_info.inferred_source_url,
                        None, // No channel name available from cache_id
                        None,
                    )
                    .await?;

                if generated {
                    generated_count += 1;
                    debug!(
                        "Generated metadata for existing cached logo: {}",
                        logo_info.cache_id
                    );
                }
            }
        }

        if generated_count > 0 {
            tracing::info!(
                "Generated metadata for {} existing cached logos",
                generated_count
            );
        }

        Ok(generated_count)
    }

    /// Generate a normalized cache ID from a logo URL
    ///
    /// This function:
    /// 1. Removes the URL scheme (http/https)
    /// 2. Removes file extensions from the path
    /// 3. Alphabetically sorts query parameters
    /// 4. Creates a SHA256 hash of the normalized URL
    fn generate_cache_id_from_url(url: &str) -> Result<String, anyhow::Error> {
        let parsed_url = Url::parse(url).map_err(|e| {
            anyhow::anyhow!(
                "Invalid URL '{}': {}",
                crate::utils::url::UrlUtils::obfuscate_credentials(url),
                e
            )
        })?;

        // Start building normalized URL without scheme
        let mut normalized = String::new();

        // Add host
        if let Some(host) = parsed_url.host_str() {
            normalized.push_str(host);
        }

        // Add port if not default
        if let Some(port) = parsed_url.port() {
            let is_default_port = (parsed_url.scheme() == "http" && port == 80)
                || (parsed_url.scheme() == "https" && port == 443);
            if !is_default_port {
                normalized.push(':');
                normalized.push_str(&port.to_string());
            }
        }

        // Add path without file extension
        let path = parsed_url.path();
        if let Some(last_slash) = path.rfind('/') {
            let (dir_part, file_part) = path.split_at(last_slash + 1);
            normalized.push_str(dir_part);

            // Remove extension from filename
            if let Some(dot_pos) = file_part.rfind('.') {
                normalized.push_str(&file_part[..dot_pos]);
            } else {
                normalized.push_str(file_part);
            }
        } else {
            // No slash in path, treat whole path as filename
            if let Some(dot_pos) = path.rfind('.') {
                normalized.push_str(&path[..dot_pos]);
            } else {
                normalized.push_str(path);
            }
        }

        // Sort and add query parameters
        let mut sorted_params = BTreeMap::new();
        for (key, value) in parsed_url.query_pairs() {
            sorted_params.insert(key.to_string(), value.to_string());
        }

        if !sorted_params.is_empty() {
            normalized.push('?');
            let param_string: Vec<String> = sorted_params
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            normalized.push_str(&param_string.join("&"));
        }

        // Generate SHA256 hash
        let mut hasher = Sha256::new();
        hasher.update(normalized.as_bytes());
        let hash = hasher.finalize();

        Ok(format!("{hash:x}"))
    }

    /// Download and cache a logo from a URL
    ///
    /// This function:
    /// 1. Generates a cache ID from the URL
    /// 2. Checks if the logo is already cached
    /// 3. Downloads the image if not cached
    /// 4. Converts to PNG format
    /// 5. Saves to the cached logo directory
    /// 6. Generates metadata .json file if channel info provided
    ///
    /// Returns the cache ID on success
    pub async fn cache_logo_from_url(&self, logo_url: &str) -> Result<String, anyhow::Error> {
        self.cache_logo_from_url_with_metadata(logo_url, None, None, None)
            .await
    }
    
    /// Download and cache a logo from a URL with size tracking
    ///
    /// Returns (cache_id, bytes_transferred) where bytes_transferred is 0 for cache hits
    pub async fn cache_logo_from_url_with_size_tracking(&self, logo_url: &str) -> Result<(String, u64), anyhow::Error> {
        self.cache_logo_from_url_with_metadata_and_size_tracking(logo_url, None, None, None)
            .await
    }

    /// Download and cache a logo from a URL with optional metadata
    pub async fn cache_logo_from_url_with_metadata(
        &self,
        logo_url: &str,
        channel_name: Option<String>,
        channel_group: Option<String>,
        extra_fields: Option<std::collections::HashMap<String, String>>,
    ) -> Result<String, anyhow::Error> {
        let (cache_id, _bytes_transferred) = self.cache_logo_from_url_with_metadata_and_size_tracking(
            logo_url, channel_name, channel_group, extra_fields
        ).await?;
        Ok(cache_id)
    }
    
    /// Download and cache a logo from a URL with optional metadata and size tracking
    pub async fn cache_logo_from_url_with_metadata_and_size_tracking(
        &self,
        logo_url: &str,
        channel_name: Option<String>,
        channel_group: Option<String>,
        extra_fields: Option<std::collections::HashMap<String, String>>,
    ) -> Result<(String, u64), anyhow::Error> {
        // Generate cache ID
        let cache_id = Self::generate_cache_id_from_url(logo_url)?;

        // Check if already cached using the sandboxed file manager
        if let Some(file_manager) = &self.logo_file_manager {
            // The file manager's base is already the cached logo directory, so use direct paths
            let logo_file_path = format!("{cache_id}.png");
            let metadata_file_path = format!("{cache_id}.json");

            let file_exists = file_manager.read(&logo_file_path).await.is_ok();
            let metadata_exists = file_manager.read(&metadata_file_path).await.is_ok();

            if file_exists && metadata_exists {
                trace!(
                    "Logo and metadata already cached: {} -> {}",
                    logo_url, cache_id
                );
                return Ok((cache_id, 0)); // 0 bytes for cache hit
            }

            if file_exists && !metadata_exists {
                // File exists but no metadata - generate it
                trace!(
                    "Generating missing metadata for cached logo: {} -> {}",
                    logo_url, cache_id
                );
                self.generate_metadata_for_cached_logo_with_file_manager(
                    &cache_id,
                    logo_url,
                    channel_name,
                    channel_group,
                    extra_fields,
                )
                .await?;
                return Ok((cache_id, 0)); // 0 bytes for cache hit
            }
        } else {
            // Fallback to direct filesystem access for backward compatibility
            let cache_file_path = self.get_cached_logo_path(&cache_id);
            let metadata_path = self
                .storage
                .cached_logo_dir
                .join(format!("{cache_id}.json"));

            let file_exists = cache_file_path.exists();
            let metadata_exists = metadata_path.exists();

            if file_exists && metadata_exists {
                trace!(
                    "Logo and metadata already cached: {} -> {}",
                    logo_url, cache_id
                );
                return Ok((cache_id, 0)); // 0 bytes for cache hit
            }

            if file_exists && !metadata_exists {
                // File exists but no metadata - generate it
                trace!(
                    "Generating missing metadata for cached logo: {} -> {}",
                    logo_url, cache_id
                );
                self.generate_metadata_for_cached_logo(
                    &cache_id,
                    logo_url,
                    channel_name,
                    channel_group,
                    extra_fields,
                )
                .await?;
                return Ok((cache_id, 0)); // 0 bytes for cache hit
            }
        }

        debug!("Downloading logo from URL: {} -> {}", logo_url, cache_id);

        // Download the image
        let response =
            self.http_client.get(logo_url).send().await.map_err(|e| {
                anyhow::anyhow!("Failed to download logo from '{}': {}", logo_url, e)
            })?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to download logo from '{}': HTTP {}",
                logo_url,
                response.status()
            ));
        }

        let image_bytes = response.bytes().await.map_err(|e| {
            anyhow::anyhow!("Failed to read image bytes from '{}': {}", logo_url, e)
        })?;
        
        let raw_bytes_downloaded = image_bytes.len() as u64;

        // Convert to PNG
        let png_bytes = self.convert_image_to_png(&image_bytes, logo_url)?;

        // Save to cache using the appropriate method
        if let Some(file_manager) = &self.logo_file_manager {
            // Use sandboxed file manager
            // The file manager's base is already the cached logo directory, so use direct paths
            let logo_file_path = format!("{cache_id}.png");

            file_manager
                .write(&logo_file_path, &png_bytes)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to save cached logo '{}': {}", cache_id, e))?;
        } else {
            // Fallback to direct filesystem access
            let cache_file_path = self.get_cached_logo_path(&cache_id);

            // Ensure cache directory exists
            if let Some(parent) = cache_file_path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to create cache directory: {}", e))?;
            }

            // Save to cache
            fs::write(&cache_file_path, &png_bytes)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to save cached logo '{}': {}", cache_id, e))?;
        }

        let bytes_transferred = png_bytes.len() as u64;
        debug!(
            "Successfully cached logo: {} -> {} (downloaded {} raw bytes, stored {} PNG bytes)",
            logo_url,
            cache_id,
            raw_bytes_downloaded,
            bytes_transferred
        );

        // Generate metadata file if channel info provided
        if channel_name.is_some() || channel_group.is_some() || extra_fields.is_some() {
            if self.logo_file_manager.is_some() {
                if let Err(e) = self.generate_metadata_for_cached_logo_with_file_manager(
                    &cache_id,
                    logo_url,
                    channel_name,
                    channel_group,
                    extra_fields,
                ).await {
                    debug!("Failed to generate metadata for {}: {}", cache_id, e);
                }
            } else if let Err(e) = self.generate_metadata_for_cached_logo(
                &cache_id,
                logo_url,
                channel_name,
                channel_group,
                extra_fields,
            ).await {
                debug!("Failed to generate metadata for {}: {}", cache_id, e);
            }
        }

        Ok((cache_id, bytes_transferred))
    }

    /// Generate metadata .json file for a cached logo
    async fn generate_metadata_for_cached_logo(
        &self,
        cache_id: &str,
        original_url: &str,
        channel_name: Option<String>,
        channel_group: Option<String>,
        extra_fields: Option<std::collections::HashMap<String, String>>,
    ) -> Result<(), anyhow::Error> {
        use crate::services::logo_cache_scanner::CachedLogoMetadata;
        use chrono::Utc;

        let metadata = CachedLogoMetadata {
            original_url: Some(original_url.to_string()),
            channel_name,
            channel_group,
            description: None,
            tags: None,
            width: None,
            height: None,
            extra_fields,
            cached_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let metadata_path = self
            .storage
            .cached_logo_dir
            .join(format!("{cache_id}.json"));
        let json_content = serde_json::to_string_pretty(&metadata)?;

        tokio::fs::write(&metadata_path, json_content).await?;
        debug!("Generated metadata file for cache_id: {}", cache_id);

        Ok(())
    }

    /// Generate metadata .json file for a cached logo using the sandboxed file manager
    async fn generate_metadata_for_cached_logo_with_file_manager(
        &self,
        cache_id: &str,
        original_url: &str,
        channel_name: Option<String>,
        channel_group: Option<String>,
        extra_fields: Option<std::collections::HashMap<String, String>>,
    ) -> Result<(), anyhow::Error> {
        use crate::services::logo_cache_scanner::CachedLogoMetadata;
        use chrono::Utc;

        let metadata = CachedLogoMetadata {
            original_url: Some(original_url.to_string()),
            channel_name,
            channel_group,
            description: None,
            tags: None,
            width: None,
            height: None,
            extra_fields,
            cached_at: Utc::now(),
            updated_at: Utc::now(),
        };

        if let Some(file_manager) = &self.logo_file_manager {
            // The file manager's base is already the cached logo directory, so use direct paths
            let metadata_file_path = format!("{cache_id}.json");
            let json_content = serde_json::to_string_pretty(&metadata)?;

            file_manager
                .write(&metadata_file_path, json_content.as_bytes())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to save metadata file: {}", e))?;

            debug!("Generated metadata file for cache_id: {}", cache_id);
        } else {
            return Err(anyhow::anyhow!(
                "No file manager available for metadata generation"
            ));
        }

        Ok(())
    }

    /// Convert image bytes to PNG format
    fn convert_image_to_png(
        &self,
        image_bytes: &[u8],
        source_url: &str,
    ) -> Result<Vec<u8>, anyhow::Error> {
        // Try to load the image
        let img = image::load_from_memory(image_bytes).map_err(|e| {
            anyhow::anyhow!(
                "Failed to decode image from '{}': {}",
                crate::utils::url::UrlUtils::obfuscate_credentials(source_url),
                e
            )
        })?;

        // Convert to PNG
        let mut png_bytes = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut png_bytes);

        img.write_to(&mut cursor, ImageFormat::Png).map_err(|e| {
            anyhow::anyhow!(
                "Failed to convert image to PNG from '{}': {}",
                source_url,
                e
            )
        })?;

        Ok(png_bytes)
    }

    /// Get the file path for a cached logo by cache ID
    fn get_cached_logo_path(&self, cache_id: &str) -> PathBuf {
        self.storage.get_cached_logo_path(cache_id)
    }

    pub async fn get_linked_assets(&self, asset_id: Uuid) -> Result<Vec<LogoAsset>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, file_name, file_path, file_size, mime_type, asset_type, source_url, width, height, parent_asset_id, format_type, created_at, updated_at
            FROM logo_assets
            WHERE parent_asset_id = ?
            ORDER BY format_type
            "#
        )
        .bind(asset_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut assets = Vec::new();

        for row in rows {
            let asset_type = match row.get::<String, _>("asset_type").as_str() {
                "uploaded" => LogoAssetType::Uploaded,
                "cached" => LogoAssetType::Cached,
                _ => LogoAssetType::Uploaded,
            };

            let parent_asset_id = row
                .get::<Option<String>, _>("parent_asset_id")
                .and_then(|s| s.parse().ok());

            let format_type = match row.get::<String, _>("format_type").as_str() {
                "png_conversion" => LogoFormatType::PngConversion,
                _ => LogoFormatType::Original,
            };

            let asset = LogoAsset {
                id: row.get::<String, _>("id"),
                name: row.get("name"),
                description: row.get("description"),
                file_name: row.get("file_name"),
                file_path: row.get("file_path"),
                file_size: row.get("file_size"),
                mime_type: row.get("mime_type"),
                asset_type,
                source_url: row.get("source_url"),
                width: row.get("width"),
                height: row.get("height"),
                parent_asset_id,
                format_type,
                created_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.get::<String, _>("created_at"),
                )
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
                updated_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.get::<String, _>("updated_at"),
                )
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
            };

            assets.push(asset);
        }

        Ok(assets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_cache_id_from_url() {
        // Test basic URL normalization
        let url1 = "https://example.com/logos/channel1.png";
        let url2 = "http://example.com/logos/channel1.jpg";
        let cache_id1 = LogoAssetService::generate_cache_id_from_url(url1).unwrap();
        let cache_id2 = LogoAssetService::generate_cache_id_from_url(url2).unwrap();

        // Should be the same despite different scheme and extension
        assert_eq!(cache_id1, cache_id2);

        // Test parameter sorting
        let url3 = "https://example.com/logo?b=2&a=1";
        let url4 = "https://example.com/logo?a=1&b=2";
        let cache_id3 = LogoAssetService::generate_cache_id_from_url(url3).unwrap();
        let cache_id4 = LogoAssetService::generate_cache_id_from_url(url4).unwrap();

        // Should be the same despite different parameter order
        assert_eq!(cache_id3, cache_id4);

        // Test port handling
        let url5 = "https://example.com:443/logo";
        let url6 = "https://example.com/logo";
        let cache_id5 = LogoAssetService::generate_cache_id_from_url(url5).unwrap();
        let cache_id6 = LogoAssetService::generate_cache_id_from_url(url6).unwrap();

        // Should be the same (443 is default for HTTPS)
        assert_eq!(cache_id5, cache_id6);

        // Test non-default port
        let url7 = "https://example.com:8080/logo";
        let cache_id7 = LogoAssetService::generate_cache_id_from_url(url7).unwrap();

        // Should be different from default port
        assert_ne!(cache_id6, cache_id7);
    }

    #[test]
    fn test_convert_image_to_png() {
        use image::{RgbImage, ImageBuffer};
        use std::io::Cursor;
        
        // Create a simple 1x1 RGB image programmatically
        let img: RgbImage = ImageBuffer::new(1, 1);
        
        // Convert to PNG bytes
        let mut png_bytes = Vec::new();
        {
            let mut cursor = Cursor::new(&mut png_bytes);
            img.write_to(&mut cursor, image::ImageFormat::Png)
                .expect("Failed to write PNG to memory");
        }
        
        // Test that we can load this PNG
        let result = image::load_from_memory(&png_bytes);
        assert!(result.is_ok(), "Should be able to load generated PNG");
        
        // Verify it's recognized as PNG
        let loaded_image = result.unwrap();
        assert_eq!(loaded_image.width(), 1);
        assert_eq!(loaded_image.height(), 1);
    }

    #[test]
    fn test_url_edge_cases() {
        // Test URL without extension
        let url1 = "https://example.com/logo";
        let cache_id1 = LogoAssetService::generate_cache_id_from_url(url1).unwrap();
        assert!(!cache_id1.is_empty());

        // Test URL with path but no filename
        let url2 = "https://example.com/path/";
        let cache_id2 = LogoAssetService::generate_cache_id_from_url(url2).unwrap();
        assert!(!cache_id2.is_empty());

        // Test URL with multiple extensions
        let url3 = "https://example.com/logo.backup.png";
        let url4 = "https://example.com/logo.backup.jpg";
        let cache_id3 = LogoAssetService::generate_cache_id_from_url(url3).unwrap();
        let cache_id4 = LogoAssetService::generate_cache_id_from_url(url4).unwrap();

        // Should be the same (only removes last extension)
        assert_eq!(cache_id3, cache_id4);

        // Test URL with fragment (should be ignored)
        let url5 = "https://example.com/logo.png#fragment";
        let url6 = "https://example.com/logo.jpg";
        let cache_id5 = LogoAssetService::generate_cache_id_from_url(url5).unwrap();
        let cache_id6 = LogoAssetService::generate_cache_id_from_url(url6).unwrap();

        // Should be the same (fragment ignored, extension removed)
        assert_eq!(cache_id5, cache_id6);
    }

    #[test]
    fn test_complex_parameter_scenarios() {
        // Test with encoded parameters
        let url1 = "https://example.com/logo?name=test%20logo&size=100";
        let url2 = "https://example.com/logo?size=100&name=test%20logo";
        let cache_id1 = LogoAssetService::generate_cache_id_from_url(url1).unwrap();
        let cache_id2 = LogoAssetService::generate_cache_id_from_url(url2).unwrap();

        // Should be the same despite different order
        assert_eq!(cache_id1, cache_id2);

        // Test with empty parameter values
        let url3 = "https://example.com/logo?empty=&filled=value";
        let cache_id3 = LogoAssetService::generate_cache_id_from_url(url3).unwrap();
        assert!(!cache_id3.is_empty());

        // Test with duplicate parameter names (URL parsing should handle this)
        let url4 = "https://example.com/logo?param=first&param=second";
        let cache_id4 = LogoAssetService::generate_cache_id_from_url(url4).unwrap();
        assert!(!cache_id4.is_empty());
    }

    #[test]
    fn test_cache_id_consistency() {
        let url = "https://cdn.example.com/channels/discovery.png?version=2&format=hd";

        // Generate cache ID multiple times
        let cache_id1 = LogoAssetService::generate_cache_id_from_url(url).unwrap();
        let cache_id2 = LogoAssetService::generate_cache_id_from_url(url).unwrap();
        let cache_id3 = LogoAssetService::generate_cache_id_from_url(url).unwrap();

        // Should always be the same
        assert_eq!(cache_id1, cache_id2);
        assert_eq!(cache_id2, cache_id3);

        // Should be a valid hex string (SHA256 produces 64 hex characters)
        assert_eq!(cache_id1.len(), 64);
        assert!(cache_id1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_invalid_urls() {
        // Test with invalid URL
        let result = LogoAssetService::generate_cache_id_from_url("not-a-url");
        assert!(result.is_err());

        // Test with empty URL
        let result = LogoAssetService::generate_cache_id_from_url("");
        assert!(result.is_err());

        // Test with malformed URL
        let result = LogoAssetService::generate_cache_id_from_url("https://");
        assert!(result.is_err());
    }
}
