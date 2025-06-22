use anyhow::Result;
use chrono::Utc;
use std::path::PathBuf;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::StorageConfig;
use crate::data_mapping::service::DataMappingService;
use crate::database::Database;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;
use crate::proxy::filter_engine::FilterEngine;

pub struct ProxyGenerator {
    storage_config: StorageConfig,
}

impl ProxyGenerator {
    pub fn new(storage_config: StorageConfig) -> Self {
        Self { storage_config }
    }

    /// Generate a complete proxy M3U with data mapping and filters applied
    pub async fn generate(
        &self,
        proxy: &StreamProxy,
        database: &Database,
        data_mapping_service: &DataMappingService,
        logo_service: &LogoAssetService,
        base_url: &str,
    ) -> Result<ProxyGeneration> {
        info!("Starting proxy generation for '{}'", proxy.name);

        // Step 1: Get all sources attached to this proxy
        let sources = database.get_proxy_sources(proxy.id).await?;
        info!("Found {} sources for proxy '{}'", sources.len(), proxy.name);

        if sources.is_empty() {
            warn!(
                "No sources found for proxy '{}', generating empty M3U",
                proxy.name
            );
            let m3u_content = "#EXTM3U\n".to_string();
            return Ok(ProxyGeneration {
                id: Uuid::new_v4(),
                proxy_id: proxy.id,
                version: 1, // TODO: Get next version number from database
                channel_count: 0,
                m3u_content,
                created_at: Utc::now(),
            });
        }

        // Step 2: Get all channels from those sources (original, unmapped data)
        let mut all_channels = Vec::new();
        for source in &sources {
            let channels = database.get_source_channels(source.id).await?;
            info!(
                "Retrieved {} channels from source '{}'",
                channels.len(),
                source.name
            );
            all_channels.extend(channels);
        }

        info!("Total channels before processing: {}", all_channels.len());

        // Step 3: Apply data mapping to transform channels
        let mut mapped_channels = Vec::new();
        for source in &sources {
            let source_channels: Vec<Channel> = all_channels
                .iter()
                .filter(|ch| ch.source_id == source.id)
                .cloned()
                .collect();

            if source_channels.is_empty() {
                continue;
            }

            info!(
                "Applying data mapping to {} channels from source '{}'",
                source_channels.len(),
                source.name
            );

            let transformed_channels = data_mapping_service
                .apply_mapping_for_proxy(source_channels, source.id, logo_service, base_url)
                .await?;

            info!(
                "Data mapping completed for source '{}', {} channels transformed",
                source.name,
                transformed_channels.len()
            );
            mapped_channels.extend(transformed_channels);
        }

        info!(
            "Total channels after data mapping: {}",
            mapped_channels.len()
        );

        // Step 4: Get all active filters for this proxy (sorted by order)
        let proxy_filters = database.get_proxy_filters_with_details(proxy.id).await?;
        info!(
            "Found {} filters for proxy '{}'",
            proxy_filters.len(),
            proxy.name
        );

        // Step 5: Apply filters to mapped channels
        let filtered_channels = if proxy_filters.is_empty() {
            info!(
                "No filters found for proxy '{}', using all mapped channels",
                proxy.name
            );
            mapped_channels
        } else {
            info!(
                "Applying {} filters to mapped channels",
                proxy_filters.len()
            );
            let mut filter_engine = FilterEngine::new();

            // Convert proxy filters to the format expected by filter engine
            let mut filter_tuples = Vec::new();
            for proxy_filter in proxy_filters {
                let conditions = database
                    .get_filter_conditions(proxy_filter.filter.id)
                    .await?;
                filter_tuples.push((proxy_filter.filter, proxy_filter.proxy_filter, conditions));
            }

            let filtered = filter_engine
                .apply_filters(mapped_channels, filter_tuples)
                .await?;
            info!("Filtering completed, {} channels remain", filtered.len());
            filtered
        };

        // Step 6: Generate M3U content
        let m3u_content = self.generate_m3u_content(&filtered_channels).await?;

        // Step 7: Save to database and return generation record
        let generation = ProxyGeneration {
            id: Uuid::new_v4(),
            proxy_id: proxy.id,
            version: 1, // TODO: Get next version number from database
            channel_count: filtered_channels.len() as i32,
            m3u_content,
            created_at: Utc::now(),
        };

        info!(
            "Proxy generation completed for '{}': {} channels in final M3U",
            proxy.name,
            filtered_channels.len()
        );

        Ok(generation)
    }

    /// Generate M3U content from a list of channels
    async fn generate_m3u_content(&self, channels: &[Channel]) -> Result<String> {
        let mut m3u = String::from("#EXTM3U\n");

        for (index, channel) in channels.iter().enumerate() {
            let channel_number = index + 1;

            // Build EXTINF line
            let mut extinf = format!("#EXTINF:-1");

            if let Some(tvg_id) = &channel.tvg_id {
                if !tvg_id.is_empty() {
                    extinf.push_str(&format!(" tvg-id=\"{}\"", tvg_id));
                }
            }

            if let Some(tvg_name) = &channel.tvg_name {
                if !tvg_name.is_empty() {
                    extinf.push_str(&format!(" tvg-name=\"{}\"", tvg_name));
                }
            }

            if let Some(tvg_logo) = &channel.tvg_logo {
                if !tvg_logo.is_empty() {
                    extinf.push_str(&format!(" tvg-logo=\"{}\"", tvg_logo));
                }
            }

            if let Some(group_title) = &channel.group_title {
                if !group_title.is_empty() {
                    extinf.push_str(&format!(" group-title=\"{}\"", group_title));
                }
            }

            extinf.push_str(&format!(" tvg-chno=\"{}\"", channel_number));
            extinf.push_str(&format!(",{}\n", channel.channel_name));

            m3u.push_str(&extinf);
            m3u.push_str(&format!("{}\n", channel.stream_url));
        }

        Ok(m3u)
    }

    /// Save M3U content to the configured storage path
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
    pub fn get_m3u_storage_path(&self) -> &PathBuf {
        &self.storage_config.m3u_path
    }

    /// Get the storage path for logos
    pub fn get_logo_storage_path(&self) -> &PathBuf {
        &self.storage_config.cached_logo_path
    }

    /// Clean up old proxy versions (keep only the configured number)
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
                    warn!(
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
