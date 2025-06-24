-- Create EPG DLQ (Dead Letter Queue) table for handling duplicate/conflicting channel data
CREATE TABLE epg_dlq (
    id TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL REFERENCES epg_sources(id) ON DELETE CASCADE,
    original_channel_id TEXT NOT NULL, -- The duplicate channel_id that caused conflict
    conflict_type TEXT NOT NULL, -- 'duplicate_identical' or 'duplicate_conflicting'
    channel_data TEXT NOT NULL, -- JSON blob of the channel data
    program_data TEXT, -- JSON blob of programs for this channel (if any)
    conflict_details TEXT, -- Human readable description of the conflict
    first_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    occurrence_count INTEGER NOT NULL DEFAULT 1,
    resolved BOOLEAN NOT NULL DEFAULT FALSE,
    resolution_notes TEXT
);

-- Index for efficient querying
CREATE INDEX idx_epg_dlq_source_id ON epg_dlq(source_id);
CREATE INDEX idx_epg_dlq_channel_id ON epg_dlq(source_id, original_channel_id);
CREATE INDEX idx_epg_dlq_conflict_type ON epg_dlq(conflict_type);
CREATE INDEX idx_epg_dlq_resolved ON epg_dlq(resolved);

-- Trigger to update last_seen_at when occurrence_count is incremented
CREATE TRIGGER epg_dlq_updated_at
    AFTER UPDATE ON epg_dlq
    WHEN NEW.occurrence_count > OLD.occurrence_count
BEGIN
    UPDATE epg_dlq SET last_seen_at = datetime('now') WHERE id = NEW.id;
END;
