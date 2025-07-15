-- Enhanced Metrics System Migration
-- This migration adds comprehensive metrics tracking with client sessions and aggregation

-- Active client sessions (ephemeral, for real-time tracking)
CREATE TABLE active_stream_sessions (
    session_id TEXT PRIMARY KEY NOT NULL,
    proxy_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    client_ip TEXT NOT NULL,
    user_agent TEXT,
    referer TEXT,
    start_time TEXT NOT NULL,
    last_access_time TEXT NOT NULL, -- Updated on each chunk served
    bytes_served INTEGER DEFAULT 0,
    proxy_mode TEXT NOT NULL CHECK (proxy_mode IN ('proxy', 'redirect')),
    relay_used BOOLEAN DEFAULT FALSE,
    relay_config_id TEXT REFERENCES relay_configs(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (proxy_id) REFERENCES stream_proxies(id) ON DELETE CASCADE
);

-- Indexes for efficient cleanup and queries
CREATE INDEX idx_active_sessions_last_access ON active_stream_sessions(last_access_time);
CREATE INDEX idx_active_sessions_proxy_channel ON active_stream_sessions(proxy_id, channel_id);
CREATE INDEX idx_active_sessions_client_ip ON active_stream_sessions(client_ip);

-- Enhance existing stream_access_logs with duration and proxy_mode
ALTER TABLE stream_access_logs ADD COLUMN duration_seconds INTEGER;
ALTER TABLE stream_access_logs ADD COLUMN proxy_mode TEXT;

-- Hourly aggregated stats (for medium-term storage and dashboard queries)
CREATE TABLE stream_stats_hourly (
    id TEXT PRIMARY KEY NOT NULL,
    hour_bucket TEXT NOT NULL, -- YYYY-MM-DD HH:00:00
    proxy_id TEXT NOT NULL,
    channel_id TEXT, -- NULL for proxy-level stats
    total_sessions INTEGER DEFAULT 0,
    unique_clients INTEGER DEFAULT 0,
    total_bytes_served INTEGER DEFAULT 0,
    total_duration_seconds INTEGER DEFAULT 0,
    peak_concurrent_sessions INTEGER DEFAULT 0,
    proxy_sessions INTEGER DEFAULT 0, -- Count where proxy_mode='proxy'
    redirect_sessions INTEGER DEFAULT 0, -- Count where proxy_mode='redirect'
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(hour_bucket, proxy_id, channel_id)
);

-- Daily aggregated stats (for long-term storage)
CREATE TABLE stream_stats_daily (
    id TEXT PRIMARY KEY NOT NULL,
    date TEXT NOT NULL, -- YYYY-MM-DD
    proxy_id TEXT NOT NULL,
    channel_id TEXT, -- NULL for proxy-level stats
    total_sessions INTEGER DEFAULT 0,
    unique_clients INTEGER DEFAULT 0,
    total_bytes_served INTEGER DEFAULT 0,
    total_duration_seconds INTEGER DEFAULT 0,
    peak_concurrent_sessions INTEGER DEFAULT 0,
    proxy_sessions INTEGER DEFAULT 0, -- Count where proxy_mode='proxy'
    redirect_sessions INTEGER DEFAULT 0, -- Count where proxy_mode='redirect'
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(date, proxy_id, channel_id)
);

-- Indexes for efficient aggregation queries
CREATE INDEX idx_stream_logs_start_time ON stream_access_logs(start_time);
CREATE INDEX idx_stream_logs_proxy_channel ON stream_access_logs(proxy_id, channel_id);
CREATE INDEX idx_stream_logs_proxy_mode ON stream_access_logs(proxy_mode);

CREATE INDEX idx_hourly_stats_hour_bucket ON stream_stats_hourly(hour_bucket);
CREATE INDEX idx_hourly_stats_proxy ON stream_stats_hourly(proxy_id);
CREATE INDEX idx_hourly_stats_channel ON stream_stats_hourly(channel_id);

CREATE INDEX idx_daily_stats_date ON stream_stats_daily(date);
CREATE INDEX idx_daily_stats_proxy ON stream_stats_daily(proxy_id);
CREATE INDEX idx_daily_stats_channel ON stream_stats_daily(channel_id);

-- Real-time stats cache (for dashboard performance)
CREATE TABLE stream_stats_realtime (
    proxy_id TEXT PRIMARY KEY NOT NULL,
    active_sessions INTEGER DEFAULT 0,
    active_clients INTEGER DEFAULT 0,
    bytes_per_second INTEGER DEFAULT 0,
    last_updated TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (proxy_id) REFERENCES stream_proxies(id) ON DELETE CASCADE
);