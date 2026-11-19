CREATE TABLE IF NOT EXISTS downloads (
    id TEXT PRIMARY KEY NOT NULL,
    url TEXT NOT NULL,
    final_url TEXT,
    output_path TEXT,
    part_path TEXT,
    state TEXT NOT NULL,
    content_length INTEGER,
    supports_ranges INTEGER NOT NULL DEFAULT 0,
    etag TEXT,
    last_modified TEXT,
    max_workers INTEGER NOT NULL,
    sync_on_complete INTEGER NOT NULL DEFAULT 0,
    bytes_downloaded INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS segments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    download_id TEXT NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    segment_index INTEGER NOT NULL,
    start_offset INTEGER NOT NULL,
    cursor_offset INTEGER NOT NULL,
    end_offset INTEGER,
    completed INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_segments_download_id ON segments(download_id);
