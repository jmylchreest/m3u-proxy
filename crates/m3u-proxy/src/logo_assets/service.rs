use crate::logo_assets::storage::LogoAssetStorage;
use crate::entities::{logo_assets, prelude::LogoAssets};
use crate::models::logo_asset::{
    LogoAssetType, LogoFormatType, LogoAsset, LogoAssetListRequest, LogoAssetListResponse,
    LogoAssetWithUrl, LogoAssetSearchRequest, LogoAssetSearchResult, LogoCacheStats
};
use crate::utils::{StandardHttpClient, HttpClientFactory};

use anyhow;
use chrono::Utc;
use image::ImageFormat;
use sandboxed_file_manager::SandboxedManager;
use sha2::{Digest, Sha256};
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, ColumnTrait, QueryOrder};
use std::collections::BTreeMap;
use std::sync::Arc;

use std::path::PathBuf;
use tokio::fs;
use tracing::{debug, trace};
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
pub struct LogoAssetService {
    connection: Arc<DatabaseConnection>,
    pub storage: LogoAssetStorage,
    http_client: StandardHttpClient,
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
    pub async fn new(
        connection: Arc<DatabaseConnection>, 
        storage: LogoAssetStorage,
        http_client_factory: &HttpClientFactory
    ) -> Self {
        // Create HTTP client using the factory for consistent circuit breaker integration
        let http_client = http_client_factory
            .create_client_for_service("logo_fetch")
            .await;

        Self {
            connection,
            storage,
            http_client,
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
    ) -> Result<logo_assets::Model, anyhow::Error> {
        use sea_orm::{ActiveModelTrait, Set};
        
        let created_at = Utc::now();
        let updated_at = created_at.clone();

        // Create using SeaORM ActiveModel - clean database-agnostic approach
        let active_model = crate::entities::logo_assets::ActiveModel {
            id: Set(params.asset_id),
            name: Set(params.name.clone()),
            description: Set(params.description.clone()),
            file_name: Set(params.file_name.clone()),
            file_path: Set(params.file_path.clone()),
            file_size: Set(params.file_size as i32),
            mime_type: Set(params.mime_type.clone()),
            asset_type: Set(params.asset_type.to_string()),
            source_url: Set(params.source_url.clone()),
            width: Set(params.width.map(|w| w as i32)),
            height: Set(params.height.map(|h| h as i32)),
            parent_asset_id: Set(None),
            format_type: Set("original".to_string()),
            created_at: Set(created_at),
            updated_at: Set(updated_at),
        };

        active_model.insert(&*self.connection).await?;

        // Return the created entity directly - clean SeaORM approach
        LogoAssets::find_by_id(params.asset_id)
            .one(&*self.connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Failed to retrieve created asset"))
    }

    pub async fn get_asset(&self, asset_id: Uuid) -> Result<logo_assets::Model, anyhow::Error> {
        LogoAssets::find_by_id(asset_id)
            .one(&*self.connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Logo asset not found: {}", asset_id))
    }

    pub async fn list_assets(
        &self,
        request: LogoAssetListRequest,
        base_url: &str,
    ) -> Result<LogoAssetListResponse, anyhow::Error> {
        use sea_orm::{QueryFilter, QueryOrder, QuerySelect, ColumnTrait, PaginatorTrait};
        
        let limit = request.limit.unwrap_or(20);
        let page = request.page.unwrap_or(1);
        let offset = (page - 1) * limit;

        // Build SeaORM query with filters
        let mut find_query = LogoAssets::find()
            .filter(logo_assets::Column::FormatType.eq("original"));

        if let Some(search) = &request.search {
            find_query = find_query.filter(logo_assets::Column::Name.contains(search));
        }

        if let Some(asset_type) = &request.asset_type {
            let type_str = match asset_type {
                LogoAssetType::Uploaded => "uploaded",
                LogoAssetType::Cached => "cached",
            };
            find_query = find_query.filter(logo_assets::Column::AssetType.eq(type_str));
        }

        // Get total count for pagination
        let total_count = find_query.clone().count(&*self.connection).await? as u32;

        // Get paginated results
        let models = find_query
            .order_by_desc(logo_assets::Column::CreatedAt)
            .offset(offset as u64)
            .limit(limit as u64)
            .all(&*self.connection)
            .await?;

        // Convert SeaORM models to response format
        let mut converted_assets = Vec::new();
        for model in models {
            // Convert SeaORM entity to domain model for response compatibility
            let asset_type = match model.asset_type.as_str() {
                "uploaded" => LogoAssetType::Uploaded,
                "cached" => LogoAssetType::Cached,
                _ => LogoAssetType::Uploaded,
            };

            let format_type = match model.format_type.as_str() {
                "png_conversion" => LogoFormatType::PngConversion,
                _ => LogoFormatType::Original,
            };

            // Temporary: Create a compatible struct for the response
            // TODO: Rationalize response structures to use SeaORM entities directly
            use crate::models::logo_asset::LogoAsset;
            let domain_asset = LogoAsset {
                id: model.id.to_string(),
                name: model.name.clone(),
                description: model.description.clone(),
                file_name: model.file_name.clone(),
                file_path: model.file_path.clone(),
                file_size: model.file_size as i64,
                mime_type: model.mime_type.clone(),
                asset_type,
                source_url: model.source_url.clone(),
                width: model.width,
                height: model.height,
                parent_asset_id: model.parent_asset_id.map(|uuid| uuid.to_string()),
                format_type,
                created_at: model.created_at,
                updated_at: model.updated_at,
            };

            let url = format!("{}/api/v1/logos/{}", base_url.trim_end_matches('/'), model.id);
            converted_assets.push(LogoAssetWithUrl { 
                asset: domain_asset,
                url 
            });
        }
        let assets_with_urls = converted_assets;

        let total_pages = ((total_count as f64) / (limit as f64)).ceil() as u32;

        Ok(LogoAssetListResponse {
            assets: assets_with_urls,
            total_count: total_count as i64,
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
    ) -> Result<logo_assets::Model, anyhow::Error> {
        use sea_orm::{EntityTrait, ActiveModelTrait, Set};
        
        let updated_at = Utc::now();

        // Find the existing asset and update it using SeaORM ActiveModel
        let existing_asset = LogoAssets::find_by_id(asset_id)
            .one(&*self.connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Logo asset not found: {}", asset_id))?;

        let mut active_model: logo_assets::ActiveModel = existing_asset.into();
        active_model.name = Set(name);
        active_model.description = Set(description);
        active_model.updated_at = Set(updated_at);

        active_model.update(&*self.connection).await.map_err(Into::into)
    }

    pub async fn replace_asset_image(
        &self,
        asset_id: Uuid,
        file_name: String,
        file_path: String,
        file_size: i64,
        mime_type: String,
        width: Option<i32>,
        height: Option<i32>,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<logo_assets::Model, anyhow::Error> {
        use sea_orm::{EntityTrait, ActiveModelTrait, Set};

        // Find the existing asset
        let existing_asset = LogoAssets::find_by_id(asset_id)
            .one(&*self.connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Logo asset not found: {}", asset_id))?;

        // Update the database record
        let updated_at = Utc::now();
        let mut active_model: logo_assets::ActiveModel = existing_asset.into();
        
        active_model.file_name = Set(file_name);
        active_model.file_path = Set(file_path);
        active_model.file_size = Set(file_size as i32);
        active_model.mime_type = Set(mime_type);
        active_model.width = Set(width);
        active_model.height = Set(height);
        active_model.updated_at = Set(updated_at);
        
        // Update name and description if provided
        if let Some(new_name) = name {
            active_model.name = Set(new_name);
        }
        if let Some(new_description) = description {
            active_model.description = Set(Some(new_description));
        }

        active_model.update(&*self.connection).await.map_err(Into::into)
    }

    pub async fn delete_asset(&self, asset_id: Uuid) -> Result<(), anyhow::Error> {
        use sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                
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

            // Find linked assets for current asset using SeaORM
            let linked_models = LogoAssets::find()
                .filter(logo_assets::Column::ParentAssetId.eq(current_id))
                .all(&*self.connection)
                .await?;

            for linked_model in linked_models {
                let linked_id = linked_model.id;
                if !processed.contains(&linked_id) {
                    assets_to_delete.push(linked_id);
                }
            }
        }

        // Delete all assets from database using SeaORM batch delete
        for asset_id_to_delete in &processed {
            LogoAssets::delete_by_id(*asset_id_to_delete)
                .exec(&*self.connection)
                .await?;
        }

        // Delete file from storage
        if let Err(e) = self.storage.delete_file(&asset.file_path).await {
            tracing::error!("Failed to delete file for asset {}: {}", asset_id, e);
        }

        Ok(())
    }

    pub async fn search_assets(
        &self,
        request: LogoAssetSearchRequest,
        base_url: &str,
    ) -> Result<LogoAssetSearchResult, anyhow::Error> {
        use sea_orm::{QueryFilter, QueryOrder, QuerySelect, ColumnTrait};
        
        let limit = request.limit.unwrap_or(20);
        let search_query = request.query.unwrap_or_default();

        // Search uploaded logos using clean SeaORM queries
        let models = LogoAssets::find()
            .filter(logo_assets::Column::FormatType.eq("original"))
            .filter(logo_assets::Column::AssetType.eq("uploaded"))
            .filter(logo_assets::Column::Name.contains(&search_query))
            .order_by_asc(logo_assets::Column::Name)
            .limit(limit as u64)
            .all(&*self.connection)
            .await?;

        // Convert SeaORM models to response format - clean entity-based approach
        let mut assets = Vec::new();
        for model in models {
            // Convert SeaORM entity to domain model for response compatibility
            let asset_type = match model.asset_type.as_str() {
                "uploaded" => LogoAssetType::Uploaded,
                "cached" => LogoAssetType::Cached,
                _ => LogoAssetType::Uploaded,
            };

            let format_type = match model.format_type.as_str() {
                "png_conversion" => LogoFormatType::PngConversion,
                _ => LogoFormatType::Original,
            };

            // Create domain model for response compatibility
            use crate::models::logo_asset::LogoAsset;
            let domain_asset = LogoAsset {
                id: model.id.to_string(),
                name: model.name.clone(),
                description: model.description.clone(),
                file_name: model.file_name.clone(),
                file_path: model.file_path.clone(),
                file_size: model.file_size as i64,
                mime_type: model.mime_type.clone(),
                asset_type,
                source_url: model.source_url.clone(),
                width: model.width,
                height: model.height,
                parent_asset_id: model.parent_asset_id.map(|uuid| uuid.to_string()),
                format_type,
                created_at: model.created_at,
                updated_at: model.updated_at,
            };

            let url = format!(
                "{}/api/v1/logos/{}",
                base_url.trim_end_matches('/'),
                model.id
            );
            assets.push(LogoAssetWithUrl { asset: domain_asset, url });
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
                        width: cached_logo.metadata.as_ref().and_then(|m| m.width),
                        height: cached_logo.metadata.as_ref().and_then(|m| m.height),
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

    pub async fn get_cache_stats(&self) -> Result<LogoCacheStats, anyhow::Error> {
        // Get all logo assets to calculate statistics
        let all_assets = LogoAssets::find()
            .all(&*self.connection)
            .await?;

        let mut total_cached_logos = 0i64;
        let mut total_uploaded_logos = 0i64;
        let mut total_storage_used = 0i64;
        let mut total_linked_assets = 0i64;

        for asset in all_assets {
            // Count by asset type and format type
            match (asset.asset_type.as_str(), asset.format_type.as_str()) {
                ("cached", "original") => total_cached_logos += 1,
                ("uploaded", "original") => total_uploaded_logos += 1,
                (_, "png_conversion") => total_linked_assets += 1,
                _ => {}
            }
            
            // Sum file sizes
            total_storage_used += asset.file_size as i64;
        }

        Ok(LogoCacheStats {
            total_cached_logos,
            total_uploaded_logos,
            total_storage_used,
            total_linked_assets,
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
                // Dimensions will be extracted automatically when metadata is loaded by the scanner
                self.generate_metadata_for_cached_logo_with_file_manager(
                    &cache_id,
                    logo_url,
                    channel_name,
                    channel_group,
                    extra_fields,
                    None, // Dimensions will be extracted when metadata is loaded
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
                // Dimensions will be extracted automatically when metadata is loaded by the scanner
                self.generate_metadata_for_cached_logo(
                    &cache_id,
                    logo_url,
                    channel_name,
                    channel_group,
                    extra_fields,
                    None, // Dimensions will be extracted when metadata is loaded
                )
                .await?;
                return Ok((cache_id, 0)); // 0 bytes for cache hit
            }
        }

        debug!("Downloading logo from URL: {} -> {}", logo_url, cache_id);

        // Download the image using circuit breaker protected HTTP client
        let image_bytes = self.http_client.fetch_logo(logo_url).await.map_err(|e| {
            anyhow::anyhow!("Failed to download logo from '{}': {}", logo_url, e)
        })?;
        
        let raw_bytes_downloaded = image_bytes.len() as u64;

        // Convert to PNG and extract dimensions
        let png_bytes = self.convert_image_to_png(&image_bytes, logo_url)?;
        
        // Extract dimensions from the converted PNG
        let dimensions = self.extract_image_dimensions(&png_bytes).unwrap_or(None);

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
                    dimensions,
                ).await {
                    debug!("Failed to generate metadata for {}: {}", cache_id, e);
                }
            } else if let Err(e) = self.generate_metadata_for_cached_logo(
                &cache_id,
                logo_url,
                channel_name,
                channel_group,
                extra_fields,
                dimensions,
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
        dimensions: Option<(i32, i32)>,
    ) -> Result<(), anyhow::Error> {
        use crate::services::logo_cache_scanner::CachedLogoMetadata;
        use chrono::Utc;

        let metadata = CachedLogoMetadata {
            original_url: Some(original_url.to_string()),
            channel_name,
            channel_group,
            description: None,
            tags: None,
            width: dimensions.map(|(w, _)| w),
            height: dimensions.map(|(_, h)| h),
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
        dimensions: Option<(i32, i32)>,
    ) -> Result<(), anyhow::Error> {
        use crate::services::logo_cache_scanner::CachedLogoMetadata;
        use chrono::Utc;

        let metadata = CachedLogoMetadata {
            original_url: Some(original_url.to_string()),
            channel_name,
            channel_group,
            description: None,
            tags: None,
            width: dimensions.map(|(w, _)| w),
            height: dimensions.map(|(_, h)| h),
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

    /// Extract image dimensions from image bytes
    fn extract_image_dimensions(&self, image_bytes: &[u8]) -> Result<Option<(i32, i32)>, anyhow::Error> {
        match image::load_from_memory(image_bytes) {
            Ok(img) => Ok(Some((img.width() as i32, img.height() as i32))),
            Err(e) => {
                debug!("Failed to extract image dimensions: {}", e);
                Ok(None)
            }
        }
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

    pub async fn get_linked_assets(&self, asset_id: Uuid) -> Result<Vec<LogoAsset>, anyhow::Error> {
        let models = LogoAssets::find()
            .filter(logo_assets::Column::ParentAssetId.eq(asset_id))
            .order_by_asc(logo_assets::Column::FormatType)
            .all(&*self.connection)
            .await?;

        let mut assets = Vec::new();

        for model in models {
            let asset_type = match model.asset_type.as_str() {
                "uploaded" => LogoAssetType::Uploaded,
                "cached" => LogoAssetType::Cached,
                _ => LogoAssetType::Uploaded,
            };

            let parent_asset_id = model.parent_asset_id
                .map(|uuid| uuid.to_string());

            let format_type = match model.format_type.as_str() {
                "png_conversion" => LogoFormatType::PngConversion,
                _ => LogoFormatType::Original,
            };

            let asset = LogoAsset {
                id: model.id.to_string(),
                name: model.name,
                description: model.description,
                file_name: model.file_name,
                file_path: model.file_path,
                file_size: model.file_size as i64,
                mime_type: model.mime_type,
                asset_type,
                source_url: model.source_url,
                width: model.width,
                height: model.height,
                parent_asset_id,
                format_type,
                created_at: model.created_at,
                updated_at: model.updated_at,
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
