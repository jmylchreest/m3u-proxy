use crate::models::logo_asset::*;
use crate::utils;
use chrono::Utc;
use image::{GenericImageView, ImageFormat};
use sqlx::{Pool, Row, Sqlite};
use std::io::Cursor;
use uuid::Uuid;

#[derive(Clone)]
pub struct LogoAssetService {
    pool: Pool<Sqlite>,
}

impl LogoAssetService {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    pub async fn create_asset(
        &self,
        name: String,
        description: Option<String>,
        file_name: String,
        file_path: String,
        file_size: i64,
        mime_type: String,
        asset_type: LogoAssetType,
        source_url: Option<String>,
        width: Option<i32>,
        height: Option<i32>,
    ) -> Result<LogoAsset, sqlx::Error> {
        let asset_id = Uuid::new_v4();
        let now = Utc::now();

        let asset = LogoAsset {
            id: asset_id,
            name,
            description,
            file_name,
            file_path,
            file_size,
            mime_type,
            asset_type,
            source_url,
            width,
            height,
            parent_asset_id: None,
            format_type: crate::models::logo_asset::LogoFormatType::Original,
            created_at: now,
            updated_at: now,
        };

        let asset_type_str = format!("{:?}", asset.asset_type).to_lowercase();

        sqlx::query(
            r#"
            INSERT INTO logo_assets (id, name, description, file_name, file_path, file_size, mime_type, asset_type, source_url, width, height, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(asset.id.to_string())
        .bind(&asset.name)
        .bind(&asset.description)
        .bind(&asset.file_name)
        .bind(&asset.file_path)
        .bind(asset.file_size)
        .bind(&asset.mime_type)
        .bind(asset_type_str)
        .bind(&asset.source_url)
        .bind(asset.width)
        .bind(asset.height)
        .bind(asset.created_at.to_rfc3339())
        .bind(asset.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(asset)
    }

    pub async fn create_asset_with_id(
        &self,
        asset_id: Uuid,
        name: String,
        description: Option<String>,
        file_name: String,
        file_path: String,
        file_size: i64,
        mime_type: String,
        asset_type: LogoAssetType,
        source_url: Option<String>,
        width: Option<i32>,
        height: Option<i32>,
    ) -> Result<LogoAsset, sqlx::Error> {
        let now = Utc::now();

        let asset = LogoAsset {
            id: asset_id,
            name,
            description,
            file_name,
            file_path,
            file_size,
            mime_type,
            asset_type,
            source_url,
            width,
            height,
            parent_asset_id: None,
            format_type: crate::models::logo_asset::LogoFormatType::Original,
            created_at: now,
            updated_at: now,
        };

        let asset_type_str = format!("{:?}", asset.asset_type).to_lowercase();

        sqlx::query(
            r#"
            INSERT INTO logo_assets (id, name, description, file_name, file_path, file_size, mime_type, asset_type, source_url, width, height, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(asset.id.to_string())
        .bind(&asset.name)
        .bind(&asset.description)
        .bind(&asset.file_name)
        .bind(&asset.file_path)
        .bind(asset.file_size)
        .bind(&asset.mime_type)
        .bind(asset_type_str)
        .bind(&asset.source_url)
        .bind(asset.width)
        .bind(asset.height)
        .bind(asset.created_at.to_rfc3339())
        .bind(asset.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(asset)
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

        let format_type = match row
            .get::<Option<String>, _>("format_type")
            .as_deref()
            .unwrap_or("original")
        {
            "png_conversion" => crate::models::logo_asset::LogoFormatType::PngConversion,
            _ => crate::models::logo_asset::LogoFormatType::Original,
        };

        let parent_asset_id = row
            .get::<Option<String>, _>("parent_asset_id")
            .map(|id| Uuid::parse_str(&id).ok())
            .flatten();

        Ok(LogoAsset {
            id: Uuid::parse_str(&row.get::<String, _>("id")).unwrap(),
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
            created_at: utils::parse_datetime(&row.get::<String, _>("created_at"))?,
            updated_at: utils::parse_datetime(&row.get::<String, _>("updated_at"))?,
        })
    }

    pub async fn list_assets(
        &self,
        request: LogoAssetListRequest,
    ) -> Result<LogoAssetListResponse, sqlx::Error> {
        let page = request.page.unwrap_or(1);
        let limit = request.limit.unwrap_or(50);
        let offset = (page - 1) * limit;

        let mut query = "SELECT id, name, description, file_name, file_path, file_size, mime_type, asset_type, source_url, width, height, parent_asset_id, format_type, created_at, updated_at FROM logo_assets WHERE format_type = 'original'".to_string();
        let mut count_query =
            "SELECT COUNT(*) as count FROM logo_assets WHERE format_type = 'original'".to_string();

        if let Some(search) = &request.search {
            query.push_str(&format!(" AND name LIKE '%{}%'", search));
            count_query.push_str(&format!(" AND name LIKE '%{}%'", search));
        }

        if let Some(asset_type) = &request.asset_type {
            let type_str = format!("{:?}", asset_type).to_lowercase();
            query.push_str(&format!(" AND asset_type = '{}'", type_str));
            count_query.push_str(&format!(" AND asset_type = '{}'", type_str));
        }

        query.push_str(&format!(
            " ORDER BY created_at DESC LIMIT {} OFFSET {}",
            limit, offset
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

            let format_type = match row
                .get::<Option<String>, _>("format_type")
                .as_deref()
                .unwrap_or("original")
            {
                "png_conversion" => crate::models::logo_asset::LogoFormatType::PngConversion,
                _ => crate::models::logo_asset::LogoFormatType::Original,
            };

            let parent_asset_id = row
                .get::<Option<String>, _>("parent_asset_id")
                .map(|id| Uuid::parse_str(&id).ok())
                .flatten();

            let asset = LogoAsset {
                id: Uuid::parse_str(&row.get::<String, _>("id")).unwrap(),
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
                created_at: utils::parse_datetime(&row.get::<String, _>("created_at"))?,
                updated_at: utils::parse_datetime(&row.get::<String, _>("updated_at"))?,
            };

            let url = format!("/api/logos/{}", asset.id);
            assets.push(LogoAssetWithUrl { asset, url });
        }

        let total_pages = ((total_count as f64) / (limit as f64)).ceil() as u32;

        Ok(LogoAssetListResponse {
            assets,
            total_count,
            page,
            limit,
            total_pages,
        })
    }

    pub async fn update_asset(
        &self,
        asset_id: Uuid,
        request: LogoAssetUpdateRequest,
    ) -> Result<LogoAsset, sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE logo_assets
            SET name = ?, description = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&request.name)
        .bind(&request.description)
        .bind(Utc::now().to_rfc3339())
        .bind(asset_id.to_string())
        .execute(&self.pool)
        .await?;

        self.get_asset(asset_id).await
    }

    pub async fn delete_asset(&self, asset_id: Uuid) -> Result<(), sqlx::Error> {
        // First, check if this asset has linked assets (PNG conversions)
        let linked_assets = sqlx::query("SELECT id FROM logo_assets WHERE parent_asset_id = ?")
            .bind(asset_id.to_string())
            .fetch_all(&self.pool)
            .await?;

        // Delete all linked assets first
        for linked_row in linked_assets {
            let linked_id: String = linked_row.get("id");
            sqlx::query("DELETE FROM logo_assets WHERE id = ?")
                .bind(linked_id)
                .execute(&self.pool)
                .await?;
        }

        // Then delete the main asset
        sqlx::query("DELETE FROM logo_assets WHERE id = ?")
            .bind(asset_id.to_string())
            .execute(&self.pool)
            .await?;

        tracing::info!("Deleted asset {} and its linked assets", asset_id);
        Ok(())
    }

    pub async fn search_assets(
        &self,
        request: LogoAssetSearchRequest,
    ) -> Result<LogoAssetSearchResult, sqlx::Error> {
        let limit = request.limit.unwrap_or(20);

        let query = format!(
            "SELECT id, name, description, file_name, file_path, file_size, mime_type, asset_type, source_url, width, height, parent_asset_id, format_type, created_at, updated_at FROM logo_assets WHERE format_type = 'original' AND name LIKE '%{}%' ORDER BY name LIMIT {}",
            request.query, limit
        );

        let rows = sqlx::query(&query).fetch_all(&self.pool).await?;

        let mut assets = Vec::new();
        for row in rows {
            let asset_type = match row.get::<String, _>("asset_type").as_str() {
                "uploaded" => LogoAssetType::Uploaded,
                "cached" => LogoAssetType::Cached,
                _ => LogoAssetType::Uploaded,
            };

            let format_type = match row
                .get::<Option<String>, _>("format_type")
                .as_deref()
                .unwrap_or("original")
            {
                "png_conversion" => crate::models::logo_asset::LogoFormatType::PngConversion,
                _ => crate::models::logo_asset::LogoFormatType::Original,
            };

            let parent_asset_id = row
                .get::<Option<String>, _>("parent_asset_id")
                .map(|id| Uuid::parse_str(&id).ok())
                .flatten();

            let asset = LogoAsset {
                id: Uuid::parse_str(&row.get::<String, _>("id")).unwrap(),
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
                created_at: utils::parse_datetime(&row.get::<String, _>("created_at"))?,
                updated_at: utils::parse_datetime(&row.get::<String, _>("updated_at"))?,
            };

            let url = format!("/api/logos/{}", asset.id);
            assets.push(LogoAssetWithUrl { asset, url });
        }

        Ok(LogoAssetSearchResult {
            total_count: assets.len(),
            assets,
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
        })
    }

    pub async fn create_asset_from_upload(
        &self,
        request: LogoAssetCreateRequest,
        file_data: &[u8],
        _content_type: &str,
    ) -> Result<LogoAsset, Box<dyn std::error::Error + Send + Sync>> {
        use crate::logo_assets::LogoAssetStorage;

        // Generate UUID for this asset
        let asset_id = Uuid::new_v4();

        // Detect file extension
        let file_extension = if file_data.starts_with(b"<svg") || file_data.starts_with(b"<?xml") {
            "svg"
        } else {
            let format = Self::detect_image_format(file_data)?;
            match format {
                ImageFormat::Png => "png",
                ImageFormat::Jpeg => "jpg",
                ImageFormat::Gif => "gif",
                ImageFormat::WebP => "webp",
                _ => "png",
            }
        };

        // Save file to storage
        let storage = LogoAssetStorage::new(
            std::path::PathBuf::from("./data/logos/uploaded"),
            std::path::PathBuf::from("./data/logos/cached"),
        );
        let (file_name, file_path, file_size, mime_type, dimensions) = storage
            .save_uploaded_file(file_data.to_vec(), asset_id, file_extension)
            .await
            .map_err(|e| format!("Failed to save file: {}", e))?;

        // Extract dimensions if available
        let (width, height) = dimensions
            .map(|(w, h)| (Some(w as i32), Some(h as i32)))
            .unwrap_or((None, None));

        // Create database record with the pre-generated UUID
        let asset = self
            .create_asset_with_id(
                asset_id,
                request.name,
                request.description,
                file_name,
                file_path,
                file_size,
                mime_type,
                LogoAssetType::Uploaded,
                None, // source_url
                width,
                height,
            )
            .await?;

        Ok(asset)
    }

    /// Convert image data to PNG format if it's not already PNG
    fn convert_to_png(
        &self,
        image_data: &[u8],
        original_format: ImageFormat,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        tracing::debug!(
            "PNG conversion check: format={:?}, size={} bytes",
            original_format,
            image_data.len()
        );

        // If it's already PNG, no conversion needed
        if matches!(original_format, ImageFormat::Png) {
            tracing::debug!("Skipping PNG conversion: already PNG format");
            return Ok(None);
        }

        // Special handling for SVG - we can't convert it with the image crate
        // but we should still create a PNG "conversion" entry for consistency
        if image_data.starts_with(b"<svg") || image_data.starts_with(b"<?xml") {
            tracing::debug!("Skipping PNG conversion: SVG format cannot be converted");
            return Ok(None);
        }

        tracing::info!("Converting {:?} to PNG format", original_format);

        // Convert to PNG for other formats
        match image::load_from_memory(image_data) {
            Ok(img) => {
                let mut png_data = Vec::new();
                let mut cursor = Cursor::new(&mut png_data);

                match img.write_to(&mut cursor, ImageFormat::Png) {
                    Ok(_) => {
                        tracing::info!(
                            "PNG conversion successful: {:?} -> PNG, original_size={} bytes, png_size={} bytes",
                            original_format,
                            image_data.len(),
                            png_data.len()
                        );
                        Ok(Some(png_data))
                    }
                    Err(e) => {
                        tracing::error!("Failed to write PNG data: {}", e);
                        Err(Box::new(e))
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to load image for PNG conversion: {}", e);
                Err(Box::new(e))
            }
        }
    }

    /// Detect image format from file data
    fn detect_image_format(
        data: &[u8],
    ) -> Result<ImageFormat, Box<dyn std::error::Error + Send + Sync>> {
        if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            Ok(ImageFormat::Png)
        } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Ok(ImageFormat::Jpeg)
        } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
            Ok(ImageFormat::Gif)
        } else if data.starts_with(b"RIFF") && data.len() > 12 && &data[8..12] == b"WEBP" {
            Ok(ImageFormat::WebP)
        } else if data.starts_with(b"<svg") || data.starts_with(b"<?xml") {
            // SVG detection - we'll handle this separately since image crate doesn't support SVG
            Ok(ImageFormat::from_extension("svg").unwrap_or(ImageFormat::Png))
        } else {
            // Try to detect using image crate
            let format = image::guess_format(data)?;
            Ok(format)
        }
    }

    /// Create asset with automatic PNG conversion for non-PNG images
    pub async fn create_asset_from_upload_with_conversion(
        &self,
        request: LogoAssetCreateRequest,
        data: &[u8],
        content_type: &str,
    ) -> Result<LogoAsset, Box<dyn std::error::Error + Send + Sync>> {
        use crate::logo_assets::LogoAssetStorage;

        tracing::info!(
            "Starting logo upload process: name='{}', size={} bytes, content_type='{}'",
            request.name,
            data.len(),
            content_type
        );

        // Generate UUID for the original asset
        let original_asset_id = Uuid::new_v4();

        // Initialize storage
        let storage = LogoAssetStorage::new(
            std::path::PathBuf::from("./data/logos/uploaded"),
            std::path::PathBuf::from("./data/logos/cached"),
        );

        // Detect the original format
        let original_format = Self::detect_image_format(data)?;

        tracing::debug!(
            "Detected image format: {:?} for upload '{}'",
            original_format,
            request.name
        );

        // Get image dimensions - special handling for SVG
        let (width, height) = if data.starts_with(b"<svg") || data.starts_with(b"<?xml") {
            tracing::debug!("SVG file detected, using default dimensions");
            (None, None)
        } else {
            let img = image::load_from_memory(data)?;
            let (w, h) = img.dimensions();
            tracing::debug!("Image dimensions: {}x{}", w, h);
            (Some(w as i32), Some(h as i32))
        };

        // Determine file extension
        let file_extension = if data.starts_with(b"<svg") || data.starts_with(b"<?xml") {
            "svg"
        } else {
            match original_format {
                ImageFormat::Png => "png",
                ImageFormat::Jpeg => "jpg",
                ImageFormat::Gif => "gif",
                ImageFormat::WebP => "webp",
                _ => "img",
            }
        };

        // Save original file to storage
        let (file_name, file_path, file_size, mime_type, storage_dimensions) = storage
            .save_uploaded_file(data.to_vec(), original_asset_id, file_extension)
            .await
            .map_err(|e| {
                tracing::error!("Failed to save original file for '{}': {}", request.name, e);
                format!("Failed to save original file: {}", e)
            })?;

        tracing::info!(
            "Saved original file: name='{}', file_name='{}', file_path='{}', size={} bytes",
            request.name,
            file_name,
            file_path,
            file_size
        );

        // Use storage dimensions if image dimensions weren't detected earlier
        let final_dimensions = if width.is_none() && height.is_none() {
            storage_dimensions
                .map(|(w, h)| (Some(w as i32), Some(h as i32)))
                .unwrap_or((None, None))
        } else {
            (width, height)
        };

        // Create the original asset database record with pre-generated UUID
        let original_asset = self
            .create_asset_with_id(
                original_asset_id,
                request.name.clone(),
                request.description.clone(),
                file_name.clone(),
                file_path.clone(),
                file_size,
                mime_type.clone(),
                LogoAssetType::Uploaded,
                None, // source_url
                final_dimensions.0,
                final_dimensions.1,
            )
            .await
            .map_err(|e| {
                tracing::error!(
                    "Failed to create database record for original asset '{}': {}",
                    request.name,
                    e
                );
                e
            })?;

        tracing::info!(
            "Created original asset record: id={}, format_type=original, mime_type='{}'",
            original_asset.id,
            mime_type
        );

        // Convert to PNG if needed and beneficial
        let should_convert = !matches!(original_format, ImageFormat::Png)
            && !data.starts_with(b"<svg")
            && !data.starts_with(b"<?xml");

        if should_convert {
            tracing::debug!("Attempting PNG conversion for '{}'", request.name);

            if let Some(png_data) = self.convert_to_png(data, original_format)? {
                tracing::info!(
                    "PNG conversion successful: original_size={} bytes, png_size={} bytes, compression_ratio={:.2}%",
                    data.len(),
                    png_data.len(),
                    (1.0 - (png_data.len() as f64 / data.len() as f64)) * 100.0
                );

                // Use same UUID as original for PNG conversion (different extension)
                let png_asset_id = Uuid::new_v4(); // Still need unique DB record ID

                // Save PNG file to storage with original asset UUID but .png extension
                let (png_file_name, png_file_path, png_file_size, png_mime_type, png_dimensions) =
                    storage
                        .save_converted_file(png_data.clone(), original_asset_id, "png")
                        .await
                        .map_err(|e| {
                            tracing::error!(
                                "Failed to save PNG conversion file for '{}': {}",
                                request.name,
                                e
                            );
                            format!("Failed to save PNG conversion file: {}", e)
                        })?;

                tracing::info!(
                    "Saved PNG conversion file: file_name='{}', file_path='{}', size={} bytes",
                    png_file_name,
                    png_file_path,
                    png_file_size
                );

                // Create PNG conversion asset linked to original
                let mut png_asset = self
                    .create_asset_with_id(
                        png_asset_id,
                        format!("{} (PNG)", request.name),
                        Some("Automatically converted PNG version".to_string()),
                        png_file_name.clone(),
                        png_file_path.clone(),
                        png_file_size,
                        png_mime_type.clone(),
                        LogoAssetType::Uploaded,
                        None, // source_url
                        png_dimensions.map(|(w, _)| w as i32),
                        png_dimensions.map(|(_, h)| h as i32),
                    )
                    .await
                    .map_err(|e| {
                        tracing::error!(
                            "Failed to create database record for PNG conversion asset '{}': {}",
                            request.name,
                            e
                        );
                        e
                    })?;

                // Update the PNG asset to link it to the original and set format type
                sqlx::query(
                    r#"
                    UPDATE logo_assets
                    SET parent_asset_id = ?, format_type = ?
                    WHERE id = ?
                    "#,
                )
                .bind(original_asset.id.to_string())
                .bind("png_conversion")
                .bind(png_asset.id.to_string())
                .execute(&self.pool)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to link PNG conversion to original asset: {}", e);
                    e
                })?;

                png_asset.parent_asset_id = Some(original_asset.id);
                png_asset.format_type = LogoFormatType::PngConversion;

                tracing::info!(
                    "Successfully created linked PNG conversion: id={}, parent_id={}, format_type=png_conversion",
                    png_asset.id,
                    original_asset.id
                );
            } else {
                tracing::warn!(
                    "PNG conversion was attempted but returned None for '{}'",
                    request.name
                );
            }
        } else {
            tracing::debug!(
                "Skipping PNG conversion for '{}': format={:?}, is_svg={}",
                request.name,
                original_format,
                data.starts_with(b"<svg") || data.starts_with(b"<?xml")
            );
        }

        tracing::info!(
            "Logo upload completed successfully: name='{}', id={}, format='{}', final_size={} bytes",
            request.name,
            original_asset.id,
            file_extension,
            file_size
        );

        Ok(original_asset)
    }

    /// Get linked assets (PNG conversions) for a primary asset
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

            let format_type = match row
                .get::<Option<String>, _>("format_type")
                .as_deref()
                .unwrap_or("original")
            {
                "png_conversion" => crate::models::logo_asset::LogoFormatType::PngConversion,
                _ => crate::models::logo_asset::LogoFormatType::Original,
            };

            let parent_asset_id = row
                .get::<Option<String>, _>("parent_asset_id")
                .map(|id| Uuid::parse_str(&id).ok())
                .flatten();

            let asset = LogoAsset {
                id: Uuid::parse_str(&row.get::<String, _>("id")).unwrap(),
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
                created_at: utils::parse_datetime(&row.get::<String, _>("created_at"))?,
                updated_at: utils::parse_datetime(&row.get::<String, _>("updated_at"))?,
            };

            assets.push(asset);
        }

        Ok(assets)
    }
}
