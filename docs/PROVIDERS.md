# Providers

## Supported Provider IDs

- `helius-sender`
- `standard-rpc`
- `jito-bundle`

## User-Facing Labels

- `Helius Sender`
- `Standard RPC`
- `Jito Bundle`

## Intent

- `helius-sender`: low-latency Sender transport with strict Sender requirements
- `standard-rpc`: explicit standard Solana RPC path
- `jito-bundle`: bundle-oriented Jito path

## Current Runtime Behavior

The app exposes provider availability and support state through `/api/status` and `/api/settings`.

The Rust host also includes provider and launchpad state in `/api/bootstrap`, so the browser can initialize directly from the same single-process backend that owns execution.

The transport layer resolves:

- requested provider
- resolved provider
- execution class: `single`, `sequential`, or `bundle`
- transport type
- ordering
- endpoint selection
- send requirements

## Transport Rules

### Helius Sender

- recommended/default provider
- requires inline tip
- requires inline compute unit price
- requires `skipPreflight=true`
- requires `maxRetries=0`
- hard-fails instead of silently downgrading
- supports endpoint profiles: `Global`, `US`, `EU`, `West`, `Asia`

`Global` uses the global Sender endpoint.

Regional profiles resolve to the documented regional Sender hosts in ordered groups.

Current send behavior fans out the same signed transaction across the endpoints in the selected profile group.

### Standard RPC

- uses standard sequential Solana sending for dependent flows
- does not use tip
- keeps standard RPC confirmation behavior

### Jito Bundle

- uses bundle send/status endpoints
- keeps separate bundle-compatible tip behavior
- bundle members are treated as an ordered grouped send
- supports endpoint profiles: `Global`, `US`, `EU`, `West`, `Asia`
- current send behavior fans out bundle submission across every endpoint in the selected profile group

## Engine vs UI

The UI lets the user express intent, but the engine is the source of truth for what gets applied.

Examples:

- `Standard RPC` should not ask for or apply tip.
- `Helius Sender` can reject a launch before send if Sender requirements are not satisfied.
- `Jito Bundle` creation may accept both tip and priority in the UI, but the engine may intentionally drop creation priority for multi-transaction creation flows.

This now happens fully inside the Rust host rather than through a separate Node-side bridge layer.

## Legacy Mapping

Older stored provider values are migrated into the current model:

- `auto` -> `helius-sender`
- `helius` -> `helius-sender`
- `jito` -> `jito-bundle`
- `astralane` -> `standard-rpc`
- `bloxroute` -> `standard-rpc`
- `hellomoon` -> `standard-rpc`
