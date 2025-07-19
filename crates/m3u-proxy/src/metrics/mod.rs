use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use uuid::Uuid;
use tracing::{error, info};
use anyhow::Result;

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
    db: SqlitePool,
}

impl MetricsLogger {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
    
    /// Get database pool for relay logging
    pub fn pool(&self) -> &SqlitePool {
        &self.db
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
        
        // Calculate duration if end_time is present
        let duration_seconds = if let Some(end_time) = metrics.end_time {
            (end_time - metrics.start_time).num_seconds() as i64
        } else {
            0
        };
        
        // Determine proxy mode based on bytes served
        let proxy_mode = if metrics.bytes_served > 0 { "proxy" } else { "redirect" };
        
        // Store in database
        let result = sqlx::query(
            r#"
            INSERT INTO stream_access_logs (
                id, proxy_name, channel_id, client_ip, user_agent, referer,
                start_time, end_time, bytes_served, relay_used, relay_config_id,
                duration_seconds, proxy_mode, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))
            "#,
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(metrics.proxy_ulid)
        .bind(metrics.channel_id.to_string())
        .bind(metrics.client_ip)
        .bind(metrics.user_agent)
        .bind(metrics.referer)
        .bind(metrics.start_time.to_rfc3339())
        .bind(metrics.end_time.map(|t| t.to_rfc3339()))
        .bind(metrics.bytes_served as i64)
        .bind(metrics.relay_used)
        .bind(metrics.relay_config_id.map(|id| id.to_string()))
        .bind(duration_seconds)
        .bind(proxy_mode)
        .execute(&self.db)
        .await;
        
        if let Err(e) = result {
            error!("Failed to log stream access to database: {}", e);
        }
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
    
    /// Create active session for real-time tracking
    pub async fn create_active_session(
        &self,
        proxy_name: String,
        channel_id: Uuid,
        client_ip: String,
        user_agent: Option<String>,
        referer: Option<String>,
        proxy_mode: &str,
    ) -> Result<String> {
        let session_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        
        sqlx::query(
            r#"
            INSERT INTO active_stream_sessions (
                session_id, proxy_name, channel_id, client_ip, user_agent, referer,
                start_time, last_access_time, proxy_mode, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))
            "#,
        )
        .bind(&session_id)
        .bind(proxy_name)
        .bind(channel_id.to_string())
        .bind(client_ip)
        .bind(user_agent)
        .bind(referer)
        .bind(&now)
        .bind(&now)
        .bind(proxy_mode)
        .execute(&self.db)
        .await?;
        
        Ok(session_id)
    }
    
    /// Update active session access time and bytes served
    pub async fn update_active_session(&self, session_id: &str, bytes_served: u64) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        
        sqlx::query(
            r#"
            UPDATE active_stream_sessions 
            SET last_access_time = ?, bytes_served = bytes_served + ?
            WHERE session_id = ?
            "#,
        )
        .bind(now)
        .bind(bytes_served as i64)
        .bind(session_id)
        .execute(&self.db)
        .await?;
        
        Ok(())
    }
    
    /// Complete active session and move to historical logs
    pub async fn complete_active_session(&self, session_id: &str) -> Result<()> {
        // Move session to historical logs with calculated duration
        sqlx::query(
            r#"
            INSERT INTO stream_access_logs (
                id, proxy_name, channel_id, client_ip, user_agent, referer,
                start_time, end_time, bytes_served, relay_used, relay_config_id,
                duration_seconds, proxy_mode, created_at
            )
            SELECT 
                session_id, proxy_name, channel_id, client_ip, user_agent, referer,
                start_time, last_access_time, bytes_served, relay_used, relay_config_id,
                CAST((julianday(last_access_time) - julianday(start_time)) * 86400 AS INTEGER),
                proxy_mode, created_at
            FROM active_stream_sessions
            WHERE session_id = ?
            "#,
        )
        .bind(session_id)
        .execute(&self.db)
        .await?;
        
        // Remove from active sessions
        sqlx::query("DELETE FROM active_stream_sessions WHERE session_id = ?")
            .bind(session_id)
        .execute(&self.db)
        .await?;
        
        Ok(())
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

// Note: Default implementation removed as MetricsLogger requires a database pool