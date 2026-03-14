/// Configuration for the SKALE transaction engine.
use clap::Parser;

pub const DEFAULT_RPC: &str =
    "https://base-sepolia-testnet.skalenodes.com/v1/jubilant-horrible-ancha";
pub const CHAIN_ID: u64 = 324_705_682;
pub const GAS_LIMIT: u64 = 21_000;
pub const TX_VALUE: u64 = 1; // 1 wei

#[derive(Parser, Debug)]
#[command(name = "skale-tx-engine")]
#[command(about = "Ultra-high-performance transaction engine for SKALE Base Sepolia")]
pub struct Config {
    /// Sender's private key (hex, with or without 0x prefix).
    #[arg(short = 'k', long, env = "PRIVATE_KEY")]
    pub private_key: String,

    /// RPC endpoints (comma-separated).
    #[arg(
        short,
        long,
        env = "RPC_URLS",
        default_value = DEFAULT_RPC,
        value_delimiter = ','
    )]
    pub rpc_urls: Vec<String>,

    /// Number of async broadcast worker tasks.
    #[arg(short = 'w', long, default_value = "64")]
    pub workers: usize,

    /// Address pool size (bounded channel capacity).
    #[arg(short = 'p', long, default_value = "100000")]
    pub pool_size: usize,

    /// Number of address generator OS threads.
    #[arg(short = 'g', long, default_value = "4")]
    pub generators: usize,

    /// Gas price in wei.
    ///
    /// When omitted the engine fetches the current gas price from the SKALE Base
    /// Sepolia block-explorer REST API and refreshes it at `--gas-price-poll-secs`
    /// interval.  The fallback when the API is unreachable is 100 wei.
    /// Set to 0 to send fee-free transactions (may be rejected by the network).
    #[arg(long, env = "GAS_PRICE")]
    pub gas_price: Option<u64>,

    /// How often (in seconds) to re-fetch the gas price from the explorer API.
    ///
    /// Only applies when `--gas-price` / `GAS_PRICE` is not set.
    /// A slower interval reduces API load; the default (60 s) is intentionally
    /// conservative.
    #[arg(long, env = "GAS_PRICE_POLL_SECS", default_value = "60")]
    pub gas_price_poll_secs: u64,
}
