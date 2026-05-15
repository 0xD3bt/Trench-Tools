# Troubleshooting

Start with the process that is failing.

- Extension trading/PnL: `execution-engine` on `8788`
- Launch/Snipe/Reports: `launchdeck-engine` on `8789`
- delayed/follow actions: `launchdeck-follow-daemon` on `8790`

## Logs

Local Windows:

```text
.local\logs\execution-engine.log
.local\logs\execution-engine.stderr.log
.local\logs\launchdeck-engine.log
.local\logs\launchdeck-engine.stderr.log
.local\logs\launchdeck-follow-daemon.log
.local\logs\launchdeck-follow-daemon.stderr.log
```

Local Linux:

```text
.local/logs/execution-engine.log
.local/logs/launchdeck-engine.log
.local/logs/launchdeck-follow-daemon.log
```

VPS systemd logs:

```bash
journalctl -u launchdeck -n 100 --no-pager
journalctl -u launchdeck -f
```

## Startup Fails

Check:

- dependencies are installed (`node -v`, `npm -v`, `cargo -V`)
- `.env` exists in the repo root
- `.env` has valid `SOLANA_RPC_URL`, `SOLANA_WS_URL`, and wallet values
- ports `8788`, `8789`, and `8790` are not already in use
- the first Rust build has enough RAM/disk and time to finish

Try the simple repo-root command first:

```bash
npm start
```

If you need to force a one-off mode, use the explicit launcher:

Windows:

```powershell
.\trench-tools-start.ps1 --mode both
```

Linux:

```bash
./trench-tools-start.sh --mode both
```

## Extension Says Execution Host Is Unreachable

The extension trading path needs `execution-engine`.

Check:

- `.env` has `TRENCH_TOOLS_MODE=ee`, `TRENCH_TOOLS_MODE=both`, or blank, and you ran `npm start`
- Options -> Global settings -> `Execution host` is `http://127.0.0.1:8788`
- `http://127.0.0.1:8788/api/extension/auth/bootstrap` opens or responds locally
- browser extension was reloaded after a repo update
- no local firewall/security tool is blocking loopback

If the host is on a VPS, use an SSH tunnel:

```bash
ssh -L 8788:127.0.0.1:8788 root@YOUR_SERVER_IP
```

For the full extension plus LaunchDeck setup, forward both browser-facing ports:

```bash
ssh -L 8788:127.0.0.1:8788 -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP
```

## Extension Auth Fails

The shared token file is:

```text
.local/trench-tools/default-engine-token.txt
```

On the default VPS install:

```text
/opt/launchdeck/.local/trench-tools/default-engine-token.txt
```

Fix:

1. open the token file
2. copy the whole token value
3. open extension Options -> Global settings
4. paste it into `Shared access token`
5. click `Save token`
6. click `Test execution host`
7. reload the supported site

The same token is used for both `execution-engine` and `launchdeck-engine`.

## LaunchDeck Popout Is Offline

Launch/Snipe/Reports need `launchdeck-engine`.

Check:

- `.env` has `TRENCH_TOOLS_MODE=both`, `TRENCH_TOOLS_MODE=ld`, or blank, and you ran `npm start`
- `http://127.0.0.1:8789` opens locally
- extension Options -> Global settings -> `LaunchDeck host` is `http://127.0.0.1:8789`
- `Test LaunchDeck connection` passes
- the shared token is saved

If `TRENCH_TOOLS_MODE=ee` is running, extension trades can work while LaunchDeck popout stays offline. That is expected.

If execution trades and execution presets work but LaunchDeck preset saving fails with `ERR_CONNECTION_REFUSED`, the VPS is usually fine and your local browser machine is missing the `8789` forward. Check from the browser machine:

```powershell
Test-NetConnection 127.0.0.1 -Port 8788
Test-NetConnection 127.0.0.1 -Port 8789
```

If `8788` succeeds and `8789` fails, add `LocalForward 8789 127.0.0.1:8789` to your SSH config or reconnect with the two-port tunnel below.

## Follow Actions Do Not Run

Follow jobs need the follow daemon.

Check:

- `.env` has `TRENCH_TOOLS_MODE=both`, `TRENCH_TOOLS_MODE=ld`, or blank, and you ran `npm start`
- `launchdeck-follow-daemon` logs are present
- `launchdeck-engine` can reach the daemon on `127.0.0.1:8790`
- your watcher websocket is healthy (`SOLANA_WS_URL`)
- your RPC is not rate-limiting block-height or account reads

Delayed actions are daemon-owned so they do not depend on one browser tab staying open, but the daemon still needs healthy RPC/websocket access.

## Wallets Do Not Show Up

Check:

- wallet keys are in `.env`, not `.env.example`
- key slots use `SOLANA_PRIVATE_KEY`, `SOLANA_PRIVATE_KEY2`, `SOLANA_PRIVATE_KEY3`, etc.
- optional labels use `PRIVATE_KEY,Label`
- there are no extra quotes around the key unless the runtime explicitly accepts them
- you restarted after editing `.env`

Never paste real private keys into public issues, screenshots, or support messages.

## RPC Or Websocket Problems

Recommended starter values:

```bash
SOLANA_RPC_URL=https://beta.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
SOLANA_WS_URL=wss://mainnet.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
WARM_RPC_URL=https://rpc.shyft.to?api_key=YOUR_SHYFT_API_KEY
```

Common issues:

- HTTP URL pasted into `SOLANA_WS_URL`
- websocket URL pasted into `SOLANA_RPC_URL`
- expired or wrong API key
- free-tier rate limiting
- VPS is far from the selected provider region
- `USER_REGION` points to a region far from the VPS

`WARM_RPC_URL` moves compatible warm/cache traffic off the main RPC budget.

## Provider Rejections

For first setup, use `Helius Sender` or `Hello Moon`.

If Hello Moon fails:

- confirm `HELLOMOON_API_KEY` is set
- confirm your account has Lunar Lander access
- try the closest supported region

`Standard RPC` and `Jito Bundle` are deferred provider paths while they are being re-tested. Do not use them as first-run defaults.

## Axiom Does Not Show Trench Tools

Check:

- extension is loaded unpacked from `extension/trench-tools`
- extension was reloaded after `git pull`
- Axiom is enabled in Options -> Sites
- the specific Axiom surfaces you want are enabled
- the page was refreshed after saving Options
- `execution-engine` is running if trade surfaces need live data

If J7 does not show Trench Tools, confirm the specific contract-address or tweet-card controls are enabled in Options -> Sites, the unpacked extension was reloaded after updating, and the page was refreshed. Terminal and GMGN are coming later.

## Axiom Button Shows Unsupported Pool Or Pair

Axiom can show a `pair` address that is useful context, but Trench Tools still verifies the account owner and layout before trading. If you see:

```text
Address ... is not a token mint or supported pool/pair account.
```

or a similar unsupported-route message, the engine could not classify that address as a supported route.

Common causes:

- the pair is an Orca Whirlpool or another pool family that is not generally supported
- the pair is a generic Raydium CLMM/CPMM pool outside the supported route rules
- the pair is a non-canonical Pump AMM pool and non-canonical trading is blocked
- the page provided a stale pair after migration
- the RPC could not read or verify the account

Open [SUPPORTED_POOLS.md](SUPPORTED_POOLS.md) for the current support matrix. When in doubt, start from the token mint or the verified pool address and use a small test amount.

## Token Split Or Consolidate Fails

Token distribution is an extension/runtime feature for the active token.

Check:

- the selected execution preset uses `Helius Sender` or `Hello Moon`
- split has at least two selected wallets
- split has at least one selected wallet that currently holds the token
- consolidate has exactly one destination wallet selected
- the token is a normal SPL Token or supported Token-2022 mint without a transfer hook
- the selected source wallets have enough SOL for fees and any provider tip

The split/consolidate buttons use the active execution preset. If the wrong provider, fee, or tip is used, change the active preset in the toolbar popup or panel before trying again.

## Pump Creator-vault Custom 2006 Errors

Pump routes can occasionally hit a creator-vault race where the first build/send sees a custom `2006`-style error. The default runtime has a narrow one-shot retry path for this class of Pump creator-vault issue.

If it keeps happening:

- update to the latest branch version
- make sure the token/pair is current and not a stale page route
- check `execution-engine` logs for `pump-creator-vault retry`
- leave `EXECUTION_ENGINE_ENABLE_PUMP_CREATOR_VAULT_AUTO_RETRY` blank unless you intentionally need to disable the retry for debugging

LaunchDeck follow buys/sells have separate Pump creator-vault retry switches documented in [.env.advanced](../.env.advanced).

## VPS Tunnel Issues

The tunnel depends on two separate things:

- SSH login to the VPS works with your private key
- the SSH session includes `LocalForward` lines for the browser ports

Recommended SSH config on your local machine:

```sshconfig
Host Trenchtools-vps
  HostName YOUR_SERVER_IP
  User root
  IdentityFile ~/.ssh/id_ed25519
  IdentitiesOnly yes
  LocalForward 8788 127.0.0.1:8788
  LocalForward 8789 127.0.0.1:8789
  ExitOnForwardFailure yes
  ServerAliveInterval 30
```

Then connect with:

```bash
ssh Trenchtools-vps
```

Manual LaunchDeck UI tunnel:

```bash
ssh -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP
```

Manual extension + LaunchDeck tunnel:

```bash
ssh -L 8788:127.0.0.1:8788 -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP
```

If the tunnel fails:

- confirm the public key was added to the VPS provider and selected during deploy
- confirm the private key still exists on your computer, usually `~/.ssh/id_ed25519`
- confirm SSH works normally with `ssh Trenchtools-vps`
- confirm `systemctl status launchdeck`
- confirm the service started the hosts
- confirm your local ports are not already in use
- keep the SSH session open while using the tunnel
- if using Cursor Remote SSH, disconnect and reconnect after editing `~/.ssh/config` or `C:\Users\<user>\.ssh\config`

The extension should still point at local-looking URLs while the tunnel is open:

- `Execution host` -> `http://127.0.0.1:8788`
- `LaunchDeck host` -> `http://127.0.0.1:8789`

Do not open `8788`, `8789`, or `8790` directly to the public internet.

## Reports Or Local History Look Wrong

Local state lives under:

```text
.local/trench-tools
```

If `launchdeck-engine` cannot reach `execution-engine`, it can queue confirmed trade records locally and replay them later. Start `execution-engine` again and let the runtime catch up before assuming history is missing.

## When To Check Advanced Config

Only go to [.env.advanced](../.env.advanced) after the starter stack is healthy.

Advanced config is for:

- provider endpoint overrides
- warm timing
- Auto Fee tuning
- follow daemon capacity
- compute/slippage overrides
- local state path overrides

Most first-run failures are `.env`, auth token, host mode, or tunnel issues, not advanced tuning.
