# Trench.Tools - Arming the Solana trenches with open-source tooling.

<p align="center">
  <img src="assets/trench-tools-hero.png" alt="Trench Tools - the open-source trading stack for the trenches" width="100%">
</p>

<div align="center">
  <table>
    <tr>
      <td align="center"><a href="https://trench.tools/"><strong>Website</strong></a></td>
      <td align="center"><a href="https://x.com/TrenchDotTools"><strong>@TrenchDotTools</strong></a></td>
      <td align="center"><a href="https://x.com/0xd3bt"><strong>@0xd3bt</strong></a></td>
      <td align="center"><a href="https://x.com/i/communities/2038790841418838419"><strong>Community</strong></a></td>
    </tr>
  </table>
</div>

<div align="center">
  <table>
    <tr>
      <td align="center">
        <strong>Contract Address</strong><br>
        <code>L73w5odyo5ZdJ1fPp319nfjqaFfHDdKifRmM8Kxpump</code>
      </td>
    </tr>
  </table>
</div>

Trench Tools is a self-hosted Solana trading stack. You run it, you choose the RPCs and senders, and your wallets stay on your own machine or VPS.

The browser extension plugs into the terminals you already use, so you can trade with your own presets and wallet groups instead of routing everything through another platform account. The toolbar popup lets you check connection/auth state, choose the active preset and wallet selection, and set a quick-buy amount without opening the full Options page. LaunchDeck is the launch side: deploy, snipe, dev-buy, dev-sell, follow flows, reports, and automation.

No mandatory accounts. No required platform fees. Clone it, run it, own what gets built, signed, and sent.

This repo is under active development. The docs reflect the setup and features we consider usable today. The software is provided as-is; by using it, you accept responsibility for your machine, VPS, wallets, keys, dependencies, provider accounts, and any trading outcome.

## What Trench Tools Is

Trench Tools has three main pieces:

- `execution engine` (`execution-engine`, port `8788`) - the local Rust trading host. It owns wallets, presets, fee and route resolution, transaction build/sign/send, confirmations, the balance/PnL event stream, and the voluntary Trench Tools fee setting. Anything that submits a trade goes through here. The browser extension talks to this for every trade.
- `Trench Tools extension` - the Chrome/Edge extension that injects Trench Tools into supported trading terminals so you can trade with your presets and wallet groups from inside those sites. It talks to the local hosts over loopback by default and uses a shared bearer token.
- `LaunchDeck` (`launchdeck-engine`, port `8789`, plus `launchdeck-follow-daemon` on port `8790`) - the launchpad feature inside Trench Tools. It handles deploy, snipe, dev-buy, dev-sell, and follow flows for Pump, Bonk, and Bagsapp. It has its own standalone UI on `http://127.0.0.1:8789` and is also available through the extension popout.

## Start Here

For most users:

1. Read [docs/QUICKSTART.md](docs/QUICKSTART.md) for local Windows/Linux setup.
2. If you are using a fresh server, use [docs/VPS_SETUP.md](docs/VPS_SETUP.md) instead.
3. Install the browser extension with [docs/EXTENSION.md](docs/EXTENSION.md). Get `extension/trench-tools` by pulling the repo with git, or download the full repository to your PC and load that extension folder.
4. Keep [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) nearby for connection/auth issues.

VPS is still the recommended real trading setup because it is cheap, private by default, and closer to the latency profile you actually care about. With a fresh VPS, the bootstrap startup script, and a Helius Developer tier plan, most users can get the stack up from scratch in about 5 minutes. Local setup is fine when you are editing, testing, or learning the tool.

If you get stuck during setup, use an AI coding assistant to walk through the steps with you. Cursor, Codex, Claude, and similar tools are all fine for checking install commands, editing `.env`, reading logs, and following the VPS guide. Do not paste real private keys, API keys, or auth tokens into any AI/chat tool.

## Which Mode Should I Run?

Set the mode in `.env` first:

- `TRENCH_TOOLS_MODE=` or `TRENCH_TOOLS_MODE=both` - normal full stack. Starts `execution-engine`, `launchdeck-engine`, and `launchdeck-follow-daemon`.
- `TRENCH_TOOLS_MODE=ee` - extension trading only. Starts only `execution-engine` on `8788`.
- `TRENCH_TOOLS_MODE=ld` - LaunchDeck only. Starts LaunchDeck and the follow daemon, but not extension trading.

Then use the simple repo-root commands:

```bash
npm start
npm stop
npm restart
```

You can still override the mode for a one-off run:

- Windows: `.\trench-tools-start.ps1 --mode both`
- Linux: `./trench-tools-start.sh --mode both`

The launcher exits after the selected services pass their health checks. `npm stop` stops the running Trench Tools processes.

## Recommended Stack

For most operators today:

- run on a VPS near the provider endpoints and RPCs you actually use
- EU VPS location: Frankfurt or Amsterdam
- US VPS location: New York / Newark area or Salt Lake City area
- Asia VPS location: Singapore or Tokyo
- [Helius Developer tier](https://www.helius.dev/pricing), about $50/month, or better for the main infrastructure
- `SOLANA_RPC_URL`: Helius Gatekeeper HTTP, `https://beta.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY`
- `SOLANA_WS_URL`: Helius standard websocket, `wss://mainnet.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY`
- `WARM_RPC_URL`: separate [Shyft](https://shyft.to/) RPC for compatible warm/cache traffic off the main Helius budget
- execution provider: `Helius Sender` or `Hello Moon`

Why this split: Helius Gatekeeper HTTP has been the best Helius HTTP path in our testing, while Helius standard websocket has been the better watcher websocket path. Shyft is a good low-priority warm RPC because its free tier is useful for warmup, cache, and block-height traffic.

Hello Moon is the recommended alternate low-latency provider. It requires Lunar Lander access from [Hello Moon docs](https://docs.hellomoon.io/reference/lunar-lander) or the [Hello Moon Discord](https://discord.com/invite/HelloMoon).

Do not treat any shared latency numbers as universal. Test from the VPS and region you actually run.

### VPS Note

[Vultr](https://www.vultr.com/?ref=9589308) is the worked example in [docs/VPS_SETUP.md](docs/VPS_SETUP.md). It is easy to deploy quickly across many regions, supports standard card/fiat payments as well as crypto, and has been reliable for long-term use. If you use Vultr, please use [my referral link](https://www.vultr.com/?ref=9589308). Any other VPS provider is fine as long as you place it close to the provider endpoints and RPCs you plan to use.

Personal note: I have used Vultr for 5+ years and have not had issues with it.

## Supported Sites

The extension site list is moving fast. Current status:

- Live: `axiom.trade`
- Available, currently disabled: `j7tracker.io`
- Coming soon: Terminal (formerly Padre), GMGN, Telegram web, Discord web, X, and more terminals

Axiom currently has the richest integration: token-page controls, Pulse quick buy and manual panel controls, watchlist and wallet-tracker quick buys, floating panel, LaunchDeck popout, Vamp import helpers, and DexScreener shortcuts. See [docs/EXTENSION.md](docs/EXTENSION.md) for the current extension setup and site-status details.

## Current Route Coverage

The execution engine verifies routes from on-chain state before trading. Current native coverage includes Pump bonding curve and Pump AMM, Bonk routes, Raydium AMM v4 and CPMM WSOL pool inputs, Raydium LaunchLab SOL pools, Meteora DBC and DAMM v2 launchpad routes, and a small trusted stable-route allowlist.

Pool/pair support is intentionally not the same as "anything a website labels as a pair." See [docs/SUPPORTED_POOLS.md](docs/SUPPORTED_POOLS.md) before assuming a route is executable.

## Voluntary Support Fee

Trench Tools defaults to a voluntary `0.1%` fee on supported trade paths.

To turn it off:

```bash
TRENCH_TOOL_FEE=0
```

To keep the default, leave it blank or set:

```bash
TRENCH_TOOL_FEE=0.1
```

To increase support to `0.2%`:

```bash
TRENCH_TOOL_FEE=0.2
```

Restart the runtime after changing `.env`. If Trench Tools has saved you money and time and you want to support development and future tools, consider leaving the default `0.1%` fee enabled. It is still much lower than the average fee charged by current trading platforms.

## Quick Verification

After setup:

- `execution-engine` is reachable at `http://127.0.0.1:8788`
- `launchdeck-engine` is reachable at `http://127.0.0.1:8789` when running `both` or `ld`
- `launchdeck-follow-daemon` is running behind LaunchDeck when running `both` or `ld`
- the token file exists at `.local/trench-tools/default-engine-token.txt`
- Extension Options -> Global settings shows the expected host connection state
- Axiom shows the enabled Trench Tools surfaces
- the toolbar popup shows the expected preset, wallet/group, and quick-buy controls

If the runtime is on a VPS and your browser is on your own computer, add both forwards to your SSH config so Cursor/SSH opens them automatically:

```sshconfig
Host Trenchtools-vps
  HostName YOUR_SERVER_IP
  User root
  LocalForward 8788 127.0.0.1:8788
  LocalForward 8789 127.0.0.1:8789
  ExitOnForwardFailure yes
  ServerAliveInterval 30
```

Manual fallback:

```bash
ssh -L 8788:127.0.0.1:8788 -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP
```

Use a small test amount first. Start with the recommended providers: `Helius Sender` or `Hello Moon`.

## Security

Keep the runtime private by default:

- do not share `.env`
- do not paste real private keys, API keys, JWTs, or auth tokens into issues, screenshots, Discord, or support messages
- do not expose raw local ports to the public internet
- use the SSH-tunnel VPS pattern in [docs/VPS_SETUP.md](docs/VPS_SETUP.md)
- use HTTPS and browser host-permission grants if you intentionally point the extension at non-loopback hosts

Read [SECURITY.md](SECURITY.md) before running this with real wallets.

## Documentation Map

Start here:

- [docs/QUICKSTART.md](docs/QUICKSTART.md) - local Windows/Linux setup, first run, and first extension connection
- [docs/VPS_SETUP.md](docs/VPS_SETUP.md) - fresh VPS setup, bootstrap script, systemd service, SSH tunnels
- [docs/EXTENSION.md](docs/EXTENSION.md) - Chrome/Edge developer-mode install, host pairing, auth token, presets, sites, updates
- [docs/CONFIG.md](docs/CONFIG.md) - recommended stack, runtime defaults, Helius guidance, regions, warm behavior
- [docs/ENV_REFERENCE.md](docs/ENV_REFERENCE.md) - every `.env.example` and `.env.advanced` variable

Execution and architecture:

- [docs/PROVIDERS.md](docs/PROVIDERS.md) - Helius Sender, Hello Moon, and deferred provider notes
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - execution engine, extension, LaunchDeck, auth flow, local state
- [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) - startup, extension auth, VPS, RPC, and provider issues

LaunchDeck:

- [docs/launchdeck/USAGE.md](docs/launchdeck/USAGE.md) - LaunchDeck operator workflow
- [docs/launchdeck/LAUNCHPADS.md](docs/launchdeck/LAUNCHPADS.md) - Pump, Bonk, Bagsapp support matrix
- [docs/launchdeck/STRATEGIES.md](docs/launchdeck/STRATEGIES.md) - dev buys, snipes, dev sells, follow sells
- [docs/launchdeck/FOLLOW_DAEMON.md](docs/launchdeck/FOLLOW_DAEMON.md) - watcher ownership, triggers, and follow timing
- [docs/launchdeck/REPORTING.md](docs/launchdeck/REPORTING.md) - reports, history, and local state

Contributor/internal reference:

- [docs/internal/EXECUTION_DOS_AND_DONTS.md](docs/internal/EXECUTION_DOS_AND_DONTS.md)
- [docs/internal/ROUTE_SOURCE_POLICY.md](docs/internal/ROUTE_SOURCE_POLICY.md)
