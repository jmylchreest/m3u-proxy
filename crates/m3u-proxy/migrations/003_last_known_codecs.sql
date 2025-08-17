-- Last Known Codecs Table Migration
-- This table stores the last known codec information for channels
-- Updated whenever mpegts.js player detects codecs, ffprobe runs, or relay probes streams

CREATE TABLE last_known_codecs (
    id TEXT PRIMARY KEY NOT NULL,
    channel_id TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    video_codec TEXT, -- e.g., 'H.264', 'H.265', 'AV1'
    audio_codec TEXT, -- e.g., 'AAC', 'MP3', 'AC3'
    container_format TEXT, -- e.g., 'mp4', 'ts', 'mkv'
    resolution TEXT, -- e.g., '1920x1080', '1280x720'
    framerate TEXT, -- e.g., '25.00', '29.97', '59.94'
    bitrate INTEGER, -- Total bitrate in kbps
    video_bitrate INTEGER, -- Video bitrate in kbps
    audio_bitrate INTEGER, -- Audio bitrate in kbps
    audio_channels TEXT, -- e.g., '2.0', '5.1', '7.1'
    audio_sample_rate INTEGER, -- e.g., 44100, 48000
    probe_method TEXT NOT NULL CHECK (probe_method IN ('mpegts_player', 'ffprobe_manual', 'ffprobe_relay', 'ffprobe_auto')),
    probe_source TEXT, -- Source of the probe: 'frontend', 'relay', 'admin'
    detected_at TEXT NOT NULL DEFAULT (datetime('now')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Index for efficient lookups by channel
CREATE INDEX idx_last_known_codecs_channel_id ON last_known_codecs(channel_id);

-- Index for querying by detection time
CREATE INDEX idx_last_known_codecs_detected_at ON last_known_codecs(detected_at);

-- Index for querying by probe method
CREATE INDEX idx_last_known_codecs_probe_method ON last_known_codecs(probe_method);

-- Trigger to update the updated_at timestamp
CREATE TRIGGER update_last_known_codecs_updated_at
    AFTER UPDATE ON last_known_codecs
    FOR EACH ROW
    BEGIN
        UPDATE last_known_codecs 
        SET updated_at = datetime('now') 
        WHERE id = NEW.id;
    END;