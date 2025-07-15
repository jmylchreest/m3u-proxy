//! Session tracking and statistics for proxy streams
//!
//! This module provides comprehensive session tracking with detailed logging
//! and periodic statistics reporting for proxy streaming sessions.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Client information for session tracking
#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub ip: String,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
}

/// Session statistics for a streaming session
#[derive(Debug, Clone)]
pub struct SessionStats {
    pub session_id: String,
    pub client_info: ClientInfo,
    pub proxy_id: String,
    pub proxy_name: String,
    pub channel_id: String,
    pub channel_name: String,
    pub start_time: Instant,
    pub last_activity: Instant,
    pub bytes_served: u64,
    pub chunks_served: u64,
    pub errors: u64,
    pub upstream_url: String,
    pub connection_attempts: u32,
    pub last_error: Option<String>,
}

impl SessionStats {
    pub fn new(
        session_id: String,
        client_info: ClientInfo,
        proxy_id: String,
        proxy_name: String,
        channel_id: String,
        channel_name: String,
        upstream_url: String,
    ) -> Self {
        let now = Instant::now();
        Self {
            session_id,
            client_info,
            proxy_id,
            proxy_name,
            channel_id,
            channel_name,
            start_time: now,
            last_activity: now,
            bytes_served: 0,
            chunks_served: 0,
            errors: 0,
            upstream_url,
            connection_attempts: 0,
            last_error: None,
        }
    }

    pub fn update_bytes_served(&mut self, bytes: u64) {
        self.bytes_served += bytes;
        self.chunks_served += 1;
        self.last_activity = Instant::now();
    }

    pub fn record_error(&mut self, error: String) {
        self.errors += 1;
        self.last_error = Some(error);
        self.last_activity = Instant::now();
    }

    pub fn record_connection_attempt(&mut self) {
        self.connection_attempts += 1;
        self.last_activity = Instant::now();
    }

    pub fn duration(&self) -> Duration {
        self.start_time.elapsed()
    }

    pub fn average_bitrate_kbps(&self) -> f64 {
        let duration_secs = self.duration().as_secs_f64();
        if duration_secs > 0.0 {
            (self.bytes_served as f64 * 8.0) / (duration_secs * 1000.0) // Convert to kbps
        } else {
            0.0
        }
    }

    pub fn format_duration(&self) -> String {
        let duration = self.duration();
        let total_seconds = duration.as_secs();
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        
        if hours > 0 {
            format!("{}h{:02}m{:02}s", hours, minutes, seconds)
        } else if minutes > 0 {
            format!("{}m{:02}s", minutes, seconds)
        } else {
            format!("{}s", seconds)
        }
    }

    pub fn format_bytes(&self) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = self.bytes_served as f64;
        let mut unit_index = 0;
        
        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }
        
        format!("{:.2} {}", size, UNITS[unit_index])
    }
}

/// Session tracker for managing and monitoring proxy sessions
pub struct SessionTracker {
    sessions: Arc<RwLock<HashMap<String, SessionStats>>>,
    stats_interval: Duration,
    cleanup_interval: Duration,
    session_timeout: Duration,
}

impl SessionTracker {
    pub fn new(
        stats_interval: Duration,
        cleanup_interval: Duration,
        session_timeout: Duration,
    ) -> Self {
        let tracker = Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            stats_interval,
            cleanup_interval,
            session_timeout,
        };
        
        // Start background tasks
        tracker.start_stats_reporter();
        tracker.start_session_cleanup();
        
        tracker
    }

    /// Start a new streaming session
    pub async fn start_session(&self, session_stats: SessionStats) {
        let session_id = session_stats.session_id.clone();
        
        debug!(
            "Starting proxy session: {} | Client: {} | Proxy: {} ({}) | Channel: {} ({}) | Upstream: {}",
            session_id,
            session_stats.client_info.ip,
            session_stats.proxy_name,
            session_stats.proxy_id,
            session_stats.channel_name,
            session_stats.channel_id,
            session_stats.upstream_url
        );
        
        if let Some(user_agent) = &session_stats.client_info.user_agent {
            debug!("Client User-Agent: {} | Session: {}", user_agent, session_id);
        }
        
        if let Some(referer) = &session_stats.client_info.referer {
            debug!("Client Referer: {} | Session: {}", referer, session_id);
        }
        
        self.sessions.write().await.insert(session_id, session_stats);
    }

    /// Update session with bytes served
    pub async fn update_session_bytes(&self, session_id: &str, bytes: u64) {
        if let Some(session) = self.sessions.write().await.get_mut(session_id) {
            session.update_bytes_served(bytes);
        }
    }

    /// Record an error for a session
    pub async fn record_session_error(&self, session_id: &str, error: String) {
        if let Some(session) = self.sessions.write().await.get_mut(session_id) {
            session.record_error(error);
            
            warn!(
                "Session error: {} | Client: {} | Proxy: {} | Channel: {} | Error: {}",
                session_id,
                session.client_info.ip,
                session.proxy_name,
                session.channel_name,
                session.last_error.as_ref().unwrap_or(&"Unknown".to_string())
            );
        }
    }

    /// Record a connection attempt
    pub async fn record_connection_attempt(&self, session_id: &str) {
        if let Some(session) = self.sessions.write().await.get_mut(session_id) {
            session.record_connection_attempt();
            
            debug!(
                "Connection attempt #{} | Session: {} | Client: {} | Proxy: {} | Channel: {}",
                session.connection_attempts,
                session_id,
                session.client_info.ip,
                session.proxy_name,
                session.channel_name
            );
        }
    }

    /// End a streaming session
    pub async fn end_session(&self, session_id: &str) {
        if let Some(session) = self.sessions.write().await.remove(session_id) {
            info!(
                "Session ended: {} | Duration: {} | Data: {} | Avg bitrate: {:.2} kbps | Chunks: {} | Errors: {} | Client: {} | Proxy: {} | Channel: {}",
                session_id,
                session.format_duration(),
                session.format_bytes(),
                session.average_bitrate_kbps(),
                session.chunks_served,
                session.errors,
                session.client_info.ip,
                session.proxy_name,
                session.channel_name
            );
        }
    }

    /// Get current session statistics
    pub async fn get_session_stats(&self, session_id: &str) -> Option<SessionStats> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Get all active sessions
    pub async fn get_active_sessions(&self) -> Vec<SessionStats> {
        self.sessions.read().await.values().cloned().collect()
    }

    /// Get session count by proxy
    pub async fn get_proxy_session_counts(&self) -> HashMap<String, usize> {
        let sessions = self.sessions.read().await;
        let mut counts = HashMap::new();
        
        for session in sessions.values() {
            *counts.entry(session.proxy_id.clone()).or_insert(0) += 1;
        }
        
        counts
    }

    /// Get session count by channel
    pub async fn get_channel_session_counts(&self) -> HashMap<String, usize> {
        let sessions = self.sessions.read().await;
        let mut counts = HashMap::new();
        
        for session in sessions.values() {
            *counts.entry(session.channel_id.clone()).or_insert(0) += 1;
        }
        
        counts
    }

    /// Start periodic statistics reporting
    fn start_stats_reporter(&self) {
        let sessions = self.sessions.clone();
        let interval = self.stats_interval;
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval);
            
            loop {
                interval.tick().await;
                
                let sessions_read = sessions.read().await;
                let session_count = sessions_read.len();
                
                if session_count == 0 {
                    continue;
                }
                
                // Calculate aggregate statistics
                let mut total_bytes = 0u64;
                let mut total_chunks = 0u64;
                let mut total_errors = 0u64;
                let mut proxy_counts = HashMap::new();
                let mut channel_counts = HashMap::new();
                let mut client_counts = HashMap::new();
                
                for session in sessions_read.values() {
                    total_bytes += session.bytes_served;
                    total_chunks += session.chunks_served;
                    total_errors += session.errors;
                    
                    *proxy_counts.entry(session.proxy_name.clone()).or_insert(0) += 1;
                    *channel_counts.entry(session.channel_name.clone()).or_insert(0) += 1;
                    *client_counts.entry(session.client_info.ip.clone()).or_insert(0) += 1;
                }
                
                // Format total bytes
                let total_bytes_formatted = {
                    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
                    let mut size = total_bytes as f64;
                    let mut unit_index = 0;
                    
                    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
                        size /= 1024.0;
                        unit_index += 1;
                    }
                    
                    format!("{:.2} {}", size, UNITS[unit_index])
                };
                
                info!(
                    "Session Summary: {} active sessions | Total data: {} | Chunks: {} | Errors: {} | Proxies: {} | Channels: {} | Clients: {}",
                    session_count,
                    total_bytes_formatted,
                    total_chunks,
                    total_errors,
                    proxy_counts.len(),
                    channel_counts.len(),
                    client_counts.len()
                );
                
                // Log top proxies by session count
                if !proxy_counts.is_empty() {
                    let mut proxy_vec: Vec<_> = proxy_counts.iter().collect();
                    proxy_vec.sort_by(|a, b| b.1.cmp(a.1));
                    
                    let top_proxies: Vec<String> = proxy_vec.iter()
                        .take(5)
                        .map(|(name, count)| format!("{}: {}", name, count))
                        .collect();
                    
                    debug!("Top proxies by sessions: [{}]", top_proxies.join(", "));
                }
                
                // Log top channels by session count
                if !channel_counts.is_empty() {
                    let mut channel_vec: Vec<_> = channel_counts.iter().collect();
                    channel_vec.sort_by(|a, b| b.1.cmp(a.1));
                    
                    let top_channels: Vec<String> = channel_vec.iter()
                        .take(5)
                        .map(|(name, count)| format!("{}: {}", name, count))
                        .collect();
                    
                    debug!("Top channels by sessions: [{}]", top_channels.join(", "));
                }
                
                // Log detailed per-session statistics (for debug level)
                for session in sessions_read.values() {
                    if session.duration().as_secs() > 0 {
                        debug!(
                            "Session detail: {} | Duration: {} | Data: {} | Bitrate: {:.2} kbps | Client: {} | Proxy: {} | Channel: {}",
                            session.session_id,
                            session.format_duration(),
                            session.format_bytes(),
                            session.average_bitrate_kbps(),
                            session.client_info.ip,
                            session.proxy_name,
                            session.channel_name
                        );
                    }
                }
            }
        });
    }

    /// Start session cleanup task
    fn start_session_cleanup(&self) {
        let sessions = self.sessions.clone();
        let cleanup_interval = self.cleanup_interval;
        let session_timeout = self.session_timeout;
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(cleanup_interval);
            
            loop {
                interval.tick().await;
                
                let mut sessions_write = sessions.write().await;
                let mut to_remove = Vec::new();
                
                for (session_id, session) in sessions_write.iter() {
                    if session.last_activity.elapsed() > session_timeout {
                        to_remove.push(session_id.clone());
                    }
                }
                
                for session_id in to_remove {
                    if let Some(session) = sessions_write.remove(&session_id) {
                        warn!(
                            "Session timeout: {} | Duration: {} | Data: {} | Client: {} | Proxy: {} | Channel: {}",
                            session_id,
                            session.format_duration(),
                            session.format_bytes(),
                            session.client_info.ip,
                            session.proxy_name,
                            session.channel_name
                        );
                    }
                }
            }
        });
    }
}

impl Default for SessionTracker {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(30),  // Stats every 30 seconds
            Duration::from_secs(60),  // Cleanup every minute
            Duration::from_secs(300), // 5 minute session timeout
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_stats_formatting() {
        let client_info = ClientInfo {
            ip: "127.0.0.1".to_string(),
            user_agent: Some("Test Agent".to_string()),
            referer: None,
        };
        
        let mut stats = SessionStats::new(
            "test-session".to_string(),
            client_info,
            "proxy-1".to_string(),
            "Test Proxy".to_string(),
            "channel-1".to_string(),
            "Test Channel".to_string(),
            "http://example.com/stream".to_string(),
        );
        
        stats.update_bytes_served(1024 * 1024 * 5); // 5MB
        
        assert_eq!(stats.format_bytes(), "5.00 MB");
        assert!(stats.average_bitrate_kbps() > 0.0);
    }

    #[tokio::test]
    async fn test_session_tracking() {
        let tracker = SessionTracker::new(
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_secs(5),
        );
        
        let client_info = ClientInfo {
            ip: "127.0.0.1".to_string(),
            user_agent: Some("Test Agent".to_string()),
            referer: None,
        };
        
        let session_stats = SessionStats::new(
            "test-session".to_string(),
            client_info,
            "proxy-1".to_string(),
            "Test Proxy".to_string(),
            "channel-1".to_string(),
            "Test Channel".to_string(),
            "http://example.com/stream".to_string(),
        );
        
        tracker.start_session(session_stats).await;
        
        // Update session
        tracker.update_session_bytes("test-session", 1024).await;
        
        // Verify session exists
        let stats = tracker.get_session_stats("test-session").await;
        assert!(stats.is_some());
        assert_eq!(stats.unwrap().bytes_served, 1024);
        
        // End session
        tracker.end_session("test-session").await;
        
        // Verify session is gone
        let stats = tracker.get_session_stats("test-session").await;
        assert!(stats.is_none());
    }
}