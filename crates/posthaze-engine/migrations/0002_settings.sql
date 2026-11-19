CREATE TABLE IF NOT EXISTS settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    default_output_dir TEXT NOT NULL,
    max_workers INTEGER NOT NULL,
    max_concurrent_downloads INTEGER NOT NULL,
    sync_on_complete INTEGER NOT NULL DEFAULT 0
);

ALTER TABLE downloads ADD COLUMN output_dir TEXT;
