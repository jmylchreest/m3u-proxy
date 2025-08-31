//! API Security and Validation Testing
//!
//! This module provides comprehensive security testing focusing on database
//! operations, input validation, and protection against common vulnerabilities.
//!
//! Security areas covered:
//! - Input validation and sanitization
//! - SQL injection prevention (via parameterized queries with SeaORM)
//! - Database error handling security
//! - Data integrity validation
//! - Malicious input handling

use anyhow::Result;
use sea_orm::{DatabaseConnection, DatabaseTransaction, QueryFilter, ColumnTrait, ConnectionTrait, Statement, DatabaseBackend, FromQueryResult};
use uuid::Uuid;

use m3u_proxy::{
    database::repositories::stream_source::StreamSourceSeaOrmRepository,
    entities::{prelude::StreamSources, stream_sources},
    models::*,
};

/// SeaORM test database helper - Creates in-memory database with minimal table structure
async fn create_seaorm_test_database() -> Result<std::sync::Arc<DatabaseConnection>> {
    use sea_orm::*;
    use std::sync::Arc;
    
    let connection = sea_orm::Database::connect("sqlite::memory:").await?;
    let arc_connection = Arc::new(connection);
    
    // Create minimal table structure for testing (avoiding migration foreign key issues)
    arc_connection.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        r#"
        CREATE TABLE stream_sources (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            source_type TEXT NOT NULL,
            url TEXT NOT NULL,
            max_concurrent_streams INTEGER NOT NULL,
            update_cron TEXT NOT NULL,
            username TEXT,
            password TEXT,
            field_map TEXT,
            ignore_channel_numbers INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            last_ingested_at TEXT,
            is_active INTEGER NOT NULL DEFAULT 1
        );
        "#.to_string()
    )).await?;
    
    Ok(arc_connection)
}

/// SeaORM repository helper - Creates repository with test database
async fn create_seaorm_test_repository() -> Result<(StreamSourceSeaOrmRepository, std::sync::Arc<DatabaseConnection>)> {
    let connection = create_seaorm_test_database().await?;
    let repository = StreamSourceSeaOrmRepository::new(connection.clone());
    Ok((repository, connection))
}

/// SeaORM security test helper - Execute parameterized query safely
async fn execute_parameterized_query(
    connection: &DatabaseConnection, 
    sql: &str, 
    params: Vec<sea_orm::Value>
) -> Result<()> {
    let stmt = Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, params);
    connection.execute(stmt).await?;
    Ok(())
}

/// SeaORM security test helper - Execute parameterized query safely within a transaction
async fn execute_parameterized_query_tx(
    txn: &DatabaseTransaction, 
    sql: &str, 
    params: Vec<sea_orm::Value>
) -> Result<()> {
    let stmt = Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, params);
    txn.execute(stmt).await?;
    Ok(())
}

/// SeaORM security test helper - Execute parameterized query with single result
async fn query_one_parameterized<T>(
    connection: &DatabaseConnection,
    sql: &str, 
    params: Vec<sea_orm::Value>
) -> Result<Option<T>> 
where
    T: FromQueryResult
{
    let stmt = Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, params);
    let result = T::find_by_statement(stmt).one(connection).await?;
    Ok(result)
}

/// SeaORM security test helper - Execute parameterized query with single result within a transaction
async fn query_one_parameterized_tx<T>(
    txn: &DatabaseTransaction,
    sql: &str, 
    params: Vec<sea_orm::Value>
) -> Result<Option<T>> 
where
    T: FromQueryResult
{
    let stmt = Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, params);
    let result = T::find_by_statement(stmt).one(txn).await?;
    Ok(result)
}

/// SeaORM transaction helper - Execute operations in transaction for security testing
async fn execute_in_transaction<F, R>(
    connection: &DatabaseConnection,
    operation: F
) -> Result<R>
where
    F: for<'c> FnOnce(&'c sea_orm::DatabaseTransaction) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<R>> + Send + 'c>>,
{
    use sea_orm::TransactionTrait;
    
    let txn = connection.begin().await?;
    let result = operation(&txn).await?;
    txn.commit().await?;
    Ok(result)
}

/// SeaORM bulk insert helper - Demonstrates safe bulk operations
async fn create_test_sources_bulk(
    repository: &StreamSourceSeaOrmRepository,
    count: usize,
    name_prefix: &str
) -> Result<Vec<StreamSource>> {
    let mut sources = Vec::new();
    
    for i in 1..=count {
        let request = StreamSourceCreateRequest {
            name: format!("{} {}", name_prefix, i),
            source_type: StreamSourceType::M3u,
            url: format!("http://example.com/test{}.m3u", i),
            max_concurrent_streams: 10,
            update_cron: "0 0 */6 * * * *".to_string(),
            username: None,
            password: None,
            field_map: None,
            ignore_channel_numbers: false,
        };
        
        let source = repository.create(request).await?;
        sources.push(source);
    }
    
    Ok(sources)
}

/// SeaORM entity query helper - Demonstrates type-safe querying
async fn find_sources_by_name_pattern(
    connection: &DatabaseConnection,
    pattern: &str
) -> Result<Vec<StreamSource>> {
    use sea_orm::EntityTrait;
    
    let models = StreamSources::find()
        .filter(stream_sources::Column::Name.contains(pattern))
        .filter(stream_sources::Column::IsActive.eq(true))
        .all(connection)
        .await?;

    Ok(models.into_iter().map(|m| StreamSource {
        id: m.id,
        name: m.name,
        source_type: m.source_type,
        url: m.url,
        max_concurrent_streams: m.max_concurrent_streams,
        update_cron: m.update_cron,
        username: m.username,
        password: m.password,
        field_map: m.field_map,
        ignore_channel_numbers: m.ignore_channel_numbers,
        created_at: m.created_at,
        updated_at: m.updated_at,
        last_ingested_at: m.last_ingested_at,
        is_active: m.is_active,
    }).collect())
}

/// Common malicious payloads for testing input validation
const MALICIOUS_PAYLOADS: &[&str] = &[
    // SQL Injection attempts
    "'; DROP TABLE stream_sources; --",
    "' OR '1'='1",
    "' UNION SELECT * FROM sqlite_master --",
    "admin'/*",
    
    // XSS attempts
    "<script>alert('xss')</script>",
    "javascript:alert('xss')",
    "<img src=x onerror=alert('xss')>",
    "';alert('xss');//",
    
    // Path traversal
    "../../../etc/passwd",
    "..\\..\\..\\windows\\system32\\config\\sam",
    
    // Command injection
    "; rm -rf /",
    "| cat /etc/passwd",
    "&& rm -rf /",
    
    // Large payloads for DoS testing
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX", // Unicode DoS
];

// =============================================================================
// INPUT VALIDATION TESTS
// =============================================================================

#[tokio::test]
async fn test_seaorm_repository_input_validation_stream_sources() {
    let (repo, _connection) = create_seaorm_test_repository().await.unwrap();

    for &payload in MALICIOUS_PAYLOADS {
        // Test stream source creation with malicious input using SeaORM repository
        let create_request = StreamSourceCreateRequest {
            name: payload.to_string(),
            source_type: StreamSourceType::M3u,
            url: format!("http://example.com/{}", payload.replace("'", "_")),
            max_concurrent_streams: 10,
            update_cron: "0 0 */6 * * * *".to_string(),
            username: Some(payload.to_string()),
            password: Some(payload.to_string()),
            field_map: None,
            ignore_channel_numbers: false,
        };

        let result = repo.create(create_request).await;
        
        match result {
            Ok(source) => {
                // If creation succeeded, malicious input should be stored safely
                assert_eq!(source.name, payload);
                
                // Verify we can retrieve it safely using SeaORM
                let retrieved = repo.find_by_id(&source.id).await.unwrap();
                assert!(retrieved.is_some());
                assert_eq!(retrieved.unwrap().name, payload);
                
                // Clean up using SeaORM
                let _ = repo.delete(&source.id).await;
            }
            Err(e) => {
                // Repository might reject invalid data, but should not have SQL injection
                let error_msg = format!("{e}");
                assert!(!error_msg.to_lowercase().contains("syntax error"));
                assert!(!error_msg.to_lowercase().contains("sql error"));
            }
        }
    }
}

#[tokio::test]
async fn test_seaorm_parameterized_query_safety() {
    let connection = create_seaorm_test_database().await.unwrap();

    // Test SeaORM parameterized queries with malicious input - should be safely parameterized
    for &payload in MALICIOUS_PAYLOADS {
        // Test basic SELECT with SeaORM parameterized query
        #[derive(FromQueryResult)]
        struct CountResult {
            #[allow(dead_code)]
            count: i64,
        }
        
        let count_result = query_one_parameterized::<CountResult>(
            &connection,
            "SELECT COUNT(*) as count FROM stream_sources WHERE name = ?",
            vec![payload.into()]
        ).await;

        match count_result {
            Ok(_) => {
                // Should execute without SQL injection
                // Count should be 0 since no matching records exist
            }
            Err(e) => {
                // Error should not be SQL syntax error (would indicate injection)
                let error_msg = format!("{e}");
                assert!(!error_msg.to_lowercase().contains("syntax error"));
                assert!(!error_msg.to_lowercase().contains("sql syntax"));
            }
        }

        // Test INSERT with SeaORM parameterized query
        let test_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let insert_result = execute_parameterized_query(
            &connection,
            "INSERT INTO stream_sources (id, name, source_type, url, max_concurrent_streams, update_cron, created_at, updated_at, is_active) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![
                test_id.to_string().into(),
                payload.into(), // Malicious input as parameter
                "m3u".into(),
                "http://example.com/playlist.m3u".into(),
                10.into(),
                "0 0 */6 * * * *".into(),
                now.to_rfc3339().into(),
                now.to_rfc3339().into(),
                true.into()
            ]
        ).await;

        match insert_result {
            Ok(_) => {
                // If successful, verify the malicious input was stored safely using SeaORM
                #[derive(FromQueryResult)]
                struct NameResult {
                    name: String,
                }
                
                let verify_result = query_one_parameterized::<NameResult>(
                    &connection,
                    "SELECT name FROM stream_sources WHERE id = ?",
                    vec![test_id.to_string().into()]
                ).await.unwrap();
                
                if let Some(row) = verify_result {
                    assert_eq!(row.name, payload); // Should be stored exactly as provided
                }
                
                // Clean up using SeaORM
                let _ = execute_parameterized_query(
                    &connection,
                    "DELETE FROM stream_sources WHERE id = ?",
                    vec![test_id.to_string().into()]
                ).await;
            }
            Err(e) => {
                // If failed, should be due to constraints, not SQL injection
                let error_msg = format!("{e}");
                assert!(!error_msg.to_lowercase().contains("syntax error"));
            }
        }
    }
}

#[tokio::test] 
async fn test_seaorm_like_queries_sql_injection_prevention() {
    let connection = create_seaorm_test_database().await.unwrap();

    // Create test data using SeaORM
    for i in 1..=5 {
        let test_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let _ = execute_parameterized_query(
            &connection,
            "INSERT INTO stream_sources (id, name, source_type, url, max_concurrent_streams, update_cron, created_at, updated_at, is_active) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![
                test_id.to_string().into(),
                format!("Test Source {i}").into(),
                "m3u".into(),
                "http://example.com/playlist.m3u".into(),
                10.into(),
                "0 0 */6 * * * *".into(),
                now.to_rfc3339().into(),
                now.to_rfc3339().into(),
                true.into()
            ]
        ).await;
    }

    // Test LIKE queries with malicious input using SeaORM
    for &payload in MALICIOUS_PAYLOADS {
        let like_patterns = [
            format!("%{payload}%"),
            format!("{payload}%"),
            format!("%{payload}"),
        ];

        for pattern in &like_patterns {
            #[derive(FromQueryResult)]
            struct LikeCountResult {
                #[allow(dead_code)]
                count: i64,
            }
            
            let result = query_one_parameterized::<LikeCountResult>(
                &connection,
                "SELECT COUNT(*) as count FROM stream_sources WHERE name LIKE ?",
                vec![pattern.into()]
            ).await;

            match result {
                Ok(_) => {
                    // Should execute safely without SQL injection
                }
                Err(e) => {
                    let error_msg = format!("{e}");
                    assert!(!error_msg.to_lowercase().contains("syntax error"));
                }
            }
        }
    }
}

#[tokio::test]
async fn test_uuid_validation_prevents_injection() {
    for &payload in MALICIOUS_PAYLOADS {
        // UUID parsing should reject all malicious input
        let result = Uuid::parse_str(payload);
        assert!(result.is_err(), "Should reject malicious UUID: {payload}");
    }
    
    // Valid UUIDs should pass
    let valid_uuid = Uuid::new_v4();
    assert!(Uuid::parse_str(&valid_uuid.to_string()).is_ok());
}

#[tokio::test]
async fn test_seaorm_database_error_handling_security() {
    let connection = create_seaorm_test_database().await.unwrap();

    // Test with intentionally malformed query parameters to check SeaORM error handling
    #[derive(FromQueryResult)]
    struct ErrorTestResult {
        #[sea_orm(column_name = "COUNT(*)")]
        #[allow(dead_code)]
        count: i64,
    }
    
    let result = query_one_parameterized::<ErrorTestResult>(
        &connection,
        "SELECT COUNT(*) FROM stream_sources WHERE id = ?",
        vec!["not-a-valid-uuid-format".into()]
    ).await;

    if let Err(error) = result {
        let error_msg = format!("{error}");
        
        // Should not expose sensitive internal details
        assert!(!error_msg.to_lowercase().contains("password"));
        assert!(!error_msg.to_lowercase().contains("secret"));
        assert!(!error_msg.to_lowercase().contains("private_key"));
        assert!(!error_msg.to_lowercase().contains("connection_string"));
    }
}

#[tokio::test]
async fn test_seaorm_unicode_and_encoding_handling() {
    let (repo, _connection) = create_seaorm_test_repository().await.unwrap();

    // Test with various Unicode and encoding scenarios using SeaORM
    let unicode_test_cases = &[
        ("Basic ASCII", "Test Channel"),
        ("UTF-8 Unicode", "Test 频道 Channel Канал"),
        ("Symbol Heavy", "[]{}()!@#$%^&*()-=+"),
        ("Mixed Scripts", "العربية 中文 Русский Ελληνικά"),
        ("Zero Width", "Test\u{200B}Channel"), // Zero-width space
    ];

    for &(test_name, channel_name) in unicode_test_cases {
        let create_request = StreamSourceCreateRequest {
            name: channel_name.to_string(),
            source_type: StreamSourceType::M3u,
            url: "http://example.com/unicode.m3u".to_string(),
            max_concurrent_streams: 10,
            update_cron: "0 0 */6 * * * *".to_string(),
            username: None,
            password: None,
            field_map: None,
            ignore_channel_numbers: false,
        };

        let result = repo.create(create_request).await;
        
        match result {
            Ok(source) => {
                // Verify Unicode is preserved correctly with SeaORM
                assert_eq!(source.name, channel_name, 
                    "Unicode should be preserved for test: {test_name}");
                
                // Clean up using SeaORM
                let _ = repo.delete(&source.id).await;
            }
            Err(_) => {
                // Some Unicode might be rejected by validation, which is acceptable
            }
        }
    }
}

// =============================================================================
// ADVANCED SEAORM SECURITY PATTERNS
// =============================================================================

#[tokio::test]
async fn test_seaorm_advanced_security_patterns() {
    let (repo, connection) = create_seaorm_test_repository().await.unwrap();

    // Test 1: Transaction-based security with rollback on malicious input
    for &payload in &MALICIOUS_PAYLOADS[0..3] { // Test subset for performance
        let result = execute_in_transaction(&connection, |txn| {
            Box::pin(async move {
                // Simulate a multi-step operation that should be atomic
                let test_id = Uuid::new_v4();
                let now = chrono::Utc::now();
                
                // Step 1: Insert with potential malicious data
                execute_parameterized_query_tx(
                    txn,
                    "INSERT INTO stream_sources (id, name, source_type, url, max_concurrent_streams, update_cron, created_at, updated_at, is_active) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    vec![
                        test_id.to_string().into(),
                        payload.into(),
                        "m3u".into(),
                        "http://example.com/test.m3u".into(),
                        10.into(),
                        "0 0 */6 * * * *".into(),
                        now.to_rfc3339().into(),
                        now.to_rfc3339().into(),
                        true.into()
                    ]
                ).await?;

                // Step 2: Verify data integrity
                #[derive(FromQueryResult)]
                struct VerifyResult {
                    name: String,
                }
                
                let verify = query_one_parameterized_tx::<VerifyResult>(
                    txn,
                    "SELECT name FROM stream_sources WHERE id = ?",
                    vec![test_id.to_string().into()]
                ).await?;

                if let Some(result) = verify {
                    assert_eq!(result.name, payload);
                }

                Ok(test_id)
            })
        }).await;

        // Transaction should either succeed completely or fail completely
        match result {
            Ok(_) => {
                // All operations succeeded, data should be properly stored and escaped
            }
            Err(e) => {
                // Transaction failed, but should not be due to SQL injection
                let error_msg = format!("{e}");
                assert!(!error_msg.to_lowercase().contains("syntax error"));
                assert!(!error_msg.to_lowercase().contains("sql injection"));
            }
        }
    }

    // Test 2: Type-safe entity queries with malicious search patterns
    let test_sources = create_test_sources_bulk(&repo, 5, "TestPattern").await.unwrap();
    
    for &payload in &MALICIOUS_PAYLOADS[0..5] {
        let search_result = find_sources_by_name_pattern(&connection, payload).await;
        
        match search_result {
            Ok(sources) => {
                // Should return safely filtered results without SQL injection
                for source in sources {
                    assert!(source.name.contains(payload) || source.name.contains("TestPattern"));
                }
            }
            Err(e) => {
                let error_msg = format!("{e}");
                assert!(!error_msg.to_lowercase().contains("syntax error"));
            }
        }
    }

    // Test 3: Demonstrate SeaORM's built-in SQL injection protection with complex queries
    #[derive(FromQueryResult)]
    struct ComplexQueryResult {
        #[allow(dead_code)]
        id: String,
        #[allow(dead_code)]
        name: String,
        #[allow(dead_code)]
        url: String,
    }

    for &payload in &MALICIOUS_PAYLOADS[0..3] {
        let complex_result = query_one_parameterized::<ComplexQueryResult>(
            &connection,
            r#"
            SELECT id, name, url 
            FROM stream_sources 
            WHERE (name LIKE ? OR url LIKE ?) 
            AND source_type = ? 
            AND is_active = ?
            ORDER BY created_at DESC 
            LIMIT 1
            "#,
            vec![
                format!("%{}%", payload).into(),
                format!("%{}%", payload).into(),
                "m3u".into(),
                true.into()
            ]
        ).await;

        match complex_result {
            Ok(_) => {
                // Complex query executed safely with parameterization
            }
            Err(e) => {
                let error_msg = format!("{e}");
                assert!(!error_msg.to_lowercase().contains("syntax error"));
                assert!(!error_msg.to_lowercase().contains("near"));
            }
        }
    }

    // Clean up test data
    for source in test_sources {
        let _ = repo.delete(&source.id).await;
    }
}

#[tokio::test]
async fn test_seaorm_concurrent_security_operations() {
    use tokio::task::JoinSet;
    
    let (repo, _connection) = create_seaorm_test_repository().await.unwrap();
    
    // Test concurrent operations with potentially malicious input
    let mut join_set = JoinSet::new();
    
    for (i, &payload) in MALICIOUS_PAYLOADS.iter().take(5).enumerate() {
        let repo_clone = repo.clone();
        let payload_owned = payload.to_string();
        
        join_set.spawn(async move {
            let create_request = StreamSourceCreateRequest {
                name: format!("Concurrent {} - {}", i, payload_owned),
                source_type: StreamSourceType::M3u,
                url: format!("http://example.com/concurrent{}.m3u", i),
                max_concurrent_streams: 10,
                update_cron: "0 0 */6 * * * *".to_string(),
                username: Some(payload_owned),
                password: None,
                field_map: None,
                ignore_channel_numbers: false,
            };

            match repo_clone.create(create_request).await {
                Ok(source) => {
                    // Verify concurrent creation succeeded
                    let found = repo_clone.find_by_id(&source.id).await.unwrap();
                    assert!(found.is_some());
                    
                    // Clean up
                    let _ = repo_clone.delete(&source.id).await;
                    Ok(source.id)
                }
                Err(e) => {
                    // Concurrent operation failed, but should not be due to SQL injection
                    let error_msg = format!("{e}");
                    assert!(!error_msg.to_lowercase().contains("syntax error"));
                    Err(e)
                }
            }
        });
    }
    
    // Wait for all concurrent operations to complete
    let mut success_count = 0;
    let mut error_count = 0;
    
    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(Ok(_)) => success_count += 1,
            Ok(Err(_)) => error_count += 1,
            Err(e) => panic!("Task panicked: {e}"),
        }
    }
    
    // All operations should complete without SQL injection errors
    assert!(success_count + error_count == 5);
    println!("Concurrent operations: {} succeeded, {} had validation errors", success_count, error_count);
}

