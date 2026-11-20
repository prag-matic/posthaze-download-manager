//! Dedicated background multi-thread Tokio runtime bridge for `DownloadEngine` (ADR-0010).

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use posthaze_engine::{DownloadEngine, EngineError, EngineEvent};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::{broadcast, oneshot};

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

    /// Returns a handle to the background Tokio runtime.
    pub fn runtime(&self) -> &Arc<Runtime> {
        &self.runtime
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
