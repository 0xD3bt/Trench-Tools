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
2. Set `SOLANA_RPC_URL`, `SOLANA_WS_URL`, and `USER_REGION`.
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

Execution-engine presets are separate. Extension token split/consolidate actions use the active execution preset, not the LaunchDeck preset.

## Name Presets

The launch form includes name preset buttons under the token name/ticker fields. Use them for repeated launch naming patterns such as applying a token name suffix, a ticker suffix, a prefix, the first word as ticker, or an abbreviated ticker.

Name presets are saved in the LaunchDeck config and can be edited from the LaunchDeck settings modal. They only change the current launch form's token name and ticker fields; they do not change execution-engine presets, LaunchDeck trade presets, wallets, providers, or fee settings.

## Metadata And Images

Metadata upload defaults to the launchpad's native uploader when `LAUNCHDECK_METADATA_UPLOAD_PROVIDER` is blank: pump-fun for Pump, Bonk's upload endpoints for Bonk, and Bags API prepare for Bagsapp.

Use Pinata only when you want it:

```bash
LAUNCHDECK_METADATA_UPLOAD_PROVIDER=pinata
PINATA_JWT=YOUR_PINATA_JWT
```

The image library is local state. Do not commit uploaded local assets or metadata that you do not intend to publish.

Pump and Bonk use LaunchDeck's shared metadata/IPFS flow. Bagsapp uses the Bags API prepare flow because Bags returns the mint and metadata URI. Pump and Bonk can also use local vanity mint queue files.

The launch form can crop a selected image before upload. When LaunchDeck opens from J7 tweet context, detected image candidates can be selected, cropped, or saved into the local image library before launch.

See [METADATA_AND_VANITY.md](METADATA_AND_VANITY.md).

## Reports

Use the Dashboard/Reports surface to review:

- launches
- transactions
- jobs
- logs
- timing detail when benchmark mode is enabled

See [REPORTING.md](REPORTING.md).

## Runtime Diagnostics

Runtime status and diagnostics can show:

- LaunchDeck host health
- follow daemon health
- launchpad backend support status
- warm-state status
- vanity queue diagnostics
- RPC traffic counters when enabled

Use these diagnostics before changing advanced settings. Most first-run issues are `.env`, provider access, auth token, tunnel, or RPC health problems.

## Follow Automation

Delayed buys, confirmed-block actions, dev-auto-sells, and follow sells are owned by the follow daemon. The browser request does not need to stay open for those actions to continue, but the daemon must be running and connected to healthy RPC/websocket infrastructure.

See [FOLLOW_DAEMON.md](FOLLOW_DAEMON.md) and [STRATEGIES.md](STRATEGIES.md).

## Launchpad Support

High-level support:

- Pump: primary verified path
- Bonk: supported Rust-native path, including SOL and USD1 quote-asset behavior where configured by the launch flow
- Bagsapp: supported Rust-native path when Bags credentials are configured; Bags uses wallet identity and its own prepare flow

See [LAUNCHPADS.md](LAUNCHPADS.md).
