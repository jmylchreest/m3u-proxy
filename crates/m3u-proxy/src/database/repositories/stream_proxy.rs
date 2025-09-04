//! SeaORM StreamProxy repository implementation
//!
//! This module provides the SeaORM implementation of stream proxy repository
//! that works across SQLite, PostgreSQL, and MySQL databases.

use anyhow::Result;
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, ColumnTrait, QueryOrder, ActiveModelTrait, Set, ModelTrait};
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::{stream_proxies, prelude::*};
use crate::models::{StreamProxy, StreamProxyCreateRequest, StreamProxyUpdateRequest};

/// SeaORM-based StreamProxy repository
#[derive(Clone)]
pub struct StreamProxySeaOrmRepository {
    connection: Arc<DatabaseConnection>,
}

impl StreamProxySeaOrmRepository {
    /// Create a new StreamProxySeaOrmRepository
    pub fn new(connection: Arc<DatabaseConnection>) -> Self {
        Self { connection }
    }

    /// Create a new stream proxy
    pub async fn create(&self, request: StreamProxyCreateRequest) -> Result<StreamProxy> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let active_model = stream_proxies::ActiveModel {
            id: Set(id),
            name: Set(request.name.clone()),
            description: Set(request.description.clone()),
            proxy_mode: Set(request.proxy_mode),
            upstream_timeout: Set(request.upstream_timeout),
            buffer_size: Set(request.buffer_size),
            max_concurrent_streams: Set(request.max_concurrent_streams),
            starting_channel_number: Set(request.starting_channel_number),
            created_at: Set(now),
            updated_at: Set(now),
            last_generated_at: Set(None),
            is_active: Set(request.is_active),
            auto_regenerate: Set(request.auto_regenerate),
            cache_channel_logos: Set(request.cache_channel_logos),
            cache_program_logos: Set(request.cache_program_logos),
            relay_profile_id: Set(request.relay_profile_id),
        };

        let model = active_model.insert(&*self.connection).await?;
        Ok(StreamProxy {
            id: model.id,
            name: model.name,
            description: model.description,
            proxy_mode: model.proxy_mode,
            upstream_timeout: model.upstream_timeout,
            buffer_size: model.buffer_size,
            max_concurrent_streams: model.max_concurrent_streams,
            starting_channel_number: model.starting_channel_number,
            created_at: model.created_at,
            updated_at: model.updated_at,
            last_generated_at: model.last_generated_at.as_ref().map(|time| *time),
            is_active: model.is_active,
            auto_regenerate: model.auto_regenerate,
            cache_channel_logos: model.cache_channel_logos,
            cache_program_logos: model.cache_program_logos,
            relay_profile_id: model.relay_profile_id,
        })
    }

    /// Find stream proxy by ID
    pub async fn find_by_id(&self, id: &Uuid) -> Result<Option<StreamProxy>> {
        let model = StreamProxies::find_by_id(*id).one(&*self.connection).await?;
        match model {
            Some(m) => Ok(Some(StreamProxy {
                id: m.id,
                name: m.name,
                description: m.description,
                proxy_mode: m.proxy_mode,
                upstream_timeout: m.upstream_timeout,
                buffer_size: m.buffer_size,
                max_concurrent_streams: m.max_concurrent_streams,
                starting_channel_number: m.starting_channel_number,
                created_at: m.created_at,
                updated_at: m.updated_at,
                last_generated_at: m.last_generated_at.as_ref().map(|time| *time),
                is_active: m.is_active,
                auto_regenerate: m.auto_regenerate,
                cache_channel_logos: m.cache_channel_logos,
                cache_program_logos: m.cache_program_logos,
                relay_profile_id: m.relay_profile_id,
            })),
            None => Ok(None)
        }
    }

    /// List all stream proxies
    pub async fn list_all(&self) -> Result<Vec<StreamProxy>> {
        let models = StreamProxies::find()
            .order_by_asc(stream_proxies::Column::Name)
            .all(&*self.connection)
            .await?;

        let mut results = Vec::new();
        for m in models {
            results.push(StreamProxy {
                id: m.id,
                name: m.name,
                description: m.description,
                proxy_mode: m.proxy_mode,
                upstream_timeout: m.upstream_timeout,
                buffer_size: m.buffer_size,
                max_concurrent_streams: m.max_concurrent_streams,
                starting_channel_number: m.starting_channel_number,
                created_at: m.created_at,
                updated_at: m.updated_at,
                last_generated_at: m.last_generated_at.as_ref().map(|time| *time),
                is_active: m.is_active,
                auto_regenerate: m.auto_regenerate,
                cache_channel_logos: m.cache_channel_logos,
                cache_program_logos: m.cache_program_logos,
                relay_profile_id: m.relay_profile_id,
            });
        }
        Ok(results)
    }

    /// Update stream proxy
    pub async fn update(&self, id: &Uuid, request: StreamProxyUpdateRequest) -> Result<StreamProxy> {
        let model = StreamProxies::find_by_id(*id)
            .one(&*self.connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Stream proxy not found"))?;

        let mut active_model: stream_proxies::ActiveModel = model.into();
        
        active_model.name = Set(request.name);
        active_model.description = Set(request.description);
        active_model.proxy_mode = Set(request.proxy_mode);
        active_model.upstream_timeout = Set(request.upstream_timeout);
        active_model.buffer_size = Set(request.buffer_size);
        active_model.max_concurrent_streams = Set(request.max_concurrent_streams);
        active_model.starting_channel_number = Set(request.starting_channel_number);
        active_model.is_active = Set(request.is_active);
        active_model.auto_regenerate = Set(request.auto_regenerate);
        active_model.cache_channel_logos = Set(request.cache_channel_logos);
        active_model.cache_program_logos = Set(request.cache_program_logos);
        active_model.relay_profile_id = Set(request.relay_profile_id);
        active_model.updated_at = Set(chrono::Utc::now());

        let updated_model = active_model.update(&*self.connection).await?;
        Ok(StreamProxy {
            id: updated_model.id,
            name: updated_model.name,
            description: updated_model.description,
            proxy_mode: updated_model.proxy_mode,
            upstream_timeout: updated_model.upstream_timeout,
            buffer_size: updated_model.buffer_size,
            max_concurrent_streams: updated_model.max_concurrent_streams,
            starting_channel_number: updated_model.starting_channel_number,
            created_at: updated_model.created_at,
            updated_at: updated_model.updated_at,
            last_generated_at: updated_model.last_generated_at.as_ref().map(|time| *time),
            is_active: updated_model.is_active,
            auto_regenerate: updated_model.auto_regenerate,
            cache_channel_logos: updated_model.cache_channel_logos,
            cache_program_logos: updated_model.cache_program_logos,
            relay_profile_id: updated_model.relay_profile_id,
        })
    }

    /// Delete stream proxy
    pub async fn delete(&self, id: &Uuid) -> Result<()> {
        let model = StreamProxies::find_by_id(*id)
            .one(&*self.connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Stream proxy not found"))?;

        model.delete(&*self.connection).await?;
        Ok(())
    }

    /// Update last_generated_at timestamp for a proxy
    pub async fn update_last_generated(&self, proxy_id: Uuid) -> Result<()> {
        use sea_orm::{ActiveModelTrait, Set, EntityTrait};
        
        let model = StreamProxies::find_by_id(proxy_id)
            .one(&*self.connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Stream proxy not found"))?;

        let mut active_model: stream_proxies::ActiveModel = model.into();
        active_model.last_generated_at = Set(Some(chrono::Utc::now()));
        active_model.updated_at = Set(chrono::Utc::now());
        
        active_model.update(&*self.connection).await?;
        Ok(())
    }

    /// Get stream source IDs for a proxy
    pub async fn get_stream_source_ids(&self, proxy_id: Uuid) -> Result<Vec<Uuid>> {
        let proxy_sources = self.get_proxy_sources(proxy_id).await?;
        Ok(proxy_sources.into_iter().map(|ps| ps.source_id).collect())
    }

    /// Get EPG source IDs for a proxy
    pub async fn get_epg_source_ids(&self, proxy_id: Uuid) -> Result<Vec<Uuid>> {
        let proxy_epg_sources = self.get_proxy_epg_sources(proxy_id).await?;
        Ok(proxy_epg_sources.into_iter().map(|pes| pes.epg_source_id).collect())
    }

    /// Alias for find_by_id to maintain API consistency
    pub async fn get_by_id(&self, id: &Uuid) -> Result<Option<StreamProxy>> {
        self.find_by_id(id).await
    }

    /// Alias for list_all to maintain API consistency  
    pub async fn find_all(&self) -> Result<Vec<StreamProxy>> {
        self.list_all().await
    }

    /// Create a stream proxy with relationships (sources, filters, epg sources)
    pub async fn create_with_relationships(
        &self, 
        request: StreamProxyCreateRequest,
        source_ids: Vec<Uuid>,
        epg_source_ids: Vec<Uuid>
    ) -> Result<StreamProxy> {
        use crate::entities::{proxy_sources, proxy_filters, proxy_epg_sources, stream_proxies};
        use sea_orm::{ActiveModelTrait, Set, TransactionTrait};

        // Start a transaction to ensure all relationships are created atomically
        let txn = self.connection.begin().await?;

        // First create the proxy within the transaction
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let active_model = stream_proxies::ActiveModel {
            id: Set(id),
            name: Set(request.name.clone()),
            description: Set(request.description.clone()),
            proxy_mode: Set(request.proxy_mode),
            upstream_timeout: Set(request.upstream_timeout),
            buffer_size: Set(request.buffer_size),
            max_concurrent_streams: Set(request.max_concurrent_streams),
            starting_channel_number: Set(request.starting_channel_number),
            created_at: Set(now),
            updated_at: Set(now),
            last_generated_at: Set(None),
            is_active: Set(request.is_active),
            auto_regenerate: Set(request.auto_regenerate),
            cache_channel_logos: Set(request.cache_channel_logos),
            cache_program_logos: Set(request.cache_program_logos),
            relay_profile_id: Set(request.relay_profile_id),
        };

        let model = active_model.insert(&txn).await?;
        let proxy = StreamProxy {
            id: model.id,
            name: model.name,
            description: model.description,
            proxy_mode: model.proxy_mode,
            upstream_timeout: model.upstream_timeout,
            buffer_size: model.buffer_size,
            max_concurrent_streams: model.max_concurrent_streams,
            starting_channel_number: model.starting_channel_number,
            created_at: model.created_at,
            updated_at: model.updated_at,
            last_generated_at: model.last_generated_at,
            is_active: model.is_active,
            auto_regenerate: model.auto_regenerate,
            cache_channel_logos: model.cache_channel_logos,
            cache_program_logos: model.cache_program_logos,
            relay_profile_id: model.relay_profile_id,
        };

        // Create proxy_sources relationships
        for (index, source_id) in source_ids.into_iter().enumerate() {
            let proxy_source = proxy_sources::ActiveModel {
                proxy_id: Set(proxy.id),
                source_id: Set(source_id),
                priority_order: Set(index as i32 + 1),
                created_at: Set(chrono::Utc::now()),
            };
            proxy_source.insert(&txn).await?;
        }

        // Create proxy_filters relationships with priority from request
        for filter_req in &request.filters {
            let proxy_filter = proxy_filters::ActiveModel {
                proxy_id: Set(proxy.id),
                filter_id: Set(filter_req.filter_id),
                priority_order: Set(filter_req.priority_order),
                is_active: Set(filter_req.is_active),
                created_at: Set(chrono::Utc::now()),
            };
            proxy_filter.insert(&txn).await?;
        }

        // Create proxy_epg_sources relationships
        for (index, epg_source_id) in epg_source_ids.into_iter().enumerate() {
            let proxy_epg_source = proxy_epg_sources::ActiveModel {
                proxy_id: Set(proxy.id),
                epg_source_id: Set(epg_source_id),
                priority_order: Set(index as i32 + 1),
                created_at: Set(chrono::Utc::now()),
            };
            proxy_epg_source.insert(&txn).await?;
        }

        // Commit the transaction
        txn.commit().await?;

        Ok(proxy)
    }

    /// Update a stream proxy with relationships
    pub async fn update_with_relationships(
        &self,
        id: &Uuid,
        request: StreamProxyUpdateRequest,
        source_ids: Vec<Uuid>, 
        epg_source_ids: Vec<Uuid>
    ) -> Result<StreamProxy> {
        use crate::entities::{proxy_sources, proxy_filters, proxy_epg_sources, stream_proxies};
        use sea_orm::{ActiveModelTrait, Set, TransactionTrait, EntityTrait, QueryFilter, ColumnTrait};

        // Start a transaction to ensure all updates are atomic
        let txn = self.connection.begin().await?;

        // First update the proxy itself within the transaction
        let model = StreamProxies::find_by_id(*id)
            .one(&txn)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Stream proxy not found"))?;

        let mut active_model: stream_proxies::ActiveModel = model.into();
        
        active_model.name = Set(request.name);
        active_model.description = Set(request.description);
        active_model.proxy_mode = Set(request.proxy_mode);
        active_model.upstream_timeout = Set(request.upstream_timeout);
        active_model.buffer_size = Set(request.buffer_size);
        active_model.max_concurrent_streams = Set(request.max_concurrent_streams);
        active_model.starting_channel_number = Set(request.starting_channel_number);
        active_model.is_active = Set(request.is_active);
        active_model.auto_regenerate = Set(request.auto_regenerate);
        active_model.cache_channel_logos = Set(request.cache_channel_logos);
        active_model.cache_program_logos = Set(request.cache_program_logos);
        active_model.relay_profile_id = Set(request.relay_profile_id);
        active_model.updated_at = Set(chrono::Utc::now());

        let updated_model = active_model.update(&txn).await?;

        // Delete existing relationships
        ProxySources::delete_many()
            .filter(proxy_sources::Column::ProxyId.eq(*id))
            .exec(&txn)
            .await?;

        ProxyFilters::delete_many()
            .filter(proxy_filters::Column::ProxyId.eq(*id))
            .exec(&txn)
            .await?;

        ProxyEpgSources::delete_many()
            .filter(proxy_epg_sources::Column::ProxyId.eq(*id))
            .exec(&txn)
            .await?;

        // Create new proxy_sources relationships
        for (index, source_id) in source_ids.into_iter().enumerate() {
            let proxy_source = proxy_sources::ActiveModel {
                proxy_id: Set(*id),
                source_id: Set(source_id),
                priority_order: Set(index as i32 + 1),
                created_at: Set(chrono::Utc::now()),
            };
            proxy_source.insert(&txn).await?;
        }

        // Create new proxy_filters relationships with priority from request
        for filter_req in &request.filters {
            let proxy_filter = proxy_filters::ActiveModel {
                proxy_id: Set(*id),
                filter_id: Set(filter_req.filter_id),
                priority_order: Set(filter_req.priority_order),
                is_active: Set(filter_req.is_active),
                created_at: Set(chrono::Utc::now()),
            };
            proxy_filter.insert(&txn).await?;
        }

        // Create new proxy_epg_sources relationships
        for (index, epg_source_id) in epg_source_ids.into_iter().enumerate() {
            let proxy_epg_source = proxy_epg_sources::ActiveModel {
                proxy_id: Set(*id),
                epg_source_id: Set(epg_source_id),
                priority_order: Set(index as i32 + 1),
                created_at: Set(chrono::Utc::now()),
            };
            proxy_epg_source.insert(&txn).await?;
        }

        // Commit the transaction
        txn.commit().await?;

        Ok(StreamProxy {
            id: updated_model.id,
            name: updated_model.name,
            description: updated_model.description,
            proxy_mode: updated_model.proxy_mode,
            upstream_timeout: updated_model.upstream_timeout,
            buffer_size: updated_model.buffer_size,
            max_concurrent_streams: updated_model.max_concurrent_streams,
            starting_channel_number: updated_model.starting_channel_number,
            created_at: updated_model.created_at,
            updated_at: updated_model.updated_at,
            last_generated_at: updated_model.last_generated_at,
            is_active: updated_model.is_active,
            auto_regenerate: updated_model.auto_regenerate,
            cache_channel_logos: updated_model.cache_channel_logos,
            cache_program_logos: updated_model.cache_program_logos,
            relay_profile_id: updated_model.relay_profile_id,
        })
    }

    /// Get proxy filters for a stream proxy
    pub async fn get_proxy_filters(&self, proxy_id: Uuid) -> Result<Vec<crate::models::ProxyFilter>> {
        use crate::entities::{proxy_filters, prelude::ProxyFilters};
        
        let proxy_filter_models = ProxyFilters::find()
            .filter(proxy_filters::Column::ProxyId.eq(proxy_id))
            .filter(proxy_filters::Column::IsActive.eq(true))
            .all(&*self.connection)
            .await?;

        let mut proxy_filters = Vec::new();
        for model in proxy_filter_models {
            proxy_filters.push(crate::models::ProxyFilter {
                proxy_id: model.proxy_id,
                filter_id: model.filter_id,
                priority_order: model.priority_order,
                is_active: model.is_active,
                created_at: model.created_at,
            });
        }

        Ok(proxy_filters)
    }

    /// Get proxy sources for a stream proxy
    pub async fn get_proxy_sources(&self, proxy_id: Uuid) -> Result<Vec<crate::models::ProxySource>> {
        use crate::entities::{proxy_sources, prelude::ProxySources};
        
        let proxy_source_models = ProxySources::find()
            .filter(proxy_sources::Column::ProxyId.eq(proxy_id))
            .all(&*self.connection)
            .await?;

        let mut proxy_sources = Vec::new();
        for model in proxy_source_models {
            proxy_sources.push(crate::models::ProxySource {
                proxy_id: model.proxy_id,
                source_id: model.source_id,
                priority_order: model.priority_order,
                created_at: model.created_at,
            });
        }

        Ok(proxy_sources)
    }

    /// Get proxy EPG sources for a stream proxy
    pub async fn get_proxy_epg_sources(&self, proxy_id: Uuid) -> Result<Vec<crate::models::ProxyEpgSource>> {
        use crate::entities::{proxy_epg_sources, prelude::ProxyEpgSources};
        
        let proxy_epg_source_models = ProxyEpgSources::find()
            .filter(proxy_epg_sources::Column::ProxyId.eq(proxy_id))
            .all(&*self.connection)
            .await?;

        let mut proxy_epg_sources = Vec::new();
        for model in proxy_epg_source_models {
            proxy_epg_sources.push(crate::models::ProxyEpgSource {
                proxy_id: model.proxy_id,
                epg_source_id: model.epg_source_id,
                priority_order: model.priority_order,
                created_at: model.created_at,
            });
        }

        Ok(proxy_epg_sources)
    }

    /// Find EPG source by ID using the EpgSourceSeaOrmRepository
    pub async fn find_epg_source_by_id(&self, epg_source_id: Uuid) -> Result<Option<crate::models::EpgSource>> {
        use crate::database::repositories::epg_source::EpgSourceSeaOrmRepository;
        
        let epg_repo = EpgSourceSeaOrmRepository::new(self.connection.clone());
        epg_repo.find_by_id(&epg_source_id).await
    }

    /// Get channel for proxy - check if channel belongs to sources linked to the proxy
    pub async fn get_channel_for_proxy(&self, proxy_id: Uuid, channel_id: Uuid) -> Result<Option<crate::models::Channel>> {
        use crate::entities::{prelude::ProxySources, proxy_sources};
        use crate::database::repositories::channel::ChannelSeaOrmRepository;
        
        // First check if the channel exists and get its source_id
        let channel_repo = ChannelSeaOrmRepository::new(self.connection.clone());
        let channel = match channel_repo.find_by_id(&channel_id).await? {
            Some(channel) => channel,
            None => return Ok(None),
        };
        
        // Then check if this channel's source is linked to the proxy
        let proxy_source_exists = ProxySources::find()
            .filter(proxy_sources::Column::ProxyId.eq(proxy_id))
            .filter(proxy_sources::Column::SourceId.eq(channel.source_id))
            .one(&*self.connection)
            .await?;
            
        match proxy_source_exists {
            Some(_) => Ok(Some(channel)),
            None => Ok(None),
        }
    }

}