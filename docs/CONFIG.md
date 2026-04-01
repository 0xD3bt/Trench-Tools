# Configuration

This page explains the configuration surface that operators interact with most often: environment variables, persisted UI settings, provider defaults, metadata upload behavior, and the rules the engine enforces regardless of what the UI stores.

`.env.example` is the best starting point for setup. This document explains what the settings actually do.

## Recommended Minimum Setup

Most operators can get started with just these values:

- `SOLANA_RPC_URL`
- `SOLANA_WS_URL`
- `SOLANA_PRIVATE_KEY` or additional `SOLANA_PRIVATE_KEY*`
- `USER_REGION` if you want a default regional provider preference

Optional but common:

- `LAUNCHDECK_METADATA_UPLOAD_PROVIDER=pinata` ([Pinata](https://pinata.cloud/))
- `PINATA_JWT`
- `BAGS_API_KEY`

## Environment Variable Categories

### Core Solana Connectivity

- `SOLANA_RPC_URL`
  Main RPC used for reads, confirmations, and general runtime behavior.
- `SOLANA_WS_URL`
  Websocket endpoint used by realtime watchers. This matters for follow actions, sniper timing, and daemon health.
- `USER_REGION`
  Default region for providers that support endpoint profiles. Supported values are `global`, `us`, `eu`, `west`, and `asia`.

Recommended practice:

- set `USER_REGION` to your nearest region instead of pinning one sender or bundle endpoint
- region fanout is usually faster and more reliable because LaunchDeck can send across that region's endpoint group instead of depending on a single host
- use provider-specific region overrides only when one provider needs a different region than your shared default
- for most operators, use Helius for both `SOLANA_RPC_URL` and `SOLANA_WS_URL` because it is currently the fastest and best-supported overall setup in LaunchDeck

If you omit `SOLANA_WS_URL`, LaunchDeck cannot do its best realtime follow behavior.

- `LAUNCHDECK_ENABLE_HELIUS_TRANSACTION_SUBSCRIBE`
  Enables the enhanced Helius `transactionSubscribe` path for slot, signature, and market watchers when your Helius websocket supports it. Recommended only for Helius dev-tier users; otherwise leave it `false` and LaunchDeck will stay on the standard websocket watcher path.

### Wallet Import

- `SOLANA_PRIVATE_KEY`
- `SOLANA_PRIVATE_KEY2`
- `SOLANA_PRIVATE_KEY3`
- `SOLANA_PRIVATE_KEY4`
- `SOLANA_KEYPAIR_PATH`

Wallet import behavior:

- the UI discovers wallets from `SOLANA_PRIVATE_KEY*`
- each wallet may optionally include a label using `<privatekey>,<label>`
- unlabeled wallets appear as numbered entries
- the selected wallet is persisted in UI state, but the secret stays env-only

### Runtime And Host Control

- `LAUNCHDECK_PORT`
  Main host port. Default `8789`.
- `LAUNCHDECK_ENGINE_AUTH_TOKEN`
  Local engine control token.
- `LAUNCHDECK_FOLLOW_DAEMON_TRANSPORT`
  Follow daemon transport. Default `local-http`.
- `LAUNCHDECK_FOLLOW_DAEMON_URL`
  Explicit daemon base URL.
- `LAUNCHDECK_FOLLOW_DAEMON_PORT`
  Follow daemon port. Default `8790`.
- `LAUNCHDECK_FOLLOW_DAEMON_AUTH_TOKEN`
  Local follow daemon control token.

Follow concurrency and capacity:

- `LAUNCHDECK_FOLLOW_MAX_ACTIVE_JOBS`
- `LAUNCHDECK_FOLLOW_MAX_CONCURRENT_COMPILES`
- `LAUNCHDECK_FOLLOW_MAX_CONCURRENT_SENDS`
- `LAUNCHDECK_FOLLOW_CAPACITY_WAIT_MS`

These matter if you are running several follow jobs at once or if the daemon is rejecting new work because capacity is exhausted.

### Local Persistence Paths

- `LAUNCHDECK_LOCAL_DATA_DIR`
  Overrides the default `.local/launchdeck` root.
- `LAUNCHDECK_SEND_LOG_DIR`
  Overrides the report directory.
- `LAUNCHDECK_ENGINE_RUNTIME_PATH`
  Overrides the main host runtime state file path.
- `LAUNCHDECK_FOLLOW_DAEMON_STATE_PATH`
  Overrides the follow daemon state file path.

Default paths:

- `.local/launchdeck/app-config.json`
- `.local/launchdeck/image-library.json`
- `.local/launchdeck/lookup-tables.json`
- `.local/launchdeck/follow-daemon-state.json`
- `.local/launchdeck/uploads/`
- `.local/launchdeck/send-reports/`
- `.local/engine-runtime.json`

### Provider Routing And Endpoint Overrides

- `USER_REGION_HELIUS_SENDER`
  Provider-specific override for Helius Sender region.
- `USER_REGION_JITO_BUNDLE`
  Provider-specific override for Jito Bundle region.
- `HELIUS_SENDER_ENDPOINT`
  Explicit Sender endpoint override.
- `HELIUS_SENDER_BASE_URL`
  Alternate Sender base URL.
- `JITO_BUNDLE_BASE_URLS`
  Comma-separated Jito bundle base URLs.
- `JITO_SEND_BUNDLE_ENDPOINT`
  Explicit Jito bundle submission endpoint.
- `JITO_BUNDLE_STATUS_ENDPOINT`
  Explicit Jito bundle status endpoint.

Important behavior:

- if you set explicit endpoint overrides, LaunchDeck bypasses normal regional fanout
- if you use profiles instead, LaunchDeck fans out across the selected profile group rather than pinning a single endpoint
- for most operators, `USER_REGION` plus normal profile fanout is the recommended setup

### Metadata Upload

- `LAUNCHDECK_METADATA_UPLOAD_PROVIDER`
  Supported values: `pump-fun`, `pinata`
- `PINATA_JWT`
  Required when `pinata` is selected

Behavior:

- blank provider defaults to `pump-fun`
- `pinata` uploads the image and metadata separately
- when using `pinata`, the app can reuse the image CID across metadata-only edits
- if Pinata upload fails, LaunchDeck falls back to `pump-fun`

### Integration Credentials

- `BAGS_API_KEY`
  Required for Bagsapp usage
- `ASTRALANE_API_KEY`
- `ASTRALANE_REGION`
- `ASTRALANE_ENDPOINT`
- `BLOXROUTE_AUTH_HEADER`
- `HELLOMOON_API_KEY`
- `HELLOMOON_RPC_URL`

Only Bagsapp is part of the current operator-facing UI flow here. The other variables exist for surrounding integration paths and compatibility.

## Persisted UI Configuration

The operator-facing app persists non-secret state in `.local/launchdeck/app-config.json`.

That includes:

- selected launchpad and mode defaults
- active preset and preset editing state
- creation settings
- buy settings
- sell settings
- post-launch strategy defaults
- default automatic dev-sell state

LaunchDeck currently uses three named presets:

- `preset1`
- `preset2`
- `preset3`

Default preset behavior:

- creation provider defaults to `helius-sender`
- buy provider defaults to `helius-sender`
- sell provider defaults to `helius-sender`
- post-launch strategy defaults to `none`
- automatic dev sell defaults to disabled

Legacy provider values in old saved configs are migrated forward so stale IDs like `auto`, `helius`, or `jito` do not remain live.

## Launch Config Shape

The normalized launch config model is centered around these categories:

- `launchpad`
- `mode`
- `quoteAsset`
- `token`
- `agent`
- `tx`
- `feeSharing`
- `creatorFee`
- `bags`
- `execution`
- `devBuy`
- `postLaunch`
- `followLaunch`
- `presets`

Important operator-facing fields:

- `launchpad`
  Current values: `pump`, `bonk`, `bagsapp`
- `mode`
  Must match the chosen launchpad
- `quoteAsset`
  `bonk` supports `sol` and `usd1`; current other launchpads are `sol` only
- `selectedWalletKey`
  The env key of the wallet selected in the UI
- `token.name` and `token.symbol`
  Required and length-limited
- `token.uri`
  Required by normalization before launch can proceed
- `execution.provider`, `execution.buyProvider`, `execution.sellProvider`
  Separate provider controls for creation, buy, and sell flows
- `tx.computeUnitPriceMicroLamports`
- `tx.jitoTipLamports`
- `followLaunch`
  Explicit follow-action configuration

## Engine-Enforced Rules

The engine is stricter than the UI and will reject incompatible combinations.

### Launchpad Rules

- `bonk` accepts only `regular` and `bonkers`
- `bonk` rejects fee-sharing setup
- `bonk` rejects `cashback`
- `bonk` rejects `mayhem`
- `bagsapp` accepts only `bags-2-2`, `bags-025-1`, and `bags-1-025`
- `bagsapp` currently supports only `quoteAsset=sol`
- `bagsapp` rejects Pump agent modes
- `bagsapp` requires creator fee to remain the deployer wallet

### Provider Rules

For `helius-sender`:

- `execution.skipPreflight` must be `true`
- `tx.computeUnitPriceMicroLamports` must be greater than `0`
- `tx.jitoTipLamports` must be at least `200000`

For all providers:

- removed provider values such as `auto` are not valid live config values anymore

### Fee-Sharing And Mode Rules

- `feeSharing.generateLaterSetup` is supported only in Pump `regular`
- if later fee-sharing setup is enabled, fee recipients must be present
- fee-sharing recipients must total `10000` bps
- mode-specific creator-fee behavior is enforced by normalization

### Follow Rules

- `followLaunch.snipes[].postBuySell` is not supported yet and is rejected
- `submitWithLaunch` cannot be combined with `submitDelayMs` or `targetBlockOffset`
- follow constraints and retry budgets are validated
- dev auto-sell supports an exclusive `time` or `market-cap` trigger family
- market-cap timeout is stored in seconds and supports `timeoutAction=stop|sell`

## Provider Defaults And Preset Defaults

The app tries to give operators a sensible baseline without manual tuning.

Current defaults include:

- default provider: `helius-sender`
- default creation tip: `0.01`
- default trade priority fee: `0.009`
- default trade tip: `0.01`
- default trade slippage: `90`
- default quick dev-buy presets: `0.5`, `1`, and `2`

Defaults are only a starting point. The engine may still override behavior depending on provider or launch shape.

## Endpoint Profiles

Endpoint profiles are available only for providers that support them:

- `Helius Sender`
- `Jito Bundle`

Supported profile values:

- `global`
- `us`
- `eu`
- `west`
- `asia`

Resolution order:

1. provider-specific region override such as `USER_REGION_HELIUS_SENDER`
2. shared `USER_REGION`
3. provider default fallback

## Metadata Upload Providers

### `pump-fun`

Use this when you want the default LaunchDeck metadata flow.

- default when no provider is specified
- uploads image and metadata together
- supports URI reuse when the metadata fingerprint is unchanged

### `pinata`

Use this when you want [Pinata](https://pinata.cloud/)-backed uploads.

- requires `PINATA_JWT`
- uploads the image to Pinata
- pins metadata JSON separately
- reuses the image CID across metadata-only edits during the current session
- falls back to `pump-fun` if the Pinata path fails

## Runtime Reports And Storage

Reports are written to `.local/launchdeck/send-reports` by default unless `LAUNCHDECK_SEND_LOG_DIR` overrides the path.

Reports can include:

- requested provider
- resolved provider
- transport type
- endpoint information
- send order
- signature and confirmation state
- benchmark timing data
- follow-job snapshot
- follow-action outcomes
- watcher health
- follow timing profiles

Timing breakdowns separate:

- `total`
- `backendTotal`
- `preRequest`
- compile sub-timings such as `altLoad`, `blockhash`, `global`, `followUpPrep`, and `serialize`
- send sub-timings such as `submit` and `confirm`
