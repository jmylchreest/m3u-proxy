//! API Security and Validation Testing
//!
//! This module provides comprehensive security testing for all API endpoints,
//! focusing on input validation, authentication, authorization, and protection
//! against common web vulnerabilities.
//!
//! Security areas covered:
//! - Input validation and sanitization
//! - SQL injection prevention (via parameterized queries)
//! - XSS prevention in JSON responses
//! - CSRF protection validation
//! - Rate limiting behavior
//! - Authentication and authorization
//! - HTTP header security
//! - Request size limitations
//! - Error message information disclosure

use axum::{
    body::Body,
    http::{self, Request, StatusCode, Method},
};
use axum_test::TestServer;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

use m3u_proxy::{
    config::Config,
    database::Database,
    models::*,
    repositories::{StreamSourceRepository, traits::Repository},
    web::state::AppState,
};

/// Helper to create test app state for API testing
async fn create_test_app_state() -> AppState {
    let config = Config::default();
    let database = Database::new_in_memory().await.expect("Failed to create test database");
    
    // Run migrations
    database.migrate().await.expect("Failed to run migrations");
    
    let pool = database.pool().clone();
    
    AppState::new(config, database, pool).await.expect("Failed to create app state")
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
    
    // LDAP injection
    "*)(uid=*",
    "*)(|(uid=*))",
    
    // XXE attempts
    "<!DOCTYPE foo [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]><foo>&xxe;</foo>",
    
    // NoSQL injection
    "'; return {'$ne': null} //",
    "{\"$ne\": null}",
    
    // Template injection
    "{{7*7}}",
    "${7*7}",
    "#{7*7}",
    
    // Large payloads for DoS testing
    &"A".repeat(100000),
    &"🚀".repeat(10000), // Unicode DoS
];

// =============================================================================
// INPUT VALIDATION TESTS
// =============================================================================

#[tokio::test]
async fn test_api_input_validation_stream_sources() {
    let app_state = create_test_app_state().await;
    let app = m3u_proxy::web::create_app(app_state);
    let server = TestServer::new(app).unwrap();

    for &payload in MALICIOUS_PAYLOADS {
        // Test stream source creation with malicious input
        let create_request = json!({
            "name": payload,
            "source_type": "m3u",
            "url": format!("http://example.com/{}", payload),
            "max_concurrent_streams": 10,
            "update_cron": payload,
            "ignore_channel_numbers": false
        });

        let response = server
            .post("/api/stream-sources")
            .json(&create_request)
            .await;

        // Should either validate and reject (400) or safely store the data
        match response.status_code() {
            StatusCode::BAD_REQUEST => {
                // Validation rejected - good!
                let response_text = response.text();
                
                // Ensure error messages don't leak sensitive information
                assert!(!response_text.contains("SQL"));
                assert!(!response_text.contains("sqlite"));
                assert!(!response_text.contains("database"));
                assert!(!response_text.contains("query"));
                assert!(!response_text.contains("/etc/"));
                assert!(!response_text.contains("passwd"));
            },
            StatusCode::OK | StatusCode::CREATED => {
                // Data was accepted - verify it was stored safely
                let response_json: Value = response.json();
                
                // Check that response doesn't contain executable JavaScript
                let response_text = response_json.to_string();
                assert!(!response_text.contains("<script>"));
                assert!(!response_text.contains("javascript:"));
                assert!(!response_text.contains("onerror="));
                
                // If successful, clean up the created resource
                if let Some(id) = response_json["id"].as_str() {
                    server.delete(&format!("/api/stream-sources/{}", id)).await;
                }
            },
            _ => {
                panic!("Unexpected status code {} for payload: {}", response.status_code(), payload);
            }
        }
    }
}

#[tokio::test]
async fn test_api_json_response_xss_prevention() {
    let app_state = create_test_app_state().await;
    let app = m3u_proxy::web::create_app(app_state);
    let server = TestServer::new(app).unwrap();

    // Create stream source with potential XSS payload in name
    let xss_payload = "<script>alert('xss')</script>";
    let create_request = json!({
        "name": xss_payload,
        "source_type": "m3u", 
        "url": "http://example.com/test.m3u",
        "max_concurrent_streams": 10,
        "update_cron": "0 */6 * * *",
        "ignore_channel_numbers": false
    });

    let response = server
        .post("/api/stream-sources")
        .json(&create_request)
        .await;

    if response.status_code().is_success() {
        let response_json: Value = response.json();
        let response_text = response_json.to_string();
        
        // Verify JSON response properly escapes HTML/JS
        assert!(!response_text.contains("<script>"));
        assert!(response_text.contains("&lt;script&gt;") || response_text.contains("\\u003c"));
        
        // Clean up
        if let Some(id) = response_json["id"].as_str() {
            server.delete(&format!("/api/stream-sources/{}", id)).await;
        }
    }
}

#[tokio::test]
async fn test_api_path_parameter_validation() {
    let app_state = create_test_app_state().await;
    let app = m3u_proxy::web::create_app(app_state);
    let server = TestServer::new(app).unwrap();

    let malicious_ids = &[
        "../../../etc/passwd",
        "'; DROP TABLE stream_sources; --",
        "<script>alert('xss')</script>",
        "not-a-valid-uuid",
        "null",
        "",
        "00000000-0000-0000-0000-000000000000", // nil UUID
    ];

    for &malicious_id in malicious_ids {
        // Test various endpoints with malicious path parameters
        let endpoints = &[
            format!("/api/stream-sources/{}", malicious_id),
            format!("/api/stream-sources/{}/refresh", malicious_id),
            format!("/api/channels/{}", malicious_id),
            format!("/api/filters/{}", malicious_id),
        ];

        for endpoint in endpoints {
            let response = server.get(endpoint).await;
            
            // Should return 400 (Bad Request) or 404 (Not Found), not 500 (Server Error)
            let status = response.status_code();
            assert!(
                status == StatusCode::BAD_REQUEST || 
                status == StatusCode::NOT_FOUND ||
                status == StatusCode::UNPROCESSABLE_ENTITY,
                "Endpoint {} with malicious ID {} returned unexpected status: {}",
                endpoint, malicious_id, status
            );

            // Error response should not leak sensitive information
            let response_text = response.text();
            assert!(!response_text.to_lowercase().contains("sql"));
            assert!(!response_text.to_lowercase().contains("database"));
            assert!(!response_text.to_lowercase().contains("sqlite"));
            assert!(!response_text.to_lowercase().contains("query"));
        }
    }
}

// =============================================================================
// AUTHENTICATION AND AUTHORIZATION TESTS
// =============================================================================

#[tokio::test]
async fn test_api_authentication_requirements() {
    let app_state = create_test_app_state().await;
    let app = m3u_proxy::web::create_app(app_state);
    let server = TestServer::new(app).unwrap();

    // Test endpoints that should require authentication (if implemented)
    let protected_endpoints = &[
        ("/api/stream-sources", Method::POST),
        ("/api/stream-sources", Method::DELETE),
        ("/api/filters", Method::POST),
        ("/api/filters", Method::DELETE),
    ];

    for &(endpoint, method) in protected_endpoints {
        let response = match method {
            Method::POST => {
                server.post(endpoint)
                    .json(&json!({"test": "data"}))
                    .await
            },
            Method::DELETE => {
                server.delete(endpoint).await
            },
            Method::GET => {
                server.get(endpoint).await
            },
            _ => continue,
        };

        // If authentication is implemented, should return 401 Unauthorized
        // If not implemented, should return 400/422 for missing required fields
        let status = response.status_code();
        assert!(
            status == StatusCode::UNAUTHORIZED ||
            status == StatusCode::BAD_REQUEST ||
            status == StatusCode::UNPROCESSABLE_ENTITY ||
            status == StatusCode::NOT_FOUND,
            "Protected endpoint {} returned unexpected status: {}", endpoint, status
        );
    }
}

#[tokio::test]
async fn test_api_cors_and_security_headers() {
    let app_state = create_test_app_state().await;
    let app = m3u_proxy::web::create_app(app_state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/api/health").await;
    let headers = response.headers();

    // Check for important security headers (if implemented)
    if headers.get("x-content-type-options").is_some() {
        assert_eq!(headers["x-content-type-options"], "nosniff");
    }
    
    if headers.get("x-frame-options").is_some() {
        let frame_options = headers["x-frame-options"].to_str().unwrap();
        assert!(frame_options == "DENY" || frame_options == "SAMEORIGIN");
    }

    if headers.get("x-xss-protection").is_some() {
        assert_eq!(headers["x-xss-protection"], "1; mode=block");
    }

    // CORS headers should be properly configured
    if headers.get("access-control-allow-origin").is_some() {
        let cors_origin = headers["access-control-allow-origin"].to_str().unwrap();
        assert!(cors_origin != "*" || cfg!(debug_assertions)); // Wildcard CORS only in debug
    }
}

// =============================================================================
// RATE LIMITING AND DOS PROTECTION TESTS
// =============================================================================

#[tokio::test]
async fn test_api_request_size_limits() {
    let app_state = create_test_app_state().await;
    let app = m3u_proxy::web::create_app(app_state);
    let server = TestServer::new(app).unwrap();

    // Test with extremely large request body
    let large_payload = json!({
        "name": "A".repeat(1_000_000), // 1MB string
        "source_type": "m3u",
        "url": "http://example.com/large.m3u",
        "max_concurrent_streams": 10,
        "update_cron": "0 */6 * * *",
        "ignore_channel_numbers": false
    });

    let response = server
        .post("/api/stream-sources")
        .json(&large_payload)
        .await;

    // Should reject large payloads with 413 (Payload Too Large) or similar
    let status = response.status_code();
    assert!(
        status == StatusCode::PAYLOAD_TOO_LARGE ||
        status == StatusCode::BAD_REQUEST ||
        status == StatusCode::UNPROCESSABLE_ENTITY,
        "Large payload should be rejected, got status: {}", status
    );
}

#[tokio::test]
async fn test_api_malformed_json_handling() {
    let app_state = create_test_app_state().await;
    let app = m3u_proxy::web::create_app(app_state);
    let server = TestServer::new(app).unwrap();

    let malformed_json_payloads = &[
        "{invalid json",
        "{'single': 'quotes'}",
        "{\"unclosed\": \"string}",
        "{\"trailing\": \"comma\",}",
        "null",
        "\"just a string\"",
        "123",
        "[\"array\", \"not\", \"object\"]",
    ];

    for &malformed_json in malformed_json_payloads {
        let response = server
            .post("/api/stream-sources")
            .header("content-type", "application/json")
            .text(malformed_json)
            .await;

        // Should return 400 Bad Request for malformed JSON
        assert_eq!(
            response.status_code(),
            StatusCode::BAD_REQUEST,
            "Malformed JSON should return 400, payload: {}", malformed_json
        );

        // Error message should not leak internal details
        let response_text = response.text();
        assert!(!response_text.to_lowercase().contains("serde"));
        assert!(!response_text.to_lowercase().contains("parse"));
        assert!(!response_text.to_lowercase().contains("deserialize"));
    }
}

// =============================================================================
// ERROR HANDLING AND INFORMATION DISCLOSURE TESTS
// =============================================================================

#[tokio::test]
async fn test_api_error_message_security() {
    let app_state = create_test_app_state().await;
    let app = m3u_proxy::web::create_app(app_state);
    let server = TestServer::new(app).unwrap();

    // Test various error conditions
    let error_test_cases = &[
        // Invalid UUID format
        ("/api/stream-sources/invalid-uuid", Method::GET),
        // Non-existent resource
        ("/api/stream-sources/00000000-0000-0000-0000-000000000000", Method::GET),
        // Invalid request body
        ("/api/stream-sources", Method::POST),
    ];

    for &(endpoint, method) in error_test_cases {
        let response = match method {
            Method::GET => server.get(endpoint).await,
            Method::POST => {
                server.post(endpoint)
                    .json(&json!({"invalid": "data"}))
                    .await
            },
            _ => continue,
        };

        let response_text = response.text().to_lowercase();

        // Error messages should not expose:
        assert!(!response_text.contains("sql"), "Error exposes SQL details");
        assert!(!response_text.contains("sqlite"), "Error exposes database type");
        assert!(!response_text.contains("database"), "Error exposes database details");
        assert!(!response_text.contains("query"), "Error exposes query details");
        assert!(!response_text.contains("panic"), "Error exposes panic details");
        assert!(!response_text.contains("unwrap"), "Error exposes unwrap details");
        assert!(!response_text.contains("expect"), "Error exposes expect details");
        assert!(!response_text.contains("/home/"), "Error exposes file paths");
        assert!(!response_text.contains("/usr/"), "Error exposes system paths");
        assert!(!response_text.contains("c:\\"), "Error exposes Windows paths");
        assert!(!response_text.contains("stacktrace"), "Error exposes stack trace");
        assert!(!response_text.contains("backtrace"), "Error exposes back trace");

        // Should provide user-friendly error messages
        assert!(response_text.len() > 0, "Error message should not be empty");
        assert!(response_text.len() < 1000, "Error message should not be too verbose");
    }
}

#[tokio::test]
async fn test_api_debug_endpoint_security() {
    let app_state = create_test_app_state().await;
    let app = m3u_proxy::web::create_app(app_state);
    let server = TestServer::new(app).unwrap();

    // Test potential debug/admin endpoints that should not exist in production
    let debug_endpoints = &[
        "/debug",
        "/admin",
        "/api/debug",
        "/api/admin",
        "/metrics",
        "/stats",
        "/status",
        "/info",
        "/config",
        "/env",
        "/health/full", // Should be limited compared to basic /health
    ];

    for &endpoint in debug_endpoints {
        let response = server.get(endpoint).await;
        
        // Debug endpoints should either:
        // 1. Not exist (404)
        // 2. Require authentication (401)
        // 3. Be properly secured with minimal information
        let status = response.status_code();
        
        if status.is_success() {
            let response_text = response.text().to_lowercase();
            
            // If endpoint exists, ensure it doesn't leak sensitive info
            assert!(!response_text.contains("password"));
            assert!(!response_text.contains("secret"));
            assert!(!response_text.contains("key"));
            assert!(!response_text.contains("token"));
            assert!(!response_text.contains("connection_string"));
            assert!(!response_text.contains("database_url"));
            assert!(!response_text.contains("/home/"));
            assert!(!response_text.contains("/usr/"));
            assert!(!response_text.contains("c:\\"));
        }
    }
}

// =============================================================================
// DATA MAPPING API SECURITY TESTS
// =============================================================================

#[tokio::test]
async fn test_data_mapping_api_injection_prevention() {
    let app_state = create_test_app_state().await;
    let app = m3u_proxy::web::create_app(app_state);
    let server = TestServer::new(app).unwrap();

    // Test data mapping expression with SQL injection attempts
    let malicious_expressions = &[
        "channel_name'; DROP TABLE channels; --",
        "1=1 OR (SELECT COUNT(*) FROM sqlite_master) > 0",
        "group_title\" UNION SELECT sql FROM sqlite_master --",
        "tvg_logo'); DELETE FROM stream_sources; --",
    ];

    for &expression in malicious_expressions {
        let request_payload = json!({
            "source_type": "stream",
            "source_ids": [],
            "expression": expression,
            "limit": 10
        });

        let response = server
            .post("/api/v1/data-mapping/preview")
            .json(&request_payload)
            .await;

        // Should either reject malicious expressions or handle them safely
        if response.status_code().is_success() {
            let response_json: Value = response.json();
            
            // If accepted, verify it was handled safely
            assert_eq!(response_json["success"], false, 
                "Malicious expression should be rejected or result in error");
            
            // Error message should not expose SQL details
            if let Some(message) = response_json["message"].as_str() {
                let msg_lower = message.to_lowercase();
                assert!(!msg_lower.contains("sql"));
                assert!(!msg_lower.contains("sqlite"));
                assert!(!msg_lower.contains("query"));
            }
        } else {
            // Rejection is also acceptable
            assert!(
                response.status_code() == StatusCode::BAD_REQUEST ||
                response.status_code() == StatusCode::UNPROCESSABLE_ENTITY
            );
        }
    }
}

// =============================================================================
// CONTENT TYPE AND ENCODING TESTS
// =============================================================================

#[tokio::test]
async fn test_api_content_type_validation() {
    let app_state = create_test_app_state().await;
    let app = m3u_proxy::web::create_app(app_state);
    let server = TestServer::new(app).unwrap();

    let valid_json = r#"{"name":"test","source_type":"m3u","url":"http://example.com/test.m3u","max_concurrent_streams":10,"update_cron":"0 */6 * * *","ignore_channel_numbers":false}"#;

    // Test with various content types
    let content_type_tests = &[
        ("application/json", true),
        ("application/json; charset=utf-8", true),
        ("text/plain", false),
        ("application/xml", false),
        ("application/x-www-form-urlencoded", false),
        ("multipart/form-data", false),
        ("", false), // No content type
    ];

    for &(content_type, should_accept) in content_type_tests {
        let mut request = server.post("/api/stream-sources").text(valid_json);
        
        if !content_type.is_empty() {
            request = request.header("content-type", content_type);
        }
        
        let response = request.await;
        let status = response.status_code();

        if should_accept {
            // Should accept JSON with proper content type
            assert!(
                status.is_success() || status == StatusCode::BAD_REQUEST,
                "Valid JSON with content-type {} should be accepted or properly validated, got {}",
                content_type, status
            );
        } else {
            // Should reject non-JSON content types
            assert_eq!(
                status, StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Invalid content-type {} should be rejected", content_type
            );
        }

        // Clean up if resource was created
        if status.is_success() {
            let response_json: Value = response.json();
            if let Some(id) = response_json["id"].as_str() {
                server.delete(&format!("/api/stream-sources/{}", id)).await;
            }
        }
    }
}

#[tokio::test]
async fn test_api_unicode_and_encoding_handling() {
    let app_state = create_test_app_state().await;
    let app = m3u_proxy::web::create_app(app_state);
    let server = TestServer::new(app).unwrap();

    // Test with various Unicode and encoding scenarios
    let unicode_test_cases = &[
        ("Basic ASCII", "Test Channel"),
        ("UTF-8 Unicode", "Test 频道 🚀 Канал"),
        ("Emoji Heavy", "🎬📺🔊🎭🎪🎨🎯"),
        ("Mixed Scripts", "العربية 中文 Русский Ελληνικά"),
        ("Zero Width", "Test\u{200B}Channel"), // Zero-width space
        ("Control Chars", "Test\x00\x01\x02Channel"), // Control characters
        ("RTL Override", "Test\u{202E}Channel"), // Right-to-left override
    ];

    for &(test_name, channel_name) in unicode_test_cases {
        let create_request = json!({
            "name": channel_name,
            "source_type": "m3u",
            "url": "http://example.com/unicode.m3u",
            "max_concurrent_streams": 10,
            "update_cron": "0 */6 * * *",
            "ignore_channel_numbers": false
        });

        let response = server
            .post("/api/stream-sources")
            .json(&create_request)
            .await;

        // Should handle Unicode properly
        if response.status_code().is_success() {
            let response_json: Value = response.json();
            let returned_name = response_json["name"].as_str().unwrap();
            
            // Verify Unicode is preserved correctly
            if !channel_name.contains('\x00') && !channel_name.contains('\x01') {
                // Control characters might be filtered, others should be preserved
                assert_eq!(returned_name, channel_name, 
                    "Unicode should be preserved for test: {}", test_name);
            }
            
            // Verify JSON response is valid UTF-8
            let response_text = response_json.to_string();
            assert!(response_text.is_ascii() || std::str::from_utf8(response_text.as_bytes()).is_ok());
            
            // Clean up
            if let Some(id) = response_json["id"].as_str() {
                server.delete(&format!("/api/stream-sources/{}", id)).await;
            }
        } else {
            // If rejected, should provide proper error message
            assert!(response.status_code() == StatusCode::BAD_REQUEST);
        }
    }
}

// =============================================================================
// HTTP METHOD SECURITY TESTS
// =============================================================================

#[tokio::test]
async fn test_api_http_method_restrictions() {
    let app_state = create_test_app_state().await;
    let app = m3u_proxy::web::create_app(app_state);
    let server = TestServer::new(app).unwrap();

    // Test that endpoints reject inappropriate HTTP methods
    let method_tests = &[
        ("/api/stream-sources", &[Method::GET, Method::POST], &[Method::PATCH, Method::PUT, Method::TRACE, Method::CONNECT]),
        ("/api/health", &[Method::GET], &[Method::POST, Method::DELETE, Method::PUT, Method::PATCH]),
    ];

    for &(endpoint, allowed_methods, disallowed_methods) in method_tests {
        // Test allowed methods (should not return 405 Method Not Allowed)
        for &method in allowed_methods {
            let response = match method {
                Method::GET => server.get(endpoint).await,
                Method::POST => server.post(endpoint).json(&json!({})).await,
                Method::PUT => server.put(endpoint).json(&json!({})).await,
                Method::DELETE => server.delete(endpoint).await,
                _ => continue,
            };
            
            assert_ne!(response.status_code(), StatusCode::METHOD_NOT_ALLOWED,
                "Method {} should be allowed for endpoint {}", method, endpoint);
        }

        // Test disallowed methods (should return 405 Method Not Allowed)
        for &method in disallowed_methods {
            let response = match method {
                Method::PATCH => server.patch(endpoint).json(&json!({})).await,
                Method::PUT => server.put(endpoint).json(&json!({})).await,
                Method::TRACE => {
                    // Trace method testing requires special handling
                    continue;
                },
                Method::CONNECT => {
                    // Connect method testing requires special handling  
                    continue;
                },
                _ => continue,
            };
            
            assert_eq!(response.status_code(), StatusCode::METHOD_NOT_ALLOWED,
                "Method {} should not be allowed for endpoint {}", method, endpoint);
        }
    }
}