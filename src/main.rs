mod broadcast;
mod config;
mod metrics;
mod rlp;
mod transaction;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use crossbeam_channel::bounded;
use k256::ecdsa::SigningKey;
use rand::RngCore;
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

    info!("=== SKALE Base Sepolia Transaction Engine ===");
    info!("Chain ID: {}", config::CHAIN_ID);
    info!("Workers: {}", config.workers);
    info!("Address pool: {}", config.pool_size);
    info!("Generator threads: {}", config.generators);
    info!("Gas price: {} wei", config.gas_price);
    info!("RPC endpoints: {:?}", config.rpc_urls);
    info!("CPU cores: {}", num_cpus::get());

    // --------------- Parse private key ---------------
    let key_hex = config.private_key.trim_start_matches("0x");
    let key_bytes = hex::decode(key_hex).expect("Invalid private key hex");
    assert!(key_bytes.len() == 32, "Private key must be 32 bytes");
    let signing_key =
        SigningKey::from_slice(&key_bytes).expect("Private key is not a valid secp256k1 scalar");

    let sender_address = address_from_key(&signing_key);
    let sender_hex = format!("0x{}", hex::encode(sender_address));
    info!("Sender address: {}", sender_hex);

    // --------------- RPC setup ---------------
    let broadcaster = Arc::new(Broadcaster::new(config.rpc_urls.clone()));

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
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let sent = m.sent.load(Ordering::Relaxed);
                let failed = m.failed.load(Ordering::Relaxed);
                let addrs = m.addresses_generated.load(Ordering::Relaxed);
                let tps = m.tps();
                let nonce = nc.load(Ordering::Relaxed);
                info!(
                    "📊 Sent: {} | Failed: {} | TPS: {:.1} | Addresses: {} | Nonce: {}",
                    sent, failed, tps, addrs, nonce
                );
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
        let gas_price = config.gas_price;

        handles.push(tokio::spawn(async move {
            broadcast_worker(worker_id, rx, key, bc, nc, m, gas_price).await;
        }));
    }

    info!("🚀 Engine started — sending transactions continuously …");

    // Block until all workers finish (they run indefinitely).
    for handle in handles {
        let _ = handle.await;
    }
}

/// Async broadcast worker.
///
/// Continuously pulls addresses from the pool, builds, signs, and broadcasts
/// transactions.  Uses non-blocking `try_recv` + `yield_now` to avoid blocking
/// the Tokio executor.
async fn broadcast_worker(
    worker_id: usize,
    addr_rx: crossbeam_channel::Receiver<[u8; 20]>,
    signing_key: SigningKey,
    broadcaster: Arc<Broadcaster>,
    nonce_counter: Arc<AtomicU64>,
    metrics: Arc<Metrics>,
    gas_price: u64,
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
                metrics.failed.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };

        let raw_tx_hex = format!("0x{}", hex::encode(&raw_tx));

        // Broadcast (fire-and-forget — we do not wait for confirmation).
        match broadcaster.send_raw_tx(&raw_tx_hex).await {
            Ok(tx_hash) => {
                metrics.sent.fetch_add(1, Ordering::Relaxed);
                tracing::debug!(
                    "Worker {}: nonce={} to=0x{} hash={}",
                    worker_id,
                    nonce,
                    hex::encode(to_address),
                    tx_hash
                );
            }
            Err(e) => {
                metrics.failed.fetch_add(1, Ordering::Relaxed);
                tracing::debug!("Worker {}: nonce={} failed: {}", worker_id, nonce, e);
            }
        }
    }
}
