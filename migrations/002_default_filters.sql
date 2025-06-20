-- Add default filters to help users get started with common filtering patterns

-- SD Quality Channels (720p and below)
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440001',
    'SD Quality Channels',
    1,
    FALSE,
    'or',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440011',
    '550e8400-e29b-41d4-a716-446655440001',
    'channel_name',
    'matches',
    '(?i)(sd|480p|576p|720p)(?![0-9])',
    0,
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440012',
    '550e8400-e29b-41d4-a716-446655440001',
    'channel_name',
    'notmatches',
    '(?i)(hd|1080|4k|uhd|8k)',
    1,
    datetime('now')
);

-- HD Quality Channels (1080p)
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440002',
    'HD Quality Channels',
    1,
    FALSE,
    'or',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440021',
    '550e8400-e29b-41d4-a716-446655440002',
    'channel_name',
    'matches',
    '(?i)(hd|1080p?|full.?hd)(?![0-9])',
    0,
    datetime('now')
);

-- 4K Quality Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440003',
    '4K Quality Channels',
    1,
    FALSE,
    'or',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440031',
    '550e8400-e29b-41d4-a716-446655440003',
    'channel_name',
    'matches',
    '(?i)(4k|uhd|ultra.?hd|2160p?)(?![0-9])',
    0,
    datetime('now')
);

-- 8K Quality Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440004',
    '8K Quality Channels',
    1,
    FALSE,
    'or',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440041',
    '550e8400-e29b-41d4-a716-446655440004',
    'channel_name',
    'matches',
    '(?i)(8k|4320p?)(?![0-9])',
    0,
    datetime('now')
);

-- Adult Content (Exclude Filter - Inverse enabled by default)
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440005',
    'Adult Content',
    1,
    TRUE,
    'or',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440051',
    '550e8400-e29b-41d4-a716-446655440005',
    'group_title',
    'matches',
    '(?i)(adult|xxx|18\+|for.?adults|erotic|porn|sex|mature)',
    0,
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440052',
    '550e8400-e29b-41d4-a716-446655440005',
    'channel_name',
    'matches',
    '(?i)(adult|xxx|18\+|for.?adults|erotic|porn|sex|mature|playboy|hustler|penthouse)',
    1,
    datetime('now')
);

-- Sports Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440006',
    'Sports Channels',
    1,
    FALSE,
    'or',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440061',
    '550e8400-e29b-41d4-a716-446655440006',
    'group_title',
    'matches',
    '(?i)(sport|football|soccer|basketball|tennis|cricket|racing|athletics|baseball|hockey)',
    0,
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440062',
    '550e8400-e29b-41d4-a716-446655440006',
    'channel_name',
    'matches',
    '(?i)(sport|football|soccer|basketball|tennis|cricket|racing|espn|fox.?sports|sky.?sports|eurosport|nba|nfl|mlb|nhl|f1|formula|premier.?league|champions.?league|athletics|baseball|hockey)',
    1,
    datetime('now')
);

-- Movies Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440007',
    'Movies Channels',
    1,
    FALSE,
    'or',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440071',
    '550e8400-e29b-41d4-a716-446655440007',
    'group_title',
    'matches',
    '(?i)(movies?|films?|cinema|hollywood|bollywood)',
    0,
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440072',
    '550e8400-e29b-41d4-a716-446655440007',
    'channel_name',
    'matches',
    '(?i)(movies?|films?|cinema|hollywood|bollywood|hbo|showtime|starz|cinemax)',
    1,
    datetime('now')
);

-- Crime/Thriller Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440008',
    'Crime/Thriller Channels',
    1,
    FALSE,
    'or',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440081',
    '550e8400-e29b-41d4-a716-446655440008',
    'group_title',
    'matches',
    '(?i)(crime|thriller|mystery|investigation|detective)',
    0,
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440082',
    '550e8400-e29b-41d4-a716-446655440008',
    'channel_name',
    'matches',
    '(?i)(crime|thriller|mystery|investigation|detective|true.?crime|forensic)',
    1,
    datetime('now')
);

-- Documentary Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440009',
    'Documentary Channels',
    1,
    FALSE,
    'or',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440091',
    '550e8400-e29b-41d4-a716-446655440009',
    'group_title',
    'matches',
    '(?i)(documentar|docu|educational|history|science|nature|wildlife)',
    0,
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440092',
    '550e8400-e29b-41d4-a716-446655440009',
    'channel_name',
    'matches',
    '(?i)(documentar|docu|educational|history|science|nature|wildlife|discovery|national.?geographic|animal.?planet)',
    1,
    datetime('now')
);

-- News Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440010',
    'News Channels',
    1,
    FALSE,
    'or',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440101',
    '550e8400-e29b-41d4-a716-446655440010',
    'group_title',
    'matches',
    '(?i)(news|noticias|nachrichten|nouvelles|nyheter)',
    0,
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440102',
    '550e8400-e29b-41d4-a716-446655440010',
    'channel_name',
    'matches',
    '(?i)(news|noticias|nachrichten|nouvelles|nyheter|cnn|bbc|fox.?news|msnbc|sky.?news|euronews|al.?jazeera|reuters|bloomberg)',
    1,
    datetime('now')
);

-- Entertainment Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440011',
    'Entertainment Channels',
    1,
    FALSE,
    'or',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440111',
    '550e8400-e29b-41d4-a716-446655440011',
    'group_title',
    'matches',
    '(?i)(entertainment|variety|comedy|drama|reality)',
    0,
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440112',
    '550e8400-e29b-41d4-a716-446655440011',
    'channel_name',
    'matches',
    '(?i)(entertainment|variety|comedy|drama|reality|e!|bravo|tlc|lifetime)',
    1,
    datetime('now')
);

-- Kids/Family Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440012',
    'Kids/Family Channels',
    1,
    FALSE,
    'or',
    datetime('now'),
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440121',
    '550e8400-e29b-41d4-a716-446655440012',
    'group_title',
    'matches',
    '(?i)(kids|children|family|cartoon|anime)',
    0,
    datetime('now')
);

INSERT INTO filter_conditions (
    id,
    filter_id,
    field_name,
    operator,
    value,
    sort_order,
    created_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440122',
    '550e8400-e29b-41d4-a716-446655440012',
    'channel_name',
    'matches',
    '(?i)(kids|children|family|cartoon|anime|disney|nickelodeon|cartoon.?network|nick.?jr)',
    1,
    datetime('now')
);
