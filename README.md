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

## Local Data

The Rust host preserves the existing local storage layout under `.local/launchdeck`:

- `app-config.json`
- `image-library.json`
- `uploads/`
- `send-reports/`

This keeps existing UI settings, uploaded images, and persisted reports compatible through the Rust-only cutover.

## Docs

- `docs/ARCHITECTURE.md`
- `docs/PROVIDERS.md`
- `docs/CONFIG.md`
- `docs/LAUNCHPADS.md`
- `docs/STRATEGIES.md`
- `EXECUTION_PROVIDER_PLAN.md`
