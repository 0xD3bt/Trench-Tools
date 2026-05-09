# Execution Engine

This crate is the local Rust trading host for Trench Tools extension trading. It owns wallets, presets, route planning, transaction build/sign/send, confirmation handling, the local ledger, live balance/PnL events, token distribution, and the extension API on port `8788`.

## Execution Model

The execution engine is authoritative for trades.

That means:

- the browser extension submits user intent, selected preset/wallet state, and route context
- the local `execution-engine` validates all inputs before use
- final route choice, lifecycle classification, normalization, build, send, confirm, and reporting stay in Rust
- wallet and preset truth belong to the host, not the extension
- the extension must not grow a parallel direct executor

This keeps execution behavior, wallet access, fee handling, and reliability policy centralized in one place.

## Local Host Trust Boundary

The local extension host is a privileged loopback companion.

Current assumptions:

- it binds to local/private operator hosts by default
- it is intended to be consumed by the packaged extension and local operator tools
- browser-facing routes require the shared bearer token, except the auth bootstrap probe
- extension UI state is untrusted input until the host validates it
- private keys stay in `.env` and are loaded by the host

Keep product runtime logic here, not in `rust/launchdeck-engine`. Shared behavior should move to neutral `rust/shared-*` crates when it is genuinely shared.

## Extension API Surface

Current primary extension routes include:

- `GET /api/extension/health`
- `GET /api/extension/runtime-status`
- `GET /api/extension/auth/bootstrap`
- `GET /api/extension/bootstrap`
- `POST /api/extension/wallet-status`
- `POST /api/extension/resolve-token`
- `POST /api/extension/prewarm`
- `POST /api/extension/trade-readiness`
- `POST /api/extension/batch/preview`
- `POST /api/extension/buy`
- `POST /api/extension/sell`
- `GET /api/extension/batch/{batch_id}`
- `POST /api/extension/token-distribution/split`
- `POST /api/extension/token-distribution/consolidate`

The contract is route-aware. Requests may include a mint, pair/pool context, selected preset id, selected wallet or wallet group, and surface/platform context. Pair/pool input is treated as a route hint until the engine verifies it from RPC owner/layout/mint state.

## Route Planner Boundary

The planner resolves a `LifecycleAndCanonicalMarket` style result before native compilation. Important fields include:

- lifecycle
- venue family
- canonical market key
- quote asset
- verification source
- wrapper action
- route-specific runtime bundle where needed

Current native route families include Pump bonding curve, Pump AMM, Raydium AMM v4, Raydium CPMM, Raydium LaunchLab, Bonk, Meteora DBC, Meteora DAMM v2, and trusted stable swaps.

Planning must stay canonical and RPC-first. External APIs can provide metadata or launchpad workflow data, but they must not choose executable markets for extension trade routing.

## Warm And Prewarm

`POST /api/extension/prewarm` lets the extension ask the engine to prepare route context before the user clicks a trade. Prewarm is best-effort:

- warm keys are side-aware where buy and sell routes differ
- repeated warm requests can be deduped through cache/single-flight behavior
- stale or invalidated warm entries fall back to fresh planning
- successful trades can invalidate related warm state so future trades re-check current accounts

Warm/prewarm improves latency, but correctness still depends on final route validation and build-time checks.

## Route Metrics And Probes

Planning and compilation can emit route metrics logs such as:

```text
[execution-engine][route-metrics] phase=plan
[execution-engine][route-metrics] phase=compile
```

Metrics include elapsed time, RPC method counts, and phase timing. The compile probe tooling uses the same planner/compiler path to diagnose route readiness without relying on browser state.

## Token Distribution

Token distribution routes are extension-owned utility actions:

- split redistributes an active token across selected wallets
- consolidate moves token balances into one destination wallet
- distribution uses the selected execution preset for provider/fee behavior
- supported providers are currently `Helius Sender` and `Hello Moon`
- transfer-hook Token-2022 mints are rejected

Distribution is not a swap route; it builds SPL token transfer transactions and submits them through the configured transport plan.

## Wrapper V3

Wrapper v3 is the active wrapper path. Route-mode wrapper compilation, route-specific wrapper actions, shared ALT coverage, and inner-program allowlisting must stay aligned with planner families. When adding a venue family, update route classification, native compilation, wrapper compatibility, and tests together.

## Running The Host

Root npm script:

```bash
npm run execution-engine
```

Direct cargo:

```bash
cargo run --manifest-path ./rust/execution-engine/Cargo.toml
```

The binary loads `.env` using `dotenvy`, so starting it from the repository root will pick up the repository root `.env` automatically.
