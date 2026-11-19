//! Shared async token-bucket bandwidth limiter across active `Worker`s.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;

#[derive(Debug)]
struct BucketState {
    tokens: f64,
    last_refill: Instant,
}

/// Shared global download bandwidth limiter (`0` = unlimited).
#[derive(Debug)]
pub(crate) struct BandwidthLimiter {
    bytes_per_sec: AtomicU64,
    bucket: Mutex<BucketState>,
}

impl BandwidthLimiter {
    /// Creates a limiter initialized to `limit_kib` KiB/s (`0` = unlimited).
    pub(crate) fn new(limit_kib: u64) -> Self {
        Self {
            bytes_per_sec: AtomicU64::new(limit_kib.saturating_mul(1024)),
            bucket: Mutex::new(BucketState {
                tokens: 0.0,
                last_refill: Instant::now(),
            }),
        }
    }

    /// Updates the rate limit dynamically in KiB/s (`0` = unlimited).
    pub(crate) fn set_limit_kib(&self, limit_kib: u64) {
        self.bytes_per_sec
            .store(limit_kib.saturating_mul(1024), Ordering::Relaxed);
    }

    /// Waits until `bytes` tokens are available under the current global limit.
    pub(crate) async fn acquire(&self, bytes: u64) {
        let rate = self.bytes_per_sec.load(Ordering::Relaxed);
        if rate == 0 || bytes == 0 {
            return;
        }

        let needed = bytes as f64;

        loop {
            let sleep_dur = {
                let rate_now = self.bytes_per_sec.load(Ordering::Relaxed);
                if rate_now == 0 {
                    return;
                }
                let rate_f = rate_now as f64;
                let max_burst = rate_f.max(needed);
                let mut guard = self.bucket.lock().await;
                let now = Instant::now();
                let elapsed = now.saturating_duration_since(guard.last_refill).as_secs_f64();
                guard.last_refill = now;
                guard.tokens = (guard.tokens + elapsed * rate_f).min(max_burst);

                if guard.tokens >= needed {
                    guard.tokens -= needed;
                    return;
                }

                // Preserve accumulated tokens while sleeping for the remaining deficit.
                let deficit = needed - guard.tokens;
                Duration::from_secs_f64((deficit / rate_f).min(0.25)).max(Duration::from_millis(5))
            };

            tokio::time::sleep(sleep_dur).await;
        }
    }
}
