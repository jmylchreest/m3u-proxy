use crate::config::DatabaseConfig;
use anyhow::Result;
use sqlx::{migrate::MigrateDatabase, Pool, Sqlite, SqlitePool};
use std::sync::Arc;
use tokio::sync::Mutex;

pub mod stream_sources;

#[derive(Clone)]
pub struct Database {
    pool: Pool<Sqlite>,
    channel_update_lock: Arc<Mutex<()>>,
}

impl Database {
    pub async fn new(config: &DatabaseConfig) -> Result<Self> {
        // Create database if it doesn't exist (for SQLite)
        if !Sqlite::database_exists(&config.url).await? {
            Sqlite::create_database(&config.url).await?;
        }

        let pool = SqlitePool::connect(&config.url).await?;

        Ok(Self {
            pool,
            channel_update_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    pub async fn acquire_channel_update_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.channel_update_lock.lock().await
    }
}
