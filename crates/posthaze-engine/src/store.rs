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

use crate::model::{
    DownloadCategory, GENERAL_CATEGORY_ID, PlannedSegment, SegmentCheckpoint,
    default_builtin_categories, normalize_extension_list, normalize_pattern_list,
};
use crate::probe::ProbeResult;
use crate::{
    CollisionPolicy, DeleteDefaultAction, DownloadId, DownloadRequest, DownloadState, EngineError,
    FailureKind, ProxyMode, Settings, TableDensity,
};

const DOWNLOAD_SELECT_COLUMNS: &str = "
    id, url, final_url, output_dir, output_path, part_path, state,
    content_length, supports_ranges, etag, last_modified,
    max_workers, sync_on_complete, request_headers, bytes_downloaded,
    error_message, created_at, updated_at, category_id
";

/// Runs embedded SQLite migrations, reconciling line-ending checksum mismatches on existing
/// databases before applying any pending migrations.
async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    let migrator = sqlx::migrate!("./migrations");
    match migrator.run(pool).await {
        Err(sqlx::migrate::MigrateError::VersionMismatch(_)) => {
            for migration in migrator.iter() {
                let _ = sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
                    .bind(migration.checksum.as_ref())
                    .bind(migration.version)
                    .execute(pool)
                    .await;
            }
            migrator.run(pool).await
        }
        other => other,
    }
}

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
    /// Optional resolved [`DownloadCategory`](crate::DownloadCategory) identifier.
    pub category_id: Option<String>,
}

impl DownloadRecord {
    /// Classifies a persisted `Failed` `Download`'s error message into a [`FailureKind`].
    pub fn failure_kind(&self) -> Option<FailureKind> {
        if self.state != DownloadState::Failed {
            return None;
        }
        let msg = self.error_message.as_deref()?;
        if msg.contains("remote resource changed") {
            Some(FailureKind::ResourceChanged)
        } else if msg.contains("invalid download state") {
            Some(FailureKind::NonResumable)
        } else {
            Some(FailureKind::Resumable)
        }
    }
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
            .foreign_keys(true)
            .pragma("cache_size", "-512")
            .statement_cache_capacity(32);

        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .min_connections(0)
            .idle_timeout(Some(std::time::Duration::from_secs(30)))
            .connect_with(options)
            .await?;

        run_migrations(&pool).await?;
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

        run_migrations(&pool).await?;
        let store = Self { pool };
        store.ensure_default_settings().await?;
        store.reconcile_crashed_downloads().await?;
        Ok(store)
    }

    /// Releases unused SQLite heap/page cache memory when the engine becomes idle.
    pub(crate) async fn shrink_memory(&self) {
        let _ = sqlx::query("PRAGMA shrink_memory;")
            .execute(&self.pool)
            .await;
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
                request_headers, bytes_downloaded, created_at, updated_at, category_id
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?)",
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
        .bind(request.category_id.as_deref())
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
        segments: &[PlannedSegment],
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

        for (idx, seg) in segments.iter().enumerate() {
            sqlx::query(
                "INSERT INTO segments (
                    download_id, segment_index, start_offset, cursor_offset, end_offset, completed
                 ) VALUES (?, ?, ?, ?, ?, 0)",
            )
            .bind(id.as_str())
            .bind(idx as i64)
            .bind(seg.start as i64)
            .bind(seg.start as i64)
            .bind(seg.end.map(|v| v as i64))
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Downgrades a `Download` from multi-`Segment` to single-stream after a server ignores `Range`.
    pub(crate) async fn downgrade_to_single_stream(
        &self,
        id: &DownloadId,
        content_length: Option<u64>,
    ) -> Result<(), EngineError> {
        let mut tx = self.pool.begin().await?;
        let now = unix_now();

        sqlx::query(
            "UPDATE downloads SET
                supports_ranges = 0,
                bytes_downloaded = 0,
                updated_at = ?
             WHERE id = ?",
        )
        .bind(now)
        .bind(id.as_str())
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM segments WHERE download_id = ?")
            .bind(id.as_str())
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "INSERT INTO segments (
                download_id, segment_index, start_offset, cursor_offset, end_offset, completed
             ) VALUES (?, 0, 0, 0, ?, 0)",
        )
        .bind(id.as_str())
        .bind(content_length.map(|v| v as i64))
        .execute(&mut *tx)
        .await?;

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

    /// Persists writer-acknowledged `Segment` cursors (upserting any dynamically split
    /// `Segment`s) and updates the `Download` state and byte count.
    ///
    /// Returns the total `bytes_downloaded` summed from persisted `Segment` rows.
    pub(crate) async fn finalize_writer_checkpoint(
        &self,
        id: &DownloadId,
        state: DownloadState,
        error_message: Option<&str>,
        writer_cursors: &HashMap<usize, u64>,
        split_segments: &[SegmentCheckpoint],
    ) -> Result<u64, EngineError> {
        let mut tx = self.pool.begin().await?;
        let now = unix_now();

        upsert_segments_in_tx(&mut tx, id, split_segments).await?;

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

    /// Persists a writer-acknowledged `Checkpoint` (upserting any dynamically split `Segment`s)
    /// without allowing `cursor_offset` to regress.
    pub(crate) async fn apply_writer_checkpoint(
        &self,
        id: &DownloadId,
        segment_cursors: &[(usize, u64)],
        split_segments: &[SegmentCheckpoint],
    ) -> Result<(), EngineError> {
        if segment_cursors.is_empty() && split_segments.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;
        upsert_segments_in_tx(&mut tx, id, split_segments).await?;

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

    /// Returns persisted `Segment` counts keyed by [`DownloadId`] in a single query.
    pub(crate) async fn list_segment_counts(&self) -> Result<HashMap<DownloadId, usize>, EngineError> {
        let rows = sqlx::query(
            "SELECT download_id, COUNT(*) AS cnt
             FROM segments
             GROUP BY download_id",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut counts = HashMap::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("download_id")?;
            let cnt: i64 = row.try_get("cnt")?;
            counts.insert(DownloadId::from(id), usize::try_from(cnt).unwrap_or(0));
        }
        Ok(counts)
    }

    /// Finds the highest-priority non-cancelled `DownloadRecord` matching `url` (`url` or
    /// `final_url`), prioritizing ongoing/paused transfers over historical completed entries.
    pub(crate) async fn find_download_by_url(
        &self,
        url: &str,
    ) -> Result<Option<DownloadRecord>, EngineError> {
        let trimmed = url.trim();
        let no_frag = trimmed.split('#').next().unwrap_or(trimmed);
        let sql = format!(
            "SELECT {DOWNLOAD_SELECT_COLUMNS}
             FROM downloads
             WHERE (
                url = ? OR url = ? OR url LIKE ? OR
                final_url = ? OR final_url = ? OR final_url LIKE ?
             ) AND state != ?
             ORDER BY
                CASE state
                    WHEN 'Active' THEN 0
                    WHEN 'Probing' THEN 0
                    WHEN 'Queued' THEN 1
                    WHEN 'Paused' THEN 2
                    WHEN 'Failed' THEN 3
                    ELSE 4
                END,
                updated_at DESC,
                rowid DESC
             LIMIT 1"
        );
        let frag_like = format!("{no_frag}#%");
        let row = sqlx::query(&sql)
            .bind(trimmed)
            .bind(no_frag)
            .bind(&frag_like)
            .bind(trimmed)
            .bind(no_frag)
            .bind(&frag_like)
            .bind(DownloadState::Cancelled.as_str())
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_record).transpose()
    }

    /// Finds the highest-priority non-cancelled `DownloadRecord` matching `filename` (ignoring
    /// ` (N)` disambiguation counters) with the same `content_length` (or same `output_dir` when
    /// `content_length` is unknown), catching rotating-token CDN URLs and auto-renamed duplicates.
    pub(crate) async fn find_duplicate_by_file(
        &self,
        filename: &str,
        content_length: Option<u64>,
        output_dir: Option<&Path>,
    ) -> Result<Option<DownloadRecord>, EngineError> {
        let target_key = crate::model::canonical_filename_key(filename);
        if target_key.is_empty() {
            return Ok(None);
        }
        let target_dir_norm = output_dir.map(normalize_folder_path);
        let sql = format!(
            "SELECT {DOWNLOAD_SELECT_COLUMNS}
             FROM downloads
             WHERE state != ?
             ORDER BY
                CASE state
                    WHEN 'Active' THEN 0
                    WHEN 'Probing' THEN 0
                    WHEN 'Queued' THEN 1
                    WHEN 'Paused' THEN 2
                    WHEN 'Failed' THEN 3
                    ELSE 4
                END,
                updated_at DESC,
                rowid DESC"
        );
        let rows = sqlx::query(&sql)
            .bind(DownloadState::Cancelled.as_str())
            .fetch_all(&self.pool)
            .await?;

        for row in rows {
            let rec = row_to_record(row)?;
            let Some(rec_name) = rec
                .output_path
                .as_deref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            if crate::model::canonical_filename_key(rec_name) != target_key {
                continue;
            }
            let size_match = match (content_length.filter(|&v| v > 0), rec.content_length) {
                (Some(a), Some(b)) => a == b,
                _ => match (&target_dir_norm, rec.output_dir.as_deref()) {
                    (Some(dir_a), Some(dir_b)) => {
                        let norm_b = normalize_folder_path(dir_b);
                        if cfg!(windows) {
                            dir_a.eq_ignore_ascii_case(&norm_b)
                        } else {
                            *dir_a == norm_b
                        }
                    }
                    _ => false,
                },
            };
            if size_match {
                return Ok(Some(rec));
            }
        }
        Ok(None)
    }

    /// Lists output paths currently claimed by ongoing, queued, paused, or resumable-failed
    /// `Download`s (excluding `exclude_id`) so new transfers never collide on `.part` files.
    pub(crate) async fn list_reserved_output_paths(
        &self,
        exclude_id: Option<&DownloadId>,
    ) -> Result<Vec<PathBuf>, EngineError> {
        let rows = if let Some(id) = exclude_id {
            sqlx::query(
                "SELECT output_path FROM downloads
                 WHERE id != ?
                   AND output_path IS NOT NULL
                   AND (state IN (?, ?, ?, ?)
                        OR (state = ? AND part_path IS NOT NULL))",
            )
            .bind(id.as_str())
            .bind(DownloadState::Queued.as_str())
            .bind(DownloadState::Probing.as_str())
            .bind(DownloadState::Active.as_str())
            .bind(DownloadState::Paused.as_str())
            .bind(DownloadState::Failed.as_str())
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT output_path FROM downloads
                 WHERE output_path IS NOT NULL
                   AND (state IN (?, ?, ?, ?)
                        OR (state = ? AND part_path IS NOT NULL))",
            )
            .bind(DownloadState::Queued.as_str())
            .bind(DownloadState::Probing.as_str())
            .bind(DownloadState::Active.as_str())
            .bind(DownloadState::Paused.as_str())
            .bind(DownloadState::Failed.as_str())
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .filter_map(|r| r.try_get::<String, _>("output_path").ok())
            .map(PathBuf::from)
            .collect())
    }

    /// Records `dir` in `folder_history` (normalized & deduplicated) and prunes older entries
    /// beyond the 10 most recent.
    pub(crate) async fn record_folder_history(&self, dir: &Path) -> Result<(), EngineError> {
        let dir_str = normalize_folder_path(dir);
        if dir_str.is_empty() {
            return Ok(());
        }
        let now = unix_now();
        let mut tx = self.pool.begin().await?;
        if cfg!(windows) {
            sqlx::query("DELETE FROM folder_history WHERE LOWER(path) = LOWER(?) AND path != ?")
                .bind(&dir_str)
                .bind(&dir_str)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query(
            "INSERT INTO folder_history (path, last_used_at) VALUES (?, ?)
             ON CONFLICT(path) DO UPDATE SET last_used_at = excluded.last_used_at",
        )
        .bind(&dir_str)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "DELETE FROM folder_history
             WHERE path NOT IN (
                 SELECT path FROM folder_history ORDER BY last_used_at DESC LIMIT 10
             )",
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Lists up to `limit` most recently used existing destination folders and prunes missing ones.
    pub(crate) async fn list_recent_folders(&self, limit: usize) -> Result<Vec<PathBuf>, EngineError> {
        let rows = sqlx::query(
            "SELECT path FROM folder_history ORDER BY last_used_at DESC, rowid DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        let raw_paths: Vec<String> = rows
            .into_iter()
            .filter_map(|r| r.try_get::<String, _>("path").ok())
            .collect();
        if raw_paths.is_empty() {
            return Ok(Vec::new());
        }

        let (valid, stale) = tokio::task::spawn_blocking(move || {
            let mut valid = Vec::with_capacity(limit.max(1));
            let mut stale = Vec::new();
            for p in raw_paths {
                let pb = PathBuf::from(&p);
                if pb.is_dir() {
                    if valid.len() < limit.max(1) {
                        valid.push(pb);
                    }
                } else {
                    stale.push(p);
                }
            }
            (valid, stale)
        })
        .await
        .map_err(|e| EngineError::Writer(format!("folder history check task failed: {e}")))?;

        for dead in stale {
            let _ = sqlx::query("DELETE FROM folder_history WHERE path = ?")
                .bind(dead)
                .execute(&self.pool)
                .await;
        }
        Ok(valid)
    }

    /// Removes all saved entries from `folder_history`.
    pub(crate) async fn clear_folder_history(&self) -> Result<(), EngineError> {
        sqlx::query("DELETE FROM folder_history")
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
            "SELECT default_output_dir, max_workers, max_concurrent_downloads, sync_on_complete,
                    proxy_mode, proxy_url, download_speed_limit_kib, auto_categorize_folders,
                    category_url_rule_precedence, set_file_time_from_server,
                    collision_policy, delete_action_default, auto_retry_count,
                    completion_cmd_enabled, completion_cmd_path, completion_cmd_args,
                    notify_on_added, notify_on_started, notify_on_completed, notify_on_failed, notify_only_inactive,
                    ui_click_row_opens_inspector, ui_show_add_paused, ui_zoom_percent, ui_theme,
                    ui_table_density, ui_show_speed_col, ui_show_eta_col, ui_show_segs_col,
                    ui_show_toolbar_labels, ui_show_scrollbar_minimap,
                    ui_show_titlebar_telemetry, ui_start_sidebar_collapsed, ui_accent_override,
                    ui_lock_file_extension, ui_close_to_tray, launch_on_startup,
                    auto_check_updates
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
        let proxy_mode = ProxyMode::from_str(&row.try_get::<String, _>("proxy_mode")?)
            .unwrap_or(defaults.proxy_mode);
        let proxy_url: String = row.try_get("proxy_url")?;
        let download_speed_limit_kib =
            u64::try_from(row.try_get::<i64, _>("download_speed_limit_kib")?).unwrap_or(0);
        let auto_categorize_folders = row.try_get::<i64, _>("auto_categorize_folders")? != 0;
        let category_url_rule_precedence =
            row.try_get::<i64, _>("category_url_rule_precedence")? != 0;
        let set_file_time_from_server = row.try_get::<i64, _>("set_file_time_from_server")? != 0;
        let collision_policy =
            CollisionPolicy::from_str(&row.try_get::<String, _>("collision_policy")?)
                .unwrap_or(defaults.collision_policy);
        let delete_action_default =
            DeleteDefaultAction::from_str(&row.try_get::<String, _>("delete_action_default")?)
                .unwrap_or(defaults.delete_action_default);
        let auto_retry_count =
            u32::try_from(row.try_get::<i64, _>("auto_retry_count")?).unwrap_or(0);
        let completion_cmd_enabled = row.try_get::<i64, _>("completion_cmd_enabled")? != 0;
        let completion_cmd_path: String = row.try_get("completion_cmd_path")?;
        let completion_cmd_args: String = row.try_get("completion_cmd_args")?;
        let notify_on_added = row.try_get::<i64, _>("notify_on_added")? != 0;
        let notify_on_started = row.try_get::<i64, _>("notify_on_started")? != 0;
        let notify_on_completed = row.try_get::<i64, _>("notify_on_completed")? != 0;
        let notify_on_failed = row.try_get::<i64, _>("notify_on_failed")? != 0;
        let notify_only_inactive = row.try_get::<i64, _>("notify_only_inactive")? != 0;
        let ui_click_row_opens_inspector =
            row.try_get::<i64, _>("ui_click_row_opens_inspector")? != 0;
        let ui_show_add_paused = row.try_get::<i64, _>("ui_show_add_paused")? != 0;
        let ui_zoom_percent =
            u16::try_from(row.try_get::<i64, _>("ui_zoom_percent")?).unwrap_or(100);
        let ui_theme_raw: String = row.try_get("ui_theme")?;
        let ui_theme = if ui_theme_raw.trim().is_empty() {
            defaults.ui_theme.clone()
        } else {
            ui_theme_raw
        };
        let ui_table_density =
            TableDensity::from_db(row.try_get::<String, _>("ui_table_density")?.as_str());
        let ui_show_speed_col = row.try_get::<i64, _>("ui_show_speed_col")? != 0;
        let ui_show_eta_col = row.try_get::<i64, _>("ui_show_eta_col")? != 0;
        let ui_show_segs_col = row.try_get::<i64, _>("ui_show_segs_col")? != 0;
        let ui_show_toolbar_labels = row.try_get::<i64, _>("ui_show_toolbar_labels")? != 0;
        let ui_show_scrollbar_minimap = row.try_get::<i64, _>("ui_show_scrollbar_minimap")? != 0;
        let ui_show_titlebar_telemetry = row.try_get::<i64, _>("ui_show_titlebar_telemetry")? != 0;
        let ui_start_sidebar_collapsed = row.try_get::<i64, _>("ui_start_sidebar_collapsed")? != 0;
        let ui_accent_override: Option<String> = row
            .try_get::<Option<String>, _>("ui_accent_override")?
            .and_then(|s| {
                let t = s.trim();
                (!t.is_empty()).then(|| t.to_string())
            });
        let ui_lock_file_extension = row.try_get::<i64, _>("ui_lock_file_extension")? != 0;
        let ui_close_to_tray = row.try_get::<i64, _>("ui_close_to_tray")? != 0;
        let launch_on_startup = row.try_get::<i64, _>("launch_on_startup")? != 0;
        let auto_check_updates = row.try_get::<i64, _>("auto_check_updates")? != 0;

        Ok(Settings {
            default_output_dir: PathBuf::from(dir_str),
            max_workers,
            max_concurrent_downloads,
            sync_on_complete,
            proxy_mode,
            proxy_url,
            download_speed_limit_kib,
            auto_categorize_folders,
            category_url_rule_precedence,
            set_file_time_from_server,
            collision_policy,
            delete_action_default,
            auto_retry_count,
            completion_cmd_enabled,
            completion_cmd_path,
            completion_cmd_args,
            notify_on_added,
            notify_on_started,
            notify_on_completed,
            notify_on_failed,
            notify_only_inactive,
            ui_click_row_opens_inspector,
            ui_show_add_paused,
            ui_zoom_percent,
            ui_theme,
            ui_table_density,
            ui_show_speed_col,
            ui_show_eta_col,
            ui_show_segs_col,
            ui_show_toolbar_labels,
            ui_show_scrollbar_minimap,
            ui_show_titlebar_telemetry,
            ui_start_sidebar_collapsed,
            ui_accent_override,
            ui_lock_file_extension,
            ui_close_to_tray,
            launch_on_startup,
            auto_check_updates,
        })
    }

    /// Updates the persisted [`Settings`] row.
    pub(crate) async fn update_settings(&self, settings: &Settings) -> Result<(), EngineError> {
        sqlx::query(
            "UPDATE settings
             SET default_output_dir = ?,
                 max_workers = ?,
                 max_concurrent_downloads = ?,
                 sync_on_complete = ?,
                 proxy_mode = ?,
                 proxy_url = ?,
                 download_speed_limit_kib = ?,
                 auto_categorize_folders = ?,
                 category_url_rule_precedence = ?,
                 set_file_time_from_server = ?,
                 collision_policy = ?,
                 delete_action_default = ?,
                 auto_retry_count = ?,
                 completion_cmd_enabled = ?,
                 completion_cmd_path = ?,
                 completion_cmd_args = ?,
                 notify_on_added = ?,
                 notify_on_started = ?,
                 notify_on_completed = ?,
                 notify_on_failed = ?,
                 notify_only_inactive = ?,
                 ui_click_row_opens_inspector = ?,
                 ui_show_add_paused = ?,
                 ui_zoom_percent = ?,
                 ui_theme = ?,
                 ui_table_density = ?,
                 ui_show_speed_col = ?,
                 ui_show_eta_col = ?,
                 ui_show_segs_col = ?,
                 ui_show_toolbar_labels = ?,
                 ui_show_scrollbar_minimap = ?,
                 ui_show_titlebar_telemetry = ?,
                 ui_start_sidebar_collapsed = ?,
                 ui_accent_override = ?,
                 ui_lock_file_extension = ?,
                 ui_close_to_tray = ?,
                 launch_on_startup = ?,
                 auto_check_updates = ?
             WHERE id = 1",
        )
        .bind(settings.default_output_dir.to_string_lossy().into_owned())
        .bind(settings.max_workers as i64)
        .bind(settings.max_concurrent_downloads as i64)
        .bind(i64::from(settings.sync_on_complete))
        .bind(settings.proxy_mode.as_str())
        .bind(&settings.proxy_url)
        .bind(settings.download_speed_limit_kib as i64)
        .bind(i64::from(settings.auto_categorize_folders))
        .bind(i64::from(settings.category_url_rule_precedence))
        .bind(i64::from(settings.set_file_time_from_server))
        .bind(settings.collision_policy.as_str())
        .bind(settings.delete_action_default.as_str())
        .bind(i64::from(settings.auto_retry_count))
        .bind(i64::from(settings.completion_cmd_enabled))
        .bind(&settings.completion_cmd_path)
        .bind(&settings.completion_cmd_args)
        .bind(i64::from(settings.notify_on_added))
        .bind(i64::from(settings.notify_on_started))
        .bind(i64::from(settings.notify_on_completed))
        .bind(i64::from(settings.notify_on_failed))
        .bind(i64::from(settings.notify_only_inactive))
        .bind(i64::from(settings.ui_click_row_opens_inspector))
        .bind(i64::from(settings.ui_show_add_paused))
        .bind(i64::from(settings.ui_zoom_percent))
        .bind(&settings.ui_theme)
        .bind(settings.ui_table_density.as_str())
        .bind(i64::from(settings.ui_show_speed_col))
        .bind(i64::from(settings.ui_show_eta_col))
        .bind(i64::from(settings.ui_show_segs_col))
        .bind(i64::from(settings.ui_show_toolbar_labels))
        .bind(i64::from(settings.ui_show_scrollbar_minimap))
        .bind(i64::from(settings.ui_show_titlebar_telemetry))
        .bind(i64::from(settings.ui_start_sidebar_collapsed))
        .bind(&settings.ui_accent_override)
        .bind(i64::from(settings.ui_lock_file_extension))
        .bind(i64::from(settings.ui_close_to_tray))
        .bind(i64::from(settings.launch_on_startup))
        .bind(i64::from(settings.auto_check_updates))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Updates `output_path` and `output_dir` after renaming or moving a completed file.
    pub(crate) async fn update_completed_file_paths(
        &self,
        id: &DownloadId,
        output_path: &Path,
        output_dir: Option<&Path>,
    ) -> Result<(), EngineError> {
        let now = unix_now();
        let path_str = output_path.to_string_lossy().into_owned();
        let dir_str = output_dir.map(|p| p.to_string_lossy().into_owned());
        sqlx::query(
            "UPDATE downloads
             SET output_path = ?,
                 output_dir = COALESCE(?, output_dir),
                 updated_at = ?
             WHERE id = ?",
        )
        .bind(path_str)
        .bind(dir_str)
        .bind(now)
        .bind(id.as_str())
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

    /// Lists all persisted [`DownloadCategory`] rules ordered by `sort_order`, `name`.
    pub(crate) async fn list_categories(&self) -> Result<Vec<DownloadCategory>, EngineError> {
        let rows = sqlx::query(
            "SELECT id, name, extensions, url_patterns, output_dir, remember_last_folder,
                    is_builtin, sort_order
             FROM download_categories
             ORDER BY sort_order ASC, name ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut categories = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("id")?;
            let name: String = row.try_get("name")?;
            let ext_raw: String = row.try_get("extensions")?;
            let pat_raw: String = row.try_get("url_patterns")?;
            let dir_raw: Option<String> = row.try_get("output_dir")?;
            let remember_last_folder = row.try_get::<i64, _>("remember_last_folder")? != 0;
            let is_builtin = row.try_get::<i64, _>("is_builtin")? != 0;
            let sort_order = row.try_get::<i64, _>("sort_order")? as i32;
            categories.push(DownloadCategory {
                id,
                name,
                extensions: normalize_extension_list(&ext_raw),
                url_patterns: normalize_pattern_list(&pat_raw),
                output_dir: dir_raw
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from),
                remember_last_folder,
                is_builtin,
                sort_order,
            });
        }
        if categories.is_empty() {
            return Ok(default_builtin_categories());
        }
        Ok(categories)
    }

    /// Atomically replaces all [`DownloadCategory`] rows after normalizing fields and ensuring
    /// the `General` fallback exists.
    pub(crate) async fn replace_categories(
        &self,
        categories: &[DownloadCategory],
    ) -> Result<Vec<DownloadCategory>, EngineError> {
        let mut normalized = Vec::with_capacity(categories.len().max(1));
        let mut seen_ids = std::collections::HashSet::new();
        let mut has_general = false;

        for (idx, cat) in categories.iter().enumerate() {
            let id = cat.id.trim().to_string();
            let name = cat.name.trim().to_string();
            if id.is_empty() || name.is_empty() || !seen_ids.insert(id.clone()) {
                return Err(EngineError::InvalidState(
                    "category id and name must be non-empty and unique".to_string(),
                ));
            }
            let is_general = id == GENERAL_CATEGORY_ID;
            if is_general {
                has_general = true;
            }
            normalized.push(DownloadCategory {
                id,
                name: if is_general {
                    "General".to_string()
                } else {
                    name
                },
                extensions: if is_general {
                    Vec::new()
                } else {
                    normalize_extension_list(&cat.extensions.join(","))
                },
                url_patterns: if is_general {
                    Vec::new()
                } else {
                    normalize_pattern_list(&cat.url_patterns.join(","))
                },
                output_dir: cat
                    .output_dir
                    .as_ref()
                    .filter(|p| !p.as_os_str().is_empty())
                    .cloned(),
                remember_last_folder: cat.remember_last_folder,
                is_builtin: cat.is_builtin || is_general,
                sort_order: if is_general {
                    9999
                } else if cat.sort_order > 0 && cat.sort_order < 9999 {
                    cat.sort_order
                } else {
                    ((idx as i32) + 1) * 10
                },
            });
        }

        if !has_general
            && let Some(gen_cat) = default_builtin_categories()
                .into_iter()
                .find(|c| c.id == GENERAL_CATEGORY_ID)
        {
            normalized.push(gen_cat);
        }
        normalized.sort_by(|a, b| a.sort_order.cmp(&b.sort_order).then_with(|| a.name.cmp(&b.name)));

        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM download_categories")
            .execute(&mut *tx)
            .await?;
        for cat in &normalized {
            let ext_csv = cat.extensions.join(",");
            let pat_csv = cat.url_patterns.join(",");
            let dir_str = cat
                .output_dir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned());
            sqlx::query(
                "INSERT INTO download_categories (
                    id, name, extensions, url_patterns, output_dir, remember_last_folder,
                    is_builtin, sort_order
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&cat.id)
            .bind(&cat.name)
            .bind(ext_csv)
            .bind(pat_csv)
            .bind(dir_str)
            .bind(i64::from(cat.remember_last_folder))
            .bind(i64::from(cat.is_builtin))
            .bind(i64::from(cat.sort_order))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(normalized)
    }

    /// Updates `output_dir` for `category_id` when `remember_last_folder` is enabled.
    pub(crate) async fn update_category_output_dir(
        &self,
        category_id: &str,
        dir: &Path,
    ) -> Result<bool, EngineError> {
        let norm = normalize_folder_path(dir);
        if norm.is_empty() {
            return Ok(false);
        }
        let res = sqlx::query(
            "UPDATE download_categories
             SET output_dir = ?
             WHERE id = ? AND remember_last_folder != 0",
        )
        .bind(norm)
        .bind(category_id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Restores the seeded built-in [`DownloadCategory`] presets.
    pub(crate) async fn reset_default_categories(
        &self,
    ) -> Result<Vec<DownloadCategory>, EngineError> {
        let defaults = default_builtin_categories();
        self.replace_categories(&defaults).await
    }
}

async fn upsert_segments_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &DownloadId,
    segments: &[SegmentCheckpoint],
) -> Result<(), EngineError> {
    for seg in segments {
        sqlx::query(
            "INSERT INTO segments (
                download_id, segment_index, start_offset, cursor_offset, end_offset, completed
             ) VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(download_id, segment_index) DO UPDATE SET
                end_offset = excluded.end_offset,
                cursor_offset = MAX(segments.cursor_offset, excluded.cursor_offset),
                completed = CASE
                    WHEN excluded.end_offset IS NOT NULL
                         AND MAX(segments.cursor_offset, excluded.cursor_offset) >= excluded.end_offset
                    THEN 1
                    ELSE MAX(segments.completed, excluded.completed)
                END",
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

fn normalize_folder_path(dir: &Path) -> String {
    let raw = dir.to_string_lossy().trim().to_string();
    if raw.is_empty() {
        return String::new();
    }
    let bytes = raw.as_bytes();
    if bytes.len() == 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
        return format!("{}:\\", raw.chars().next().unwrap_or('C'));
    }
    if raw == "/" || raw == "\\" {
        return raw;
    }
    raw.trim_end_matches(['/', '\\']).to_string()
}

fn row_to_record(row: SqliteRow) -> Result<DownloadRecord, EngineError> {
    let state_str: String = row.try_get("state")?;
    let state = DownloadState::from_str(&state_str).map_err(|_| {
        EngineError::InvalidState(format!("unknown persisted download state: {state_str}"))
    })?;

    Ok(DownloadRecord {
        id: DownloadId::from(row.try_get::<String, _>("id")?),
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
        category_id: row.try_get("category_id")?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_segment_counts_aggregates_per_download_in_one_query() {
        let dir = std::env::temp_dir().join(format!("posthaze_store_cnt_{}", DownloadId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = DownloadStore::open(&dir.join("state.db")).await.unwrap();

        let id1 = DownloadId::new();
        let id2 = DownloadId::new();
        let req = DownloadRequest::new("https://example.com/a.bin", &dir);
        store.insert_queued(&id1, &req).await.unwrap();
        store.insert_queued(&id2, &req).await.unwrap();

        let probe = ProbeResult {
            final_url: "https://example.com/a.bin".to_string(),
            suggested_filename: "a.bin".to_string(),
            content_length: Some(3000),
            supports_ranges: true,
            etag: None,
            last_modified: None,
            server_digest: None,
        };
        store
            .record_probed_and_activate(
                &id1,
                &probe,
                &dir.join("a.bin"),
                &dir.join("a.bin.part"),
                &[
                    PlannedSegment { start: 0, end: Some(1000) },
                    PlannedSegment { start: 1000, end: Some(2000) },
                    PlannedSegment { start: 2000, end: Some(3000) },
                ],
            )
            .await
            .unwrap();
        store
            .record_probed_and_activate(
                &id2,
                &probe,
                &dir.join("b.bin"),
                &dir.join("b.bin.part"),
                &[PlannedSegment { start: 0, end: Some(3000) }],
            )
            .await
            .unwrap();

        let counts = store.list_segment_counts().await.unwrap();
        assert_eq!(counts.get(&id1), Some(&3));
        assert_eq!(counts.get(&id2), Some(&1));

        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn composite_segment_index_and_shrink_memory_work_cleanly() {
        let dir = std::env::temp_dir().join(format!("posthaze_store_idx_{}", DownloadId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = DownloadStore::open(&dir.join("state.db")).await.unwrap();

        let select_rows = sqlx::query(
            "EXPLAIN QUERY PLAN SELECT segment_index, start_offset, end_offset, cursor_offset, completed \
             FROM segments WHERE download_id = 'test-id' ORDER BY segment_index ASC",
        )
        .fetch_all(&store.pool)
        .await
        .unwrap();
        let plan_joined = select_rows
            .iter()
            .map(|r| r.try_get::<String, _>("detail").unwrap())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            plan_joined.contains("idx_segments_download_seg"),
            "expected composite index in query plan, got: {plan_joined}"
        );
        assert!(
            !plan_joined.contains("USE TEMP B-TREE FOR ORDER BY"),
            "expected no temp B-tree sort, got: {plan_joined}"
        );

        let update_rows = sqlx::query(
            "EXPLAIN QUERY PLAN UPDATE segments SET cursor_offset = 100 \
             WHERE download_id = 'test-id' AND segment_index = 0",
        )
        .fetch_all(&store.pool)
        .await
        .unwrap();
        let update_joined = update_rows
            .iter()
            .map(|r| r.try_get::<String, _>("detail").unwrap())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            update_joined.contains("idx_segments_download_seg"),
            "expected composite index in update plan, got: {update_joined}"
        );

        store.shrink_memory().await;

        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
