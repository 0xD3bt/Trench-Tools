# Reporting

LaunchDeck reports are local operator history. They are useful for reviewing launches, transactions, jobs, logs, and timings.

## What Reports Cover

- launch records
- transaction records
- follow jobs
- runtime logs
- timing detail when benchmark reporting is enabled

## Local State

Reports and local runtime state live under `.local/trench-tools` when started through the unified launcher.

Do not commit local reports if they contain wallet addresses, token plans, private infrastructure details, or anything you do not intend to publish.

## Benchmark Detail

`LAUNCHDECK_BENCHMARK_MODE` controls report timing detail.

Supported values:

- `off`
- `light`
- `full`

Blank defaults to `full`.

Advanced block-height report capture can be controlled with `LAUNCHDECK_TRACK_SEND_BLOCK_HEIGHT`.

## Pending Ledger Replay

If LaunchDeck confirms a trade while `execution-engine` is offline, it can queue the record locally and replay it into the execution ledger when the execution host comes back.

If history looks incomplete, start the full stack again and let the replay catch up before assuming data is lost.

## Useful Docs

- [USAGE.md](USAGE.md)
- [FOLLOW_DAEMON.md](FOLLOW_DAEMON.md)
- [../TROUBLESHOOTING.md](../TROUBLESHOOTING.md)
