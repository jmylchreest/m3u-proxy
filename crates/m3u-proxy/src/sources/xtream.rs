//! Xtream source handler implementation
//!
//! This module provides the concrete implementation for handling Xtream Codes API sources.
//! It supports authentication, channel listing, EPG data, and VOD content retrieval.

use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, trace};
use uuid::Uuid;

use crate::errors::{AppError, AppResult, SourceError};
use crate::models::{StreamSource, StreamSourceType, Channel};
use crate::utils::{DecompressingHttpClient, StandardHttpClient};
use super::traits::*;

/// Xtream source handler
///
/// This handler implements the full source handler interface for Xtream Codes API sources.
/// It supports authentication, live TV, VOD, and EPG data retrieval.
///
/// # Features
/// - User/password authentication
/// - Live TV channel retrieval with automatic decompression
/// - VOD content support (future)
/// - EPG data integration (future)
/// - Server information and capabilities detection
/// - Health monitoring with detailed metrics
/// - Stream URL generation with authentication
pub struct XtreamSourceHandler {
    http_client: StandardHttpClient,
    raw_client: Client,
}

impl XtreamSourceHandler {
    /// Create a new Xtream source handler
    pub fn new() -> Self {
        let http_client = StandardHttpClient::with_timeout(Duration::from_secs(30));
        let raw_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("Xtream-Proxy/1.0")
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { http_client, raw_client }
    }

    /// Get the base API URL for an Xtream source
    fn get_api_base_url(&self, source: &StreamSource) -> AppResult<String> {
        // If URL already has a scheme, use it as-is
        let url_to_parse = if source.url.starts_with("http://") || source.url.starts_with("https://") {
            source.url.clone()
        } else {
            // Default to HTTPS for security
            format!("https://{}", source.url)
        };

        debug!("Parsing base URL from: {}", crate::utils::url::UrlUtils::obfuscate_credentials(&url_to_parse));

        // Parse and validate the URL
        let parsed = reqwest::Url::parse(&url_to_parse)
            .map_err(|e| AppError::validation(format!("Invalid Xtream URL '{}': {}", source.url, e)))?;

        // Build the base URL
        let base_url = if let Some(port) = parsed.port() {
            format!(
                "{}://{}:{}",
                parsed.scheme(),
                parsed.host_str().unwrap_or("localhost"),
                port
            )
        } else {
            format!(
                "{}://{}",
                parsed.scheme(),
                parsed.host_str().unwrap_or("localhost")
            )
        };

        Ok(format!("{}/player_api.php", base_url.trim_end_matches('/')))
    }


    /// Basic connection test (lighter than full test_authentication)
    #[allow(dead_code)]
    async fn test_connection_basic(&self, source: &StreamSource) -> AppResult<()> {
        let base_url = self.get_api_base_url(source)?;
        let auth_params = self.get_auth_params(source)?;
        let mut url = reqwest::Url::parse(&base_url)
            .map_err(|e| AppError::validation(format!("Invalid Xtream URL: {}", e)))?;
        
        // Add authentication parameters
        for (key, value) in &auth_params {
            url.query_pairs_mut().append_pair(key, value);
        }

        // Quick connection test with shorter timeout
        let response = self.raw_client
            .get(url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| {
                let error_msg = e.to_string();
                let obfuscated_msg = crate::utils::url::UrlUtils::obfuscate_credentials(&error_msg);
                AppError::ExternalService { 
                    service: "xtream_api".to_string(), 
                    message: obfuscated_msg 
                }
            })?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(AppError::source_error(format!("Server returned status: {}", response.status())))
        }
    }

    /// Build authentication parameters for API calls
    fn get_auth_params(&self, source: &StreamSource) -> AppResult<HashMap<String, String>> {
        let username = source.username.as_ref()
            .ok_or_else(|| AppError::validation("Xtream source requires username"))?;
        let password = source.password.as_ref()
            .ok_or_else(|| AppError::validation("Xtream source requires password"))?;

        let mut params = HashMap::new();
        params.insert("username".to_string(), username.clone());
        params.insert("password".to_string(), password.clone());
        
        Ok(params)
    }

    /// Test authentication with the Xtream server
    async fn test_authentication(&self, source: &StreamSource) -> AppResult<XtreamServerInfo> {
        let base_url = self.get_api_base_url(source)?;
        let auth_params = self.get_auth_params(source)?;

        let mut url = reqwest::Url::parse(&base_url)
            .map_err(|e| AppError::validation(format!("Invalid Xtream URL: {}", e)))?;

        // Add authentication parameters
        for (key, value) in &auth_params {
            url.query_pairs_mut().append_pair(key, value);
        }

        debug!("Testing Xtream authentication for: {}", source.name);

        let server_info: XtreamServerInfo = self.http_client.fetch_json(url.as_str()).await
            .map_err(|e| e)?; // Pass through the original HTTP error without wrapping

        // Check if authentication was successful
        if let Some(ref auth) = server_info.user_info {
            if auth.status != "Active" {
                return Err(AppError::Source(SourceError::auth_failed("xtream", format!("user status is {}", auth.status))));
            }
        } else {
            return Err(AppError::Source(SourceError::auth_failed("xtream", "server did not return user information")));
        }

        Ok(server_info)
    }

    /// Get live TV channels from Xtream server
    async fn get_live_channels(&self, source: &StreamSource) -> AppResult<Vec<XtreamChannel>> {
        let base_url = self.get_api_base_url(source)?;
        let auth_params = self.get_auth_params(source)?;

        let mut url = reqwest::Url::parse(&base_url)
            .map_err(|e| AppError::validation(format!("Invalid Xtream URL: {}", e)))?;

        // Add parameters for live TV channels
        for (key, value) in &auth_params {
            url.query_pairs_mut().append_pair(key, value);
        }
        url.query_pairs_mut().append_pair("action", "get_live_streams");

        debug!("Fetching live channels from Xtream source: {}", source.name);

        let channels: Vec<XtreamChannel> = self.http_client.fetch_json(url.as_str()).await
            .map_err(|e| e)?; // Pass through the original HTTP error

        info!("Retrieved {} live channels from Xtream source: {}", channels.len(), source.name);
        Ok(channels)
    }

    /// Convert Xtream channel to internal Channel model
    fn convert_xtream_channel(&self, xtream_channel: &XtreamChannel, source: &StreamSource) -> Channel {
        let now = Utc::now();
        
        // Debug logging for channel number handling
        let tvg_chno_value = if source.ignore_channel_numbers {
            trace!("Channel '{}': ignore_channel_numbers=true, setting tvg_chno to None (original num={})", 
                xtream_channel.name, xtream_channel.num.map_or("None".to_string(), |n| n.to_string()));
            None // Ignore channel numbers from Xtream API, allow numbering stage to assign
        } else {
            let result = xtream_channel.num.map(|n| n.to_string());
            trace!("Channel '{}': ignore_channel_numbers=false, preserving tvg_chno={:?} (original num={})", 
                xtream_channel.name, result, xtream_channel.num.map_or("None".to_string(), |n| n.to_string()));
            result // Preserve original channel numbers
        };
        
        Channel {
            id: Uuid::new_v4(),
            source_id: source.id,
            tvg_id: xtream_channel.epg_channel_id.clone(),
            tvg_name: Some(xtream_channel.name.clone()),
            tvg_chno: tvg_chno_value,
            tvg_logo: xtream_channel.stream_icon.clone(),
            tvg_shift: None,
            group_title: xtream_channel.category_name.clone(),
            channel_name: xtream_channel.name.clone(),
            stream_url: self.generate_xtream_stream_url(source, &xtream_channel.stream_id.to_string()),
            created_at: now,
            updated_at: now,
        }
    }

    /// Generate Xtream stream URL for a channel
    fn generate_xtream_stream_url(&self, source: &StreamSource, stream_id: &str) -> String {
        let base_url = source.url.trim_end_matches('/');
        let empty_string = String::new();
        let username = source.username.as_ref().unwrap_or(&empty_string);
        let password = source.password.as_ref().unwrap_or(&empty_string);
        
        format!("{}/live/{}/{}/{}.ts", base_url, username, password, stream_id)
    }

    /// Validate Xtream URL format
    fn validate_xtream_url(&self, url: &str) -> AppResult<SourceValidationResult> {
        let mut result = SourceValidationResult::success();

        // Basic URL format validation
        if !url.starts_with("http://") && !url.starts_with("https://") {
            result.errors.push("Xtream URL must use HTTP or HTTPS protocol".to_string());
            result.is_valid = false;
        }

        // Check for typical Xtream patterns
        if !url.contains("/player_api.php") && !url.contains("get.php") && !url.contains("xmltv.php") {
            result = result.with_warning("URL doesn't contain typical Xtream API endpoints");
        }

        // Check for port (many Xtream servers use non-standard ports)
        if let Ok(parsed_url) = reqwest::Url::parse(url) {
            if let Some(port) = parsed_url.port() {
                result = result.with_context("port", port.to_string());
                #[allow(unused_comparisons)]
                if port < 1024 || port > 65535 {
                    result = result.with_warning("Port number is outside typical range");
                }
            }
        }

        Ok(result)
    }

    /// Get categories from Xtream server
    #[allow(dead_code)]
    async fn get_categories(&self, source: &StreamSource) -> AppResult<Vec<XtreamCategory>> {
        let base_url = self.get_api_base_url(source)?;
        let auth_params = self.get_auth_params(source)?;

        let mut url = reqwest::Url::parse(&base_url)
            .map_err(|e| AppError::validation(format!("Invalid Xtream URL: {}", e)))?;

        for (key, value) in &auth_params {
            url.query_pairs_mut().append_pair(key, value);
        }
        url.query_pairs_mut().append_pair("action", "get_live_categories");

        let categories: Vec<XtreamCategory> = self.http_client.fetch_json(url.as_str()).await
            .map_err(|e| e)?; // Pass through the original HTTP error

        Ok(categories)
    }
}

/// Xtream server information response
#[derive(Debug, Clone, Deserialize)]
struct XtreamServerInfo {
    pub user_info: Option<XtreamUserInfo>,
    pub server_info: Option<XtreamServerDetails>,
}

/// Xtream user authentication info
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct XtreamUserInfo {
    pub username: String,
    pub password: String,
    pub message: Option<String>,
    pub auth: Option<i32>,
    pub status: String,
    pub exp_date: Option<String>,
    pub is_trial: Option<String>,
    pub active_cons: Option<String>,
    pub created_at: Option<String>,
    pub max_connections: Option<String>,
    pub allowed_output_formats: Option<Vec<String>>,
}

/// Xtream server details
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct XtreamServerDetails {
    pub url: Option<String>,
    pub port: Option<String>,
    pub https_port: Option<String>,
    pub server_protocol: Option<String>,
    pub rtmp_port: Option<String>,
    pub timezone: Option<String>,
    pub timestamp_now: Option<i64>,
    pub time_now: Option<String>,
}

/// Xtream channel information
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct XtreamChannel {
    #[serde(deserialize_with = "deserialize_string_or_int_option", default)]
    pub num: Option<i32>,
    pub name: String,
    #[serde(default = "default_stream_type")]
    pub stream_type: String,
    #[serde(deserialize_with = "deserialize_string_or_int")]
    pub stream_id: i32,
    #[serde(default)]
    pub stream_icon: Option<String>,
    #[serde(default)]
    pub epg_channel_id: Option<String>,
    #[serde(default)]
    pub added: Option<String>,
    #[serde(default)]
    pub category_name: Option<String>,
    #[serde(deserialize_with = "deserialize_string_or_int_option", default)]
    pub category_id: Option<i32>,
    #[serde(deserialize_with = "deserialize_string_or_int_option", default)]
    pub tv_archive: Option<i32>,
    #[serde(deserialize_with = "deserialize_string_or_int_option", default)]
    pub tv_archive_duration: Option<i32>,
    #[serde(default)]
    pub direct_source: Option<String>,
}

/// Xtream category information
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct XtreamCategory {
    pub category_id: String,
    pub category_name: String,
    pub parent_id: Option<i32>,
}

// Helper functions for deserialization
fn default_stream_type() -> String {
    "live".to_string()
}

fn deserialize_string_or_int<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Unexpected, Visitor};
    
    struct StringOrIntVisitor;
    
    impl<'de> Visitor<'de> for StringOrIntVisitor {
        type Value = i32;
        
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or integer")
        }
        
        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value >= i32::MIN as i64 && value <= i32::MAX as i64 {
                Ok(value as i32)
            } else {
                Err(E::invalid_value(Unexpected::Signed(value), &self))
            }
        }
        
        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value <= i32::MAX as u64 {
                Ok(value as i32)
            } else {
                Err(E::invalid_value(Unexpected::Unsigned(value), &self))
            }
        }
        
        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            value.parse().map_err(|_| E::invalid_value(Unexpected::Str(value), &self))
        }
    }
    
    deserializer.deserialize_any(StringOrIntVisitor)
}

fn deserialize_string_or_int_option<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Unexpected, Visitor};
    
    struct StringOrIntOptionVisitor;
    
    impl<'de> Visitor<'de> for StringOrIntOptionVisitor {
        type Value = Option<i32>;
        
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string, integer, or null")
        }
        
        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
        
        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
        
        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value >= i32::MIN as i64 && value <= i32::MAX as i64 {
                Ok(Some(value as i32))
            } else {
                Err(E::invalid_value(Unexpected::Signed(value), &self))
            }
        }
        
        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value <= i32::MAX as u64 {
                Ok(Some(value as i32))
            } else {
                Err(E::invalid_value(Unexpected::Unsigned(value), &self))
            }
        }
        
        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value.is_empty() {
                Ok(None)
            } else {
                value.parse().map(Some).map_err(|_| E::invalid_value(Unexpected::Str(value), &self))
            }
        }
    }
    
    deserializer.deserialize_any(StringOrIntOptionVisitor)
}

#[async_trait]
impl SourceHandler for XtreamSourceHandler {
    fn source_type(&self) -> StreamSourceType {
        StreamSourceType::Xtream
    }

    async fn validate_source(&self, source: &StreamSource) -> AppResult<SourceValidationResult> {
        debug!("Validating Xtream source: {}", source.name);
        
        let mut result = self.validate_xtream_url(&source.url)?;

        // Check required authentication
        if source.username.is_none() {
            result.errors.push("Xtream source requires username".to_string());
            result.is_valid = false;
        }

        if source.password.is_none() {
            result.errors.push("Xtream source requires password".to_string());
            result.is_valid = false;
        }

        // If basic validation passed, test authentication
        if result.is_valid {
            match self.test_authentication(source).await {
                Ok(server_info) => {
                    if let Some(user_info) = &server_info.user_info {
                        result = result.with_context("user_status", user_info.status.clone());
                        result = result.with_context("max_connections", user_info.max_connections.clone().unwrap_or_default());
                        
                        if let Some(exp_date) = &user_info.exp_date {
                            result = result.with_context("expiry_date", exp_date.clone());
                        }
                    }
                    
                    if let Some(server_details) = &server_info.server_info {
                        if let Some(timezone) = &server_details.timezone {
                            result = result.with_context("server_timezone", timezone.clone());
                        }
                    }
                }
                Err(e) => {
                    result.errors.push(format!("Authentication test failed: {}", e));
                    result.is_valid = false;
                }
            }
        }

        Ok(result)
    }

    async fn get_capabilities(&self, source: &StreamSource) -> AppResult<SourceCapabilities> {
        // Test authentication to get detailed capabilities
        match self.test_authentication(source).await {
            Ok(server_info) => {
                let mut capabilities = SourceCapabilities::xtream_full();
                
                if let Some(user_info) = &server_info.user_info {
                    if let Some(max_conn_str) = &user_info.max_connections {
                        if let Ok(max_conn) = max_conn_str.parse::<u32>() {
                            capabilities.max_concurrent_connections = Some(max_conn);
                        }
                    }
                    
                    if let Some(formats) = &user_info.allowed_output_formats {
                        capabilities.supported_formats = formats.clone();
                    }
                }
                
                Ok(capabilities)
            }
            Err(_) => {
                // Return basic capabilities if authentication fails
                Ok(SourceCapabilities::xtream_full())
            }
        }
    }

    async fn test_connectivity(&self, source: &StreamSource) -> AppResult<bool> {
        match self.test_authentication(source).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn get_source_info(&self, source: &StreamSource) -> AppResult<HashMap<String, String>> {
        let mut info = HashMap::new();
        info.insert("source_type".to_string(), "Xtream".to_string());
        
        match self.test_authentication(source).await {
            Ok(server_info) => {
                if let Some(user_info) = &server_info.user_info {
                    info.insert("user_status".to_string(), user_info.status.clone());
                    if let Some(exp_date) = &user_info.exp_date {
                        info.insert("expiry_date".to_string(), exp_date.clone());
                    }
                    if let Some(max_conn) = &user_info.max_connections {
                        info.insert("max_connections".to_string(), max_conn.clone());
                    }
                    if let Some(active_conn) = &user_info.active_cons {
                        info.insert("active_connections".to_string(), active_conn.clone());
                    }
                }
                
                if let Some(server_details) = &server_info.server_info {
                    if let Some(timezone) = &server_details.timezone {
                        info.insert("server_timezone".to_string(), timezone.clone());
                    }
                    if let Some(timestamp) = &server_details.timestamp_now {
                        info.insert("server_timestamp".to_string(), timestamp.to_string());
                    }
                }
            }
            Err(e) => {
                info.insert("error".to_string(), e.to_string());
            }
        }
        
        Ok(info)
    }
}

#[async_trait]
impl ChannelIngestor for XtreamSourceHandler {
    async fn ingest_channels(&self, source: &StreamSource) -> AppResult<Vec<Channel>> {
        // Directly ingest channels without progress callbacks
        info!("Starting Xtream channel ingestion for source: {}", source.name);
        
        // Test authentication first
        let _server_info = self.test_authentication(source).await?;
        
        // Get live channels
        let xtream_channels = self.get_live_channels(source).await?;
        
        // Convert to internal format
        let channels: Vec<Channel> = xtream_channels.iter()
            .map(|ch| self.convert_xtream_channel(ch, source))
            .collect();
        
        info!("Successfully ingested {} channels from Xtream source: {}", channels.len(), source.name);
        Ok(channels)
    }


    async fn estimate_channel_count(&self, source: &StreamSource) -> AppResult<Option<u32>> {
        // For Xtream, we need to actually fetch the channel list to get the count
        // This is less efficient than M3U but more accurate
        match self.get_live_channels(source).await {
            Ok(channels) => Ok(Some(channels.len() as u32)),
            Err(_) => Ok(None),
        }
    }


}

#[async_trait]
impl HealthChecker for XtreamSourceHandler {
    async fn check_health(&self, source: &StreamSource) -> AppResult<SourceHealthStatus> {
        let start_time = std::time::Instant::now();
        let checked_at = Utc::now();

        match self.test_authentication(source).await {
            Ok(server_info) => {
                let response_time_ms = start_time.elapsed().as_millis() as u64;
                let is_healthy = server_info.user_info
                    .map(|u| u.status == "Active")
                    .unwrap_or(false);

                Ok(SourceHealthStatus {
                    is_healthy,
                    response_time_ms: Some(response_time_ms),
                    last_success: if is_healthy { Some(checked_at) } else { None },
                    error_message: if !is_healthy { 
                        Some("User account is not active".to_string()) 
                    } else { 
                        None 
                    },
                    checked_at,
                })
            }
            Err(e) => {
                Ok(SourceHealthStatus {
                    is_healthy: false,
                    response_time_ms: Some(start_time.elapsed().as_millis() as u64),
                    last_success: None,
                    error_message: Some(e.to_string()),
                    checked_at,
                })
            }
        }
    }

    async fn get_health_metrics(&self, source: &StreamSource) -> AppResult<SourceHealthMetrics> {
        let status = self.check_health(source).await?;
        let mut metrics = HashMap::new();

        if let Ok(source_info) = self.get_source_info(source).await {
            for (key, value) in source_info {
                metrics.insert(key, value);
            }
        }

        // Try to get channel count
        let channel_count = if status.is_healthy {
            self.estimate_channel_count(source).await.unwrap_or(None)
        } else {
            None
        };

        Ok(SourceHealthMetrics {
            status,
            channel_count,
            server_version: None, // Xtream doesn't typically provide version info
            uptime: None,
            metrics,
        })
    }
}

#[async_trait]
impl StreamUrlGenerator for XtreamSourceHandler {
    async fn generate_stream_url(
        &self,
        source: &StreamSource,
        channel_id: &str,
    ) -> AppResult<String> {
        Ok(self.generate_xtream_stream_url(source, channel_id))
    }

    async fn generate_stream_urls(
        &self,
        source: &StreamSource,
        channel_ids: &[String],
    ) -> AppResult<HashMap<String, String>> {
        let mut urls = HashMap::new();
        
        for channel_id in channel_ids {
            let url = self.generate_xtream_stream_url(source, channel_id);
            urls.insert(channel_id.clone(), url);
        }
        
        Ok(urls)
    }

    async fn validate_stream_url(
        &self,
        _source: &StreamSource,
        url: &str,
    ) -> AppResult<bool> {
        match self.raw_client.head(url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}

#[async_trait]
impl Authenticator for XtreamSourceHandler {
    async fn authenticate(&self, source: &StreamSource) -> AppResult<AuthenticationResult> {
        match self.test_authentication(source).await {
            Ok(server_info) => {
                if let Some(user_info) = server_info.user_info {
                    let success = user_info.status == "Active";
                    
                    Ok(AuthenticationResult {
                        success,
                        token: None, // Xtream uses username/password, not tokens
                        expires_at: user_info.exp_date.and_then(|d| {
                            // Try to parse the expiry date
                            // This is a simplified parser - real implementation would be more robust
                            chrono::DateTime::parse_from_str(&d, "%Y-%m-%d %H:%M:%S")
                                .ok()
                                .map(|dt| dt.with_timezone(&Utc))
                        }),
                        error_message: if !success { 
                            Some(format!("User status: {}", user_info.status)) 
                        } else { 
                            None 
                        },
                        metadata: {
                            let mut meta = HashMap::new();
                            meta.insert("username".to_string(), user_info.username);
                            meta.insert("status".to_string(), user_info.status);
                            if let Some(max_conn) = user_info.max_connections {
                                meta.insert("max_connections".to_string(), max_conn);
                            }
                            if let Some(active_conn) = user_info.active_cons {
                                meta.insert("active_connections".to_string(), active_conn);
                            }
                            meta
                        },
                    })
                } else {
                    Ok(AuthenticationResult {
                        success: false,
                        token: None,
                        expires_at: None,
                        error_message: Some("No user information returned from server".to_string()),
                        metadata: HashMap::new(),
                    })
                }
            }
            Err(e) => {
                Ok(AuthenticationResult {
                    success: false,
                    token: None,
                    expires_at: None,
                    error_message: Some(e.to_string()),
                    metadata: HashMap::new(),
                })
            }
        }
    }

    async fn refresh_authentication(&self, source: &StreamSource) -> AppResult<AuthenticationResult> {
        // For Xtream, refresh is the same as authenticate since it's stateless
        self.authenticate(source).await
    }

    async fn is_authenticated(&self, source: &StreamSource) -> AppResult<bool> {
        match self.authenticate(source).await {
            Ok(result) => Ok(result.success),
            Err(_) => Ok(false),
        }
    }
}

impl FullSourceHandler for XtreamSourceHandler {
    fn get_handler_summary(&self) -> SourceHandlerSummary {
        SourceHandlerSummary {
            source_type: StreamSourceType::Xtream,
            supports_ingestion: true,
            supports_health_check: true,
            supports_url_generation: true,
            supports_authentication: true,
        }
    }
}

impl Default for XtreamSourceHandler {
    fn default() -> Self {
        Self::new()
    }
}