# Launchpads

LaunchDeck is the launchpad feature inside Trench Tools. This page summarizes supported launchpad coverage.

## Pump

Status: primary verified path.

Supported operator flows:

- deploy
- dev buy
- sniper buys
- dev auto-sell
- sniper/follow sells
- Pump AMM market handling where supported

Use Pump as the first launchpad path when validating a new install.

Metadata uses the shared LaunchDeck uploader. Vanity mint queues are supported through `.local/trench-tools/vanity/pump.txt`; queued public mints must end with `pump`.

## Bonk

Status: supported Rust-native path.

Supported operator flows depend on the current Bonk mode and quote-asset requirements. Bonk launches can use SOL or USD1 quote behavior where the launch flow requires it. The runtime handles the pinned SOL/USD1 route setup and related shared lookup-table behavior internally.

Keep the first run small and verify build/preview output before sending.

Metadata uses the shared LaunchDeck uploader with the Bonk default upload endpoints unless Pinata is selected. Vanity mint queues are supported through `.local/trench-tools/vanity/bonk.txt`; queued public mints must end with `bonk`.

## Bagsapp

Status: supported when Bags credentials are configured.

Required starter config:

```bash
BAGS_API_KEY=YOUR_BAGS_API_KEY
```

Advanced Bags setup, tip, and confirmation controls live in [.env.advanced](../../.env.advanced) and [../ENV_REFERENCE.md](../ENV_REFERENCE.md).

Bagsapp owns mint and metadata preparation through the Bags API. The operator-facing Bags flow is wallet-identity based, and LaunchDeck does not use a Bags vanity queue because Bags returns the mint during prepare.

Bags routes can involve Meteora DBC before migration and Meteora DAMM v2 after migration. Follow and sell behavior depends on the market state the runtime can verify from RPC.

See [METADATA_AND_VANITY.md](METADATA_AND_VANITY.md) for metadata/IPFS and vanity queue details.

## Provider Guidance

For first live use, stick to:

- `Helius Sender`
- `Hello Moon`

`Standard RPC` and `Jito Bundle` are deferred provider paths while they are being re-tested. See [../PROVIDERS.md](../PROVIDERS.md).

## Practical Rule

Before using size:

1. confirm wallets are loaded
2. confirm RPC/websocket are healthy
3. choose the recommended provider path
4. build/preview
5. use a small first live amount
