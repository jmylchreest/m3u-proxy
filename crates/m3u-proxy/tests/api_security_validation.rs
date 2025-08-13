//! API Security and Validation Testing
//!
//! This module provides comprehensive security testing focusing on database
//! operations, input validation, and protection against common vulnerabilities.
//!
//! Security areas covered:
//! - Input validation and sanitization
//! - SQL injection prevention (via parameterized queries)
//! - Database error handling security
//! - Data integrity validation
//! - Malicious input handling


use sqlx::{Pool, Sqlite, Row};
use uuid::Uuid;

use m3u_proxy::{
    config::{DatabaseConfig, IngestionConfig},
    database::Database,
    models::*,
    repositories::{traits::Repository, StreamSourceRepository},
};

/// Create in-memory database for testing
async fn create_in_memory_database() -> anyhow::Result<Database> {
    let db_config = DatabaseConfig {
        url: "sqlite::memory:".to_string(),
        max_connections: Some(10),
        batch_sizes: None,
        busy_timeout: "5000".to_string(),
        cache_size: "-64000".to_string(),
        wal_autocheckpoint: 1000,
    };
    
    let ingestion_config = IngestionConfig::default();
    
    Database::new(&db_config, &ingestion_config).await
}

/// Helper to create test database
async fn create_test_database() -> anyhow::Result<(Database, Pool<Sqlite>)> {
    let database = create_in_memory_database().await?;
    database.migrate().await?;
    let pool = database.pool();
    Ok((database, pool))
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
    "🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀", // Unicode DoS
];

// =============================================================================
// INPUT VALIDATION TESTS
// =============================================================================

#[tokio::test]
async fn test_repository_input_validation_stream_sources() {
    let (_db, pool) = create_test_database().await.unwrap();
    let repo = StreamSourceRepository::new(pool);

    for &payload in MALICIOUS_PAYLOADS {
        // Test stream source creation with malicious input
        let create_request = StreamSourceCreateRequest {
            name: payload.to_string(),
            source_type: StreamSourceType::M3u,
            url: format!("http://example.com/{}", payload.replace("'", "_")),
            max_concurrent_streams: 10,
            update_cron: "0 */6 * * *".to_string(),
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
                
                // Verify we can retrieve it safely
                let retrieved = repo.find_by_id(source.id).await.unwrap();
                assert!(retrieved.is_some());
                assert_eq!(retrieved.unwrap().name, payload);
                
                // Clean up
                let _ = repo.delete(source.id).await;
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
async fn test_parameterized_query_safety() {
    let (_db, pool) = create_test_database().await.unwrap();

    // Test direct SQL queries with malicious input - should be safely parameterized
    for &payload in MALICIOUS_PAYLOADS {
        // Test basic SELECT with parameterized query
        let result = sqlx::query("SELECT COUNT(*) as count FROM stream_sources WHERE name = ?")
            .bind(payload)
            .fetch_one(&pool)
            .await;

        match result {
            Ok(_row) => {
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

        // Test INSERT with parameterized query
        let test_id = Uuid::new_v4();
        let insert_result = sqlx::query(
            "INSERT INTO stream_sources (id, name, source_type, url, max_concurrent_streams, update_cron, created_at, updated_at, is_active) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(test_id.to_string())
        .bind(payload) // Malicious input as parameter
        .bind("m3u")
        .bind("http://example.com/playlist.m3u")
        .bind(10)
        .bind("0 */6 * * *")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(true)
        .execute(&pool)
        .await;

        match insert_result {
            Ok(_) => {
                // If successful, verify the malicious input was stored safely
                let verify_result = sqlx::query("SELECT name FROM stream_sources WHERE id = ?")
                    .bind(test_id.to_string())
                    .fetch_optional(&pool)
                    .await
                    .unwrap();
                
                if let Some(row) = verify_result {
                    let stored_name: String = row.try_get("name").unwrap();
                    assert_eq!(stored_name, payload); // Should be stored exactly as provided
                }
                
                // Clean up
                let _ = sqlx::query("DELETE FROM stream_sources WHERE id = ?")
                    .bind(test_id.to_string())
                    .execute(&pool)
                    .await;
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
async fn test_like_queries_sql_injection_prevention() {
    let (_db, pool) = create_test_database().await.unwrap();

    // Create test data
    for i in 1..=5 {
        let test_id = Uuid::new_v4();
        let _ = sqlx::query(
            "INSERT INTO stream_sources (id, name, source_type, url, max_concurrent_streams, update_cron, created_at, updated_at, is_active) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(test_id.to_string())
        .bind(format!("Test Source {i}"))
        .bind("m3u")
        .bind("http://example.com/playlist.m3u")
        .bind(10)
        .bind("0 */6 * * *")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(true)
        .execute(&pool)
        .await;
    }

    // Test LIKE queries with malicious input
    for &payload in MALICIOUS_PAYLOADS {
        let like_patterns = [
            format!("%{payload}%"),
            format!("{payload}%"),
            format!("%{payload}"),
        ];

        for pattern in &like_patterns {
            let result = sqlx::query("SELECT COUNT(*) as count FROM stream_sources WHERE name LIKE ?")
                .bind(pattern)
                .fetch_one(&pool)
                .await;

            match result {
                Ok(_row) => {
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
async fn test_database_error_handling_security() {
    let (_db, pool) = create_test_database().await.unwrap();

    // Test with intentionally malformed query parameters to check error handling
    let result = sqlx::query("SELECT COUNT(*) FROM stream_sources WHERE id = ?")
        .bind("not-a-valid-uuid-format")
        .fetch_one(&pool)
        .await;

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
async fn test_unicode_and_encoding_handling() {
    let (_db, pool) = create_test_database().await.unwrap();
    let repo = StreamSourceRepository::new(pool);

    // Test with various Unicode and encoding scenarios
    let unicode_test_cases = &[
        ("Basic ASCII", "Test Channel"),
        ("UTF-8 Unicode", "Test 频道 🚀 Канал"),
        ("Emoji Heavy", "🎬📺🔊🎭🎪🎨🎯"),
        ("Mixed Scripts", "العربية 中文 Русский Ελληνικά"),
        ("Zero Width", "Test\u{200B}Channel"), // Zero-width space
    ];

    for &(test_name, channel_name) in unicode_test_cases {
        let create_request = StreamSourceCreateRequest {
            name: channel_name.to_string(),
            source_type: StreamSourceType::M3u,
            url: "http://example.com/unicode.m3u".to_string(),
            max_concurrent_streams: 10,
            update_cron: "0 */6 * * *".to_string(),
            username: None,
            password: None,
            field_map: None,
            ignore_channel_numbers: false,
        };

        let result = repo.create(create_request).await;
        
        match result {
            Ok(source) => {
                // Verify Unicode is preserved correctly
                assert_eq!(source.name, channel_name, 
                    "Unicode should be preserved for test: {test_name}");
                
                // Clean up
                let _ = repo.delete(source.id).await;
            }
            Err(_) => {
                // Some Unicode might be rejected by validation, which is acceptable
            }
        }
    }
}

