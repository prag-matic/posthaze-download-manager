//! Throughput sampling and debug JSON report serialization for `posthaze-cli`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use posthaze_engine::{DownloadOutcome, ProgressSnapshot};
use serde::Serialize;

#[derive(Default)]
pub(crate) struct ProgressStats {
    pub peak_bps: f64,
    pub last_snapshot: Option<ProgressSnapshot>,
    pub samples: Vec<ThroughputSample>,
    last_bytes: u64,
}

impl ProgressStats {
    pub(crate) fn record_tick(
        &mut self,
        snap: ProgressSnapshot,
        elapsed: Duration,
        dt_secs: f64,
        record_samples: bool,
    ) -> f64 {
        let inst_bps = if dt_secs > 0.0 {
            snap.bytes_downloaded.saturating_sub(self.last_bytes) as f64 / dt_secs
        } else {
            0.0
        };
        if inst_bps > self.peak_bps {
            self.peak_bps = inst_bps;
        }
        self.last_bytes = snap.bytes_downloaded;
        self.push_sample(snap, elapsed, inst_bps, record_samples);
        inst_bps
    }

    pub(crate) fn record_terminal(
        &mut self,
        snap: ProgressSnapshot,
        elapsed: Duration,
        record_samples: bool,
    ) {
        self.last_bytes = snap.bytes_downloaded;
        self.push_sample(snap, elapsed, 0.0, record_samples);
    }

    fn push_sample(
        &mut self,
        snap: ProgressSnapshot,
        elapsed: Duration,
        inst_bps: f64,
        record_samples: bool,
    ) {
        if record_samples {
            self.samples.push(ThroughputSample {
                elapsed_ms: elapsed.as_millis() as u64,
                bytes_downloaded: snap.bytes_downloaded,
                inst_bytes_per_sec: inst_bps,
                active_workers: snap.active_workers,
            });
        }
        self.last_snapshot = Some(snap);
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ThroughputSample {
    pub elapsed_ms: u64,
    pub bytes_downloaded: u64,
    pub inst_bytes_per_sec: f64,
    pub active_workers: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct DebugStatsReport {
    pub status: &'static str,
    pub error: Option<String>,
    pub final_path: Option<PathBuf>,
    pub final_url: String,
    pub bytes_downloaded: u64,
    pub content_length: Option<u64>,
    pub supports_ranges: bool,
    pub workers_used: usize,
    pub splits_performed: usize,
    pub retries_performed: usize,
    pub probe_ms: Option<f64>,
    pub transfer_ms: Option<f64>,
    pub total_ms: f64,
    pub avg_bytes_per_sec: f64,
    pub avg_mib_per_sec: f64,
    pub avg_mbps: f64,
    pub peak_bytes_per_sec: f64,
    pub peak_mib_per_sec: f64,
    pub peak_mbps: f64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub samples: Vec<ThroughputSample>,
}

pub(crate) fn default_stats_path(final_path: &Path) -> PathBuf {
    let mut os: OsString = final_path.as_os_str().to_os_string();
    os.push(".stats.json");
    PathBuf::from(os)
}

pub(crate) fn write_success_stats(
    path: &Path,
    outcome: &DownloadOutcome,
    stats: &ProgressStats,
) -> Result<(), std::io::Error> {
    let avg_bps = outcome.average_bytes_per_sec();
    let peak_bps = stats.peak_bps.max(avg_bps);
    let report = DebugStatsReport {
        status: "completed",
        error: None,
        final_path: Some(outcome.final_path.clone()),
        final_url: outcome.final_url.clone(),
        bytes_downloaded: outcome.bytes_downloaded,
        content_length: outcome.content_length,
        supports_ranges: outcome.supports_ranges,
        workers_used: outcome.workers_used,
        splits_performed: outcome.splits_performed,
        retries_performed: outcome.retries_performed,
        probe_ms: Some(outcome.probe_duration.as_secs_f64() * 1000.0),
        transfer_ms: Some(outcome.transfer_duration.as_secs_f64() * 1000.0),
        total_ms: outcome.total_duration.as_secs_f64() * 1000.0,
        avg_bytes_per_sec: avg_bps,
        avg_mib_per_sec: avg_bps / (1024.0 * 1024.0),
        avg_mbps: (avg_bps * 8.0) / 1_000_000.0,
        peak_bytes_per_sec: peak_bps,
        peak_mib_per_sec: peak_bps / (1024.0 * 1024.0),
        peak_mbps: (peak_bps * 8.0) / 1_000_000.0,
        etag: outcome.etag.clone(),
        last_modified: outcome.last_modified.clone(),
        samples: stats.samples.clone(),
    };

    let json = serde_json::to_string_pretty(&report).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

pub(crate) fn write_failure_stats(
    path: &Path,
    url: &str,
    error: &str,
    total_elapsed: Duration,
    stats: &ProgressStats,
) -> Result<(), std::io::Error> {
    let bytes = stats
        .last_snapshot
        .as_ref()
        .map_or(0, |snap| snap.bytes_downloaded);
    let content_length = stats
        .last_snapshot
        .as_ref()
        .and_then(|snap| snap.total_bytes);
    let secs = total_elapsed.as_secs_f64();
    let avg_bps = if secs > 0.0 { bytes as f64 / secs } else { 0.0 };
    let peak_bps = stats.peak_bps.max(avg_bps);

    let report = DebugStatsReport {
        status: "failed",
        error: Some(error.to_string()),
        final_path: None,
        final_url: url.to_string(),
        bytes_downloaded: bytes,
        content_length,
        supports_ranges: false,
        workers_used: 0,
        splits_performed: 0,
        retries_performed: 0,
        probe_ms: None,
        transfer_ms: None,
        total_ms: secs * 1000.0,
        avg_bytes_per_sec: avg_bps,
        avg_mib_per_sec: avg_bps / (1024.0 * 1024.0),
        avg_mbps: (avg_bps * 8.0) / 1_000_000.0,
        peak_bytes_per_sec: peak_bps,
        peak_mib_per_sec: peak_bps / (1024.0 * 1024.0),
        peak_mbps: (peak_bps * 8.0) / 1_000_000.0,
        etag: None,
        last_modified: None,
        samples: stats.samples.clone(),
    };

    let json = serde_json::to_string_pretty(&report).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaze_engine::DownloadId;

    #[test]
    fn default_stats_path_appends_stats_json() {
        assert_eq!(
            default_stats_path(Path::new("downloads/archive.zip")),
            PathBuf::from("downloads/archive.zip.stats.json")
        );
    }

    #[test]
    fn writes_success_and_failure_json_reports() {
        let dir = std::env::temp_dir().join(format!("posthaze_cli_stats_{}", DownloadId::new()));
        std::fs::create_dir_all(&dir).unwrap();

        let stats = ProgressStats {
            peak_bps: 2_000_000.0,
            last_snapshot: Some(ProgressSnapshot {
                bytes_downloaded: 512,
                total_bytes: Some(1024),
                ..Default::default()
            }),
            samples: vec![ThroughputSample {
                elapsed_ms: 250,
                bytes_downloaded: 512,
                inst_bytes_per_sec: 2_000_000.0,
                active_workers: 4,
            }],
            last_bytes: 512,
        };

        let ok_path = dir.join("ok.stats.json");
        let outcome = DownloadOutcome {
            final_path: dir.join("file.bin"),
            bytes_downloaded: 1024,
            content_length: Some(1024),
            supports_ranges: true,
            workers_used: 4,
            splits_performed: 1,
            retries_performed: 0,
            probe_duration: Duration::from_millis(20),
            transfer_duration: Duration::from_millis(500),
            total_duration: Duration::from_millis(520),
            etag: Some("\"e1\"".to_string()),
            last_modified: None,
            final_url: "https://example.com/file.bin".to_string(),
        };
        write_success_stats(&ok_path, &outcome, &stats).unwrap();
        let ok_json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&ok_path).unwrap()).unwrap();
        assert_eq!(ok_json["status"], "completed");
        assert_eq!(ok_json["bytes_downloaded"], 1024);
        assert_eq!(ok_json["samples"].as_array().unwrap().len(), 1);

        let err_path = dir.join("err.stats.json");
        write_failure_stats(
            &err_path,
            "https://example.com/file.bin",
            "HTTP 503",
            Duration::from_secs(1),
            &stats,
        )
        .unwrap();
        let err_json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&err_path).unwrap()).unwrap();
        assert_eq!(err_json["status"], "failed");
        assert_eq!(err_json["error"], "HTTP 503");
        assert_eq!(err_json["bytes_downloaded"], 512);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn progress_stats_records_throughput_ticks_and_peak() {
        let mut stats = ProgressStats::default();
        let bps = stats.record_tick(
            ProgressSnapshot {
                bytes_downloaded: 1000,
                active_workers: 2,
                ..Default::default()
            },
            Duration::from_millis(200),
            0.5,
            true,
        );
        assert!((bps - 2000.0).abs() < 1e-6);
        assert!((stats.peak_bps - 2000.0).abs() < 1e-6);

        stats.record_terminal(
            ProgressSnapshot {
                bytes_downloaded: 1500,
                active_workers: 0,
                ..Default::default()
            },
            Duration::from_millis(400),
            true,
        );
        assert_eq!(stats.samples.len(), 2);
        assert_eq!(stats.last_snapshot.as_ref().unwrap().bytes_downloaded, 1500);
    }
}
