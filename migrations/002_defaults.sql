-- Default Data and Rules Migration
-- Provides default data mapping rules for common use cases

-- Insert default timeshift rule using regex capture groups (for stream sources)
INSERT INTO data_mapping_rules (id, name, description, source_type, sort_order, is_active, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440001',
    'Default Timeshift Detection (Regex)',
    'Automatically detects timeshift channels (+1, +24, etc.) and sets tvg-shift field using regex capture groups.',
    'stream',
    1,
    TRUE,
    datetime('now'),
    datetime('now')
);

-- Add condition to match timeshift channels and capture the timeshift value
INSERT INTO data_mapping_conditions (id, rule_id, field_name, operator, value, logical_operator, sort_order, created_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440002',
    '550e8400-e29b-41d4-a716-446655440001',
    'channel_name',
    'matches',
    '.*(?:\+([0-9]+)|(-[0-9]+)).*',
    'and',
    0,
    datetime('now')
);

-- Add set_value action to set tvg-shift field using captured timeshift value
INSERT INTO data_mapping_actions (id, rule_id, action_type, target_field, value, sort_order, created_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440003',
    '550e8400-e29b-41d4-a716-446655440001',
    'set_value',
    'tvg_shift',
    '$1$2',
    0,
    datetime('now')
);

-- Default Filters
-- These provide common filtering templates that users can enable/modify as needed

-- Hide Adult Content Filter (Exclude channels containing adult content)
INSERT INTO filters (id, name, source_type, is_inverse, logical_operator, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440010',
    'Hide Adult Content',
    'stream',
    TRUE, -- Exclude filter: removes channels that match conditions
    'any', -- Exclude if ANY condition matches
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (id, filter_id, field_name, operator, value, sort_order, created_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440011',
    '550e8400-e29b-41d4-a716-446655440010',
    'group_title',
    'contains',
    'Adult',
    0,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440012',
    '550e8400-e29b-41d4-a716-446655440010',
    'group_title',
    'contains',
    'XXX',
    1,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440013',
    '550e8400-e29b-41d4-a716-446655440010',
    'group_title',
    'contains',
    '18+',
    2,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440014',
    '550e8400-e29b-41d4-a716-446655440010',
    'channel_name',
    'contains',
    'Adult',
    3,
    datetime('now')
);

-- Sports Channels Only Filter
INSERT INTO filters (id, name, source_type, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440020',
    'Sports Channels Only',
    'stream',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (id, filter_id, field_name, operator, value, sort_order, created_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440021',
    '550e8400-e29b-41d4-a716-446655440020',
    'group_title',
    'contains',
    'Sport',
    0,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440022',
    '550e8400-e29b-41d4-a716-446655440020',
    'group_title',
    'contains',
    'ESPN',
    1,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440023',
    '550e8400-e29b-41d4-a716-446655440020',
    'channel_name',
    'contains',
    'Sport',
    2,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440024',
    '550e8400-e29b-41d4-a716-446655440020',
    'group_title',
    'contains',
    'Football',
    3,
    datetime('now')
);

-- HD Channels Only Filter
INSERT INTO filters (id, name, source_type, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440030',
    'HD Channels Only',
    'stream',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (id, filter_id, field_name, operator, value, sort_order, created_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440031',
    '550e8400-e29b-41d4-a716-446655440030',
    'channel_name',
    'contains',
    'HD',
    0,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440032',
    '550e8400-e29b-41d4-a716-446655440030',
    'channel_name',
    'contains',
    'FHD',
    1,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440033',
    '550e8400-e29b-41d4-a716-446655440030',
    'channel_name',
    'contains',
    '4K',
    2,
    datetime('now')
);

-- English Channels Only Filter
INSERT INTO filters (id, name, source_type, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440040',
    'English Channels Only',
    'stream',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (id, filter_id, field_name, operator, value, sort_order, created_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440041',
    '550e8400-e29b-41d4-a716-446655440040',
    'group_title',
    'starts_with',
    'UK:',
    0,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440042',
    '550e8400-e29b-41d4-a716-446655440040',
    'group_title',
    'starts_with',
    'US:',
    1,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440043',
    '550e8400-e29b-41d4-a716-446655440040',
    'group_title',
    'contains',
    'English',
    2,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440044',
    '550e8400-e29b-41d4-a716-446655440040',
    'group_title',
    'not_contains',
    'Arabic',
    3,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440045',
    '550e8400-e29b-41d4-a716-446655440040',
    'group_title',
    'not_contains',
    'German',
    4,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440046',
    '550e8400-e29b-41d4-a716-446655440040',
    'group_title',
    'not_contains',
    'French',
    5,
    datetime('now')
);

-- Remove Low Quality Channels Filter (Exclude channels with quality indicators)
INSERT INTO filters (id, name, source_type, is_inverse, logical_operator, created_at, updated_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440050',
    'Remove Low Quality Channels',
    'stream',
    TRUE, -- Exclude filter: removes channels that match conditions
    'any', -- Exclude if ANY condition matches
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (id, filter_id, field_name, operator, value, sort_order, created_at)
VALUES (
    '550e8400-e29b-41d4-a716-446655440051',
    '550e8400-e29b-41d4-a716-446655440050',
    'channel_name',
    'contains',
    'SD',
    0,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440052',
    '550e8400-e29b-41d4-a716-446655440050',
    'channel_name',
    'contains',
    'LOW',
    1,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440053',
    '550e8400-e29b-41d4-a716-446655440050',
    'channel_name',
    'contains',
    '240p',
    2,
    datetime('now')
),
(
    '550e8400-e29b-41d4-a716-446655440054',
    '550e8400-e29b-41d4-a716-446655440050',
    'channel_name',
    'contains',
    '360p',
    3,
    datetime('now')
);