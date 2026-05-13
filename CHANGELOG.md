# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.3] - 2026-05-13

### Added

- Pump.fun v2 prebond trading support now uses a dedicated wrapper instruction for supported buys and sells, with separate account layouts for the v2 buy and sell paths.
- Pump.fun v2 wrapper metadata now carries quote-fee mode fields for the upcoming token-quote path, laying the client-side ABI groundwork for future USDC prebond pairs once Pump.fun enables them.

### Changed

- LaunchDeck follow sells now route through the Pump.fun v2 wrapper path, and agent-unlocked launches no longer initialize the agent account.
- Legacy client-side Pump wrapper ABI paths were removed from the active Trench Tools code while keeping the deployed wrapper program upgradeable.
- Trench Tools, LaunchDeck, the extension, and the execution engine now report the same `1.1.3` patch version.

### Fixed

- Pump.fun v2 buys no longer wrap trade input into the user's WSOL account, preventing leftover WSOL after successful buys.
- Pump.fun v2 sells now measure native SOL output and collect native SOL fees, preventing WSOL payouts on the dedicated v2 sell path.
- Pump.fun v2 wrapper validation now checks the inner Pump account layout, base token mint, token owner, quote account, and fee-vault requirements more strictly.
- SOL-output sell wrapping now applies configured slippage to minimum net output and rejects malformed slippage settings instead of silently defaulting.
- Bonk launchpad pair routing now preserves quote/config metadata, so direct Axiom pair inputs for USD1 Bonk launchpad coins resolve instead of falling through to unsupported route discovery.
- Axiom token-detail panel positioning and hardpanel action state handling are more stable when switching routes, pairs, and sell units.

## [1.1.2] - 2026-05-12

### Added

- Browser extension: expanded Axiom instant panel support with richer inline controls, TT-only panel behavior, wallet-aware quick actions, and percent-sell affordances.
- Browser extension: added the percent icon asset used by updated Axiom sell controls.
- Browser extension: added Axiom support for `backup.axiom.trade` alongside the primary `axiom.trade` host.
- Execution engine: added fallback mint hints through preview, prewarm, buy, sell, wrapper, and probe paths so trades from pair-centric surfaces can still resolve the intended token mint.

### Changed

- Execution engine: moved confirmed-trade ledger writes off the confirmation hot path, with separate ledger recording status and warnings so confirmed trades are not held up by ledger persistence.
- Shared auto-fee refresh now retries transient Helius/Jito refresh failures quickly instead of leaving a longer stale-fee window.
- LaunchDeck and extension modal layouts were tightened for smaller embedded panels, including auto-sell, sniper, and image library height and width behavior.
- Runtime control now honors `TRENCH_TOOLS_MODE` from the environment or `.env`, and scopes startup diagnostics to the selected `ee`, `ld`, or `both` mode.
- LaunchDeck sniper configuration was simplified by removing stale refresh/reset header actions from the modal.

### Fixed

- Axiom instant panel rendering, compact panel sizing, and popout wallet dropdown styling are more reliable across host page states.
- Axiom wallet selection is hardened so trades use the intended selected wallet, including wallet changes made through the embedded panel.
- Extension popup wallet dropdown layout and generic `SOLANA_PRIVATE_KEY` wallet labels are more compact and readable.
- Pulse and newly indexed pair trades can resolve the token mint when the page provides pair-oriented context first.
- Route discovery no longer poisons caches with overly fast negative results while newly indexed routes are still becoming available.
- Auto-fee availability recovers faster after transient provider failures, reducing spurious "Auto Fee unavailable" states.
- Extension wallet selection payloads are validated more strictly before execution.
- LaunchDeck auto-sell and sniper modal max-height behavior no longer overflows constrained extension views.

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
