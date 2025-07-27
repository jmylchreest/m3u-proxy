-- Remove obsolete starting_channel_number column from filters table
-- This field is no longer used as filters no longer need to specify starting channel numbers

-- Create a new filters table without the starting_channel_number column
CREATE TABLE filters_new (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    source_type TEXT NOT NULL DEFAULT 'stream' CHECK (source_type IN ('stream', 'epg')),
    is_inverse BOOLEAN NOT NULL DEFAULT FALSE,
    is_system_default BOOLEAN NOT NULL DEFAULT FALSE,
    condition_tree TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Copy existing data from old table to new table, excluding starting_channel_number
INSERT INTO filters_new (id, name, source_type, is_inverse, is_system_default, condition_tree, created_at, updated_at)
SELECT id, name, source_type, is_inverse, is_system_default, condition_tree, created_at, updated_at
FROM filters;

-- Drop the old table
DROP TABLE filters;

-- Rename the new table to the original name
ALTER TABLE filters_new RENAME TO filters;

-- Recreate any indexes that existed on the filters table
CREATE INDEX IF NOT EXISTS idx_filters_name ON filters(name);
CREATE INDEX IF NOT EXISTS idx_filters_source_type ON filters(source_type);
CREATE INDEX IF NOT EXISTS idx_filters_is_system_default ON filters(is_system_default);