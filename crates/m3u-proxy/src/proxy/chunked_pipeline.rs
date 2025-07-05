//! Chunked processing pipeline for memory-constrained proxy generation
//!
//! This module provides an alternative pipeline that processes data in chunks
//! when memory limits are reached, trading some optimizations for memory efficiency.

use anyhow::Result;
use sandboxed_file_manager::SandboxedManager;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::data_mapping::service::DataMappingService;
use crate::database::Database;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;
use crate::proxy::filter_engine::FilterEngine;
use crate::proxy::pipeline::MappedChannel;
use crate::utils::{MemoryLimitStatus, MemoryStats, SimpleMemoryMonitor};

/// Chunked processing pipeline that handles memory pressure gracefully
pub struct ChunkedProxyPipeline {
    database: Database,
    data_mapping_service: DataMappingService,
    logo_service: LogoAssetService,
    filter_engine: FilterEngine,
    memory_monitor: Option<SimpleMemoryMonitor>,
    chunk_size: usize,
    temp_file_manager: SandboxedManager,
}

/// Temporary file for storing intermediate channel data using sandboxed file manager
struct TempChannelFile {
    file_id: Option<String>,
    channels: Vec<Channel>,
    channel_count: usize,
    file_manager: SandboxedManager,
}

impl TempChannelFile {
    async fn new(file_manager: SandboxedManager) -> Result<Self> {
        Ok(Self {
            file_id: None,
            channels: Vec::new(),
            channel_count: 0,
            file_manager,
        })
    }

    fn write_channel(&mut self, channel: &Channel) -> Result<()> {
        self.channels.push(channel.clone());
        self.channel_count += 1;
        Ok(())
    }

    async fn flush(&mut self) -> Result<()> {
        if !self.channels.is_empty() && self.file_id.is_none() {
            let json_lines: Vec<String> = self
                .channels
                .iter()
                .map(|ch| serde_json::to_string(ch))
                .collect::<Result<Vec<_>, _>>()?;
            let content = json_lines.join("\n");

            let file_id = format!("chunked-{}.json", uuid::Uuid::new_v4());
            self.file_manager
                .write(&file_id, &content)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to flush channels to file: {}", e))?;
            self.file_id = Some(file_id);

            // Clear channels from memory to save space
            self.channels.clear();
        }
        Ok(())
    }

    async fn read_channels(&mut self) -> Result<Vec<Channel>> {
        // First, return any channels still in memory
        if !self.channels.is_empty() {
            return Ok(self.channels.clone());
        }

        // Then try to read from file
        if let Some(file_id) = &self.file_id {
            match self.file_manager.read_to_string(file_id).await {
                Ok(content) => {
                    let mut channels = Vec::new();
                    for line in content.lines() {
                        if !line.is_empty() {
                            let mapped_channel: MappedChannel = serde_json::from_str(line)?;
                            channels.push(mapped_channel.channel);
                        }
                    }
                    Ok(channels)
                }
                Err(e) => Err(anyhow::anyhow!("Failed to read channels from file: {}", e)),
            }
        } else {
            Ok(Vec::new())
        }
    }
}

impl ChunkedProxyPipeline {
    pub fn new(
        database: Database,
        data_mapping_service: DataMappingService,
        logo_service: LogoAssetService,
        chunk_size: usize,
        memory_limit_mb: Option<usize>,
        temp_file_manager: SandboxedManager,
    ) -> Self {
        let memory_monitor = memory_limit_mb.map(|limit| SimpleMemoryMonitor::new(Some(limit)));

        Self {
            database,
            data_mapping_service,
            logo_service,
            filter_engine: FilterEngine::new(),
            memory_monitor,
            chunk_size,
            temp_file_manager,
        }
    }

    /// Generate proxy using chunked processing approach
    pub async fn generate_proxy_chunked(
        &mut self,
        proxy: &StreamProxy,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
    ) -> Result<(ProxyGeneration, Option<MemoryStats>)> {
        info!(
            "Starting chunked proxy generation for '{}' (chunk_size: {})",
            proxy.name, self.chunk_size
        );

        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.initialize()?;
            monitor.observe_stage("chunked_initialization")?;
        }

        // Get all sources for this proxy
        let sources = self.database.get_proxy_sources(proxy.id).await?;

        if sources.is_empty() {
            warn!("No sources found for proxy '{}'", proxy.name);
            return self.create_empty_generation(proxy).await;
        }

        // Process sources in chunks, writing intermediate results to temp files
        let mut temp_files = Vec::new();
        let mut total_channels = 0;

        for (source_index, source) in sources.iter().enumerate() {
            info!(
                "Processing source {}/{}: '{}'",
                source_index + 1,
                sources.len(),
                source.name
            );

            // Check memory before processing each source
            if let Some(ref mut monitor) = self.memory_monitor {
                match monitor.check_memory_limit()? {
                    MemoryLimitStatus::Exceeded => {
                        warn!(
                            "Memory limit exceeded, stopping at source {}/{}",
                            source_index + 1,
                            sources.len()
                        );
                        break;
                    }
                    MemoryLimitStatus::Warning => {
                        info!("Memory warning during source processing, using smaller chunks");
                    }
                    MemoryLimitStatus::Ok => {}
                }
                monitor.observe_stage(&format!("source_{}_start", source_index))?;
            }

            let source_temp_file = self
                .process_source_chunked(source, base_url, engine_config.clone())
                .await?;

            total_channels += source_temp_file.channel_count;
            temp_files.push(source_temp_file);

            if let Some(ref mut monitor) = self.memory_monitor {
                monitor.observe_stage(&format!("source_{}_complete", source_index))?;
            }
        }

        info!(
            "Processed {} sources, {} total channels",
            temp_files.len(),
            total_channels
        );

        // Apply filters using chunked approach
        let mut filtered_temp_file = self.apply_filters_chunked(proxy, temp_files).await?;

        if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("filtering_complete")?;
        }

        // Generate final M3U content from temp file
        let m3u_content = self
            .generate_m3u_from_temp_file(&mut filtered_temp_file, &proxy.ulid, base_url)
            .await?;

        // Create generation record
        let generation = ProxyGeneration {
            id: Uuid::new_v4(),
            proxy_id: proxy.id,
            version: 1,
            channel_count: filtered_temp_file.channel_count as i32,
            m3u_content,
            created_at: chrono::Utc::now(),
            // New fields for enhanced tracking
            total_channels,
            filtered_channels: filtered_temp_file.channel_count,
            applied_filters: Vec::new(), // TODO: Track applied filters in chunked pipeline
            stats: None, // Chunked pipeline doesn't collect comprehensive stats yet
        };

        let memory_stats = if let Some(ref mut monitor) = self.memory_monitor {
            monitor.observe_stage("generation_complete")?;
            Some(monitor.get_statistics())
        } else {
            None
        };

        info!(
            "Chunked generation completed: {} channels",
            generation.channel_count
        );

        Ok((generation, memory_stats))
    }

    /// Process a single source in chunks, writing results to temp file
    async fn process_source_chunked(
        &mut self,
        source: &StreamSource,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
    ) -> Result<TempChannelFile> {
        let mut temp_file = TempChannelFile::new(self.temp_file_manager.clone()).await?;

        // For now, we'll use the existing method and process in chunks
        // TODO: Implement proper pagination methods in database
        let all_channels = self.database.get_source_channels(source.id).await?;
        let total_channels = all_channels.len();
        let chunk_count = (total_channels + self.chunk_size - 1) / self.chunk_size;

        debug!(
            "Processing {} channels in {} chunks for source '{}'",
            total_channels, chunk_count, source.name
        );

        for chunk_index in 0..chunk_count {
            let offset = chunk_index * self.chunk_size;
            let limit = self.chunk_size.min(total_channels - offset);
            let end = (offset + limit).min(total_channels);

            debug!(
                "Processing chunk {}/{} (offset: {}, limit: {})",
                chunk_index + 1,
                chunk_count,
                offset,
                limit
            );

            // Take chunk from all channels
            let chunk_channels = &all_channels[offset..end];

            if chunk_channels.is_empty() {
                continue;
            }

            // Apply data mapping to this chunk
            let mapped_channels = self
                .data_mapping_service
                .apply_mapping_for_proxy(
                    chunk_channels.to_vec(),
                    source.id,
                    &self.logo_service,
                    base_url,
                    engine_config.clone(),
                )
                .await?;

            // Write mapped channels to temp file
            for channel in mapped_channels {
                temp_file.write_channel(&channel)?;
            }

            // Check memory after each chunk
            if let Some(ref monitor) = self.memory_monitor {
                match monitor.check_memory_limit()? {
                    MemoryLimitStatus::Exceeded => {
                        warn!("Memory limit exceeded during chunk processing, stopping early");
                        break;
                    }
                    _ => {}
                }
            }
        }

        temp_file.flush().await?;
        Ok(temp_file)
    }

    /// Apply filters using chunked approach - reads from multiple temp files, writes to one
    async fn apply_filters_chunked(
        &mut self,
        proxy: &StreamProxy,
        source_temp_files: Vec<TempChannelFile>,
    ) -> Result<TempChannelFile> {
        let mut filtered_temp_file = TempChannelFile::new(self.temp_file_manager.clone()).await?;

        // Get filters for this proxy
        let proxy_filters = self
            .database
            .get_proxy_filters_with_details(proxy.id)
            .await?;

        if proxy_filters.is_empty() {
            info!("No filters, copying all channels to filtered temp file");

            // No filters - just copy all channels
            for mut temp_file in source_temp_files {
                let channels = temp_file.read_channels().await?;
                for channel in channels {
                    filtered_temp_file.write_channel(&channel)?;
                }
            }
        } else {
            info!(
                "Applying {} filters using chunked approach",
                proxy_filters.len()
            );

            // Apply filters in chunks to manage memory
            let mut filter_tuples = Vec::new();
            for proxy_filter in proxy_filters {
                filter_tuples.push((proxy_filter.filter.clone(), proxy_filter.proxy_filter));
            }

            // Process each source temp file through filters
            for mut temp_file in source_temp_files {
                let channels = temp_file.read_channels().await?;

                // Process channels in chunks through filter engine
                for chunk in channels.chunks(self.chunk_size) {
                    let chunk_filtered = self
                        .filter_engine
                        .apply_filters(chunk.to_vec(), filter_tuples.clone())
                        .await?;

                    for channel in chunk_filtered {
                        filtered_temp_file.write_channel(&channel)?;
                    }

                    // Check memory after each chunk
                    if let Some(ref monitor) = self.memory_monitor {
                        match monitor.check_memory_limit()? {
                            MemoryLimitStatus::Exceeded => {
                                warn!("Memory limit exceeded during filter processing");
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        filtered_temp_file.flush().await?;
        Ok(filtered_temp_file)
    }

    /// Generate M3U content from temp file containing filtered channels
    async fn generate_m3u_from_temp_file(
        &self,
        temp_file: &mut TempChannelFile,
        proxy_ulid: &str,
        base_url: &str,
    ) -> Result<String> {
        if temp_file.channel_count == 0 {
            return Ok("#EXTM3U\n".to_string());
        }

        let mut m3u_content = String::from("#EXTM3U\n");

        // For chunked processing, we'll do a simple approach for channel numbering
        // More sophisticated numbering would require loading all channels into memory
        // which defeats the purpose of chunked processing

        let channels = temp_file.read_channels().await?;

        for (index, channel) in channels.iter().enumerate() {
            let channel_number = (index + 1) as i32;

            // Build EXTINF line (similar to existing pipeline)
            let mut extinf_parts = Vec::new();
            extinf_parts.push(format!("#EXTINF:-1"));

            if let Some(tvg_id) = &channel.tvg_id {
                if !tvg_id.is_empty() {
                    extinf_parts.push(format!("tvg-id=\"{}\"", tvg_id));
                }
            }

            if let Some(tvg_name) = &channel.tvg_name {
                if !tvg_name.is_empty() {
                    extinf_parts.push(format!("tvg-name=\"{}\"", tvg_name));
                }
            }

            if let Some(tvg_logo) = &channel.tvg_logo {
                if !tvg_logo.is_empty() {
                    extinf_parts.push(format!("tvg-logo=\"{}\"", tvg_logo));
                }
            }

            if let Some(group_title) = &channel.group_title {
                if !group_title.is_empty() {
                    extinf_parts.push(format!("group-title=\"{}\"", group_title));
                }
            }

            extinf_parts.push(format!("tvg-chno=\"{}\"", channel_number));
            extinf_parts.push(channel.channel_name.clone());

            let extinf_line = extinf_parts.join(" ");
            m3u_content.push_str(&extinf_line);
            m3u_content.push('\n');

            // Add stream URL
            let stream_url = if channel.stream_url.starts_with("http") {
                format!("{}/stream/{}/{}", base_url, proxy_ulid, channel.id)
            } else {
                channel.stream_url.clone()
            };

            m3u_content.push_str(&stream_url);
            m3u_content.push('\n');
        }

        Ok(m3u_content)
    }

    async fn create_empty_generation(
        &self,
        proxy: &StreamProxy,
    ) -> Result<(ProxyGeneration, Option<MemoryStats>)> {
        let generation = ProxyGeneration {
            id: Uuid::new_v4(),
            proxy_id: proxy.id,
            version: 1,
            channel_count: 0,
            m3u_content: "#EXTM3U\n".to_string(),
            created_at: chrono::Utc::now(),
            // New fields for enhanced tracking
            total_channels: 0,
            filtered_channels: 0,
            applied_filters: Vec::new(),
            stats: None, // Empty generation doesn't have stats
        };

        let memory_stats = self.memory_monitor.as_ref().map(|m| m.get_statistics());
        Ok((generation, memory_stats))
    }
}

// Note: This would require adding these methods to the Database trait:
// - count_source_channels(source_id: Uuid) -> Result<usize>
// - get_source_channels_paginated(source_id: Uuid, offset: usize, limit: usize) -> Result<Vec<Channel>>
