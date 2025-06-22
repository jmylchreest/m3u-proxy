-- Default filters with nested condition trees for complex expressions

-- SD Quality Channels (720p and below)
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    condition_tree,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440001',
    'SD Quality Channels',
    1,
    FALSE,
    'all',
    '{"type":"group","operator":"all","children":[{"type":"condition","field":"channel_name","operator":"matches","value":"(sd|480p|576p|720p)(?![0-9])"},{"type":"condition","field":"channel_name","operator":"not_matches","value":"(hd|1080|4k|uhd|8k)"}]}',
    datetime('now'),
    datetime('now')
);

-- HD Quality Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    condition_tree,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440002',
    'HD Quality Channels',
    1,
    FALSE,
    'all',
    '{"type":"condition","field":"channel_name","operator":"matches","value":"(hd|1080p?|full.?hd)(?![0-9])"}',
    datetime('now'),
    datetime('now')
);

-- 4K Quality Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    condition_tree,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440003',
    '4K Quality Channels',
    1,
    FALSE,
    'all',
    '{"type":"condition","field":"channel_name","operator":"matches","value":"(4k|uhd|ultra.?hd|2160p?)(?![0-9])"}',
    datetime('now'),
    datetime('now')
);

-- 8K Quality Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    condition_tree,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440004',
    '8K Quality Channels',
    1,
    FALSE,
    'all',
    '{"type":"condition","field":"channel_name","operator":"matches","value":"(8k|4320p?)(?![0-9])"}',
    datetime('now'),
    datetime('now')
);

-- Adult Content (Inverse Filter - Excludes matching channels)
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    condition_tree,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440005',
    'Adult Content',
    1,
    TRUE,
    'any',
    '{"type":"group","operator":"any","children":[{"type":"condition","field":"group_title","operator":"matches","value":"(adult|xxx|18\\\\+|for.?adults|erotic|porn|sex|mature)"},{"type":"condition","field":"channel_name","operator":"matches","value":"(adult|xxx|18\\\\+|for.?adults|erotic|porn|sex|mature|playboy|hustler|penthouse)"}]}',
    datetime('now'),
    datetime('now')
);

-- Sports Channels with complex nested conditions
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    condition_tree,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440006',
    'Sports Channels',
    1,
    FALSE,
    'any',
    '{"type":"group","operator":"any","children":[{"type":"condition","field":"group_title","operator":"matches","value":"(sport|football|soccer|basketball|tennis|cricket|racing|athletics|baseball|hockey)"},{"type":"condition","field":"channel_name","operator":"matches","value":"(sport|football|soccer|basketball|tennis|cricket|racing|espn|fox.?sports|sky.?sports|eurosport|nba|nfl|mlb|nhl|f1|formula|premier.?league|champions.?league|athletics|baseball|hockey)"}]}',
    datetime('now'),
    datetime('now')
);

-- Movies Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    condition_tree,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440007',
    'Movies Channels',
    1,
    FALSE,
    'any',
    '{"type":"group","operator":"any","children":[{"type":"condition","field":"group_title","operator":"matches","value":"(movies?|films?|cinema|hollywood|bollywood)"},{"type":"condition","field":"channel_name","operator":"matches","value":"(movies?|films?|cinema|hollywood|bollywood|hbo|showtime|starz|cinemax)"}]}',
    datetime('now'),
    datetime('now')
);

-- News Channels (example of complex nested expression)
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    condition_tree,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440008',
    'News Channels',
    1,
    FALSE,
    'any',
    '{"type":"group","operator":"any","children":[{"type":"condition","field":"group_title","operator":"matches","value":"(news|noticias|nachrichten|nouvelles|nyheter)"},{"type":"group","operator":"all","children":[{"type":"condition","field":"channel_name","operator":"matches","value":"(news|noticias|nachrichten|nouvelles|nyheter|cnn|bbc|fox.?news|msnbc|sky.?news|euronews|al.?jazeera|reuters|bloomberg)"},{"type":"condition","field":"channel_name","operator":"not_contains","value":"adult"}]}]}',
    datetime('now'),
    datetime('now')
);

-- Entertainment Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    condition_tree,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440009',
    'Entertainment Channels',
    1,
    FALSE,
    'any',
    '{"type":"group","operator":"any","children":[{"type":"condition","field":"group_title","operator":"matches","value":"(entertainment|variety|comedy|drama|reality)"},{"type":"condition","field":"channel_name","operator":"matches","value":"(entertainment|variety|comedy|drama|reality|e!|bravo|tlc|lifetime)"}]}',
    datetime('now'),
    datetime('now')
);

-- Kids/Family Channels
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    condition_tree,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440010',
    'Kids/Family Channels',
    1,
    FALSE,
    'any',
    '{"type":"group","operator":"any","children":[{"type":"condition","field":"group_title","operator":"matches","value":"(kids|children|family|cartoon|anime)"},{"type":"condition","field":"channel_name","operator":"matches","value":"(kids|children|family|cartoon|anime|disney|nickelodeon|cartoon.?network|nick.?jr)"}]}',
    datetime('now'),
    datetime('now')
);

-- Complex example: High-Quality Sports (demonstrates nested AND/OR)
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    condition_tree,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440011',
    'High-Quality Sports',
    1,
    FALSE,
    'all',
    '{"type":"group","operator":"all","children":[{"type":"group","operator":"any","children":[{"type":"condition","field":"channel_name","operator":"contains","value":"sport"},{"type":"condition","field":"group_title","operator":"contains","value":"sport"},{"type":"condition","field":"channel_name","operator":"matches","value":"(espn|fox.?sports|sky.?sports)"}]},{"type":"group","operator":"any","children":[{"type":"condition","field":"channel_name","operator":"contains","value":"hd"},{"type":"condition","field":"channel_name","operator":"contains","value":"4k"}]}]}',
    datetime('now'),
    datetime('now')
);

-- Example with very complex nesting: Premium Content
INSERT INTO filters (
    id,
    name,
    starting_channel_number,
    is_inverse,
    logical_operator,
    condition_tree,
    created_at,
    updated_at
) VALUES (
    '550e8400-e29b-41d4-a716-446655440012',
    'Premium Content',
    1,
    FALSE,
    'all',
    '{"type":"group","operator":"all","children":[{"type":"group","operator":"any","children":[{"type":"condition","field":"channel_name","operator":"matches","value":"(hbo|showtime|starz|cinemax|premium)"},{"type":"group","operator":"all","children":[{"type":"condition","field":"group_title","operator":"contains","value":"movies"},{"type":"condition","field":"channel_name","operator":"matches","value":"(hd|4k)"}]}]},{"type":"condition","field":"channel_name","operator":"not_matches","value":"(adult|xxx|18\\\\+)"}]}',
    datetime('now'),
    datetime('now')
);
