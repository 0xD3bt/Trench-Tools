# Configuration

This guide explains the recommended Trench Tools setup and the defaults most users should leave alone. Use [.env.example](../.env.example) for first setup and [.env.advanced](../.env.advanced) only when you intentionally need more knobs.

## The Three Pieces

- `execution engine` (`execution-engine`, `8788`) owns trades, wallets, presets, fee/route resolution, sends, confirmations, PnL, and the extension event stream.
- `Trench Tools extension` talks to the execution engine for trades and to LaunchDeck for launchpad screens.
- `LaunchDeck` (`launchdeck-engine`, `8789`, plus `launchdeck-follow-daemon`, `8790`) owns deploy, snipe, dev-buy, dev-sell, follow, and reports.

## Recommended Stack

For most operators today:

- run on a VPS close to your provider endpoints and RPCs
- use [Helius Developer tier](https://www.helius.dev/pricing), about $50/month, or better for primary infrastructure
- `SOLANA_RPC_URL`: Helius Gatekeeper HTTP
- `SOLANA_WS_URL`: Helius standard websocket
- `WARM_RPC_URL`: separate [Shyft](https://shyft.to/) RPC for compatible warm/cache traffic off the main Helius budget
- provider: `Helius Sender` or `Hello Moon`

Examples:

```bash
SOLANA_RPC_URL=https://beta.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
SOLANA_WS_URL=wss://mainnet.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
WARM_RPC_URL=https://rpc.shyft.to?api_key=YOUR_SHYFT_API_KEY
WARM_WS_URL=wss://rpc.shyft.to?api_key=YOUR_SHYFT_API_KEY
```

Why this split:

- Helius Gatekeeper HTTP has benchmarked best for the main HTTP/read path.
- Helius standard websocket has benchmarked best for watcher websocket subscriptions.
- Shyft provides low-priority warm RPC capacity for warm/cache/block-height traffic.

Benchmark your own setup from the exact machine and region you use. Do not assume shared latency numbers will match your VPS, provider tier, or route.

## Starter `.env`

Most users only need:

```bash
TRENCH_TOOL_FEE=
TRENCH_TOOLS_MODE=
SOLANA_PRIVATE_KEY=
SOLANA_RPC_URL=
SOLANA_WS_URL=
USER_REGION=
WARM_RPC_URL=
WARM_WS_URL=
HELLOMOON_API_KEY=
BAGS_API_KEY=
LAUNCHDECK_METADATA_UPLOAD_PROVIDER=
PINATA_JWT=
```

Fill only what you need. Leave advanced defaults alone until the runtime is healthy.

## Run Mode

`TRENCH_TOOLS_MODE` is optional. Blank defaults to `both`.

- `ee`: start `execution-engine` only. Use this for extension trading and PnL.
- `ld`: start `launchdeck-engine` plus `launchdeck-follow-daemon`. Use this for standalone LaunchDeck.
- `both`: start all three. This is the normal full setup.

After setting the value, use the simple repo-root commands:

```bash
npm start
npm stop
npm restart
```

You can still override the mode for a one-off run:

```bash
./trench-tools-start.sh --mode both
```

or on Windows:

```powershell
.\trench-tools-start.ps1 --mode both
```

## Wallets

Wallet slots are open-ended:

```bash
SOLANA_PRIVATE_KEY=YOUR_PRIVATE_KEY,Main Wallet
SOLANA_PRIVATE_KEY2=YOUR_PRIVATE_KEY,Sniper 2
SOLANA_PRIVATE_KEY3=YOUR_PRIVATE_KEY,Sniper 3
```

The comma label is optional. Untagged wallets show by slot number.

Do not share `.env`. Do not paste private keys into screenshots, public issues, Discord, or support messages.

## Region Routing

`USER_REGION` is the shared default profile for region-aware providers.

Groups:

- `global`
- `us`
- `eu`
- `asia`

Metros:

- `slc`
- `ewr`
- `lon`
- `fra`
- `ams`
- `sg`
- `tyo`

Practical guidance:

- EU: use `eu`, `fra`, or `ams`; place the VPS in Frankfurt or Amsterdam.
- US: use `ewr` or `slc` when you want to pin closer to one side; `us` fans out across a wide region.
- Asia: use `sg` or `tyo` when you know which side you are closer to; `asia` spans far-apart endpoints.

Helius Sender supports exact metro routing where those metros exist. Hello Moon maps unsupported metros to the closest endpoints it exposes. For example, Hello Moon does not expose every Helius metro one-to-one.

Provider-specific overrides (`USER_REGION_HELIUS_SENDER`, `USER_REGION_HELLOMOON`, `USER_REGION_JITO_BUNDLE`) live in [.env.advanced](../.env.advanced). Most users should not set them.

## Warmup And Keep-warm

Trench Tools separates:

- execution transport
- read/confirm RPC
- watcher websocket
- warm/cache/block-height RPC

In practice:

- `Helius Sender` or `Hello Moon` handle the low-latency send path.
- `SOLANA_RPC_URL` handles reads, confirmations, and general runtime RPC behavior.
- `SOLANA_WS_URL` handles realtime watchers.
- `WARM_RPC_URL` handles compatible best-effort startup/keep-warm probes and low-priority sampled state reads.
- `WARM_WS_URL` handles optional warm websocket probes or non-authoritative observers only.

Default behavior:

- Blank `WARM_RPC_URL` or `WARM_WS_URL` keeps warm behavior on the primary Helius endpoints.
- Configured warm endpoints add optional cost-saving capacity. Failures or rate limits trigger cooldown; user-visible/execution paths stay on primary Helius endpoints.
- Env-backed warm endpoint changes apply after local services restart.

- startup warm runs once when the runtime starts
- continuous warm keeps active routes hot while the app is being used
- idle warm suspend pauses background warm traffic while idle
- watcher websocket warm probes the configured watcher path

If your RPC budget is effectively unlimited, `TRADING_RESOURCE_MODE=always-on` disables idle suspension for balance streams and provider warm loops. It does not change confirmation windows or provider safety limits.

## Helius Priority Fees

If `SOLANA_RPC_URL` is a Helius URL, Trench Tools can use it for Helius priority-fee estimates automatically.

Only set `HELIUS_RPC_URL` if:

- your main `SOLANA_RPC_URL` is not Helius, and
- you still want Helius priority-fee estimates.

Only set `HELIUS_WS_URL` if:

- your `SOLANA_WS_URL` is not Helius, or
- you intentionally want a separate Helius watcher path.

The advanced defaults are:

- `AUTO_FEE_BUFFER_PERCENT=10`
- `HELIUS_PRIORITY_LEVEL=high`
- `HELIUS_PRIORITY_REFRESH_INTERVAL_MS=30000`
- `HELIUS_PRIORITY_STALE_MS=45000`

See [ENV_REFERENCE.md](ENV_REFERENCE.md) before changing them.

## Voluntary Support Fee

Trench Tools defaults to a voluntary `0.1%` fee on supported trade paths.

```bash
TRENCH_TOOL_FEE=
```

Values:

- blank or `0.1`: `0.1%`
- `0`: off
- `0.2`: increased support at `0.2%`

Restart the runtime after changing `.env`. If Trench Tools has saved you money and time and you want to support development and future tools, consider leaving the default `0.1%` fee enabled.

## Metadata Upload

Blank/default uses the launchpad's native metadata flow: pump-fun for Pump, Bonk's upload endpoints for Bonk, and Bags API prepare for Bagsapp.

Use Pinata only when you want it:

```bash
LAUNCHDECK_METADATA_UPLOAD_PROVIDER=pinata
PINATA_JWT=YOUR_PINATA_JWT
```

Get a JWT from [Pinata](https://pinata.cloud/).

Pump and Bonk use the shared LaunchDeck metadata/IPFS flow. Bagsapp uses the Bags API prepare flow and returns its own mint and metadata URI. See [launchdeck/METADATA_AND_VANITY.md](launchdeck/METADATA_AND_VANITY.md) for platform behavior and vanity queue formatting.

## Token Distribution

Token split/consolidate is an execution-engine extension feature, not a LaunchDeck launch action. It uses the active execution preset for provider, priority fee, and tip behavior.

Current provider support:

- `Helius Sender`
- `Hello Moon`

If token distribution fails with a provider error, switch the active execution preset to one of those providers before retrying.

## Local State

The unified launcher stores local runtime state under:

```text
.local/trench-tools
```

The shared default auth token is:

```text
.local/trench-tools/default-engine-token.txt
```

Logs default to:

```text
.local/logs
```

Do not commit `.local/`, `.env`, reports containing sensitive data, or screenshots with tokens/keys.

## Advanced Settings

Use [.env.advanced](../.env.advanced) and [ENV_REFERENCE.md](ENV_REFERENCE.md) for:

- host/port/log overrides
- provider endpoint overrides
- warm timing
- Auto Fee tuning
- follow daemon capacity
- launchpad compute/slippage overrides
- execution route-family rollout and warm switches
- local state path overrides
- deferred provider settings

If a setting is not in [.env.example](../.env.example), assume you do not need it for first setup.
