-- Composite unique index for fast Segment checkpoint updates, splits, and ordered fetches.
CREATE UNIQUE INDEX IF NOT EXISTS idx_segments_download_seg
    ON segments(download_id, segment_index);

-- Redundant prefix index superseded by idx_segments_download_seg.
DROP INDEX IF EXISTS idx_segments_download_id;
