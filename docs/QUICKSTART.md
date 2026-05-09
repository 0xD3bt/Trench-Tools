# Quickstart

Use this guide when you want to run Trench Tools on a local Windows or Linux machine. If you are starting from a fresh VPS, use [VPS_SETUP.md](VPS_SETUP.md) instead; the VPS path can be up in about 5 minutes with the startup script and a Helius Developer tier plan.

If setup feels annoying, it is completely fine to use an AI coding assistant to help. Cursor, Codex, Claude, and similar tools can walk through dependency installs, `.env` editing, startup commands, and log errors. Do not paste real private keys, API keys, or auth tokens into any AI/chat tool.

## What You Are Starting

- `execution engine` (`execution-engine`, `http://127.0.0.1:8788`) handles extension trades, wallets, presets, fee/route resolution, sends, confirmations, and PnL events.
- `Trench Tools extension` injects into supported terminals and talks to the execution engine for trading.
- `LaunchDeck` (`launchdeck-engine`, `http://127.0.0.1:8789`, plus `launchdeck-follow-daemon` on `8790`) handles launchpad deploy/snipe/follow flows.

Run `both` when you want the full stack. Run `ee` when you only need extension trading.

## 1. Install Dependencies

### Windows

Use PowerShell.

Install:

- [Git](https://git-scm.com/downloads)
- [Node.js 20](https://nodejs.org/en/download)
- Rust stable from [rustup](https://rustup.rs/)
- [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/) with the C++ toolchain if Rust complains about a linker/compiler

After first installing Rust, reopen PowerShell.

### Linux

These commands assume Debian or Ubuntu. On another distro, install the equivalent packages.

```bash
sudo apt-get update
sudo apt-get install -y git curl build-essential pkg-config libssl-dev
curl https://sh.rustup.rs -sSf | sh -s -- -y
source "$HOME/.cargo/env"
```

Make sure you have [Node.js 20](https://nodejs.org/en/download).

## 2. Install Project Dependencies

From the repo root:

```bash
npm install
```

## 3. Create `.env`

Copy the starter file:

Windows:

```powershell
Copy-Item .env.example .env
```

Linux:

```bash
cp .env.example .env
```

Fill the practical starter values:

- `SOLANA_PRIVATE_KEY` or the `SOLANA_PRIVATE_KEY*` wallet slots you want to load
- `SOLANA_RPC_URL`
- `SOLANA_WS_URL`
- `USER_REGION`
- `TRENCH_TOOL_FEE` only if you want to turn the voluntary fee off or increase it
- `WARM_RPC_URL` moves compatible warm/cache traffic off the primary RPC
- `HELLOMOON_API_KEY` only if you want Hello Moon
- `BAGS_API_KEY` only if you use Bags launchpad flows
- `PINATA_JWT` only if you set `LAUNCHDECK_METADATA_UPLOAD_PROVIDER=pinata`

Recommended Helius/Shyft examples:

```bash
SOLANA_RPC_URL=https://beta.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
SOLANA_WS_URL=wss://mainnet.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
WARM_RPC_URL=https://rpc.shyft.to?api_key=YOUR_SHYFT_API_KEY
WARM_WS_URL=wss://rpc.shyft.to?api_key=YOUR_SHYFT_API_KEY
```

Put your Helius key immediately after `api-key=`. Put your Shyft key immediately after `api_key=`.

For the full list of advanced options, see [.env.advanced](../.env.advanced) and [ENV_REFERENCE.md](ENV_REFERENCE.md).

## 4. Choose A Run Mode

Choose what you want to run in `.env`:

```bash
# blank also means both
TRENCH_TOOLS_MODE=both
```

Modes:

- `ee` - `execution-engine` only. Use this for extension trading and PnL.
- `ld` - `launchdeck-engine` plus `launchdeck-follow-daemon`. Use this for standalone LaunchDeck.
- `both` - all three processes. This is the normal full setup.

Then use the simple repo-root commands:

```bash
npm start
npm stop
npm restart
```

You can still override the mode for a one-off run.

Windows:

```powershell
.\trench-tools-start.ps1 --mode both
```

Linux:

```bash
./trench-tools-start.sh --mode both
```

The first startup can take a few minutes while Rust builds the binaries. Later starts should be much faster.

### VPS + Local Browser Tunnel

If Trench Tools runs on a VPS but Chrome/Edge runs on your own computer, your browser cannot directly see the VPS loopback ports. `127.0.0.1` in the browser means your computer, not the VPS.

That VPS setup is the recommended real trading path because it keeps services private and lets you run closer to RPC/provider endpoints. Follow [VPS_SETUP.md](VPS_SETUP.md) for the full 5-minute startup-script flow.

Minimum tunnel command:

```bash
ssh -L 8788:127.0.0.1:8788 -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP
```

Keep that SSH session open while using the browser. With the tunnel open, the extension Options page still uses local-looking URLs:

- `Execution host URL` -> `http://127.0.0.1:8788`
- `LaunchDeck host URL` -> `http://127.0.0.1:8789`

Quick checks from the browser machine:

```powershell
Test-NetConnection 127.0.0.1 -Port 8788
Test-NetConnection 127.0.0.1 -Port 8789
```

macOS/Linux:

```bash
curl http://127.0.0.1:8788/api/extension/auth/bootstrap
curl http://127.0.0.1:8789/health
```

## 5. Find The Auth Token

The shared default bearer token is written here after startup:

```text
.local/trench-tools/default-engine-token.txt
```

Use the contents of that file in the extension Options page:

- `Execution host URL` -> `http://127.0.0.1:8788`
- `LaunchDeck host URL` -> `http://127.0.0.1:8789`
- `Shared access token` -> paste the token from `.local/trench-tools/default-engine-token.txt`

The same token authenticates the extension to both `execution-engine` and `launchdeck-engine`.

## 6. Install The Extension

Follow [EXTENSION.md](EXTENSION.md) for the full guide. Short version:

1. Open Chrome or Edge.
2. Open `chrome://extensions` or `edge://extensions`.
3. Enable Developer mode.
4. Get the extension folder by pulling this repo with git, or download the full repository to your PC.
5. Click `Load unpacked`.
6. Select the `extension/trench-tools` folder.
7. Open the extension Options page and fill the host URLs and shared access token.

[EXTENSION.md](EXTENSION.md) also shows a git sparse-checkout flow if you only want to pull `extension/trench-tools`.

## 7. Verify Setup

Before using real size:

- `execution-engine` is reachable on `http://127.0.0.1:8788`
- `launchdeck-engine` is reachable on `http://127.0.0.1:8789` when running `both` or `ld`
- auth token exists at `.local/trench-tools/default-engine-token.txt`
- extension Options -> Global settings shows the expected host connection state
- Axiom shows the enabled Trench Tools surfaces
- the toolbar popup shows host status, active preset, wallet/group selection, and quick-buy amount
- `j7tracker.io` is available in the codebase but currently disabled, so do not expect it to be live until re-enabled

Start with a small test amount and the recommended providers: `Helius Sender` or `Hello Moon`.

## First Extension Use

For the first live test, keep the flow simple:

1. Open the Trench Tools toolbar popup.
2. Confirm it is connected to the host.
3. Choose one execution preset.
4. Choose one wallet or wallet group.
5. Set a small quick-buy amount, or leave it blank and use the panel buttons.
6. Open Axiom and refresh the page after changing Options.

The popup and panel share the same selection. If you change the preset or wallets in the popup, the Axiom controls use that selection too.

## 8. Stop The Runtime

Use the simple command first:

```bash
npm stop
```

Or stop a specific one-off mode directly.

Windows:

```powershell
.\trench-tools-stop.ps1 --mode both
```

Linux:

```bash
./trench-tools-stop.sh --mode both
```

## Logs

If startup fails, check:

Windows:

```text
.local\logs\execution-engine.log
.local\logs\execution-engine.stderr.log
.local\logs\launchdeck-engine.log
.local\logs\launchdeck-engine.stderr.log
.local\logs\launchdeck-follow-daemon.log
.local\logs\launchdeck-follow-daemon.stderr.log
```

Linux:

```text
.local/logs/execution-engine.log
.local/logs/launchdeck-engine.log
.local/logs/launchdeck-follow-daemon.log
```

For common issues, see [TROUBLESHOOTING.md](TROUBLESHOOTING.md).
