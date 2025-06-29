-- M3U Proxy Initial Database Schema
-- Complete schema for stream sources, EPG sources, data mapping, and filtering

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
    timezone TEXT DEFAULT 'UTC',
    timezone_detected BOOLEAN DEFAULT FALSE, -- Whether timezone was auto-detected
    time_offset TEXT DEFAULT '0', -- Time offset like '+1h30m', '-45m', '+5s'
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_ingested_at TEXT,
    is_active BOOLEAN NOT NULL DEFAULT TRUE
);

-- Stream Proxies Table
CREATE TABLE stream_proxies (
    id TEXT PRIMARY KEY NOT NULL,
    ulid TEXT UNIQUE NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_generated_at TEXT,
    is_active BOOLEAN NOT NULL DEFAULT TRUE
);

-- Junction table for proxy-source relationships
CREATE TABLE proxy_sources (
    proxy_id TEXT NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    source_id TEXT NOT NULL REFERENCES stream_sources(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (proxy_id, source_id)
);

-- Filters Table (supports both stream and EPG filtering)
CREATE TABLE filters (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    source_type TEXT NOT NULL DEFAULT 'stream' CHECK (source_type IN ('stream', 'epg')),
    starting_channel_number INTEGER NOT NULL DEFAULT 1,
    is_inverse BOOLEAN NOT NULL DEFAULT FALSE,
    condition_tree TEXT NOT NULL, -- JSON tree structure for complex nested conditions
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);



-- Junction table for proxy-filter relationships with ordering
CREATE TABLE proxy_filters (
    proxy_id TEXT NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    filter_id TEXT NOT NULL REFERENCES filters(id) ON DELETE CASCADE,
    sort_order INTEGER NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (proxy_id, filter_id)
);

-- Channels Table (ingested stream data)
CREATE TABLE channels (
    id TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL REFERENCES stream_sources(id) ON DELETE CASCADE,
    tvg_id TEXT,
    tvg_name TEXT,
    tvg_logo TEXT,
    tvg_shift TEXT, -- Timeshift offset for M3U (e.g., "+1", "+24")
    group_title TEXT,
    channel_name TEXT NOT NULL,
    stream_url TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- EPG Channels Table (channel metadata from EPG sources)
CREATE TABLE epg_channels (
    id TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL REFERENCES epg_sources(id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL, -- Original channel ID from EPG source
    channel_name TEXT NOT NULL,
    channel_logo TEXT,
    channel_group TEXT,
    language TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(source_id, channel_id)
);

-- EPG Programs Table (parsed XMLTV data)
CREATE TABLE epg_programs (
    id TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL REFERENCES epg_sources(id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL, -- Channel identifier from EPG source
    channel_name TEXT NOT NULL,
    program_title TEXT NOT NULL,
    program_description TEXT,
    program_category TEXT,
    start_time TEXT NOT NULL, -- ISO 8601 datetime
    end_time TEXT NOT NULL,   -- ISO 8601 datetime
    episode_num TEXT,
    season_num TEXT,
    rating TEXT,
    language TEXT,
    subtitles TEXT,
    aspect_ratio TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Junction table for mapping stream channels to EPG channels
CREATE TABLE channel_epg_mapping (
    id TEXT PRIMARY KEY NOT NULL,
    stream_channel_id TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    epg_channel_id TEXT NOT NULL REFERENCES epg_channels(id) ON DELETE CASCADE,
    mapping_type TEXT NOT NULL DEFAULT 'manual' CHECK (mapping_type IN ('manual', 'auto_name', 'auto_tvg_id')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(stream_channel_id, epg_channel_id)
);

-- Proxy Generations Table (versioned M3U outputs)
CREATE TABLE proxy_generations (
    id TEXT PRIMARY KEY NOT NULL,
    proxy_id TEXT NOT NULL REFERENCES stream_proxies(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    channel_count INTEGER NOT NULL DEFAULT 0,
    m3u_content TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(proxy_id, version)
);

-- Data Mapping Rules Table (supports both stream and EPG transformation)
CREATE TABLE data_mapping_rules (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    source_type TEXT NOT NULL DEFAULT 'stream' CHECK (source_type IN ('stream', 'epg')),
    scope TEXT NOT NULL DEFAULT 'individual' CHECK (scope IN ('individual', 'streamwide', 'epgwide')),
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    expression TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Data Mapping Conditions Table
CREATE TABLE data_mapping_conditions (
    id TEXT PRIMARY KEY NOT NULL,
    rule_id TEXT NOT NULL REFERENCES data_mapping_rules(id) ON DELETE CASCADE,
    field_name TEXT NOT NULL,
    operator TEXT NOT NULL CHECK (operator IN ('matches', 'equals', 'contains', 'starts_with', 'ends_with', 'not_matches', 'not_equals', 'not_contains')),
    value TEXT NOT NULL,
    logical_operator TEXT CHECK (logical_operator IN ('and', 'or', 'all', 'any')),
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Data Mapping Actions Table (enhanced with EPG-specific actions)
CREATE TABLE data_mapping_actions (
    id TEXT PRIMARY KEY NOT NULL,
    rule_id TEXT NOT NULL REFERENCES data_mapping_rules(id) ON DELETE CASCADE,
    action_type TEXT NOT NULL CHECK (action_type IN ('set_value', 'set_default_if_empty', 'set_logo', 'timeshift_epg', 'deduplicate_stream_urls', 'remove_channel')),
    target_field TEXT NOT NULL,
    value TEXT,
    logo_asset_id TEXT,
    timeshift_minutes INTEGER, -- For timeshift EPG action

    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Logo Assets Table
CREATE TABLE logo_assets (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    file_name TEXT NOT NULL,
    file_path TEXT NOT NULL UNIQUE,
    file_size INTEGER NOT NULL,
    mime_type TEXT NOT NULL,
    asset_type TEXT NOT NULL CHECK (asset_type IN ('uploaded', 'cached')),
    source_url TEXT,
    width INTEGER,
    height INTEGER,
    parent_asset_id TEXT,
    format_type TEXT DEFAULT 'original' CHECK (format_type IN ('original', 'png_conversion')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Performance Indexes

-- Stream Sources
CREATE INDEX idx_stream_sources_active ON stream_sources(is_active);
CREATE INDEX idx_stream_sources_type ON stream_sources(source_type);

-- EPG Sources
CREATE INDEX idx_epg_sources_active ON epg_sources(is_active);
CREATE INDEX idx_epg_sources_type ON epg_sources(source_type);

-- Stream Proxies
CREATE INDEX idx_stream_proxies_ulid ON stream_proxies(ulid);
CREATE INDEX idx_stream_proxies_active ON stream_proxies(is_active);

-- Channels
CREATE INDEX idx_channels_source_id ON channels(source_id);
CREATE INDEX idx_channels_group_title ON channels(group_title);
CREATE INDEX idx_channels_tvg_id ON channels(tvg_id);

-- EPG Channels
CREATE INDEX idx_epg_channels_source_id ON epg_channels(source_id);
CREATE INDEX idx_epg_channels_channel_id ON epg_channels(source_id, channel_id);

-- EPG Programs
CREATE INDEX idx_epg_programs_source_id ON epg_programs(source_id);
CREATE INDEX idx_epg_programs_channel_id ON epg_programs(source_id, channel_id);
CREATE INDEX idx_epg_programs_time_range ON epg_programs(start_time, end_time);
CREATE INDEX idx_epg_programs_start_time ON epg_programs(start_time);

-- Channel EPG Mapping
CREATE INDEX idx_channel_epg_mapping_stream ON channel_epg_mapping(stream_channel_id);
CREATE INDEX idx_channel_epg_mapping_epg ON channel_epg_mapping(epg_channel_id);

-- Proxy Generations
CREATE INDEX idx_proxy_generations_proxy_version ON proxy_generations(proxy_id, version DESC);

-- Filters
CREATE INDEX idx_filters_source_type ON filters(source_type);

-- Proxy Filters
CREATE INDEX idx_proxy_filters_sort_order ON proxy_filters(proxy_id, sort_order);



-- Data Mapping Rules
CREATE INDEX idx_data_mapping_rules_sort_order ON data_mapping_rules(sort_order);
CREATE INDEX idx_data_mapping_rules_active ON data_mapping_rules(is_active);
CREATE INDEX idx_data_mapping_rules_source_type ON data_mapping_rules(source_type);
CREATE INDEX idx_data_mapping_rules_expression ON data_mapping_rules(expression);

-- Data Mapping Conditions
CREATE INDEX idx_data_mapping_conditions_rule_id ON data_mapping_conditions(rule_id);
CREATE INDEX idx_data_mapping_conditions_sort_order ON data_mapping_conditions(rule_id, sort_order);

-- Data Mapping Actions
CREATE INDEX idx_data_mapping_actions_rule_id ON data_mapping_actions(rule_id);
CREATE INDEX idx_data_mapping_actions_sort_order ON data_mapping_actions(rule_id, sort_order);
CREATE INDEX idx_data_mapping_actions_logo_asset ON data_mapping_actions(logo_asset_id);

-- Logo Assets
CREATE INDEX idx_logo_assets_type ON logo_assets(asset_type);
CREATE INDEX idx_logo_assets_name ON logo_assets(name);
CREATE INDEX idx_logo_assets_id ON logo_assets(id);
CREATE INDEX idx_logo_assets_parent ON logo_assets(parent_asset_id);
CREATE INDEX idx_logo_assets_format_type ON logo_assets(format_type);

-- Automatic Timestamp Updates

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

CREATE TRIGGER data_mapping_rules_updated_at
    AFTER UPDATE ON data_mapping_rules
    FOR EACH ROW
BEGIN
    UPDATE data_mapping_rules SET updated_at = datetime('now') WHERE id = NEW.id;
END;

CREATE TRIGGER logo_assets_updated_at
    AFTER UPDATE ON logo_assets
    FOR EACH ROW
BEGIN
    UPDATE logo_assets SET updated_at = datetime('now') WHERE id = NEW.id;
END;
