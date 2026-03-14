# SKALE Base Sepolia Transaction Engine

Ultra-high-performance Rust transaction engine for the SKALE Base Sepolia network.
Continuously sends 1 wei (`10⁻¹⁸ CREDIT`) transactions, each to a newly
generated unique Ethereum address.

## Network

| Field | Value |
|---|---|
| Network | SKALE Base Sepolia |
| RPC | `https://base-sepolia-testnet.skalenodes.com/v1/jubilant-horrible-ancha` |
| Chain ID | `324705682` (`0x135A9D92`) |
| Native Token | CREDIT (18 decimals) |
| Explorer | <https://base-sepolia-testnet-explorer.skalenodes.com/> |

## Features

- Sends **1 wei** per transaction to a unique, freshly generated address
- **Multi-stage pipeline**: address generators → transaction builder/signer → broadcast workers
- **Atomic nonce management** – no RPC round-trips for nonce; zero contention
- **Multi-RPC load distribution** – round-robin across multiple endpoints
- **Connection pooling** and HTTP keep-alive for maximum throughput
- **Real-time metrics** – TPS, sent/failed counts, nonce position (every 5 s)
- **Gas-style terminal stats** – speed + fee telemetry inspired by explorer transaction panels (TPS avg/peak, gas used, fee, gas price, RPC latency)
- **Continuous operation** – runs indefinitely; handles RPC errors gracefully

## Prerequisites

- [Rust toolchain](https://rustup.rs/) (1.70+)
- A funded wallet on SKALE Base Sepolia (even a tiny amount suffices)

## Usage

```bash
# Via environment variable
export PRIVATE_KEY=your_private_key_hex
cargo run --release

# Via CLI argument
cargo run --release -- --private-key YOUR_KEY

# Custom settings
cargo run --release -- \
  -k YOUR_KEY \
  -w 128 \
  -p 200000 \
  -g 8 \
  --rpc-urls "https://rpc1.example.com,https://rpc2.example.com"
```

## Configuration

| Flag | Env Var | Default | Description |
|---|---|---|---|
| `-k`, `--private-key` | `PRIVATE_KEY` | *required* | Sender's private key (hex) |
| `-r`, `--rpc-urls` | `RPC_URLS` | SKALE default | Comma-separated RPC endpoints |
| `-w`, `--workers` | — | `64` | Async broadcast worker tasks |
| `-p`, `--pool-size` | — | `100000` | Address pool channel capacity |
| `-g`, `--generators` | — | `4` | Address generator OS threads |
| `--gas-price` | — | `0` | Gas price in wei |

## Architecture

```
Address Generator Threads (OS threads, random 20-byte addresses)
            ↓
     crossbeam bounded channel (address pool)
            ↓
Broadcast Workers (tokio async tasks)
  ├─ fetch address from pool
  ├─ reserve nonce (AtomicU64)
  ├─ build EIP-155 legacy tx
  ├─ sign with secp256k1 (k256)
  └─ POST eth_sendRawTransaction (reqwest, connection-pooled)
            ↓
      Metrics Reporter (periodic TPS logging)
```

## Expected Performance

| VPS | Approx. TPS |
|---|---|
| 2 vCPU | 100 – 300 |
| 4 vCPU | 300 – 900 |
| 8 vCPU | 1 000 – 3 000 |


## Terminal telemetry style

The metrics output now includes a "gas-like" line similar to explorer transaction detail pages, for example:

```text
⛽ Speed/Stats | Sent: 1800 | Failed: 2 | TPS(avg): 360.0 | TPS(peak): 402.5 | Avg RPC: 11.42 ms | Nonce: 159594
⛽ Gas usage | Total gas used: 37800000 | Fee: 0 wei | Gas price: 0 wei (0.000000 Gwei) | Addr pool produced: 500000
```
