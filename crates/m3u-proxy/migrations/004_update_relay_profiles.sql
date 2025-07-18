-- Add missing audio configuration columns to relay profiles table
-- This migration adds only the missing audio_sample_rate and audio_channels columns

-- Add missing audio configuration columns
ALTER TABLE relay_profiles ADD COLUMN audio_sample_rate INTEGER;
ALTER TABLE relay_profiles ADD COLUMN audio_channels INTEGER;

-- Update existing profiles to use new codec-based approach
UPDATE relay_profiles SET
    video_codec = 'copy',
    audio_codec = 'copy',
    enable_hardware_acceleration = false
WHERE id = '00000000-0000-0000-0000-000000000001'; -- Transport Stream Passthrough

UPDATE relay_profiles SET
    video_codec = 'copy',
    audio_codec = 'copy',
    enable_hardware_acceleration = false
WHERE id = '00000000-0000-0000-0000-000000000002'; -- HLS Conversion

UPDATE relay_profiles SET
    video_codec = 'h264',
    audio_codec = 'copy',
    video_preset = 'fast',
    video_bitrate = 2000,
    enable_hardware_acceleration = true,
    preferred_hwaccel = 'nvenc'
WHERE id = '00000000-0000-0000-0000-000000000003'; -- NVIDIA Hardware Acceleration

UPDATE relay_profiles SET
    video_codec = 'h264',
    audio_codec = 'copy',
    video_bitrate = 2000,
    enable_hardware_acceleration = true,
    preferred_hwaccel = 'vaapi'
WHERE id = '00000000-0000-0000-0000-000000000004'; -- Intel/AMD Hardware Acceleration

UPDATE relay_profiles SET
    video_codec = 'h264',
    audio_codec = 'aac',
    video_preset = 'fast',
    video_bitrate = 2000,
    audio_bitrate = 128,
    enable_hardware_acceleration = false
WHERE id = '00000000-0000-0000-0000-000000000005'; -- H.264 Transcode

UPDATE relay_profiles SET
    video_codec = 'copy',
    audio_codec = 'copy',
    enable_hardware_acceleration = false
WHERE id = '00000000-0000-0000-0000-000000000006'; -- Low Latency HLS

-- Update migration notes
INSERT INTO migration_notes (version, note) VALUES
('004', 'Added missing audio_sample_rate and audio_channels columns to relay profiles table');