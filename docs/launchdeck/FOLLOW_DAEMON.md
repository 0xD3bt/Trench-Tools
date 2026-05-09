# Follow Daemon

`launchdeck-follow-daemon` owns delayed and follow actions for LaunchDeck.

It normally runs on:

```text
http://127.0.0.1:8790
```

You usually do not talk to it directly. `launchdeck-engine` talks to it for follow jobs.

## What It Owns

- delayed sniper buys
- confirmed-block / offset actions
- dev auto-sells
- sniper/follow sells
- market-cap triggered actions where supported
- watcher health for active jobs
- persisted follow job state

This separation keeps delayed work alive outside the lifetime of one browser request.

## When It Runs

Run modes:

- `both` - starts execution engine, LaunchDeck, and follow daemon
- `ld` - starts LaunchDeck and follow daemon
- `ee` - does not start the follow daemon

If follow actions are not running, check that you are not in `ee` mode.

## State

Default state path under the unified launcher:

```text
.local/trench-tools/follow-daemon-state.json
```

Do not commit local state.

## Config

Most users should leave follow daemon settings blank.

Advanced controls live in [.env.advanced](../../.env.advanced):

- `LAUNCHDECK_FOLLOW_DAEMON_TRANSPORT`
- `LAUNCHDECK_FOLLOW_DAEMON_URL`
- `LAUNCHDECK_FOLLOW_DAEMON_PORT`
- `LAUNCHDECK_FOLLOW_MAX_ACTIVE_JOBS`
- `LAUNCHDECK_FOLLOW_MAX_CONCURRENT_COMPILES`
- `LAUNCHDECK_FOLLOW_MAX_CONCURRENT_SENDS`
- `LAUNCHDECK_FOLLOW_CAPACITY_WAIT_MS`
- `LAUNCHDECK_FOLLOW_OFFSET_POLL_INTERVAL_MS`
- `LAUNCHDECK_ENABLE_APPROXIMATE_FOLLOW_OFFSET_TIMER`
- `LAUNCHDECK_SOL_USD_HTTP_PRICE_URL`
- `LAUNCHDECK_ENABLE_PUMP_BUY_CREATOR_VAULT_AUTO_RETRY`
- `LAUNCHDECK_ENABLE_PUMP_SELL_CREATOR_VAULT_AUTO_RETRY`

Do not tune capacity until the default path is healthy.

## Timing And Capacity

Confirmed-block and offset actions use a real block-height offset worker by default. `LAUNCHDECK_ENABLE_APPROXIMATE_FOLLOW_OFFSET_TIMER` is an advanced fallback and should stay off unless you intentionally want local timer approximation.

Capacity settings are optional caps:

- `LAUNCHDECK_FOLLOW_MAX_ACTIVE_JOBS` limits active jobs.
- `LAUNCHDECK_FOLLOW_MAX_CONCURRENT_COMPILES` limits compile work.
- `LAUNCHDECK_FOLLOW_MAX_CONCURRENT_SENDS` limits send work.
- `LAUNCHDECK_FOLLOW_CAPACITY_WAIT_MS` controls how long LaunchDeck waits for capacity when caps are set.

Blank or `0` capacity values mean uncapped.

## Pump Creator-vault Retries

Pump follow buys and sells have narrow creator-vault retry paths enabled by default. Leave these blank for normal operation:

```bash
LAUNCHDECK_ENABLE_PUMP_BUY_CREATOR_VAULT_AUTO_RETRY=
LAUNCHDECK_ENABLE_PUMP_SELL_CREATOR_VAULT_AUTO_RETRY=
```

Set a false-like value only when debugging the retry path itself.

## Troubleshooting

Check:

- `--mode both` or `--mode ld` is running
- `launchdeck-follow-daemon` logs exist
- `SOLANA_WS_URL` is healthy
- `SOLANA_RPC_URL` is not rate-limiting block-height/account reads
- the VPS is close to selected providers/RPCs

See [../TROUBLESHOOTING.md](../TROUBLESHOOTING.md).
