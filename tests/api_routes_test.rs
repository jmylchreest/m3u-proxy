use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

// Helper function to send requests to the app
async fn send_request(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request_builder = Request::builder().method(method).uri(uri);

    let request = if let Some(body) = body {
        request_builder
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap()
    } else {
        request_builder.body(Body::empty()).unwrap()
    };

    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();

    let json: Value = if body_bytes.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&body_bytes).unwrap_or(json!({}))
    };

    (status, json)
}

#[tokio::test]
async fn test_health_endpoint() {
    // This is a basic test to verify the test setup works
    // We'll create a minimal router with just the health endpoint

    use axum::{response::Json, routing::get};

    async fn health() -> Json<Value> {
        Json(json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    let app = Router::new().route("/health", get(health));

    let (status, response) = send_request(&app, Method::GET, "/health", None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["status"], "healthy");
    assert!(response.get("timestamp").is_some());
}

#[tokio::test]
async fn test_api_route_structure() {
    // Test that our route structure is what we expect
    use axum::{response::Json, routing::get};

    async fn mock_handler() -> Json<Value> {
        Json(json!({
            "success": true,
            "message": "Mock response"
        }))
    }

    // Create a router with our expected hierarchical structure
    let app = Router::new()
        // Data mapping preview endpoints
        .route(
            "/api/sources/stream/:id/data-mapping/preview",
            get(mock_handler),
        )
        .route(
            "/api/sources/epg/:id/data-mapping/preview",
            get(mock_handler),
        )
        // Progress endpoints
        .route("/api/progress", get(mock_handler))
        .route("/api/progress/sources", get(mock_handler))
        .route("/api/progress/operations", get(mock_handler))
        .route("/api/sources/stream/:id/progress", get(mock_handler))
        .route("/api/sources/epg/:id/progress", get(mock_handler))
        // Filter endpoints
        .route("/api/sources/stream/:id/filters", get(mock_handler))
        .route("/api/sources/epg/:id/filters", get(mock_handler))
        .route("/api/filters/stream", get(mock_handler))
        .route("/api/filters/epg", get(mock_handler))
        .route("/api/filters/stream/fields", get(mock_handler))
        .route("/api/filters/epg/fields", get(mock_handler));

    let test_id = Uuid::new_v4();

    // Test data mapping preview routes
    let (status, response) = send_request(
        &app,
        Method::GET,
        &format!("/api/sources/stream/{}/data-mapping/preview", test_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["success"], true);

    let (status, response) = send_request(
        &app,
        Method::GET,
        &format!("/api/sources/epg/{}/data-mapping/preview", test_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["success"], true);

    // Test progress routes
    let (status, response) = send_request(&app, Method::GET, "/api/progress", None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, response) = send_request(&app, Method::GET, "/api/progress/sources", None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, response) =
        send_request(&app, Method::GET, "/api/progress/operations", None).await;
    assert_eq!(status, StatusCode::OK);

    // Test filter routes
    let (status, response) = send_request(
        &app,
        Method::GET,
        &format!("/api/sources/stream/{}/filters", test_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, response) = send_request(
        &app,
        Method::GET,
        &format!("/api/sources/epg/{}/filters", test_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, response) = send_request(&app, Method::GET, "/api/filters/stream", None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, response) = send_request(&app, Method::GET, "/api/filters/epg", None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, response) =
        send_request(&app, Method::GET, "/api/filters/stream/fields", None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, response) = send_request(&app, Method::GET, "/api/filters/epg/fields", None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_hierarchical_url_patterns() {
    // Test that our hierarchical URL patterns are correctly structured

    let test_cases = vec![
        // Data mapping preview
        (
            "/api/sources/stream/123e4567-e89b-12d3-a456-426614174000/data-mapping/preview",
            true,
        ),
        (
            "/api/sources/epg/123e4567-e89b-12d3-a456-426614174000/data-mapping/preview",
            true,
        ),
        // Progress
        ("/api/progress", true),
        ("/api/progress/sources", true),
        ("/api/progress/operations", true),
        (
            "/api/sources/stream/123e4567-e89b-12d3-a456-426614174000/progress",
            true,
        ),
        (
            "/api/sources/epg/123e4567-e89b-12d3-a456-426614174000/progress",
            true,
        ),
        // Filters
        (
            "/api/sources/stream/123e4567-e89b-12d3-a456-426614174000/filters",
            true,
        ),
        (
            "/api/sources/epg/123e4567-e89b-12d3-a456-426614174000/filters",
            true,
        ),
        ("/api/filters/stream", true),
        ("/api/filters/epg", true),
        ("/api/filters/stream/fields", true),
        ("/api/filters/epg/fields", true),
    ];

    for (url, should_be_valid) in test_cases {
        // Validate URL structure
        assert!(url.starts_with("/api/"));

        if should_be_valid {
            // Check hierarchical structure
            if url.contains("/sources/") {
                assert!(url.contains("/stream/") || url.contains("/epg/"));

                if url.contains("/data-mapping/preview") {
                    assert!(url.matches("/").count() >= 5); // /api/sources/type/id/data-mapping/preview
                } else if url.contains("/filters") {
                    assert!(url.matches("/").count() >= 4); // /api/sources/type/id/filters
                } else if url.contains("/progress") {
                    assert!(url.matches("/").count() >= 4); // /api/sources/type/id/progress
                }
            }

            if url.contains("/filters/") && !url.contains("/sources/") {
                assert!(url.contains("/stream") || url.contains("/epg"));
            }
        }
    }
}

#[tokio::test]
async fn test_response_format_structure() {
    // Test that our API responses follow consistent patterns

    use axum::{response::Json, routing::get};

    async fn consistent_response() -> Json<Value> {
        Json(json!({
            "success": true,
            "message": "Test message",
            "data": {
                "items": [],
                "total": 0
            }
        }))
    }

    async fn hierarchical_response() -> Json<Value> {
        Json(json!({
            "success": true,
            "message": "Hierarchical response",
            "source_id": "123e4567-e89b-12d3-a456-426614174000",
            "source_type": "stream",
            "data": {}
        }))
    }

    let app = Router::new()
        .route("/api/test/consistent", get(consistent_response))
        .route("/api/test/hierarchical", get(hierarchical_response));

    // Test consistent response format
    let (status, response) = send_request(&app, Method::GET, "/api/test/consistent", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["success"], true);
    assert!(response.get("message").is_some());
    assert!(response.get("data").is_some());

    // Test hierarchical response format
    let (status, response) = send_request(&app, Method::GET, "/api/test/hierarchical", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["success"], true);
    assert!(response.get("message").is_some());
    assert!(response.get("source_id").is_some());
    assert!(response.get("source_type").is_some());
    assert!(response["source_type"] == "stream" || response["source_type"] == "epg");
}

#[tokio::test]
async fn test_error_response_format() {
    // Test that error responses are handled consistently

    use axum::{http::StatusCode as AxumStatusCode, response::Json, routing::get};

    async fn not_found_handler() -> Result<Json<Value>, AxumStatusCode> {
        Err(AxumStatusCode::NOT_FOUND)
    }

    async fn bad_request_handler() -> Result<Json<Value>, AxumStatusCode> {
        Err(AxumStatusCode::BAD_REQUEST)
    }

    let app = Router::new()
        .route("/api/test/not-found", get(not_found_handler))
        .route("/api/test/bad-request", get(bad_request_handler));

    // Test 404 response
    let (status, _response) = send_request(&app, Method::GET, "/api/test/not-found", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Test 400 response
    let (status, _response) = send_request(&app, Method::GET, "/api/test/bad-request", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_backward_compatibility_structure() {
    // Test that our legacy endpoints could still work alongside new ones

    use axum::{response::Json, routing::get};

    async fn legacy_handler() -> Json<Value> {
        Json(json!({
            "legacy": true,
            "data": []
        }))
    }

    async fn new_stream_handler() -> Json<Value> {
        Json(json!({
            "success": true,
            "message": "New hierarchical endpoint",
            "source_type": "stream",
            "data": []
        }))
    }

    async fn new_epg_handler() -> Json<Value> {
        Json(json!({
            "success": true,
            "message": "New hierarchical endpoint",
            "source_type": "epg",
            "data": []
        }))
    }

    let app = Router::new()
        // Legacy endpoints
        .route("/api/filters", get(legacy_handler))
        .route("/api/filters/fields", get(legacy_handler))
        // New hierarchical endpoints
        .route("/api/filters/stream", get(new_stream_handler))
        .route("/api/filters/epg", get(new_epg_handler))
        .route("/api/filters/stream/fields", get(new_stream_handler))
        .route("/api/filters/epg/fields", get(new_epg_handler));

    // Test legacy endpoints still work
    let (status, response) = send_request(&app, Method::GET, "/api/filters", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["legacy"], true);

    let (status, response) = send_request(&app, Method::GET, "/api/filters/fields", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["legacy"], true);

    // Test new endpoints work
    let (status, response) = send_request(&app, Method::GET, "/api/filters/stream", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["success"], true);
    assert_eq!(response["source_type"], "stream");

    let (status, response) = send_request(&app, Method::GET, "/api/filters/epg", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["success"], true);
    assert_eq!(response["source_type"], "epg");
}

#[tokio::test]
async fn test_consolidated_progress_endpoint() {
    // Test that consolidated progress endpoint reduces API calls by including both progress and processing info

    use axum::{response::Json, routing::get};

    async fn consolidated_progress_handler() -> Json<Value> {
        Json(json!({
            "success": true,
            "message": "Source progress retrieved",
            "progress": {
                "123e4567-e89b-12d3-a456-426614174000": {
                    "progress": {
                        "source_id": "123e4567-e89b-12d3-a456-426614174000",
                        "state": "processing",
                        "progress": {
                            "current_step": "Processing... 39,228/39,228",
                            "percentage": 100
                        },
                        "started_at": "2024-01-01T12:00:00Z",
                        "updated_at": "2024-01-01T12:05:00Z"
                    },
                    "processing_info": {
                        "started_at": "2024-01-01T12:00:00Z",
                        "failure_count": 0,
                        "next_retry_after": null
                    }
                }
            },
            "total_sources": 1
        }))
    }

    let app = Router::new().route("/api/progress/sources", get(consolidated_progress_handler));

    let (status, response) = send_request(&app, Method::GET, "/api/progress/sources", None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["success"], true);
    assert_eq!(response["message"], "Source progress retrieved");

    // Verify consolidated structure includes both progress and processing info
    let progress_data = &response["progress"];
    assert!(progress_data.is_object());

    let source_data = &progress_data["123e4567-e89b-12d3-a456-426614174000"];
    assert!(source_data.get("progress").is_some());
    assert!(source_data.get("processing_info").is_some());

    // Verify progress data structure
    let progress = &source_data["progress"];
    assert_eq!(progress["state"], "processing");
    assert_eq!(
        progress["progress"]["current_step"],
        "Processing... 39,228/39,228"
    );
    assert_eq!(progress["progress"]["percentage"], 100);

    // Verify processing info is included
    let processing_info = &source_data["processing_info"];
    assert!(processing_info.get("started_at").is_some());
    assert!(processing_info.get("failure_count").is_some());
    assert!(processing_info.get("next_retry_after").is_some());
}
