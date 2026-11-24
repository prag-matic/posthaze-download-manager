CREATE TABLE IF NOT EXISTS folder_history (
    path TEXT PRIMARY KEY NOT NULL,
    last_used_at INTEGER NOT NULL
);
