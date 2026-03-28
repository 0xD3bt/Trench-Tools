# Architecture

`LaunchDeck` is organized as a standalone publishable product.

## Layers

- `ui/`: browser form, preview, validation, and status rendering
- `rust/launchdeck-engine`: native UI/API host plus launch config validation, transaction assembly, transport planning, simulation, sending, durable send logs, runtime workers, local settings persistence, image library APIs, reports browsing, and vamp import
- `rust/launchdeck-engine/src/bin/launchdeck-cli.rs`: Rust-native CLI for build/simulate/send from config files
- `providers/`: provider adapter layer and execution routing
- `launchpads/`: launchpad registry and launchpad-specific capability metadata

## Execution Flow

1. UI reads backend status and defaults.
2. UI submits normalized form data to `/api/run`.
3. `rust/launchdeck-engine` converts UI form state into raw launch config and normalizes the request.
4. The Rust engine builds a provider-aware transport plan.
5. The Rust engine validates provider requirements, builds native transactions, and applies provider-specific tx shaping.
6. The Rust engine simulates and/or sends transactions through the selected transport.
7. The Rust engine persists execution/send report output for later auditing.

## Transport Layer

The runtime now uses explicit transport types instead of implicit provider fallback:

- `helius-sender`
- `standard-rpc-sequential`
- `jito-bundle`

The selected transport controls:

- whether tip is inline or separate
- whether priority fee is required, allowed, or intentionally dropped
- `skipPreflight`
- `maxRetries`
- endpoint selection
- endpoint profile selection where the provider supports multiple documented endpoint groups
- sequential vs bundle send semantics

## Engine-Owned Decisions

The UI collects requested settings, but the engine owns final transaction shaping.

Important examples:

- `Helius Sender` requires inline tip and compute unit price, and hard-fails if the request is incompatible.
- `Standard RPC` remains sequential for dependent launch/follow-up flows.
- `Jito Bundle` creation may accept both tip and priority in the UI, but the engine can intentionally drop creation priority for multi-transaction launch flows where it is not useful.

## Current Boundary

Pump is the verified native build/send path today.

Bonk and Bags are modeled in the product structure and status contract so the app can evolve cleanly, but they still need live validation before they should be treated as fully verified launch flows.

## Runtime Shape

LaunchDeck now runs as a single Rust process on the UI port.

- `GET /`, `/app.js`, `/styles.css`, root image assets, and `/uploads/*` are served directly by the Rust host.
- Browser-facing `/api/*` routes call Rust internals directly rather than proxying through a Node bridge.
- `/engine/*` routes remain available on the same process for engine-oriented tooling and compatibility.
- Existing `.local/launchdeck` files remain the source of persistent UI settings, uploads, and send reports.
