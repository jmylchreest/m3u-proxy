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
    models::{Channel, StreamProxy, StreamProxyCreateRequest, StreamProxyUpdateRequest},
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
    /// This uses in-memory processing without touching the database
    pub async fn generate_preview(
        &self,
        request: PreviewProxyRequest,
    ) -> Result<PreviewProxyResponse, AppError> {
        tracing::info!("Starting preview generation for proxy: {}", request.name);
        use std::collections::HashMap;

        // Collect all source channels directly from database without temporary data
        let mut all_channels = Vec::new();
        let mut channels_by_source = HashMap::new();

        // Process each stream source
        for source_req in &request.stream_sources {
            tracing::debug!("Processing stream source: {}", source_req.source_id);
            // Get the source to validate it exists
            let source = self
                .stream_source_repo
                .find_by_id(source_req.source_id)
                .await
                .map_err(|e| {
                    tracing::error!(
                        "Failed to find stream source {}: {}",
                        source_req.source_id,
                        e
                    );
                    AppError::Repository(e)
                })?
                .ok_or_else(|| {
                    tracing::error!("Stream source not found: {}", source_req.source_id);
                    AppError::NotFound {
                        resource: "stream_source".to_string(),
                        id: source_req.source_id.to_string(),
                    }
                })?;

            // Get channels for this source
            let source_channels = self
                .database
                .get_source_channels(source_req.source_id)
                .await
                .map_err(|e| {
                    tracing::error!(
                        "Failed to get source channels for {}: {}",
                        source_req.source_id,
                        e
                    );
                    AppError::Internal {
                        message: format!("Failed to get source channels: {}", e),
                    }
                })?;

            tracing::debug!(
                "Found {} channels for source {}",
                source_channels.len(),
                source.name
            );

            let channel_count = source_channels.len();
            channels_by_source.insert(source.name.clone(), channel_count);

            // Apply data mapping to channels in memory
            let mapped_channels = self
                .data_mapping_service
                .apply_mapping_for_proxy(
                    source_channels,
                    source_req.source_id,
                    &self.logo_service,
                    "http://localhost:8080", // TODO: Get from config
                    None,                    // TODO: Get data mapping config from settings
                )
                .await
                .map_err(|e| {
                    tracing::error!(
                        "Data mapping failed for source {}: {}",
                        source_req.source_id,
                        e
                    );
                    AppError::Internal {
                        message: format!("Data mapping failed: {}", e),
                    }
                })?;

            tracing::debug!(
                "Mapped {} channels for source {}",
                mapped_channels.len(),
                source.name
            );

            all_channels.extend(mapped_channels);
        }

        // Apply filters in memory
        let mut filtered_channels = all_channels.clone();
        let mut applied_filters = Vec::new();

        tracing::debug!("Total channels before filtering: {}", all_channels.len());

        if !request.filters.is_empty() {
            tracing::debug!("Applying {} filters", request.filters.len());
            // Get filters and apply them
            let mut filter_tuples = Vec::new();
            for filter_req in &request.filters {
                if filter_req.is_active {
                    let filter = self
                        .filter_repo
                        .find_by_id(filter_req.filter_id)
                        .await
                        .map_err(|e| {
                            tracing::error!(
                                "Failed to find filter {}: {}",
                                filter_req.filter_id,
                                e
                            );
                            AppError::Repository(e)
                        })?
                        .ok_or_else(|| {
                            tracing::error!("Filter not found: {}", filter_req.filter_id);
                            AppError::NotFound {
                                resource: "filter".to_string(),
                                id: filter_req.filter_id.to_string(),
                            }
                        })?;

                    applied_filters.push(filter.name.clone());

                    // Create a fake ProxyFilter for the filter engine
                    let proxy_filter = crate::models::ProxyFilter {
                        proxy_id: Uuid::new_v4(), // Temporary ID
                        filter_id: filter.id,
                        priority_order: filter_req.priority_order,
                        is_active: filter_req.is_active,
                        created_at: chrono::Utc::now(),
                    };

                    filter_tuples.push((filter, proxy_filter));
                }
            }

            // Apply filters using the filter engine
            if !filter_tuples.is_empty() {
                tracing::debug!("Applying {} filter tuples", filter_tuples.len());
                let mut filter_engine = self.filter_engine.lock().await;
                filtered_channels = filter_engine
                    .apply_filters(all_channels.clone(), filter_tuples)
                    .await
                    .map_err(|e| {
                        tracing::error!("Filter application failed: {}", e);
                        AppError::Internal {
                            message: format!("Filter application failed: {}", e),
                        }
                    })?;
                tracing::debug!(
                    "Filters applied successfully, {} channels remain",
                    filtered_channels.len()
                );
            }
        }

        // Generate M3U content in memory
        tracing::debug!(
            "Generating M3U content for {} channels",
            filtered_channels.len()
        );
        let m3u_content = self
            .generate_preview_m3u(&filtered_channels, &request)
            .await
            .map_err(|e| {
                tracing::error!("Failed to generate M3U content: {}", e);
                e
            })?;

        // Store the M3U content in preview file manager
        let m3u_file_id = format!("preview-{}.m3u", uuid::Uuid::new_v4());
        tracing::debug!("Storing M3U content to file: {}", m3u_file_id);
        self.preview_file_manager
            .write(&m3u_file_id, &m3u_content)
            .await
            .map_err(|e| {
                tracing::error!("Failed to write M3U file {}: {}", m3u_file_id, e);
                AppError::Internal {
                    message: format!("Failed to write preview file: {}", e),
                }
            })?;

        // Generate preview channels and statistics
        let preview_channels = self
            .generate_preview_channels(&filtered_channels, &request)
            .await?;
        let mut channels_by_group = HashMap::new();

        // Calculate group statistics
        for channel in &preview_channels {
            let group = channel
                .group_title
                .clone()
                .unwrap_or_else(|| "Uncategorized".to_string());
            *channels_by_group.entry(group).or_insert(0) += 1;
        }

        let stats = crate::web::handlers::proxies::PreviewStats {
            total_sources: request.stream_sources.len(),
            total_channels_before_filters: all_channels.len(),
            total_channels_after_filters: filtered_channels.len(),
            channels_by_source,
            channels_by_group,
            applied_filters,
            excluded_channels: all_channels.len() - filtered_channels.len(),
            included_channels: filtered_channels.len(),
        };

        Ok(crate::web::handlers::proxies::PreviewProxyResponse {
            channels: preview_channels,
            stats,
            m3u_content: Some(m3u_content), // Return actual M3U content
            total_channels: all_channels.len(),
            filtered_channels: filtered_channels.len(),
        })
    }

    /// Generate M3U content for preview
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
