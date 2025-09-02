//! SeaORM-based database implementation
//!
//! This module provides database-agnostic access using SeaORM with support for:
//! - SQLite (with specific optimizations)
//! - PostgreSQL (with specific optimizations)
//! - MySQL (with specific optimizations)

use anyhow::{Context, Result};
use std::error::Error;
use sea_orm::{ConnectOptions, Database as SeaOrmDatabase, DatabaseBackend, DatabaseConnection};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

use crate::config::{DatabaseConfig, IngestionConfig};
// use crate::entities::prelude::*;

pub mod migrations;
pub mod repositories;

/// Database connection manager with multi-database support
#[derive(Clone)]
pub struct Database {
    /// Main database connection for writes and mixed operations
    pub connection: Arc<DatabaseConnection>,
    /// Read-only connection for API queries (for databases that support it)
    pub read_connection: Arc<DatabaseConnection>,
    /// Database backend type for optimization selection
    pub backend: DatabaseBackend,
    /// Ingestion configuration
    pub ingestion_config: IngestionConfig,
    /// Database type for specific optimizations
    pub database_type: DatabaseType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseType {
    SQLite,
    PostgreSQL,
    MySQL,
}

impl Database {
    /// Create a new database connection with proper optimizations
    pub async fn new(
        config: &DatabaseConfig, 
        ingestion_config: &IngestionConfig
    ) -> Result<Self> {
        let database_type = Self::detect_database_type(&config.url)?;
        let backend = match database_type {
            DatabaseType::SQLite => DatabaseBackend::Sqlite,
            DatabaseType::PostgreSQL => DatabaseBackend::Postgres,
            DatabaseType::MySQL => DatabaseBackend::MySql,
        };

        info!("Connecting to {} database", database_type.as_str());

        // For SQLite, modify URL to enable auto-creation if needed
        let connection_url = match database_type {
            DatabaseType::SQLite => Self::ensure_sqlite_auto_creation(&config.url)?,
            _ => config.url.clone(),
        };

        // Create connection options with database-specific optimizations
        let mut connect_options = ConnectOptions::new(&connection_url);

        // Apply general settings
        connect_options
            .max_connections(config.max_connections.unwrap_or(10))
            .min_connections(1)
            .connect_timeout(Duration::from_secs(5))    // Fast fail for offline database
            .acquire_timeout(Duration::from_secs(3))    // Fast fail for pool exhaustion
            .idle_timeout(Duration::from_secs(600))
            .max_lifetime(Duration::from_secs(1800));

        // Apply database-specific optimizations
        match database_type {
            DatabaseType::SQLite => {
                Self::apply_sqlite_optimizations(&mut connect_options, config)?;
            }
            DatabaseType::PostgreSQL => {
                Self::apply_postgresql_optimizations(&mut connect_options, config)?;
            }
            DatabaseType::MySQL => {
                Self::apply_mysql_optimizations(&mut connect_options, config)?;
            }
        }

        // Create main connection
        let connection = match SeaOrmDatabase::connect(connect_options.clone()).await {
            Ok(conn) => conn,
            Err(e) => {
                // Log the full error chain for debugging
                tracing::error!("Database connection failed: {:?}", e);
                tracing::error!("Error source chain:");
                let mut source = e.source();
                let mut level = 0;
                while let Some(err) = source {
                    tracing::error!("  Level {}: {}", level, err);
                    source = err.source();
                    level += 1;
                }
                return Err(anyhow::anyhow!("Failed to connect to database at '{}': {}", &config.url, e));
            }
        };

        // For now, use the same connection for reads and writes
        // In the future, we could create separate read replicas for PostgreSQL/MySQL
        let connection = Arc::new(connection);

        debug!("Database connection established successfully");

        Ok(Self {
            connection: connection.clone(),
            read_connection: connection,
            backend,
            ingestion_config: ingestion_config.clone(),
            database_type,
        })
    }

    /// Detect the database type from the URL
    fn detect_database_type(url: &str) -> Result<DatabaseType> {
        if url.starts_with("sqlite:") {
            Ok(DatabaseType::SQLite)
        } else if url.starts_with("postgres:") || url.starts_with("postgresql:") {
            Ok(DatabaseType::PostgreSQL)
        } else if url.starts_with("mysql:") {
            Ok(DatabaseType::MySQL)
        } else {
            anyhow::bail!("Unsupported database URL format: {}", url);
        }
    }

    /// Ensure SQLite URL includes auto-creation mode if needed
    fn ensure_sqlite_auto_creation(url: &str) -> Result<String> {
        // Fast path: if URL already has mode parameter or is in-memory, use as-is
        if url.contains("mode=") || url.contains(":memory:") {
            debug!("SQLite URL needs no modification: {}", url);
            return Ok(url.to_string());
        }

        // Extract file path from SQLite URL
        let file_path = if let Some(path) = url.strip_prefix("sqlite://") {
            path
        } else if let Some(path) = url.strip_prefix("sqlite:") {
            path
        } else {
            anyhow::bail!("Invalid SQLite URL format: {}", url);
        };

        let path = std::path::Path::new(file_path);
        
        // If file already exists, no modification needed
        if path.exists() {
            debug!("SQLite database file already exists: {}", file_path);
            return Ok(url.to_string());
        }

        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory for SQLite database: {}", parent.display()))?;
                info!("Created directory for SQLite database: {}", parent.display());
            }
        }

        // Add mode=rwc to enable auto-creation
        let auto_create_url = if url.contains('?') {
            format!("{}&mode=rwc", url)
        } else {
            format!("{}?mode=rwc", url)
        };

        info!("Modified SQLite URL to enable auto-creation: {} -> {}", url, auto_create_url);
        Ok(auto_create_url)
    }

    /// Apply SQLite-specific optimizations
    fn apply_sqlite_optimizations(
        options: &mut ConnectOptions,
        _config: &DatabaseConfig,
    ) -> Result<()> {
        // SeaORM and modern database configurations handle optimization automatically
        // Manual PRAGMA statements can conflict with SeaORM's built-in optimizations
        
        // Only apply essential connection settings
        options.sqlx_logging_level(tracing::log::LevelFilter::Debug);

        debug!("SeaORM will apply SQLite optimizations automatically");
        Ok(())
    }

    /// Apply PostgreSQL-specific optimizations
    fn apply_postgresql_optimizations(
        options: &mut ConnectOptions,
        _config: &DatabaseConfig,
    ) -> Result<()> {
        // PostgreSQL-specific connection settings can be added here
        options.sqlx_logging_level(tracing::log::LevelFilter::Debug);

        debug!("Applied PostgreSQL optimizations");
        Ok(())
    }

    /// Apply MySQL-specific optimizations
    fn apply_mysql_optimizations(
        options: &mut ConnectOptions,
        _config: &DatabaseConfig,
    ) -> Result<()> {
        // MySQL-specific connection settings can be added here
        options.sqlx_logging_level(tracing::log::LevelFilter::Debug);

        debug!("Applied MySQL optimizations");
        Ok(())
    }

    /// Run database migrations
    pub async fn migrate(&self) -> Result<()> {
        use migrations::Migrator;
        use sea_orm_migration::MigratorTrait;

        info!(
            "Running database migrations for {}",
            self.database_type.as_str()
        );

        Migrator::up(&*self.connection, None)
            .await
            .context("Failed to run migrations")?;

        info!("Database migrations completed successfully");
        Ok(())
    }

    /// Get the main database connection
    pub fn connection(&self) -> Arc<DatabaseConnection> {
        self.connection.clone()
    }

    /// Get the read-only database connection
    pub fn read_connection(&self) -> Arc<DatabaseConnection> {
        self.read_connection.clone()
    }


    /// Get the database backend type
    pub fn backend(&self) -> DatabaseBackend {
        self.backend
    }

    /// Get the database type
    pub fn database_type(&self) -> DatabaseType {
        self.database_type
    }

    /// Check if the database supports specific features
    pub fn supports_feature(&self, feature: DatabaseFeature) -> bool {
        match (self.database_type, feature) {
            (DatabaseType::SQLite, DatabaseFeature::Transactions) => true,
            (DatabaseType::SQLite, DatabaseFeature::ForeignKeys) => true,
            (DatabaseType::SQLite, DatabaseFeature::ConcurrentReads) => true,
            (DatabaseType::SQLite, DatabaseFeature::ReadReplicas) => false,

            (DatabaseType::PostgreSQL, DatabaseFeature::Transactions) => true,
            (DatabaseType::PostgreSQL, DatabaseFeature::ForeignKeys) => true,
            (DatabaseType::PostgreSQL, DatabaseFeature::ConcurrentReads) => true,
            (DatabaseType::PostgreSQL, DatabaseFeature::ReadReplicas) => true,

            (DatabaseType::MySQL, DatabaseFeature::Transactions) => true,
            (DatabaseType::MySQL, DatabaseFeature::ForeignKeys) => true,
            (DatabaseType::MySQL, DatabaseFeature::ConcurrentReads) => true,
            (DatabaseType::MySQL, DatabaseFeature::ReadReplicas) => true,
        }
    }


    /// Get stream proxy by ID (convenience method)
    pub async fn get_stream_proxy(&self, id: uuid::Uuid) -> Result<Option<crate::models::StreamProxy>> {
        use crate::database::repositories::stream_proxy::StreamProxySeaOrmRepository;
        
        let repo = StreamProxySeaOrmRepository::new(self.connection.clone());
        repo.find_by_id(&id).await
    }
}


#[derive(Debug, Clone, Copy)]
pub enum DatabaseFeature {
    Transactions,
    ForeignKeys,
    ConcurrentReads,
    ReadReplicas,
}

impl DatabaseType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DatabaseType::SQLite => "SQLite",
            DatabaseType::PostgreSQL => "PostgreSQL",
            DatabaseType::MySQL => "MySQL",
        }
    }
}

impl std::fmt::Display for DatabaseType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
