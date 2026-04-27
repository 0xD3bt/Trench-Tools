# Execution Engine

This crate is the standalone runtime home for the new execution-engine product.

## V1 Execution Model

`v1` uses an engine-authoritative execution model.

That means:

- the browser extension submits mint and user-selected execution intent only
- the local `execution-engine` host is the only execution authority
- final route choice, lifecycle classification, normalization, send, confirm, and reporting stay in Rust
- the extension must not grow a parallel direct executor by accident

This keeps execution behavior, wallet access, and reliability policy centralized in one place.

## Local Host Trust Boundary

The local extension host is a privileged loopback-only companion.

Current assumptions for `v1`:

- it binds only to `127.0.0.1`
- it is intended to be consumed only by the packaged extension and local operator tools
- extension UI state is untrusted input until the host validates it
- wallet and preset truth belong to the host, not the extension

As the host contract evolves, authentication/session validation, request identity, and secret-handling policy should be made explicit rather than relying on localhost alone.

Initial scaffold rules:

- keep product runtime logic here, not in `rust/launchdeck-engine`
- copy proven Rust logic only when it is intentionally brought over
- avoid runtime imports from `launchdeck-engine` until neutral shared crates are deliberately extracted later

Current scaffolded extension contract:

- `GET /api/extension/bootstrap`
- `POST /api/extension/resolve-token`
- `POST /api/extension/batch/preview`
- `POST /api/extension/buy`
- `POST /api/extension/sell`
- `GET /api/extension/batch/:batch_id`

Trade submission is intentionally mint-only at the token boundary:

- `buy` / `sell` / `preview` submit `mint`, user-selected wallet target, selected preset id, and any amount override needed for the action
- token-page metadata used for panel state is resolved separately via `resolve-token`
- `resolve-token` may carry `platform`, `surface`, and `url`, but those are UI context fields, not execution authority

## Shared Planner Boundary

The host now owns a shared planner result boundary for venue families.

Current shape:

- `lifecycle`
- `family`
- `canonical_market_key`
- `quote_asset`
- `verification_source`

`Pump` is the first path threaded through this boundary. The planner result is produced before transaction compilation, but execution still uses the current native Pump compiler for now. This preserves behavior while establishing the selector contract needed for `Bonk`, `Bags/Meteora`, and later route-specific execution metadata without reshaping the planner output again.

## Running The Host

The new extension host is separate from the legacy `launchdeck-engine`.

- root npm script: `npm run execution-engine`
- direct cargo: `cargo run --manifest-path ./rust/execution-engine/Cargo.toml`

The binary loads `.env` using `dotenvy`, so starting it from the repository root will pick up `/root/Execution-engine/.env` automatically.
