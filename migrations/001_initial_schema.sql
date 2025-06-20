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

-- Filters Table
CREATE TABLE filters (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    starting_channel_number INTEGER NOT NULL DEFAULT 1,
    is_inverse BOOLEAN NOT NULL DEFAULT FALSE,
    logical_operator TEXT NOT NULL DEFAULT 'and' CHECK (logical_operator IN ('and', 'or')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Filter Conditions Table
CREATE TABLE filter_conditions (
    id TEXT PRIMARY KEY NOT NULL,
    filter_id TEXT NOT NULL REFERENCES filters(id) ON DELETE CASCADE,
    field_name TEXT NOT NULL,
    operator TEXT NOT NULL,
    value TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (filter_id) REFERENCES filters(id) ON DELETE CASCADE
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

-- Channels Table (ingested data)
CREATE TABLE channels (
    id TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL REFERENCES stream_sources(id) ON DELETE CASCADE,
    tvg_id TEXT,
    tvg_name TEXT,
    tvg_logo TEXT,
    group_title TEXT,
    channel_name TEXT NOT NULL,
    stream_url TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
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

-- Indexes for performance
CREATE INDEX idx_stream_sources_active ON stream_sources(is_active);
CREATE INDEX idx_stream_proxies_ulid ON stream_proxies(ulid);
CREATE INDEX idx_stream_proxies_active ON stream_proxies(is_active);
CREATE INDEX idx_channels_source_id ON channels(source_id);
CREATE INDEX idx_channels_group_title ON channels(group_title);
CREATE INDEX idx_channels_tvg_id ON channels(tvg_id);
CREATE INDEX idx_proxy_generations_proxy_version ON proxy_generations(proxy_id, version DESC);
CREATE INDEX idx_proxy_filters_sort_order ON proxy_filters(proxy_id, sort_order);
CREATE INDEX idx_filter_conditions_filter_id ON filter_conditions(filter_id);
CREATE INDEX idx_filter_conditions_sort_order ON filter_conditions(filter_id, sort_order);

-- Triggers to update 'updated_at' timestamps
CREATE TRIGGER stream_sources_updated_at
    AFTER UPDATE ON stream_sources
BEGIN
    UPDATE stream_sources SET updated_at = datetime('now') WHERE id = NEW.id;
END;

CREATE TRIGGER stream_proxies_updated_at
    AFTER UPDATE ON stream_proxies
BEGIN
    UPDATE stream_proxies SET updated_at = datetime('now') WHERE id = NEW.id;
END;

CREATE TRIGGER filters_updated_at
    AFTER UPDATE ON filters
BEGIN
    UPDATE filters SET updated_at = datetime('now') WHERE id = NEW.id;
END;

CREATE TRIGGER channels_updated_at
    AFTER UPDATE ON channels
BEGIN
    UPDATE channels SET updated_at = datetime('now') WHERE id = NEW.id;
END;
