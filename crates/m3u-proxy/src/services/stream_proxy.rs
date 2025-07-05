//! Stream Proxy Service
//!
//! This module contains business logic for stream proxy operations.

use regex::Regex;
use uuid::Uuid;

use crate::{
    config::StorageConfig,
    data_mapping::service::DataMappingService,
    database::Database,
    errors::types::AppError,
    logo_assets::service::LogoAssetService,
    models::{
        Channel, GenerationOutput, StreamProxy, StreamProxyCreateRequest, StreamProxyUpdateRequest,
    },
    proxy::filter_engine::FilterEngine,
    repositories::{
        ChannelRepository, FilterRepository, StreamProxyRepository, StreamSourceRepository,
        traits::Repository,
    },
    web::handlers::proxies::{PreviewProxyRequest, PreviewProxyResponse, StreamProxyResponse},
};
use sandboxed_file_manager::SandboxedManager;

pub struct StreamProxyService {
    proxy_repo: StreamProxyRepository,
    #[allow(dead_code)]
    channel_repo: ChannelRepository,
    filter_repo: FilterRepository,
    stream_source_repo: StreamSourceRepository,
    #[allow(dead_code)]
    filter_engine: tokio::sync::Mutex<FilterEngine>,
    database: Database,
    preview_file_manager: SandboxedManager,
    data_mapping_service: DataMappingService,
    logo_service: LogoAssetService,
    storage_config: StorageConfig,
}

impl StreamProxyService {
    pub fn new(
        proxy_repo: StreamProxyRepository,
        channel_repo: ChannelRepository,
        filter_repo: FilterRepository,
        stream_source_repo: StreamSourceRepository,
        filter_engine: FilterEngine,
        database: Database,
        preview_file_manager: SandboxedManager,
        data_mapping_service: DataMappingService,
        logo_service: LogoAssetService,
        storage_config: StorageConfig,
    ) -> Self {
        Self {
            proxy_repo,
            channel_repo,
            filter_repo,
            stream_source_repo,
            filter_engine: tokio::sync::Mutex::new(filter_engine),
            database,
            preview_file_manager,
            data_mapping_service,
            logo_service,
            storage_config,
        }
    }

    /// Create a new stream proxy
    pub async fn create(
        &self,
        request: StreamProxyCreateRequest,
    ) -> Result<StreamProxyResponse, AppError> {
        // Validate that all sources and filters exist
        self.validate_proxy_request(&request.stream_sources, &request.filters)
            .await?;

        // Create the proxy with all relationships
        let proxy = self
            .proxy_repo
            .create_with_relationships(request)
            .await
            .map_err(|e| AppError::Repository(e))?;

        // Build full response with relationships
        self.build_proxy_response(proxy).await
    }

    /// Update an existing stream proxy
    pub async fn update(
        &self,
        proxy_id: Uuid,
        request: StreamProxyUpdateRequest,
    ) -> Result<StreamProxyResponse, AppError> {
        // Validate that proxy exists
        let _existing = self
            .proxy_repo
            .find_by_id(proxy_id)
            .await
            .map_err(|e| AppError::Repository(e))?
            .ok_or_else(|| AppError::NotFound {
                resource: "stream_proxy".to_string(),
                id: proxy_id.to_string(),
            })?;

        // Validate that all sources and filters exist
        self.validate_proxy_request(&request.stream_sources, &request.filters)
            .await?;

        // Update the proxy with all relationships
        let proxy = self
            .proxy_repo
            .update_with_relationships(proxy_id, request)
            .await
            .map_err(|e| AppError::Repository(e))?;

        // Build full response with relationships
        self.build_proxy_response(proxy).await
    }

    /// Get a stream proxy by ID with all relationships
    pub async fn get_by_id(&self, proxy_id: Uuid) -> Result<Option<StreamProxyResponse>, AppError> {
        let proxy = self
            .proxy_repo
            .find_by_id(proxy_id)
            .await
            .map_err(|e| AppError::Repository(e))?;

        match proxy {
            Some(proxy) => Ok(Some(self.build_proxy_response(proxy).await?)),
            None => Ok(None),
        }
    }

    /// Get a stream proxy by ULID with all relationships
    pub async fn get_by_ulid(&self, ulid: &str) -> Result<Option<StreamProxyResponse>, AppError> {
        let proxy = self
            .proxy_repo
            .get_by_ulid(ulid)
            .await
            .map_err(|e| AppError::Repository(e))?;

        match proxy {
            Some(proxy) => Ok(Some(self.build_proxy_response(proxy).await?)),
            None => Ok(None),
        }
    }

    /// List all stream proxies with pagination
    pub async fn list(
        &self,
        _limit: Option<usize>,
        _offset: Option<usize>,
    ) -> Result<Vec<StreamProxyResponse>, AppError> {
        let proxies = self
            .proxy_repo
            .find_all(crate::repositories::traits::QueryParams::new())
            .await
            .map_err(|e| AppError::Repository(e))?;

        let mut responses = Vec::new();
        for proxy in proxies {
            responses.push(self.build_proxy_response(proxy).await?);
        }

        Ok(responses)
    }

    /// Delete a stream proxy
    pub async fn delete(&self, proxy_id: Uuid) -> Result<(), AppError> {
        // Validate that proxy exists
        let _existing = self
            .proxy_repo
            .find_by_id(proxy_id)
            .await
            .map_err(|e| AppError::Repository(e))?
            .ok_or_else(|| AppError::NotFound {
                resource: "stream_proxy".to_string(),
                id: proxy_id.to_string(),
            })?;

        self.proxy_repo
            .delete(proxy_id)
            .await
            .map_err(|e| AppError::Repository(e))?;

        Ok(())
    }

    /// Generate a preview of what a proxy configuration would produce
    /// This uses the new dependency injection architecture - no temporary database entries!
    pub async fn generate_preview(
        &self,
        request: PreviewProxyRequest,
    ) -> Result<PreviewProxyResponse, AppError> {
        tracing::info!("Starting preview generation for proxy: {}", request.name);
        use std::collections::HashMap;

        // Create config resolver
        let config_resolver = crate::proxy::config_resolver::ProxyConfigResolver::new(
            self.proxy_repo.clone(),
            self.stream_source_repo.clone(),
            self.filter_repo.clone(),
            self.database.clone(),
        );

        // Resolve preview configuration (no database writes!)
        let resolved_config = config_resolver
            .resolve_preview_config(request.clone())
            .await?;

        // Validate configuration
        config_resolver.validate_config(&resolved_config)?;

        // Create preview output destination
        let output = GenerationOutput::Preview {
            file_manager: self.preview_file_manager.clone(),
            proxy_name: request.name.clone(),
        };

        // Use the new generator architecture
        let proxy_service = crate::proxy::ProxyService::new(self.storage_config.clone());
        let proxy_generation = proxy_service
            .generate_proxy_with_config(
                resolved_config.clone(),
                output,
                &self.database,
                &self.data_mapping_service,
                &self.logo_service,
                "http://localhost:8080", // TODO: Get from config
                None,                    // engine_config
            )
            .await
            .map_err(|e| {
                tracing::error!("Generator failed for preview: {}", e);
                AppError::Internal {
                    message: format!("Generator failed: {}", e),
                }
            })?;

        // Parse the generated M3U content to extract channels for preview
        let preview_channels = self
            .parse_m3u_for_preview(&proxy_generation.m3u_content, &request)
            .await?;

        // Build statistics from the generated content
        let mut channels_by_group = HashMap::new();
        let mut channels_by_source = HashMap::new();

        // Calculate group statistics
        for channel in &preview_channels {
            let group = channel
                .group_title
                .clone()
                .unwrap_or_else(|| "Uncategorized".to_string());
            *channels_by_group.entry(group).or_insert(0) += 1;
        }

        // Calculate source statistics from resolved config
        for source_config in &resolved_config.sources {
            let source_channels = self
                .database
                .get_source_channels(source_config.source.id)
                .await
                .unwrap_or_default();
            channels_by_source.insert(source_config.source.name.clone(), source_channels.len());
        }

        // Extract enhanced stats from GenerationStats if available
        let generation_stats = proxy_generation.stats.as_ref();

        let stats = crate::web::handlers::proxies::PreviewStats {
            total_sources: resolved_config.sources.len(),
            total_channels_before_filters: proxy_generation.total_channels,
            total_channels_after_filters: proxy_generation.filtered_channels,
            channels_by_source,
            channels_by_group,
            applied_filters: proxy_generation.applied_filters,
            excluded_channels: proxy_generation.total_channels - proxy_generation.filtered_channels,
            included_channels: proxy_generation.filtered_channels,
            // Enhanced pipeline metrics from GenerationStats (fix types)
            pipeline_stages: generation_stats.map(|gs| gs.stage_timings.len()),
            filter_execution_time: generation_stats.and_then(|gs| {
                gs.stage_timings
                    .get("filtering")
                    .map(|&t| format!("{}ms", t))
            }),
            processing_rate: generation_stats
                .map(|gs| format!("{:.1} ch/s", gs.channels_per_second)),
            pipeline_stages_detail: generation_stats.map(|gs| {
                gs.stage_timings
                    .iter()
                    .map(
                        |(stage, &duration)| crate::web::handlers::proxies::PipelineStageDetail {
                            name: stage.clone(),
                            duration,
                            channels_processed: gs.total_channels_processed,
                            memory_used: gs.stage_memory_usage.get(stage).cloned(),
                        },
                    )
                    .collect()
            }),
            // Memory metrics from GenerationStats (fix types)
            current_memory: None, // Not tracked in current implementation
            peak_memory: generation_stats.and_then(|gs| {
                gs.peak_memory_usage_mb
                    .map(|mb| (mb * 1024.0 * 1024.0) as u64)
            }),
            memory_efficiency: generation_stats
                .and_then(|gs| gs.memory_efficiency.map(|eff| format!("{:.1} ch/MB", eff))),
            gc_collections: generation_stats.and_then(|gs| gs.gc_collections),
            memory_by_stage: generation_stats.map(|gs| gs.stage_memory_usage.clone()),
            // Processing metrics from GenerationStats (fix types)
            total_processing_time: generation_stats.map(|gs| format!("{}ms", gs.total_duration_ms)),
            avg_channel_time: generation_stats
                .map(|gs| format!("{:.2}ms", gs.average_channel_processing_ms)),
            throughput: generation_stats.map(|gs| format!("{:.1} ch/s", gs.channels_per_second)),
            errors: generation_stats.map(|gs| gs.errors.len()),
            processing_timeline: generation_stats.map(|gs| {
                let now = chrono::Utc::now();
                vec![
                    crate::web::handlers::proxies::ProcessingEvent {
                        timestamp: now,
                        description: format!(
                            "Source loading: {}ms",
                            gs.stage_timings.get("source_loading").unwrap_or(&0)
                        ),
                        stage: Some("source_loading".to_string()),
                        channels_count: Some(gs.total_channels_processed),
                    },
                    crate::web::handlers::proxies::ProcessingEvent {
                        timestamp: now,
                        description: format!("Data mapping: {}ms", gs.data_mapping_duration_ms),
                        stage: Some("data_mapping".to_string()),
                        channels_count: Some(gs.channels_mapped),
                    },
                    crate::web::handlers::proxies::ProcessingEvent {
                        timestamp: now,
                        description: format!(
                            "Filtering: {}ms",
                            gs.stage_timings.get("filtering").unwrap_or(&0)
                        ),
                        stage: Some("filtering".to_string()),
                        channels_count: Some(gs.channels_after_filtering),
                    },
                    crate::web::handlers::proxies::ProcessingEvent {
                        timestamp: now,
                        description: format!(
                            "Channel numbering: {}ms",
                            gs.channel_numbering_duration_ms
                        ),
                        stage: Some("channel_numbering".to_string()),
                        channels_count: Some(gs.channels_after_filtering),
                    },
                    crate::web::handlers::proxies::ProcessingEvent {
                        timestamp: now,
                        description: format!("M3U generation: {}ms", gs.m3u_generation_duration_ms),
                        stage: Some("m3u_generation".to_string()),
                        channels_count: Some(gs.channels_after_filtering),
                    },
                ]
            }),
        };

        Ok(crate::web::handlers::proxies::PreviewProxyResponse {
            channels: preview_channels,
            stats,
            m3u_content: Some(proxy_generation.m3u_content),
            total_channels: proxy_generation.total_channels,
            filtered_channels: proxy_generation.filtered_channels,
        })
    }

    /// Parse M3U content to extract channels for preview display
    async fn parse_m3u_for_preview(
        &self,
        m3u_content: &str,
        request: &PreviewProxyRequest,
    ) -> Result<Vec<crate::web::handlers::proxies::PreviewChannel>, AppError> {
        let mut preview_channels = Vec::new();
        let limit = 20; // Limit for performance

        let lines: Vec<&str> = m3u_content.lines().collect();
        let mut i = 0;
        let mut channel_count = 0;

        while i < lines.len() && channel_count < limit {
            let line = lines[i].trim();
            if line.starts_with("#EXTINF:") {
                // Parse the EXTINF line
                let channel_name = line.split(',').last().unwrap_or("Unknown").to_string();
                let group_title = Self::extract_attribute(line, "group-title");
                let tvg_id = Self::extract_attribute(line, "tvg-id");
                let tvg_logo = Self::extract_attribute(line, "tvg-logo");
                let tvg_chno = Self::extract_attribute(line, "tvg-chno");

                // Get the stream URL from the next line
                let stream_url = if i + 1 < lines.len() {
                    // Fix the hardcoded preview URL to use the actual proxy name
                    let original_url = lines[i + 1].trim();
                    if original_url.contains("/stream/preview/") {
                        original_url
                            .replace("/stream/preview/", &format!("/stream/{}/", request.name))
                    } else {
                        original_url.to_string()
                    }
                } else {
                    "".to_string()
                };

                let channel_number = tvg_chno
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(channel_count + 1);

                preview_channels.push(crate::web::handlers::proxies::PreviewChannel {
                    channel_name,
                    group_title,
                    tvg_id,
                    tvg_logo,
                    stream_url,
                    source_name: "Generated".to_string(),
                    channel_number,
                });

                channel_count += 1;
                i += 2; // Skip the URL line
            } else {
                i += 1;
            }
        }

        Ok(preview_channels)
    }

    /// Generate M3U content for preview
    #[allow(dead_code)]
    async fn generate_preview_m3u(
        &self,
        channels: &[Channel],
        request: &PreviewProxyRequest,
    ) -> Result<String, AppError> {
        let mut m3u_content = String::from("#EXTM3U\n");

        for (index, channel) in channels.iter().enumerate() {
            // Generate channel number (1-based)
            let channel_number = (index + 1) as i32 + request.starting_channel_number - 1;

            // Build EXTINF line
            let mut extinf_parts = Vec::new();
            extinf_parts.push(format!("#EXTINF:-1"));

            // Add tvg-id if available
            if let Some(tvg_id) = &channel.tvg_id {
                if !tvg_id.is_empty() {
                    extinf_parts.push(format!("tvg-id=\"{}\"", tvg_id));
                }
            }

            // Add tvg-name if available
            if let Some(tvg_name) = &channel.tvg_name {
                if !tvg_name.is_empty() {
                    extinf_parts.push(format!("tvg-name=\"{}\"", tvg_name));
                }
            }

            // Add tvg-logo if available
            if let Some(tvg_logo) = &channel.tvg_logo {
                if !tvg_logo.is_empty() {
                    extinf_parts.push(format!("tvg-logo=\"{}\"", tvg_logo));
                }
            }

            // Add group-title if available
            if let Some(group_title) = &channel.group_title {
                if !group_title.is_empty() {
                    extinf_parts.push(format!("group-title=\"{}\"", group_title));
                }
            }

            // Add channel number
            extinf_parts.push(format!("tvg-chno=\"{}\"", channel_number));

            // Add channel name at the end
            extinf_parts.push(channel.channel_name.clone());

            let extinf_line = extinf_parts.join(" ");
            m3u_content.push_str(&extinf_line);
            m3u_content.push('\n');

            // Add stream URL (would be proxied in real scenario)
            let stream_url = format!("http://localhost:8080/stream/preview/{}", channel.id);
            m3u_content.push_str(&stream_url);
            m3u_content.push('\n');
        }

        Ok(m3u_content)
    }

    /// Generate preview channel list (limited to first 20 for performance)
    #[allow(dead_code)]
    async fn generate_preview_channels(
        &self,
        channels: &[Channel],
        request: &PreviewProxyRequest,
    ) -> Result<Vec<crate::web::handlers::proxies::PreviewChannel>, AppError> {
        let mut preview_channels = Vec::new();
        let limit = 20; // Limit for performance

        for (index, channel) in channels.iter().take(limit).enumerate() {
            let channel_number = (index + 1) as i32 + request.starting_channel_number - 1;

            preview_channels.push(crate::web::handlers::proxies::PreviewChannel {
                channel_name: channel.channel_name.clone(),
                group_title: channel.group_title.clone(),
                tvg_id: channel.tvg_id.clone(),
                tvg_logo: channel.tvg_logo.clone(),
                stream_url: format!("http://localhost:8080/stream/preview/{}", channel.id),
                source_name: "Preview".to_string(), // TODO: Get actual source name
                channel_number,
            });
        }

        Ok(preview_channels)
    }

    /// Extract attribute value from EXTINF line
    fn extract_attribute(line: &str, attr: &str) -> Option<String> {
        let pattern = format!(r#"{}="([^"]*)""#, attr);
        if let Ok(re) = Regex::new(&pattern) {
            if let Some(captures) = re.captures(line) {
                return captures.get(1).map(|m| m.as_str().to_string());
            }
        }
        None
    }

    /// Build a complete proxy response with all relationships
    async fn build_proxy_response(
        &self,
        proxy: StreamProxy,
    ) -> Result<StreamProxyResponse, AppError> {
        // Load all relationships in parallel
        let (proxy_sources, proxy_epg_sources, proxy_filters) = tokio::try_join!(
            self.proxy_repo.get_proxy_sources(proxy.id),
            self.proxy_repo.get_proxy_epg_sources(proxy.id),
            self.proxy_repo.get_proxy_filters(proxy.id)
        )
        .map_err(|e| AppError::Repository(e))?;

        // Build stream sources responses
        let mut stream_sources = Vec::new();
        for proxy_source in proxy_sources {
            if let Some(source) = self
                .stream_source_repo
                .find_by_id(proxy_source.source_id)
                .await
                .map_err(|e| AppError::Repository(e))?
            {
                stream_sources.push(crate::web::handlers::proxies::ProxySourceResponse {
                    source_id: proxy_source.source_id,
                    source_name: source.name,
                    priority_order: proxy_source.priority_order,
                });
            }
        }

        // Build EPG sources responses
        let mut epg_sources = Vec::new();
        for proxy_epg_source in proxy_epg_sources {
            // Query EPG source directly for now (should be moved to proper repository later)
            if let Some(epg_source) = self
                .proxy_repo
                .find_epg_source_by_id(proxy_epg_source.epg_source_id)
                .await
                .map_err(|e| AppError::Repository(e))?
            {
                epg_sources.push(crate::web::handlers::proxies::ProxyEpgSourceResponse {
                    epg_source_id: proxy_epg_source.epg_source_id,
                    epg_source_name: epg_source.name,
                    priority_order: proxy_epg_source.priority_order,
                });
            }
        }

        // Build filters responses
        let mut filters = Vec::new();
        for proxy_filter in proxy_filters {
            if let Some(filter) = self
                .filter_repo
                .find_by_id(proxy_filter.filter_id)
                .await
                .map_err(|e| AppError::Repository(e))?
            {
                filters.push(crate::web::handlers::proxies::ProxyFilterResponse {
                    filter_id: proxy_filter.filter_id,
                    filter_name: filter.name,
                    priority_order: proxy_filter.priority_order,
                    is_active: proxy_filter.is_active,
                });
            }
        }

        // Build the response with populated relationships
        let mut response = StreamProxyResponse::from(proxy);
        response.stream_sources = stream_sources;
        response.epg_sources = epg_sources;
        response.filters = filters;

        Ok(response)
    }

    /// Build a complete proxy response with all relationships (placeholder)
    async fn _build_proxy_response_full(
        &self,
        _proxy: StreamProxy,
    ) -> Result<StreamProxyResponse, AppError> {
        // TODO: Implement when repository methods are ready
        unimplemented!("Placeholder method")
    }

    /// Validate that all referenced sources and filters exist
    async fn validate_proxy_request(
        &self,
        stream_sources: &[crate::models::ProxySourceCreateRequest],
        filters: &[crate::models::ProxyFilterCreateRequest],
    ) -> Result<(), AppError> {
        // Validate stream sources exist
        for source_req in stream_sources {
            let _source = self
                .stream_source_repo
                .find_by_id(source_req.source_id)
                .await
                .map_err(|e| AppError::Repository(e))?
                .ok_or_else(|| AppError::NotFound {
                    resource: "stream_source".to_string(),
                    id: source_req.source_id.to_string(),
                })?;
        }

        // Validate filters exist
        for filter_req in filters {
            let _filter = self
                .filter_repo
                .find_by_id(filter_req.filter_id)
                .await
                .map_err(|e| AppError::Repository(e))?
                .ok_or_else(|| AppError::NotFound {
                    resource: "filter".to_string(),
                    id: filter_req.filter_id.to_string(),
                })?;
        }

        Ok(())
    }

    /// Generate a sample M3U playlist for preview
    #[allow(dead_code)]
    async fn generate_m3u_sample(
        &self,
        channels: &[Channel],
        starting_number: i32,
    ) -> Result<String, AppError> {
        let mut m3u = String::from("#EXTM3U\n");

        for (index, channel) in channels.iter().take(10).enumerate() {
            let channel_number = starting_number + index as i32;

            // Build EXTINF line
            let mut extinf = format!("#EXTINF:-1");

            if let Some(tvg_id) = &channel.tvg_id {
                extinf.push_str(&format!(" tvg-id=\"{}\"", tvg_id));
            }

            if let Some(tvg_logo) = &channel.tvg_logo {
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

        if channels.len() > 10 {
            m3u.push_str(&format!(
                "\n# ... and {} more channels\n",
                channels.len() - 10
            ));
        }

        Ok(m3u)
    }
}
