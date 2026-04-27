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

## Bonk

Status: supported helper-backed path.

Supported operator flows depend on the current Bonk mode and quote-asset requirements. Keep the first run small and verify build/preview output before sending.

## Bagsapp

Status: supported when Bags credentials are configured.

Required starter config:

```bash
BAGS_API_KEY=YOUR_BAGS_API_KEY
```

Advanced Bags setup, tip, and confirmation controls live in [.env.advanced](../../.env.advanced) and [../ENV_REFERENCE.md](../ENV_REFERENCE.md).

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
