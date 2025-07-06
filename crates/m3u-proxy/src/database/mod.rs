use crate::assets::MigrationAssets;
use crate::config::{DatabaseBatchConfig, DatabaseConfig, IngestionConfig};
use crate::models::*;
use anyhow::Result;
use sqlx::{migrate::MigrateDatabase, Pool, Row, Sqlite, SqlitePool};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing;
use uuid::Uuid;
pub mod channel_mapping;
pub mod epg_sources;
pub mod filters;
pub mod linked_xtream;
pub mod stream_sources;

#[derive(Clone, Debug)]
pub struct Database {
    pool: Pool<Sqlite>,
    channel_update_lock: Arc<Mutex<()>>,
    ingestion_config: IngestionConfig,
    batch_config: DatabaseBatchConfig,
}

impl Database {
    pub fn pool(&self) -> Pool<Sqlite> {
        self.pool.clone()
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
        })
    }

    pub async fn migrate(&self) -> Result<()> {
        self.run_embedded_migrations().await?;
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
            "SELECT id, ulid, name, created_at, updated_at, last_generated_at, is_active,
             proxy_mode, upstream_timeout, buffer_size, max_concurrent_streams
             FROM stream_proxies WHERE id = ?",
        )
        .bind(proxy_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(StreamProxy {
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                ulid: row.get("ulid"),
                name: row.get("name"),
                description: None, // Field was added later, not in current schema
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                last_generated_at: row.get("last_generated_at"),
                is_active: row.get("is_active"),
                auto_regenerate: false, // Default value, field was added later
                proxy_mode: match row.get::<String, _>("proxy_mode").as_str() {
                    "proxy" => crate::models::StreamProxyMode::Proxy,
                    _ => crate::models::StreamProxyMode::Redirect,
                },
                upstream_timeout: row.get("upstream_timeout"),
                buffer_size: row.get("buffer_size"),
                max_concurrent_streams: row.get("max_concurrent_streams"),
                starting_channel_number: 1, // Default value, field was added later
            })),
            None => Ok(None),
        }
    }

    pub async fn get_proxy_by_ulid(&self, ulid: &str) -> Result<StreamProxy> {
        let row = sqlx::query(
            "SELECT id, ulid, name, created_at, updated_at, last_generated_at, is_active,
             proxy_mode, upstream_timeout, buffer_size, max_concurrent_streams
             FROM stream_proxies WHERE ulid = ?",
        )
        .bind(ulid)
        .fetch_one(&self.pool)
        .await?;

        Ok(StreamProxy {
            id: Uuid::parse_str(&row.get::<String, _>("id"))?,
            ulid: row.get("ulid"),
            name: row.get("name"),
            description: None, // Field was added later, not in current schema
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            last_generated_at: row.get("last_generated_at"),
            is_active: row.get("is_active"),
            auto_regenerate: false, // Default value, field was added later
            proxy_mode: match row.get::<String, _>("proxy_mode").as_str() {
                "proxy" => StreamProxyMode::Proxy,
                _ => StreamProxyMode::Redirect,
            },
            upstream_timeout: row.get("upstream_timeout"),
            buffer_size: row.get("buffer_size"),
            max_concurrent_streams: row.get("max_concurrent_streams"),
            starting_channel_number: 1, // Default value, field was added later
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
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type,
                url: row.get("url"),
                max_concurrent_streams: row.get("max_concurrent_streams"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                field_map: row.get("field_map"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                last_ingested_at: row.get("last_ingested_at"),
                is_active: row.get("is_active"),
            };
            sources.push(source);
        }

        Ok(sources)
    }

    pub async fn get_proxy_filters_with_details(
        &self,
        proxy_id: Uuid,
    ) -> Result<Vec<ProxyFilterWithDetails>> {
        let rows = sqlx::query(
            "SELECT pf.proxy_id, pf.filter_id, pf.sort_order, pf.is_active, pf.created_at,
                    f.name, f.starting_channel_number, f.is_inverse, f.condition_tree, f.updated_at as filter_updated_at
             FROM proxy_filters pf
             JOIN filters f ON pf.filter_id = f.id
             WHERE pf.proxy_id = ? AND pf.is_active = 1
             ORDER BY pf.sort_order"
        )
        .bind(proxy_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            let proxy_filter = ProxyFilter {
                proxy_id: Uuid::parse_str(&row.get::<String, _>("proxy_id"))?,
                filter_id: Uuid::parse_str(&row.get::<String, _>("filter_id"))?,
                priority_order: row.get("sort_order"),
                is_active: row.get("is_active"),
                created_at: row.get("created_at"),
            };

            let filter = Filter {
                id: proxy_filter.filter_id,
                name: row.get("name"),
                source_type: FilterSourceType::Stream,
                starting_channel_number: row.get("starting_channel_number"),
                is_inverse: row.get("is_inverse"),
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
        proxy_ulid: &str,
        channel_id: Uuid,
    ) -> Result<Option<Channel>> {
        let row = sqlx::query(
            "SELECT c.id, c.source_id, c.tvg_id, c.tvg_name, c.tvg_logo, c.tvg_shift,
             c.group_title, c.channel_name, c.stream_url, c.created_at, c.updated_at
             FROM channels c
             JOIN proxy_sources ps ON c.source_id = ps.source_id
             JOIN stream_proxies sp ON ps.proxy_id = sp.id
             WHERE sp.ulid = ? AND c.id = ? AND sp.is_active = 1",
        )
        .bind(proxy_ulid)
        .bind(channel_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(Channel {
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                source_id: Uuid::parse_str(&row.get::<String, _>("source_id"))?,
                tvg_id: row.get("tvg_id"),
                tvg_name: row.get("tvg_name"),
                tvg_logo: row.get("tvg_logo"),
                tvg_shift: row.get("tvg_shift"),
                group_title: row.get("group_title"),
                channel_name: row.get("channel_name"),
                stream_url: row.get("stream_url"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            })),
            None => Ok(None),
        }
    }

    /// Get EPG sources associated with a proxy
    pub async fn get_proxy_epg_sources(&self, proxy_id: Uuid) -> Result<Vec<EpgSource>> {
        let rows = sqlx::query(
            "SELECT e.id, e.name, e.source_type, e.url, e.update_cron, e.username, e.password,
             e.timezone, e.timezone_detected, e.time_offset, e.created_at, e.updated_at,
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
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                name: row.get("name"),
                source_type,
                url: row.get("url"),
                update_cron: row.get("update_cron"),
                username: row.get("username"),
                password: row.get("password"),
                timezone: row.get("timezone"),
                timezone_detected: row.get("timezone_detected"),
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
            "SELECT id, source_id, channel_id, channel_name, program_title, program_description,
             program_category, start_time, end_time, episode_num, season_num, rating,
             language, subtitles, aspect_ratio, created_at, updated_at
             FROM epg_programs
             WHERE channel_id = ? AND start_time >= ? AND end_time <= ?
             ORDER BY start_time",
        )
        .bind(channel_id.to_string())
        .bind(start_time)
        .bind(end_time)
        .fetch_all(&self.pool)
        .await?;

        let mut programs = Vec::new();
        for row in rows {
            let program = EpgProgram {
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                source_id: Uuid::parse_str(&row.get::<String, _>("source_id"))?,
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
            "SELECT id, source_id, channel_id, channel_name, program_title, program_description,
             program_category, start_time, end_time, episode_num, season_num, rating,
             language, subtitles, aspect_ratio, created_at, updated_at
             FROM epg_programs
             WHERE channel_id = ?
             ORDER BY start_time",
        )
        .bind(channel_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut programs = Vec::new();
        for row in rows {
            let program = EpgProgram {
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                source_id: Uuid::parse_str(&row.get::<String, _>("source_id"))?,
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
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            };
            programs.push(program);
        }

        Ok(programs)
    }
}
