/// Real-time metrics tracking for the transaction engine.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub struct Metrics {
    pub sent: AtomicU64,
    pub failed: AtomicU64,
    pub addresses_generated: AtomicU64,
    pub start_time: Instant,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            sent: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            addresses_generated: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    /// Calculate the current average transactions per second.
    pub fn tps(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.sent.load(Ordering::Relaxed) as f64 / elapsed
        } else {
            0.0
        }
    }
}
