-- Convert relay profile IDs from strings to proper UUIDs
-- This migration ensures all relay profiles use standard UUID format

-- Create a temporary table to store the ID mappings
CREATE TEMPORARY TABLE relay_profile_id_mapping (
    old_id TEXT PRIMARY KEY,
    new_id TEXT NOT NULL
);

-- Insert mappings for all existing relay profiles with new UUIDs
-- We use fixed UUIDs so this migration is deterministic
INSERT INTO relay_profile_id_mapping (old_id, new_id) VALUES
    ('av1-aac-default', '01c0a8e0-1234-4567-8901-000000000001'),
    ('copy-passthrough', '01c0a8e0-1234-4567-8901-000000000002'),
    ('h264-aac-hwaccel', '01c0a8e0-1234-4567-8901-000000000003'),
    ('h264-ac3-dolby', '01c0a8e0-1234-4567-8901-000000000004'),
    ('h264-mp3-legacy', '01c0a8e0-1234-4567-8901-000000000005'),
    ('h265-aac-default', '01c0a8e0-1234-4567-8901-000000000006'),
    ('h265-aac-hwaccel', '01c0a8e0-1234-4567-8901-000000000007'),
    ('h265-eac3-enhanced', '01c0a8e0-1234-4567-8901-000000000008'),
    ('h265-main10-aac', '01c0a8e0-1234-4567-8901-000000000009'),
    ('mpeg2-mp3-broadcast', '01c0a8e0-1234-4567-8901-000000000010');

-- Update stream_proxies table to use new UUIDs where relay_profile_id matches old IDs
UPDATE stream_proxies 
SET relay_profile_id = (
    SELECT new_id 
    FROM relay_profile_id_mapping 
    WHERE old_id = stream_proxies.relay_profile_id
) 
WHERE relay_profile_id IN (SELECT old_id FROM relay_profile_id_mapping);

-- Update relay_profiles table with new UUIDs
UPDATE relay_profiles 
SET id = (
    SELECT new_id 
    FROM relay_profile_id_mapping 
    WHERE old_id = relay_profiles.id
) 
WHERE id IN (SELECT old_id FROM relay_profile_id_mapping);

-- Add migration note
INSERT INTO migration_notes (version, note) VALUES
('005', 'Converted relay profile IDs from strings to proper UUIDs for consistency');