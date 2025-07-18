-- M3U Proxy Complete Database Schema
-- This consolidated schema includes all tables, indexes, and triggers
-- Last updated: 2025-07-16

-- =============================================================================
-- CORE TABLES
-- =============================================================================

-- Stream Sources Table
CREATE TABLE stream_sources (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    source_type TEXT NOT NULL CHECK (source_type IN ('m3u', 'xtream')),
    url TEXT NOT NULL,
    max_concurrent_streams INTEGER NOT NULL DEFAULT 1,
    update_cron TEXT NOT NULL DEFAULT '0 0 */6 * * * *', -- Every 6 hours
    username TEXT,
    password TEXT,
    field_map TEXT, -- JSON string for M3U field mapping
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_ingested_at TEXT,
    is_active BOOLEAN NOT NULL DEFAULT TRUE
);

-- EPG Sources Table
CREATE TABLE epg_sources (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    source_type TEXT NOT NULL CHECK (source_type IN ('xmltv', 'xtream')),
    url TEXT NOT NULL,
    update_cron TEXT NOT NULL DEFAULT '0 0 */12 * * * *', -- Every 12 hours
    username TEXT, -- For Xtream Codes
    password TEXT, -- For Xtream Codes
    original_timezone TEXT, -- Original timezone for reference (all times stored as UTC)
    time_offset TEXT DEFAULT '0', -- Time offset like '+1h30m', '-45m', '+5s'
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_ingested_at TEXT,
    is_active BOOLEAN NOT NULL DEFAULT TRUE
);

-- Stream Proxies Table
CREATE TABLE stream_proxies (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    proxy_mode TEXT NOT NULL DEFAULT 'redirect' CHECK (proxy_mode IN ('redirect', 'proxy', 'relay')),
    upstream_timeout INTEGER DEFAULT 30, -- Timeout in seconds
    buffer_size INTEGER DEFAULT 8192, -- Buffer size in bytes
    max_concurrent_streams INTEGER DEFAULT 1, -- Max concurrent streams for this proxy
    starting_channel_number INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_generated_at TEXT,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    auto_regenerate BOOLEAN NOT NULL DEFAULT FALSE,
    cache_channel_logos BOOLEAN NOT NULL DEFAULT TRUE,
    cache_program_logos BOOLEAN NOT NULL DEFAULT FALSE,
    relay_profile_id TEXT REFERENCES relay_profiles(id) ON DELETE SET NULL
);

-- =============================================================================
-- RELATIONSHIP TABLES
-- =============================================================================

-- Junction table for proxy-source relationships
CREATE TABLE proxy_sources (
    proxy_id TEXT NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    source_id TEXT NOT NULL REFERENCES stream_sources(id) ON DELETE CASCADE,
    priority_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (proxy_id, source_id)
);

-- Junction table for proxy-EPG source relationships
CREATE TABLE proxy_epg_sources (
    proxy_id TEXT NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    epg_source_id TEXT NOT NULL REFERENCES epg_sources(id) ON DELETE CASCADE,
    priority_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (proxy_id, epg_source_id)
);

-- =============================================================================
-- FILTERING SYSTEM
-- =============================================================================

-- Filters Table
CREATE TABLE filters (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    source_type TEXT NOT NULL DEFAULT 'stream' CHECK (source_type IN ('stream', 'epg')),
    starting_channel_number INTEGER NOT NULL DEFAULT 1,
    is_inverse BOOLEAN NOT NULL DEFAULT FALSE,
    is_system_default BOOLEAN NOT NULL DEFAULT FALSE,
    condition_tree TEXT NOT NULL, -- JSON tree structure for complex nested conditions
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Junction table for proxy-filter relationships
CREATE TABLE proxy_filters (
    proxy_id TEXT NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    filter_id TEXT NOT NULL REFERENCES filters(id) ON DELETE CASCADE,
    priority_order INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (proxy_id, filter_id)
);

-- =============================================================================
-- CHANNEL AND EPG DATA
-- =============================================================================

-- Channels Table (ingested stream data)
CREATE TABLE channels (
    id TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL REFERENCES stream_sources(id) ON DELETE CASCADE,
    tvg_id TEXT,
    tvg_name TEXT,
    channel_name TEXT NOT NULL,
    tvg_logo TEXT,
    tvg_shift TEXT, -- Timeshift offset for M3U (e.g., "+1", "+24")
    group_title TEXT,
    stream_url TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- EPG Channels Table
CREATE TABLE epg_channels (
    id TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL REFERENCES epg_sources(id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL, -- The original channel ID from XMLTV
    channel_name TEXT NOT NULL,
    channel_logo TEXT,
    channel_group TEXT,
    language TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(source_id, channel_id)
);

-- EPG Programs Table
CREATE TABLE epg_programs (
    id TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL REFERENCES epg_sources(id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL,
    channel_name TEXT NOT NULL,
    start_time TEXT NOT NULL, -- Always stored as UTC
    end_time TEXT NOT NULL,  -- Always stored as UTC
    program_title TEXT NOT NULL,
    program_description TEXT,
    program_category TEXT,
    episode_num TEXT,
    season_num TEXT,
    rating TEXT,
    language TEXT,
    subtitles TEXT,
    aspect_ratio TEXT,
    program_icon TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Channel to EPG Mapping Table
CREATE TABLE channel_epg_mapping (
    id TEXT PRIMARY KEY NOT NULL,
    stream_channel_id TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    epg_channel_id TEXT NOT NULL REFERENCES epg_channels(id) ON DELETE CASCADE,
    mapping_type TEXT NOT NULL CHECK (mapping_type IN ('auto', 'manual')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(stream_channel_id, epg_channel_id)
);

-- =============================================================================
-- PROXY GENERATION AND OUTPUT
-- =============================================================================

-- Proxy Generations Table
CREATE TABLE proxy_generations (
    id TEXT PRIMARY KEY NOT NULL,
    proxy_id TEXT NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    m3u_content TEXT,
    channel_count INTEGER NOT NULL DEFAULT 0,
    generated_at TEXT NOT NULL DEFAULT (datetime('now')),
    generation_time_ms INTEGER, -- Time taken to generate in milliseconds
    is_current BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE(proxy_id, version)
);

-- =============================================================================
-- DATA MAPPING AND TRANSFORMATION
-- =============================================================================

-- Data Mapping Rules Table
CREATE TABLE data_mapping_rules (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    source_type TEXT CHECK (source_type IN ('stream', 'epg')),
    scope TEXT CHECK (scope IN ('individual', 'stream_wide', 'epg_wide')),
    expression TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- =============================================================================
-- ASSETS AND MEDIA
-- =============================================================================

-- Logo Assets Table
CREATE TABLE logo_assets (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    file_name TEXT NOT NULL,
    file_path TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    mime_type TEXT NOT NULL,
    asset_type TEXT NOT NULL CHECK (asset_type IN ('uploaded', 'cached')),
    source_url TEXT,
    width INTEGER,
    height INTEGER,
    parent_asset_id TEXT REFERENCES logo_assets(id) ON DELETE SET NULL,
    format_type TEXT NOT NULL DEFAULT 'original' CHECK (format_type IN ('original', 'png_conversion')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- =============================================================================
-- RELAY SYSTEM (FFmpeg-based stream relay)
-- =============================================================================

-- Relay Profiles Table
CREATE TABLE relay_profiles (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    ffmpeg_args TEXT NOT NULL, -- JSON array of FFmpeg arguments
    output_format TEXT NOT NULL DEFAULT 'transport_stream' CHECK (output_format IN ('transport_stream', 'hls', 'dash', 'copy')),
    segment_duration INTEGER, -- For segmented formats (seconds)
    max_segments INTEGER,     -- For circular buffer
    input_timeout INTEGER NOT NULL DEFAULT 30,
    hardware_acceleration TEXT, -- 'cuda', 'vaapi', etc.
    is_system_default BOOLEAN NOT NULL DEFAULT FALSE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Channel Relay Configurations
CREATE TABLE channel_relay_configs (
    id TEXT PRIMARY KEY NOT NULL,
    proxy_id TEXT NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL, -- References channels but not FK due to dynamic nature
    profile_id TEXT NOT NULL REFERENCES relay_profiles(id),
    name TEXT NOT NULL,
    description TEXT,
    custom_args TEXT, -- Optional JSON array to override/extend profile args
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(proxy_id, channel_id) -- One relay config per channel per proxy
);

-- Runtime Status for Active Relay Processes
CREATE TABLE relay_runtime_status (
    channel_relay_config_id TEXT PRIMARY KEY, -- References channel_relay_configs(id)
    process_id TEXT, -- System process ID
    sandbox_path TEXT,
    is_running BOOLEAN NOT NULL DEFAULT FALSE,
    started_at TEXT,
    client_count INTEGER NOT NULL DEFAULT 0,
    bytes_served INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    last_heartbeat TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Relay Events for Metrics and Monitoring
CREATE TABLE relay_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    config_id TEXT NOT NULL REFERENCES channel_relay_configs(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL CHECK (event_type IN ('start', 'stop', 'error', 'client_connect', 'client_disconnect')),
    details TEXT,
    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- =============================================================================
-- METRICS AND LOGGING
-- =============================================================================

-- Stream Access Logs Table
CREATE TABLE stream_access_logs (
    id TEXT PRIMARY KEY NOT NULL,
    proxy_id TEXT NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL, -- References channels(id) but not FK due to channel lifecycle
    client_ip TEXT NOT NULL,
    user_agent TEXT,
    referer TEXT, -- HTTP referer header
    start_time TEXT NOT NULL DEFAULT (datetime('now')),
    end_time TEXT,
    bytes_served INTEGER NOT NULL DEFAULT 0,
    relay_used BOOLEAN NOT NULL DEFAULT FALSE,
    relay_config_id TEXT REFERENCES channel_relay_configs(id) ON DELETE SET NULL,
    duration_seconds INTEGER, -- Duration in seconds
    proxy_mode TEXT, -- 'redirect', 'proxy', or 'relay'
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Active Stream Sessions (for real-time tracking)
CREATE TABLE active_stream_sessions (
    session_id TEXT PRIMARY KEY NOT NULL,
    proxy_id TEXT NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL,
    client_ip TEXT NOT NULL,
    user_agent TEXT,
    referer TEXT, -- HTTP referer header
    start_time TEXT NOT NULL DEFAULT (datetime('now')),
    last_activity TEXT NOT NULL DEFAULT (datetime('now')),
    last_access_time TEXT NOT NULL DEFAULT (datetime('now')), -- Alias for last_activity
    bytes_served INTEGER NOT NULL DEFAULT 0,
    relay_used BOOLEAN NOT NULL DEFAULT FALSE,
    relay_config_id TEXT,
    proxy_mode TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Hourly Statistics
CREATE TABLE stream_stats_hourly (
    id TEXT PRIMARY KEY NOT NULL, -- Changed to TEXT for composite IDs
    proxy_id TEXT NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    channel_id TEXT,
    hour_bucket TEXT NOT NULL, -- Renamed from hour_start to match app code
    unique_clients INTEGER NOT NULL DEFAULT 0,
    total_sessions INTEGER NOT NULL DEFAULT 0,
    total_bytes_served INTEGER NOT NULL DEFAULT 0, -- Renamed from total_bytes
    total_duration_seconds INTEGER NOT NULL DEFAULT 0,
    relay_sessions INTEGER NOT NULL DEFAULT 0,
    proxy_sessions INTEGER NOT NULL DEFAULT 0,
    redirect_sessions INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(proxy_id, channel_id, hour_bucket)
);

-- Daily Statistics
CREATE TABLE stream_stats_daily (
    id TEXT PRIMARY KEY NOT NULL, -- Changed to TEXT for composite IDs
    proxy_id TEXT NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    channel_id TEXT,
    date TEXT NOT NULL,
    unique_clients INTEGER NOT NULL DEFAULT 0,
    total_sessions INTEGER NOT NULL DEFAULT 0,
    total_bytes_served INTEGER NOT NULL DEFAULT 0, -- Renamed from total_bytes
    total_duration_seconds INTEGER NOT NULL DEFAULT 0,
    peak_concurrent_sessions INTEGER NOT NULL DEFAULT 0,
    relay_sessions INTEGER NOT NULL DEFAULT 0,
    proxy_sessions INTEGER NOT NULL DEFAULT 0,
    redirect_sessions INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(proxy_id, channel_id, date)
);

-- Real-time Statistics Cache
CREATE TABLE stream_stats_realtime (
    proxy_id TEXT PRIMARY KEY NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    active_sessions INTEGER NOT NULL DEFAULT 0,
    active_clients INTEGER NOT NULL DEFAULT 0,
    bytes_per_second INTEGER NOT NULL DEFAULT 0,
    last_updated TEXT NOT NULL DEFAULT (datetime('now'))
);

-- =============================================================================
-- SYSTEM MANAGEMENT
-- =============================================================================

-- Proxy Regeneration Queue Table
CREATE TABLE proxy_regeneration_queue (
    id TEXT PRIMARY KEY NOT NULL,
    proxy_id TEXT NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    trigger_source_id TEXT, -- Optional source ID that triggered regeneration
    trigger_source_type TEXT, -- Type of source that triggered regeneration
    reason TEXT,
    priority INTEGER NOT NULL DEFAULT 0,
    scheduled_at TEXT NOT NULL DEFAULT (datetime('now')),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'processing', 'completed', 'failed')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    started_at TEXT, -- When processing began
    completed_at TEXT, -- When processing completed
    error_message TEXT -- Error message if failed
);

-- Migration Notes Table
CREATE TABLE migration_notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version TEXT NOT NULL,
    note TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Linked Xtream Sources Table
CREATE TABLE linked_xtream_sources (
    id TEXT PRIMARY KEY NOT NULL,
    link_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    username TEXT NOT NULL,
    password TEXT NOT NULL,
    stream_source_id TEXT REFERENCES stream_sources(id) ON DELETE SET NULL,
    epg_source_id TEXT REFERENCES epg_sources(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- =============================================================================
-- INDEXES FOR PERFORMANCE
-- =============================================================================

-- Stream Sources
CREATE INDEX idx_stream_sources_active ON stream_sources(is_active);
CREATE INDEX idx_stream_sources_type ON stream_sources(source_type);
CREATE INDEX idx_stream_sources_last_ingested ON stream_sources(last_ingested_at);

-- EPG Sources
CREATE INDEX idx_epg_sources_active ON epg_sources(is_active);
CREATE INDEX idx_epg_sources_type ON epg_sources(source_type);
CREATE INDEX idx_epg_sources_last_ingested ON epg_sources(last_ingested_at);

-- Stream Proxies
CREATE INDEX idx_stream_proxies_active ON stream_proxies(is_active);
CREATE INDEX idx_stream_proxies_proxy_mode ON stream_proxies(proxy_mode);
CREATE INDEX idx_stream_proxies_auto_regenerate ON stream_proxies(auto_regenerate);
CREATE INDEX idx_stream_proxies_last_generated ON stream_proxies(last_generated_at);
CREATE INDEX idx_stream_proxies_relay_profile_id ON stream_proxies(relay_profile_id);

-- Proxy Sources
CREATE INDEX idx_proxy_sources_source_id ON proxy_sources(source_id);
CREATE INDEX idx_proxy_sources_priority ON proxy_sources(proxy_id, priority_order);

-- Proxy EPG Sources
CREATE INDEX idx_proxy_epg_sources_epg_source_id ON proxy_epg_sources(epg_source_id);
CREATE INDEX idx_proxy_epg_sources_priority ON proxy_epg_sources(proxy_id, priority_order);

-- Filters
CREATE INDEX idx_filters_source_type ON filters(source_type);

-- Proxy Filters
CREATE INDEX idx_proxy_filters_filter_id ON proxy_filters(filter_id);
CREATE INDEX idx_proxy_filters_active ON proxy_filters(is_active);
CREATE INDEX idx_proxy_filters_priority ON proxy_filters(proxy_id, priority_order);

-- Channels
CREATE INDEX idx_channels_source_id ON channels(source_id);
CREATE INDEX idx_channels_tvg_id ON channels(tvg_id);
CREATE INDEX idx_channels_tvg_name ON channels(tvg_name);
CREATE INDEX idx_channels_channel_name ON channels(channel_name);
CREATE INDEX idx_channels_group_title ON channels(group_title);

-- EPG Channels
CREATE INDEX idx_epg_channels_source_id ON epg_channels(source_id);
CREATE INDEX idx_epg_channels_channel_id ON epg_channels(channel_id);
CREATE INDEX idx_epg_channels_channel_name ON epg_channels(channel_name);

-- EPG Programs
CREATE INDEX idx_epg_programs_source_id ON epg_programs(source_id);
CREATE INDEX idx_epg_programs_channel_id ON epg_programs(channel_id);
CREATE INDEX idx_epg_programs_start_time ON epg_programs(start_time);
CREATE INDEX idx_epg_programs_end_time ON epg_programs(end_time);
CREATE INDEX idx_epg_programs_program_title ON epg_programs(program_title);
CREATE INDEX idx_epg_programs_time_range ON epg_programs(source_id, channel_id, start_time, end_time);
CREATE INDEX idx_epg_programs_channel_lookup ON epg_programs(source_id, channel_id);

-- Channel EPG Mapping
CREATE INDEX idx_channel_epg_mapping_stream_channel_id ON channel_epg_mapping(stream_channel_id);
CREATE INDEX idx_channel_epg_mapping_epg_channel_id ON channel_epg_mapping(epg_channel_id);
CREATE INDEX idx_channel_epg_mapping_mapping_type ON channel_epg_mapping(mapping_type);

-- Proxy Generations
CREATE INDEX idx_proxy_generations_proxy_id ON proxy_generations(proxy_id);
CREATE INDEX idx_proxy_generations_current ON proxy_generations(is_current);
CREATE INDEX idx_proxy_generations_generated_at ON proxy_generations(generated_at);

-- Data Mapping Rules
CREATE INDEX idx_data_mapping_rules_active ON data_mapping_rules(is_active);
CREATE INDEX idx_data_mapping_rules_source_type ON data_mapping_rules(source_type);
CREATE INDEX idx_data_mapping_rules_sort_order ON data_mapping_rules(sort_order, is_active);

-- Logo Assets
CREATE INDEX idx_logo_assets_asset_type ON logo_assets(asset_type);
CREATE INDEX idx_logo_assets_format_type ON logo_assets(format_type);
CREATE INDEX idx_logo_assets_parent_asset_id ON logo_assets(parent_asset_id);

-- Relay System
CREATE INDEX idx_channel_relay_configs_proxy_id ON channel_relay_configs(proxy_id);
CREATE INDEX idx_channel_relay_configs_channel_id ON channel_relay_configs(channel_id);
CREATE INDEX idx_channel_relay_configs_profile_id ON channel_relay_configs(profile_id);
CREATE INDEX idx_relay_runtime_status_running ON relay_runtime_status(is_running);
CREATE INDEX idx_relay_events_config_id ON relay_events(config_id);
CREATE INDEX idx_relay_events_timestamp ON relay_events(timestamp);
CREATE INDEX idx_relay_events_event_type ON relay_events(event_type);

-- Stream Access Logs
CREATE INDEX idx_stream_access_logs_proxy_id ON stream_access_logs(proxy_id);
CREATE INDEX idx_stream_access_logs_channel_id ON stream_access_logs(channel_id);
CREATE INDEX idx_stream_access_logs_start_time ON stream_access_logs(start_time);
CREATE INDEX idx_stream_access_logs_client_ip ON stream_access_logs(client_ip);
CREATE INDEX idx_stream_access_logs_relay_used ON stream_access_logs(relay_used);

-- Metrics Tables
CREATE INDEX idx_active_sessions_proxy_channel ON active_stream_sessions(proxy_id, channel_id);
CREATE INDEX idx_active_sessions_client_ip ON active_stream_sessions(client_ip);
CREATE INDEX idx_active_sessions_start_time ON active_stream_sessions(start_time);
CREATE INDEX idx_active_sessions_last_activity ON active_stream_sessions(last_activity);
CREATE INDEX idx_hourly_stats_lookup ON stream_stats_hourly(proxy_id, hour_bucket);
CREATE INDEX idx_hourly_stats_channel ON stream_stats_hourly(channel_id, hour_bucket);
CREATE INDEX idx_daily_stats_lookup ON stream_stats_daily(proxy_id, date);
CREATE INDEX idx_daily_stats_channel ON stream_stats_daily(channel_id, date);
CREATE INDEX idx_realtime_stats_updated ON stream_stats_realtime(last_updated);

-- Proxy Regeneration Queue
CREATE INDEX idx_proxy_regeneration_queue_scheduled ON proxy_regeneration_queue(scheduled_at, status);
CREATE INDEX idx_proxy_regeneration_queue_status ON proxy_regeneration_queue(status);
CREATE INDEX idx_proxy_regeneration_queue_proxy_id ON proxy_regeneration_queue(proxy_id);

-- Linked Xtream Sources
CREATE INDEX idx_linked_xtream_sources_link_id ON linked_xtream_sources(link_id);
CREATE INDEX idx_linked_xtream_sources_stream_source_id ON linked_xtream_sources(stream_source_id);
CREATE INDEX idx_linked_xtream_sources_epg_source_id ON linked_xtream_sources(epg_source_id);
CREATE INDEX idx_linked_xtream_sources_credentials ON linked_xtream_sources(url, username, password);

-- =============================================================================
-- TRIGGERS FOR AUTOMATIC TIMESTAMPS
-- =============================================================================

CREATE TRIGGER stream_sources_updated_at
    AFTER UPDATE ON stream_sources
    FOR EACH ROW
    BEGIN
        UPDATE stream_sources SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER epg_sources_updated_at
    AFTER UPDATE ON epg_sources
    FOR EACH ROW
    BEGIN
        UPDATE epg_sources SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER stream_proxies_updated_at
    AFTER UPDATE ON stream_proxies
    FOR EACH ROW
    BEGIN
        UPDATE stream_proxies SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER filters_updated_at
    AFTER UPDATE ON filters
    FOR EACH ROW
    BEGIN
        UPDATE filters SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER channels_updated_at
    AFTER UPDATE ON channels
    FOR EACH ROW
    BEGIN
        UPDATE channels SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER epg_channels_updated_at
    AFTER UPDATE ON epg_channels
    FOR EACH ROW
    BEGIN
        UPDATE epg_channels SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER epg_programs_updated_at
    AFTER UPDATE ON epg_programs
    FOR EACH ROW
    BEGIN
        UPDATE epg_programs SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER channel_epg_mapping_updated_at
    AFTER UPDATE ON channel_epg_mapping
    FOR EACH ROW
    BEGIN
        UPDATE channel_epg_mapping SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER data_mapping_rules_updated_at
    AFTER UPDATE ON data_mapping_rules
    FOR EACH ROW
    BEGIN
        UPDATE data_mapping_rules SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER relay_profiles_updated_at
    AFTER UPDATE ON relay_profiles
    FOR EACH ROW
    BEGIN
        UPDATE relay_profiles SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER channel_relay_configs_updated_at
    AFTER UPDATE ON channel_relay_configs
    FOR EACH ROW
    BEGIN
        UPDATE channel_relay_configs SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER relay_runtime_status_updated_at
    AFTER UPDATE ON relay_runtime_status
    FOR EACH ROW
    BEGIN
        UPDATE relay_runtime_status SET updated_at = datetime('now') WHERE channel_relay_config_id = NEW.channel_relay_config_id;
    END;

CREATE TRIGGER linked_xtream_sources_updated_at
    AFTER UPDATE ON linked_xtream_sources
    FOR EACH ROW
    BEGIN
        UPDATE linked_xtream_sources SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER proxy_regeneration_queue_updated_at
    AFTER UPDATE ON proxy_regeneration_queue
    FOR EACH ROW
    BEGIN
        UPDATE proxy_regeneration_queue SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER logo_assets_updated_at
    AFTER UPDATE ON logo_assets
    FOR EACH ROW
    BEGIN
        UPDATE logo_assets SET updated_at = datetime('now') WHERE id = NEW.id;
    END;
