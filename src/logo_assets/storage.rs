use image::ImageFormat;
use std::path::PathBuf;
use tokio::fs;
use uuid::Uuid;

#[derive(Clone)]
pub struct LogoAssetStorage {
    uploaded_logo_dir: PathBuf,
    cached_logo_dir: PathBuf,
}

impl LogoAssetStorage {
    pub fn new(uploaded_logo_dir: PathBuf, cached_logo_dir: PathBuf) -> Self {
        Self {
            uploaded_logo_dir,
            cached_logo_dir,
        }
    }

    pub async fn ensure_storage_dirs(&self) -> Result<(), std::io::Error> {
        if !self.uploaded_logo_dir.exists() {
            fs::create_dir_all(&self.uploaded_logo_dir).await?;
        }
        if !self.cached_logo_dir.exists() {
            fs::create_dir_all(&self.cached_logo_dir).await?;
        }
        Ok(())
    }

    pub async fn save_uploaded_file(
        &self,
        file_data: Vec<u8>,
        asset_id: Uuid,
        file_extension: &str,
    ) -> Result<(String, String, i64, String, Option<(u32, u32)>), Box<dyn std::error::Error>> {
        self.ensure_storage_dirs().await?;

        let file_name = format!("{}.{}", asset_id, file_extension);
        let relative_path = format!("uploaded/{}", file_name);
        let file_path = self.uploaded_logo_dir.join(&file_name);

        let mime_type = match file_extension.to_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream",
        }
        .to_string();

        let dimensions = if mime_type.starts_with("image/") && mime_type != "image/svg+xml" {
            Self::get_image_dimensions(&file_data)?
        } else {
            None
        };

        fs::write(&file_path, &file_data).await?;

        let file_size = file_data.len() as i64;

        Ok((file_name, relative_path, file_size, mime_type, dimensions))
    }

    pub async fn save_converted_file(
        &self,
        file_data: Vec<u8>,
        asset_id: Uuid,
        file_extension: &str,
    ) -> Result<(String, String, i64, String, Option<(u32, u32)>), Box<dyn std::error::Error>> {
        self.ensure_storage_dirs().await?;

        let file_name = format!("{}.{}", asset_id, file_extension);
        let relative_path = format!("uploaded/{}", file_name);
        let file_path = self.uploaded_logo_dir.join(&file_name);

        let mime_type = match file_extension.to_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream",
        }
        .to_string();

        let dimensions = Self::get_image_dimensions(&file_data)?;

        fs::write(&file_path, &file_data).await?;

        let file_size = file_data.len() as i64;

        Ok((file_name, relative_path, file_size, mime_type, dimensions))
    }

    pub async fn save_cached_logo(
        &self,
        logo_data: Vec<u8>,
        source_url: &str,
    ) -> Result<(String, String, i64, String, Option<(u32, u32)>), Box<dyn std::error::Error>> {
        self.ensure_storage_dirs().await?;

        let url_hash = format!("{:x}", md5::compute(source_url.as_bytes()));

        let format = Self::detect_image_format(&logo_data)?;
        let extension = match format {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Gif => "gif",
            ImageFormat::WebP => "webp",
            _ => "png",
        };

        let file_name = format!("cached_{}.{}", url_hash, extension);
        let relative_path = format!("cached/{}", file_name);
        let file_path = self.cached_logo_dir.join(&file_name);

        if file_path.exists() {
            let metadata = fs::metadata(&file_path).await?;
            let file_size = metadata.len() as i64;
            let mime_type = Self::format_to_mime_type(format);
            let dimensions = Self::get_image_dimensions(&logo_data)?;
            return Ok((
                file_name.clone(),
                relative_path,
                file_size,
                mime_type,
                dimensions,
            ));
        }

        let dimensions = Self::get_image_dimensions(&logo_data)?;
        fs::write(&file_path, &logo_data).await?;

        let file_size = logo_data.len() as i64;
        let mime_type = Self::format_to_mime_type(format);

        Ok((
            file_name.clone(),
            relative_path,
            file_size,
            mime_type,
            dimensions,
        ))
    }

    pub async fn delete_file(&self, file_path: &str) -> Result<(), std::io::Error> {
        let full_path = if file_path.starts_with("uploaded/") {
            self.uploaded_logo_dir
                .join(file_path.strip_prefix("uploaded/").unwrap())
        } else if file_path.starts_with("cached/") {
            self.cached_logo_dir
                .join(file_path.strip_prefix("cached/").unwrap())
        } else {
            // Legacy support - try uploaded first, then cached
            let uploaded_path = self.uploaded_logo_dir.join(file_path);
            if uploaded_path.exists() {
                uploaded_path
            } else {
                self.cached_logo_dir.join(file_path)
            }
        };

        if full_path.exists() {
            fs::remove_file(full_path).await?;
        }
        Ok(())
    }

    pub async fn get_file(&self, file_path: &str) -> Result<Vec<u8>, std::io::Error> {
        let full_path = if file_path.starts_with("uploaded/") {
            self.uploaded_logo_dir
                .join(file_path.strip_prefix("uploaded/").unwrap())
        } else if file_path.starts_with("cached/") {
            self.cached_logo_dir
                .join(file_path.strip_prefix("cached/").unwrap())
        } else {
            // Legacy support - try uploaded first, then cached
            let uploaded_path = self.uploaded_logo_dir.join(file_path);
            if uploaded_path.exists() {
                uploaded_path
            } else {
                self.cached_logo_dir.join(file_path)
            }
        };

        fs::read(full_path).await
    }

    pub fn get_file_path(&self, relative_path: &str) -> PathBuf {
        if relative_path.starts_with("uploaded/") {
            self.uploaded_logo_dir
                .join(relative_path.strip_prefix("uploaded/").unwrap())
        } else if relative_path.starts_with("cached/") {
            self.cached_logo_dir
                .join(relative_path.strip_prefix("cached/").unwrap())
        } else {
            // Legacy support - default to uploaded
            self.uploaded_logo_dir.join(relative_path)
        }
    }

    fn detect_image_format(data: &[u8]) -> Result<ImageFormat, Box<dyn std::error::Error>> {
        if data.len() < 8 {
            return Err("Invalid image data".into());
        }

        if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
            Ok(ImageFormat::Png)
        } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Ok(ImageFormat::Jpeg)
        } else if data.starts_with(&[0x47, 0x49, 0x46]) {
            Ok(ImageFormat::Gif)
        } else if data.len() >= 12 && &data[8..12] == b"WEBP" {
            Ok(ImageFormat::WebP)
        } else {
            Ok(ImageFormat::Png)
        }
    }

    fn format_to_mime_type(format: ImageFormat) -> String {
        match format {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Gif => "image/gif",
            ImageFormat::WebP => "image/webp",
            _ => "image/png",
        }
        .to_string()
    }

    fn get_image_dimensions(data: &[u8]) -> Result<Option<(u32, u32)>, Box<dyn std::error::Error>> {
        match image::load_from_memory(data) {
            Ok(img) => Ok(Some((img.width(), img.height()))),
            Err(_) => Ok(None),
        }
    }

    pub async fn download_logo(url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("m3u-proxy/1.0")
            .build()?;

        let response = client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(format!("Failed to download logo: HTTP {}", response.status()).into());
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !content_type.starts_with("image/") {
            return Err("URL does not point to an image".into());
        }

        let bytes = response.bytes().await?;
        if bytes.len() > 10 * 1024 * 1024 {
            return Err("Image too large (max 10MB)".into());
        }

        Ok(bytes.to_vec())
    }
}
