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

Do not tune capacity until the default path is healthy.

## Troubleshooting

Check:

- `--mode both` or `--mode ld` is running
- `launchdeck-follow-daemon` logs exist
- `SOLANA_WS_URL` is healthy
- `SOLANA_RPC_URL` is not rate-limiting block-height/account reads
- the VPS is close to selected providers/RPCs

See [../TROUBLESHOOTING.md](../TROUBLESHOOTING.md).
