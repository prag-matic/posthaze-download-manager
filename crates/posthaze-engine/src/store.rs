//! SQLite persistence store for `Download`s and `Segment`s.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use sqlx::Row;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteRow,
    SqliteSynchronous,
};

use crate::probe::ProbeResult;
use crate::transfer::{SegmentCheckpoint, plan_segments};
use crate::{DownloadId, DownloadRequest, DownloadState, EngineError, Settings};

const DOWNLOAD_SELECT_COLUMNS: &str = "
    id, url, final_url, output_dir, output_path, part_path, state,
    content_length, supports_ranges, etag, last_modified,
    max_workers, sync_on_complete, request_headers, bytes_downloaded,
    error_message, created_at, updated_at
";

/// Persisted metadata and status for a `Download`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRecord {
    /// Persistent `DownloadId` (UUID v4).
    pub id: DownloadId,
    /// Original source URL requested by the user.
    pub url: String,
    /// Final effective URL after redirects during `Probing`, if completed.
    pub final_url: Option<String>,
    /// Target directory requested when `output_path` was not explicitly set.
    pub output_dir: Option<PathBuf>,
    /// Resolved final output file path, if known.
    pub output_path: Option<PathBuf>,
    /// Temporary `.part` file path, if known.
    pub part_path: Option<PathBuf>,
    /// Current lifecycle state.
    pub state: DownloadState,
    /// Server-reported `Content-Length`, if known.
    pub content_length: Option<u64>,
    /// Whether the server advertised byte-range support.
    pub supports_ranges: bool,
    /// Server `ETag` `Validator`, if present.
    pub etag: Option<String>,
    /// Server `Last-Modified` `Validator`, if present.
    pub last_modified: Option<String>,
    /// Maximum concurrent `Worker`s configured for this `Download`.
    pub max_workers: usize,
    /// Whether `sync_all()` is enabled on completion.
    pub sync_on_complete: bool,
    /// Custom HTTP request headers persisted for probes, transfers, and resume.
    pub headers: Vec<(String, String)>,
    /// Total bytes written to disk so far.
    pub bytes_downloaded: u64,
    /// Error message if the `Download` transitioned to `Failed`.
    pub error_message: Option<String>,
    /// Unix timestamp (seconds) when the record was created.
    pub created_at: i64,
    /// Unix timestamp (seconds) when the record was last updated.
    pub updated_at: i64,
}

/// SQLite-backed persistence store for `Download`s and `Segment`s.
#[derive(Debug, Clone)]
pub(crate) struct DownloadStore {
    pool: SqlitePool,
}

impl DownloadStore {
    /// Opens or creates a SQLite database at `db_path`, enables WAL mode, and runs migrations.
    pub(crate) async fn open(db_path: &Path) -> Result<Self, EngineError> {
        if let Some(parent) = db_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;

        sqlx::migrate!("./migrations").run(&pool).await?;
        let store = Self { pool };
        store.ensure_default_settings().await?;
        store.reconcile_crashed_downloads().await?;
        Ok(store)
    }

    /// Opens an ephemeral in-memory SQLite database and runs embedded migrations.
    pub(crate) async fn in_memory() -> Result<Self, EngineError> {
        let options = SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;

        sqlx::migrate!("./migrations").run(&pool).await?;
        let store = Self { pool };
        store.ensure_default_settings().await?;
        store.reconcile_crashed_downloads().await?;
        Ok(store)
    }

    /// Inserts a newly queued `Download` record.
    pub(crate) async fn insert_queued(
        &self,
        id: &DownloadId,
        request: &DownloadRequest,
    ) -> Result<(), EngineError> {
        self.insert_with_state(id, request, DownloadState::Queued)
            .await
    }

    /// Inserts a newly staged `Paused` `Download` record without starting transfer.
    pub(crate) async fn insert_paused(
        &self,
        id: &DownloadId,
        request: &DownloadRequest,
    ) -> Result<(), EngineError> {
        self.insert_with_state(id, request, DownloadState::Paused)
            .await
    }

    async fn insert_with_state(
        &self,
        id: &DownloadId,
        request: &DownloadRequest,
        state: DownloadState,
    ) -> Result<(), EngineError> {
        let now = unix_now();
        let output_dir = request.output_dir.to_string_lossy().into_owned();
        let output_path = request
            .output_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let request_headers = encode_headers(&request.headers);
        sqlx::query(
            "INSERT INTO downloads (
                id, url, output_dir, output_path, state, max_workers, sync_on_complete,
                request_headers, bytes_downloaded, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(id.as_str())
        .bind(&request.url)
        .bind(output_dir)
        .bind(output_path)
        .bind(state.as_str())
        .bind(request.max_workers as i64)
        .bind(i64::from(request.sync_on_complete))
        .bind(request_headers)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Updates a `Download`'s state and `updated_at` timestamp.
    pub(crate) async fn set_state(
        &self,
        id: &DownloadId,
        state: DownloadState,
    ) -> Result<(), EngineError> {
        sqlx::query("UPDATE downloads SET state = ?, updated_at = ? WHERE id = ?")
            .bind(state.as_str())
            .bind(unix_now())
            .bind(id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Records `Probing` metadata, transitions to `Active`, and inserts initial `Segment` rows.
    pub(crate) async fn record_probed_and_activate(
        &self,
        id: &DownloadId,
        probe: &ProbeResult,
        output_path: &Path,
        part_path: &Path,
        max_workers: usize,
    ) -> Result<(), EngineError> {
        let mut tx = self.pool.begin().await?;
        let now = unix_now();

        sqlx::query(
            "UPDATE downloads SET
                final_url = ?,
                output_path = ?,
                part_path = ?,
                state = ?,
                content_length = ?,
                supports_ranges = ?,
                etag = ?,
                last_modified = ?,
                updated_at = ?
             WHERE id = ?",
        )
        .bind(&probe.final_url)
        .bind(output_path.to_string_lossy().as_ref())
        .bind(part_path.to_string_lossy().as_ref())
        .bind(DownloadState::Active.as_str())
        .bind(probe.content_length.map(|v| v as i64))
        .bind(i64::from(probe.supports_ranges))
        .bind(probe.etag.as_deref())
        .bind(probe.last_modified.as_deref())
        .bind(now)
        .bind(id.as_str())
        .execute(&mut *tx)
        .await?;

        let segments: Vec<(u64, Option<u64>)> = match (probe.supports_ranges, probe.content_length)
        {
            (true, Some(total)) if total > 0 => plan_segments(total, max_workers)
                .into_iter()
                .map(|s| (s.start, Some(s.end)))
                .collect(),
            _ => vec![(0, probe.content_length)],
        };

        for (idx, (start, end)) in segments.into_iter().enumerate() {
            sqlx::query(
                "INSERT INTO segments (
                    download_id, segment_index, start_offset, cursor_offset, end_offset, completed
                 ) VALUES (?, ?, ?, ?, ?, 0)",
            )
            .bind(id.as_str())
            .bind(idx as i64)
            .bind(start as i64)
            .bind(start as i64)
            .bind(end.map(|v| v as i64))
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Marks a `Download` as `Completed` and marks all its `Segment`s completed.
    pub(crate) async fn mark_completed(
        &self,
        id: &DownloadId,
        bytes_downloaded: u64,
    ) -> Result<(), EngineError> {
        let mut tx = self.pool.begin().await?;
        let now = unix_now();

        sqlx::query(
            "UPDATE downloads SET
                state = ?,
                bytes_downloaded = ?,
                error_message = NULL,
                updated_at = ?
             WHERE id = ?",
        )
        .bind(DownloadState::Completed.as_str())
        .bind(bytes_downloaded as i64)
        .bind(now)
        .bind(id.as_str())
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "UPDATE segments SET
                cursor_offset = COALESCE(end_offset, cursor_offset),
                completed = 1
             WHERE download_id = ?",
        )
        .bind(id.as_str())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Marks a `Download` as `Failed` with an error message without advancing `Segment` cursors.
    pub(crate) async fn mark_failed(
        &self,
        id: &DownloadId,
        bytes_downloaded: u64,
        error_message: &str,
    ) -> Result<(), EngineError> {
        sqlx::query(
            "UPDATE downloads SET
                state = ?,
                bytes_downloaded = ?,
                error_message = ?,
                updated_at = ?
             WHERE id = ?",
        )
        .bind(DownloadState::Failed.as_str())
        .bind(bytes_downloaded as i64)
        .bind(error_message)
        .bind(unix_now())
        .bind(id.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Persists writer-acknowledged `Segment` cursors and updates the `Download` state and byte count.
    ///
    /// Returns the total `bytes_downloaded` summed from persisted `Segment` rows.
    pub(crate) async fn finalize_writer_checkpoint(
        &self,
        id: &DownloadId,
        state: DownloadState,
        error_message: Option<&str>,
        writer_cursors: &HashMap<usize, u64>,
    ) -> Result<u64, EngineError> {
        let mut tx = self.pool.begin().await?;
        let now = unix_now();

        for (&segment_index, &cursor) in writer_cursors {
            sqlx::query(
                "UPDATE segments
                 SET cursor_offset = MAX(cursor_offset, ?),
                     completed = CASE
                         WHEN end_offset IS NOT NULL AND MAX(cursor_offset, ?) >= end_offset THEN 1
                         ELSE completed
                     END
                 WHERE download_id = ? AND segment_index = ?",
            )
            .bind(cursor as i64)
            .bind(cursor as i64)
            .bind(id.as_str())
            .bind(segment_index as i64)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            "UPDATE downloads
             SET state = ?,
                 error_message = ?,
                 bytes_downloaded = (
                     SELECT COALESCE(SUM(MAX(0, cursor_offset - start_offset)), 0)
                     FROM segments
                     WHERE download_id = ?
                 ),
                 updated_at = ?
             WHERE id = ?",
        )
        .bind(state.as_str())
        .bind(error_message)
        .bind(id.as_str())
        .bind(now)
        .bind(id.as_str())
        .execute(&mut *tx)
        .await?;

        let total_row = sqlx::query("SELECT bytes_downloaded FROM downloads WHERE id = ?")
            .bind(id.as_str())
            .fetch_one(&mut *tx)
            .await?;
        let total = u64::try_from(total_row.try_get::<i64, _>("bytes_downloaded")?).unwrap_or(0);

        tx.commit().await?;
        Ok(total)
    }

    /// Marks a `Download` as `Cancelled` in SQLite.
    pub(crate) async fn mark_cancelled(&self, id: &DownloadId) -> Result<(), EngineError> {
        sqlx::query("UPDATE downloads SET state = ?, updated_at = ? WHERE id = ?")
            .bind(DownloadState::Cancelled.as_str())
            .bind(unix_now())
            .bind(id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Transitions a resumed `Download` to `Active` and updates its `final_url`.
    pub(crate) async fn mark_resumed_active(
        &self,
        id: &DownloadId,
        final_url: &str,
    ) -> Result<(), EngineError> {
        sqlx::query(
            "UPDATE downloads SET
                final_url = ?,
                state = ?,
                error_message = NULL,
                updated_at = ?
             WHERE id = ?",
        )
        .bind(final_url)
        .bind(DownloadState::Active.as_str())
        .bind(unix_now())
        .bind(id.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Atomically updates the bisected `Segment`'s `end_offset` and inserts the stolen `Segment`.
    pub(crate) async fn split_segment(
        &self,
        id: &DownloadId,
        target_index: usize,
        new_end: u64,
        stolen_index: usize,
        stolen_start: u64,
        stolen_end: u64,
    ) -> Result<(), EngineError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE segments SET end_offset = ? WHERE download_id = ? AND segment_index = ?",
        )
        .bind(new_end as i64)
        .bind(id.as_str())
        .bind(target_index as i64)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO segments (
                download_id, segment_index, start_offset, cursor_offset, end_offset, completed
             ) VALUES (?, ?, ?, ?, ?, 0)",
        )
        .bind(id.as_str())
        .bind(stolen_index as i64)
        .bind(stolen_start as i64)
        .bind(stolen_start as i64)
        .bind(stolen_end as i64)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Reconciles stale `Queued`, `Active`, and `Probing` rows left behind by an ungraceful process exit.
    pub(crate) async fn reconcile_crashed_downloads(&self) -> Result<(), EngineError> {
        let now = unix_now();
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "UPDATE downloads SET state = ?, updated_at = ?
             WHERE state = ? OR (state = ? AND supports_ranges = 1)",
        )
        .bind(DownloadState::Paused.as_str())
        .bind(now)
        .bind(DownloadState::Queued.as_str())
        .bind(DownloadState::Active.as_str())
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "UPDATE downloads SET state = ?, error_message = ?, updated_at = ?
             WHERE state = ? OR (state = ? AND supports_ranges = 0)",
        )
        .bind(DownloadState::Failed.as_str())
        .bind("interrupted before completion")
        .bind(now)
        .bind(DownloadState::Probing.as_str())
        .bind(DownloadState::Active.as_str())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Persists a writer-acknowledged `Checkpoint` without allowing `cursor_offset` to regress.
    pub(crate) async fn apply_writer_checkpoint(
        &self,
        id: &DownloadId,
        segment_cursors: &[(usize, u64)],
    ) -> Result<(), EngineError> {
        if segment_cursors.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;
        for &(segment_index, cursor) in segment_cursors {
            sqlx::query(
                "UPDATE segments
                 SET cursor_offset = MAX(cursor_offset, ?),
                     completed = CASE
                         WHEN end_offset IS NOT NULL AND MAX(cursor_offset, ?) >= end_offset THEN 1
                         ELSE completed
                     END
                 WHERE download_id = ? AND segment_index = ?",
            )
            .bind(cursor as i64)
            .bind(cursor as i64)
            .bind(id.as_str())
            .bind(segment_index as i64)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            "UPDATE downloads
             SET bytes_downloaded = (
                 SELECT COALESCE(SUM(MAX(0, cursor_offset - start_offset)), 0)
                 FROM segments
                 WHERE download_id = ?
             ),
             updated_at = ?
             WHERE id = ? AND state = ?",
        )
        .bind(id.as_str())
        .bind(unix_now())
        .bind(id.as_str())
        .bind(DownloadState::Active.as_str())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Loads all persisted `SegmentCheckpoint`s for a `Download`, ordered by `segment_index`.
    pub(crate) async fn get_segments(
        &self,
        id: &DownloadId,
    ) -> Result<Vec<SegmentCheckpoint>, EngineError> {
        let rows = sqlx::query(
            "SELECT segment_index, start_offset, cursor_offset, end_offset, completed
             FROM segments
             WHERE download_id = ?
             ORDER BY segment_index ASC",
        )
        .bind(id.as_str())
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(SegmentCheckpoint {
                segment_index: usize::try_from(row.try_get::<i64, _>("segment_index")?)
                    .unwrap_or(0),
                start: u64::try_from(row.try_get::<i64, _>("start_offset")?).unwrap_or(0),
                cursor: u64::try_from(row.try_get::<i64, _>("cursor_offset")?).unwrap_or(0),
                end: row
                    .try_get::<Option<i64>, _>("end_offset")?
                    .map(|v| u64::try_from(v).unwrap_or(0)),
                completed: row.try_get::<i64, _>("completed")? != 0,
            });
        }
        Ok(out)
    }

    /// Fetches a single `DownloadRecord` by `DownloadId`.
    pub(crate) async fn get_download(
        &self,
        id: &DownloadId,
    ) -> Result<Option<DownloadRecord>, EngineError> {
        let sql = format!("SELECT {DOWNLOAD_SELECT_COLUMNS} FROM downloads WHERE id = ?");
        let row = sqlx::query(&sql)
            .bind(id.as_str())
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_record).transpose()
    }

    /// Lists all persisted `DownloadRecord`s ordered newest-first.
    pub(crate) async fn list_downloads(&self) -> Result<Vec<DownloadRecord>, EngineError> {
        let sql =
            format!("SELECT {DOWNLOAD_SELECT_COLUMNS} FROM downloads ORDER BY created_at DESC, rowid DESC");
        let rows = sqlx::query(&sql).fetch_all(&self.pool).await?;

        rows.into_iter().map(row_to_record).collect()
    }

    /// Ensures the singleton `settings` row exists with default values.
    pub(crate) async fn ensure_default_settings(&self) -> Result<(), EngineError> {
        let defaults = Settings::default();
        sqlx::query(
            "INSERT OR IGNORE INTO settings (
                id, default_output_dir, max_workers, max_concurrent_downloads, sync_on_complete
             ) VALUES (1, ?, ?, ?, ?)",
        )
        .bind(defaults.default_output_dir.to_string_lossy().into_owned())
        .bind(defaults.max_workers as i64)
        .bind(defaults.max_concurrent_downloads as i64)
        .bind(i64::from(defaults.sync_on_complete))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Reads the persisted [`Settings`] row.
    pub(crate) async fn get_settings(&self) -> Result<Settings, EngineError> {
        let row = sqlx::query(
            "SELECT default_output_dir, max_workers, max_concurrent_downloads, sync_on_complete
             FROM settings WHERE id = 1",
        )
        .fetch_one(&self.pool)
        .await?;

        let defaults = Settings::default();
        let dir_str: String = row.try_get("default_output_dir")?;
        let max_workers = usize::try_from(row.try_get::<i64, _>("max_workers")?)
            .unwrap_or(defaults.max_workers)
            .max(1);
        let max_concurrent_downloads =
            usize::try_from(row.try_get::<i64, _>("max_concurrent_downloads")?)
                .unwrap_or(defaults.max_concurrent_downloads)
                .max(1);
        let sync_on_complete = row.try_get::<i64, _>("sync_on_complete")? != 0;

        Ok(Settings {
            default_output_dir: PathBuf::from(dir_str),
            max_workers,
            max_concurrent_downloads,
            sync_on_complete,
        })
    }

    /// Updates the persisted [`Settings`] row.
    pub(crate) async fn update_settings(&self, settings: &Settings) -> Result<(), EngineError> {
        sqlx::query(
            "UPDATE settings
             SET default_output_dir = ?,
                 max_workers = ?,
                 max_concurrent_downloads = ?,
                 sync_on_complete = ?
             WHERE id = 1",
        )
        .bind(settings.default_output_dir.to_string_lossy().into_owned())
        .bind(settings.max_workers as i64)
        .bind(settings.max_concurrent_downloads as i64)
        .bind(i64::from(settings.sync_on_complete))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Deletes a `Download` record and its cascaded `Segment`s.
    pub(crate) async fn delete_download(&self, id: &DownloadId) -> Result<(), EngineError> {
        sqlx::query("DELETE FROM downloads WHERE id = ?")
            .bind(id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Clears `Segment`s and probe metadata so a `Download` can restart cleanly from byte `0`.
    pub(crate) async fn reset_for_fresh_restart(&self, id: &DownloadId) -> Result<(), EngineError> {
        let now = unix_now();
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM segments WHERE download_id = ?")
            .bind(id.as_str())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE downloads
             SET state = ?,
                 final_url = NULL,
                 part_path = NULL,
                 content_length = NULL,
                 supports_ranges = 0,
                 etag = NULL,
                 last_modified = NULL,
                 bytes_downloaded = 0,
                 error_message = NULL,
                 updated_at = ?
             WHERE id = ?",
        )
        .bind(DownloadState::Queued.as_str())
        .bind(now)
        .bind(id.as_str())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

fn row_to_record(row: SqliteRow) -> Result<DownloadRecord, EngineError> {
    let state_str: String = row.try_get("state")?;
    let state = DownloadState::from_str(&state_str).map_err(|_| {
        EngineError::InvalidState(format!("unknown persisted download state: {state_str}"))
    })?;

    Ok(DownloadRecord {
        id: DownloadId(row.try_get("id")?),
        url: row.try_get("url")?,
        final_url: row.try_get("final_url")?,
        output_dir: row
            .try_get::<Option<String>, _>("output_dir")?
            .map(PathBuf::from),
        output_path: row
            .try_get::<Option<String>, _>("output_path")?
            .map(PathBuf::from),
        part_path: row
            .try_get::<Option<String>, _>("part_path")?
            .map(PathBuf::from),
        state,
        content_length: row
            .try_get::<Option<i64>, _>("content_length")?
            .map(|v| u64::try_from(v).unwrap_or(0)),
        supports_ranges: row.try_get::<i64, _>("supports_ranges")? != 0,
        etag: row.try_get("etag")?,
        last_modified: row.try_get("last_modified")?,
        max_workers: usize::try_from(row.try_get::<i64, _>("max_workers")?).unwrap_or(1),
        sync_on_complete: row.try_get::<i64, _>("sync_on_complete")? != 0,
        headers: decode_headers(
            &row.try_get::<String, _>("request_headers")
                .unwrap_or_default(),
        ),
        bytes_downloaded: u64::try_from(row.try_get::<i64, _>("bytes_downloaded")?).unwrap_or(0),
        error_message: row.try_get("error_message")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn encode_headers(headers: &[(String, String)]) -> String {
    headers
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                utf8_percent_encode(k, NON_ALPHANUMERIC),
                utf8_percent_encode(v, NON_ALPHANUMERIC)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_headers(raw: &str) -> Vec<(String, String)> {
    raw.lines()
        .filter_map(|line| {
            let (k, v) = line.split_once('=')?;
            let key = percent_decode_str(k).decode_utf8().ok()?.into_owned();
            let val = percent_decode_str(v).decode_utf8().ok()?.into_owned();
            if key.is_empty() {
                None
            } else {
                Some((key, val))
            }
        })
        .collect()
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
