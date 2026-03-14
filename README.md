<div align="center">

<img src="https://readme-typing-svg.demolab.com?font=Fira+Code&size=32&duration=2800&pause=2000&color=00D4FF&center=true&vCenter=true&width=800&lines=⚡+SKALE+Tx+Engine;Ultra-High-Performance+Rust+%7C+SKALE+Base+Sepolia;Continuous+Transactions+at+Lightning+Speed" alt="SKALE Tx Engine - Ultra-High-Performance Rust for SKALE Base Sepolia - Continuous Transactions at Lightning Speed" />

<br/>

[![Rust](https://img.shields.io/badge/Rust-1.70%2B-orange?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![SKALE](https://img.shields.io/badge/SKALE-Base%20Sepolia-blue?style=for-the-badge)](https://skale.space/)
[![License](https://img.shields.io/badge/License-MIT-green?style=for-the-badge)](LICENSE)
[![TPS](https://img.shields.io/badge/TPS-Up%20to%203000%2B-brightgreen?style=for-the-badge)](https://skale.space/)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20Windows%20%7C%20Cloud-lightgrey?style=for-the-badge)](https://github.com)

<br/>

> **Ultra-high-performance Rust transaction engine for the SKALE Base Sepolia network.**  
> Continuously sends **1 wei** (`10⁻¹⁸ CREDIT`) transactions, each to a freshly generated unique Ethereum address.  
> Zero-fee, atomic nonce management, connection pooling, real-time TPS telemetry.

</div>

---

## 📋 Table of Contents

- [🌐 Network Info](#-network-info)
- [✨ Features](#-features)
- [🏗️ Architecture](#-architecture)
- [⚙️ Configuration](#-configuration)
- [⛽ Gas Price](#-gas-price)
- [🖥️ Windows — Local Setup](#-windows--local-setup)
- [🐧 Ubuntu / Debian Linux — Local or VPS](#-ubuntu--debian-linux--local-or-vps)
- [☁️ AWS EC2 — Ubuntu VPS](#-aws-ec2--ubuntu-vps)
- [🔷 Azure VM — Ubuntu Linux](#-azure-vm--ubuntu-linux)
- [🌊 DigitalOcean / Generic Linux VPS](#-digitalocean--generic-linux-vps)
- [📟 Terminal Output & Expected TPS](#-terminal-output--expected-tps)
- [🔧 Troubleshooting](#-troubleshooting)

---

## 🌐 Network Info

<div align="center">

| Field | Value |
|:---:|:---|
| 🌍 Network | SKALE Base Sepolia |
| 🔗 RPC | `https://base-sepolia-testnet.skalenodes.com/v1/jubilant-horrible-ancha` |
| 🔢 Chain ID | `324705682` (`0x135A9D92`) |
| 💎 Native Token | CREDIT (18 decimals) |
| 🔍 Explorer | [base-sepolia-testnet-explorer.skalenodes.com](https://base-sepolia-testnet-explorer.skalenodes.com/) |

</div>

---

## ✨ Features

<div align="center">

| Feature | Detail |
|:---:|:---|
| 💸 **1 wei per tx** | Sends to a unique freshly generated address every time |
| 🔄 **Multi-stage pipeline** | Address generators → tx builder/signer → broadcast workers |
| ⚛️ **Atomic nonce** | No RPC round-trips for nonce; zero contention |
| 🔀 **Multi-RPC load balance** | Round-robin across multiple endpoints |
| 🏊 **Connection pooling** | HTTP keep-alive for maximum throughput |
| 📊 **Real-time metrics** | TPS, sent/failed counts, nonce (every 5 s) |
| ⛽ **Gas-style telemetry** | TPS avg/peak, gas used, fee, gas price, RPC latency |
| 🔄 **Dynamic gas price** | Auto-fetches gas price from explorer API; slow polling keeps it current |
| ♾️ **Continuous operation** | Runs indefinitely; graceful RPC error handling |

</div>

---

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│           Address Generator Threads (OS threads)            │
│          Generate random 20-byte Ethereum addresses         │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│         crossbeam bounded channel  (address pool)           │
│              capacity: 100 000 addresses                    │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│              Broadcast Workers (tokio async tasks)          │
│   ├─ fetch address from pool                                │
│   ├─ reserve nonce (AtomicU64, lock-free)                   │
│   ├─ build EIP-155 legacy transaction                       │
│   ├─ sign with secp256k1 (k256 crate)                       │
│   └─ POST eth_sendRawTransaction (reqwest, pooled)          │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│         Metrics Reporter (every 5 seconds)                  │
│    TPS avg/peak · Sent/Failed · Nonce · RPC latency         │
└─────────────────────────────────────────────────────────────┘
```

---

## ⚙️ Configuration

> **📝 Note:** Only `--private-key` / `PRIVATE_KEY` is required. All other flags have sensible defaults.

| Flag | Env Var | Default | Description |
|:---|:---|:---:|:---|
| `-k`, `--private-key` | `PRIVATE_KEY` | **required** | Sender's private key (hex, with or without `0x`) |
| `-r`, `--rpc-urls` | `RPC_URLS` | SKALE default | Comma-separated list of RPC endpoints |
| `-w`, `--workers` | — | `64` | Number of async broadcast worker tasks |
| `-p`, `--pool-size` | — | `100000` | Address pool channel capacity |
| `-g`, `--generators` | — | `4` | Number of address generator OS threads |
| `--gas-price` | `GAS_PRICE` | *auto-fetch* | Gas price in wei (see [Gas Price](#-gas-price) below) |
| `--gas-price-poll-secs` | `GAS_PRICE_POLL_SECS` | `60` | How often (seconds) to refresh gas price from the explorer API |

---

## ⛽ Gas Price

SKALE Base Sepolia has a **non-zero base fee** (currently `0.0001 Gwei = 100 wei`).
Sending transactions with `gas_price = 0` causes them to be rejected, which is why you
may see a very high "Failed" count when using the historical default.

### How it works

1. **Auto-fetch (default)** — On startup the engine calls the
   [SKALE Base Sepolia explorer gastracker API](https://base-sepolia-testnet-explorer.skalenodes.com/api?module=gastracker&action=gasoracle)
   and uses the reported `SafeGasPrice` (converted from Gwei to wei) for all
   transactions.
2. **Background polling** — A background task re-fetches the gas price every
   `--gas-price-poll-secs` seconds (default: **60 s**). All workers read the latest
   value atomically, so gas price changes are picked up without a restart.
3. **Safe fallback** — If the API call fails for any reason, the engine falls back to
   **100 wei**. A warning is logged to indicate that the fallback is in use.
4. **Explicit override** — Pass `--gas-price <WEI>` (or set `GAS_PRICE=<WEI>` in the
   environment) to pin a specific value. The background poller is disabled when an
   explicit value is provided.

### Examples

```bash
# Let the engine auto-fetch the gas price (recommended):
cargo run --release -- -k YOUR_PRIVATE_KEY

# Pin an explicit gas price (100 wei = 0.0001 Gwei):
cargo run --release -- -k YOUR_PRIVATE_KEY --gas-price 100

# Override via environment variable:
GAS_PRICE=100 cargo run --release -- -k YOUR_PRIVATE_KEY

# Change the poll interval to 30 seconds:
cargo run --release -- -k YOUR_PRIVATE_KEY --gas-price-poll-secs 30
```

> ⚠️ **Warning:** Setting `--gas-price 0` will produce a warning in the logs
> and is likely to result in transaction rejections on SKALE Base Sepolia.

---

## 🖥️ Windows — Local Setup

<details>
<summary><b>Click to expand Windows setup steps</b></summary>

### Step 1 — Install Rust

Open **PowerShell** or **Command Prompt** and run:

```powershell
# Download and run the official Rust installer
winget install Rustlang.Rustup
```

Or visit [rustup.rs](https://rustup.rs/) and download `rustup-init.exe`.

After installation, **close and reopen your terminal**, then verify:

```powershell
rustc --version
cargo --version
```

Expected output:
```
rustc 1.XX.X (XXXXXXX XXXX-XX-XX)
cargo 1.XX.X (XXXXXXX XXXX-XX-XX)
```

> ⚠️ **Note:** If `rustc` is not found, ensure `%USERPROFILE%\.cargo\bin` is in your `PATH`.

---

### Step 2 — Install Git and Clone the Repository

```powershell
winget install Git.Git
git clone https://github.com/vinayakkumar9000/vuuu.git
cd vuuu
```

---

### Step 3 — Set Your Private Key

> ⚠️ **IMPORTANT:** Replace `YOUR_PRIVATE_KEY_HEX` with your actual wallet private key (64 hex characters, no `0x` prefix needed). **Never share your private key.**

```powershell
# PowerShell — set for this session only
$env:PRIVATE_KEY = "YOUR_PRIVATE_KEY_HEX"
```

Or pass it directly via CLI (see Step 4).

---

### Step 4 — Build and Run

```powershell
# Build in release mode (optimized — do this once)
cargo build --release

# Run using environment variable set above
cargo run --release

# OR pass the private key directly
cargo run --release -- --private-key YOUR_PRIVATE_KEY_HEX

# Custom workers and pool size (optional tuning)
cargo run --release -- `
  -k YOUR_PRIVATE_KEY_HEX `
  -w 128 `
  -p 200000 `
  -g 8
```

> 📝 **Note:** The first `cargo build --release` will take several minutes to compile all dependencies. Subsequent builds are fast.

**What you will see when it starts:**

```
2024-01-01T00:00:00Z  INFO skale_tx_engine: === SKALE Base Sepolia Transaction Engine ===
2024-01-01T00:00:00Z  INFO skale_tx_engine: Chain ID: 324705682
2024-01-01T00:00:00Z  INFO skale_tx_engine: Workers: 64
2024-01-01T00:00:00Z  INFO skale_tx_engine: Address pool: 100000
2024-01-01T00:00:00Z  INFO skale_tx_engine: Generator threads: 4
2024-01-01T00:00:00Z  INFO skale_tx_engine: Gas price: 0 wei
2024-01-01T00:00:00Z  INFO skale_tx_engine: RPC endpoints: ["https://base-sepolia-testnet.skalenodes.com/v1/jubilant-horrible-ancha"]
2024-01-01T00:00:00Z  INFO skale_tx_engine: CPU cores: 8
2024-01-01T00:00:00Z  INFO skale_tx_engine: Sender address: 0xYOUR_ADDRESS
2024-01-01T00:00:00Z  INFO skale_tx_engine: Sender balance: 1000000000000000000 wei
2024-01-01T00:00:00Z  INFO skale_tx_engine: Initial nonce: 0
2024-01-01T00:00:00Z  INFO skale_tx_engine: 🚀 Engine started — sending transactions continuously …
```

</details>

---

## 🐧 Ubuntu / Debian Linux — Local or VPS

<details>
<summary><b>Click to expand Ubuntu / Debian setup steps</b></summary>

### Step 1 — Update System and Install Dependencies

```bash
sudo apt update && sudo apt upgrade -y
sudo apt install -y curl git build-essential pkg-config libssl-dev
```

---

### Step 2 — Install Rust via rustup

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

When prompted, the installer asks:

```
1) Proceed with standard installation (default)
2) Customize installation
3) Cancel installation
```

> 📝 **Note:** Press **`1`** then **Enter** to accept the default installation.

After it finishes, load Rust into the current shell:

```bash
source "$HOME/.cargo/env"
```

Verify:

```bash
rustc --version
cargo --version
```

---

### Step 3 — Clone the Repository

```bash
git clone https://github.com/vinayakkumar9000/vuuu.git
cd vuuu
```

---

### Step 4 — Set Your Private Key and Run

> ⚠️ **IMPORTANT:** Replace `YOUR_PRIVATE_KEY_HEX` with your real private key (64 hex characters). Keep it secret.

```bash
# Option A — via environment variable
export PRIVATE_KEY=YOUR_PRIVATE_KEY_HEX
cargo run --release

# Option B — inline for this session only (doesn't persist across reboots)
PRIVATE_KEY=YOUR_PRIVATE_KEY_HEX cargo run --release

# Option C — pass as CLI argument
cargo run --release -- --private-key YOUR_PRIVATE_KEY_HEX

# High-performance tuning (use for 4+ vCPU machines)
cargo run --release -- \
  -k YOUR_PRIVATE_KEY_HEX \
  -w 128 \
  -p 200000 \
  -g 8
```

> 📝 **Note:** On first run `cargo run --release` will compile the project (~2–5 min). Subsequent runs start in seconds.

---

### Step 5 — Keep It Running with `screen` or `tmux`

```bash
# Using screen
screen -S skale-engine
export PRIVATE_KEY=YOUR_PRIVATE_KEY_HEX
cargo run --release
# Detach: Ctrl+A then D
# Reattach: screen -r skale-engine

# Using tmux
tmux new -s skale-engine
export PRIVATE_KEY=YOUR_PRIVATE_KEY_HEX
cargo run --release
# Detach: Ctrl+B then D
# Reattach: tmux attach -t skale-engine
```

</details>

---

## ☁️ AWS EC2 — Ubuntu VPS

<details>
<summary><b>Click to expand AWS EC2 setup steps</b></summary>

### Step 1 — Launch an EC2 Instance

1. Go to [AWS EC2 Console](https://console.aws.amazon.com/ec2/) → **Launch Instance**
2. Choose **Ubuntu Server 22.04 LTS (HVM)** AMI
3. Select instance type:
   - `t3.medium` (2 vCPU, 4 GB) → ~100–300 TPS
   - `t3.large` (2 vCPU, 8 GB) → ~200–400 TPS
   - `c5.xlarge` (4 vCPU, 8 GB) → ~400–900 TPS ← **Recommended**
   - `c5.2xlarge` (8 vCPU, 16 GB) → ~1000–3000 TPS
4. Under **Security Group**, allow **outbound HTTPS (443)** (enabled by default)
5. Create or select a **Key Pair** (`.pem` file) — save it securely
6. Click **Launch Instance**

> ⚠️ **Note:** No inbound ports need to be opened (the engine only makes outbound HTTP/HTTPS requests).

---

### Step 2 — Connect to Your EC2 Instance

```bash
# From your local machine (Linux/macOS)
chmod 400 your-key.pem
ssh -i your-key.pem ubuntu@<EC2_PUBLIC_IP>

# From Windows PowerShell / Windows Terminal
ssh -i C:\path\to\your-key.pem ubuntu@<EC2_PUBLIC_IP>
```

> 📝 **Note:** Replace `<EC2_PUBLIC_IP>` with your instance's **Public IPv4 address** shown in the EC2 console.

---

### Step 3 — Install Dependencies

Once connected to the EC2 instance:

```bash
sudo apt update && sudo apt upgrade -y
sudo apt install -y curl git build-essential pkg-config libssl-dev screen
```

---

### Step 4 — Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Press 1 when prompted
source "$HOME/.cargo/env"
```

---

### Step 5 — Clone and Configure

```bash
git clone https://github.com/vinayakkumar9000/vuuu.git
cd vuuu
```

> ⚠️ **IMPORTANT:** Set your private key below. This is the hex private key of a wallet funded on SKALE Base Sepolia.

```bash
export PRIVATE_KEY=YOUR_PRIVATE_KEY_HEX
```

To make it persistent across reconnections, add it to `~/.bashrc`:

```bash
echo 'export PRIVATE_KEY=YOUR_PRIVATE_KEY_HEX' >> ~/.bashrc
source ~/.bashrc
```

> ⚠️ **Security Note:** Do not store your private key in plain text on shared or production systems. Consider using AWS Secrets Manager or environment files with restricted permissions (`chmod 600`).

---

### Step 6 — Build and Run in a Screen Session

```bash
screen -S skale-engine

# Inside screen — run with tuning for your instance size
# For c5.xlarge (4 vCPU):
cargo run --release -- -w 128 -g 8

# For c5.2xlarge (8 vCPU):
cargo run --release -- -w 256 -g 16

# Detach from screen without stopping engine
# Press: Ctrl+A, then D

# Reattach later
screen -r skale-engine
```

---

### Step 7 — Optional: Run as a systemd Service

```bash
sudo nano /etc/systemd/system/skale-engine.service
```

Paste the following (edit paths and key as needed):

```ini
[Unit]
Description=SKALE Transaction Engine
After=network.target

[Service]
Type=simple
User=ubuntu
WorkingDirectory=/home/ubuntu/vuuu
Environment="PRIVATE_KEY=YOUR_PRIVATE_KEY_HEX"
ExecStart=/home/ubuntu/.cargo/bin/cargo run --release -- -w 128 -g 8
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable skale-engine
sudo systemctl start skale-engine

# Check status
sudo systemctl status skale-engine

# View live logs
sudo journalctl -u skale-engine -f
```

> 📝 **Note:** For production systemd usage, build the binary first with `cargo build --release` and use `ExecStart=/home/ubuntu/vuuu/target/release/skale-tx-engine` for faster restarts.

</details>

---

## 🔷 Azure VM — Ubuntu Linux

<details>
<summary><b>Click to expand Azure VM setup steps</b></summary>

### Step 1 — Create an Azure Virtual Machine

1. Go to [Azure Portal](https://portal.azure.com/) → **Virtual Machines** → **Create**
2. Choose:
   - **Image:** Ubuntu Server 22.04 LTS
   - **Size:**
     - `Standard_B2s` (2 vCPU, 4 GB) → ~100–300 TPS
     - `Standard_D4s_v3` (4 vCPU, 16 GB) → ~400–900 TPS ← **Recommended**
     - `Standard_D8s_v3` (8 vCPU, 32 GB) → ~1000–3000 TPS
3. **Authentication:** SSH public key (generate a new key pair or upload existing)
4. **Inbound ports:** Allow **SSH (22)** only (outbound is unrestricted by default)
5. Click **Review + Create** → **Create**

> 📝 **Note:** Download the private key (`.pem`) immediately when prompted — you cannot retrieve it later.

---

### Step 2 — Connect via SSH

```bash
# From Linux/macOS
chmod 400 your-azure-key.pem
ssh -i your-azure-key.pem azureuser@<AZURE_PUBLIC_IP>

# From Windows PowerShell
ssh -i C:\path\to\your-azure-key.pem azureuser@<AZURE_PUBLIC_IP>
```

> 📝 **Note:** Replace `<AZURE_PUBLIC_IP>` with the **Public IP address** shown on the VM's Overview page. Default username is `azureuser`.

---

### Step 3 — Install Dependencies

```bash
sudo apt update && sudo apt upgrade -y
sudo apt install -y curl git build-essential pkg-config libssl-dev screen
```

---

### Step 4 — Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Press 1 when prompted
source "$HOME/.cargo/env"
```

---

### Step 5 — Clone and Run

```bash
git clone https://github.com/vinayakkumar9000/vuuu.git
cd vuuu

export PRIVATE_KEY=YOUR_PRIVATE_KEY_HEX

# Run inside screen
screen -S skale

# For Standard_D4s_v3 (4 vCPU):
cargo run --release -- -w 128 -g 8 -p 200000

# For Standard_D8s_v3 (8 vCPU):
cargo run --release -- -w 256 -g 16 -p 500000

# Detach: Ctrl+A then D
```

> ⚠️ **Note:** Azure NSGs (Network Security Groups) block inbound by default but **allow all outbound** — the engine will work without any extra NSG rule changes.

</details>

---

## 🌊 DigitalOcean / Generic Linux VPS

<details>
<summary><b>Click to expand DigitalOcean / Generic VPS setup steps</b></summary>

### Step 1 — Create a Droplet (DigitalOcean)

1. Go to [DigitalOcean](https://www.digitalocean.com/) → **Droplets** → **Create Droplet**
2. Choose:
   - **OS:** Ubuntu 22.04 LTS
   - **Plan:**
     - `Basic — 2 vCPU / 4 GB` → ~100–300 TPS
     - `Basic — 4 vCPU / 8 GB` → ~300–900 TPS
     - `CPU-Optimized — 4 vCPU / 8 GB` → ~500–1200 TPS ← **Recommended**
3. Add your SSH key or use a root password
4. Click **Create Droplet**

---

### Step 2 — Connect via SSH

```bash
ssh root@<DROPLET_IP>
# Or with SSH key:
ssh -i ~/.ssh/id_rsa root@<DROPLET_IP>
```

> 📝 **Note:** Replace `<DROPLET_IP>` with the IP shown in the DigitalOcean dashboard.

---

### Step 3 — Install Everything and Run

```bash
# Update & install deps
apt update && apt upgrade -y
apt install -y curl git build-essential pkg-config libssl-dev screen

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Press 1
source "$HOME/.cargo/env"

# Clone repo
git clone https://github.com/vinayakkumar9000/vuuu.git
cd vuuu

# Set private key
export PRIVATE_KEY=YOUR_PRIVATE_KEY_HEX

# Run in screen
screen -S skale
cargo run --release -- -w 128 -g 8
# Ctrl+A then D to detach
```

</details>

---

## 📟 Terminal Output & Expected TPS

### What the Engine Prints at Startup

When you run the engine, you will immediately see:

```
2024-01-15T12:00:00.000000Z  INFO skale_tx_engine: === SKALE Base Sepolia Transaction Engine ===
2024-01-15T12:00:00.000001Z  INFO skale_tx_engine: Chain ID: 324705682
2024-01-15T12:00:00.000002Z  INFO skale_tx_engine: Workers: 64
2024-01-15T12:00:00.000003Z  INFO skale_tx_engine: Address pool: 100000
2024-01-15T12:00:00.000004Z  INFO skale_tx_engine: Generator threads: 4
2024-01-15T12:00:00.000005Z  INFO skale_tx_engine: Gas price: 0 wei
2024-01-15T12:00:00.000006Z  INFO skale_tx_engine: RPC endpoints: ["https://base-sepolia-testnet.skalenodes.com/v1/jubilant-horrible-ancha"]
2024-01-15T12:00:00.000007Z  INFO skale_tx_engine: CPU cores: 4
2024-01-15T12:00:00.000008Z  INFO skale_tx_engine: Sender address: 0xAbCd...1234
2024-01-15T12:00:00.000009Z  INFO skale_tx_engine: Sender balance: 9999999999999999 wei
2024-01-15T12:00:00.000010Z  INFO skale_tx_engine: Initial nonce: 0
2024-01-15T12:00:00.000011Z  INFO skale_tx_engine: 🚀 Engine started — sending transactions continuously …
```

### Live Metrics (every 5 seconds)

```
2024-01-15T12:00:05Z  INFO skale_tx_engine: ⛽ Speed/Stats | Sent: 1800 | Failed: 2 | TPS(avg): 360.0 | TPS(peak): 402.5 | Avg RPC: 11.42 ms | Nonce: 1800
2024-01-15T12:00:05Z  INFO skale_tx_engine: ⛽ Gas-like terminal | Gas usage: 37,800,000 / 37,800,000 (100%) | Fee: 0 wei | Gas price: 0 wei (0.000000 Gwei) | Addr pool produced: 500000
2024-01-15T12:00:10Z  INFO skale_tx_engine: ⛽ Speed/Stats | Sent: 3600 | Failed: 3 | TPS(avg): 360.0 | TPS(peak): 402.5 | Avg RPC: 11.38 ms | Nonce: 3600
2024-01-15T12:00:10Z  INFO skale_tx_engine: ⛽ Gas-like terminal | Gas usage: 75,600,000 / 75,600,000 (100%) | Fee: 0 wei | Gas price: 0 wei (0.000000 Gwei) | Addr pool produced: 1000000
```

### What Each Field Means

| Field | Meaning |
|:---|:---|
| `Sent` | Total transactions successfully broadcast |
| `Failed` | Transactions that got an RPC error |
| `TPS(avg)` | Average transactions per second since start |
| `TPS(peak)` | Highest TPS recorded in any single 5-second window |
| `Avg RPC` | Average RPC round-trip latency in milliseconds |
| `Nonce` | Current nonce counter (= total transactions attempted) |
| `Gas usage` | `GAS_LIMIT × Sent` (21 000 per tx on SKALE) |
| `Fee` | Total fees paid (0 on SKALE) |
| `Addr pool produced` | Total addresses generated so far |

### Expected TPS by Machine Size

<div align="center">

| 🖥️ Machine | vCPU | RAM | Recommended Flags | Expected TPS |
|:---:|:---:|:---:|:---|:---:|
| Entry VPS | 1 | 1 GB | `-w 32 -g 2` | 50 – 150 |
| Basic VPS | 2 | 4 GB | `-w 64 -g 4` (default) | 100 – 300 |
| Standard VM | 4 | 8 GB | `-w 128 -g 8 -p 200000` | 300 – 900 |
| Performance VM | 8 | 16 GB | `-w 256 -g 16 -p 500000` | 1 000 – 3 000 |
| High-End Server | 16+ | 32+ GB | `-w 512 -g 32 -p 1000000` | 3 000+ |

</div>

---

## 🔧 Troubleshooting

<details>
<summary><b>❌ "Sender has zero balance — fund the wallet first."</b></summary>

Your wallet has no CREDIT tokens on SKALE Base Sepolia.

1. Visit the [SKALE Faucet](https://www.sfuelstation.com/) or the official SKALE testnet faucet.
2. Connect your wallet and request testnet CREDIT tokens.
3. Restart the engine once funded.

</details>

<details>
<summary><b>❌ "Invalid private key hex" or "Private key must be 32 bytes"</b></summary>

Your private key is malformed. Ensure:
- It is exactly **64 hexadecimal characters** long (representing 32 bytes).
- It does **not** include the `0x` prefix (or if it does, the engine strips it automatically).
- There are no extra spaces or newline characters.

</details>

<details>
<summary><b>❌ "Failed to check balance: RPC request failed"</b></summary>

The engine cannot reach the RPC endpoint.

1. Check your internet/firewall — outbound HTTPS (port 443) must be allowed.
2. Try using a different RPC:
   ```bash
   cargo run --release -- -k YOUR_KEY --rpc-urls "https://base-sepolia-testnet.skalenodes.com/v1/jubilant-horrible-ancha"
   ```
3. Verify the network is up at [SKALE Status](https://skale.space/).

</details>

<details>
<summary><b>❌ High "Failed" count in metrics</b></summary>

This is usually caused by one of the following:

- **Gas price too low (most common on SKALE Base Sepolia)** — The network requires a non-zero
  gas price. Starting from v0.2, the engine auto-fetches the correct gas price from the
  explorer API. If you are running an older build or the API is unreachable, set it explicitly:
  ```bash
  cargo run --release -- -k YOUR_KEY --gas-price 100
  ```
- **Nonce too low** — if restarting after a crash, the engine auto-fetches the correct nonce from the RPC.
- **RPC rate-limiting** — add more RPC endpoints with `--rpc-urls "url1,url2,url3"`.
- **Network congestion** — reduce workers: `-w 32`.

</details>

<details>
<summary><b>⚠️ TPS lower than expected</b></summary>

Try tuning these flags:

```bash
# Increase workers (more concurrent senders)
-w 128

# Increase address generators (ensure pool never empties)
-g 8

# Increase address pool buffer
-p 500000

# Add multiple RPC endpoints
--rpc-urls "https://rpc1.example.com,https://rpc2.example.com"
```

Also check: CPU usage with `top` or `htop`. If CPU is not at 100%, the bottleneck is RPC latency — add more endpoints.

</details>

<details>
<summary><b>❌ cargo: command not found</b></summary>

Rust is installed but not in `PATH`. Run:

```bash
source "$HOME/.cargo/env"
# Or add permanently:
echo 'source "$HOME/.cargo/env"' >> ~/.bashrc
source ~/.bashrc
```

</details>

---

<div align="center">

**Built with ❤️ in Rust · Zero fees on SKALE · Continuous high-throughput transactions**

[![GitHub](https://img.shields.io/badge/GitHub-vinayakkumar9000%2Fvuuu-black?style=flat-square&logo=github)](https://github.com/vinayakkumar9000/vuuu)

</div>
