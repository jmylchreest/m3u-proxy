//! Stream Proxy Service
//!
//! This module contains business logic for stream proxy operations.

use regex::Regex;
use uuid::Uuid;

use crate::{
    config::StorageConfig,
    data_mapping::DataMappingService,
    database::Database,
    errors::types::AppError,
    logo_assets::service::LogoAssetService,
    models::{
        GenerationOutput, StreamProxy, StreamProxyCreateRequest, StreamProxyUpdateRequest,
    },
    // TODO: Remove - superseded by pipeline-based filtering
    database::repositories::{
        ChannelSeaOrmRepository, FilterSeaOrmRepository, StreamProxySeaOrmRepository, StreamSourceSeaOrmRepository,
    },
    web::handlers::proxies::{PreviewProxyRequest, PreviewProxyResponse, StreamProxyResponse},
};
use sandboxed_file_manager::SandboxedManager;

pub struct StreamProxyService {
    proxy_repo: StreamProxySeaOrmRepository,
    
    channel_repo: ChannelSeaOrmRepository,
    filter_repo: FilterSeaOrmRepository,
    stream_source_repo: StreamSourceSeaOrmRepository,
    
    // TODO: Remove - superseded by pipeline-based filtering
    database: Database,
    preview_file_manager: SandboxedManager,
    data_mapping_service: DataMappingService,
    logo_service: LogoAssetService,
    _storage_config: StorageConfig,
    app_config: crate::config::Config,
    temp_file_manager: SandboxedManager,
    proxy_output_file_manager: SandboxedManager,
    _system: std::sync::Arc<tokio::sync::RwLock<sysinfo::System>>,
}


/// Builder for StreamProxyService with many dependencies
pub struct StreamProxyServiceBuilder {
    pub proxy_repo: StreamProxySeaOrmRepository,
    pub channel_repo: ChannelSeaOrmRepository,
    pub filter_repo: FilterSeaOrmRepository,
    pub stream_source_repo: StreamSourceSeaOrmRepository,
    // TODO: Remove - superseded by pipeline-based filtering
    pub database: Database,
    pub preview_file_manager: SandboxedManager,
    pub data_mapping_service: DataMappingService,
    pub logo_service: LogoAssetService,
    pub storage_config: StorageConfig,
    pub app_config: crate::config::Config,
    pub temp_file_manager: SandboxedManager,
    pub proxy_output_file_manager: SandboxedManager,
    pub system: std::sync::Arc<tokio::sync::RwLock<sysinfo::System>>,
}

impl StreamProxyServiceBuilder {
    pub fn build(self) -> StreamProxyService {
        StreamProxyService::new_from_builder(self)
    }
}
impl StreamProxyService {
    pub fn new(builder: StreamProxyServiceBuilder) -> Self {
        Self::new_from_builder(builder)
    }
    
    fn new_from_builder(builder: StreamProxyServiceBuilder) -> Self {
        Self {
            proxy_repo: builder.proxy_repo,
            channel_repo: builder.channel_repo,
            filter_repo: builder.filter_repo,
            stream_source_repo: builder.stream_source_repo,
            // TODO: Remove - superseded by pipeline-based filtering
            database: builder.database,
            preview_file_manager: builder.preview_file_manager,
            data_mapping_service: builder.data_mapping_service,
            logo_service: builder.logo_service,
            _storage_config: builder.storage_config,
            app_config: builder.app_config,
            temp_file_manager: builder.temp_file_manager,
            proxy_output_file_manager: builder.proxy_output_file_manager,
            _system: builder.system,
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

        // Extract relationship IDs from request
        let source_ids: Vec<Uuid> = request.stream_sources.iter().map(|s| s.source_id).collect();
        let epg_source_ids: Vec<Uuid> = request.epg_sources.iter().map(|e| e.epg_source_id).collect();

        // Create the proxy with all relationships
        let proxy = self
            .proxy_repo
            .create_with_relationships(request, source_ids, epg_source_ids)
            .await
            .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?;

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
            .find_by_id(&proxy_id)
            .await
            .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?
            .ok_or_else(|| AppError::NotFound {
                resource: "stream_proxy".to_string(),
                id: proxy_id.to_string(),
            })?;

        // Validate that all sources and filters exist
        self.validate_proxy_request(&request.stream_sources, &request.filters)
            .await?;

        // Extract relationship IDs from request
        let source_ids: Vec<Uuid> = request.stream_sources.iter().map(|s| s.source_id).collect();
        let epg_source_ids: Vec<Uuid> = request.epg_sources.iter().map(|e| e.epg_source_id).collect();

        // Update the proxy with all relationships
        let proxy = self
            .proxy_repo
            .update_with_relationships(&proxy_id, request, source_ids, epg_source_ids)
            .await
            .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?;

        // Build full response with relationships
        self.build_proxy_response(proxy).await
    }

    /// Get a stream proxy by ID with all relationships
    pub async fn get_by_id(&self, proxy_id: Uuid) -> Result<Option<StreamProxyResponse>, AppError> {
        let proxy = self
            .proxy_repo
            .find_by_id(&proxy_id)
            .await
            .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?;

        match proxy {
            Some(proxy) => Ok(Some(self.build_proxy_response(proxy).await?)),
            None => Ok(None),
        }
    }

    /// Get a stream proxy by ID string with all relationships
    pub async fn get_by_id_string(
        &self,
        id: &str,
    ) -> Result<Option<StreamProxyResponse>, AppError> {
        let proxy_uuid = crate::utils::uuid_parser::parse_uuid_flexible(id)
            .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?;
        let proxy = self
            .proxy_repo
            .get_by_id(&proxy_uuid)
            .await
            .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?;

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
        tracing::debug!("StreamProxyService::list called");
        let proxies = self
            .proxy_repo
            .find_all()
            .await
            .map_err(|e| {
                tracing::error!("Failed to find all proxies: {}", e);
                AppError::Repository(crate::errors::RepositoryError::UuidParse(e))
            })?;

        tracing::debug!("Found {} proxies", proxies.len());
        let mut responses = Vec::new();
        for proxy in proxies {
            tracing::debug!("Building response for proxy: {}", proxy.id);
            responses.push(self.build_proxy_response(proxy).await?);
        }

        tracing::debug!("Successfully built {} proxy responses", responses.len());
        Ok(responses)
    }

    /// Delete a stream proxy
    pub async fn delete(&self, proxy_id: Uuid) -> Result<(), AppError> {
        // Validate that proxy exists
        let _existing = self
            .proxy_repo
            .find_by_id(&proxy_id)
            .await
            .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?
            .ok_or_else(|| AppError::NotFound {
                resource: "stream_proxy".to_string(),
                id: proxy_id.to_string(),
            })?;

        self.proxy_repo
            .delete(&proxy_id)
            .await
            .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?;

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

        // Use the native pipeline
        let proxy_service = crate::proxy::ProxyService::new(
            self.temp_file_manager.clone(),
            self.proxy_output_file_manager.clone(),
        );
        let params = crate::proxy::GenerateProxyParams {
            config: resolved_config.clone(),
            output,
            database: &self.database,
            data_mapping_service: &self.data_mapping_service,
            logo_service: &self.logo_service,
            base_url: &self.app_config.web.base_url,
            engine_config: None,
            app_config: &self.app_config,
        };
        let proxy_generation = proxy_service
            .generate_proxy_with_config(params)
            .await
            .map_err(|e| {
                tracing::error!("Generator failed for preview: {}", e);
                AppError::Internal {
                    message: format!("Generator failed: {e}"),
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
                .channel_repo
                .find_by_source_id(&source_config.source.id)
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
                    .map(|&t| format!("{t}ms"))
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
                .and_then(|gs| gs.memory_efficiency.map(|eff| format!("{eff:.1} ch/MB"))),
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
        // No limit - show all channels

        let lines: Vec<&str> = m3u_content.lines().collect();
        let mut i = 0;
        let mut channel_count = 0;

        while i < lines.len() {
            let line = lines[i].trim();
            if line.starts_with("#EXTINF:") {
                // Parse the EXTINF line
                let channel_name = line.split(',').next_back().unwrap_or("Unknown").to_string();
                let group_title = Self::extract_attribute(line, "group-title");
                let tvg_id = Self::extract_attribute(line, "tvg-id");
                let tvg_logo = Self::extract_attribute(line, "tvg-logo");
                let tvg_chno = Self::extract_attribute(line, "tvg-chno");
                let tvg_shift = Self::extract_attribute(line, "tvg-shift");
                let tvg_language = Self::extract_attribute(line, "tvg-language");
                let tvg_country = Self::extract_attribute(line, "tvg-country");
                let group_logo = Self::extract_attribute(line, "group-logo");
                let radio = Self::extract_attribute(line, "radio");

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
                    .as_ref()
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
                    tvg_chno,
                    tvg_shift,
                    tvg_language,
                    tvg_country,
                    group_logo,
                    radio,
                    extinf_line: line.to_string(),
                });

                channel_count += 1;
                i += 2; // Skip the URL line
            } else {
                i += 1;
            }
        }

        Ok(preview_channels)
    }

    // TODO: Move to API layer - preview functionality should be exposed via REST endpoints

    // TODO: Move to API layer - preview functionality should be exposed via REST endpoints

    /// Extract attribute value from EXTINF line
    fn extract_attribute(line: &str, attr: &str) -> Option<String> {
        let pattern = format!(r#"{attr}="([^"]*)""#);
        if let Ok(re) = Regex::new(&pattern)
            && let Some(captures) = re.captures(line) {
                return captures.get(1).map(|m| m.as_str().to_string());
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
        .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?;

        // Build stream sources responses
        let mut stream_sources = Vec::new();
        for proxy_source in proxy_sources {
            if let Some(source) = self
                .stream_source_repo
                .find_by_id(&proxy_source.source_id)
                .await
                .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?
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
                .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?
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
                .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?
            {
                filters.push(crate::web::handlers::proxies::ProxyFilterResponse {
                    filter_id: proxy_filter.filter_id,
                    filter_name: filter.name,
                    priority_order: proxy_filter.priority_order,
                    is_active: proxy_filter.is_active,
                    is_inverse: filter.is_inverse,
                    source_type: filter.source_type,
                });
            }
        }

        // Build the response with populated relationships and URLs
        let response = StreamProxyResponse {
            id: proxy.id,
            name: proxy.name,
            description: proxy.description,
            proxy_mode: match proxy.proxy_mode {
                crate::models::StreamProxyMode::Redirect => "redirect".to_string(),
                crate::models::StreamProxyMode::Proxy => "proxy".to_string(),
                crate::models::StreamProxyMode::Relay => "relay".to_string(),
            },
            upstream_timeout: proxy.upstream_timeout,
            buffer_size: proxy.buffer_size,
            max_concurrent_streams: proxy.max_concurrent_streams,
            starting_channel_number: proxy.starting_channel_number,
            created_at: proxy.created_at,
            updated_at: proxy.updated_at,
            is_active: proxy.is_active,
            auto_regenerate: proxy.auto_regenerate,
            cache_channel_logos: proxy.cache_channel_logos,
            cache_program_logos: proxy.cache_program_logos,
            relay_profile_id: proxy.relay_profile_id,
            stream_sources,
            epg_sources,
            filters,
            m3u8_url: format!("{}/proxy/{}/m3u8", self.app_config.web.base_url.trim_end_matches('/'), crate::utils::uuid_parser::uuid_to_base64(&proxy.id)),
            xmltv_url: format!("{}/proxy/{}/xmltv", self.app_config.web.base_url.trim_end_matches('/'), crate::utils::uuid_parser::uuid_to_base64(&proxy.id)),
        };

        Ok(response)
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
                .find_by_id(&source_req.source_id)
                .await
                .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?
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
                .map_err(|e| AppError::Repository(crate::errors::RepositoryError::UuidParse(e)))?
                .ok_or_else(|| AppError::NotFound {
                    resource: "filter".to_string(),
                    id: filter_req.filter_id.to_string(),
                })?;
        }

        Ok(())
    }
}
