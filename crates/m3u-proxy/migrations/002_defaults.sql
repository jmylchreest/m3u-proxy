-- M3U Proxy Default Data
-- This file contains all default filters, rules, relay profiles, and system data
-- Last updated: 2025-07-16

-- =============================================================================
-- DEFAULT FILTER TEMPLATES
-- =============================================================================

INSERT INTO filters (id, name, source_type, is_inverse, is_system_default, expression) VALUES
-- Include All Valid Stream URLs
('00000000-0000-0000-0000-000000000001',
 'Include All Valid Stream URLs',
 'stream',
 false,
 true,
 '(stream_url starts_with "http")'),

-- Exclude Adult Content
('00000000-0000-0000-0000-000000000002',
 'Exclude Adult Content',
 'stream',
 true,
 true,
 '(group_title contains "adult" OR group_title contains "xxx" OR group_title contains "porn" OR channel_name contains "adult" OR channel_name contains "xxx" OR channel_name contains "porn" OR group_title matches "\\b18\\+\\b" OR channel_name matches "\\b18\\+\\b")'),

-- HD Only Filter
('00000000-0000-0000-0000-000000000003',
 'HD Only',
 'stream',
 false,
 true,
 '(channel_name contains "HD" OR channel_name contains "FHD" OR channel_name contains "4K" OR channel_name matches "\\b(720p|1080p|1080i|2160p)\\b")');

-- =============================================================================
-- DEFAULT RELAY PROFILES (Modern codec-based configurations)
-- =============================================================================

INSERT INTO relay_profiles (
    id, name, description,
    video_codec, audio_codec,
    video_profile, video_preset,
    video_bitrate, audio_bitrate, audio_sample_rate, audio_channels,
    enable_hardware_acceleration, preferred_hwaccel,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active
) VALUES
-- H.264 + AAC (Maximum compatibility)
('01c0a8e0-1234-4567-8901-000000000001',
 'H.264 + AAC',
 'Maximum compatibility profile with H.264 video and AAC audio',
 'h264', 'aac',
 'main', 'fast',
 2000, 128, 48000, 2,
 FALSE, NULL,
 'transport_stream', NULL, NULL, 30,
 TRUE, TRUE),

-- H.265 + AAC (Better compression)
('01c0a8e0-1234-4567-8901-000000000002',
 'H.265 + AAC',
 'Better compression with H.265 video and AAC audio',
 'h265', 'aac',
 'main', 'fast',
 1500, 128, 48000, 2,
 FALSE, NULL,
 'transport_stream', NULL, NULL, 30,
 TRUE, TRUE),

-- H.265 Main10 + AAC (10-bit color depth)
('01c0a8e0-1234-4567-8901-000000000003',
 'H.265 Main10 + AAC',
 'H.265 with 10-bit color depth and AAC audio',
 'h265', 'aac',
 'main10', 'fast',
 1800, 128, 48000, 2,
 FALSE, NULL,
 'transport_stream', NULL, NULL, 30,
 TRUE, TRUE),

-- AV1 + AAC (Next-gen compression)
('01c0a8e0-1234-4567-8901-000000000004',
 'AV1 + AAC',
 'Next-generation compression with AV1 video and AAC audio',
 'av1', 'aac',
 NULL, 'fast',
 1200, 128, 48000, 2,
 FALSE, NULL,
 'transport_stream', NULL, NULL, 30,
 TRUE, TRUE),

-- H.264 + MP3 (Legacy compatibility)
('01c0a8e0-1234-4567-8901-000000000005',
 'H.264 + MP3',
 'Legacy compatibility with H.264 video and MP3 audio',
 'h264', 'mp3',
 'main', 'fast',
 2000, 128, 48000, 2,
 FALSE, NULL,
 'transport_stream', NULL, NULL, 30,
 TRUE, TRUE),

-- H.264 + AC3 (Dolby Digital)
('01c0a8e0-1234-4567-8901-000000000006',
 'H.264 + AC3',
 'H.264 video with Dolby Digital AC3 audio',
 'h264', 'ac3',
 'main', 'fast',
 2000, 192, 48000, 2,
 FALSE, NULL,
 'transport_stream', NULL, NULL, 30,
 TRUE, TRUE),

-- H.265 + EAC3 (Enhanced Dolby Digital)
('01c0a8e0-1234-4567-8901-000000000007',
 'H.265 + EAC3',
 'H.265 video with Enhanced Dolby Digital audio',
 'h265', 'eac3',
 'main', 'fast',
 1500, 256, 48000, 2,
 FALSE, NULL,
 'transport_stream', NULL, NULL, 30,
 TRUE, TRUE),

-- MPEG-2 + MP3 (Legacy broadcast)
('01c0a8e0-1234-4567-8901-000000000008',
 'MPEG-2 + MP3',
 'Legacy broadcast standard with MPEG-2 video and MP3 audio',
 'mpeg2', 'mp3',
 NULL, NULL,
 3000, 128, 48000, 2,
 FALSE, NULL,
 'transport_stream', NULL, NULL, 30,
 TRUE, TRUE),

-- Copy (Pass-through, no transcoding)
('01c0a8e0-1234-4567-8901-000000000009',
 'Copy',
 'Pass-through mode with no transcoding',
 'copy', 'copy',
 NULL, NULL,
 NULL, NULL, NULL, NULL,
 FALSE, NULL,
 'transport_stream', NULL, NULL, 30,
 TRUE, TRUE),

-- H.264 + AAC with Auto Hardware Acceleration
('01c0a8e0-1234-4567-8901-000000000010',
 'H.264 + AAC (HW Accel)',
 'H.264 with automatic hardware acceleration',
 'h264', 'aac',
 'main', 'fast',
 2000, 128, 48000, 2,
 TRUE, 'auto',
 'transport_stream', NULL, NULL, 30,
 TRUE, TRUE),

-- H.265 + AAC with Auto Hardware Acceleration
('01c0a8e0-1234-4567-8901-000000000011',
 'H.265 + AAC (HW Accel)',
 'H.265 with automatic hardware acceleration',
 'h265', 'aac',
 'main', 'fast',
 1500, 128, 48000, 2,
 TRUE, 'auto',
 'transport_stream', NULL, NULL, 30,
 TRUE, TRUE);

-- =============================================================================
-- DEFAULT DATA MAPPING RULES
-- =============================================================================

INSERT INTO data_mapping_rules (id, name, description, source_type, scope, expression, sort_order, is_active) VALUES
-- Timeshift Channel Detection and Processing (Advanced Regex)
('550e8400-e29b-41d4-a716-446655440001',
 'Default Timeshift Detection (Regex)',
 'Automatically detects timeshift channels (+1, +24, etc.) and sets tvg-shift field using regex capture groups.',
 'stream',
 'individual',
 'channel_name matches ".*[ ](?:\+([0-9]{1,2})|(-[0-9]{1,2}))([hH]?)(?:$|[ ]).*" AND channel_name not_matches ".*(?:start:|stop:|24[-/]7).*" AND tvg_id matches "^.+$" SET tvg_shift = "$1$2"',
 1,
 true);

-- =============================================================================
-- MIGRATION NOTES
-- =============================================================================

INSERT INTO migration_notes (version, note) VALUES
('001', 'Initial schema with core tables including modern relay profiles structure'),
('002', 'Default filters, data mapping rules, and codec-based relay profiles');
