-- Enhanced Relay Profiles Migration
-- Adds codec-based profile structure with hardware acceleration support
-- Migration Date: 2025-07-18

-- =============================================================================
-- MIGRATE RELAY PROFILES TABLE
-- =============================================================================

-- Create new relay profiles table with enhanced structure
CREATE TABLE relay_profiles_new (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    
    -- TS-compatible codec selection
    video_codec TEXT NOT NULL DEFAULT 'h264' CHECK (video_codec IN ('h264', 'h265', 'av1', 'mpeg2', 'mpeg4', 'copy')),
    audio_codec TEXT NOT NULL DEFAULT 'aac' CHECK (audio_codec IN ('aac', 'mp3', 'ac3', 'eac3', 'mpeg2audio', 'dts', 'copy')),
    video_profile TEXT, -- 'main', 'main10', 'high'
    video_preset TEXT,  -- 'fast', 'medium', 'slow'
    video_bitrate INTEGER, -- kbps
    audio_bitrate INTEGER, -- kbps
    
    -- Hardware acceleration
    enable_hardware_acceleration BOOLEAN NOT NULL DEFAULT FALSE,
    preferred_hwaccel TEXT, -- 'auto', 'vaapi', 'nvenc', 'qsv', 'amf'
    
    -- Manual override and legacy support
    manual_args TEXT,   -- User-defined args override (JSON)
    ffmpeg_args TEXT,   -- Legacy: JSON array of FFmpeg arguments
    
    -- Container and streaming settings
    output_format TEXT NOT NULL DEFAULT 'transport_stream' CHECK (output_format IN ('transport_stream', 'hls', 'dash', 'copy')),
    segment_duration INTEGER, -- For segmented formats (seconds)
    max_segments INTEGER,     -- For circular buffer
    input_timeout INTEGER NOT NULL DEFAULT 30,
    
    -- System flags
    is_system_default BOOLEAN NOT NULL DEFAULT FALSE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Migrate existing data from old table
INSERT INTO relay_profiles_new (
    id, name, description,
    video_codec, audio_codec,
    ffmpeg_args,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active, created_at, updated_at
)
SELECT 
    id, name, description,
    'copy', 'copy', -- Default to copy for existing profiles
    ffmpeg_args,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active, created_at, updated_at
FROM relay_profiles;

-- Drop old table and rename new one
DROP TABLE relay_profiles;
ALTER TABLE relay_profiles_new RENAME TO relay_profiles;

-- =============================================================================
-- RECREATE TRIGGERS
-- =============================================================================

-- Recreate the updated_at trigger for relay profiles
CREATE TRIGGER relay_profiles_updated_at
    AFTER UPDATE ON relay_profiles
    FOR EACH ROW
    BEGIN
        UPDATE relay_profiles SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

-- =============================================================================
-- CREATE PREDEFINED PROFILES
-- =============================================================================

-- Clear existing profiles and add new codec-based ones
DELETE FROM relay_profiles WHERE is_system_default = TRUE;

-- H.264 + AAC (Maximum compatibility)
INSERT INTO relay_profiles (
    id, name, description,
    video_codec, audio_codec,
    video_profile, video_preset,
    video_bitrate, audio_bitrate,
    enable_hardware_acceleration, preferred_hwaccel,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active
) VALUES (
    '11111111-1111-1111-1111-111111111111', 'H.264 + AAC', 'Maximum compatibility profile with H.264 video and AAC audio',
    'h264', 'aac',
    'main', 'fast',
    2000, 128,
    FALSE, NULL,
    'transport_stream', 30, 10, 30,
    TRUE, TRUE
);

-- H.265 + AAC (Better compression)
INSERT INTO relay_profiles (
    id, name, description,
    video_codec, audio_codec,
    video_profile, video_preset,
    video_bitrate, audio_bitrate,
    enable_hardware_acceleration, preferred_hwaccel,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active
) VALUES (
    'h265-aac-default', 'H.265 + AAC', 'Better compression with H.265 video and AAC audio',
    'h265', 'aac',
    'main', 'fast',
    1500, 128,
    FALSE, NULL,
    'transport_stream', 30, 10, 30,
    TRUE, TRUE
);

-- H.265 Main10 + AAC (10-bit color depth)
INSERT INTO relay_profiles (
    id, name, description,
    video_codec, audio_codec,
    video_profile, video_preset,
    video_bitrate, audio_bitrate,
    enable_hardware_acceleration, preferred_hwaccel,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active
) VALUES (
    'h265-main10-aac', 'H.265 Main10 + AAC', 'H.265 with 10-bit color depth and AAC audio',
    'h265', 'aac',
    'main10', 'fast',
    1800, 128,
    FALSE, NULL,
    'transport_stream', 30, 10, 30,
    TRUE, TRUE
);

-- AV1 + AAC (Next-gen compression)
INSERT INTO relay_profiles (
    id, name, description,
    video_codec, audio_codec,
    video_profile, video_preset,
    video_bitrate, audio_bitrate,
    enable_hardware_acceleration, preferred_hwaccel,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active
) VALUES (
    'av1-aac-default', 'AV1 + AAC', 'Next-generation compression with AV1 video and AAC audio',
    'av1', 'aac',
    NULL, 'fast',
    1200, 128,
    FALSE, NULL,
    'transport_stream', 30, 10, 30,
    TRUE, TRUE
);

-- H.264 + MP3 (Legacy compatibility)
INSERT INTO relay_profiles (
    id, name, description,
    video_codec, audio_codec,
    video_profile, video_preset,
    video_bitrate, audio_bitrate,
    enable_hardware_acceleration, preferred_hwaccel,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active
) VALUES (
    'h264-mp3-legacy', 'H.264 + MP3', 'Legacy compatibility with H.264 video and MP3 audio',
    'h264', 'mp3',
    'main', 'fast',
    2000, 128,
    FALSE, NULL,
    'transport_stream', 30, 10, 30,
    TRUE, TRUE
);

-- H.264 + AC3 (Dolby Digital)
INSERT INTO relay_profiles (
    id, name, description,
    video_codec, audio_codec,
    video_profile, video_preset,
    video_bitrate, audio_bitrate,
    enable_hardware_acceleration, preferred_hwaccel,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active
) VALUES (
    'h264-ac3-dolby', 'H.264 + AC3', 'H.264 video with Dolby Digital AC3 audio',
    'h264', 'ac3',
    'main', 'fast',
    2000, 192,
    FALSE, NULL,
    'transport_stream', 30, 10, 30,
    TRUE, TRUE
);

-- H.265 + EAC3 (Enhanced Dolby Digital)
INSERT INTO relay_profiles (
    id, name, description,
    video_codec, audio_codec,
    video_profile, video_preset,
    video_bitrate, audio_bitrate,
    enable_hardware_acceleration, preferred_hwaccel,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active
) VALUES (
    'h265-eac3-enhanced', 'H.265 + EAC3', 'H.265 video with Enhanced Dolby Digital audio',
    'h265', 'eac3',
    'main', 'fast',
    1500, 256,
    FALSE, NULL,
    'transport_stream', 30, 10, 30,
    TRUE, TRUE
);

-- MPEG-2 + MP3 (Legacy broadcast)
INSERT INTO relay_profiles (
    id, name, description,
    video_codec, audio_codec,
    video_profile, video_preset,
    video_bitrate, audio_bitrate,
    enable_hardware_acceleration, preferred_hwaccel,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active
) VALUES (
    'mpeg2-mp3-broadcast', 'MPEG-2 + MP3', 'Legacy broadcast standard with MPEG-2 video and MP3 audio',
    'mpeg2', 'mp3',
    NULL, NULL,
    3000, 128,
    FALSE, NULL,
    'transport_stream', 30, 10, 30,
    TRUE, TRUE
);

-- Copy (Pass-through, no transcoding)
INSERT INTO relay_profiles (
    id, name, description,
    video_codec, audio_codec,
    video_profile, video_preset,
    video_bitrate, audio_bitrate,
    enable_hardware_acceleration, preferred_hwaccel,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active
) VALUES (
    'copy-passthrough', 'Copy', 'Pass-through mode with no transcoding',
    'copy', 'copy',
    NULL, NULL,
    NULL, NULL,
    FALSE, NULL,
    'transport_stream', 30, 10, 30,
    TRUE, TRUE
);

-- =============================================================================
-- HARDWARE ACCELERATION PROFILES
-- =============================================================================

-- H.264 + AAC with Auto Hardware Acceleration
INSERT INTO relay_profiles (
    id, name, description,
    video_codec, audio_codec,
    video_profile, video_preset,
    video_bitrate, audio_bitrate,
    enable_hardware_acceleration, preferred_hwaccel,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active
) VALUES (
    'h264-aac-hwaccel', 'H.264 + AAC (HW Accel)', 'H.264 with automatic hardware acceleration',
    'h264', 'aac',
    'main', 'fast',
    2000, 128,
    TRUE, 'auto',
    'transport_stream', 30, 10, 30,
    TRUE, TRUE
);

-- H.265 + AAC with Auto Hardware Acceleration
INSERT INTO relay_profiles (
    id, name, description,
    video_codec, audio_codec,
    video_profile, video_preset,
    video_bitrate, audio_bitrate,
    enable_hardware_acceleration, preferred_hwaccel,
    output_format, segment_duration, max_segments, input_timeout,
    is_system_default, is_active
) VALUES (
    'h265-aac-hwaccel', 'H.265 + AAC (HW Accel)', 'H.265 with automatic hardware acceleration',
    'h265', 'aac',
    'main', 'fast',
    1500, 128,
    TRUE, 'auto',
    'transport_stream', 30, 10, 30,
    TRUE, TRUE
);