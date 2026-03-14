/// Real-time metrics tracking for the transaction engine.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub struct Metrics {
    pub sent: AtomicU64,
    pub failed: AtomicU64,
    pub addresses_generated: AtomicU64,
    pub total_gas_used: AtomicU64,
    pub total_fee_wei: AtomicU64,
    pub rpc_latency_micros_sum: AtomicU64,
    pub rpc_latency_samples: AtomicU64,
    pub peak_tps_milli: AtomicU64,
    pub start_time: Instant,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            sent: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            addresses_generated: AtomicU64::new(0),
            total_gas_used: AtomicU64::new(0),
            total_fee_wei: AtomicU64::new(0),
            rpc_latency_micros_sum: AtomicU64::new(0),
            rpc_latency_samples: AtomicU64::new(0),
            peak_tps_milli: AtomicU64::new(0),
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

    /// Record a successful transaction and fee accounting.
    pub fn record_success(&self, gas_limit: u64, gas_price: u64) {
        self.sent.fetch_add(1, Ordering::Relaxed);
        self.total_gas_used.fetch_add(gas_limit, Ordering::Relaxed);
        let fee = gas_limit.saturating_mul(gas_price);
        let _ = self
            .total_fee_wei
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |prev| {
                Some(prev.saturating_add(fee))
            });
    }

    /// Record one failed transaction.
    pub fn record_failure(&self) {
        self.failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Record RPC latency in microseconds.
    pub fn record_rpc_latency(&self, latency_micros: u64) {
        self.rpc_latency_micros_sum
            .fetch_add(latency_micros, Ordering::Relaxed);
        self.rpc_latency_samples.fetch_add(1, Ordering::Relaxed);
    }

    /// Update peak TPS if `current_tps` is higher than the previous maximum.
    pub fn update_peak_tps(&self, current_tps: f64) -> f64 {
        let scaled = (current_tps * 1000.0) as u64;

        loop {
            let prev = self.peak_tps_milli.load(Ordering::Relaxed);
            if scaled <= prev {
                return prev as f64 / 1000.0;
            }

            if self
                .peak_tps_milli
                .compare_exchange(prev, scaled, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return current_tps;
            }
        }
    }
}
