# Architecture

`LaunchDeck` is organized as a standalone publishable product.

## Layers

- `ui/`: browser form, preview, validation, and status rendering
- `ui-server.js`: thin HTTP API for UI status, settings, quote, image upload, and run actions
- `rust/launchdeck-engine`: native engine for launch config validation, transaction assembly, simulation, sending, and runtime workers
- `rust/launchdeck-engine/src/bin/launchdeck-cli.rs`: Rust-native CLI for build/simulate/send from config files
- `config/app-config.js`: persisted app defaults, presets, provider metadata, and local config paths
- `providers/`: provider adapter layer and execution routing
- `launchpads/`: launchpad registry and launchpad-specific capability metadata
- `strategies/`: post-launch strategy metadata

## Execution Flow

1. UI reads backend status and defaults.
2. UI submits normalized form data to `/api/run`.
3. `ui-server.js` converts UI form state into raw launch config.
4. `rust/launchdeck-engine` normalizes the request, validates wallet/provider state, and builds native transactions.
5. The Rust engine simulates and/or sends transactions through native RPC / bundle transport.

## Current Boundary

Pump is the verified native build/send path today.

Bonk and Bags are modeled in the product structure and status contract so the app can evolve cleanly, but they still need live validation before they should be treated as fully verified launch flows.
