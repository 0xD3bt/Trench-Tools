# Environment Reference

Use [.env.example](../.env.example) for first setup. Use [.env.advanced](../.env.advanced) only when you intentionally need tuning or overrides.

Never commit `.env`. Examples below use placeholders only.

## Starter Variables

These are the values most users may need.

- `TRENCH_TOOLS_MODE` - launcher mode. Blank defaults to `both`. Use `ee` for execution engine only, `ld` for LaunchDeck only, `both` for all services.
- `TRENCH_TOOL_FEE` - voluntary Trench Tools fee. Blank or `0.1` = `0.1%`, `0` = off, `0.2` = increased support at `0.2%`.
- `SOLANA_PRIVATE_KEY`, `SOLANA_PRIVATE_KEY2`, ... - wallet private-key slots. Optional label format: `YOUR_PRIVATE_KEY,Main Wallet`.
- `SOLANA_RPC_URL` - primary HTTP RPC for reads, confirmations, and general runtime behavior. Recommended Helius Gatekeeper format: `https://beta.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY`.
- `SOLANA_WS_URL` - primary websocket for live watchers. Recommended Helius format: `wss://mainnet.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY`.
- `USER_REGION` - default region profile for region-aware providers. Groups: `global`, `us`, `eu`, `asia`. Metros: `slc`, `ewr`, `lon`, `fra`, `ams`, `sg`, `tyo`.
- `WARM_RPC_URL` - best-effort warm/cache RPC for compatible background reads. When set, those reads can move off `SOLANA_RPC_URL`. Execution, confirmations, visible balances, and quotes stay on `SOLANA_RPC_URL`.
- `WARM_WS_URL` - best-effort websocket for warm probes or non-authoritative observers. When set, those probes can move off `SOLANA_WS_URL`. Live visible subscriptions and confirmations stay on `SOLANA_WS_URL`.
- `HELLOMOON_API_KEY` - optional Hello Moon Lunar Lander API key.
- `BAGS_API_KEY` - optional Bags API key for Bags launchpad flows.
- `LAUNCHDECK_METADATA_UPLOAD_PROVIDER` - blank/default uses the launchpad's native metadata flow. Set `pinata` to use Pinata for Pump/Bonk.
- `PINATA_JWT` - required only when metadata provider is `pinata`.

For per-launchpad metadata/IPFS behavior and local Pump/Bonk vanity mint queue formatting, see [launchdeck/METADATA_AND_VANITY.md](launchdeck/METADATA_AND_VANITY.md).

## Launcher / Host Overrides

Most users should leave these blank because the launcher sets safe defaults.

- `TRENCH_TOOLS_DATA_ROOT` - shared data root. Launcher default: `.local/trench-tools`.
- `TRENCH_TOOLS_TERMINALS` - Windows launcher log windows. Default: `none`. Use `logs` only when you want live log windows.
- `EXECUTION_ENGINE_PORT` - execution engine port. Default: `8788`.
- `LAUNCHDECK_PORT` - LaunchDeck host port. Default: `8789`.
- `LAUNCHDECK_FOLLOW_DAEMON_PORT` - follow daemon port. Default: `8790`.
- `LOG_DIR` - process log directory. Default: `.local/logs`.

## Core RPC / Helius Overrides

- `HELIUS_RPC_URL` - optional Helius HTTP override for priority-fee estimates. Blank uses a Helius `SOLANA_RPC_URL` when detected.
- `HELIUS_WS_URL` - optional Helius websocket override for transactionSubscribe watchers. Blank uses a Helius `SOLANA_WS_URL` when detected.
- `LAUNCHDECK_EXTRA_STANDARD_RPC_SEND_URLS` - optional submit-only fanout endpoints for the deferred Standard RPC provider path. Not recommended in default setup while Standard RPC is pending re-validation.

## Region / Provider Routing

- `USER_REGION_HELIUS_SENDER` - Helius Sender-specific region override. Blank uses `USER_REGION`.
- `USER_REGION_HELLOMOON` - Hello Moon-specific region override. Blank uses `USER_REGION`.
- `USER_REGION_JITO_BUNDLE` - Jito-specific region override. Deferred provider path.
- `HELIUS_SENDER_ENDPOINT` - explicit Helius Sender endpoint. Blank keeps region fanout.
- `HELIUS_SENDER_BASE_URL` - alternate Helius Sender base URL. Usually blank.
- `HELLOMOON_QUIC_ENDPOINT` - explicit Hello Moon QUIC endpoint in `host:port` format. Blank keeps region routing.
- `HELLOMOON_MEV_PROTECT` - enables Hello Moon QUIC MEV protection when set to `1`, `true`, `yes`, or `on`.
- `JITO_BUNDLE_BASE_URLS`, `JITO_SEND_BUNDLE_ENDPOINT`, `JITO_BUNDLE_STATUS_ENDPOINT` - Jito bundle endpoints. Deferred provider path; not recommended in setup guides until re-tested.

## Provider / Launchpad Integrations

- `BAGS_API_BASE_URL` - Bags API base URL override. Usually blank.
- `LAUNCHDECK_BAGS_HELPER_BLOCKHASH_FROM_RUST` - pass Rust-cached blockhash values into Bags helper requests. Blank defaults to enabled.
- `LAUNCHDECK_BAGS_SETUP_JITO_TIP_MIN_LAMPORTS` - minimum Bags setup Jito tip. Blank uses code default.
- `LAUNCHDECK_BAGS_SETUP_JITO_TIP_CAP_LAMPORTS` - maximum Bags setup Jito tip. Blank uses code default.
- `LAUNCHDECK_BAGS_SETUP_CONFIRM_TIMEOUT_SECS` - Bags setup confirmation timeout. Blank uses code default.
- `LAUNCHDECK_BAGS_SETUP_GATE_COMMITMENT` - commitment gate before Bags final launch build. Supported: `processed`, `confirmed`, `finalized`.
- `LAUNCHDECK_BONK_STARTUP_WARM_BACKEND` - Bonk startup warm backend override. Advanced/debug only; blank uses the current Rust-native default.
- `LAUNCHDECK_BAGSAPP_STARTUP_WARM_BACKEND` - Bagsapp startup warm backend override. Advanced/debug only; blank uses the current Rust-native default.
- `BONK_USD1_ROUTE_SETUP_CACHE_TTL_MS` - Bonk SOL/USD1 route setup cache TTL. Advanced tuning only.
- `BONK_USD1_SEARCH_TOLERANCE_BPS` - Bonk USD1 route input search tolerance. Advanced tuning only.
- `BONK_USD1_SEARCH_MIN_LAMPORTS` - Bonk USD1 route input search lower bound. Advanced tuning only.
- `BONK_USD1_MIN_REMAINING_SOL` - minimum remaining SOL cushion for Bonk USD1 route work. Blank uses code default.
- `BONK_USD1_MAX_INPUT_SEARCH_ITERATIONS` - maximum Bonk USD1 input search iterations. Advanced tuning only.

## Warmup / Keep-warm

- `LAUNCHDECK_ENABLE_STARTUP_WARM` - one-shot startup warm. Blank/default enabled.
- `LAUNCHDECK_ENABLE_CONTINUOUS_WARM` - active keep-warm. Blank/default enabled.
- `LAUNCHDECK_ENABLE_IDLE_WARM_SUSPEND` - suspend warm loops while idle. Blank/default enabled.
- `TRADING_RESOURCE_MODE` - blank keeps credit-saving idle suspension. `always-on` keeps balance streams and provider warm loops active while idle.
- `LAUNCHDECK_IDLE_WARM_TIMEOUT_MS` - idle timeout before warm suspension. Blank uses code default.
- `LAUNCHDECK_CONTINUOUS_WARM_INTERVAL_MS` - active keep-warm cadence. Blank uses code default.
- `LAUNCHDECK_CONTINUOUS_WARM_PASS_TIMEOUT_MS` - timeout for one warm pass. Blank uses code default.
- `LAUNCHDECK_WARM_PROBE_TIMEOUT_MS` - timeout for one warm probe. Blank uses code default.
- `LAUNCHDECK_DISABLE_STARTUP_WARM` - legacy fallback to disable startup warm. Prefer `LAUNCHDECK_ENABLE_STARTUP_WARM=false`.
- `LAUNCHDECK_LAUNCHPAD_WARM_CONTEXT` - build warm context during launch requests. Blank/default enabled.
- `LAUNCHDECK_LAUNCHPAD_PARALLEL_WARM_FETCH` - opt-in parallel warm fetch. Blank/default disabled.
- `LAUNCHDECK_LAUNCHPAD_WARM_MAX_PARALLEL_FETCH` - parallel warm fetch cap. Blank uses code default.
- `EXECUTION_ENGINE_WARM_PUMP` - Pump-family execution-engine warm toggle. Operational safety switch; normally blank.
- `EXECUTION_ENGINE_WARM_RAYDIUM_AMM_V4` - Raydium AMM v4 execution-engine warm toggle. Operational safety switch; normally blank.
- `EXECUTION_ENGINE_WARM_RAYDIUM_CPMM` - Raydium CPMM execution-engine warm toggle. Operational safety switch; normally blank.
- `EXECUTION_ENGINE_WARM_RAYDIUM_LAUNCHLAB` - Raydium LaunchLab execution-engine warm toggle. Operational safety switch; normally blank.
- `EXECUTION_ENGINE_WARM_BONK` - Bonk execution-engine warm toggle. Operational safety switch; normally blank.
- `EXECUTION_ENGINE_WARM_BAGS` - Meteora DBC/DAMM/Bags-family execution-engine warm toggle. Operational safety switch; normally blank.
- `EXECUTION_ENGINE_WARM_TRUSTED_STABLE_SWAP` - trusted stable route warm toggle. Operational safety switch; normally blank.

## Block Height / Follow Timing

- `LAUNCHDECK_BLOCK_HEIGHT_CACHE_TTL_MS` - shared block-height cache TTL. Blank uses code default.
- `LAUNCHDECK_BLOCK_HEIGHT_SAMPLE_MAX_AGE_MS` - max age for sampled block-height diagnostics. Blank uses code default.
- `LAUNCHDECK_FOLLOW_OFFSET_POLL_INTERVAL_MS` - follow offset worker cadence. Blank uses code default.
- `LAUNCHDECK_ENABLE_APPROXIMATE_FOLLOW_OFFSET_TIMER` - use local timer approximation for follow offsets. Disabled by default.
- `LAUNCHDECK_FOLLOW_BLOCK_HEIGHT_REFRESH_MS` - legacy/no-op after offset-worker migration. Safe to delete from local envs.

## Reporting / Traffic / UI

- `LAUNCHDECK_BENCHMARK_MODE` - report timing detail. Supported: `off`, `light`, `full`. Blank defaults to `full`.
- `LAUNCHDECK_TRACK_SEND_BLOCK_HEIGHT` - report send/confirm block heights when supported. Usually blank.
- `LAUNCHDECK_RPC_TRAFFIC_METER` - UI rolling RPC traffic counter. Blank/default enabled; set false-like values to disable.
- `LAUNCHDECK_WALLET_STATUS_REFRESH_INTERVAL_MS` - frontend wallet-balance refresh cadence. Blank uses code default.

## Auto Fee Estimates

These are advanced tuning knobs. Most users should keep them blank and let defaults apply.

- `AUTO_FEE_BUFFER_PERCENT` - extra buffer added to live estimates. Default: `10`.
- `HELIUS_PRIORITY_LEVEL` - Helius priority level. Default: `high`. Supported: `recommended`, `none`, `low`, `medium`, `high`, `veryHigh`, `unsafeMax`.
- `HELIUS_PRIORITY_REFRESH_INTERVAL_MS` - Helius estimate refresh interval. Default: `30000`.
- `HELIUS_PRIORITY_STALE_MS` - Helius estimate stale window. Default: `45000`.
- `JITO_TIP_PERCENTILE` - Jito tip percentile. Default: `p99`. Supported: `p25`, `p50`, `p75`, `p95`, `p99`.
- `JITO_TIP_REFRESH_INTERVAL_MS` - Jito websocket reconnect/refresh cadence. Default: `2000`.
- `JITO_TIP_STALE_MS` - Jito tip stale window. Default: `45000`.

Legacy aliases still accepted:

- `LAUNCHDECK_AUTO_FEE_HELIUS_PRIORITY_LEVEL`
- `LAUNCHDECK_HELIUS_PRIORITY_REFRESH_INTERVAL_MS`
- `LAUNCHDECK_AUTO_FEE_JITO_TIP_PERCENTILE`
- `TRENCH_AUTO_FEE_BUFFER_PERCENT`

Use the shorter names for new installs.

## Voluntary Fee

- `TRENCH_TOOL_FEE` - user-facing voluntary fee. Blank or `0.1` = `0.1%`, `0` = off, `0.2` = increased support at `0.2%`.
- `EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS` - legacy alias for existing installs. Values are basis points: blank or `10` = `0.1%`, `0` = off, `20` = `0.2%`. Prefer `TRENCH_TOOL_FEE` for new installs.
- `ALT_COVERAGE_DIAGNOSTICS` - emit ALT coverage diagnostics to logs. Debugging only.

## Execution-engine Rollout / Safety

- `EXECUTION_ENGINE_ENABLE_PUMP_NATIVE` - enable/disable native Pump family path. Operational safety switch.
- `EXECUTION_ENGINE_ENABLE_RAYDIUM_AMM_V4_NATIVE` - enable/disable native Raydium AMM v4 path. Operational safety switch.
- `EXECUTION_ENGINE_ENABLE_RAYDIUM_CPMM_NATIVE` - enable/disable native Raydium CPMM path. Operational safety switch.
- `EXECUTION_ENGINE_ENABLE_RAYDIUM_LAUNCHLAB_NATIVE` - enable/disable native Raydium LaunchLab path. Operational safety switch.
- `EXECUTION_ENGINE_ENABLE_BONK_NATIVE` - enable/disable native Bonk family path. Operational safety switch.
- `EXECUTION_ENGINE_ENABLE_METEORA_NATIVE` - enable/disable native Meteora family path. Operational safety switch.
- `EXECUTION_ENGINE_ENABLE_TRUSTED_STABLE_SWAP` - enable/disable trusted stable swap route path. Operational safety switch.
- `EXECUTION_ENGINE_ALLOW_NON_CANONICAL_POOL_TRADES` - allow pinned non-canonical Pump AMM pool trades where the source path supports it. Current safe behavior is fail-closed; keep this off unless source and docs both confirm the intended route.
- `EXECUTION_ENGINE_ENABLE_PUMP_CREATOR_VAULT_AUTO_RETRY` - Pump bonding-curve creator-vault retry path. Blank/default enabled.

## Follow Daemon

- `LAUNCHDECK_FOLLOW_DAEMON_TRANSPORT` - follow daemon transport. Default is local HTTP.
- `LAUNCHDECK_FOLLOW_DAEMON_URL` - explicit follow daemon URL. Blank uses local default.
- `LAUNCHDECK_FOLLOW_DAEMON_PORT` - follow daemon port. Default: `8790`.
- `LAUNCHDECK_FOLLOW_MAX_ACTIVE_JOBS` - max simultaneous follow jobs. Blank/0 means uncapped.
- `LAUNCHDECK_FOLLOW_MAX_CONCURRENT_COMPILES` - max concurrent follow compiles. Blank/0 means uncapped.
- `LAUNCHDECK_FOLLOW_MAX_CONCURRENT_SENDS` - max concurrent follow sends. Blank/0 means uncapped.
- `LAUNCHDECK_FOLLOW_CAPACITY_WAIT_MS` - wait time for daemon capacity. Only matters when caps are set.
- `LAUNCHDECK_SOL_USD_HTTP_PRICE_URL` - follow-daemon SOL/USD HTTP fallback price URL for market-cap-trigger paths.
- `LAUNCHDECK_ENABLE_PUMP_BUY_CREATOR_VAULT_AUTO_RETRY` - Pump buy creator-vault retry path. Blank/default enabled.
- `LAUNCHDECK_ENABLE_PUMP_SELL_CREATOR_VAULT_AUTO_RETRY` - Pump sell creator-vault retry path. Blank/default enabled.

## Compute / Slippage Overrides

- `TRUSTED_STABLE_SWAP_MAX_SLIPPAGE_BPS` - stable-swap safety cap. Advanced risk setting.
- `LAUNCHDECK_LAUNCH_COMPUTE_UNIT_LIMIT` - launch transaction compute unit override.
- `LAUNCHDECK_AGENT_SETUP_COMPUTE_UNIT_LIMIT` - setup transaction compute unit override.
- `LAUNCHDECK_FOLLOW_UP_COMPUTE_UNIT_LIMIT` - follow transaction compute unit override.
- `LAUNCHDECK_SNIPER_BUY_COMPUTE_UNIT_LIMIT` - sniper-buy compute unit override.
- `LAUNCHDECK_DEV_AUTO_SELL_COMPUTE_UNIT_LIMIT` - dev-auto-sell compute unit override.
- `LAUNCHDECK_LAUNCH_USD1_TOPUP_COMPUTE_UNIT_LIMIT` - USD1 top-up compute unit override.
- `LAUNCHDECK_BONK_USD1_SELL_TO_SOL_COMPUTE_UNIT_LIMIT` - Bonk USD1 sell-to-SOL compute unit override.

## Local State / Token Overrides

The launcher points state at `.local/trench-tools`. Only override these when running binaries manually or when you know why you need custom paths.

- `LAUNCHDECK_LOCAL_DATA_DIR` - base LaunchDeck local state directory.
- `LAUNCHDECK_SEND_LOG_DIR` - launch report directory.
- `LAUNCHDECK_ENGINE_RUNTIME_PATH` - engine runtime state file path.
- `LAUNCHDECK_FOLLOW_DAEMON_STATE_PATH` - follow daemon state file path.
- `LAUNCHDECK_EXECUTION_ENGINE_TOKEN` - direct shared bearer token override. Avoid for normal use.
- `LAUNCHDECK_EXECUTION_ENGINE_TOKEN_FILE` - shared bearer token file override. Default file: `.local/trench-tools/default-engine-token.txt`.

## Developer Script Compatibility

- `RPC_URL` - compatibility alias for standalone utility scripts. Main runtime should use `SOLANA_RPC_URL`.
