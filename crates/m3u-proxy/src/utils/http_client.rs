use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use reqwest::{Client, Response};
use serde::de::DeserializeOwned;
use tracing::debug;

use crate::errors::{AppError, AppResult};
use crate::utils::{CompressionFormat, DecompressionService, CircuitBreaker};
use crate::utils::url::UrlUtils;
use crate::services::CircuitBreakerManager;

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
#[derive(Clone)]
pub struct StandardHttpClient {
    client: Client,
    circuit_breaker: Option<Arc<crate::utils::ConcreteCircuitBreaker>>,
    acceptable_status_codes: Vec<String>,
}

impl StandardHttpClient {
    /// Create new HTTP client (factory use only)
    pub(crate) fn new(
        connect_timeout: Duration,
        circuit_breaker: Option<Arc<crate::utils::ConcreteCircuitBreaker>>,
        user_agent: &str,
        acceptable_status_codes: Vec<String>,
    ) -> Self {
        let client = Client::builder()
            .connect_timeout(connect_timeout)
            .user_agent(user_agent)
            .build()
            .expect("Failed to create HTTP client");
            
        Self { 
            client,
            circuit_breaker,
            acceptable_status_codes,
        }
    }


    /// Create new HTTP client with circuit breaker protection
    pub async fn with_circuit_breaker_manager(
        connect_timeout: Duration,
        circuit_breaker_manager: Arc<CircuitBreakerManager>,
        service_name: &str,
    ) -> Result<Self, String> {
        let client = Client::builder()
            .connect_timeout(connect_timeout)
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        // Get circuit breaker from centralized manager
        let circuit_breaker = circuit_breaker_manager
            .get_circuit_breaker(service_name)
            .await?;
            
        Ok(Self { 
            client,
            circuit_breaker: Some(circuit_breaker),
            acceptable_status_codes: vec!["2xx".to_string(), "3xx".to_string()], // Default
        })
    }

    /// Fetch a logo URL with circuit breaker protection (returns raw bytes)
    pub async fn fetch_logo(&self, logo_url: &str) -> Result<bytes::Bytes, String> {
        debug!("Attempting to fetch logo: {}", UrlUtils::obfuscate_credentials(logo_url));

        let request_fn = || async {
            let response = self.client
                .get(logo_url)
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {}", e))?;

            // Check if status code is acceptable based on configuration
            if !crate::utils::is_status_acceptable(&response.status(), &self.acceptable_status_codes) {
                return Err(format!("HTTP error: {}", response.status()));
            }

            let content_length = response.content_length().unwrap_or(0);
            if content_length > 5_000_000 { // 5MB limit
                return Err("Logo file too large".to_string());
            }

            let bytes = response
                .bytes()
                .await
                .map_err(|e| format!("Failed to read response body: {}", e))?;

            Ok(bytes)
        };

        if let Some(circuit_breaker) = &self.circuit_breaker {
            // Use circuit breaker protection
            let cb_result = circuit_breaker.as_ref().execute(request_fn).await;
            match cb_result.result {
                Ok(bytes) => {
                    debug!("Successfully fetched logo: {} ({} bytes, took {:?}, CB state: {:?})", 
                           UrlUtils::obfuscate_credentials(logo_url), bytes.len(), cb_result.execution_time, cb_result.state);
                    Ok(bytes)
                },
                Err(crate::utils::circuit_breaker::CircuitBreakerError::CircuitOpen) => {
                    debug!("Logo fetch blocked by circuit breaker: {} (state: {:?})", UrlUtils::obfuscate_credentials(logo_url), cb_result.state);
                    Err("Circuit breaker open - logo service unavailable".to_string())
                },
                Err(crate::utils::circuit_breaker::CircuitBreakerError::ServiceError(e)) => {
                    debug!("Logo fetch failed: {} - {} (CB state: {:?}, took {:?})", UrlUtils::obfuscate_credentials(logo_url), e, cb_result.state, cb_result.execution_time);
                    Err(format!("Logo fetch error: {}", e))
                },
                Err(crate::utils::circuit_breaker::CircuitBreakerError::Timeout) => {
                    debug!("Logo fetch timed out: {} (CB state: {:?}, took {:?})", UrlUtils::obfuscate_credentials(logo_url), cb_result.state, cb_result.execution_time);
                    Err("Logo fetch timeout".to_string())
                },
            }
        } else {
            // No circuit breaker - direct request
            request_fn().await
        }
    }

    /// Check if the circuit breaker is available (for monitoring)
    pub async fn is_available(&self) -> bool {
        if let Some(circuit_breaker) = &self.circuit_breaker {
            circuit_breaker.as_ref().is_available().await
        } else {
            true // Always available if no circuit breaker
        }
    }

    /// Get circuit breaker statistics (if enabled)
    pub async fn stats(&self) -> Option<crate::utils::circuit_breaker::CircuitBreakerStats> {
        if let Some(circuit_breaker) = &self.circuit_breaker {
            Some(circuit_breaker.as_ref().stats().await)
        } else {
            None
        }
    }

    /// Force circuit breaker open (for testing)
    pub async fn force_circuit_open(&self) {
        if let Some(circuit_breaker) = &self.circuit_breaker {
            circuit_breaker.as_ref().force_open().await;
        }
    }

    /// Force circuit breaker closed (for testing)
    pub async fn force_circuit_closed(&self) {
        if let Some(circuit_breaker) = &self.circuit_breaker {
            circuit_breaker.as_ref().force_closed().await;
        }
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


#[async_trait]
impl DecompressingHttpClient for StandardHttpClient {
    async fn fetch_text(&self, url: &str) -> AppResult<String> {
        debug!("Fetching text content from: {}", UrlUtils::obfuscate_credentials(url));
        
        let request_fn = || async {
            self.client
                .get(url)
                .send()
                .await
                .map_err(|e| {
                    // Create a custom error message with obfuscated URL
                    let error_msg = e.to_string();
                    let obfuscated_msg = UrlUtils::obfuscate_credentials(&error_msg);
                    format!("HTTP request failed: {}", obfuscated_msg)
                })
        };

        let response = if let Some(circuit_breaker) = &self.circuit_breaker {
            // Use circuit breaker protection
            let cb_result = circuit_breaker.as_ref().execute(request_fn).await;
            match cb_result.result {
                Ok(response) => response,
                Err(crate::utils::circuit_breaker::CircuitBreakerError::CircuitOpen) => {
                    return Err(AppError::ExternalService { 
                        service: "http_client".to_string(), 
                        message: "Circuit breaker is open - too many failures".to_string()
                    });
                }
                Err(crate::utils::circuit_breaker::CircuitBreakerError::Timeout) => {
                    return Err(AppError::ExternalService { 
                        service: "http_client".to_string(), 
                        message: "Request timed out".to_string()
                    });
                }
                Err(crate::utils::circuit_breaker::CircuitBreakerError::ServiceError(msg)) => {
                    return Err(AppError::ExternalService { 
                        service: "http_client".to_string(), 
                        message: msg
                    });
                }
            }
        } else {
            // No circuit breaker - direct request
            request_fn().await.map_err(|e| {
                AppError::ExternalService { 
                    service: "http_client".to_string(), 
                    message: e 
                }
            })?
        };

        let decompressed_bytes = Self::process_response_to_bytes(response, url).await?;
        
        // Convert decompressed bytes to UTF-8 string
        let content = String::from_utf8(decompressed_bytes)
            .map_err(|e| AppError::source_error(format!("Failed to decode content as UTF-8: {e}")))?;

        debug!("Successfully fetched {} characters of text content", content.len());
        Ok(content)
    }
    
    async fn fetch_json<T: DeserializeOwned + Send>(&self, url: &str) -> AppResult<T> {
        debug!("Fetching JSON content from: {}", UrlUtils::obfuscate_credentials(url));
        
        let request_fn = || async {
            self.client
                .get(url)
                .send()
                .await
                .map_err(|e| {
                    let error_msg = e.to_string();
                    let obfuscated_msg = UrlUtils::obfuscate_credentials(&error_msg);
                    format!("HTTP request failed: {}", obfuscated_msg)
                })
        };

        let response = if let Some(circuit_breaker) = &self.circuit_breaker {
            let cb_result = circuit_breaker.as_ref().execute(request_fn).await;
            match cb_result.result {
                Ok(response) => {
                    debug!("JSON request successful: {} (CB state: {:?}, took {:?})", 
                           UrlUtils::obfuscate_credentials(url), cb_result.state, cb_result.execution_time);
                    response
                },
                Err(crate::utils::circuit_breaker::CircuitBreakerError::CircuitOpen) => {
                    return Err(AppError::ExternalService { 
                        service: "http_client".to_string(), 
                        message: "Circuit breaker is open - too many failures".to_string()
                    });
                }
                Err(crate::utils::circuit_breaker::CircuitBreakerError::Timeout) => {
                    return Err(AppError::ExternalService { 
                        service: "http_client".to_string(), 
                        message: "Request timed out".to_string()
                    });
                }
                Err(crate::utils::circuit_breaker::CircuitBreakerError::ServiceError(msg)) => {
                    return Err(AppError::ExternalService { 
                        service: "http_client".to_string(), 
                        message: msg
                    });
                }
            }
        } else {
            request_fn().await.map_err(|e| {
                AppError::ExternalService { 
                    service: "http_client".to_string(), 
                    message: e 
                }
            })?
        };

        let decompressed_bytes = Self::process_response_to_bytes(response, url).await?;
        
        // Parse JSON from decompressed bytes
        let json_value = serde_json::from_slice(&decompressed_bytes)
            .map_err(|e| AppError::source_error(format!("Failed to parse JSON: {e}")))?;

        debug!("Successfully fetched and parsed JSON content");
        Ok(json_value)
    }
    
    async fn fetch_bytes(&self, url: &str) -> AppResult<Vec<u8>> {
        debug!("Fetching binary content from: {}", UrlUtils::obfuscate_credentials(url));
        
        let request_fn = || async {
            self.client
                .get(url)
                .send()
                .await
                .map_err(|e| {
                    let error_msg = e.to_string();
                    let obfuscated_msg = UrlUtils::obfuscate_credentials(&error_msg);
                    format!("HTTP request failed: {}", obfuscated_msg)
                })
        };

        let response = if let Some(circuit_breaker) = &self.circuit_breaker {
            let cb_result = circuit_breaker.as_ref().execute(request_fn).await;
            match cb_result.result {
                Ok(response) => {
                    debug!("Binary request successful: {} (CB state: {:?}, took {:?})", 
                           UrlUtils::obfuscate_credentials(url), cb_result.state, cb_result.execution_time);
                    response
                },
                Err(crate::utils::circuit_breaker::CircuitBreakerError::CircuitOpen) => {
                    return Err(AppError::ExternalService { 
                        service: "http_client".to_string(), 
                        message: "Circuit breaker is open - too many failures".to_string()
                    });
                }
                Err(crate::utils::circuit_breaker::CircuitBreakerError::Timeout) => {
                    return Err(AppError::ExternalService { 
                        service: "http_client".to_string(), 
                        message: "Request timed out".to_string()
                    });
                }
                Err(crate::utils::circuit_breaker::CircuitBreakerError::ServiceError(msg)) => {
                    return Err(AppError::ExternalService { 
                        service: "http_client".to_string(), 
                        message: msg
                    });
                }
            }
        } else {
            request_fn().await.map_err(|e| {
                AppError::ExternalService { 
                    service: "http_client".to_string(), 
                    message: e 
                }
            })?
        };

        let decompressed_bytes = Self::process_response_to_bytes(response, url).await?;
        
        debug!("Successfully fetched {} bytes of binary content", decompressed_bytes.len());
        Ok(decompressed_bytes)
    }
    
    async fn fetch_text_with_headers(&self, url: &str, headers: &[(&str, &str)]) -> AppResult<String> {
        debug!("Fetching text content with headers from: {}", UrlUtils::obfuscate_credentials(url));
        
        let headers_clone: Vec<(String, String)> = headers.iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        
        let request_fn = || {
            let url = url.to_string();
            let headers = headers_clone.clone();
            async move {
                let mut request = self.client.get(&url);
                for (name, value) in headers {
                    request = request.header(name, value);
                }
                
                request.send()
                    .await
                    .map_err(|e| {
                        let error_msg = e.to_string();
                        let obfuscated_msg = UrlUtils::obfuscate_credentials(&error_msg);
                        format!("HTTP request failed: {}", obfuscated_msg)
                    })
            }
        };

        let response = if let Some(circuit_breaker) = &self.circuit_breaker {
            let cb_result = circuit_breaker.as_ref().execute(request_fn).await;
            match cb_result.result {
                Ok(response) => {
                    debug!("Text with headers request successful: {} (CB state: {:?}, took {:?})", 
                           UrlUtils::obfuscate_credentials(url), cb_result.state, cb_result.execution_time);
                    response
                },
                Err(crate::utils::circuit_breaker::CircuitBreakerError::CircuitOpen) => {
                    return Err(AppError::ExternalService { 
                        service: "http_client".to_string(), 
                        message: "Circuit breaker is open - too many failures".to_string()
                    });
                }
                Err(crate::utils::circuit_breaker::CircuitBreakerError::Timeout) => {
                    return Err(AppError::ExternalService { 
                        service: "http_client".to_string(), 
                        message: "Request timed out".to_string()
                    });
                }
                Err(crate::utils::circuit_breaker::CircuitBreakerError::ServiceError(msg)) => {
                    return Err(AppError::ExternalService { 
                        service: "http_client".to_string(), 
                        message: msg
                    });
                }
            }
        } else {
            request_fn().await.map_err(|e| {
                AppError::ExternalService { 
                    service: "http_client".to_string(), 
                    message: e 
                }
            })?
        };

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
    /// Create new fallback HTTP client (factory use only)
    pub(crate) fn new(
        connect_timeout: Duration,
        circuit_breaker: Option<Arc<crate::utils::ConcreteCircuitBreaker>>,
        user_agent: &str,
        acceptable_status_codes: Vec<String>,
    ) -> Self {
        Self {
            base_client: StandardHttpClient::new(connect_timeout, circuit_breaker, user_agent, acceptable_status_codes),
        }
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