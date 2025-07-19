-- M3U Proxy Default Data
-- This file contains all default filters, rules, relay profiles, and system data
-- Last updated: 2025-07-16

-- =============================================================================
-- DEFAULT FILTER TEMPLATES
-- =============================================================================

INSERT INTO filters (id, name, source_type, starting_channel_number, is_inverse, is_system_default, condition_tree) VALUES
-- Include All Valid Stream URLs
('00000000-0000-0000-0000-000000000001',
 'Include All Valid Stream URLs',
 'stream',
 1,
 false,
 true,
 '{
   "root": {
     "type": "condition",
     "field": "stream_url",
     "operator": "starts_with",
     "value": "http"
   }
 }'),

-- Exclude Adult Content
('00000000-0000-0000-0000-000000000002',
 'Exclude Adult Content',
 'stream',
 1,
 true,
 true,
 '{
   "root": {
     "type": "group",
     "operator": "or",
     "children": [
       {"type": "condition", "field": "group_title", "operator": "contains", "value": "adult"},
       {"type": "condition", "field": "group_title", "operator": "contains", "value": "xxx"},
       {"type": "condition", "field": "group_title", "operator": "contains", "value": "porn"},
       {"type": "condition", "field": "channel_name", "operator": "contains", "value": "adult"},
       {"type": "condition", "field": "channel_name", "operator": "contains", "value": "xxx"},
       {"type": "condition", "field": "channel_name", "operator": "contains", "value": "porn"},
       {"type": "condition", "field": "group_title", "operator": "matches", "value": "\\b18\\+\\b"},
       {"type": "condition", "field": "channel_name", "operator": "matches", "value": "\\b18\\+\\b"}
     ]
   }
 }'),

-- HD Only Filter
('00000000-0000-0000-0000-000000000003',
 'HD Only',
 'stream',
 1,
 false,
 true,
 '{
   "root": {
     "type": "group",
     "operator": "or",
     "children": [
       {"type": "condition", "field": "channel_name", "operator": "contains", "value": "HD", "case_sensitive": true},
       {"type": "condition", "field": "channel_name", "operator": "contains", "value": "FHD", "case_sensitive": true},
       {"type": "condition", "field": "channel_name", "operator": "contains", "value": "4K", "case_sensitive": true},
       {"type": "condition", "field": "channel_name", "operator": "matches", "value": "\\b(720p|1080p|1080i|2160p)\\b"}
     ]
   }
 }');

-- =============================================================================
-- DEFAULT RELAY PROFILES (FFmpeg configurations)
-- =============================================================================

INSERT INTO relay_profiles (id, name, description, ffmpeg_args, output_format, segment_duration, max_segments, input_timeout, is_system_default, is_active) VALUES
-- Profile 1: Transport Stream Passthrough
('00000000-0000-0000-0000-000000000001',
 'Transport Stream Passthrough',
 'Direct copy to transport stream with no transcoding',
 '["-i", "{input_url}", "-c", "copy", "-f", "mpegts", "-y", "{output_path}/stream.ts"]',
 'transport_stream', NULL, 1, 30, true, true),

-- Profile 2: HLS Conversion
('00000000-0000-0000-0000-000000000002',
 'HLS Conversion',
 'Convert to HLS format with 6-second segments for broad compatibility',
 '["-i", "{input_url}", "-c", "copy", "-f", "hls", "-hls_time", "{segment_duration}", "-hls_list_size", "{max_segments}", "-hls_flags", "delete_segments", "-hls_segment_filename", "{output_path}/segment_%03d.ts", "-y", "{output_path}/playlist.m3u8"]',
 'hls', 6, 10, 30, true, true),

-- Profile 3: NVIDIA Hardware Acceleration
('00000000-0000-0000-0000-000000000003',
 'NVIDIA Hardware Acceleration',
 'GPU-accelerated encoding using NVENC for high performance',
 '["-hwaccel", "cuda", "-i", "{input_url}", "-c:v", "h264_nvenc", "-c:a", "copy", "-preset", "fast", "-b:v", "2M", "-f", "mpegts", "-y", "{output_path}/stream.ts"]',
 'transport_stream', NULL, 1, 30, true, true),

-- Profile 4: Intel/AMD Hardware Acceleration
('00000000-0000-0000-0000-000000000004',
 'Intel/AMD Hardware Acceleration',
 'GPU-accelerated encoding using VAAPI for Intel/AMD GPUs',
 '["-hwaccel", "vaapi", "-hwaccel_device", "/dev/dri/renderD128", "-i", "{input_url}", "-c:v", "h264_vaapi", "-c:a", "copy", "-b:v", "2M", "-f", "mpegts", "-y", "{output_path}/stream.ts"]',
 'transport_stream', NULL, 1, 30, true, true),

-- Profile 5: Software H.264 Transcoding
('00000000-0000-0000-0000-000000000005',
 'H.264 Transcode',
 'Software transcoding to H.264 with AAC audio',
 '["-i", "{input_url}", "-c:v", "libx264", "-c:a", "aac", "-preset", "fast", "-b:v", "2M", "-b:a", "128k", "-f", "mpegts", "-y", "{output_path}/stream.ts"]',
 'transport_stream', NULL, 1, 30, true, true),

-- Profile 6: Low Latency HLS
('00000000-0000-0000-0000-000000000006',
 'Low Latency HLS',
 'HLS with 2-second segments for reduced latency',
 '["-i", "{input_url}", "-c", "copy", "-f", "hls", "-hls_time", "2", "-hls_list_size", "6", "-hls_flags", "delete_segments", "-hls_segment_filename", "{output_path}/segment_%03d.ts", "-y", "{output_path}/playlist.m3u8"]',
 'hls', 2, 6, 30, true, true);

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
 'channel_name matches ".*[ ](?:\\+([0-9]{1,2})|(-[0-9]{1,2}))([hH]?)(?:$|[ ]).*" AND channel_name not_matches ".*(?:start:|stop:|24[-/]7).*" AND tvg_id matches "^.+$" SET tvg_shift = "$1$2"',
 1,
 true);

-- =============================================================================
-- MIGRATION NOTES
-- =============================================================================

INSERT INTO migration_notes (version, note) VALUES
('001', 'Initial schema with core tables for stream sources, EPG sources, proxies, and filtering'),
('002', 'Default filters and data mapping rules added'),
('003', 'Enhanced metrics tracking with session management and aggregated statistics'),
('004', 'FFmpeg relay system with profiles and channel-specific configurations');
