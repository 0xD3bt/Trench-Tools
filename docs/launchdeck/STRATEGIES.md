# Strategies

LaunchDeck supports launchpad actions that happen at deploy time, after submit, after confirmation, or after market conditions are met.

## Dev Buy

Dev buy is the creator-side buy path used during launch workflows. Keep the first run simple:

- one wallet
- recommended provider
- small amount
- build/preview before send

## Sniper Buys

Common timing modes:

- same-time buy
- on-submit plus delay
- confirmed-block / offset buy

Delayed and confirmed-block actions are handed to the follow daemon so the browser request does not need to stay open.

## Dev Auto-sell

Dev auto-sell is a post-launch sell action. It depends on the follow daemon and healthy watcher/RPC paths.

Use conservative first-run sizing and verify the sell route before relying on automation.

## Follow Sells

Follow sells can be driven by configured triggers and watcher state. The follow daemon owns the active jobs, timing, and persistence.

## Market-cap Triggers

Where supported, market-cap-based actions depend on live market snapshots and SOL/USD pricing. Helius asset pricing is preferred when available, with configured HTTP fallback paths where supported.

## Operational Notes

- Run `--mode both` for full LaunchDeck automation.
- Keep `SOLANA_WS_URL` healthy; watcher quality matters.
- Keep the VPS close to selected provider endpoints.
- Start with `Helius Sender` or `Hello Moon`.
- Do not tune advanced follow capacity until the default path is healthy.
