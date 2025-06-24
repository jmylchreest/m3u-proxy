-- Update EPG sources default cron to match stream sources (every 6 hours instead of 12)
-- This migration changes the default for new EPG sources to refresh every 6 hours

-- Update the table schema default
-- Note: SQLite doesn't support ALTER TABLE ... ALTER COLUMN, so we need to recreate the table

-- First, create a backup of the current data
CREATE TABLE epg_sources_backup AS SELECT * FROM epg_sources;

-- Drop the existing table (this will also drop the indexes and triggers)
DROP TABLE epg_sources;

-- Recreate the table with the updated default
CREATE TABLE epg_sources (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    source_type TEXT NOT NULL CHECK (source_type IN ('xmltv', 'xtream')),
    url TEXT NOT NULL,
    update_cron TEXT NOT NULL DEFAULT '0 0 */6 * * * *', -- Every 6 hours (changed from 12)
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

-- Restore the data
INSERT INTO epg_sources SELECT * FROM epg_sources_backup;

-- Drop the backup table
DROP TABLE epg_sources_backup;

-- Recreate the indexes
CREATE INDEX idx_epg_sources_active ON epg_sources(is_active);
CREATE INDEX idx_epg_sources_type ON epg_sources(source_type);

-- Recreate the trigger
CREATE TRIGGER epg_sources_updated_at
    AFTER UPDATE ON epg_sources
BEGIN
    UPDATE epg_sources SET updated_at = datetime('now') WHERE id = NEW.id;
END;
