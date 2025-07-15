-- Default Data and Configurations
-- Provides default data mapping rules and filter templates for common use cases
-- Consolidated migration including all fixes and updates

-- =============================================================================
-- DEFAULT DATA MAPPING RULES
-- =============================================================================

-- Default Timeshift Detection Rule (for stream sources)
INSERT INTO data_mapping_rules (id, name, description, source_type, sort_order, is_active, expression, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440001',
    'Default Timeshift Detection (Regex)',
    'Automatically detects timeshift channels (+1, +24, etc.) and sets tvg-shift field using regex capture groups.',
    'stream',
    1,
    TRUE,
    'channel_name matches ".*(?:(?:\s|^)\+([0-9]+)h?(?:\s|$)|(?:\s|^)(-[0-9]+)h?(?:\s|$)).*" AND channel_name not_matches ".*(?:start:|stop:|\d{4}-\d{2}-\d{2}|\d{2}:\d{2}:\d{2}|\d{2}-\d).*" AND tvg_id matches "^.+$" SET tvg_shift = "$1$2"',
    datetime('now'),
    datetime('now')
);



-- =============================================================================
-- DEFAULT FILTER TEMPLATES
-- =============================================================================

-- Hide Adult Content Filter (Exclude channels containing adult content)
INSERT INTO filters (id, name, source_type, starting_channel_number, is_inverse, condition_tree, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440010',
    'Hide Adult Content',
    'stream',
    1,
    TRUE, -- Exclude filter: removes channels that match conditions
    '{"root":{"type":"group","operator":"or","children":[{"type":"condition","field":"group_title","operator":"contains","value":"Adult","case_sensitive":false,"negate":false},{"type":"condition","field":"group_title","operator":"contains","value":"XXX","case_sensitive":false,"negate":false},{"type":"condition","field":"group_title","operator":"contains","value":"18+","case_sensitive":false,"negate":false},{"type":"condition","field":"channel_name","operator":"contains","value":"Adult","case_sensitive":false,"negate":false}]}}',
    datetime('now'),
    datetime('now')
);

-- Sports Channels Only Filter
INSERT INTO filters (id, name, source_type, starting_channel_number, is_inverse, condition_tree, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440020',
    'Sports Channels Only',
    'stream',
    1,
    FALSE, -- Include filter: includes channels that match conditions
    '{"root":{"type":"group","operator":"or","children":[{"type":"condition","field":"group_title","operator":"contains","value":"Sport","case_sensitive":false,"negate":false},{"type":"condition","field":"group_title","operator":"contains","value":"ESPN","case_sensitive":false,"negate":false},{"type":"condition","field":"channel_name","operator":"contains","value":"Sport","case_sensitive":false,"negate":false},{"type":"condition","field":"group_title","operator":"contains","value":"Football","case_sensitive":false,"negate":false}]}}',
    datetime('now'),
    datetime('now')
);

-- HD Channels Only Filter
INSERT INTO filters (id, name, source_type, starting_channel_number, is_inverse, condition_tree, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440030',
    'HD Channels Only',
    'stream',
    1,
    FALSE, -- Include filter: includes channels that match conditions
    '{"root":{"type":"group","operator":"or","children":[{"type":"condition","field":"channel_name","operator":"contains","value":"HD","case_sensitive":false,"negate":false},{"type":"condition","field":"channel_name","operator":"contains","value":"FHD","case_sensitive":false,"negate":false},{"type":"condition","field":"channel_name","operator":"contains","value":"4K","case_sensitive":false,"negate":false}]}}',
    datetime('now'),
    datetime('now')
);

-- English Channels Only Filter
INSERT INTO filters (id, name, source_type, starting_channel_number, is_inverse, condition_tree, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440040',
    'English Channels Only',
    'stream',
    1,
    FALSE, -- Include filter: includes channels that match conditions
    '{"root":{"type":"group","operator":"or","children":[{"type":"condition","field":"channel_name","operator":"contains","value":"US","case_sensitive":false,"negate":false},{"type":"condition","field":"channel_name","operator":"contains","value":"UK","case_sensitive":false,"negate":false},{"type":"condition","field":"channel_name","operator":"contains","value":"EN","case_sensitive":false,"negate":false},{"type":"condition","field":"channel_name","operator":"contains","value":"BBC","case_sensitive":false,"negate":false}]}}',
    datetime('now'),
    datetime('now')
);

-- Remove Low Quality Channels Filter (Exclude channels with quality indicators)
INSERT INTO filters (id, name, source_type, starting_channel_number, is_inverse, condition_tree, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440050',
    'Remove Low Quality Channels',
    'stream',
    1,
    TRUE, -- Exclude filter: removes channels that match conditions
    '{"root":{"type":"group","operator":"or","children":[{"type":"condition","field":"channel_name","operator":"contains","value":"SD","case_sensitive":false,"negate":false},{"type":"condition","field":"channel_name","operator":"contains","value":"LOW","case_sensitive":false,"negate":false},{"type":"condition","field":"channel_name","operator":"contains","value":"240p","case_sensitive":false,"negate":false},{"type":"condition","field":"channel_name","operator":"contains","value":"360p","case_sensitive":false,"negate":false}]}}',
    datetime('now'),
    datetime('now')
);

-- Channels with valid stream URLs filter
INSERT INTO filters (id, name, source_type, starting_channel_number, is_inverse, condition_tree, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440060',
    'Channels with valid stream URLs',
    'stream',
    1,
    FALSE, -- Include filter: includes channels that match conditions
    '{"root":{"type":"condition","field":"stream_url","operator":"contains","value":"http","case_sensitive":false,"negate":false}}',
    datetime('now'),
    datetime('now')
);

-- =============================================================================
-- MIGRATION NOTES (for documentation)
-- =============================================================================

-- Note about simplified timezone handling
INSERT INTO migration_notes (migration_id, note) VALUES
(1, 'Initial schema with simplified timezone handling: all times stored as UTC, original_timezone field for reference only, time_offset applied after UTC normalization');
