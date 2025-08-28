use std::time::Duration;
use async_trait::async_trait;
use reqwest::{Client, Response};
use serde::de::DeserializeOwned;
use tracing::debug;

use crate::errors::{AppError, AppResult};
use crate::utils::{CompressionFormat, DecompressionService};
use crate::utils::url::UrlUtils;

/// HTTP client trait that provides automatic decompression for all content types
#[async_trait]
pub trait DecompressingHttpClient {
    /// Fetch URL and return decompressed text content
    async fn fetch_text(&self, url: &str) -> AppResult<String>;
    
    /// Fetch URL and return decompressed JSON content
    async fn fetch_json<T: DeserializeOwned + Send>(&self, url: &str) -> AppResult<T>;
    
    /// Fetch URL and return raw decompressed bytes
    async fn fetch_bytes(&self, url: &str) -> AppResult<Vec<u8>>;
    
    /// Fetch URL with custom headers and return decompressed text
    async fn fetch_text_with_headers(&self, url: &str, headers: &[(&str, &str)]) -> AppResult<String>;
    
    /// Test connectivity to URL (HEAD request)
    async fn test_connectivity(&self, url: &str) -> AppResult<bool>;
    
    /// Get underlying reqwest client for custom operations
    fn inner_client(&self) -> &Client;
}

/// Default implementation of DecompressingHttpClient using reqwest
pub struct StandardHttpClient {
    client: Client,
}

impl StandardHttpClient {
    /// Create new HTTP client with connection timeout only
    pub fn new() -> Self {
        Self::with_connection_timeout(Duration::from_secs(10))
    }
    
    /// Create new HTTP client with custom timeout (legacy, prefer connection_timeout)
    pub fn with_timeout(timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to create HTTP client");
            
        Self { client }
    }
    
    /// Create new HTTP client with only connection timeout (no total request timeout)
    /// This respects cancellation tokens and allows long data transfers
    pub fn with_connection_timeout(connect_timeout: Duration) -> Self {
        let client = Client::builder()
            .connect_timeout(connect_timeout)
            .build()
            .expect("Failed to create HTTP client");
            
        Self { client }
    }
    
    /// Process response with automatic decompression
    async fn process_response_to_bytes(response: Response, url: &str) -> AppResult<Vec<u8>> {
        if !response.status().is_success() {
            return Err(AppError::source_error(format!(
                "HTTP error: {} {} - URL: {}",
                response.status(),
                response.status().canonical_reason().unwrap_or("Unknown"),
                UrlUtils::obfuscate_credentials(url)
            )));
        }

        // Get raw bytes to detect compression
        let bytes = response
            .bytes()
            .await
            .map_err(|e| AppError::source_error(format!("Failed to read response: {e}")))?;

        debug!("Fetched {} bytes of raw content", bytes.len());

        // Detect compression format and decompress if needed
        let compression_format = DecompressionService::detect_compression_format(&bytes);
        debug!("Detected compression format: {:?}", compression_format);

        let decompressed_bytes = match compression_format {
            CompressionFormat::Uncompressed => {
                debug!("Content is uncompressed, using as-is");
                bytes.to_vec()
            }
            _ => {
                debug!("Content is compressed, decompressing...");
                DecompressionService::decompress(bytes)
                    .map_err(|e| AppError::source_error(format!("Failed to decompress content: {e}")))?
            }
        };

        debug!("Successfully processed {} bytes of content (compression: {:?})", 
               decompressed_bytes.len(), compression_format);
        
        Ok(decompressed_bytes)
    }
}

impl Default for StandardHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DecompressingHttpClient for StandardHttpClient {
    async fn fetch_text(&self, url: &str) -> AppResult<String> {
        debug!("Fetching text content from: {}", UrlUtils::obfuscate_credentials(url));
        
        let response = self.client
            .get(url)
            .send()
            .await
            .map_err(|e| {
                // Create a custom error message with obfuscated URL
                let error_msg = e.to_string();
                let obfuscated_msg = UrlUtils::obfuscate_credentials(&error_msg);
                AppError::ExternalService { 
                    service: "http_client".to_string(), 
                    message: obfuscated_msg 
                }
            })?;

        let decompressed_bytes = Self::process_response_to_bytes(response, url).await?;
        
        // Convert decompressed bytes to UTF-8 string
        let content = String::from_utf8(decompressed_bytes)
            .map_err(|e| AppError::source_error(format!("Failed to decode content as UTF-8: {e}")))?;

        debug!("Successfully fetched {} characters of text content", content.len());
        Ok(content)
    }
    
    async fn fetch_json<T: DeserializeOwned + Send>(&self, url: &str) -> AppResult<T> {
        debug!("Fetching JSON content from: {}", UrlUtils::obfuscate_credentials(url));
        
        let response = self.client
            .get(url)
            .send()
            .await
            .map_err(|e| {
                // Create a custom error message with obfuscated URL
                let error_msg = e.to_string();
                let obfuscated_msg = UrlUtils::obfuscate_credentials(&error_msg);
                AppError::ExternalService { 
                    service: "http_client".to_string(), 
                    message: obfuscated_msg 
                }
            })?;

        let decompressed_bytes = Self::process_response_to_bytes(response, url).await?;
        
        // Parse JSON from decompressed bytes
        let json_value = serde_json::from_slice(&decompressed_bytes)
            .map_err(|e| AppError::source_error(format!("Failed to parse JSON: {e}")))?;

        debug!("Successfully fetched and parsed JSON content");
        Ok(json_value)
    }
    
    async fn fetch_bytes(&self, url: &str) -> AppResult<Vec<u8>> {
        debug!("Fetching binary content from: {}", UrlUtils::obfuscate_credentials(url));
        
        let response = self.client
            .get(url)
            .send()
            .await
            .map_err(|e| {
                // Create a custom error message with obfuscated URL
                let error_msg = e.to_string();
                let obfuscated_msg = UrlUtils::obfuscate_credentials(&error_msg);
                AppError::ExternalService { 
                    service: "http_client".to_string(), 
                    message: obfuscated_msg 
                }
            })?;

        let decompressed_bytes = Self::process_response_to_bytes(response, url).await?;
        
        debug!("Successfully fetched {} bytes of binary content", decompressed_bytes.len());
        Ok(decompressed_bytes)
    }
    
    async fn fetch_text_with_headers(&self, url: &str, headers: &[(&str, &str)]) -> AppResult<String> {
        debug!("Fetching text content with headers from: {}", UrlUtils::obfuscate_credentials(url));
        
        let mut request = self.client.get(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        
        let response = request
            .send()
            .await
            .map_err(|e| {
                // Create a custom error message with obfuscated URL
                let error_msg = e.to_string();
                let obfuscated_msg = UrlUtils::obfuscate_credentials(&error_msg);
                AppError::ExternalService { 
                    service: "http_client".to_string(), 
                    message: obfuscated_msg 
                }
            })?;

        let decompressed_bytes = Self::process_response_to_bytes(response, url).await?;
        
        // Convert decompressed bytes to UTF-8 string
        let content = String::from_utf8(decompressed_bytes)
            .map_err(|e| AppError::source_error(format!("Failed to decode content as UTF-8: {e}")))?;

        debug!("Successfully fetched {} characters of text content with headers", content.len());
        Ok(content)
    }
    
    async fn test_connectivity(&self, url: &str) -> AppResult<bool> {
        match self.client.head(url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }
    
    fn inner_client(&self) -> &Client {
        &self.client
    }
}

/// Extended HTTP client that includes HTTPS/HTTP fallback capability
pub struct FallbackHttpClient {
    base_client: StandardHttpClient,
}

impl FallbackHttpClient {
    /// Create new fallback HTTP client with default timeout
    pub fn new() -> Self {
        Self {
            base_client: StandardHttpClient::new(),
        }
    }
    
    /// Create new fallback HTTP client with custom timeout
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            base_client: StandardHttpClient::with_timeout(timeout),
        }
    }
}

impl Default for FallbackHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DecompressingHttpClient for FallbackHttpClient {
    async fn fetch_text(&self, url: &str) -> AppResult<String> {
        // Try the original URL first
        match self.base_client.fetch_text(url).await {
            Ok(result) => Ok(result),
            Err(e) => {
                // If HTTPS failed and URL starts with https://, try HTTP fallback
                if url.starts_with("https://") {
                    debug!("HTTPS fetch failed, trying HTTP fallback");
                    
                    let http_url = url.replace("https://", "http://");
                    match self.base_client.fetch_text(&http_url).await {
                        Ok(result) => {
                            debug!("Successfully fetched content using HTTP fallback");
                            Ok(result)
                        }
                        Err(fallback_e) => {
                            Err(AppError::source_error(format!(
                                "Failed to fetch content: HTTPS error: {}, HTTP fallback error: {}",
                                UrlUtils::obfuscate_credentials(&e.to_string()),
                                UrlUtils::obfuscate_credentials(&fallback_e.to_string())
                            )))
                        }
                    }
                } else {
                    Err(e)
                }
            }
        }
    }
    
    async fn fetch_json<T: DeserializeOwned + Send>(&self, url: &str) -> AppResult<T> {
        // Try the original URL first
        match self.base_client.fetch_json(url).await {
            Ok(result) => Ok(result),
            Err(e) => {
                // If HTTPS failed and URL starts with https://, try HTTP fallback
                if url.starts_with("https://") {
                    debug!("HTTPS fetch failed, trying HTTP fallback");
                    
                    let http_url = url.replace("https://", "http://");
                    match self.base_client.fetch_json(&http_url).await {
                        Ok(result) => {
                            debug!("Successfully fetched content using HTTP fallback");
                            Ok(result)
                        }
                        Err(fallback_e) => {
                            Err(AppError::source_error(format!(
                                "Failed to fetch content: HTTPS error: {}, HTTP fallback error: {}",
                                UrlUtils::obfuscate_credentials(&e.to_string()),
                                UrlUtils::obfuscate_credentials(&fallback_e.to_string())
                            )))
                        }
                    }
                } else {
                    Err(e)
                }
            }
        }
    }
    
    async fn fetch_bytes(&self, url: &str) -> AppResult<Vec<u8>> {
        // Try the original URL first
        match self.base_client.fetch_bytes(url).await {
            Ok(result) => Ok(result),
            Err(e) => {
                // If HTTPS failed and URL starts with https://, try HTTP fallback
                if url.starts_with("https://") {
                    debug!("HTTPS fetch failed, trying HTTP fallback");
                    
                    let http_url = url.replace("https://", "http://");
                    match self.base_client.fetch_bytes(&http_url).await {
                        Ok(result) => {
                            debug!("Successfully fetched content using HTTP fallback");
                            Ok(result)
                        }
                        Err(fallback_e) => {
                            Err(AppError::source_error(format!(
                                "Failed to fetch content: HTTPS error: {}, HTTP fallback error: {}",
                                UrlUtils::obfuscate_credentials(&e.to_string()),
                                UrlUtils::obfuscate_credentials(&fallback_e.to_string())
                            )))
                        }
                    }
                } else {
                    Err(e)
                }
            }
        }
    }
    
    async fn fetch_text_with_headers(&self, url: &str, headers: &[(&str, &str)]) -> AppResult<String> {
        // Try the original URL first
        match self.base_client.fetch_text_with_headers(url, headers).await {
            Ok(result) => Ok(result),
            Err(e) => {
                // If HTTPS failed and URL starts with https://, try HTTP fallback
                if url.starts_with("https://") {
                    debug!("HTTPS fetch failed, trying HTTP fallback");
                    
                    let http_url = url.replace("https://", "http://");
                    match self.base_client.fetch_text_with_headers(&http_url, headers).await {
                        Ok(result) => {
                            debug!("Successfully fetched content using HTTP fallback");
                            Ok(result)
                        }
                        Err(fallback_e) => {
                            Err(AppError::source_error(format!(
                                "Failed to fetch content: HTTPS error: {}, HTTP fallback error: {}",
                                UrlUtils::obfuscate_credentials(&e.to_string()),
                                UrlUtils::obfuscate_credentials(&fallback_e.to_string())
                            )))
                        }
                    }
                } else {
                    Err(e)
                }
            }
        }
    }
    
    async fn test_connectivity(&self, url: &str) -> AppResult<bool> {
        self.base_client.test_connectivity(url).await
    }
    
    fn inner_client(&self) -> &Client {
        self.base_client.inner_client()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use flate2::write::GzEncoder;
    use flate2::Compression;

    #[tokio::test]
    async fn test_decompression_detection() {
        let original_data = "Hello, world!";
        
        // Create gzip compressed data
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original_data.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();
        
        // Test detection
        let format = DecompressionService::detect_compression_format(&compressed);
        assert_eq!(format, CompressionFormat::Gzip);
        
        // Test decompression
        let bytes = bytes::Bytes::from(compressed);
        let decompressed = DecompressionService::decompress(bytes).unwrap();
        let result = String::from_utf8(decompressed).unwrap();
        assert_eq!(result, original_data);
    }
}