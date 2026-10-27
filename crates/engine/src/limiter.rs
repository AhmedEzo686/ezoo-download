//! Global speed limiter (token bucket) shared by every connection.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct SpeedLimiter {
    /// bytes per second, 0 = unlimited
    rate: AtomicU64,
    bucket: Mutex<(f64, Instant)>,
}

impl SpeedLimiter {
    pub fn new(bytes_per_sec: u64) -> Self {
        SpeedLimiter {
            rate: AtomicU64::new(bytes_per_sec),
            bucket: Mutex::new((bytes_per_sec as f64, Instant::now())),
        }
    }

    pub fn set_rate(&self, bytes_per_sec: u64) {
        self.rate.store(bytes_per_sec, Ordering::Relaxed);
    }

    /// Account for `n` bytes; sleeps when we are over budget.
    pub async fn acquire(&self, n: usize) {
        let rate = self.rate.load(Ordering::Relaxed);
        if rate == 0 {
            return;
        }
        let wait = {
            let mut b = self.bucket.lock().unwrap();
            let now = Instant::now();
            let elapsed = now.duration_since(b.1).as_secs_f64();
            b.1 = now;
            let cap = rate as f64;
            b.0 = (b.0 + elapsed * cap).min(cap);
            b.0 -= n as f64;
            // many connections at once: the debt stays within one second
            // of the limit, so no connection waits for seconds on end
            b.0 = b.0.max(-cap);
            if b.0 < 0.0 {
                -b.0 / cap
            } else {
                0.0
            }
        };
        if wait > 0.0 {
            tokio::time::sleep(Duration::from_secs_f64(wait.min(5.0))).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn no_long_stalls_with_many_connections() {
        let l = SpeedLimiter::new(1_000_000);
        let start = Instant::now();
        // 32 connections asking for a big piece each at the same moment
        for _ in 0..32 {
            l.acquire(64 * 1024).await;
        }
        // about 2 MB at 1 MB/s: roughly two seconds, never many times that
        assert!(start.elapsed() < Duration::from_secs(4));
        let t = Instant::now();
        l.acquire(1).await;
        assert!(t.elapsed() < Duration::from_millis(1100), "one more small piece waits at most about a second");
    }
}
