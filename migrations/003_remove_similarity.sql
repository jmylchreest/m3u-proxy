-- Migration 003: Remove similarity-related database elements
-- This migration removes the channel similarity detection functionality

-- Remove similarity_threshold column from data_mapping_actions table
ALTER TABLE data_mapping_actions DROP COLUMN similarity_threshold;

-- Update the CHECK constraint to remove 'deduplicate_cloned_channel' action type
-- SQLite doesn't support modifying CHECK constraints directly, so we need to recreate the table

-- Create new table without similarity-related elements
CREATE TABLE data_mapping_actions_new (
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

-- Copy data from old table to new table (excluding similarity_threshold column)
-- Also filter out any existing 'deduplicate_cloned_channel' actions
INSERT INTO data_mapping_actions_new (
    id, rule_id, action_type, target_field, value, logo_asset_id,
    timeshift_minutes, sort_order, created_at
)
SELECT
    id, rule_id, action_type, target_field, value, logo_asset_id,
    timeshift_minutes, sort_order, created_at
FROM data_mapping_actions
WHERE action_type != 'deduplicate_cloned_channel';

-- Drop old table and rename new table
DROP TABLE data_mapping_actions;
ALTER TABLE data_mapping_actions_new RENAME TO data_mapping_actions;

-- Recreate any indexes that might have existed on the actions table
-- (The migration system will handle this automatically based on the schema)
