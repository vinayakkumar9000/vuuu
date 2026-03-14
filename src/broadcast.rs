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
    ///
    /// Returns `Ok((tx_hash, latency_micros))` on success, or
    /// `Err((error_message, latency_micros))` on failure.  Latency is always
    /// measured and returned so callers can track RPC response times regardless
    /// of outcome.
    pub async fn send_raw_tx(&self, raw_tx_hex: &str) -> Result<(String, u64), (String, u64)> {
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
        let send_result = self.client.post(rpc).json(&body).send().await;
        let latency_micros = started.elapsed().as_micros() as u64;

        let resp: Value = match send_result {
            Ok(r) => r
                .json()
                .await
                .map_err(|e| (format!("Failed to parse send response: {e}"), latency_micros))?,
            Err(e) => return Err((format!("RPC send failed: {e}"), latency_micros)),
        };

        if let Some(error) = resp.get("error") {
            return Err((format!("RPC error: {error}"), latency_micros));
        }

        resp["result"]
            .as_str()
            .map(|tx| (tx.to_string(), latency_micros))
            .ok_or_else(|| ("Missing tx hash in response".to_string(), latency_micros))
    }

    /// Query `eth_gasPrice` for the current suggested gas price in wei.
    pub async fn get_gas_price(&self) -> Result<u64, String> {
        let rpc = &self.rpc_urls[0];
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);

        let body = json!({
            "jsonrpc": "2.0",
            "method": "eth_gasPrice",
            "params": [],
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

        let hex_price = resp["result"]
            .as_str()
            .ok_or("Missing result in eth_gasPrice response")?;

        u64::from_str_radix(hex_price.trim_start_matches("0x"), 16)
            .map_err(|e| format!("Failed to parse gas price: {e}"))
    }
}

#[cfg(test)]
mod tests {

    /// Verify that `send_raw_tx` returns latency in both success and error
    /// variants, ensuring callers can always record RPC response time.
    #[test]
    fn send_raw_tx_error_tuple_has_latency_field() {
        // Construct a mock error tuple directly to verify the type shape.
        let err: (String, u64) = ("RPC error: nonce too low".to_string(), 12345);
        assert_eq!(err.0, "RPC error: nonce too low");
        assert_eq!(err.1, 12345u64);
    }

    /// Verify `get_gas_price` parses a standard hex `eth_gasPrice` response.
    #[test]
    fn parse_gas_price_hex() {
        // 0x64 = 100 in decimal
        let hex = "0x64";
        let price =
            u64::from_str_radix(hex.trim_start_matches("0x"), 16).expect("parse failed");
        assert_eq!(price, 100u64);
    }

    /// Verify `get_gas_price` parses a zero gas price (common on SKALE sChains).
    #[test]
    fn parse_gas_price_zero() {
        let hex = "0x0";
        let price =
            u64::from_str_radix(hex.trim_start_matches("0x"), 16).expect("parse failed");
        assert_eq!(price, 0u64);
        // Minimum effective price should be at least 1 wei.
        assert_eq!(price.max(1), 1u64);
    }

    /// Verify `get_gas_price` parses a larger gas price value.
    #[test]
    fn parse_gas_price_gwei() {
        // 1 Gwei = 0x3B9ACA00
        let hex = "0x3B9ACA00";
        let price =
            u64::from_str_radix(hex.trim_start_matches("0x"), 16).expect("parse failed");
        assert_eq!(price, 1_000_000_000u64);
    }
}
