# Multi-Database Support Implementation Plan

This document outlines the plan for adding MySQL and PostgreSQL support to the M3U Proxy application, which currently only supports SQLite.

## Current State Analysis

### Database Support Status
- **Current:** SQLite only (fully implemented)
- **Planned:** MySQL, PostgreSQL support
- **Framework:** SQLx (already supports all three databases)

### Current SQLite-Specific Implementation
The application is heavily optimized for SQLite with the following specific dependencies:

#### Database Pool Management
```rust
// Current SQLite-only implementation
pub struct Database {
    pool: Pool<Sqlite>,              // ← SQLite-specific
    channel_update_lock: Arc<Mutex<()>>,
    ingestion_config: IngestionConfig,
    batch_config: DatabaseBatchConfig,
}
```

#### Schema and Migrations
- Uses SQLite-specific syntax: `datetime('now')`, `TEXT` types
- PRAGMA statements for optimization: `PRAGMA busy_timeout`, `PRAGMA wal_checkpoint`
- SQLite-specific triggers and constraints

#### Batch Processing Optimization
- Optimized for SQLite's 32,766 variable limit
- EPG Channels: 3,600 records (32,400 variables)
- EPG Programs: 1,900 records (32,300 variables)

## Database Comparison

### Parameter/Variable Limits

| Database | Parameter Limit | Current Batch Impact | Potential Optimization |
|----------|----------------|----------------------|----------------------|
| **SQLite** | 32,766 variables | ✅ Fully optimized | Current implementation |
| **MySQL** | ~65,535 parameters | ✅ Safe (49% usage) | Could double batch sizes |
| **PostgreSQL** | 65,535 / 32,767* | ✅ Safe | Similar to current |

*PostgreSQL has different limits depending on driver (65,535 for native, 32,767 for JDBC)

### Performance Potential

#### Current Performance (SQLite optimized)
- **EPG Channels:** 100 → 3,600 records per batch (**36x improvement**)
- **EPG Programs:** 50 → 1,900 records per batch (**38x improvement**)

#### Potential MySQL Performance
- **EPG Channels:** Could increase to 7,200 records (**2x additional improvement**)
- **EPG Programs:** Could increase to 3,800 records (**2x additional improvement**)

## Required Changes

### 1. Database Abstraction Layer

#### Current Implementation
```rust
// SQLite-only
impl Database {
    pub async fn new(config: &DatabaseConfig, ingestion_config: &IngestionConfig) -> Result<Self> {
        let pool = SqlitePool::connect(&config.url).await?;
        // ...
    }
}
```

#### Required Multi-Database Implementation
```rust
#[derive(Debug, Clone)]
pub enum DatabaseType {
    Sqlite,
    MySQL,
    PostgreSQL,
}

pub enum DatabasePool {
    Sqlite(SqlitePool),
    MySQL(MySqlPool),
    PostgreSQL(PgPool),
}

pub struct Database {
    pool: DatabasePool,
    db_type: DatabaseType,
    channel_update_lock: Arc<Mutex<()>>,
    ingestion_config: IngestionConfig,
    batch_config: DatabaseBatchConfig,
}

impl Database {
    pub async fn new(config: &DatabaseConfig, ingestion_config: &IngestionConfig) -> Result<Self> {
        let db_type = DatabaseType::from_url(&config.url)?;
        let pool = match db_type {
            DatabaseType::Sqlite => {
                DatabasePool::Sqlite(SqlitePool::connect(&config.url).await?)
            },
            DatabaseType::MySQL => {
                DatabasePool::MySQL(MySqlPool::connect(&config.url).await?)
            },
            DatabaseType::PostgreSQL => {
                DatabasePool::PostgreSQL(PgPool::connect(&config.url).await?)
            },
        };
        // ...
    }
}
```

### 2. Schema Migration System

#### Current SQLite Schema
```sql
-- SQLite-specific syntax
CREATE TABLE stream_sources (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    is_active BOOLEAN NOT NULL DEFAULT TRUE
);
```

#### Required MySQL Schema
```sql
-- MySQL-compatible syntax
CREATE TABLE stream_sources (
    id VARCHAR(36) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    url TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT NOW(),
    updated_at DATETIME NOT NULL DEFAULT NOW() ON UPDATE NOW(),
    is_active TINYINT(1) NOT NULL DEFAULT 1
);
```

#### Required PostgreSQL Schema
```sql
-- PostgreSQL-compatible syntax
CREATE TABLE stream_sources (
    id UUID PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    url TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    is_active BOOLEAN NOT NULL DEFAULT TRUE
);
```

### 3. Database-Specific Operations

#### Create Database Operations Trait
```rust
#[async_trait]
pub trait DatabaseOperations {
    async fn get_current_timestamp(&self) -> Result<String>;
    async fn bulk_insert_channels(&self, channels: &[Channel]) -> Result<usize>;
    async fn bulk_insert_programs(&self, programs: &[Program]) -> Result<usize>;
    async fn optimize_database(&self) -> Result<()>;
    fn get_optimal_batch_sizes(&self) -> BatchConfig;
    async fn check_database_exists(&self, url: &str) -> Result<bool>;
    async fn create_database(&self, url: &str) -> Result<()>;
}
```

#### Implementation per Database
```rust
// SQLite implementation
impl DatabaseOperations for SqliteDatabase {
    async fn get_current_timestamp(&self) -> Result<String> {
        Ok("datetime('now')".to_string())
    }
    
    async fn optimize_database(&self) -> Result<()> {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&self.pool).await?;
        sqlx::query("PRAGMA optimize").execute(&self.pool).await?;
        Ok(())
    }
    
    fn get_optimal_batch_sizes(&self) -> BatchConfig {
        BatchConfig {
            epg_channels: 3600,
            epg_programs: 1900,
            stream_channels: 1000,
        }
    }
}

// MySQL implementation
impl DatabaseOperations for MySqlDatabase {
    async fn get_current_timestamp(&self) -> Result<String> {
        Ok("NOW()".to_string())
    }
    
    async fn optimize_database(&self) -> Result<()> {
        sqlx::query("OPTIMIZE TABLE channels").execute(&self.pool).await?;
        sqlx::query("OPTIMIZE TABLE epg_programs").execute(&self.pool).await?;
        Ok(())
    }
    
    fn get_optimal_batch_sizes(&self) -> BatchConfig {
        BatchConfig {
            epg_channels: 7200,  // 2x larger for MySQL
            epg_programs: 3800,  // 2x larger for MySQL
            stream_channels: 2000,
        }
    }
}
```

### 4. Configuration Updates

#### Enhanced Database Configuration
```toml
[database]
# Database type: sqlite, mysql, postgresql
type = "mysql" 
url = "mysql://user:password@localhost/m3u_proxy"
max_connections = 10

# Database-specific options
[database.mysql]
ssl_mode = "preferred"
charset = "utf8mb4"
timezone = "+00:00"
max_allowed_packet = 67108864  # 64MB

[database.postgresql]
ssl_mode = "prefer"
schema = "public"
application_name = "m3u-proxy"

[database.sqlite]
busy_timeout = 30000
wal_mode = true

# Database-specific batch sizes
[database.batch_sizes]
# SQLite optimized
sqlite_epg_channels = 3600
sqlite_epg_programs = 1900
sqlite_stream_channels = 1000

# MySQL optimized (higher limits)
mysql_epg_channels = 7200
mysql_epg_programs = 3800
mysql_stream_channels = 2000

# PostgreSQL optimized
postgresql_epg_channels = 3600
postgresql_epg_programs = 1900
postgresql_stream_channels = 1000
```

### 5. Migration System Overhaul

#### Directory Structure
```
migrations/
├── sqlite/
│   ├── 001_initial_schema.sql
│   ├── 002_defaults.sql
│   └── ...
├── mysql/
│   ├── 001_initial_schema.sql
│   ├── 002_defaults.sql
│   └── ...
└── postgresql/
    ├── 001_initial_schema.sql
    ├── 002_defaults.sql
    └── ...
```

#### Migration Runner
```rust
impl Database {
    async fn run_database_migrations(&self) -> Result<()> {
        match &self.db_type {
            DatabaseType::Sqlite => {
                self.run_migrations_from_dir("migrations/sqlite").await
            },
            DatabaseType::MySQL => {
                self.run_migrations_from_dir("migrations/mysql").await
            },
            DatabaseType::PostgreSQL => {
                self.run_migrations_from_dir("migrations/postgresql").await
            },
        }
    }
}
```

## Implementation Phases

### Phase 1: Database Abstraction (2-3 days)
**Objective:** Create database-agnostic foundation

#### Tasks:
1. ✅ Create `DatabaseType` enum and detection logic
2. ✅ Implement `DatabasePool` enum wrapper
3. ✅ Update `Database` struct to use abstracted pool
4. ✅ Create `DatabaseOperations` trait
5. ✅ Update configuration system for database-specific settings

#### Deliverables:
- Database type detection from connection URL
- Abstracted pool management
- Configuration support for multiple databases

### Phase 2: Schema Migration (3-4 days)
**Objective:** Create database-specific schemas

#### Tasks:
1. 🔄 Analyze current SQLite schema for compatibility issues
2. 🔄 Create MySQL-compatible schema files
3. 🔄 Create PostgreSQL-compatible schema files
4. 🔄 Implement database-specific migration runner
5. 🔄 Update date/time handling throughout codebase

#### Deliverables:
- Complete schema files for all three databases
- Database-specific migration system
- Compatibility layer for date/time operations

### Phase 3: Query Adaptation (2-3 days)
**Objective:** Replace database-specific SQL

#### Tasks:
1. 🔄 Replace PRAGMA statements with database-specific equivalents
2. 🔄 Update bulk insert logic for database-specific optimizations
3. 🔄 Implement database-specific query builders where needed
4. 🔄 Update introspection queries (table info, etc.)

#### Deliverables:
- Database-agnostic query layer
- Optimized bulk insert implementations
- Database-specific optimization routines

### Phase 4: Testing & Optimization (2-3 days)
**Objective:** Ensure reliability and performance

#### Tasks:
1. 🔄 Create database-specific test suites
2. 🔄 Implement integration tests for all databases
3. 🔄 Performance testing and batch size optimization
4. 🔄 Create Docker Compose for multi-database testing
5. 🔄 Documentation and examples

#### Deliverables:
- Comprehensive test coverage
- Performance benchmarks
- Multi-database Docker setup
- Migration guides and documentation

## Testing Strategy

### Unit Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_sqlite_operations() {
        let db = Database::new_sqlite(":memory:").await.unwrap();
        // Test SQLite-specific functionality
    }
    
    #[tokio::test]
    async fn test_mysql_operations() {
        let db = Database::new_mysql("mysql://root@localhost/test").await.unwrap();
        // Test MySQL-specific functionality
    }
    
    #[tokio::test]
    async fn test_postgresql_operations() {
        let db = Database::new_postgresql("postgresql://localhost/test").await.unwrap();
        // Test PostgreSQL-specific functionality
    }
}
```

### Integration Tests
```rust
#[tokio::test]
async fn test_cross_database_compatibility() {
    for db_type in &[DatabaseType::Sqlite, DatabaseType::MySQL, DatabaseType::PostgreSQL] {
        let db = create_test_database(db_type).await;
        
        // Test that all core operations work consistently
        test_channel_operations(&db).await;
        test_epg_operations(&db).await;
        test_batch_processing(&db).await;
    }
}
```

### Docker Compose for Testing
```yaml
services:
  mysql:
    image: mysql:8.0
    environment:
      MYSQL_ROOT_PASSWORD: test
      MYSQL_DATABASE: m3u_proxy_test
    ports:
      - "3306:3306"
      
  postgresql:
    image: postgres:15
    environment:
      POSTGRES_PASSWORD: test
      POSTGRES_DB: m3u_proxy_test
    ports:
      - "5432:5432"
      
  m3u-proxy:
    build: .
    depends_on:
      - mysql
      - postgresql
    environment:
      - DATABASE_URL=mysql://root:test@mysql/m3u_proxy_test
```

## Performance Considerations

### Batch Size Optimization

#### Current SQLite Performance
- **EPG Channels:** 3,600 records/batch = 36x improvement over original
- **EPG Programs:** 1,900 records/batch = 38x improvement over original

#### Projected MySQL Performance
- **EPG Channels:** 7,200 records/batch = **72x improvement** over original
- **EPG Programs:** 3,800 records/batch = **76x improvement** over original

#### Performance Testing Metrics
```rust
struct PerformanceMetrics {
    database_type: DatabaseType,
    epg_channels_per_second: u32,
    epg_programs_per_second: u32,
    memory_usage_mb: u32,
    connection_pool_efficiency: f64,
}
```

### Memory Usage Optimization
- **SQLite:** Single-file, minimal memory overhead
- **MySQL:** Connection pooling, result set streaming
- **PostgreSQL:** Advanced query planning, prepared statement caching

## Risk Assessment

### High Risk Items
1. **Data Migration:** Moving existing SQLite data to MySQL/PostgreSQL
2. **Performance Regression:** Ensuring new abstraction doesn't hurt performance
3. **Configuration Complexity:** Managing database-specific settings

### Mitigation Strategies
1. **Gradual Rollout:** Implement and test one database at a time
2. **Backward Compatibility:** Maintain full SQLite support throughout
3. **Comprehensive Testing:** Extensive automated testing before release
4. **Documentation:** Clear migration guides and examples

### Rollback Plan
- Maintain SQLite as primary/fallback option
- Feature flags for database selection
- Database-specific Docker images for easy switching

## Success Criteria

### Functional Requirements
- [ ] All three databases (SQLite, MySQL, PostgreSQL) fully supported
- [ ] Identical functionality across all database types
- [ ] Smooth migration path from SQLite to other databases
- [ ] Database-specific optimizations (batch sizes, queries)

### Performance Requirements
- [ ] No performance regression for SQLite (current performance maintained)
- [ ] Improved performance for MySQL (2x batch size increase)
- [ ] Comparable performance for PostgreSQL
- [ ] Memory usage within 10% of current SQLite implementation

### Operational Requirements
- [ ] Simple configuration switching between databases
- [ ] Automated migration tools
- [ ] Comprehensive monitoring and logging
- [ ] Docker support for all database types

## Future Considerations

### Additional Database Support
- **Redis:** For caching and session storage
- **ClickHouse:** For analytics and time-series data
- **CockroachDB:** For distributed deployments

### Cloud Database Support
- **AWS RDS:** MySQL, PostgreSQL
- **Google Cloud SQL:** MySQL, PostgreSQL
- **Azure Database:** MySQL, PostgreSQL
- **PlanetScale:** MySQL-compatible serverless

### Performance Monitoring
- Database-specific metrics collection
- Query performance analysis
- Automated batch size optimization
- Connection pool monitoring

---

## Estimated Timeline

| Phase | Duration | Dependencies | Key Deliverables |
|-------|----------|-------------|------------------|
| **Phase 1** | 2-3 days | None | Database abstraction layer |
| **Phase 2** | 3-4 days | Phase 1 | Multi-database schemas |
| **Phase 3** | 2-3 days | Phase 2 | Database-agnostic queries |
| **Phase 4** | 2-3 days | Phase 3 | Testing & optimization |
| **Total** | **9-13 days** | | Full multi-database support |

**Note:** Timeline assumes one developer working full-time with good familiarity with the codebase and database systems.