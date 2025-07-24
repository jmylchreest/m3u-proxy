use crate::assets::MigrationAssets;
use crate::config::{DatabaseBatchConfig, DatabaseConfig, IngestionConfig};
use crate::models::*;
use crate::ingestor::scheduler::SchedulerEvent;
use anyhow::Result;
use sqlx::{Pool, Row, Sqlite, SqlitePool, migrate::MigrateDatabase};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tracing;
use uuid::Uuid;
use crate::utils::uuid_parser::parse_uuid_flexible;
pub mod channel_mapping;
pub mod epg_sources;
pub mod filters;
pub mod linked_xtream;
pub mod stream_sources;
pub mod url_linking;

#[derive(Clone)]
pub struct Database {
    pool: Pool<Sqlite>,
    channel_update_lock: Arc<Mutex<()>>,
    ingestion_config: IngestionConfig,
    batch_config: DatabaseBatchConfig,
    scheduler_event_tx: Option<mpsc::UnboundedSender<SchedulerEvent>>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("pool", &"SqlitePool")
            .field("channel_update_lock", &"Mutex<()>")
            .field("ingestion_config", &self.ingestion_config)
            .field("batch_config", &self.batch_config)
            .field("scheduler_event_tx", &self.scheduler_event_tx.is_some())
            .finish()
    }
}

impl Database {
    pub fn pool(&self) -> Pool<Sqlite> {
        self.pool.clone()
    }

    /// Set the scheduler event sender for database operations to notify scheduler of changes
    pub fn set_scheduler_event_sender(&mut self, sender: mpsc::UnboundedSender<SchedulerEvent>) {
        self.scheduler_event_tx = Some(sender);
    }

    /// Emit a scheduler event if the sender is available
    pub fn emit_scheduler_event(&self, event: SchedulerEvent) {
        if let Some(ref sender) = self.scheduler_event_tx {
            if let Err(e) = sender.send(event) {
                tracing::warn!("Failed to send scheduler event: {}", e);
            }
        }
    }

    pub async fn new(config: &DatabaseConfig, ingestion_config: &IngestionConfig) -> Result<Self> {
        // Create database if it doesn't exist (for SQLite)
        if !Sqlite::database_exists(&config.url).await? {
            Sqlite::create_database(&config.url).await?;
        }

        let pool = SqlitePool::connect(&config.url).await?;

        // Optimize SQLite for large dataset handling
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA synchronous = NORMAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA cache_size = -64000")
            .execute(&pool)
            .await?; // 64MB cache
        sqlx::query("PRAGMA temp_store = MEMORY")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA mmap_size = 268435456")
            .execute(&pool)
            .await?; // 256MB mmap
        sqlx::query("PRAGMA busy_timeout = 5000")
            .execute(&pool)
            .await?; // 5 second global timeout to reduce deadlock duration

        let batch_config = config.batch_sizes.clone().unwrap_or_default();

        // Validate batch configuration
        if let Err(e) = batch_config.validate() {
            return Err(anyhow::anyhow!(
                "Invalid database batch configuration: {}",
                e
            ));
        }

        Ok(Self {
            pool,
            channel_update_lock: Arc::new(Mutex::new(())),
            ingestion_config: ingestion_config.clone(),
            batch_config,
            scheduler_event_tx: None,
        })
    }

    pub async fn migrate(&self) -> Result<()> {
        self.run_embedded_migrations().await?;
        self.ensure_default_filters().await?;
        Ok(())
    }

    async fn run_embedded_migrations(&self) -> Result<()> {
        // Create migrations table if it doesn't exist
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS _sqlx_migrations (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                success BOOLEAN NOT NULL,
                checksum BLOB NOT NULL,
                execution_time BIGINT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Get embedded migrations
        let migrations = MigrationAssets::get_migrations();
        
        // Debug: Log what migrations are available
        tracing::info!("Available migrations: {:?}", migrations.iter().map(|(name, _)| name).collect::<Vec<_>>());

        for (name, content) in migrations {
            // Extract version from filename (e.g., "001_initial_schema.sql" -> 1)
            let version: i64 = name
                .split('_')
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| {
                    // Fallback: use hash of filename as version
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    name.hash(&mut hasher);
                    hasher.finish() as i64
                });

            // Check if migration is already applied
            let existing = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = ? AND success = true",
            )
            .bind(version)
            .fetch_one(&self.pool)
            .await?;

            if existing > 0 {
                continue; // Migration already applied
            }

            // Apply migration
            let start = std::time::Instant::now();
            let mut transaction = self.pool.begin().await?;

            match sqlx::query(&content).execute(&mut *transaction).await {
                Ok(_) => {
                    let execution_time = start.elapsed().as_millis() as i64;
                    let checksum = Self::calculate_checksum(&content);

                    // Record successful migration
                    sqlx::query(
                        r#"
                        INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
                        VALUES (?, ?, true, ?, ?)
                        "#,
                    )
                    .bind(version)
                    .bind(&name)
                    .bind(&checksum)
                    .bind(execution_time)
                    .execute(&mut *transaction)
                    .await?;

                    transaction.commit().await?;
                    tracing::info!("Applied migration: {} ({}ms)", name, execution_time);
                }
                Err(e) => {
                    tracing::error!("Migration {} failed: {}", name, e);
                    transaction.rollback().await?;
                    return Err(anyhow::anyhow!("Migration {} failed: {}", name, e));
                }
            }
        }

        Ok(())
    }

    fn calculate_checksum(content: &str) -> Vec<u8> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        hasher.finish().to_be_bytes().to_vec()
    }

    pub async fn acquire_channel_update_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.channel_update_lock.lock().await
    }

    // Proxy-related database methods
    pub async fn get_stream_proxy(&self, proxy_id: Uuid) -> Result<Option<StreamProxy>> {
        let row = sqlx::query(
            "SELECT id, name, created_at, updated_at, last_generated_at, is_active,
             proxy_mode, upstream_timeout, buffer_size, max_concurrent_streams
             FROM stream_proxies WHERE id = ?",
        )
        .bind(proxy_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(StreamProxy {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                description: None, // Field was added later, not in current schema
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                last_generated_at: row.get("last_generated_at"),
                is_active: row.get("is_active"),
                auto_regenerate: false, // Default value, field was added later
                proxy_mode: crate::models::StreamProxyMode::from_str(&row.get::<String, _>("proxy_mode")),
                upstream_timeout: row.get("upstream_timeout"),
                buffer_size: row.get("buffer_size"),
                max_concurrent_streams: row.get("max_concurrent_streams"),
                starting_channel_number: 1, // Default value, field was added later
                cache_channel_logos: true,  // Default value, field was added later
                cache_program_logos: false, // Default value, field was added later
                relay_profile_id: None,     // Field was added later, not in current schema
            })),
            None => Ok(None),
        }
    }

    pub async fn get_proxy_by_id(&self, id: &str) -> Result<StreamProxy> {
        let row = sqlx::query(
            "SELECT id, name, created_at, updated_at, last_generated_at, is_active,
             proxy_mode, upstream_timeout, buffer_size, max_concurrent_streams, relay_profile_id
             FROM stream_proxies WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;

        Ok(StreamProxy {
            id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
            name: row.get("name"),
            description: None, // Field was added later, not in current schema
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            last_generated_at: row.get("last_generated_at"),
            is_active: row.get("is_active"),
            auto_regenerate: false, // Default value, field was added later
            proxy_mode: StreamProxyMode::from_str(&row.get::<String, _>("proxy_mode")),
            upstream_timeout: row.get("upstream_timeout"),
            buffer_size: row.get("buffer_size"),
            max_concurrent_streams: row.get("max_concurrent_streams"),
            starting_channel_number: 1, // Default value, field was added later
            cache_channel_logos: true,  // Default value, field was added later
            cache_program_logos: false, // Default value, field was added later
            relay_profile_id: row.get::<Option<String>, _>("relay_profile_id")
                .and_then(|s| s.parse::<Uuid>().ok()),
        })
    }

    pub async fn get_proxy_sources(&self, proxy_id: Uuid) -> Result<Vec<StreamSource>> {
        let rows = sqlx::query(
            "SELECT s.id, s.name, s.source_type, s.url, s.max_concurrent_streams, s.update_cron,
             s.username, s.password, s.field_map, s.created_at, s.updated_at, s.last_ingested_at, s.is_active
             FROM stream_sources s
             JOIN proxy_sources ps ON s.id = ps.source_id
             WHERE ps.proxy_id = ? AND s.is_active = 1
             ORDER BY s.name"
        )
        .bind(proxy_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut sources = Vec::new();
        for row in rows {
            let source_type_str: String = row.get("source_type");
            let source_type = match source_type_str.as_str() {
                "m3u" => StreamSourceType::M3u,
                "xtream" => StreamSourceType::Xtream,
                _ => continue,
            };

            let source = StreamSource {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type,
                url: row.get("url"),
                max_concurrent_streams: row.get("max_concurrent_streams"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                field_map: row.get("field_map"),
                ignore_channel_numbers: row.get("ignore_channel_numbers"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                last_ingested_at: row.get("last_ingested_at"),
                is_active: row.get("is_active"),
            };
            sources.push(source);
        }

        Ok(sources)
    }

    /// Update the last_generated_at timestamp for a proxy
    pub async fn update_proxy_last_generated(&self, proxy_id: Uuid) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE stream_proxies SET last_generated_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(proxy_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_proxy_filters_with_details(
        &self,
        proxy_id: Uuid,
    ) -> Result<Vec<ProxyFilterWithDetails>> {
        let rows = sqlx::query(
            "SELECT pf.proxy_id, pf.filter_id, pf.priority_order, pf.is_active, pf.created_at,
                    f.name, f.starting_channel_number, f.is_inverse, f.is_system_default, f.condition_tree, f.updated_at as filter_updated_at
             FROM proxy_filters pf
             JOIN filters f ON pf.filter_id = f.id
             WHERE pf.proxy_id = ? AND pf.is_active = 1
             ORDER BY pf.priority_order"
        )
        .bind(proxy_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            let proxy_filter = ProxyFilter {
                proxy_id: parse_uuid_flexible(&row.get::<String, _>("proxy_id"))?,
                filter_id: parse_uuid_flexible(&row.get::<String, _>("filter_id"))?,
                priority_order: row.get("priority_order"),
                is_active: row.get("is_active"),
                created_at: row.get("created_at"),
            };

            let filter = Filter {
                id: proxy_filter.filter_id,
                name: row.get("name"),
                source_type: FilterSourceType::Stream,
                starting_channel_number: row.get("starting_channel_number"),
                is_inverse: row.get("is_inverse"),
                is_system_default: row.get("is_system_default"),
                condition_tree: row.get("condition_tree"),
                created_at: row.get("created_at"),
                updated_at: row.get("filter_updated_at"),
            };

            result.push(ProxyFilterWithDetails {
                proxy_filter,
                filter,
            });
        }

        Ok(result)
    }

    /// Get a channel by ID within the context of a specific proxy
    /// This validates that the channel belongs to one of the proxy's sources
    pub async fn get_channel_for_proxy(
        &self,
        proxy_id: &str,
        channel_id: Uuid,
    ) -> Result<Option<Channel>> {
        let row = sqlx::query(
            "SELECT c.id, c.source_id, c.tvg_id, c.tvg_name, c.tvg_chno, c.tvg_logo, c.tvg_shift,
             c.group_title, c.channel_name, c.stream_url, c.created_at, c.updated_at
             FROM channels c
             JOIN proxy_sources ps ON c.source_id = ps.source_id
             JOIN stream_proxies sp ON ps.proxy_id = sp.id
             WHERE sp.id = ? AND c.id = ? AND sp.is_active = 1",
        )
        .bind(proxy_id.to_string())
        .bind(channel_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(Channel {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                source_id: parse_uuid_flexible(&row.get::<String, _>("source_id"))?,
                tvg_id: row.get("tvg_id"),
                tvg_name: row.get("tvg_name"),
                tvg_chno: row.try_get("tvg_chno").unwrap_or(None),
                tvg_logo: row.get("tvg_logo"),
                tvg_shift: row.get("tvg_shift"),
                group_title: row.get("group_title"),
                channel_name: row.get("channel_name"),
                stream_url: row.get("stream_url"),
                created_at: chrono::DateTime::parse_from_rfc3339(
                    &row.get::<String, _>("created_at"),
                )?
                .with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(
                    &row.get::<String, _>("updated_at"),
                )?
                .with_timezone(&chrono::Utc),
            })),
            None => Ok(None),
        }
    }

    /// Get all active stream proxies
    pub async fn get_all_active_proxies(&self) -> Result<Vec<StreamProxy>> {
        let rows = sqlx::query(
            "SELECT id, name, created_at, updated_at, last_generated_at, is_active,
             auto_regenerate, cache_channel_logos, proxy_mode, upstream_timeout,
             buffer_size, max_concurrent_streams, starting_channel_number,
             description, cache_program_logos, relay_profile_id
             FROM stream_proxies
             WHERE is_active = 1",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut proxies = Vec::new();
        for row in rows {
            let proxy = StreamProxy {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                description: row.get("description"),
                proxy_mode: match row.get::<String, _>("proxy_mode").as_str() {
                    "redirect" => StreamProxyMode::Redirect,
                    "proxy" => StreamProxyMode::Proxy,
                    "relay" => StreamProxyMode::Relay,
                    _ => StreamProxyMode::Redirect,
                },
                upstream_timeout: row
                    .get::<Option<i64>, _>("upstream_timeout")
                    .map(|v| v as i32),
                buffer_size: row.get::<Option<i64>, _>("buffer_size").map(|v| v as i32),
                max_concurrent_streams: row
                    .get::<Option<i64>, _>("max_concurrent_streams")
                    .map(|v| v as i32),
                starting_channel_number: row.get::<i64, _>("starting_channel_number") as i32,
                created_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.get::<String, _>("created_at")
                ).map_err(|e| anyhow::anyhow!("Failed to parse created_at: {}", e))?,
                updated_at: crate::utils::datetime::DateTimeParser::parse_flexible(
                    &row.get::<String, _>("updated_at")
                ).map_err(|e| anyhow::anyhow!("Failed to parse updated_at: {}", e))?,
                last_generated_at: row
                    .get::<Option<String>, _>("last_generated_at")
                    .map(|s| crate::utils::datetime::DateTimeParser::parse_flexible(&s).ok())
                    .flatten(),
                is_active: row.get("is_active"),
                auto_regenerate: row.get("auto_regenerate"),
                cache_channel_logos: row.get("cache_channel_logos"),
                cache_program_logos: row.get("cache_program_logos"),
                relay_profile_id: row
                    .get::<Option<String>, _>("relay_profile_id")
                    .map(|s| parse_uuid_flexible(&s).ok())
                    .flatten(),
            };
            proxies.push(proxy);
        }

        Ok(proxies)
    }

    /// Get EPG sources associated with a proxy
    pub async fn get_proxy_epg_sources(&self, proxy_id: Uuid) -> Result<Vec<EpgSource>> {
        let rows = sqlx::query(
            "SELECT e.id, e.name, e.source_type, e.url, e.update_cron, e.username, e.password,
             e.original_timezone, e.time_offset, e.created_at, e.updated_at,
             e.last_ingested_at, e.is_active
             FROM epg_sources e
             JOIN proxy_epg_sources pes ON e.id = pes.epg_source_id
             WHERE pes.proxy_id = ? AND e.is_active = 1
             ORDER BY pes.priority_order",
        )
        .bind(proxy_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut sources = Vec::new();
        for row in rows {
            let source_type_str: String = row.get("source_type");
            let source_type = match source_type_str.as_str() {
                "xmltv" => EpgSourceType::Xmltv,
                "xtream" => EpgSourceType::Xtream,
                _ => continue,
            };

            let source = EpgSource {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type,
                url: row.get("url"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                original_timezone: row.get("original_timezone"),
                time_offset: row.get("time_offset"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                last_ingested_at: row.get("last_ingested_at"),
                is_active: row.get("is_active"),
            };
            sources.push(source);
        }

        Ok(sources)
    }

    /// Get EPG programs for a specific channel within a time range
    pub async fn get_epg_programs_for_channel_in_timerange(
        &self,
        channel_id: Uuid,
        start_time: chrono::DateTime<chrono::Utc>,
        end_time: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<EpgProgram>> {
        let rows = sqlx::query(
            "SELECT ep.id, ep.source_id, ep.channel_id, ep.channel_name, ep.program_title, ep.program_description,
             ep.program_category, ep.start_time, ep.end_time, ep.episode_num, ep.season_num, ep.rating,
             ep.language, ep.subtitles, ep.aspect_ratio, ep.program_icon, ep.created_at, ep.updated_at
             FROM epg_programs ep
             JOIN epg_channels ec ON ep.channel_id = ec.channel_id AND ep.source_id = ec.source_id
             WHERE ec.id = ? AND ep.start_time >= ? AND ep.end_time <= ?
             ORDER BY ep.start_time",
        )
        .bind(channel_id.to_string())
        .bind(start_time)
        .bind(end_time)
        .fetch_all(&self.pool)
        .await?;

        let mut programs = Vec::new();
        for row in rows {
            let program = EpgProgram {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                source_id: parse_uuid_flexible(&row.get::<String, _>("source_id"))?,
                channel_id: row.get("channel_id"),
                channel_name: row.get("channel_name"),
                program_title: row.get("program_title"),
                program_description: row.get("program_description"),
                program_category: row.get("program_category"),
                start_time: row.get("start_time"),
                end_time: row.get("end_time"),
                episode_num: row.get("episode_num"),
                season_num: row.get("season_num"),
                rating: row.get("rating"),
                language: row.get("language"),
                subtitles: row.get("subtitles"),
                aspect_ratio: row.get("aspect_ratio"),
                program_icon: row.get("program_icon"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            };
            programs.push(program);
        }

        Ok(programs)
    }

    /// Get all EPG programs for a specific channel
    pub async fn get_epg_programs_for_channel(&self, channel_id: Uuid) -> Result<Vec<EpgProgram>> {
        let rows = sqlx::query(
            "SELECT ep.id, ep.source_id, ep.channel_id, ep.channel_name, ep.program_title, ep.program_description,
             ep.program_category, ep.start_time, ep.end_time, ep.episode_num, ep.season_num, ep.rating,
             ep.language, ep.subtitles, ep.aspect_ratio, ep.program_icon, ep.created_at, ep.updated_at
             FROM epg_programs ep
             JOIN epg_channels ec ON ep.channel_id = ec.channel_id AND ep.source_id = ec.source_id
             WHERE ec.id = ?
             ORDER BY ep.start_time",
        )
        .bind(channel_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut programs = Vec::new();
        for row in rows {
            let program = EpgProgram {
                id: parse_uuid_flexible(&row.get::<String, _>("id"))?,
                source_id: parse_uuid_flexible(&row.get::<String, _>("source_id"))?,
                channel_id: row.get("channel_id"),
                channel_name: row.get("channel_name"),
                program_title: row.get("program_title"),
                program_description: row.get("program_description"),
                program_category: row.get("program_category"),
                start_time: row.get("start_time"),
                end_time: row.get("end_time"),
                episode_num: row.get("episode_num"),
                season_num: row.get("season_num"),
                rating: row.get("rating"),
                language: row.get("language"),
                subtitles: row.get("subtitles"),
                aspect_ratio: row.get("aspect_ratio"),
                program_icon: row.get("program_icon"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            };
            programs.push(program);
        }

        Ok(programs)
    }

}
