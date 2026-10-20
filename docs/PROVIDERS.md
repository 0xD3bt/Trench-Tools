# Providers

This page covers send-provider guidance for Trench Tools.

Current default recommendation:

- start with `Helius Sender`
- use `Hello Moon` when you want a strong alternate low-latency path
- use `OrbitFlare Apex` for extension trade presets when you have an Apex API key

`Standard RPC` and `Jito Bundle` still have code paths, but they are not currently recommended in setup docs while they are being re-tested. See [Deferred Providers](#deferred-providers).

## Runtime Split

Do not think of the provider as the whole stack. Trench Tools separates:

- send provider - where signed transactions are submitted
- read/confirm RPC - `SOLANA_RPC_URL`
- watcher websocket - `SOLANA_WS_URL`
- warm/cache RPC - `WARM_RPC_URL`

Recommended baseline:

```bash
SOLANA_RPC_URL=https://beta.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
SOLANA_WS_URL=wss://mainnet.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
WARM_RPC_URL=https://rpc.shyft.to?api_key=YOUR_SHYFT_API_KEY
```

Helius Gatekeeper HTTP is recommended for the HTTP/read path. Helius standard websocket is recommended for watcher subscriptions. Shyft is useful for warm/cache/block-height traffic.

## Helius Sender

Use this first if you want the easiest production default.

What it does:

- sends through Helius Sender endpoints
- supports region-aware routing through `USER_REGION`
- pairs naturally with Helius RPC, websocket, and priority-fee estimates
- works well with the recommended Helius Gatekeeper HTTP / standard websocket split

Useful config:

```bash
USER_REGION=eu
SOLANA_RPC_URL=https://beta.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
SOLANA_WS_URL=wss://mainnet.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
```

Region examples:

- EU: `eu`, `fra`, `ams`
- US: `ewr`, `slc`
- Asia: `sg`, `tyo`

Advanced overrides:

- `USER_REGION_HELIUS_SENDER`
- `HELIUS_SENDER_ENDPOINT`
- `HELIUS_SENDER_BASE_URL`

Most users should leave endpoint overrides blank and use region routing.

## Hello Moon

Use this when you want a strong alternate low-latency execution path and have Lunar Lander access.

Requirements:

- Hello Moon Lunar Lander API key
- endpoint access from [Hello Moon docs](https://docs.hellomoon.io/reference/lunar-lander) or the [Hello Moon Discord](https://discord.com/invite/HelloMoon)

Config:

```bash
HELLOMOON_API_KEY=YOUR_HELLOMOON_API_KEY
USER_REGION=eu
```

Advanced overrides:

- `USER_REGION_HELLOMOON`
- `HELLOMOON_QUIC_ENDPOINT`
- `HELLOMOON_MEV_PROTECT`

Hello Moon does not expose the exact same metro map as Helius. Unsupported metros are mapped to the closest Hello Moon endpoints the provider exposes.

## OrbitFlare Apex

Use this as a low-latency send path for extension trade presets (buy and sell). LaunchDeck launches do not route through Apex yet.

What it does:

- sends each transaction over a persistent QUIC connection through the official [`orbitflare-apex`](https://crates.io/crates/orbitflare-apex) client; the API key derives a client certificate and never crosses the wire
- forwards to the upcoming leaders over staked validator connections, Jito and direct TPU
- requires an inline tip of at least `0.001 SOL` to one of Apex's tip accounts; the engine picks one for you
- applies MEV protection (skips leaders on OrbitFlare's Shield blocklist) when the preset's MEV mode is not `off`

Requirements:

- OrbitFlare Apex API key from [apex.orbitflare.com](https://apex.orbitflare.com)

Config:

```bash
APEX_API_KEY=YOUR_APEX_API_KEY
USER_REGION=eu
```

Region mapping:

- `global`: `global.apex.orbitflare.com`, which resolves to the nearest endpoint
- `us`: New York and Salt Lake City; `ewr` maps to New York
- `eu`: Frankfurt, Amsterdam and London
- `asia`: Singapore and Tokyo; `sg` maps to Singapore
- `slc`, `lon`, `fra`, `ams`, `tyo`: the matching endpoint

Advanced overrides:

- `USER_REGION_APEX`
- `APEX_QUIC_ENDPOINT`
- `APEX_MEV_PROTECT` (warmed connections only; trades follow the preset's MEV mode)

See the [Apex documentation](https://docs.orbitflare.com/apex) for tiers and protocol details.

## Region Notes

`USER_REGION` is shared across region-aware providers.

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

If you use grouped routing like `us` or `asia`, remember those regions can span far-apart endpoints. In practice, you usually get better results by placing the VPS near one of the provider metros you care about and using the matching metro token.

## Auto Fee

Auto Fee stays warm in the Rust hosts and resolves from local snapshots when you trade or launch.

Useful defaults:

- `AUTO_FEE_BUFFER_PERCENT=10`
- `HELIUS_PRIORITY_LEVEL=high`
- `HELIUS_PRIORITY_REFRESH_INTERVAL_MS=30000`
- `HELIUS_PRIORITY_STALE_MS=45000`

Those values live in [.env.advanced](../.env.advanced). Most users should not tune them at first.

Jito tip estimate settings also exist because Auto Fee can use the feed, but `Jito Bundle` as a provider is deferred until re-tested.

## Deferred Providers

These paths still exist, but they are not currently recommended in user setup guides.

### Standard RPC

Status: not currently recommended; pending re-validation.

Standard RPC submits through normal Solana RPC transport. It is useful when you explicitly want plain-RPC behavior or need to test fallback paths, but it is not part of the current default operator recommendation.

Related advanced config:

- `LAUNCHDECK_EXTRA_STANDARD_RPC_SEND_URLS`

### Jito Bundle

Status: not currently recommended; pending re-validation.

Jito Bundle paths are kept for users who explicitly understand bundle semantics and want to test them, but setup docs should not steer new users there until the path is re-tested.

Related advanced config:

- `USER_REGION_JITO_BUNDLE`
- `JITO_BUNDLE_BASE_URLS`
- `JITO_SEND_BUNDLE_ENDPOINT`
- `JITO_BUNDLE_STATUS_ENDPOINT`
- `JITO_TIP_PERCENTILE`
- `JITO_TIP_REFRESH_INTERVAL_MS`
- `JITO_TIP_STALE_MS`

## First-run Provider Rule

For first live use:

1. choose `Helius Sender`
2. set Helius RPC and websocket
3. set `USER_REGION`
4. use a small test amount
5. only switch providers after the full stack is healthy
