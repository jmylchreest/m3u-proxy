use anyhow::Result;
use std::path::PathBuf;
use tokio::fs;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct LogoAssetStorage {
    pub uploaded_logo_dir: PathBuf,
    pub cached_logo_dir: PathBuf,
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
    ) -> Result<(String, String, i64, String, Option<(u32, u32)>)> {
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
    ) -> Result<(String, String, i64, String, Option<(u32, u32)>)> {
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

    fn get_image_dimensions(data: &[u8]) -> Result<Option<(u32, u32)>> {
        match image::load_from_memory(data) {
            Ok(img) => Ok(Some((img.width(), img.height()))),
            Err(_) => Ok(None),
        }
    }

    /// Get the file path for a cached logo by cache ID
    pub fn get_cached_logo_path(&self, cache_id: &str) -> PathBuf {
        self.cached_logo_dir.join(format!("{}.png", cache_id))
    }
}
