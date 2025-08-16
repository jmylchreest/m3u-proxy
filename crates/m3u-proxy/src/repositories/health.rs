use crate::errors::RepositoryResult;
use sqlx::{Pool, Sqlite, Row};
use uuid::Uuid;

/// Repository for health check and monitoring queries
/// 
/// This repository provides specialized queries for health monitoring,
/// status checks, and operational metrics that don't fit into entity-specific repositories.
#[derive(Clone)]
pub struct HealthRepository {
    pool: Pool<Sqlite>,
}

impl HealthRepository {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Get count of active stream sources with scheduling enabled
    pub async fn get_active_stream_sources_count(&self) -> RepositoryResult<u32> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM stream_sources WHERE is_active = 1 AND update_cron IS NOT NULL AND update_cron != ''"
        )
        .fetch_one(&self.pool)
        .await?;
        
        Ok(count as u32)
    }

    /// Get count of active EPG sources with scheduling enabled
    pub async fn get_active_epg_sources_count(&self) -> RepositoryResult<u32> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM epg_sources WHERE is_active = 1 AND update_cron IS NOT NULL AND update_cron != ''"
        )
        .fetch_one(&self.pool)
        .await?;
        
        Ok(count as u32)
    }

    /// Get stream sources with scheduling information
    pub async fn get_scheduled_stream_sources(&self) -> RepositoryResult<Vec<ScheduledSource>> {
        let rows = sqlx::query(
            "SELECT id, name, update_cron FROM stream_sources 
             WHERE is_active = 1 AND update_cron IS NOT NULL AND update_cron != '' 
             ORDER BY name"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut sources = Vec::new();
        for row in rows {
            if let (Ok(id_str), Ok(name), Ok(cron)) = (
                row.try_get::<String, _>("id"),
                row.try_get::<String, _>("name"),
                row.try_get::<String, _>("update_cron")
            ) {
                if let Ok(id) = crate::utils::uuid_parser::parse_uuid_flexible(&id_str) {
                    sources.push(ScheduledSource {
                        id,
                        name,
                        source_type: "Stream".to_string(),
                        cron_expression: cron,
                    });
                }
            }
        }

        Ok(sources)
    }

    /// Get EPG sources with scheduling information
    pub async fn get_scheduled_epg_sources(&self) -> RepositoryResult<Vec<ScheduledSource>> {
        let rows = sqlx::query(
            "SELECT id, name, update_cron FROM epg_sources 
             WHERE is_active = 1 AND update_cron IS NOT NULL AND update_cron != '' 
             ORDER BY name"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut sources = Vec::new();
        for row in rows {
            if let (Ok(id_str), Ok(name), Ok(cron)) = (
                row.try_get::<String, _>("id"),
                row.try_get::<String, _>("name"),
                row.try_get::<String, _>("update_cron")
            ) {
                if let Ok(id) = crate::utils::uuid_parser::parse_uuid_flexible(&id_str) {
                    sources.push(ScheduledSource {
                        id,
                        name,
                        source_type: "EPG".to_string(),
                        cron_expression: cron,
                    });
                }
            }
        }

        Ok(sources)
    }

    /// Get channel information by ID (for proxies.rs refactor)
    pub async fn get_channel_info(&self, channel_id: Uuid) -> RepositoryResult<Option<ChannelInfo>> {
        let row = sqlx::query(
            "SELECT c.channel_name, ss.name as source_name 
             FROM channels c 
             LEFT JOIN stream_sources ss ON c.source_id = ss.id 
             WHERE c.id = ?"
        )
        .bind(channel_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let channel_name: String = row.get("channel_name");
                let source_name: Option<String> = row.get("source_name");
                Ok(Some(ChannelInfo {
                    channel_name,
                    source_name,
                }))
            }
            None => Ok(None),
        }
    }

    /// Get proxy last generated timestamp
    pub async fn get_proxy_last_generated(&self, proxy_id: Uuid) -> RepositoryResult<Option<String>> {
        let timestamp = sqlx::query_scalar::<_, Option<String>>(
            "SELECT last_generated_at FROM stream_proxies WHERE id = ?"
        )
        .bind(proxy_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        Ok(timestamp.flatten())
    }
}

/// Scheduled source information for health checks
#[derive(Debug, Clone)]
pub struct ScheduledSource {
    pub id: Uuid,
    pub name: String,
    pub source_type: String,
    pub cron_expression: String,
}

/// Channel information for health/status checks
#[derive(Debug, Clone)]
pub struct ChannelInfo {
    pub channel_name: String,
    pub source_name: Option<String>,
}