# Execution Provider Plan

This is the merged in-repo execution design for `LaunchDeck`.

## Product Model

The standalone product supports:

- launchpads: `pump`, `bonk`, `bagsapp`
- providers: `auto`, `helius`, `jito`, `astralane`, `bloxroute`, `hellomoon`
- policies: `fast`, `safe`
- strategies: `none`, `dev-buy`, `snipe-own-launch`, `automatic-dev-sell`

## Architecture

The code is split into:

- `rust/launchdeck-engine`: native execution engine, config normalization, transaction assembly, simulation, send path, and runtime workers
- `rust/launchdeck-engine/src/bin/launchdeck-cli.rs`: native CLI for build/simulate/send from config files
- `config/app-config.js`: persisted app defaults and presets
- `providers/provider-adapters.js`: provider routing and execution-class decisions
- `launchpads/`: launchpad registry and launchpad capability metadata
- `strategies/`: post-launch strategy metadata
- `ui-server.js`: UI/backend bridge

## Provider Intent

- `auto`: default beginner-friendly route selection
- `helius`: default fast single-tx route
- `jito`: primary native bundle route
- `astralane`: advanced low-latency route
- `bloxroute`: modeled but currently unverified
- `hellomoon`: modeled but currently unverified

## Launch Auto vs Buy Auto

The product treats launch-side auto and buy-side auto differently:

- `launchAuto` favors fastest reliable landing
- `buyAuto` favors better MEV/slippage-aware routing where supported

## Launchpad Rules

### Pump

- current verified native execution path
- LaunchDeck launch modes are built by the Rust engine rather than the legacy JS planner

### Bonk

- should use official Raydium SDK surfaces
- prefer Raydium SDK v2 where applicable
- community repos are reference-only, not implementation sources

### Bagsapp

- should use official Bags SDK/docs
- creator BPS must be explicit
- total fee-claimer BPS must equal `10000`
- LUT-aware config creation may be required for larger fee-claimer sets

## Provider Availability

The UI learns provider and launchpad support state from backend status before deploy.

The backend reports:

- `available`
- `verified`
- `supportState`
- `reason`
- bundle/sequential/single support flags

## Current Runtime Notes

- Pump is the verified launch flow in the code today.
- Jito is the main live bundle path in the current runtime.
- Astralane, bloXroute, and Hello Moon are represented in the provider model, but only Jito currently owns the active bundle transport path.
- Bonk and Bags are represented in the launchpad model, but still require live validation before they should be treated as production-verified launch builders.

## Documentation Pointers

- [`README.md`](README.md)
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- [`docs/CONFIG.md`](docs/CONFIG.md)
- [`docs/PROVIDERS.md`](docs/PROVIDERS.md)
- [`docs/LAUNCHPADS.md`](docs/LAUNCHPADS.md)
- [`docs/STRATEGIES.md`](docs/STRATEGIES.md)
