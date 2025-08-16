//! Connection Limiter Service
//!
//! This module provides connection limit enforcement for streams and proxies
//! to prevent system overload and trigger error video generation when limits are exceeded.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Error types for connection limit violations
#[derive(Debug, Clone)]
pub enum LimitExceededError {
    ChannelClientLimit { 
        channel_id: String, 
        current: u32, 
        max: u32 
    },
    ProxyClientLimit { 
        proxy_id: String, 
        current: u32, 
        max: u32 
    },
    UpstreamSourceLimit { 
        source_url: String, 
        error: String 
    },
    StreamUnavailable { 
        reason: String 
    },
}

impl LimitExceededError {
    pub fn error_type(&self) -> &'static str {
        match self {
            LimitExceededError::ChannelClientLimit { .. } => "channel_client_limit",
            LimitExceededError::ProxyClientLimit { .. } => "proxy_client_limit", 
            LimitExceededError::UpstreamSourceLimit { .. } => "upstream_source_limit",
            LimitExceededError::StreamUnavailable { .. } => "stream_unavailable",
        }
    }
}

/// Configuration for connection limits
#[derive(Debug, Clone)]
pub struct ConnectionLimitsConfig {
    pub max_clients_per_channel: Option<u32>,
    pub max_clients_per_proxy: Option<u32>,
    pub enabled: bool,
}

impl Default for ConnectionLimitsConfig {
    fn default() -> Self {
        Self {
            max_clients_per_channel: Some(10),
            max_clients_per_proxy: Some(50),
            enabled: true,
        }
    }
}

/// Connection limiter service
pub struct ConnectionLimiter {
    config: ConnectionLimitsConfig,
    /// Key: channel_id or proxy_id, Value: current connection count
    active_connections: Arc<RwLock<HashMap<String, u32>>>,
}

impl ConnectionLimiter {
    pub fn new(config: ConnectionLimitsConfig) -> Self {
        Self {
            config,
            active_connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if a new connection can be accepted for a channel
    pub async fn check_channel_limit(&self, channel_id: &str) -> Result<(), LimitExceededError> {
        if !self.config.enabled {
            return Ok(());
        }

        if let Some(max_clients) = self.config.max_clients_per_channel {
            let connections = self.active_connections.read().await;
            let current = connections.get(channel_id).copied().unwrap_or(0);
            
            if current >= max_clients {
                return Err(LimitExceededError::ChannelClientLimit {
                    channel_id: channel_id.to_string(),
                    current,
                    max: max_clients,
                });
            }
        }

        Ok(())
    }

    /// Check if a new connection can be accepted for a proxy
    pub async fn check_proxy_limit(&self, proxy_id: &str) -> Result<(), LimitExceededError> {
        if !self.config.enabled {
            return Ok(());
        }

        if let Some(max_clients) = self.config.max_clients_per_proxy {
            let connections = self.active_connections.read().await;
            let current = connections.get(proxy_id).copied().unwrap_or(0);
            
            if current >= max_clients {
                return Err(LimitExceededError::ProxyClientLimit {
                    proxy_id: proxy_id.to_string(),
                    current,
                    max: max_clients,
                });
            }
        }

        Ok(())
    }

    /// Check both channel and proxy limits
    pub async fn check_limits(&self, proxy_id: &str, channel_id: &str) -> Result<(), LimitExceededError> {
        self.check_proxy_limit(proxy_id).await?;
        self.check_channel_limit(channel_id).await?;
        Ok(())
    }

    /// Register a new connection
    pub async fn register_connection(&self, proxy_id: &str, channel_id: &str) -> Result<ConnectionHandle, LimitExceededError> {
        // Check limits first
        self.check_limits(proxy_id, channel_id).await?;

        let mut connections = self.active_connections.write().await;
        
        // Increment proxy connection count
        let proxy_count = {
            let count = connections.entry(proxy_id.to_string()).or_insert(0);
            *count += 1;
            *count
        };
        
        // Increment channel connection count
        let channel_count = {
            let count = connections.entry(channel_id.to_string()).or_insert(0);
            *count += 1;
            *count
        };

        debug!("Registered connection - proxy {}: {}, channel {}: {}", 
               proxy_id, proxy_count, channel_id, channel_count);

        Ok(ConnectionHandle {
            limiter: self.active_connections.clone(),
            proxy_id: proxy_id.to_string(),
            channel_id: channel_id.to_string(),
        })
    }

    /// Get current connection counts
    pub async fn get_connection_counts(&self) -> HashMap<String, u32> {
        self.active_connections.read().await.clone()
    }

    /// Get current connection count for a specific resource
    pub async fn get_connection_count(&self, resource_id: &str) -> u32 {
        self.active_connections.read().await.get(resource_id).copied().unwrap_or(0)
    }
}

/// Handle for a registered connection that auto-decrements when dropped
pub struct ConnectionHandle {
    limiter: Arc<RwLock<HashMap<String, u32>>>,
    proxy_id: String,
    channel_id: String,
}

impl Drop for ConnectionHandle {
    fn drop(&mut self) {
        let limiter = self.limiter.clone();
        let proxy_id = self.proxy_id.clone();
        let channel_id = self.channel_id.clone();
        
        // Spawn task to decrement connection counts
        tokio::spawn(async move {
            let mut connections = limiter.write().await;
            
            // Decrement proxy count
            if let Some(count) = connections.get_mut(&proxy_id) {
                if *count > 0 {
                    *count -= 1;
                    if *count == 0 {
                        connections.remove(&proxy_id);
                    }
                }
            }
            
            // Decrement channel count  
            if let Some(count) = connections.get_mut(&channel_id) {
                if *count > 0 {
                    *count -= 1;
                    if *count == 0 {
                        connections.remove(&channel_id);
                    }
                }
            }

            debug!("Released connection - proxy: {}, channel: {}", proxy_id, channel_id);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_limits() {
        let config = ConnectionLimitsConfig {
            max_clients_per_channel: Some(2),
            max_clients_per_proxy: Some(3),
            enabled: true,
        };
        
        let limiter = ConnectionLimiter::new(config);
        
        // First connection should succeed
        let _handle1 = limiter.register_connection("proxy1", "channel1").await.unwrap();
        assert_eq!(limiter.get_connection_count("proxy1").await, 1);
        assert_eq!(limiter.get_connection_count("channel1").await, 1);
        
        // Second connection should succeed
        let _handle2 = limiter.register_connection("proxy1", "channel1").await.unwrap();
        assert_eq!(limiter.get_connection_count("channel1").await, 2);
        
        // Third connection should fail (channel limit exceeded)
        let result = limiter.register_connection("proxy1", "channel1").await;
        assert!(matches!(result, Err(LimitExceededError::ChannelClientLimit { .. })));
        
        // Drop handles and verify counts decrease
        drop(_handle1);
        drop(_handle2);
        
        // Give time for async drop to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        assert_eq!(limiter.get_connection_count("channel1").await, 0);
        assert_eq!(limiter.get_connection_count("proxy1").await, 0);
    }
}