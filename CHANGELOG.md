# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Documentation for current extension popup, Axiom surfaces, token split/consolidate, live PnL controls, and route prewarm behavior.
- Documentation for current execution route coverage, including Raydium CPMM, Raydium LaunchLab, Meteora DBC/DAMM v2, wrapper v3 notes, and route metrics.

### Changed

- Refreshed LaunchDeck, environment, and configuration docs for native metadata defaults, Pump/Bonk vanity queues, Bonk USD1 behavior, Bags prepare flow, follow daemon capacity/offset behavior, and newer route-family safety switches.

## [1.0.0] - 2026-04-27

### Added

- Trench Tools umbrella release with three documented pieces: execution engine, browser extension, and LaunchDeck.
- `execution-engine` as the primary local trading host for extension trades, wallets, presets, fee/route resolution, sends, confirmations, PnL, and event streaming.
- Trench Tools browser extension docs for Chrome/Edge developer-mode install, local host pairing, shared auth token setup, presets, wallet groups, supported sites, and update flow.
- Split-host runtime documentation for `execution-engine` on `8788`, `launchdeck-engine` on `8789`, and `launchdeck-follow-daemon` on `8790`.
- Shared bearer token flow documented around `.local/trench-tools/default-engine-token.txt`.
- New quickstart guide for local Windows/Linux setup and first extension connection.
- New extension guide and refreshed VPS, config, environment, provider, architecture, troubleshooting, and security docs.
- Voluntary fee documentation with the simpler `TRENCH_TOOL_FEE` setting: blank/`0.1` by default, `0` to turn off, `0.2` for increased support.

### Changed

- Rebranded user-facing docs from LaunchDeck-first wording to Trench Tools, with LaunchDeck kept as the named launchpad feature.
- Simplified `.env.example` into a practical starter file and moved tuning/override knobs to `.env.advanced`.
- Limited default provider recommendations to `Helius Sender` and `Hello Moon`.
- Moved Standard RPC and Jito Bundle guidance into a deferred provider section pending re-validation.
- Kept Helius Gatekeeper HTTP, Helius standard websocket, Shyft warm RPC, region routing, and priority-fee guidance in the main setup docs.
- Updated VPS setup around the current bootstrap script, shared token path, three local hosts, and SSH tunnel patterns.
- Reorganized LaunchDeck-specific operator docs under `docs/launchdeck/` with redirect stubs at old paths.
- Reorganized lower-level contributor execution policy docs under `docs/internal/` with redirect stubs at old paths.

### Security

- Rewrote `SECURITY.md` around the current local-first model, private keys in `.env`, shared bearer token handling, VPS SSH tunnels, remote-host HTTPS requirements, and third-party trust boundaries.

### Notes

- `j7tracker.io` integration is shipped but currently disabled by default and expected to return soon.
- Terminal (formerly Padre), GMGN, and more terminals are planned on top of the current extension foundation.

## [0.1.0] - 2026-04-13

### Added

- New LaunchDeck browser shell under `ui/launchdeck/` with image assets under `ui/images/`.
- Image-library workflow for uploading, reusing, and organizing images with persisted local metadata.
- `Vamp` import workflow for seeding token metadata and images from an existing mint.
- `Dashboard`-based reporting flow covering persisted `Transactions`, `Launches`, `Jobs`, and `Logs`.
- Runtime status indicators in the UI for warm health, follow-daemon health, and active operator state.
- Rust runtime modules for launchpad execution and warm-state handling: `launchpad_runtime`, `launchpad_warm`, and `warm_manager`.
- Expanded native Rust Bags runtime coverage for launch compilation, quoting, follow actions, market snapshots, import-context detection, and reporting.
- Canonical Bags market/import recovery paths that can detect local Meteora Dynamic Bonding Curve and post-migration Meteora DAMM v2 markets from RPC state.
- Startup launchpad warm flows, warm-target telemetry, and active/idle warm lifecycle handling surfaced back to the UI/runtime layer.
- Warmed lookup-table and launchpad-state handling with local persistence and cached blockhash priming across the host/runtime path.
- Market-cap-based follow actions and the related Helius-first SOL/USD price lookup with HTTP fallback configuration path.
- Expanded operator documentation for VPS provisioning, dependency installation, bootstrap flow, and first-run validation.

### Changed

- Reworked onboarding docs so the default recommendation is a VPS-first setup, including Windows, Linux, and fresh-VPS setup paths.
- Updated setup guidance with region placement advice, explicit dependency baselines, SSH-tunnel usage, and practical VPS notes for testing as well as production.
- Updated README and setup docs to recommend placing the VPS near the provider endpoints and RPCs you actually plan to use, with explicit EU, US, and Asia guidance.
- Updated README and VPS docs to call out the worked Vultr example, referral link, and the practical note that other providers are also fine.
- Updated documentation to reflect the current UI shell, `Dashboard` terminology, runtime warm behavior, and the current launchpad support model.
- Updated follow-daemon docs to explain delayed, confirmed-slot, and market-cap trigger modes plus watcher ownership boundaries.
- Updated environment docs for launchpad warm settings, Bags setup settings, warm probe controls, and market-cap price-source variables.
- Updated wallet configuration docs to clarify that `SOLANA_PRIVATE_KEY<number>` supports open-ended numeric suffixes rather than a fixed wallet cap.
- Updated Bagsapp messaging across runtime and docs to reflect that it is supported when configured.
- Updated the Bags path so the shipped operator flow is explicitly wallet-only in the UI while preserving native follow, fee-split, and automation support.
- Updated Bags setup handling so prelaunch setup, fee-share preparation, and related transport-aware setup orchestration are owned more directly by the Rust runtime.
- Updated the runtime support-state payload so configured Bagsapp now reports as supported instead of unverified.
- Expanded Rust launchpad dispatch and orchestration so Pump, Bonk, and Bags now expose clearer runtime capabilities for compile, warm, quote, follow, and prelaunch-setup behavior.
- Expanded Bonk support to better document and surface the `USD1` quote-asset path, including top-up handling and atomic or split launch/buy behavior where required.
- Cleaned `.gitignore` to remove stale project-specific entries while keeping standard local, build, and editor ignores.

### Fixed

- Restored the live UI image asset set under `ui/images/` so the new shell ships with its referenced marks and branding files.

### Removed

- Previous flat `ui/` app files in favor of the new `ui/launchdeck/` shell layout.
- Old `runtime-bench` package script.
- Old browser-matrix package scripts from the root package manifest.
- Unused `@pump-fun/pump-sdk` dependency from `package.json`.

### Notes

- This entry captures the current repository refresh and documentation pass; future work should be added under `Unreleased` until the next tagged version is cut.
