use anyhow::Result;
use chrono::Utc;
use std::path::PathBuf;
use uuid::Uuid;

use crate::config::StorageConfig;
use crate::models::*;

#[allow(dead_code)]
pub struct ProxyGenerator {
    storage_config: StorageConfig,
}

impl ProxyGenerator {
    #[allow(dead_code)]
    pub fn new(storage_config: StorageConfig) -> Self {
        Self { storage_config }
    }

    #[allow(dead_code)]
    pub async fn generate(&self, proxy: &StreamProxy) -> Result<ProxyGeneration> {
        // TODO: Implement full proxy generation logic
        // 1. Get all sources attached to this proxy
        // 2. Get all channels from those sources
        // 3. Get all active filters for this proxy (sorted by order)
        // 4. Apply filters to channels
        // 5. Generate M3U content
        // 6. Cache logos and update URLs
        // 7. Save to database and disk

        let m3u_content = self.generate_m3u_content(&[]).await?;

        Ok(ProxyGeneration {
            id: Uuid::new_v4(),
            proxy_id: proxy.id,
            version: 1, // TODO: Get next version number
            channel_count: 0,
            m3u_content,
            created_at: Utc::now(),
        })
    }

    #[allow(dead_code)]
    async fn generate_m3u_content(&self, channels: &[Channel]) -> Result<String> {
        let mut m3u = String::from("#EXTM3U\n");

        for (index, channel) in channels.iter().enumerate() {
            let channel_number = index + 1;

            // Build EXTINF line
            let mut extinf = format!("#EXTINF:-1");

            if let Some(tvg_id) = &channel.tvg_id {
                extinf.push_str(&format!(" tvg-id=\"{}\"", tvg_id));
            }

            if let Some(tvg_name) = &channel.tvg_name {
                extinf.push_str(&format!(" tvg-name=\"{}\"", tvg_name));
            }

            if let Some(tvg_logo) = &channel.tvg_logo {
                // TODO: Replace with cached logo URL
                extinf.push_str(&format!(" tvg-logo=\"{}\"", tvg_logo));
            }

            if let Some(group_title) = &channel.group_title {
                extinf.push_str(&format!(" group-title=\"{}\"", group_title));
            }

            extinf.push_str(&format!(" tvg-chno=\"{}\"", channel_number));
            extinf.push_str(&format!(",{}\n", channel.channel_name));

            m3u.push_str(&extinf);
            m3u.push_str(&format!("{}\n", channel.stream_url));
        }

        Ok(m3u)
    }

    #[allow(dead_code)]
    async fn cache_logos(&self, _channels: &mut [Channel]) -> Result<()> {
        // TODO: Implement logo caching
        // 1. For each channel with a logo URL
        // 2. Generate ULID hash of the URL
        // 3. Check if logo already exists on disk
        // 4. If not, download and cache it
        // 5. Update channel.tvg_logo to point to local cached version
        Ok(())
    }

    /// Save M3U content to the configured storage path
    #[allow(dead_code)]
    pub async fn save_m3u_file(&self, proxy_id: Uuid, content: &str) -> Result<PathBuf> {
        // Ensure the M3U storage directory exists
        std::fs::create_dir_all(&self.storage_config.m3u_path)?;

        // Generate filename: proxy_id.m3u8
        let filename = format!("{}.m3u8", proxy_id);
        let file_path = self.storage_config.m3u_path.join(filename);

        // Write content to file
        std::fs::write(&file_path, content)?;

        Ok(file_path)
    }

    /// Get the storage path for M3U files
    #[allow(dead_code)]
    pub fn get_m3u_storage_path(&self) -> &PathBuf {
        &self.storage_config.m3u_path
    }

    /// Get the storage path for logos
    #[allow(dead_code)]
    pub fn get_logo_storage_path(&self) -> &PathBuf {
        &self.storage_config.cached_logo_path
    }

    /// Clean up old proxy versions (keep only the configured number)
    #[allow(dead_code)]
    pub async fn cleanup_old_versions(&self, proxy_id: Uuid) -> Result<()> {
        let m3u_dir = &self.storage_config.m3u_path;
        if !m3u_dir.exists() {
            return Ok(());
        }

        // Find all files matching proxy_id pattern
        let proxy_pattern = format!("{}_", proxy_id);
        let mut versions = Vec::new();

        for entry in std::fs::read_dir(m3u_dir)? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().to_string();

            if file_name.starts_with(&proxy_pattern) && file_name.ends_with(".m3u8") {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        versions.push((file_name, modified, entry.path()));
                    }
                }
            }
        }

        // Sort by modification time (newest first)
        versions.sort_by(|a, b| b.1.cmp(&a.1));

        // Keep only the configured number of versions
        let keep_count = self.storage_config.proxy_versions_to_keep as usize;
        if versions.len() > keep_count {
            for (_, _, path) in versions.into_iter().skip(keep_count) {
                if let Err(e) = std::fs::remove_file(&path) {
                    tracing::warn!(
                        "Failed to remove old proxy version {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }

        Ok(())
    }
}
