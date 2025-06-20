use crate::assets::MigrationAssets;
use crate::config::{DatabaseConfig, IngestionConfig};
use anyhow::Result;
use sqlx::{migrate::MigrateDatabase, Pool, Sqlite, SqlitePool};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing;
pub mod filters;
pub mod stream_sources;

#[derive(Clone)]
pub struct Database {
    pool: Pool<Sqlite>,
    channel_update_lock: Arc<Mutex<()>>,
    ingestion_config: IngestionConfig,
}

impl Database {
    pub async fn new(config: &DatabaseConfig, ingestion_config: &IngestionConfig) -> Result<Self> {
        // Create database if it doesn't exist (for SQLite)
        if !Sqlite::database_exists(&config.url).await? {
            Sqlite::create_database(&config.url).await?;
        }

        let pool = SqlitePool::connect(&config.url).await?;

        Ok(Self {
            pool,
            channel_update_lock: Arc::new(Mutex::new(())),
            ingestion_config: ingestion_config.clone(),
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

    #[allow(dead_code)]
    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    pub async fn acquire_channel_update_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.channel_update_lock.lock().await
    }
}
