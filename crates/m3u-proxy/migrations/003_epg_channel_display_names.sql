-- Migration 003: Add EPG Channel Display Names table
-- This adds support for storing multiple display names per EPG channel
-- to properly handle XMLTV channels that have names in multiple languages

-- Create table for storing multiple display names per EPG channel
CREATE TABLE epg_channel_display_names (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    epg_channel_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    language TEXT, -- Language code (e.g., 'en', 'es', 'fr') - NULL for primary/unspecified
    is_primary BOOLEAN NOT NULL DEFAULT FALSE, -- Whether this is the primary display name
    created_at TEXT NOT NULL DEFAULT (datetime('now', 'utc')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now', 'utc')),
    
    FOREIGN KEY (epg_channel_id) REFERENCES epg_channels(id) ON DELETE CASCADE
);

-- Create indexes for efficient lookups
CREATE INDEX idx_epg_channel_display_names_channel_id ON epg_channel_display_names(epg_channel_id);
CREATE INDEX idx_epg_channel_display_names_language ON epg_channel_display_names(language);
CREATE INDEX idx_epg_channel_display_names_primary ON epg_channel_display_names(is_primary) WHERE is_primary = TRUE;

-- Create composite index for common queries
CREATE INDEX idx_epg_channel_display_names_channel_lang ON epg_channel_display_names(epg_channel_id, language);

-- Update trigger to automatically update the updated_at timestamp
CREATE TRIGGER update_epg_channel_display_names_updated_at
    AFTER UPDATE ON epg_channel_display_names
BEGIN
    UPDATE epg_channel_display_names 
    SET updated_at = datetime('now', 'utc')
    WHERE id = NEW.id;
END;