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
            // Prevent token debt from growing arbitrarily large when many connections
            // acquire tokens concurrently. Clamping to -cap bounds debt to at most 1 second of rate.
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
    async fn limiter_unlimited() {
        let limiter = SpeedLimiter::new(0);
        limiter.acquire(1_000_000).await;
    }

    #[tokio::test]
    async fn limiter_clamps_debt() {
        let rate = 100_000; // 100 KB/s
        let limiter = SpeedLimiter::new(rate);

        // Huge chunk exceeding 10x the rate
        limiter.acquire(1_000_000).await;

        let b = limiter.bucket.lock().unwrap();
        let cap = rate as f64;
        // The bucket debt should be clamped to -cap, not -900_000
        assert!(b.0 >= -cap, "bucket value {} should be >= -cap (-{})", b.0, cap);
    }

    #[tokio::test]
    async fn limiter_set_rate() {
        let limiter = SpeedLimiter::new(10_000);
        limiter.set_rate(50_000);
        assert_eq!(limiter.rate.load(Ordering::Relaxed), 50_000);
    }
}
