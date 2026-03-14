/// RPC broadcaster with connection pooling and multi-endpoint load distribution.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use reqwest::Client;
use serde_json::{json, Value};

pub struct Broadcaster {
    client: Client,
    rpc_urls: Vec<String>,
    request_id: AtomicU64,
}

impl Broadcaster {
    /// Create a new broadcaster with connection pooling.
    pub fn new(rpc_urls: Vec<String>) -> Self {
        let client = Client::builder()
            .pool_max_idle_per_host(100)
            .pool_idle_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            rpc_urls,
            request_id: AtomicU64::new(1),
        }
    }

    /// Query `eth_getBalance` for the given address.
    pub async fn get_balance(&self, address: &str) -> Result<u128, String> {
        let rpc = &self.rpc_urls[0];
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);

        let body = json!({
            "jsonrpc": "2.0",
            "method": "eth_getBalance",
            "params": [address, "latest"],
            "id": id
        });

        let resp: Value = self
            .client
            .post(rpc)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("RPC request failed: {e}"))?
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {e}"))?;

        if let Some(error) = resp.get("error") {
            return Err(format!("RPC error: {error}"));
        }

        let hex_balance = resp["result"]
            .as_str()
            .ok_or("Missing result in balance response")?;

        u128::from_str_radix(hex_balance.trim_start_matches("0x"), 16)
            .map_err(|e| format!("Failed to parse balance: {e}"))
    }

    /// Query `eth_getTransactionCount` for the given address.
    pub async fn get_nonce(&self, address: &str) -> Result<u64, String> {
        let rpc = &self.rpc_urls[0];
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);

        let body = json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionCount",
            "params": [address, "pending"],
            "id": id
        });

        let resp: Value = self
            .client
            .post(rpc)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("RPC request failed: {e}"))?
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {e}"))?;

        if let Some(error) = resp.get("error") {
            return Err(format!("RPC error: {error}"));
        }

        let hex_nonce = resp["result"]
            .as_str()
            .ok_or("Missing result in nonce response")?;

        u64::from_str_radix(hex_nonce.trim_start_matches("0x"), 16)
            .map_err(|e| format!("Failed to parse nonce: {e}"))
    }

    /// Broadcast a raw signed transaction via `eth_sendRawTransaction`.
    /// Distributes requests across configured RPC endpoints.
    pub async fn send_raw_tx(&self, raw_tx_hex: &str) -> Result<(String, u64), String> {
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);
        let rpc_idx = id as usize % self.rpc_urls.len();
        let rpc = &self.rpc_urls[rpc_idx];

        let body = json!({
            "jsonrpc": "2.0",
            "method": "eth_sendRawTransaction",
            "params": [raw_tx_hex],
            "id": id
        });

        let started = Instant::now();
        let resp: Value = self
            .client
            .post(rpc)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("RPC send failed: {e}"))?
            .json()
            .await
            .map_err(|e| format!("Failed to parse send response: {e}"))?;
        let latency_micros = started.elapsed().as_micros().min(u64::MAX as u128) as u64;

        if let Some(error) = resp.get("error") {
            return Err(format!("RPC error: {error}"));
        }

        resp["result"]
            .as_str()
            .map(|tx| (tx.to_string(), latency_micros))
            .ok_or_else(|| "Missing tx hash in response".to_string())
    }
}
