use chrono::{DateTime, Utc};
use uuid::Uuid;
use tracing::{info, error};

/// Stream access metrics for logging and analytics
#[derive(Debug, Clone)]
pub struct StreamAccessMetrics {
    pub proxy_ulid: String,
    pub channel_id: Uuid,
    pub client_ip: String,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub bytes_served: u64,
    pub relay_used: bool,
    pub relay_config_id: Option<Uuid>,
}

/// Metrics logger service
#[derive(Clone)]
pub struct MetricsLogger {
    // For now we'll use structured logging
    // In the future this could write to database or external analytics service
}

impl MetricsLogger {
    pub fn new() -> Self {
        Self {}
    }
    
    /// Log a stream access event
    pub async fn log_stream_access(&self, metrics: StreamAccessMetrics) {
        info!(
            proxy_ulid = %metrics.proxy_ulid,
            channel_id = %metrics.channel_id,
            client_ip = %metrics.client_ip,
            user_agent = ?metrics.user_agent,
            referer = ?metrics.referer,
            start_time = %metrics.start_time,
            end_time = ?metrics.end_time,
            bytes_served = metrics.bytes_served,
            relay_used = metrics.relay_used,
            relay_config_id = ?metrics.relay_config_id,
            "Stream access logged"
        );
        
        // TODO: Implement database storage
        // INSERT INTO stream_access_logs (proxy_ulid, channel_id, client_ip, ...)
        // VALUES (?, ?, ?, ...)
    }
    
    /// Log stream start
    pub async fn log_stream_start(
        &self,
        proxy_ulid: String,
        channel_id: Uuid,
        client_ip: String,
        user_agent: Option<String>,
        referer: Option<String>,
    ) -> StreamAccessSession {
        let session = StreamAccessSession {
            proxy_ulid: proxy_ulid.clone(),
            channel_id,
            client_ip: client_ip.clone(),
            user_agent: user_agent.clone(),
            referer: referer.clone(),
            start_time: Utc::now(),
            bytes_served: 0,
            relay_used: false,
            relay_config_id: None,
        };
        
        info!(
            proxy_ulid = %proxy_ulid,
            channel_id = %channel_id,
            client_ip = %client_ip,
            user_agent = ?user_agent,
            "Stream session started"
        );
        
        session
    }
}

/// Active stream access session for tracking
#[derive(Debug, Clone)]
pub struct StreamAccessSession {
    pub proxy_ulid: String,
    pub channel_id: Uuid,
    pub client_ip: String,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub start_time: DateTime<Utc>,
    pub bytes_served: u64,
    pub relay_used: bool,
    pub relay_config_id: Option<Uuid>,
}

impl StreamAccessSession {
    /// Finish the session and log final metrics
    pub async fn finish(self, metrics_logger: &MetricsLogger, bytes_served: u64) {
        let metrics = StreamAccessMetrics {
            proxy_ulid: self.proxy_ulid,
            channel_id: self.channel_id,
            client_ip: self.client_ip,
            user_agent: self.user_agent,
            referer: self.referer,
            start_time: self.start_time,
            end_time: Some(Utc::now()),
            bytes_served,
            relay_used: self.relay_used,
            relay_config_id: self.relay_config_id,
        };
        
        metrics_logger.log_stream_access(metrics).await;
    }
    
    /// Update bytes served
    pub fn add_bytes(&mut self, bytes: u64) {
        self.bytes_served += bytes;
    }
    
    /// Mark as using a relay
    pub fn set_relay(&mut self, relay_config_id: Uuid) {
        self.relay_used = true;
        self.relay_config_id = Some(relay_config_id);
    }
}

impl Default for MetricsLogger {
    fn default() -> Self {
        Self::new()
    }
}