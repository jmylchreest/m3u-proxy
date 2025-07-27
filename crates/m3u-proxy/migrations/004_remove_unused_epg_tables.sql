-- Remove unused EPG tables and related structures
-- Since we moved to programs-only approach, these tables are no longer needed

-- Drop dependent tables first (foreign key constraints)
DROP TABLE IF EXISTS epg_channel_display_names;
DROP TABLE IF EXISTS channel_epg_mapping;

-- Drop the main epg_channels table
DROP TABLE IF EXISTS epg_channels;

-- Clean up any related indexes that might still exist
-- (The table drops should handle most of these automatically)