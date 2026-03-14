/// Dynamic gas price support for SKALE Base Sepolia.
///
/// Fetches the current gas price from the SKALE Base Sepolia block explorer's
/// gastracker REST API. Falls back to a safe default when the API is unreachable.
/// A background poller keeps the shared price up to date at a configurable interval.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use tracing::{info, warn};

/// Safe fallback gas price in wei, used when the explorer API is unavailable.
/// Matches the observed base fee on SKALE Base Sepolia (100 wei = 0.0001 Gwei).
pub const FALLBACK_GAS_PRICE: u64 = 100;

const EXPLORER_API_URL: &str =
    "https://base-sepolia-testnet-explorer.skalenodes.com/api";

/// Fetch the current gas price in wei from the explorer's gastracker API.
///
/// Calls `GET /api?module=gastracker&action=gasoracle` and parses the
/// `SafeGasPrice` field (in Gwei), converting it to wei.
async fn fetch_from_explorer(client: &Client) -> Result<u64, String> {
    let resp: serde_json::Value = client
        .get(EXPLORER_API_URL)
        .query(&[("module", "gastracker"), ("action", "gasoracle")])
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("HTTP error: {e}"))?
        .json()
        .await
        .map_err(|e| format!("JSON parse error: {e}"))?;

    let status = resp["status"].as_str().unwrap_or("0");
    if status != "1" {
        return Err(format!(
            "API returned status={status} message={:?}",
            resp["message"]
        ));
    }

    let safe_gwei_str = resp["result"]["SafeGasPrice"]
        .as_str()
        .ok_or_else(|| "Missing SafeGasPrice in API response".to_string())?;

    let gwei: f64 = safe_gwei_str
        .parse()
        .map_err(|_| format!("Invalid SafeGasPrice value: {safe_gwei_str}"))?;

    // Convert Gwei → wei (1 Gwei = 10^9 wei).
    let wei = (gwei * 1_000_000_000.0).round() as u64;
    Ok(wei)
}

/// Resolve the effective gas price at startup:
///
/// - If `explicit` is `Some(v)`, use `v` (set via `--gas-price` or `GAS_PRICE` env).
/// - Otherwise, query the explorer API; fall back to [`FALLBACK_GAS_PRICE`] on error.
///
/// Emits a warning when the chosen price is 0.
pub async fn resolve_initial_gas_price(client: &Client, explicit: Option<u64>) -> u64 {
    if let Some(price) = explicit {
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
        return price;
    }

    // Auto-fetch from the block explorer REST API.
    match fetch_from_explorer(client).await {
        Ok(price) => {
            info!(
                "⛽ Gas price (from explorer API): {} wei ({:.6} Gwei)",
                price,
                price as f64 / 1e9
            );
            if price == 0 {
                warn!(
                    "⚠️  Explorer reports gas price 0 wei — transactions may be rejected. \
                     Override with --gas-price <WEI> or GAS_PRICE env."
                );
            }
            price
        }
        Err(e) => {
            warn!(
                "⚠️  Gas price fetch failed ({}). Using fallback {} wei ({:.6} Gwei). \
                 Override with --gas-price <WEI> or GAS_PRICE env.",
                e,
                FALLBACK_GAS_PRICE,
                FALLBACK_GAS_PRICE as f64 / 1e9
            );
            FALLBACK_GAS_PRICE
        }
    }
}

/// Spawn a background tokio task that re-fetches the gas price from the explorer
/// every `poll_secs` seconds and updates the shared `state`.
///
/// Only call this when the gas price is not explicitly overridden by the user.
pub fn spawn_gas_price_poller(client: Client, state: Arc<AtomicU64>, poll_secs: u64) {
    tokio::spawn(async move {
        info!("⛽ Gas price poller started (interval: {} s)", poll_secs);
        loop {
            tokio::time::sleep(Duration::from_secs(poll_secs)).await;
            match fetch_from_explorer(&client).await {
                Ok(price) => {
                    let old = state.swap(price, Ordering::Relaxed);
                    if price != old {
                        info!(
                            "⛽ Gas price updated: {} wei ({:.6} Gwei) [was {} wei]",
                            price,
                            price as f64 / 1e9,
                            old
                        );
                    }
                    if price == 0 {
                        warn!(
                            "⚠️  Gas price polled as 0 wei — transactions may be rejected on \
                             SKALE Base Sepolia."
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        "⚠️  Gas price poll failed ({}). Keeping current value {} wei.",
                        e,
                        state.load(Ordering::Relaxed)
                    );
                }
            }
        }
    });
}
