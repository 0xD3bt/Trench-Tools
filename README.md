# LaunchDeck by Trench.tools

LaunchDeck is a self-hosted Solana launch and snipe tool built under the broader `Trench.tools` project.

Instead of paying fees to third-party launch platforms, LaunchDeck lets you run the launcher locally, use your own wallets and provider keys, and customize how launches are built, simulated, and sent. The basic version can be run with a free-tier Helius key, so getting started does not require paid infrastructure.

## What LaunchDeck Is

LaunchDeck is built for anyone who wants to:

- run token launches locally
- run launch/snipe workflows without platform fees
- use their own infrastructure and API keys
- avoid relying on third-party launch services
- customize execution settings, wallets, and launch behavior

## Current Features

- local browser UI for launch configuration
- wallet selection from `SOLANA_PRIVATE_KEY`, `SOLANA_PRIVATE_KEY2`, and more
- token metadata setup, image library, presets, and saved settings
- Rust-native build, simulate, and deploy launch flows
- Rust-native CLI and diagnostics for manual launch operations and inspection
- execution provider controls for launch and buy behavior
- support for multiple launchpad/provider models in one product

## Runtime Boundary

LaunchDeck now uses a Rust-native execution core for launch planning, transaction assembly, simulation, sending, and CLI diagnostics.

The JavaScript side remains for the local UI and thin backend support only:

- `ui-server.js` handles browser/API requests, uploads, and settings
- `keypair.js` and `rpc.js` remain UI/backend helper modules
- `rust/launchdeck-engine` owns the launch engine and CLI path

## Install

```bash
npm install
```

## Environment Setup

Copy `.env.example` to `.env` and fill in the values you want to use.

For the basic version, a free-tier Helius key is enough to get started and run LaunchDeck locally.

Most important variables:

- `SOLANA_PRIVATE_KEY`
- `SOLANA_PRIVATE_KEY2`
- `LAUNCHDECK_PORT`
- `HELIUS_RPC_URL` or `HELIUS_API_KEY`
- `ASTRALANE_API_KEY`
- `BAGS_API_KEY`
- `BLOXROUTE_AUTH_HEADER`
- `HELLOMOON_API_KEY`
- `HELLOMOON_RPC_URL`

Optional advanced Jito overrides:

- `JITO_BUNDLE_BASE_URLS`
- `JITO_SEND_BUNDLE_ENDPOINT`
- `JITO_BUNDLE_STATUS_ENDPOINT`

## Run LaunchDeck

Start the local UI:

```bash
npm run bot
```

This launcher stops any older LaunchDeck UI process first, then starts a fresh one.

Open:

`[http://127.0.0.1:8789](http://127.0.0.1:8789)`

If you change `LAUNCHDECK_PORT`, use that port instead.

## Available Scripts

```bash
npm run bot
npm run ui
npm run build-launch
npm run simulate-launch
npm run send-launch
npm run analyze
npm run trace-agent
```

Script roles:

- `build-launch`, `simulate-launch`, `send-launch`: Rust-native CLI launch commands
- `analyze`: Rust-native transaction inspection helper
- `trace-agent`: Rust-native agent escrow/activity inspection helper

## Local Data

LaunchDeck stores local runtime data in:

- `.local/launchdeck/uploads`
- `.local/launchdeck/app-config.json`
- `.local/*.json`

## Documentation

- `[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)`
- `[docs/CONFIG.md](docs/CONFIG.md)`
- `[docs/PROVIDERS.md](docs/PROVIDERS.md)`
- `[docs/LAUNCHPADS.md](docs/LAUNCHPADS.md)`
- `[docs/STRATEGIES.md](docs/STRATEGIES.md)`

