//! SQLite persistence store for `Download`s and `Segment`s.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::Row;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteRow,
    SqliteSynchronous,
};

use crate::probe::ProbeResult;
use crate::transfer::{SegmentCheckpoint, plan_segments};
use crate::{DownloadId, DownloadRequest, DownloadState, EngineError};

const DOWNLOAD_SELECT_COLUMNS: &str = "
    id, url, final_url, output_path, part_path, state,
    content_length, supports_ranges, etag, last_modified,
    max_workers, sync_on_complete, bytes_downloaded,
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
        Ok(Self { pool })
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
        Ok(Self { pool })
    }

    /// Inserts a newly queued `Download` record.
    pub(crate) async fn insert_queued(
        &self,
        id: &DownloadId,
        request: &DownloadRequest,
    ) -> Result<(), EngineError> {
        let now = unix_now();
        let output_path = request
            .output_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        sqlx::query(
            "INSERT INTO downloads (
                id, url, output_path, state, max_workers, sync_on_complete,
                bytes_downloaded, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(id.as_str())
        .bind(&request.url)
        .bind(output_path)
        .bind(DownloadState::Queued.as_str())
        .bind(request.max_workers as i64)
        .bind(i64::from(request.sync_on_complete))
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

    /// Marks a `Download` as `Failed` with an error message, optionally persisting `Segment` `Checkpoint`s.
    pub(crate) async fn mark_failed(
        &self,
        id: &DownloadId,
        bytes_downloaded: u64,
        error_message: &str,
    ) -> Result<(), EngineError> {
        self.save_failed_with_segments(id, bytes_downloaded, error_message, &[])
            .await
    }

    /// Marks a `Download` as `Failed` and replaces `Segment` `Checkpoint`s if `segments` is non-empty.
    pub(crate) async fn save_failed_with_segments(
        &self,
        id: &DownloadId,
        bytes_downloaded: u64,
        error_message: &str,
        segments: &[SegmentCheckpoint],
    ) -> Result<(), EngineError> {
        self.save_state_with_segments(
            id,
            DownloadState::Failed,
            bytes_downloaded,
            Some(error_message),
            segments,
        )
        .await
    }

    /// Transitions a `Download` to `Paused` and persists exact `Segment` `Checkpoint`s.
    pub(crate) async fn save_paused(
        &self,
        id: &DownloadId,
        bytes_downloaded: u64,
        segments: &[SegmentCheckpoint],
    ) -> Result<(), EngineError> {
        self.save_state_with_segments(id, DownloadState::Paused, bytes_downloaded, None, segments)
            .await
    }

    async fn save_state_with_segments(
        &self,
        id: &DownloadId,
        state: DownloadState,
        bytes_downloaded: u64,
        error_message: Option<&str>,
        segments: &[SegmentCheckpoint],
    ) -> Result<(), EngineError> {
        let mut tx = self.pool.begin().await?;
        let now = unix_now();

        sqlx::query(
            "UPDATE downloads SET
                state = ?,
                bytes_downloaded = ?,
                error_message = ?,
                updated_at = ?
             WHERE id = ?",
        )
        .bind(state.as_str())
        .bind(bytes_downloaded as i64)
        .bind(error_message)
        .bind(now)
        .bind(id.as_str())
        .execute(&mut *tx)
        .await?;

        if !segments.is_empty() {
            replace_segments_tx(&mut tx, id, segments).await?;
        }

        tx.commit().await?;
        Ok(())
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
        bytes_downloaded: u64::try_from(row.try_get::<i64, _>("bytes_downloaded")?).unwrap_or(0),
        error_message: row.try_get("error_message")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

async fn replace_segments_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &DownloadId,
    segments: &[SegmentCheckpoint],
) -> Result<(), EngineError> {
    sqlx::query("DELETE FROM segments WHERE download_id = ?")
        .bind(id.as_str())
        .execute(&mut **tx)
        .await?;

    for seg in segments {
        sqlx::query(
            "INSERT INTO segments (
                download_id, segment_index, start_offset, cursor_offset, end_offset, completed
             ) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(seg.segment_index as i64)
        .bind(seg.start as i64)
        .bind(seg.cursor as i64)
        .bind(seg.end.map(|v| v as i64))
        .bind(i64::from(seg.completed))
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}
