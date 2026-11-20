//! Rolling file diagnostics subscriber and panic hook for `posthaze-ui`.

use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

/// Maximum size of a single diagnostic log file (`2 MiB`) before rotation.
pub(crate) const MAX_LOG_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Number of rotated historical log files retained alongside `posthaze.log`.
pub(crate) const MAX_ROTATED_LOGS: usize = 5;

/// Size-bounded rolling log writer targeting `<logs_dir>/posthaze.log`.
pub(crate) struct RollingLogWriter {
    logs_dir: PathBuf,
    current_path: PathBuf,
    file: Option<File>,
    current_bytes: u64,
    max_bytes: u64,
    max_files: usize,
}

impl RollingLogWriter {
    /// Opens or creates `posthaze.log` inside `logs_dir`.
    pub(crate) fn open_in_dir(
        logs_dir: &Path,
        max_bytes: u64,
        max_files: usize,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(logs_dir)?;
        let current_path = logs_dir.join("posthaze.log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&current_path)?;
        let current_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            logs_dir: logs_dir.to_path_buf(),
            current_path,
            file: Some(file),
            current_bytes,
            max_bytes: max_bytes.max(1),
            max_files,
        })
    }

    /// Appends a single log line, rotating `posthaze.log` when `max_bytes` is exceeded.
    pub(crate) fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        let bytes_needed = line.len() as u64 + 1;
        if self.current_bytes > 0 && self.current_bytes + bytes_needed > self.max_bytes {
            self.rotate()?;
        }
        if self.file.is_none() {
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.current_path)?,
            );
        }
        if let Some(file) = self.file.as_mut() {
            writeln!(file, "{line}")?;
            file.flush()?;
        }
        self.current_bytes += bytes_needed;
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        if let Some(mut file) = self.file.take() {
            let _ = file.flush();
            drop(file);
        }
        if self.max_files > 0 {
            let oldest = self.logs_dir.join(format!("posthaze.{}.log", self.max_files));
            let _ = std::fs::remove_file(oldest);

            for idx in (1..self.max_files).rev() {
                let src = self.logs_dir.join(format!("posthaze.{idx}.log"));
                let dst = self.logs_dir.join(format!("posthaze.{}.log", idx + 1));
                if src.exists() {
                    let _ = std::fs::rename(src, dst);
                }
            }

            let first_rotated = self.logs_dir.join("posthaze.1.log");
            if self.current_path.exists() {
                let _ = std::fs::rename(&self.current_path, &first_rotated);
            }
        }
        self.file = Some(
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&self.current_path)?,
        );
        self.current_bytes = 0;
        Ok(())
    }
}

struct EventFieldVisitor {
    message: String,
    fields: String,
}

impl EventFieldVisitor {
    fn new() -> Self {
        Self {
            message: String::new(),
            fields: String::new(),
        }
    }

    fn record_formatted(&mut self, field: &Field, formatted: std::fmt::Arguments<'_>) {
        if field.name() == "message" {
            let _ = write!(self.message, "{formatted}");
        } else {
            let _ = write!(self.fields, " {}={formatted}", field.name());
        }
    }
}

impl Visit for EventFieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.record_formatted(field, format_args!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_formatted(field, format_args!("{value}"));
    }
}

struct DiagnosticsSubscriber {
    writer: Arc<Mutex<RollingLogWriter>>,
}

impl Subscriber for DiagnosticsSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() <= Level::INFO
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut visitor = EventFieldVisitor::new();
        event.record(&mut visitor);
        let ts = unix_seconds();
        let meta = event.metadata();
        let line = format!(
            "[{ts}] {:5} {}: {}{}",
            meta.level(),
            meta.target(),
            visitor.message,
            visitor.fields
        );
        #[cfg(debug_assertions)]
        eprintln!("{line}");
        if let Ok(mut guard) = self.writer.lock() {
            let _ = guard.write_line(&line);
        }
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Initializes rolling file diagnostics and installs a panic hook logging to `logs_dir`.
pub(crate) fn init_diagnostics(logs_dir: PathBuf) {
    let shared = RollingLogWriter::open_in_dir(&logs_dir, MAX_LOG_FILE_BYTES, MAX_ROTATED_LOGS)
        .ok()
        .map(|writer| Arc::new(Mutex::new(writer)));

    if let Some(writer) = &shared {
        let subscriber = DiagnosticsSubscriber {
            writer: Arc::clone(writer),
        };
        let _ = tracing::subscriber::set_global_default(subscriber);
    }

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "non-string panic payload".to_string()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let line = format!("[{}] PANIC at {location}: {payload}", unix_seconds());

        let wrote_via_shared = shared
            .as_ref()
            .and_then(|lock| lock.try_lock().ok())
            .is_some_and(|mut guard| guard.write_line(&line).is_ok());

        if !wrote_via_shared {
            let _ = std::fs::create_dir_all(&logs_dir);
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(logs_dir.join("posthaze.log"))
            {
                let _ = writeln!(file, "{line}");
                let _ = file.flush();
            }
        }
        prev_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::RollingLogWriter;

    #[test]
    fn rolling_log_writer_rotates_and_caps_retained_files() {
        let temp_dir =
            std::env::temp_dir().join(format!("posthaze-logs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let mut writer = RollingLogWriter::open_in_dir(&temp_dir, 64, 3)
            .expect("should create logs directory and open posthaze.log");

        for i in 0..8 {
            writer
                .write_line(&format!("entry-{i:02}: diagnostic payload padding 1234567890"))
                .expect("write_line succeeds");
        }

        assert!(temp_dir.join("posthaze.log").is_file());
        assert!(temp_dir.join("posthaze.1.log").is_file());
        assert!(temp_dir.join("posthaze.2.log").is_file());
        assert!(temp_dir.join("posthaze.3.log").is_file());
        assert!(!temp_dir.join("posthaze.4.log").exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
