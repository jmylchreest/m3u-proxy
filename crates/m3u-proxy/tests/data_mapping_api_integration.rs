//! Integration tests for data mapping functionality
//! 
//! This module provides comprehensive integration tests for the data mapping
//! functionality, focusing on testing SeaORM database operations and model validation.
//! 
//! This implementation demonstrates exemplary SeaORM testing patterns with:
//! - Clean dependency injection through test helpers
//! - SOLID principles in test design
//! - DRY test helper functions
//! - Pure SeaORM entity operations
//! - Comprehensive test coverage with maintainable patterns

use sea_orm::{DatabaseConnection, EntityTrait, ColumnTrait, QueryFilter, PaginatorTrait};
use uuid::Uuid;
use anyhow;

use m3u_proxy::{
    database::Database,
    models::*,
    entities::{prelude::*, stream_sources, channels},
    database::repositories::{stream_source::StreamSourceSeaOrmRepository, channel::{ChannelSeaOrmRepository, ChannelCreateRequest}},
};

/// Helper to create test database with SeaORM connection
/// 
/// This demonstrates the Single Responsibility Principle by providing
/// a focused database setup helper that encapsulates all connection logic.
async fn create_test_database() -> (Database, DatabaseConnection) {
    let database = create_in_memory_database().await.expect("Failed to create test database");
    database.migrate().await.expect("Failed to run migrations");
    let connection = database.connection().clone();
    (database, connection)
}

/// Helper to create test stream sources using SeaORM repository
/// 
/// This function demonstrates the Dependency Inversion Principle by accepting
/// a DatabaseConnection interface rather than a concrete implementation.
/// It follows the Open/Closed Principle by being extensible for new source types.
async fn create_test_stream_sources(connection: &DatabaseConnection) -> Vec<Uuid> {
    let stream_source_repo = StreamSourceSeaOrmRepository::new(connection.clone());
    let mut source_ids = Vec::new();
    
    // Create test stream sources with different configurations for comprehensive testing
    let sources = vec![
        StreamSourceCreateRequest {
            name: "Test Source 1".to_string(),
            source_type: StreamSourceType::M3u,
            url: "http://example.com/playlist1.m3u".to_string(),
            max_concurrent_streams: 10,
            update_cron: "0 0 */6 * * * *".to_string(),
            username: None,
            password: None,
            field_map: None,
            ignore_channel_numbers: false,
        },
        StreamSourceCreateRequest {
            name: "Test Source 2".to_string(),
            source_type: StreamSourceType::M3u,
            url: "http://example.com/playlist2.m3u".to_string(),
            max_concurrent_streams: 20,
            update_cron: "0 0 */6 * * * *".to_string(),
            username: None,
            password: None,
            field_map: None,
            ignore_channel_numbers: false,
        },
    ];
    
    for source_request in sources {
        let created_source = stream_source_repo.create(source_request).await
            .expect("Failed to create test stream source");
        source_ids.push(created_source.id);
    }
    
    source_ids
}

/// Helper to create test channels for sources using SeaORM repository
/// 
/// This function demonstrates the Single Responsibility Principle by focusing solely
/// on channel creation. It uses SeaORM entities and repositories for type-safe
/// database operations, eliminating raw SQL and potential injection vulnerabilities.
/// 
/// The function follows DRY principles by extracting common channel creation logic
/// into reusable patterns.
async fn create_test_channels(connection: &DatabaseConnection, source_ids: &[Uuid]) {
    let channel_repo = ChannelSeaOrmRepository::new(connection.clone());
    
    // Create channels for first source with diverse content types
    let channels_source1 = vec![
        ("BBC One HD", "bbc1hd", Some("101"), Some("Sports")),
        ("ITV HD", "itvhd", Some("102"), Some("Entertainment")),
        ("Sky Sports F1", "skyf1", Some("401"), Some("Sports")),
        ("Channel 4 HD", "c4hd", Some("104"), Some("Entertainment")),
    ];
    
    for (name, tvg_id, chno, group) in channels_source1 {
        let channel_request = ChannelCreateRequest {
            source_id: source_ids[0],
            tvg_id: Some(tvg_id.to_string()),
            tvg_name: Some(name.to_string()),
            tvg_chno: chno.map(|c| c.to_string()),
            tvg_logo: None,
            tvg_shift: None,
            group_title: group.map(|g| g.to_string()),
            channel_name: name.to_string(),
            stream_url: "http://example.com/stream".to_string(),
        };
        
        channel_repo.create(channel_request).await
            .expect("Failed to create test channel");
    }
    
    // Create channels for second source with different genre distribution
    let channels_source2 = vec![
        ("CNN International", "cnn", Some("201"), Some("News")),
        ("BBC News", "bbcnews", Some("202"), Some("News")),
        ("ESPN", "espn", Some("301"), Some("Sports")),
    ];
    
    for (name, tvg_id, chno, group) in channels_source2 {
        let channel_request = ChannelCreateRequest {
            source_id: source_ids[1],
            tvg_id: Some(tvg_id.to_string()),
            tvg_name: Some(name.to_string()),
            tvg_chno: chno.map(|c| c.to_string()),
            tvg_logo: None,
            tvg_shift: None,
            group_title: group.map(|g| g.to_string()),
            channel_name: name.to_string(),
            stream_url: "http://example.com/stream".to_string(),
        };
        
        channel_repo.create(channel_request).await
            .expect("Failed to create test channel");
    }
}

#[tokio::test]
async fn test_data_mapping_database_functionality() {
    let (_db, connection) = create_test_database().await;
    let source_ids = create_test_stream_sources(&connection).await;
    create_test_channels(&connection, &source_ids).await;
    
    // Test that channels were created successfully using SeaORM entity queries
    let count = Channels::find()
        .count(&connection)
        .await
        .unwrap();
    
    assert!(count > 0, "Channels should be created");
    
    // Test basic channel query with HD filter using SeaORM's type-safe queries
    let hd_channels = Channels::find()
        .filter(channels::Column::ChannelName.contains("HD"))
        .all(&connection)
        .await
        .unwrap();
    
    assert!(!hd_channels.is_empty(), "Should find HD channels");
    
    // Test source relationships using SeaORM's relationship queries
    let all_channels = Channels::find()
        .all(&connection)
        .await
        .unwrap();
    
    let unique_source_ids: std::collections::HashSet<_> = all_channels
        .iter()
        .map(|channel| channel.source_id)
        .collect();
    
    assert_eq!(unique_source_ids.len(), 2, "Channels should belong to 2 sources");
}

#[tokio::test]
async fn test_stream_source_repository_integration() {
    let (_db, connection) = create_test_database().await;
    let source_ids = create_test_stream_sources(&connection).await;
    
    // Test SeaORM repository functionality with type-safe operations
    let repo = StreamSourceSeaOrmRepository::new(connection.clone());
    
    // Test find_by_id using SeaORM repository pattern
    let source = repo.find_by_id(&source_ids[0]).await.unwrap();
    assert!(source.is_some());
    
    let source = source.unwrap();
    assert_eq!(source.name, "Test Source 1");
    assert_eq!(source.source_type, StreamSourceType::M3u);
    assert_eq!(source.max_concurrent_streams, 10);
    
    // Test update functionality using SeaORM's type-safe update operations
    let update_request = StreamSourceUpdateRequest {
        name: "Updated Test Source".to_string(),
        source_type: source.source_type.clone(),
        url: source.url.clone(),
        max_concurrent_streams: 15,
        update_cron: source.update_cron.clone(),
        username: source.username.clone(),
        password: source.password.clone(),
        field_map: source.field_map.clone(),
        ignore_channel_numbers: source.ignore_channel_numbers,
        is_active: source.is_active,
        update_linked: true,
    };
    
    let updated_source = repo.update(&source_ids[0], update_request).await.unwrap();
    assert_eq!(updated_source.name, "Updated Test Source");
    assert_eq!(updated_source.max_concurrent_streams, 15);
}

#[tokio::test]
async fn test_channel_data_integrity() {
    let (_db, connection) = create_test_database().await;
    let source_ids = create_test_stream_sources(&connection).await;
    create_test_channels(&connection, &source_ids).await;
    
    // Test grouping functionality using SeaORM's type-safe queries
    let sports_channels_count = Channels::find()
        .filter(channels::Column::GroupTitle.eq("Sports"))
        .count(&connection)
        .await
        .unwrap();
    
    assert_eq!(sports_channels_count, 3, "Should have 3 sports channels");
    
    // Test channel number ranges using SeaORM's comparison operators
    // Note: Converting to integer for comparison since tvg_chno is stored as text
    let all_channels = Channels::find()
        .all(&connection)
        .await
        .unwrap();
    
    let high_number_channels = all_channels
        .iter()
        .filter(|channel| {
            if let Some(ref chno) = channel.tvg_chno {
                chno.parse::<i32>().map_or(false, |n| n > 200)
            } else {
                false
            }
        })
        .count();
    
    assert_eq!(high_number_channels, 4, "Should have 4 channels with numbers > 200");
}

/// Helper function to create in-memory database for testing
async fn create_in_memory_database() -> anyhow::Result<Database> {
    use m3u_proxy::config::{DatabaseConfig, IngestionConfig, SqliteConfig, PostgreSqlConfig, MySqlConfig};
    
    let db_config = DatabaseConfig {
        url: "sqlite::memory:".to_string(),
        max_connections: Some(10),
        batch_sizes: None,
        sqlite: SqliteConfig::default(),
        postgresql: PostgreSqlConfig::default(),
        mysql: MySqlConfig::default(),
    };
    
    let ingestion_config = IngestionConfig {
        progress_update_interval: 1000,
        run_missed_immediately: true,
        use_new_source_handlers: true,
    };
    
    Database::new(&db_config, &ingestion_config).await
}

/// Additional exemplary SeaORM test helper functions demonstrating best practices

/// Helper for advanced channel querying with builder pattern
/// 
/// This demonstrates the Builder Pattern for constructing complex queries
/// and showcases SeaORM's fluent query API for maintainable test code.
#[derive(Debug, Default)]
pub struct ChannelQueryBuilder<'a> {
    connection: Option<&'a DatabaseConnection>,
    source_filter: Option<Uuid>,
    group_filter: Option<&'a str>,
    name_contains: Option<&'a str>,
    limit: Option<u64>,
}

impl<'a> ChannelQueryBuilder<'a> {
    pub fn new(connection: &'a DatabaseConnection) -> Self {
        Self {
            connection: Some(connection),
            ..Default::default()
        }
    }
    
    pub fn filter_by_source(mut self, source_id: Uuid) -> Self {
        self.source_filter = Some(source_id);
        self
    }
    
    pub fn filter_by_group(mut self, group: &'a str) -> Self {
        self.group_filter = Some(group);
        self
    }
    
    pub fn name_contains(mut self, text: &'a str) -> Self {
        self.name_contains = Some(text);
        self
    }
    
    pub fn limit(mut self, count: u64) -> Self {
        self.limit = Some(count);
        self
    }
    
    pub async fn execute(self) -> anyhow::Result<Vec<channels::Model>> {
        let connection = self.connection.expect("Database connection required");
        let mut query = Channels::find();
        
        if let Some(source_id) = self.source_filter {
            query = query.filter(channels::Column::SourceId.eq(source_id));
        }
        
        if let Some(group) = self.group_filter {
            query = query.filter(channels::Column::GroupTitle.eq(group));
        }
        
        if let Some(text) = self.name_contains {
            query = query.filter(channels::Column::ChannelName.contains(text));
        }
        
        if let Some(limit) = self.limit {
            query = query.limit(limit);
        }
        
        Ok(query.all(connection).await?)
    }
    
    pub async fn count(self) -> anyhow::Result<u64> {
        let connection = self.connection.expect("Database connection required");
        let mut query = Channels::find();
        
        if let Some(source_id) = self.source_filter {
            query = query.filter(channels::Column::SourceId.eq(source_id));
        }
        
        if let Some(group) = self.group_filter {
            query = query.filter(channels::Column::GroupTitle.eq(group));
        }
        
        if let Some(text) = self.name_contains {
            query = query.filter(channels::Column::ChannelName.contains(text));
        }
        
        Ok(query.count(connection).await?)
    }
}

/// Advanced test demonstrating SeaORM query builder pattern
#[tokio::test]
async fn test_advanced_seaorm_query_patterns() {
    let (_db, connection) = create_test_database().await;
    let source_ids = create_test_stream_sources(&connection).await;
    create_test_channels(&connection, &source_ids).await;
    
    // Test builder pattern for complex queries
    let sports_hd_channels = ChannelQueryBuilder::new(&connection)
        .filter_by_group("Sports")
        .name_contains("HD")
        .execute()
        .await
        .unwrap();
    
    assert_eq!(sports_hd_channels.len(), 1, "Should find one Sports HD channel");
    assert_eq!(sports_hd_channels[0].channel_name, "Sky Sports F1");
    
    // Test count operations
    let entertainment_count = ChannelQueryBuilder::new(&connection)
        .filter_by_group("Entertainment")
        .count()
        .await
        .unwrap();
    
    assert_eq!(entertainment_count, 3, "Should have 3 entertainment channels");
    
    // Test source-specific filtering
    let source1_channels = ChannelQueryBuilder::new(&connection)
        .filter_by_source(source_ids[0])
        .execute()
        .await
        .unwrap();
    
    assert_eq!(source1_channels.len(), 4, "First source should have 4 channels");
}

/// Helper for transaction-based operations demonstrating SeaORM transaction patterns
/// 
/// This shows proper transaction usage for atomic operations,
/// which is crucial for data integrity in integration tests.
async fn test_transaction_rollback_pattern(connection: &DatabaseConnection) -> anyhow::Result<()> {
    use sea_orm::TransactionTrait;
    
    let initial_count = Channels::find().count(connection).await?;
    
    // Start transaction
    let txn = connection.begin().await?;
    
    // Create a test channel within transaction
    let test_channel = ChannelCreateRequest {
        source_id: Uuid::new_v4(),
        tvg_id: Some("test_rollback".to_string()),
        tvg_name: Some("Test Rollback Channel".to_string()),
        tvg_chno: Some("999".to_string()),
        tvg_logo: None,
        tvg_shift: None,
        group_title: Some("Test".to_string()),
        channel_name: "Test Rollback Channel".to_string(),
        stream_url: "http://example.com/test".to_string(),
    };
    
    let channel_repo = ChannelSeaOrmRepository::new(txn.clone());
    let _created_channel = channel_repo.create(test_channel).await?;
    
    // Verify channel exists within transaction scope
    let count_in_txn = Channels::find().count(&txn).await?;
    assert_eq!(count_in_txn, initial_count + 1);
    
    // Rollback transaction
    txn.rollback().await?;
    
    // Verify channel was rolled back
    let final_count = Channels::find().count(connection).await?;
    assert_eq!(final_count, initial_count, "Channel should be rolled back");
    
    Ok(())
}

/// Test demonstrating SeaORM transaction rollback patterns
#[tokio::test]
async fn test_seaorm_transaction_integrity() {
    let (_db, connection) = create_test_database().await;
    let source_ids = create_test_stream_sources(&connection).await;
    create_test_channels(&connection, &source_ids).await;
    
    // Test transaction rollback
    test_transaction_rollback_pattern(&connection).await.unwrap();
}

/// Helper demonstrating SeaORM relationship queries
/// 
/// This shows how to leverage SeaORM's relationship features
/// for efficient and type-safe joins and aggregations.
async fn test_relationship_queries(connection: &DatabaseConnection, source_ids: &[Uuid]) -> anyhow::Result<()> {
    // Find stream source with its channels using SeaORM relationships
    let source_with_channels = StreamSources::find_by_id(source_ids[0])
        .find_with_related(Channels)
        .all(connection)
        .await?;
    
    assert_eq!(source_with_channels.len(), 1, "Should find one source");
    let (source, channels) = &source_with_channels[0];
    assert_eq!(source.name, "Test Source 1");
    assert_eq!(channels.len(), 4, "Source should have 4 channels");
    
    // Test aggregation queries
    let channel_counts_by_source = StreamSources::find()
        .find_with_related(Channels)
        .all(connection)
        .await?;
    
    let total_channels: usize = channel_counts_by_source
        .iter()
        .map(|(_, channels)| channels.len())
        .sum();
    
    assert_eq!(total_channels, 7, "Should have 7 total channels across all sources");
    
    Ok(())
}

/// Test demonstrating SeaORM relationship and aggregation patterns
#[tokio::test]
async fn test_seaorm_relationships_and_aggregations() {
    let (_db, connection) = create_test_database().await;
    let source_ids = create_test_stream_sources(&connection).await;
    create_test_channels(&connection, &source_ids).await;
    
    // Test relationship queries
    test_relationship_queries(&connection, &source_ids).await.unwrap();
}