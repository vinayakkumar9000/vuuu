mod broadcast;
mod config;
mod gas_price;
mod metrics;
mod rlp;
mod transaction;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::{io, io::IsTerminal, io::Write};

use clap::Parser;
use crossbeam_channel::bounded;
use k256::ecdsa::SigningKey;
use rand::RngCore;
use reqwest::Client;
use tracing::{error, info, warn};

use crate::broadcast::Broadcaster;
use crate::config::Config;
use crate::metrics::Metrics;
use crate::transaction::{address_from_key, LegacyTx};

#[tokio::main]
async fn main() {
    // Initialise structured logging.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("skale_tx_engine=info".parse().unwrap()),
        )
        .init();

    let config = Config::parse();

    // --------------- Validate worker count ---------------
    if config.workers == 0 || config.workers > config::MAX_WORKERS {
        error!(
            "Worker count must be between 1 and {} (got {}). \
             {} workers is already very aggressive for a single wallet.",
            config::MAX_WORKERS,
            config.workers,
            config::MAX_WORKERS
        );
        std::process::exit(1);
    }

    info!("=== SKALE Base Sepolia Transaction Engine ===");
    info!("Chain ID: {}", config::CHAIN_ID);
    info!("Workers: {}", config.workers);
    info!("Address pool: {}", config.pool_size);
    info!("Generator threads: {}", config.generators);
    info!("RPC endpoints: {:?}", config.rpc_urls);
    info!("CPU cores: {}", num_cpus::get());

    // --------------- Parse private key ---------------
    let key_hex = config.private_key.trim_start_matches("0x");
    let key_bytes = hex::decode(key_hex).expect("Invalid private key hex");
    assert_eq!(key_bytes.len(), 32, "Private key must be 32 bytes");
    let signing_key =
        SigningKey::from_slice(&key_bytes).expect("Private key is not a valid secp256k1 scalar");

    let sender_address = address_from_key(&signing_key);
    let sender_hex = format!("0x{}", hex::encode(sender_address));
    info!("Sender address: {}", sender_hex);

    // --------------- RPC setup ---------------
    let broadcaster = Arc::new(Broadcaster::new(config.rpc_urls.clone()));

    // --------------- Gas price resolution ---------------
    // Build a dedicated HTTP client for the gas price fetcher so it doesn't
    // share the broadcaster's connection pool / timeout settings.
    let gas_price_http = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to build gas-price HTTP client");

    let initial_gas_price = if let Some(price) = config.gas_price {
        info!(
            "⛽ Gas price (explicit override): {} wei ({:.6} Gwei)",
            price,
            price as f64 / 1e9
        );
        if price == 0 {
            warn!(
                "⚠️  Gas price is 0 wei — transactions are likely to be rejected on \
                 SKALE Base Sepolia (base fee is non-zero). Pass --gas-price <WEI> \
                 or set GAS_PRICE env to fix this."
            );
        }
        price
    } else {
        // Try eth_gasPrice from the RPC first (most reliable source).
        match broadcaster.get_gas_price().await {
            Ok(price) => {
                info!(
                    "⛽ Gas price (from RPC eth_gasPrice): {} wei ({:.6} Gwei)",
                    price,
                    price as f64 / 1e9
                );
                // Ensure at least 1 wei so SKALE does not reject zero-price txs.
                let effective = price.max(1);
                if effective != price {
                    info!("⛽ Gas price adjusted to minimum 1 wei");
                }
                effective
            }
            Err(e) => {
                warn!(
                    "⚠️  eth_gasPrice RPC call failed ({}). Falling back to explorer API.",
                    e
                );
                gas_price::resolve_initial_gas_price(&gas_price_http, None).await
            }
        }
    };

    let gas_price_state = Arc::new(AtomicU64::new(initial_gas_price));

    // Start the background poller only when the user has not pinned a price.
    if config.gas_price.is_none() {
        gas_price::spawn_gas_price_poller(
            gas_price_http,
            gas_price_state.clone(),
            config.gas_price_poll_secs,
        );
    }

    // Check balance.
    match broadcaster.get_balance(&sender_hex).await {
        Ok(balance) => {
            info!("Sender balance: {} wei", balance);
            if balance == 0 {
                error!("Sender has zero balance — fund the wallet first.");
                std::process::exit(1);
            }
        }
        Err(e) => {
            warn!("Failed to check balance: {}. Proceeding anyway.", e);
        }
    }

    // Fetch initial nonce.
    let initial_nonce = match broadcaster.get_nonce(&sender_hex).await {
        Ok(n) => {
            info!("Initial nonce: {}", n);
            n
        }
        Err(e) => {
            error!("Failed to get nonce: {}. Starting from 0.", e);
            0
        }
    };

    let nonce_counter = Arc::new(AtomicU64::new(initial_nonce));
    let metrics = Arc::new(Metrics::new());

    // --------------- Nonce resync background task ---------------
    // When the local nonce counter runs ahead of the on-chain confirmed nonce
    // (e.g. due to cascading transaction failures), all new transactions carry
    // "too high" nonces and are rejected.  This task periodically re-fetches
    // the pending nonce from the network and resets the counter whenever it
    // has drifted more than `MAX_NONCE_DRIFT` ahead.
    {
        // MAX_NONCE_DRIFT: tolerate up to this many in-flight/pending nonces
        // ahead of the confirmed chain nonce before forcing a resync.  With up
        // to 60 workers each incrementing by 1, ≈200 covers ~3 full scheduling
        // rounds while being small enough to recover within seconds when all
        // workers are stuck in a failure cascade.
        const MAX_NONCE_DRIFT: u64 = 200;
        const RESYNC_INTERVAL_SECS: u64 = 10;

        let bc = broadcaster.clone();
        let nc = nonce_counter.clone();
        let sender = sender_hex.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(RESYNC_INTERVAL_SECS)).await;
                match bc.get_nonce(&sender).await {
                    Ok(chain_nonce) => {
                        let local_nonce = nc.load(Ordering::Relaxed);
                        if local_nonce > chain_nonce + MAX_NONCE_DRIFT {
                            // Use compare_exchange so only one concurrent resync wins.
                            if nc
                                .compare_exchange(
                                    local_nonce,
                                    chain_nonce,
                                    Ordering::Relaxed,
                                    Ordering::Relaxed,
                                )
                                .is_ok()
                            {
                                warn!(
                                    "🔄 Nonce resynced: local {} → chain {} (drift was {})",
                                    local_nonce,
                                    chain_nonce,
                                    local_nonce - chain_nonce
                                );
                            }
                        }
                    }
                    Err(e) => warn!("⚠️  Nonce resync failed: {}", e),
                }
            }
        });
    }

    // --------------- Address generator threads ---------------
    let (addr_tx, addr_rx) = bounded::<[u8; 20]>(config.pool_size);

    for _ in 0..config.generators {
        let tx = addr_tx.clone();
        let m = metrics.clone();
        std::thread::spawn(move || {
            let mut rng = rand::thread_rng();
            loop {
                let mut addr = [0u8; 20];
                rng.fill_bytes(&mut addr);
                m.addresses_generated.fetch_add(1, Ordering::Relaxed);
                if tx.send(addr).is_err() {
                    break; // channel closed
                }
            }
        });
    }
    // Drop the original sender so that the channel is only kept alive by the
    // cloned senders living inside the generator threads.
    drop(addr_tx);

    // --------------- Metrics reporter ---------------
    {
        let m = metrics.clone();
        let nc = nonce_counter.clone();
        let gp_state = gas_price_state.clone();
        tokio::spawn(async move {
            let use_inline_status = io::stdout().is_terminal();
            let mut prev_status_len = 0usize;
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let sent = m.sent.load(Ordering::Relaxed);
                let failed = m.failed.load(Ordering::Relaxed);
                let addrs = m.addresses_generated.load(Ordering::Relaxed);
                let total_gas = m.total_gas_used.load(Ordering::Relaxed);
                let total_fee = m.total_fee_wei.load(Ordering::Relaxed);
                let latency_samples = m.rpc_latency_samples.load(Ordering::Relaxed);
                let latency_sum = m.rpc_latency_micros_sum.load(Ordering::Relaxed);
                let avg_rpc_ms = if latency_samples > 0 {
                    (latency_sum as f64 / latency_samples as f64) / 1000.0
                } else {
                    0.0
                };
                let tps = m.tps();
                let peak_tps = m.update_peak_tps(tps);
                let nonce = nc.load(Ordering::Relaxed);
                let gas_price = gp_state.load(Ordering::Relaxed);
                let gas_price_gwei = gas_price as f64 / 1_000_000_000.0;
                let status_line = format!(
                    "⛽ Live Stats | Sent: {} | Failed: {} | TPS(avg): {:.1} | TPS(peak): {:.1} | Avg RPC: {:.2} ms | Nonce: {} | Gas: {} | Fee: {} wei | Gas price: {} wei ({:.6} Gwei) | Addr pool produced: {}",
                    sent,
                    failed,
                    tps,
                    peak_tps,
                    avg_rpc_ms,
                    nonce,
                    total_gas,
                    total_fee,
                    gas_price,
                    gas_price_gwei,
                    addrs
                );

                if use_inline_status {
                    let pad = prev_status_len.saturating_sub(status_line.len());
                    print!("\r{}{}", status_line, " ".repeat(pad));
                    let _ = io::stdout().flush();
                    prev_status_len = status_line.len();
                } else {
                    info!("{}", status_line);
                }
            }
        });
    }

    // --------------- Broadcast workers ---------------
    let mut handles = Vec::with_capacity(config.workers);
    for worker_id in 0..config.workers {
        let rx = addr_rx.clone();
        let key = signing_key.clone();
        let bc = broadcaster.clone();
        let nc = nonce_counter.clone();
        let m = metrics.clone();
        let gp = gas_price_state.clone();

        handles.push(tokio::spawn(async move {
            broadcast_worker(worker_id, rx, key, bc, nc, m, gp).await;
        }));
    }

    info!("🚀 Engine started — sending transactions continuously …");

    // Block until all workers finish (they run indefinitely).
    for handle in handles {
        let _ = handle.await;
    }
}

/// Returns `true` when the RPC error message indicates a gas-price-too-low
/// rejection (SKALE error code -32004 or similar wording).
fn is_gas_price_too_low(err: &str) -> bool {
    let lower = err.to_lowercase();
    lower.contains("gas price")
        || lower.contains("gasprice")
        || lower.contains("-32004")
        || lower.contains("underpriced")
}

/// Apply a 20 % safety bump to a gas price (integer arithmetic, minimum 1 wei).
fn bump_gas_price(price: u64) -> u64 {
    // 6/5 = 1.2×
    ((price * 6) / 5).max(1)
}

/// Async broadcast worker.
///
/// Continuously pulls addresses from the pool, builds, signs, and broadcasts
/// transactions.  Uses non-blocking `try_recv` + `yield_now` to avoid blocking
/// the Tokio executor.
///
/// When a transaction is rejected because the gas price is too low the worker:
///   1. Fetches a fresh gas price via `eth_gasPrice` and applies a 1.2× bump.
///   2. Updates the shared `gas_price_state` so other workers benefit too.
///   3. Retries the **same nonce** once with the updated price.
async fn broadcast_worker(
    worker_id: usize,
    addr_rx: crossbeam_channel::Receiver<[u8; 20]>,
    signing_key: SigningKey,
    broadcaster: Arc<Broadcaster>,
    nonce_counter: Arc<AtomicU64>,
    metrics: Arc<Metrics>,
    gas_price_state: Arc<AtomicU64>,
) {
    loop {
        // Non-blocking receive from the crossbeam channel.
        let to_address = loop {
            match addr_rx.try_recv() {
                Ok(addr) => break addr,
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    tokio::task::yield_now().await;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    warn!("Worker {}: address channel closed", worker_id);
                    return;
                }
            }
        };

        // Atomically reserve a nonce.
        let nonce = nonce_counter.fetch_add(1, Ordering::Relaxed);

        // Read the latest gas price (updated in the background by the poller).
        let gas_price = gas_price_state.load(Ordering::Relaxed);

        // Build & sign transaction.
        let tx = LegacyTx {
            nonce,
            gas_price,
            gas_limit: config::GAS_LIMIT,
            to: to_address,
            value: config::TX_VALUE,
        };

        let raw_tx = match tx.sign(&signing_key) {
            Ok(raw) => raw,
            Err(e) => {
                error!("Worker {}: signing failed: {}", worker_id, e);
                metrics.record_failure();
                continue;
            }
        };

        let raw_tx_hex = format!("0x{}", hex::encode(&raw_tx));

        // Broadcast (fire-and-forget — we do not wait for confirmation).
        match broadcaster.send_raw_tx(&raw_tx_hex).await {
            Ok((tx_hash, latency_micros)) => {
                metrics.record_success(config::GAS_LIMIT, gas_price);
                metrics.record_rpc_latency(latency_micros);
                tracing::debug!(
                    "Worker {}: nonce={} to=0x{} hash={}",
                    worker_id,
                    nonce,
                    hex::encode(to_address),
                    tx_hash
                );
            }
            Err((e, latency_micros)) => {
                metrics.record_rpc_latency(latency_micros);

                if is_gas_price_too_low(&e) {
                    // ---- Gas-price-too-low: refresh price & retry once ----
                    warn!(
                        "Worker {}: nonce={} gas price too low — refreshing",
                        worker_id, nonce
                    );

                    let new_gas_price = match broadcaster.get_gas_price().await {
                        Ok(rpc_price) => bump_gas_price(rpc_price),
                        Err(gp_err) => {
                            warn!(
                                "Worker {}: eth_gasPrice failed ({}), bumping current price 1.2×",
                                worker_id, gp_err
                            );
                            bump_gas_price(gas_price)
                        }
                    };

                    // Update shared state so all workers pick up the new price.
                    gas_price_state.fetch_max(new_gas_price, Ordering::Relaxed);
                    info!(
                        "Worker {}: gas price bumped to {} wei ({:.6} Gwei)",
                        worker_id,
                        new_gas_price,
                        new_gas_price as f64 / 1e9
                    );

                    // Retry the same nonce with the higher gas price.
                    let retry_tx = LegacyTx {
                        nonce,
                        gas_price: new_gas_price,
                        gas_limit: config::GAS_LIMIT,
                        to: to_address,
                        value: config::TX_VALUE,
                    };

                    match retry_tx.sign(&signing_key) {
                        Ok(raw) => {
                            let raw_hex = format!("0x{}", hex::encode(&raw));
                            match broadcaster.send_raw_tx(&raw_hex).await {
                                Ok((tx_hash, lat)) => {
                                    metrics.record_success(config::GAS_LIMIT, new_gas_price);
                                    metrics.record_rpc_latency(lat);
                                    tracing::debug!(
                                        "Worker {}: nonce={} RETRY ok hash={}",
                                        worker_id,
                                        nonce,
                                        tx_hash
                                    );
                                }
                                Err((retry_err, lat)) => {
                                    metrics.record_failure();
                                    metrics.record_rpc_latency(lat);
                                    warn!(
                                        "Worker {}: nonce={} retry failed: {}",
                                        worker_id, nonce, retry_err
                                    );
                                }
                            }
                        }
                        Err(sign_err) => {
                            error!(
                                "Worker {}: retry signing failed: {}",
                                worker_id, sign_err
                            );
                            metrics.record_failure();
                        }
                    }
                } else {
                    // ---- Any other error: log and move on ----
                    metrics.record_failure();
                    warn!("Worker {}: nonce={} failed: {}", worker_id, nonce, e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gas_price_too_low_detects_error_code() {
        assert!(is_gas_price_too_low(
            "RPC error: {\"code\":-32004,\"message\":\"Transaction gas price lower than current eth_gasPrice\"}"
        ));
    }

    #[test]
    fn gas_price_too_low_detects_underpriced() {
        assert!(is_gas_price_too_low("transaction underpriced"));
    }

    #[test]
    fn gas_price_too_low_detects_gas_price_wording() {
        assert!(is_gas_price_too_low(
            "Transaction gas price lower than current eth_gasPrice"
        ));
    }

    #[test]
    fn gas_price_too_low_detects_camel_case() {
        assert!(is_gas_price_too_low("invalid gasPrice"));
    }

    #[test]
    fn gas_price_too_low_case_insensitive() {
        assert!(is_gas_price_too_low("Gas Price too low"));
        assert!(is_gas_price_too_low("GASPRICE invalid"));
        assert!(is_gas_price_too_low("UNDERPRICED transaction"));
    }

    #[test]
    fn gas_price_too_low_ignores_unrelated_errors() {
        assert!(!is_gas_price_too_low("nonce too low"));
        assert!(!is_gas_price_too_low("insufficient funds"));
        assert!(!is_gas_price_too_low("RPC send failed: timeout"));
    }

    #[test]
    fn bump_gas_price_applies_20_percent() {
        assert_eq!(bump_gas_price(100), 120);
        assert_eq!(bump_gas_price(1000), 1200);
    }

    #[test]
    fn bump_gas_price_minimum_is_one() {
        assert_eq!(bump_gas_price(0), 1);
    }

    #[test]
    fn bump_gas_price_handles_small_values() {
        // 1 * 6 / 5 = 1 (integer truncation), still >= 1
        assert_eq!(bump_gas_price(1), 1);
        // 5 * 6 / 5 = 6
        assert_eq!(bump_gas_price(5), 6);
    }
}
