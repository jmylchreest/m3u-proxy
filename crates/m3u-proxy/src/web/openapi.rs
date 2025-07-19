//! OpenAPI documentation generation using utoipa
//!
//! This module provides OpenAPI specification generation using utoipa annotations
//! on handler functions, with RapiDoc integration for interactive documentation.

use axum::{Json, response::IntoResponse};
use utoipa::OpenApi;

/// Main OpenAPI specification for M3U Proxy API
/// 
/// This defines the complete OpenAPI specification using utoipa annotations.
/// Handler functions are annotated with #[utoipa::path] for documentation.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "M3U Proxy API",
        version = "0.0.1",
        description = "
# M3U Proxy API

A modern IPTV proxy service with comprehensive OpenAPI documentation.

## 🚀 Features

This API provides complete OpenAPI documentation:
- **Handler annotations** with `#[utoipa::path]` for endpoint documentation
- **Schema generation** happens at compile time via `#[derive(ToSchema)]`
- **RapiDoc integration** provides interactive documentation
- **Comprehensive coverage** of all API endpoints

## 📋 Features

- **Multi-source aggregation**: Combine M3U playlists and Xtream Codes APIs
- **Advanced filtering**: Channel filtering with complex conditions  
- **Data mapping**: Transform channel metadata (names, logos, EPG IDs)
- **Logo management**: Automatic logo caching and optimization
- **EPG integration**: Electronic Program Guide aggregation
- **Real-time streaming**: Efficient proxy streaming with health monitoring
- **Relay system**: Channel restreaming with transcoding

## 🔧 Adding New Endpoints

To add a new endpoint to this documentation:

1. **Add the annotation** to your handler:
   ```rust
   #[utoipa::path(
       get,
       path = \"/api/v1/your-endpoint\",
       tag = \"your-tag\",
       summary = \"Your endpoint summary\"
   )]
   pub async fn your_handler() -> impl IntoResponse {
       // handler implementation
   }
   ```

2. **Include in the paths section** below (if using path-based documentation)

3. **Handler annotations are used** to generate comprehensive documentation

## 📊 Current API Coverage

All endpoints are documented with utoipa annotations.
Check the endpoints below to see the complete API surface.
        ",
        contact(name = "M3U Proxy Support"),
        license(name = "MIT", url = "https://opensource.org/licenses/MIT")
    ),
    servers(
        (url = "/api/v1", description = "API Version 1 - Auto-discovered routes"),
    ),
    tags(
        (name = "stream-sources", description = "Stream source management - M3U playlists and Xtream Codes APIs"),
        (name = "epg-sources", description = "Electronic Program Guide (EPG) source management"),  
        (name = "proxies", description = "Proxy configuration and M3U playlist generation"),
        (name = "filters", description = "Channel filtering and rules management"),
        (name = "data-mapping", description = "Channel metadata transformation rules"),
        (name = "logos", description = "Logo asset management and optimization"),
        (name = "relay", description = "Relay system for channel restreaming and transcoding"),
        (name = "health", description = "Service health monitoring and diagnostics"),
        (name = "metrics", description = "Performance metrics and analytics"),
        (name = "progress", description = "Background operation progress tracking"),
    ),
    components(
        schemas(
            // Core models
            crate::models::StreamSource,
            crate::models::StreamSourceType,
            crate::models::EpgSource,
            crate::models::EpgSourceType,
            
            // Stream Sources DTOs
            crate::web::handlers::stream_sources::CreateStreamSourceRequest,
            crate::web::handlers::stream_sources::UpdateStreamSourceRequest,
            crate::web::handlers::stream_sources::StreamSourceResponse,
            
            // EPG Sources DTOs
            crate::web::handlers::epg_sources::CreateEpgSourceRequest,
            crate::web::handlers::epg_sources::UpdateEpgSourceRequest,
            crate::web::handlers::epg_sources::EpgSourceResponse,
            
            // Response wrappers
            crate::web::responses::ApiResponse<crate::web::handlers::stream_sources::StreamSourceResponse>,
            crate::web::responses::PaginatedResponse<crate::web::handlers::stream_sources::StreamSourceResponse>,
            crate::web::responses::ApiResponse<crate::web::handlers::epg_sources::EpgSourceResponse>,
            crate::web::responses::PaginatedResponse<crate::web::handlers::epg_sources::EpgSourceResponse>,
            
            // Query parameters
            crate::web::extractors::PaginationParams,
            crate::web::extractors::StreamSourceFilterParams,
            crate::web::extractors::EpgSourceFilterParams,
        )
    ),
    paths(
        // Stream Sources endpoints
        crate::web::handlers::stream_sources::list_stream_sources,
        crate::web::handlers::stream_sources::get_stream_source,
        crate::web::handlers::stream_sources::create_stream_source,
        crate::web::handlers::stream_sources::update_stream_source,
        crate::web::handlers::stream_sources::delete_stream_source,
        crate::web::handlers::stream_sources::validate_stream_source,
        crate::web::handlers::stream_sources::get_stream_source_capabilities,
        
        // EPG Sources endpoints
        crate::web::handlers::epg_sources::list_epg_sources,
        crate::web::handlers::epg_sources::get_epg_source,
        crate::web::handlers::epg_sources::create_epg_source,
        crate::web::handlers::epg_sources::update_epg_source,
        crate::web::handlers::epg_sources::delete_epg_source,
        crate::web::handlers::epg_sources::validate_epg_source,
        
        // Logo endpoints
        crate::web::api::list_logo_assets,
        crate::web::api::upload_logo_asset,
        crate::web::api::get_logo_asset_image,
        crate::web::api::search_logo_assets,
        
        // Filter endpoints
        crate::web::api::list_filters,
        crate::web::api::create_filter,
        
        // Data mapping endpoints
        crate::web::api::list_data_mapping_rules,
        crate::web::api::create_data_mapping_rule,
        crate::web::api::get_data_mapping_rule,
        crate::web::api::update_data_mapping_rule,
        crate::web::api::delete_data_mapping_rule,
        crate::web::api::reorder_data_mapping_rules,
        crate::web::api::validate_data_mapping_expression,
        crate::web::api::get_data_mapping_stream_fields,
        crate::web::api::get_data_mapping_epg_fields,
        crate::web::api::test_data_mapping_rule,
        
        // Proxy endpoints
        crate::web::handlers::proxies::list_proxies,
        crate::web::handlers::proxies::get_proxy,
        crate::web::handlers::proxies::create_proxy,
        crate::web::handlers::proxies::update_proxy,
        crate::web::handlers::proxies::delete_proxy,
        crate::web::handlers::proxies::serve_proxy_m3u,
        
        // Relay endpoints
        crate::web::api::relay::list_profiles,
        crate::web::api::relay::get_profile,
        crate::web::api::relay::create_profile,
        crate::web::api::relay::update_profile,
        crate::web::api::relay::delete_profile,
        
        // Metrics endpoints
        crate::web::api::get_dashboard_metrics,
        crate::web::api::get_realtime_metrics,
        crate::web::api::get_usage_metrics,
    )
)]
pub struct ApiDoc;

/// Get the OpenAPI specification
pub fn get_openapi_spec() -> utoipa::openapi::OpenApi {
    let mut openapi = ApiDoc::openapi();
    
    // Update with dynamic version
    openapi.info.version = env!("CARGO_PKG_VERSION").to_string();
    
    openapi
}

/// Get the comprehensive OpenAPI specification with version info
pub fn get_comprehensive_openapi_spec() -> utoipa::openapi::OpenApi {
    let mut openapi = get_openapi_spec();
    
    // Enhanced description with version info
    let version = env!("CARGO_PKG_VERSION");
    let enhanced_description = format!(
        "
# M3U Proxy API v{}

A modern IPTV proxy service with comprehensive OpenAPI documentation.

## ✨ Key Benefits

- **📚 Always Up-to-Date**: Documentation is generated from source code annotations
- **🔧 Easy to Extend**: Add `#[utoipa::path]` annotations to functions  
- **⚡ Fast**: Schema generation happens at compile time, not runtime
- **📖 Interactive**: Full RapiDoc integration with try-it-out functionality

## 🏗️ Architecture

- **Version**: {}
- **Schema Generation**: ✅ Compile-time via `#[derive(ToSchema)]`
- **Documentation UI**: ✅ RapiDoc interactive interface
- **Handler Annotations**: ✅ Complete endpoint documentation

## 📍 Current Endpoints

All endpoints are documented with utoipa annotations.

## 🚀 Getting Started

Visit `/docs` for the interactive API documentation.
        ",
        version, version
    );
    
    openapi.info.description = Some(enhanced_description);
    openapi
}

/// Serve the OpenAPI specification JSON
pub async fn serve_openapi_spec() -> impl IntoResponse {
    let openapi = get_comprehensive_openapi_spec();
    Json(openapi)
}