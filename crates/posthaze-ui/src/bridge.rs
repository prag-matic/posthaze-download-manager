//! Dedicated background multi-thread Tokio runtime bridge for `DownloadEngine` (ADR-0010).

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use posthaze_engine::{DownloadEngine, EngineError, EngineEvent};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::{broadcast, oneshot};

/// Returns the default SQLite state database path (`%LOCALAPPDATA%\posthaze\state.db`).
pub fn default_db_path() -> PathBuf {
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local_app_data)
            .join("posthaze")
            .join("state.db");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".posthaze").join("state.db");
    }
    PathBuf::from("posthaze-state.db")
}

/// Bridges a background multi-thread Tokio [`DownloadEngine`] into GPUI's async executor.
#[derive(Clone)]
pub struct EngineBridge {
    runtime: Arc<Runtime>,
    engine: DownloadEngine,
}

impl EngineBridge {
    /// Starts the dedicated Tokio runtime and opens [`DownloadEngine`] at `db_path`.
    pub fn open(db_path: PathBuf) -> Result<Self, EngineError> {
        let runtime = Builder::new_multi_thread()
            .enable_all()
            .thread_name("posthaze-engine-rt")
            .build()
            .map_err(EngineError::Io)?;
        let engine = runtime.block_on(DownloadEngine::open(db_path))?;
        Ok(Self {
            runtime: Arc::new(runtime),
            engine,
        })
    }

    /// Subscribes to engine-wide [`EngineEvent`] notifications.
    pub fn subscribe_events(&self) -> broadcast::Receiver<EngineEvent> {
        self.engine.subscribe_events()
    }

    /// Dispatches an async engine operation onto the background Tokio runtime.
    pub fn dispatch<F, Fut, T>(&self, make_fut: F) -> oneshot::Receiver<T>
    where
        F: FnOnce(DownloadEngine) -> Fut,
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let engine = self.engine.clone();
        let (tx, rx) = oneshot::channel();
        let fut = make_fut(engine);
        self.runtime.spawn(async move {
            let out = fut.await;
            let _ = tx.send(out);
        });
        rx
    }
}
