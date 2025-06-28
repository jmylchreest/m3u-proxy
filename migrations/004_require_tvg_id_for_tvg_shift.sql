-- Migration to add tvg_id requirement for tvg_shift data mapping rules
-- This ensures that tvg_shift is only applied to channels that have a tvg_id,
-- since channels without tvg_id won't show up in EPG anyway

-- Add condition to ensure tvg_id exists and is not empty
-- Using matches operator with a regex that requires at least one character
INSERT INTO data_mapping_conditions (id, rule_id, field_name, operator, value, logical_operator, sort_order, created_at)
VALUES (
    '9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d',
    '550e8400-e29b-41d4-a716-446655440001',
    'tvg_id',
    'matches',
    '^.+$',
    'and',
    2,
    datetime('now')
);
