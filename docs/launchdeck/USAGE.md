# LaunchDeck Usage

LaunchDeck is the launchpad feature inside Trench Tools. It runs through `launchdeck-engine` on `http://127.0.0.1:8789` and uses `launchdeck-follow-daemon` on `8790` for delayed/follow actions. When the execution engine is also running, confirmed LaunchDeck trades are recorded into the shared execution ledger.

Use LaunchDeck for deploy, snipe, dev-buy, dev-sell, follow, and reports across Pump, Bonk, and Bagsapp.

## Start It

Full stack:

```bash
./trench-tools-start.sh --mode both
```

Windows:

```powershell
.\trench-tools-start.ps1 --mode both
```

LaunchDeck only:

```bash
./trench-tools-start.sh --mode ld
```

Open:

```text
http://127.0.0.1:8789
```

## First Run Flow

1. Load at least one wallet through `SOLANA_PRIVATE_KEY*`.
2. Set `SOLANA_RPC_URL`, `SOLANA_WS_URL`, `USER_REGION`, and optionally `WARM_RPC_URL`.
3. Start with `Helius Sender` or `Hello Moon`.
4. Create or choose a LaunchDeck preset.
5. Build.
6. Simulate where the flow supports it.
7. Use a small first live amount.
8. Deploy or send only after the preview looks right.

## Presets

LaunchDeck presets control launchpad defaults:

- creation provider
- buy provider
- sell provider
- buy amounts
- sell percentages
- priority fee / tip / Auto Fee behavior
- slippage
- snipe defaults

LaunchDeck presets can be edited inside LaunchDeck and from the extension Options page.

## Metadata And Images

Metadata upload defaults to pump-fun when `LAUNCHDECK_METADATA_UPLOAD_PROVIDER` is blank.

Use Pinata only when you want it:

```bash
LAUNCHDECK_METADATA_UPLOAD_PROVIDER=pinata
PINATA_JWT=YOUR_PINATA_JWT
```

The image library is local state. Do not commit uploaded local assets or metadata that you do not intend to publish.

## Reports

Use the Dashboard/Reports surface to review:

- launches
- transactions
- jobs
- logs
- timing detail when benchmark mode is enabled

See [REPORTING.md](REPORTING.md).

## Follow Automation

Delayed buys, confirmed-block actions, dev-auto-sells, and follow sells are owned by the follow daemon. The browser request does not need to stay open for those actions to continue, but the daemon must be running and connected to healthy RPC/websocket infrastructure.

See [FOLLOW_DAEMON.md](FOLLOW_DAEMON.md) and [STRATEGIES.md](STRATEGIES.md).

## Launchpad Support

High-level support:

- Pump: primary verified path
- Bonk: supported helper-backed path
- Bagsapp: supported when Bags credentials are configured

See [LAUNCHPADS.md](LAUNCHPADS.md).
