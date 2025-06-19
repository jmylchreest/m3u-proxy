-- Add default "Live Streams" filter to help identify live TV channels
-- This filter excludes common patterns for movies, series, and VOD content

INSERT INTO filters (
    id,
    name,
    pattern,
    starting_channel_number,
    is_inverse,
    created_at,
    updated_at
) VALUES (
    'live-streams-filter-default',
    'Live Streams Only',
    '(?i)(netflix|movies?|films?|series|episodes?|shows?|cinema|movie|film|serie|temporada|season|filmes?|películas?|kino|cinéma|фильм|adult|xxx|18\+|for adults|documentar|reality|docu|anime|kids|cartoon|old|archive|arkiva|ancien|alt|gammal|release|bluray|1080p|720p|4k|uhd|hd|collection|pack|library|biblioteca)',
    1,
    TRUE,
    datetime('now'),
    datetime('now')
);

-- Add a second filter for known live TV patterns (positive filter)
INSERT INTO filters (
    id,
    name,
    pattern,
    starting_channel_number,
    is_inverse,
    created_at,
    updated_at
) VALUES (
    'live-tv-patterns-default',
    'Live TV Channels',
    '(?i)(^[A-Z]{2}\s*[-|]|sport|football|soccer|basketball|tennis|cricket|racing|news|noticias|nachrichten|nouvelles|nyheter|cnn|bbc|espn|sky|live|tv|channel)',
    1,
    FALSE,
    datetime('now'),
    datetime('now')
);

-- Add a filter for country-specific live channels
INSERT INTO filters (
    id,
    name,
    pattern,
    starting_channel_number,
    is_inverse,
    created_at,
    updated_at
) VALUES (
    'country-channels-default',
    'Country TV Channels',
    '^(US|UK|CA|AU|DE|FR|IT|ES|NL|BE|CH|AT|SE|NO|DK|FI|IE|PT|PL|CZ|HU|RO|BG|HR|SI|SK|EE|LV|LT|MT|CY|LU|GR|TR|RU|UA|BY|MD|RS|BA|ME|MK|AL|XK|AM|AZ|GE|KZ|KG|TJ|TM|UZ|AF|PK|IN|BD|LK|MV|NP|BT|MM|TH|LA|VN|KH|MY|SG|BN|ID|TL|PH|CN|TW|HK|MO|JP|KR|KP|MN)\s*[-|]',
    1,
    FALSE,
    datetime('now'),
    datetime('now')
);
