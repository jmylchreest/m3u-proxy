use anyhow::Result;
use chrono::Utc;
use humantime::parse_duration;
use sqlx::SqlitePool;
use std::time::Duration;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{error, info, trace};

use crate::config::MetricsConfig;

/// Metrics housekeeper service for data retention and aggregation
pub struct MetricsHousekeeper {
    db: SqlitePool,
    raw_log_retention: Duration,
    hourly_stats_retention: Duration,
    daily_stats_retention: Duration,
    session_timeout: Duration,
    aggregation_interval: Duration,
}

impl MetricsHousekeeper {
    /// Create a new MetricsHousekeeper from configuration
    pub fn from_config(db: SqlitePool, config: &MetricsConfig) -> Result<Self> {
        Ok(Self {
            db,
            raw_log_retention: parse_duration(&config.raw_log_retention)?,
            hourly_stats_retention: parse_duration(&config.hourly_stats_retention)?,
            daily_stats_retention: parse_duration(&config.daily_stats_retention)?,
            session_timeout: parse_duration(&config.session_timeout)?,
            aggregation_interval: parse_duration(&config.housekeeper_interval)?,
        })
    }

    /// Start the housekeeper background task
    pub async fn start(self) {
        let mut interval = interval(self.aggregation_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        info!(
            "Starting metrics housekeeper with interval: {:?}",
            self.aggregation_interval
        );

        loop {
            interval.tick().await;

            if let Err(e) = self.run_cleanup().await {
                error!("Metrics housekeeper error: {}", e);
            }
        }
    }

    /// Run a complete cleanup cycle
    pub async fn run_cleanup(&self) -> Result<()> {
        let start_time = Utc::now();
        
        // 1. Clean up stale active sessions
        let stale_sessions = self.cleanup_stale_sessions().await?;
        
        // 2. Aggregate hourly stats from completed sessions
        let hourly_aggregated = self.aggregate_hourly_stats().await?;
        
        // 3. Aggregate daily stats from hourly stats
        let daily_aggregated = self.aggregate_daily_stats().await?;
        
        // 4. Prune old raw logs beyond retention period
        let pruned_logs = self.prune_old_logs().await?;
        
        // 5. Prune old hourly stats beyond retention period
        let pruned_hourly = self.prune_old_hourly_stats().await?;
        
        // 6. Prune old daily stats beyond retention period
        let pruned_daily = self.prune_old_daily_stats().await?;
        
        // 7. Update real-time stats cache
        self.update_realtime_stats().await?;

        let duration = Utc::now().signed_duration_since(start_time);
        let duration_ms = duration.num_milliseconds();
        
        // Only log if something was actually done or if it took longer than 10 seconds
        let total_work = stale_sessions + hourly_aggregated + daily_aggregated + pruned_logs + pruned_hourly + pruned_daily;
        if total_work > 0 || duration_ms > 10_000 {
            info!(
                "Metrics housekeeper completed in {}ms: {} stale sessions, {} hourly aggregated, {} daily aggregated, {} logs pruned, {} hourly pruned, {} daily pruned",
                duration_ms,
                stale_sessions,
                hourly_aggregated,
                daily_aggregated,
                pruned_logs,
                pruned_hourly,
                pruned_daily
            );
        } else {
            trace!(
                "Metrics housekeeper completed in {}ms: no work performed",
                duration_ms
            );
        }

        Ok(())
    }

    /// Clean up stale active sessions that haven't been accessed recently
    async fn cleanup_stale_sessions(&self) -> Result<u64> {
        let cutoff_time = Utc::now() - chrono::Duration::from_std(self.session_timeout)?;
        let cutoff_str = cutoff_time.to_rfc3339();

        // Move stale sessions to historical logs
        let result = sqlx::query(
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
            WHERE last_access_time < ?
            "#,
        )
        .bind(&cutoff_str)
        .execute(&self.db)
        .await?;

        let moved_sessions = result.rows_affected();

        // Remove the stale sessions from active table
        if moved_sessions > 0 {
            sqlx::query("DELETE FROM active_stream_sessions WHERE last_access_time < ?")
                .bind(&cutoff_str)
                .execute(&self.db)
                .await?;
        }

        Ok(moved_sessions)
    }

    /// Aggregate hourly stats from completed sessions
    async fn aggregate_hourly_stats(&self) -> Result<u64> {
        let result = sqlx::query(
            r#"
            INSERT OR REPLACE INTO stream_stats_hourly (
                id, hour_bucket, proxy_name, channel_id,
                total_sessions, unique_clients, total_bytes_served, total_duration_seconds,
                proxy_sessions, redirect_sessions, created_at
            )
            SELECT 
                proxy_name || '_' || channel_id || '_' || strftime('%Y-%m-%d %H:00:00', start_time) as id,
                strftime('%Y-%m-%d %H:00:00', start_time) as hour_bucket,
                proxy_name,
                channel_id,
                COUNT(*) as total_sessions,
                COUNT(DISTINCT client_ip) as unique_clients,
                SUM(bytes_served) as total_bytes_served,
                SUM(COALESCE(duration_seconds, 0)) as total_duration_seconds,
                SUM(CASE WHEN proxy_mode = 'proxy' THEN 1 ELSE 0 END) as proxy_sessions,
                SUM(CASE WHEN proxy_mode = 'redirect' THEN 1 ELSE 0 END) as redirect_sessions,
                datetime('now') as created_at
            FROM stream_access_logs
            WHERE start_time >= datetime('now', '-2 hours')
            GROUP BY proxy_name, channel_id, strftime('%Y-%m-%d %H:00:00', start_time)
            "#
        )
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected())
    }

    /// Aggregate daily stats from hourly stats
    async fn aggregate_daily_stats(&self) -> Result<u64> {
        let result = sqlx::query(
            r#"
            INSERT OR REPLACE INTO stream_stats_daily (
                id, date, proxy_name, channel_id,
                total_sessions, unique_clients, total_bytes_served, total_duration_seconds,
                peak_concurrent_sessions, proxy_sessions, redirect_sessions, created_at
            )
            SELECT 
                proxy_name || '_' || channel_id || '_' || date(hour_bucket) as id,
                date(hour_bucket) as date,
                proxy_name,
                channel_id,
                SUM(total_sessions) as total_sessions,
                MAX(unique_clients) as unique_clients, -- Approximation for daily unique clients
                SUM(total_bytes_served) as total_bytes_served,
                SUM(total_duration_seconds) as total_duration_seconds,
                MAX(total_sessions) as peak_concurrent_sessions, -- Approximation using max hourly sessions
                SUM(proxy_sessions) as proxy_sessions,
                SUM(redirect_sessions) as redirect_sessions,
                datetime('now') as created_at
            FROM stream_stats_hourly
            WHERE hour_bucket >= date('now', '-2 days')
            GROUP BY proxy_name, channel_id, date(hour_bucket)
            "#
        )
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected())
    }

    /// Prune old raw access logs beyond retention period
    async fn prune_old_logs(&self) -> Result<u64> {
        let retention_days = self.raw_log_retention.as_secs() / 86400;
        let query = format!("DELETE FROM stream_access_logs WHERE start_time < datetime('now', '-{} days')", retention_days);
        let result = sqlx::query(&query)
            .execute(&self.db)
            .await?;

        Ok(result.rows_affected())
    }

    /// Prune old hourly stats beyond retention period
    async fn prune_old_hourly_stats(&self) -> Result<u64> {
        let retention_days = self.hourly_stats_retention.as_secs() / 86400;
        let query = format!("DELETE FROM stream_stats_hourly WHERE hour_bucket < datetime('now', '-{} days')", retention_days);
        let result = sqlx::query(&query)
            .execute(&self.db)
            .await?;

        Ok(result.rows_affected())
    }

    /// Prune old daily stats beyond retention period
    async fn prune_old_daily_stats(&self) -> Result<u64> {
        let retention_days = self.daily_stats_retention.as_secs() / 86400;
        let query = format!("DELETE FROM stream_stats_daily WHERE date < date('now', '-{} days')", retention_days);
        let result = sqlx::query(&query)
            .execute(&self.db)
            .await?;

        Ok(result.rows_affected())
    }

    /// Update real-time stats cache for dashboard
    async fn update_realtime_stats(&self) -> Result<()> {
        // Clear existing real-time stats
        sqlx::query("DELETE FROM stream_stats_realtime")
            .execute(&self.db)
            .await?;

        // Calculate current active sessions and clients per proxy
        sqlx::query(
            r#"
            INSERT INTO stream_stats_realtime (
                proxy_name, active_sessions, active_clients, bytes_per_second, last_updated
            )
            SELECT 
                proxy_name,
                COUNT(*) as active_sessions,
                COUNT(DISTINCT client_ip) as active_clients,
                COALESCE(SUM(bytes_served) / MAX(1, CAST((julianday('now') - julianday(start_time)) * 86400 AS INTEGER)), 0) as bytes_per_second,
                datetime('now') as last_updated
            FROM active_stream_sessions
            GROUP BY proxy_name
            "#
        )
        .execute(&self.db)
        .await?;

        Ok(())
    }
}