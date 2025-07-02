//! Stream Proxy Service
//!
//! This module contains business logic for stream proxy operations.

use uuid::Uuid;
use std::collections::HashMap;

use crate::{
    models::{
        StreamProxy, StreamProxyCreateRequest, StreamProxyUpdateRequest,
        Channel, Filter, StreamSource, ProxyFilter,
    },
    repositories::{
        StreamProxyRepository, ChannelRepository, FilterRepository, StreamSourceRepository,
        traits::Repository,
    },
    web::handlers::proxies::{
        StreamProxyResponse, ProxySourceResponse, ProxyEpgSourceResponse, ProxyFilterResponse,
        PreviewProxyRequest, PreviewProxyResponse, PreviewChannel, PreviewStats,
    },
    proxy::filter_engine::FilterEngine,
    errors::types::AppError,
};

pub struct StreamProxyService {
    proxy_repo: StreamProxyRepository,
    channel_repo: ChannelRepository,
    filter_repo: FilterRepository,
    stream_source_repo: StreamSourceRepository,
    filter_engine: std::sync::Mutex<FilterEngine>,
}

impl StreamProxyService {
    pub fn new(
        proxy_repo: StreamProxyRepository,
        channel_repo: ChannelRepository,
        filter_repo: FilterRepository,
        stream_source_repo: StreamSourceRepository,
        filter_engine: FilterEngine,
    ) -> Self {
        Self {
            proxy_repo,
            channel_repo,
            filter_repo,
            stream_source_repo,
            filter_engine: std::sync::Mutex::new(filter_engine),
        }
    }

    /// Create a new stream proxy
    pub async fn create(&self, request: StreamProxyCreateRequest) -> Result<StreamProxyResponse, AppError> {
        // Validate that all sources and filters exist
        self.validate_proxy_request(&request.stream_sources, &request.filters).await?;

        // Create the proxy with all relationships
        let proxy = self.proxy_repo.create_with_relationships(request).await
            .map_err(|e| AppError::Repository(e))?;

        // Build full response with relationships
        self.build_proxy_response(proxy).await
    }

    /// Update an existing stream proxy
    pub async fn update(&self, proxy_id: Uuid, request: StreamProxyUpdateRequest) -> Result<StreamProxyResponse, AppError> {
        // Validate that proxy exists
        let _existing = self.proxy_repo.find_by_id(proxy_id).await
            .map_err(|e| AppError::Repository(e))?
            .ok_or_else(|| AppError::NotFound { resource: "stream_proxy".to_string(), id: proxy_id.to_string() })?;

        // Validate that all sources and filters exist
        self.validate_proxy_request(&request.stream_sources, &request.filters).await?;

        // Update the proxy with all relationships
        let proxy = self.proxy_repo.update_with_relationships(proxy_id, request).await
            .map_err(|e| AppError::Repository(e))?;

        // Build full response with relationships
        self.build_proxy_response(proxy).await
    }

    /// Get a stream proxy by ID with all relationships
    pub async fn get_by_id(&self, proxy_id: Uuid) -> Result<Option<StreamProxyResponse>, AppError> {
        let proxy = self.proxy_repo.find_by_id(proxy_id).await
            .map_err(|e| AppError::Repository(e))?;

        match proxy {
            Some(proxy) => Ok(Some(self.build_proxy_response(proxy).await?)),
            None => Ok(None),
        }
    }

    /// Get a stream proxy by ULID with all relationships
    pub async fn get_by_ulid(&self, ulid: &str) -> Result<Option<StreamProxyResponse>, AppError> {
        let proxy = self.proxy_repo.get_by_ulid(ulid).await
            .map_err(|e| AppError::Repository(e))?;

        match proxy {
            Some(proxy) => Ok(Some(self.build_proxy_response(proxy).await?)),
            None => Ok(None),
        }
    }

    /// List all stream proxies with pagination
    pub async fn list(&self, limit: Option<usize>, offset: Option<usize>) -> Result<Vec<StreamProxyResponse>, AppError> {
        let proxies = self.proxy_repo.find_all(crate::repositories::traits::QueryParams::new()).await
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
        let _existing = self.proxy_repo.find_by_id(proxy_id).await
            .map_err(|e| AppError::Repository(e))?
            .ok_or_else(|| AppError::NotFound { resource: "stream_proxy".to_string(), id: proxy_id.to_string() })?;

        self.proxy_repo.delete(proxy_id).await
            .map_err(|e| AppError::Repository(e))?;

        Ok(())
    }

    /// Generate a preview of what a proxy configuration would produce
    pub async fn generate_preview(&self, _request: PreviewProxyRequest) -> Result<PreviewProxyResponse, AppError> {
        // TODO: Implement preview functionality when repository methods are ready
        // For now, return a placeholder response to enable compilation
        Ok(PreviewProxyResponse {
            channels: vec![],
            stats: PreviewStats {
                total_sources: 0,
                total_channels_before_filters: 0,
                total_channels_after_filters: 0,
                channels_by_source: HashMap::new(),
                channels_by_group: HashMap::new(),
                applied_filters: vec![],
                excluded_channels: 0,
                included_channels: 0,
            },
            m3u_content: None,
            total_channels: 0,
            filtered_channels: 0,
        })
    }

    /// Build a complete proxy response with all relationships
    async fn build_proxy_response(&self, proxy: StreamProxy) -> Result<StreamProxyResponse, AppError> {
        // TODO: Implement relationship loading when repository methods are ready
        // For now, return a basic response to enable compilation
        Ok(StreamProxyResponse::from(proxy))
    }

    /// Build a complete proxy response with all relationships (placeholder)
    async fn _build_proxy_response_full(&self, _proxy: StreamProxy) -> Result<StreamProxyResponse, AppError> {
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
            let _source = self.stream_source_repo.find_by_id(source_req.source_id).await
                .map_err(|e| AppError::Repository(e))?
                .ok_or_else(|| AppError::NotFound { resource: "stream_source".to_string(), id: source_req.source_id.to_string() })?;
        }

        // Validate filters exist
        for filter_req in filters {
            let _filter = self.filter_repo.find_by_id(filter_req.filter_id).await
                .map_err(|e| AppError::Repository(e))?
                .ok_or_else(|| AppError::NotFound { resource: "filter".to_string(), id: filter_req.filter_id.to_string() })?;
        }

        Ok(())
    }

    /// Generate a sample M3U playlist for preview
    async fn generate_m3u_sample(&self, channels: &[Channel], starting_number: i32) -> Result<String, AppError> {
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
            m3u.push_str(&format!("\n# ... and {} more channels\n", channels.len() - 10));
        }
        
        Ok(m3u)
    }
}