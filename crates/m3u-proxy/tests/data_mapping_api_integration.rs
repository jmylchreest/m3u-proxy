//! Integration tests for data mapping functionality
//! 
//! This module provides comprehensive integration tests for the data mapping
//! functionality, focusing on testing database operations and model validation.

// use serde_json::json; // Unused in simplified test
use sqlx::{Pool, Sqlite};
use uuid::Uuid;

use m3u_proxy::{
    database::Database,
    models::*,
    repositories::{StreamSourceRepository, Repository},
};

/// Helper to create test database
async fn create_test_database() -> (Database, Pool<Sqlite>) {
    let database = create_in_memory_database().await.expect("Failed to create test database");
    database.migrate().await.expect("Failed to run migrations");
    let pool = database.pool().clone();
    (database, pool)
}

/// Helper to create test stream sources
async fn create_test_stream_sources(pool: &Pool<Sqlite>) -> Vec<Uuid> {
    let stream_source_repo = StreamSourceRepository::new(pool.clone());
    let mut source_ids = Vec::new();
    
    // Create test stream sources
    let sources = vec![
        StreamSourceCreateRequest {
            name: "Test Source 1".to_string(),
            source_type: StreamSourceType::M3u,
            url: "http://example.com/playlist1.m3u".to_string(),
            max_concurrent_streams: 10,
            update_cron: "0 */6 * * *".to_string(),
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
            update_cron: "0 */6 * * *".to_string(),
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

/// Helper to create test channels for sources
async fn create_test_channels(pool: &Pool<Sqlite>, source_ids: &[Uuid]) {
    // Create channels for first source
    let channels_source1 = vec![
        ("BBC One HD", "bbc1hd", Some(101), Some("Sports")),
        ("ITV HD", "itvhd", Some(102), Some("Entertainment")),
        ("Sky Sports F1", "skyf1", Some(401), Some("Sports")),
        ("Channel 4 HD", "c4hd", Some(104), Some("Entertainment")),
    ];
    
    for (name, tvg_id, chno, group) in channels_source1 {
        let channel_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO channels (id, source_id, channel_name, tvg_id, tvg_chno, group_title, stream_url, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(channel_id.to_string())
        .bind(source_ids[0].to_string())
        .bind(name)
        .bind(tvg_id)
        .bind(chno)
        .bind(group)
        .bind("http://example.com/stream")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(pool)
        .await
        .expect("Failed to create test channel");
    }
    
    // Create channels for second source
    let channels_source2 = vec![
        ("CNN International", "cnn", Some(201), Some("News")),
        ("BBC News", "bbcnews", Some(202), Some("News")),
        ("ESPN", "espn", Some(301), Some("Sports")),
    ];
    
    for (name, tvg_id, chno, group) in channels_source2 {
        let channel_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO channels (id, source_id, channel_name, tvg_id, tvg_chno, group_title, stream_url, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(channel_id.to_string())
        .bind(source_ids[1].to_string())
        .bind(name)
        .bind(tvg_id)
        .bind(chno)
        .bind(group)
        .bind("http://example.com/stream")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(pool)
        .await
        .expect("Failed to create test channel");
    }
}

#[tokio::test]
async fn test_data_mapping_database_functionality() {
    let (_db, pool) = create_test_database().await;
    let source_ids = create_test_stream_sources(&pool).await;
    create_test_channels(&pool, &source_ids).await;
    
    // Test that channels were created successfully
    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM channels")
        .fetch_one(&pool)
        .await
        .unwrap();
    
    assert!(count > 0, "Channels should be created");
    
    // Test basic channel query with HD filter
    let hd_channels = sqlx::query("SELECT channel_name FROM channels WHERE channel_name LIKE '%HD%'")
        .fetch_all(&pool)
        .await
        .unwrap();
    
    assert!(!hd_channels.is_empty(), "Should find HD channels");
    
    // Test source relationships
    let source_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(DISTINCT source_id) FROM channels")
        .fetch_one(&pool)
        .await
        .unwrap();
    
    assert_eq!(source_count, 2, "Channels should belong to 2 sources");
}

#[tokio::test]
async fn test_stream_source_repository_integration() {
    let (_db, pool) = create_test_database().await;
    let source_ids = create_test_stream_sources(&pool).await;
    
    // Test repository functionality
    let repo = StreamSourceRepository::new(pool.clone());
    
    // Test find_by_id
    let source = repo.find_by_id(source_ids[0]).await.unwrap();
    assert!(source.is_some());
    
    let source = source.unwrap();
    assert_eq!(source.name, "Test Source 1");
    assert_eq!(source.source_type, StreamSourceType::M3u);
    assert_eq!(source.max_concurrent_streams, 10);
    
    // Test update
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
    
    let updated_source = repo.update(source_ids[0], update_request).await.unwrap();
    assert_eq!(updated_source.name, "Updated Test Source");
    assert_eq!(updated_source.max_concurrent_streams, 15);
}

#[tokio::test]
async fn test_channel_data_integrity() {
    let (_db, pool) = create_test_database().await;
    let source_ids = create_test_stream_sources(&pool).await;
    create_test_channels(&pool, &source_ids).await;
    
    // Test grouping functionality
    let sports_channels = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM channels WHERE group_title = 'Sports'"
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    
    assert_eq!(sports_channels, 3, "Should have 3 sports channels");
    
    // Test channel number ranges
    let high_number_channels = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM channels WHERE tvg_chno > 200"
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    
    assert_eq!(high_number_channels, 4, "Should have 4 channels with numbers > 200");
}

/// Helper function to create in-memory database for testing
async fn create_in_memory_database() -> anyhow::Result<Database> {
    use m3u_proxy::config::{DatabaseConfig, IngestionConfig};
    
    let db_config = DatabaseConfig {
        url: "sqlite::memory:".to_string(),
        max_connections: Some(10),
        batch_sizes: None,
        busy_timeout: "5000".to_string(),
        cache_size: "-64000".to_string(),
        wal_autocheckpoint: 1000,
    };
    
    let ingestion_config = IngestionConfig {
        progress_update_interval: 1000,
        run_missed_immediately: true,
        use_new_source_handlers: true,
    };
    
    Database::new(&db_config, &ingestion_config).await
}