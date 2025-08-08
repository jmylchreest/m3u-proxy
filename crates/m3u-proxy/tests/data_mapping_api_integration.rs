//! Integration tests for data mapping API endpoints
//! 
//! This module provides comprehensive integration tests for the data mapping
//! preview and helper APIs, focusing on testing the actual HTTP endpoints
//! with realistic data scenarios.

use axum::{
    body::Body,
    http::{self, Request, StatusCode},
};
use axum_test::TestServer;
use serde_json::{json, Value};
use std::collections::HashMap;
use tower::ServiceExt;
use uuid::Uuid;

use m3u_proxy::{
    config::Config,
    database::Database,
    models::*,
    repositories::{StreamSourceRepository, traits::Repository},
    web::{api::apply_data_mapping_rules_post, state::AppState},
};

/// Helper to create test app state with in-memory database
async fn create_test_app_state() -> AppState {
    let config = Config::default();
    let database = Database::new_in_memory().await.expect("Failed to create test database");
    
    // Run migrations
    database.migrate().await.expect("Failed to run migrations");
    
    let pool = database.pool().clone();
    
    AppState::new(
        config,
        database,
        pool,
    ).await.expect("Failed to create app state")
}

/// Helper to create test stream sources
async fn create_test_stream_sources(app_state: &AppState) -> Vec<Uuid> {
    let stream_source_repo = StreamSourceRepository::new(app_state.pool.clone());
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
async fn create_test_channels(app_state: &AppState, source_ids: &[Uuid]) {
    let pool = &app_state.pool;
    
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
            "INSERT INTO channels (id, source_id, channel_name, tvg_id, tvg_chno, group_title, url) VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(channel_id.to_string())
        .bind(source_ids[0].to_string())
        .bind(name)
        .bind(tvg_id)
        .bind(chno)
        .bind(group)
        .bind("http://example.com/stream")
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
            "INSERT INTO channels (id, source_id, channel_name, tvg_id, tvg_chno, group_title, url) VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(channel_id.to_string())
        .bind(source_ids[1].to_string())
        .bind(name)
        .bind(tvg_id)
        .bind(chno)
        .bind(group)
        .bind("http://example.com/stream")
        .execute(pool)
        .await
        .expect("Failed to create test channel");
    }
}

#[tokio::test]
async fn test_data_mapping_preview_api_basic_functionality() {
    let app_state = create_test_app_state().await;
    let source_ids = create_test_stream_sources(&app_state).await;
    create_test_channels(&app_state, &source_ids).await;
    
    // Test basic data mapping preview request
    let request_payload = json!({
        "source_type": "stream",
        "source_ids": [source_ids[0]],
        "expression": "channel_name contains \"HD\" SET group_title = \"High Definition\"",
        "limit": 10
    });
    
    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/api/v1/data-mapping/preview")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&request_payload).unwrap()))
        .unwrap();
    
    // Create a simple test app with the handler
    let app = axum::Router::new()
        .route("/api/v1/data-mapping/preview", axum::routing::post(apply_data_mapping_rules_post))
        .with_state(app_state);
    
    let response = app.oneshot(request).await.unwrap();
    
    // Should return HTTP 200
    assert_eq!(response.status(), StatusCode::OK);
    
    // Parse response body
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response_json: Value = serde_json::from_slice(&body).unwrap();
    
    // Verify response structure
    assert_eq!(response_json["success"], true);
    assert_eq!(response_json["source_type"], "stream");
    assert!(response_json["total_channels"].as_u64().unwrap() > 0);
    assert!(response_json["affected_channels"].as_u64().unwrap() > 0);
    
    // Should have sample changes showing HD channels being updated
    let sample_changes = response_json["sample_changes"].as_array().unwrap();
    assert!(!sample_changes.is_empty());
    
    // Check that sample changes have the expected structure
    let first_change = &sample_changes[0];
    assert!(first_change["channel_name"].as_str().unwrap().contains("HD"));
    assert!(first_change["changes"].is_object());
    
    let changes = &first_change["changes"];
    if let Some(group_title_change) = changes.get("group_title") {
        assert_eq!(group_title_change["new_value"], "High Definition");
    }
}

#[tokio::test]
async fn test_data_mapping_preview_with_multiple_source_ids() {
    let app_state = create_test_app_state().await;
    let source_ids = create_test_stream_sources(&app_state).await;
    create_test_channels(&app_state, &source_ids).await;
    
    // Test with multiple source IDs
    let request_payload = json!({
        "source_type": "stream",
        "source_ids": source_ids,  // All source IDs
        "expression": "group_title equals \"Sports\" SET tvg_logo = \"@logo:sports-logo\"",
        "limit": 20
    });
    
    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/api/v1/data-mapping/preview")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&request_payload).unwrap()))
        .unwrap();
    
    let app = axum::Router::new()
        .route("/api/v1/data-mapping/preview", axum::routing::post(apply_data_mapping_rules_post))
        .with_state(app_state);
    
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response_json: Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(response_json["success"], true);
    
    // Should process channels from multiple sources
    let source_info = &response_json["source_info"];
    assert_eq!(source_info["source_count"], 2);
    
    let source_ids_in_response = source_info["source_ids"].as_array().unwrap();
    assert_eq!(source_ids_in_response.len(), 2);
    
    // Should have affected channels (Sports group exists in test data)
    assert!(response_json["affected_channels"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn test_data_mapping_preview_with_comparison_operators() {
    let app_state = create_test_app_state().await;
    let source_ids = create_test_stream_sources(&app_state).await;
    create_test_channels(&app_state, &source_ids).await;
    
    // Test with new comparison operators
    let request_payload = json!({
        "source_type": "stream",
        "source_ids": [source_ids[0]],
        "expression": "tvg_chno > \"200\" SET group_title = \"Premium Channels\"",
        "limit": 10
    });
    
    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/api/v1/data-mapping/preview")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&request_payload).unwrap()))
        .unwrap();
    
    let app = axum::Router::new()
        .route("/api/v1/data-mapping/preview", axum::routing::post(apply_data_mapping_rules_post))
        .with_state(app_state);
    
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response_json: Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(response_json["success"], true);
    
    // Test data has channels with tvg_chno > 200 (401), so should have matches
    let sample_changes = response_json["sample_changes"].as_array().unwrap();
    if !sample_changes.is_empty() {
        let first_change = &sample_changes[0];
        let channel_chno = first_change["tvg_chno"].as_u64().unwrap();
        assert!(channel_chno > 200, "Channel number should be > 200 for this test");
    }
}

#[tokio::test]
async fn test_data_mapping_preview_range_query() {
    let app_state = create_test_app_state().await;
    let source_ids = create_test_stream_sources(&app_state).await;
    create_test_channels(&app_state, &source_ids).await;
    
    // Test range query with comparison operators
    let request_payload = json!({
        "source_type": "stream",
        "source_ids": source_ids,
        "expression": "tvg_chno >= \"100\" AND tvg_chno <= \"200\" SET group_title = \"Standard Channels\"",
        "limit": 10
    });
    
    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/api/v1/data-mapping/preview")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&request_payload).unwrap()))
        .unwrap();
    
    let app = axum::Router::new()
        .route("/api/v1/data-mapping/preview", axum::routing::post(apply_data_mapping_rules_post))
        .with_state(app_state);
    
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response_json: Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(response_json["success"], true);
    
    // Should match channels in the 100-200 range (101, 102, 104, 201, 202 from test data)
    assert!(response_json["affected_channels"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn test_data_mapping_preview_empty_source_ids() {
    let app_state = create_test_app_state().await;
    let source_ids = create_test_stream_sources(&app_state).await;
    create_test_channels(&app_state, &source_ids).await;
    
    // Test with empty source_ids array (should process all sources)
    let request_payload = json!({
        "source_type": "stream",
        "source_ids": [],
        "expression": "channel_name contains \"BBC\" SET group_title = \"BBC Channels\"",
        "limit": 10
    });
    
    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/api/v1/data-mapping/preview")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&request_payload).unwrap()))
        .unwrap();
    
    let app = axum::Router::new()
        .route("/api/v1/data-mapping/preview", axum::routing::post(apply_data_mapping_rules_post))
        .with_state(app_state);
    
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response_json: Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(response_json["success"], true);
    
    // Should process all sources when source_ids is empty
    let source_info = &response_json["source_info"];
    assert_eq!(source_info["source_count"], 2);
    
    // Should match BBC channels from test data
    assert!(response_json["affected_channels"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn test_data_mapping_preview_error_handling() {
    let app_state = create_test_app_state().await;
    
    // Test missing expression
    let request_payload = json!({
        "source_type": "stream",
        "source_ids": [],
        "limit": 10
    });
    
    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/api/v1/data-mapping/preview")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&request_payload).unwrap()))
        .unwrap();
    
    let app = axum::Router::new()
        .route("/api/v1/data-mapping/preview", axum::routing::post(apply_data_mapping_rules_post))
        .with_state(app_state.clone());
    
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK); // Consistent error handling returns 200
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response_json: Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(response_json["success"], false);
    assert!(response_json["message"].as_str().unwrap().contains("Expression is required"));
    
    // Test invalid expression
    let request_payload = json!({
        "source_type": "stream", 
        "source_ids": [],
        "expression": "invalid expression syntax",
        "limit": 10
    });
    
    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/api/v1/data-mapping/preview")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&request_payload).unwrap()))
        .unwrap();
    
    let app = axum::Router::new()
        .route("/api/v1/data-mapping/preview", axum::routing::post(apply_data_mapping_rules_post))
        .with_state(app_state);
    
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response_json: Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(response_json["success"], false);
    // Should contain error information about invalid expression
    assert!(response_json["message"].is_string());
}

#[tokio::test]
async fn test_data_mapping_preview_limit_functionality() {
    let app_state = create_test_app_state().await;
    let source_ids = create_test_stream_sources(&app_state).await;
    create_test_channels(&app_state, &source_ids).await;
    
    // Test with small limit
    let request_payload = json!({
        "source_type": "stream",
        "source_ids": source_ids,
        "expression": "tvg_chno > \"0\" SET group_title = \"All Channels\"", // Should match all
        "limit": 2
    });
    
    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/api/v1/data-mapping/preview")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&request_payload).unwrap()))
        .unwrap();
    
    let app = axum::Router::new()
        .route("/api/v1/data-mapping/preview", axum::routing::post(apply_data_mapping_rules_post))
        .with_state(app_state);
    
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response_json: Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(response_json["success"], true);
    
    // Sample changes should respect the limit
    let sample_changes = response_json["sample_changes"].as_array().unwrap();
    assert!(sample_changes.len() <= 2, "Sample changes should respect limit of 2");
    
    // But total_channels and affected_channels should show full counts
    assert!(response_json["total_channels"].as_u64().unwrap() > 2);
    assert!(response_json["affected_channels"].as_u64().unwrap() > 2);
}

#[tokio::test]
async fn test_data_mapping_preview_complex_expression() {
    let app_state = create_test_app_state().await;
    let source_ids = create_test_stream_sources(&app_state).await;
    create_test_channels(&app_state, &source_ids).await;
    
    // Test complex expression with multiple conditions and actions
    let request_payload = json!({
        "source_type": "stream",
        "source_ids": [source_ids[0]],
        "expression": "(channel_name contains \"BBC\" OR channel_name contains \"Sky\") AND tvg_chno >= \"100\" SET group_title = \"Premium UK\", tvg_logo = \"@logo:uk-premium\"",
        "limit": 10
    });
    
    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/api/v1/data-mapping/preview")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&request_payload).unwrap()))
        .unwrap();
    
    let app = axum::Router::new()
        .route("/api/v1/data-mapping/preview", axum::routing::post(apply_data_mapping_rules_post))
        .with_state(app_state);
    
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response_json: Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(response_json["success"], true);
    
    // Should match appropriate channels based on complex criteria
    let sample_changes = response_json["sample_changes"].as_array().unwrap();
    if !sample_changes.is_empty() {
        let first_change = &sample_changes[0];
        let channel_name = first_change["channel_name"].as_str().unwrap();
        
        // Should match the complex condition
        assert!(channel_name.contains("BBC") || channel_name.contains("Sky"));
        
        // Should have multiple changes
        let changes = &first_change["changes"];
        assert!(changes.get("group_title").is_some());
        assert!(changes.get("tvg_logo").is_some());
    }
}

#[tokio::test] 
async fn test_data_mapping_preview_response_structure_consistency() {
    let app_state = create_test_app_state().await;
    let source_ids = create_test_stream_sources(&app_state).await;
    create_test_channels(&app_state, &source_ids).await;
    
    let request_payload = json!({
        "source_type": "stream",
        "source_ids": [source_ids[0]],
        "expression": "channel_name contains \"Test\" SET group_title = \"Test Group\"",
        "limit": 5
    });
    
    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/api/v1/data-mapping/preview")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&request_payload).unwrap()))
        .unwrap();
    
    let app = axum::Router::new()
        .route("/api/v1/data-mapping/preview", axum::routing::post(apply_data_mapping_rules_post))
        .with_state(app_state);
    
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response_json: Value = serde_json::from_slice(&body).unwrap();
    
    // Verify consistent response structure
    assert!(response_json.get("success").is_some());
    assert!(response_json.get("message").is_some());
    assert!(response_json.get("source_type").is_some());
    assert!(response_json.get("source_info").is_some());
    assert!(response_json.get("total_channels").is_some());
    assert!(response_json.get("affected_channels").is_some());
    assert!(response_json.get("sample_changes").is_some());
    
    // Verify source_info structure
    let source_info = &response_json["source_info"];
    assert!(source_info.get("source_count").is_some());
    assert!(source_info.get("source_names").is_some());
    assert!(source_info.get("source_ids").is_some());
    
    // Verify sample_changes structure if any exist
    let sample_changes = response_json["sample_changes"].as_array().unwrap();
    for change in sample_changes {
        assert!(change.get("channel_name").is_some());
        assert!(change.get("changes").is_some());
        // Additional fields should be preserved
        if change.get("tvg_id").is_some() {
            assert!(change["tvg_id"].is_string());
        }
    }
}

// Performance and load testing
#[tokio::test]
async fn test_data_mapping_preview_performance() {
    let app_state = create_test_app_state().await;
    let source_ids = create_test_stream_sources(&app_state).await;
    
    // Create more channels for performance testing
    let pool = &app_state.pool;
    for i in 1..=100 {
        let channel_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO channels (id, source_id, channel_name, tvg_id, tvg_chno, group_title, url) VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(channel_id.to_string())
        .bind(source_ids[0].to_string())
        .bind(format!("Performance Test Channel {}", i))
        .bind(format!("perf{}", i))
        .bind(Some(i as i64))
        .bind(Some("Performance"))
        .bind("http://example.com/stream")
        .execute(pool)
        .await
        .expect("Failed to create performance test channel");
    }
    
    let start_time = std::time::Instant::now();
    
    let request_payload = json!({
        "source_type": "stream",
        "source_ids": [source_ids[0]],
        "expression": "channel_name contains \"Performance\" SET group_title = \"Performance Channels\"",
        "limit": 50
    });
    
    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/api/v1/data-mapping/preview")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&request_payload).unwrap()))
        .unwrap();
    
    let app = axum::Router::new()
        .route("/api/v1/data-mapping/preview", axum::routing::post(apply_data_mapping_rules_post))
        .with_state(app_state);
    
    let response = app.oneshot(request).await.unwrap();
    let elapsed = start_time.elapsed();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response_json: Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(response_json["success"], true);
    assert!(response_json["affected_channels"].as_u64().unwrap() >= 100);
    
    // Performance assertion - should complete within reasonable time
    assert!(elapsed.as_millis() < 5000, "API call took too long: {:?}", elapsed);
}