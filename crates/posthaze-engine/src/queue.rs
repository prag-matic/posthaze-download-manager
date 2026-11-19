//! Engine-owned FIFO `Queue` concurrency coordinator.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::sync::{Notify, watch};

use crate::transfer::ControlSignal;
use crate::{DownloadId, EngineError};

#[derive(Debug)]
struct QueueState {
    max_concurrent: usize,
    active_count: usize,
    waiting: VecDeque<DownloadId>,
}

/// Coordinates FIFO `Queue` slot admission up to `max_concurrent_downloads`.
#[derive(Debug)]
pub(crate) struct QueueCoordinator {
    state: Mutex<QueueState>,
    notify: Notify,
}

/// RAII guard releasing an active `Queue` slot on drop.
#[derive(Debug)]
pub(crate) struct SlotGuard {
    queue: Arc<QueueCoordinator>,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        {
            let mut st = self.queue.state.lock().unwrap();
            st.active_count = st.active_count.saturating_sub(1);
        }
        self.queue.notify.notify_waiters();
    }
}

impl QueueCoordinator {
    /// Creates a new [`QueueCoordinator`] with the given concurrency ceiling.
    pub(crate) fn new(max_concurrent: usize) -> Self {
        Self {
            state: Mutex::new(QueueState {
                max_concurrent: max_concurrent.max(1),
                active_count: 0,
                waiting: VecDeque::new(),
            }),
            notify: Notify::new(),
        }
    }

    /// Appends `id` to the back of the FIFO waiting queue.
    pub(crate) fn enqueue(&self, id: DownloadId) {
        {
            let mut st = self.state.lock().unwrap();
            if !st.waiting.iter().any(|existing| existing == &id) {
                st.waiting.push_back(id);
            }
        }
        self.notify.notify_waiters();
    }

    /// Updates `max_concurrent` and wakes waiting `Queued` `Download`s immediately.
    pub(crate) fn set_max_concurrent(&self, max_concurrent: usize) {
        {
            let mut st = self.state.lock().unwrap();
            st.max_concurrent = max_concurrent.max(1);
        }
        self.notify.notify_waiters();
    }

    fn remove_waiting(&self, id: &DownloadId) {
        let removed = {
            let mut st = self.state.lock().unwrap();
            if let Some(pos) = st.waiting.iter().position(|existing| existing == id) {
                st.waiting.remove(pos);
                true
            } else {
                false
            }
        };
        if removed {
            self.notify.notify_waiters();
        }
    }

    /// Waits in FIFO order until a concurrency slot is available or `control_rx` interrupts.
    pub(crate) async fn wait_for_slot(
        self: &Arc<Self>,
        id: &DownloadId,
        control_rx: &mut watch::Receiver<ControlSignal>,
    ) -> Result<SlotGuard, EngineError> {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let sig = *control_rx.borrow();
            if sig != ControlSignal::Run {
                self.remove_waiting(id);
                return Err(match sig {
                    ControlSignal::Pause => EngineError::Paused,
                    ControlSignal::Cancel => EngineError::Cancelled,
                    ControlSignal::Run => unreachable!(),
                });
            }

            {
                let mut st = self.state.lock().unwrap();
                let is_head = st.waiting.front().is_some_and(|front| front == id);
                if is_head && st.active_count < st.max_concurrent {
                    st.waiting.pop_front();
                    st.active_count += 1;
                    self.notify.notify_waiters();
                    return Ok(SlotGuard {
                        queue: Arc::clone(self),
                    });
                }
            }

            tokio::select! {
                _ = &mut notified => {}
                changed = control_rx.changed() => {
                    if changed.is_err() {
                        self.remove_waiting(id);
                        return Err(EngineError::Cancelled);
                    }
                }
            }
        }
    }
}
