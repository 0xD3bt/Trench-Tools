# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.1] - 2026-05-10

### Added

- Documentation for current extension popup, Axiom surfaces, token split/consolidate, live PnL controls, and route prewarm behavior.
- Documentation for current execution route coverage, including Raydium CPMM, Raydium LaunchLab, Meteora DBC/DAMM v2, wrapper v3 notes, and route metrics.
- Execution engine: `sellOutputSol` across supported venue families by resolving a target-sized token input, then building swaps with the existing venue compilers (Pump, Raydium AMM v4, Raydium CPMM, Raydium LaunchLab, Bonk, Meteora/Bags paths, trusted stable swaps).
- Shared `sell_target_sizing` logic with estimate-first narrowing, bounded refinement for RPC-heavy quotes, and clearer user-facing errors when the requested SOL output is not reachable from the current balance.
- Wrapper route metadata sets a hard `min_net_output` floor for SOL-output sells so execution cannot settle below the requested SOL target while keeping normal slippage behavior on the quoted instruction path.
- Stronger balance verification when applying target-sized overrides to reduce stale-cache oversell risk on Bonk and Meteora/Bags compilation paths.
- Infer minimum SOL output from Orca Whirlpool venue swap instructions during wrapper compilation where applicable.
- Browser extension: instant-trade preset controls stay in sync after the host page edits and saves preset amounts without a full reload.
- Browser extension: compact TT-only instant-trade mode keeps TT preset controls visible and editable during host preset edit flows, with edits written back to the underlying host inputs.
- Browser extension: manual hardplaced trade panel exposes a matching TT execute control that reads the shared amount field and respects SOL vs percentage sell selection.
- Browser extension: token split and consolidate actions use the same pending and completion toast styling as trade confirmations.
- Balance-gated execution checks for extension trades and token split/consolidate actions before submission.

### Changed

- Refreshed LaunchDeck, environment, and configuration docs for native metadata defaults, Pump/Bonk vanity queues, Bonk USD1 behavior, Bags prepare flow, follow daemon capacity/offset behavior, and newer route-family safety switches.
- Stricter API validation for `sellOutputSol` on routes that require SOL-denominated output or USDC-only stable quoting for certain Meteora stable targets.
- Improved extension trade readiness and prewarm handling before inline trade execution.
- Improved execution latency, event streaming, and live balance cache updates across the extension and execution engine.
- Hardened extension message boundaries between content scripts, background services, and LaunchDeck shell surfaces.

### Fixed

- Meteora/Bags: USD1/USDT restrictions for target SOL-output sizing no longer block ordinary percentage sells; stable sell-output quoting follows the real token-to-stable-to-SOL path.
- Target sizing rejects unreachable SOL output targets instead of submitting a full-balance sell below the request, and prefers the smallest feasible token input when quotes plateau at the target.
- Balance reflection after batch token split/consolidate actions is more reliable.

## [1.1.0] - 2026-05-09

### Added

- Native execution-engine routing and compilation for Raydium CPMM and Raydium LaunchLab SOL routes.
- Expanded Raydium AMM v4, CPMM, LaunchLab, Meteora, Pump, Bonk, Bags, wrapper, and trusted-stable route classification and compile coverage.
- Broader Meteora DBC and DAMM v2 support across launchpad routes instead of only Bags-specific Meteora paths.
- USDC swap route support for compatible stable and launchpad swap flows.
- Route planning and compile metrics for route-family timing, RPC usage, warm-hit behavior, and route diagnostics.
- Runtime diagnostics collection for local services, RPC endpoints, websocket endpoints, auth token state, and startup health checks.
- Extension popup controls for quick-buy amount, active preset selection, wallet group selection, and manual wallet selection.
- Axiom surface support refinements across Pulse, token detail, watchlist, wallet tracker, and token distribution flows.
- Extension background diagnostics storage and richer runtime status/event plumbing.
- Rewards page and claim flow for checking and claiming supported wallet rewards across enabled wallets.
- Token split and consolidate transfer execution through the selected preset provider, endpoint profile, priority fee, and inline tip path for Helius Sender and Hello Moon presets.
- LaunchDeck vanity-key pool support and improved native metadata/upload handling for Pump, Bonk, and Bags launch flows.
- Follow daemon capacity, compile/send concurrency, transport timing, and market-cap trigger controls.
- Bonk USD1 route setup, top-up, sell-to-SOL, and quote-asset execution support.
- Dexscreener and provider/fee/slippage asset coverage in the extension bundle.
- Shared Raydium LaunchLab support crate and updated shared transaction-submit support for low-latency providers.

### Changed

- Bumped the browser extension package and manifest to `1.1.0`.
- Improved warm-route lifecycle handling, prewarm cache matching, route invalidation, and background warm controls per route family.
- Reworked provider routing and submission behavior for Helius Sender, Hello Moon QUIC/bundle paths, priority fees, tips, skip-preflight behavior, and confirmation tracking.
- Improved auto-fee fallback behavior and provider-specific tip handling.
- Refined Pump bonding-curve compile behavior to freshen creator-vault authority at compile time.
- Improved Axiom route extraction, pair/mint handling, UI blockers/overrides, panel behavior, and site-feature toggles.
- Expanded balance streaming, wallet-token caching, trade ledger updates, and live PnL handling.
- Refined LaunchDeck Pump, Bonk, Bags, wrapper, follow, prelaunch setup, and runtime dispatch behavior.
- Updated startup scripts with clearer service readiness output, runtime health checks, tunnel guidance, and optional final diagnostics.
- Simplified root package dependencies and moved the extension into an npm workspace.
- Expanded `.env.example` and `.env.advanced` runtime switches for route families, warm paths, provider behavior, Bonk USD1 tuning, follow capacity, and Pump creator-vault retry control.

### Fixed

- Added one-shot forced-fresh retry for Pump bonding-curve `Custom(2006)` creator-vault seed races.
- Fixed stale warmed Pump creator-vault authority usage by resolving the current creator-vault authority during compile.
- Improved fail-closed behavior for unsupported or non-canonical pair/pool inputs.
- Improved token distribution sends so split/consolidate no longer default to standard RPC with no preset fee policy.
- Improved runtime status, diagnostics, and local auth error reporting in the extension and startup flow.

### Removed

- Removed obsolete extension icon assets that were replaced by the current provider/fee/slippage assets.
- Removed the old extension Axiom smoke script entry.
- Removed unused root package dependencies that are no longer required by the runtime scripts.

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
