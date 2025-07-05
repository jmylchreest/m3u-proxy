//! Proxy Configuration Resolver
//!
//! This module handles resolving complete proxy configurations from the database
//! upfront, eliminating the need for database queries during generation.

use anyhow::Result;
use tracing::{debug, info};
use uuid::Uuid;

use crate::{
    database::Database,
    errors::types::AppError,
    models::*,
    repositories::{
        FilterRepository, StreamProxyRepository, StreamSourceRepository, traits::Repository,
    },
    web::handlers::proxies::PreviewProxyRequest,
};

/// Service for resolving proxy configurations from database
pub struct ProxyConfigResolver {
    proxy_repo: StreamProxyRepository,
    stream_source_repo: StreamSourceRepository,
    filter_repo: FilterRepository,
    #[allow(dead_code)]
    database: Database,
}

impl ProxyConfigResolver {
    pub fn new(
        proxy_repo: StreamProxyRepository,
        stream_source_repo: StreamSourceRepository,
        filter_repo: FilterRepository,
        database: Database,
    ) -> Self {
        Self {
            proxy_repo,
            stream_source_repo,
            filter_repo,
            database,
        }
    }

    /// Resolve complete configuration for an existing proxy
    pub async fn resolve_config(&self, proxy_id: Uuid) -> Result<ResolvedProxyConfig, AppError> {
        debug!("Resolving configuration for proxy {}", proxy_id);

        // Get the proxy
        let proxy = self
            .proxy_repo
            .find_by_id(proxy_id)
            .await
            .map_err(|e| AppError::Repository(e))?
            .ok_or_else(|| AppError::NotFound {
                resource: "stream_proxy".to_string(),
                id: proxy_id.to_string(),
            })?;

        // Load all relationships in parallel
        let (proxy_sources, proxy_epg_sources, proxy_filters) = tokio::try_join!(
            self.proxy_repo.get_proxy_sources(proxy.id),
            self.proxy_repo.get_proxy_epg_sources(proxy.id),
            self.proxy_repo.get_proxy_filters(proxy.id)
        )
        .map_err(|e| AppError::Repository(e))?;

        // Resolve source configurations
        let mut sources = Vec::new();
        for proxy_source in proxy_sources {
            if let Some(source) = self
                .stream_source_repo
                .find_by_id(proxy_source.source_id)
                .await
                .map_err(|e| AppError::Repository(e))?
            {
                sources.push(ProxySourceConfig {
                    source,
                    priority_order: proxy_source.priority_order,
                });
            } else {
                debug!("Skipping missing source {}", proxy_source.source_id);
            }
        }

        // Resolve filter configurations
        let mut filters = Vec::new();
        for proxy_filter in proxy_filters {
            if let Some(filter) = self
                .filter_repo
                .find_by_id(proxy_filter.filter_id)
                .await
                .map_err(|e| AppError::Repository(e))?
            {
                filters.push(ProxyFilterConfig {
                    filter,
                    priority_order: proxy_filter.priority_order,
                    is_active: proxy_filter.is_active,
                });
            } else {
                debug!("Skipping missing filter {}", proxy_filter.filter_id);
            }
        }

        // Resolve EPG source configurations
        let mut epg_sources = Vec::new();
        for proxy_epg_source in proxy_epg_sources {
            if let Some(epg_source) = self
                .proxy_repo
                .find_epg_source_by_id(proxy_epg_source.epg_source_id)
                .await
                .map_err(|e| AppError::Repository(e))?
            {
                epg_sources.push(ProxyEpgSourceConfig {
                    epg_source,
                    priority_order: proxy_epg_source.priority_order,
                });
            } else {
                debug!(
                    "Skipping missing EPG source {}",
                    proxy_epg_source.epg_source_id
                );
            }
        }

        // Sort by priority
        sources.sort_by_key(|s| s.priority_order);
        filters.sort_by_key(|f| f.priority_order);
        epg_sources.sort_by_key(|e| e.priority_order);

        let config = ResolvedProxyConfig {
            proxy,
            sources,
            filters,
            epg_sources,
        };

        info!(
            "Resolved configuration: {} sources, {} filters, {} EPG sources",
            config.sources.len(),
            config.filters.len(),
            config.epg_sources.len()
        );

        Ok(config)
    }

    /// Resolve configuration for a preview request (no database proxy)
    pub async fn resolve_preview_config(
        &self,
        request: PreviewProxyRequest,
    ) -> Result<ResolvedProxyConfig, AppError> {
        debug!("Resolving preview configuration for '{}'", request.name);

        // Create temporary proxy from request
        let temp_proxy = StreamProxy {
            id: Uuid::new_v4(),
            ulid: format!("preview-{}", Uuid::new_v4()),
            name: request.name.clone(),
            description: Some(format!("Preview proxy for {}", request.name)),
            starting_channel_number: request.starting_channel_number,
            is_active: true,
            auto_regenerate: false,
            proxy_mode: crate::models::StreamProxyMode::Proxy, // Default to Proxy mode
            upstream_timeout: None,
            buffer_size: None,
            max_concurrent_streams: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_generated_at: None,
        };

        // Resolve source configurations
        let mut sources = Vec::new();
        for source_req in &request.stream_sources {
            if let Some(source) = self
                .stream_source_repo
                .find_by_id(source_req.source_id)
                .await
                .map_err(|e| AppError::Repository(e))?
            {
                sources.push(ProxySourceConfig {
                    source,
                    priority_order: source_req.priority_order,
                });
            } else {
                return Err(AppError::NotFound {
                    resource: "stream_source".to_string(),
                    id: source_req.source_id.to_string(),
                });
            }
        }

        // Resolve filter configurations
        let mut filters = Vec::new();
        for filter_req in &request.filters {
            if let Some(filter) = self
                .filter_repo
                .find_by_id(filter_req.filter_id)
                .await
                .map_err(|e| AppError::Repository(e))?
            {
                filters.push(ProxyFilterConfig {
                    filter,
                    priority_order: filter_req.priority_order,
                    is_active: filter_req.is_active,
                });
            } else {
                return Err(AppError::NotFound {
                    resource: "filter".to_string(),
                    id: filter_req.filter_id.to_string(),
                });
            }
        }

        // Sort by priority
        sources.sort_by_key(|s| s.priority_order);
        filters.sort_by_key(|f| f.priority_order);

        let config = ResolvedProxyConfig {
            proxy: temp_proxy,
            sources,
            filters,
            epg_sources: Vec::new(), // Preview doesn't need EPG sources for now
        };

        debug!(
            "Resolved preview configuration: {} sources, {} filters",
            config.sources.len(),
            config.filters.len()
        );

        Ok(config)
    }

    /// Validate that a resolved configuration is complete and valid
    pub fn validate_config(&self, config: &ResolvedProxyConfig) -> Result<(), AppError> {
        if config.sources.is_empty() {
            return Err(AppError::Internal {
                message: "Proxy must have at least one source".to_string(),
            });
        }

        // Validate all sources are active
        for source_config in &config.sources {
            if !source_config.source.is_active {
                debug!(
                    "Warning: Source '{}' is not active",
                    source_config.source.name
                );
            }
        }

        Ok(())
    }
}
