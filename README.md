# LaunchDeck by Trench.tools

LaunchDeck is a self-hosted Solana launch and snipe tool built under the broader `Trench.tools` project.

Instead of paying fees to third-party launch platforms, LaunchDeck lets you run the launcher locally, use your own wallets and provider keys, and customize how launches are built, simulated, and sent. The basic version can be run with a free-tier Helius key, so getting started does not require paid infrastructure.

This is a fresh and actively worked product. As features become functional, tested, and ready, they will be listed here more clearly over time.

LaunchDeck is open-source tooling provided as-is. Running it, configuring it, modifying it, deploying it, or using it in any way is entirely the user's own responsibility. By using this software, you accept full responsibility for your environment, infrastructure, wallets, keys, dependencies, third-party packages, and any outcomes that result from its use. Trench.tools is not responsible for losses, damages, exploits, malicious code, compromised packages, misconfiguration, misuse, downtime, failed transactions, or any other direct or indirect consequences related to the software or its dependencies.

## What LaunchDeck Is

LaunchDeck is built for anyone who wants to:

- run token launches locally
- run launch/snipe workflows without platform fees
- use their own infrastructure and API keys
- avoid relying on third-party launch services
- customize execution settings, wallets, and launch behavior

## Current Runtime Model

LaunchDeck now runs as a Rust-only local host.

The Rust backend serves:

- the browser UI static files
- browser-facing `/api/*` routes
- engine `/engine/*` routes
- local uploads under `/uploads/*`

The current verified native launch runtime is still centered on `pump`, but the UI host, settings persistence, image library, reports browser, and vamp import flow now also live in Rust.

The default local entrypoint is `http://127.0.0.1:8789`.

## Current Pump Coverage

The Rust-native Pump path currently covers:

- `regular`
- `cashback`
- `agent-custom`
- `agent-unlocked`
- `agent-locked`

Pump launch assembly, transaction shaping, reporting, simulation, and send execution now run through the Rust engine rather than the legacy JS compile bridge for these verified launch shapes.

## Run Locally

Primary local entrypoints:

- `npm run bot`
- `npm run ui`

`npm run bot` uses `start-bot.ps1` to launch the Rust host and open the local UI.

`npm run ui` starts the Rust host directly without the helper script.

The host uses `LAUNCHDECK_PORT` as the primary local port. `LAUNCHDECK_ENGINE_PORT` is only kept as a legacy fallback for migration compatibility.

The send layer now exposes explicit provider choices:

- `Helius Sender`
- `Standard RPC`
- `Jito Bundle`

There is no `auto` provider fallback anymore. The selected provider determines transport shape, send requirements, and reporting.

## Provider Rules

- `Helius Sender` is the recommended default.
- `Helius Sender` requires inline tip, inline priority fee, `skipPreflight=true`, and `maxRetries=0`.
- `Standard RPC` uses standard sequential Solana sending for dependent flows and does not use tip.
- `Jito Bundle` keeps bundle-specific tip behavior.

Providers that expose multiple documented endpoint groups can also use an `Endpoint Profile`.

Current profiles:

- `Global`
- `US`
- `EU`
- `West`
- `Asia`

This is currently relevant to:

- `Helius Sender`
- `Jito Bundle`

When an endpoint profile is selected for a supported provider, LaunchDeck now fans out across the endpoints in that profile group rather than single-picking one endpoint.

The UI collects user intent, but the engine is the source of truth for what actually gets applied to each transaction.

Examples:

- `Standard RPC` ignores tip even if a preset still has an old tip value.
- `Helius Sender` hard-forces Sender-compatible send flags.
- `Jito Bundle` creation can accept both tip and priority in the UI, but the engine may intentionally drop creation priority for multi-transaction launch flows where it would only waste money.

## Reporting

LaunchDeck now writes richer execution reports that capture:

- requested provider
- resolved provider
- transport type
- endpoint used
- send order
- signature and confirmation status
- tip and compute-unit settings actually included

Durable send reports are persisted under the local runtime area so launches can be audited after the fact.

The benchmark output now includes both end-to-end and backend-only timings:

- `total`: full click-to-finish time seen by the user
- `backendTotal`: Rust-side execution time after `/api/run` is received
- `preRequest`: browser-side wait before the request is dispatched
- compile breakdowns such as `altLoad`, `blockhash`, `global`, `followUpPrep`, and `serialize`
- send breakdowns such as `submit` and `confirm`

This makes metadata wait, compile latency, and chain confirmation time visible separately in both the main output and persisted reports.

## Launch Optimizations

Recent Pump-focused optimizations in the Rust runtime include:

- measured versioned transaction selection with lookup-table-aware sizing diagnostics
- curated default lookup table coverage for launch and follow-up flows
- local lookup-table persistence and warm-up on page load
- background blockhash refresh with cache age limits
- cached Pump global state for dev-buy quoting and compile-time launch assembly
- immediate metadata pre-upload from the UI once image, name, and ticker are present
- configurable metadata upload provider:
  - `pump-fun` remains the default
  - `pinata` is optional through env config
  - Pinata uploads reuse the uploaded image CID across metadata-only edits so name/symbol/description changes only need metadata JSON repinning
  - when `pinata` is selected and its upload fails, LaunchDeck automatically falls back to `pump-fun`

Pinata can also be tested on its free tier, which is enough for basic LaunchDeck experimentation:

- storage: `1 GB`
- pinned files: `500`
- API rate limit: `60 requests/minute`
- dedicated gateway: `1`
- gateway bandwidth: `10 GB/month`
- gateway requests: `10,000/month`

## Local Data

The Rust host preserves the existing local storage layout under `.local/launchdeck`:

- `app-config.json`
- `image-library.json`
- `lookup-tables.json`
- `uploads/`
- `send-reports/`

This keeps existing UI settings, uploaded images, and persisted reports compatible through the Rust-only cutover.

## Docs

- `docs/ARCHITECTURE.md`
- `docs/PROVIDERS.md`
- `docs/CONFIG.md`
- `docs/LAUNCHPADS.md`
- `docs/STRATEGIES.md`
- `docs/EXECUTION_PROVIDER_PLAN.md`
