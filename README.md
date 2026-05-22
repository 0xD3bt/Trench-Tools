# Trench.Tools - The open-source trading stack for Solana traders.

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

Trench Tools is an open-source, self-hosted execution stack for Solana trading and launch workflows.

The stack is built around a local Rust execution engine, a browser extension for supported terminals, and LaunchDeck for launchpad operations. The runtime keeps wallets, presets, RPC configuration, transaction construction, signing, and send policy on infrastructure you control.

Use it locally for setup and testing. For lower latency and a cleaner security boundary in live trading, run it on a cheap private VPS near your closest RPC and execution-provider endpoints.

The project is under active development. Make sure your setup is configured and verified end to end before using it for proper trading.

## How It Works

Trench Tools separates the browser surface from execution:

- Run `execution-engine` and `LaunchDeck` on your own machine or a private VPS. For live trading, the recommended setup is a cheap VPS near your RPC and execution-provider endpoints.
- Install the Chrome/Edge extension in your local browser. It injects Trench Tools controls into supported platforms such as Axiom and J7Tracker.
- When you trade from a supported platform, the extension sends the trade intent to your own execution engine. The engine handles route validation, transaction build/sign/send, confirmations, and PnL events using your configured wallets, presets, RPCs, and providers.
- If the runtime is on a VPS, keep the raw ports private and connect your local browser through SSH forwards to `127.0.0.1:8788` and `127.0.0.1:8789`.

## Recommended Stack

For most operators today:

- run on a VPS near the provider endpoints and RPCs you actually use
- EU VPS location: Frankfurt or Amsterdam
- US VPS location: New York / Newark area for the default east-side endpoints, or Salt Lake City area for western users who set the Salt Lake provider endpoint
- Asia VPS location: Singapore or Tokyo
- [Helius Developer tier](https://www.helius.dev/pricing), about $50/month, for the main infrastructure
- `SOLANA_RPC_URL`: Helius Gatekeeper HTTP, `https://beta.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY`
- `SOLANA_WS_URL`: Helius standard websocket, `wss://mainnet.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY`
- `WARM_RPC_URL`: separate [Shyft](https://shyft.to/) RPC for compatible warm/cache traffic off the main Helius budget
- execution provider: `Helius Sender` or `Hello Moon`

Why this split: Helius Gatekeeper HTTP has been the best Helius HTTP path in our testing, while Helius standard websocket has been the better watcher websocket path. Shyft is a good low-priority warm RPC because its free tier is useful for warmup, cache, and block-height traffic.

Hello Moon is the recommended alternate low-latency provider. It requires Lunar Lander access from [Hello Moon docs](https://docs.hellomoon.io/reference/lunar-lander) or the [Hello Moon Discord](https://discord.com/invite/HelloMoon).

Do not treat any shared latency numbers as universal. Test from the VPS and region you actually run.

### VPS Note

[Vultr](https://www.vultr.com/?ref=9589308) is the worked example in [docs/VPS_SETUP.md](docs/VPS_SETUP.md). It is easy to deploy quickly across many regions, supports standard card/fiat payments as well as crypto, and has been reliable for long-term use. Any other VPS provider is fine as long as you place it close to the provider endpoints and RPCs you plan to use.

Personal note: I have used Vultr for 5+ years and have not had issues with it.

## Start Here

For most users:

1. Read [docs/QUICKSTART.md](docs/QUICKSTART.md) for local Windows/Linux setup.
2. If you are using a fresh server, use [docs/VPS_SETUP.md](docs/VPS_SETUP.md) instead.
3. Install the browser extension with [docs/EXTENSION.md](docs/EXTENSION.md). For most users, download the latest `trench-tools-extension.zip`, unzip it, and load the unzipped folder as an unpacked Chrome/Edge extension.
4. Keep [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) nearby for connection/auth issues.

With a fresh VPS, the bootstrap startup script, and a Helius Developer tier plan, most users can get the stack up from scratch in about 5-10 minutes. Local setup is fine when you are editing, testing, or learning the tool.

If you get stuck during setup, use an AI coding assistant to walk through the steps with you. [Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI), Codex, Claude, and similar tools are all fine for checking install commands, editing `.env`, reading logs, and following the VPS guide.

## Runtime Modes

The starter `.env.example` runs the full stack by default:

```bash
TRENCH_TOOLS_MODE=both
```

Use `both` for the normal setup. It starts `execution-engine`, `launchdeck-engine`, and `launchdeck-follow-daemon`.

Other modes:

- `ee` - extension trading only. Starts `execution-engine` on port `8788`.
- `ld` - LaunchDeck only. Starts `launchdeck-engine` on port `8789` and `launchdeck-follow-daemon` on port `8790`.

Run from the repo root:

```bash
npm start
npm stop
npm restart
```

The launcher exits after health checks pass, and the selected services keep running in the background. Use `npm stop` to stop them.

For one-off mode overrides:

```bash
# Windows
.\trench-tools-start.ps1 --mode both

# Linux
./trench-tools-start.sh --mode both
```

## Supported Sites

The extension site list is moving fast. Current shipped support:

- Live: `axiom.trade` / `backup.axiom.trade`
- Live: `j7tracker.io`
- Planned: Terminal (formerly Padre), GMGN, Telegram web, Discord web, X, and more terminals

See [docs/EXTENSION.md](docs/EXTENSION.md) for current install steps, site toggles, and platform-specific surfaces.

## Current Route Coverage

The execution engine verifies routes from on-chain state before trading. Current native coverage includes Pump bonding curve, Pump AMM, LetsBonk launchpad and supported post-migration Bonk routes, Raydium LaunchLab SOL pools, Raydium AMM v4 and CPMM WSOL pool inputs, Meteora DBC, Meteora DAMM v2, and a small trusted stable-route allowlist.

Raydium AMM v4 and CPMM support does not mean generic best-pool discovery. Standalone Raydium pools must be submitted as verified pool accounts and pass owner/layout/mint checks.

Pool/pair support is intentionally not the same as "anything a website labels as a pair." See [docs/SUPPORTED_POOLS.md](docs/SUPPORTED_POOLS.md) before assuming a route is executable.

## Voluntary Support Fee

Trench Tools has a voluntary fee setting on supported trade paths. The value is a percentage, so `0.1` means `0.1%`. For example, `100 SOL` in volume would total `0.1 SOL` in support fees at `0.1%`. The fee helps support continued development and maintenance.

The starter `.env.example` uses the default:

```bash
TRENCH_TOOL_FEE=0.1
```

To turn it off:

```bash
TRENCH_TOOL_FEE=0
```

To increase it to `0.2%`:

```bash
TRENCH_TOOL_FEE=0.2
```

Restart the runtime after changing `.env`. The setting only applies to supported trade paths that include the Trench Tools fee route.

## Quick Verification

After setup:

- `execution-engine` is reachable at `http://127.0.0.1:8788`
- `launchdeck-engine` is reachable at `http://127.0.0.1:8789` when running `both` or `ld`
- `launchdeck-follow-daemon` is running behind LaunchDeck when running `both` or `ld`
- the token file exists at `.local/trench-tools/default-engine-token.txt`
- Extension Options -> Global settings shows the expected host connection state
- Axiom shows the enabled Trench Tools surfaces
- the toolbar popup shows the expected preset, wallet/group, and quick-buy controls

If the runtime is on a VPS and your browser is on your own computer, add both forwards to your SSH config so [Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI)/SSH opens them automatically:

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
- [docs/SUPPORTED_POOLS.md](docs/SUPPORTED_POOLS.md) - supported execution-engine pools, routes, and pair caveats
- [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) - startup, extension auth, VPS, RPC, and provider issues

LaunchDeck:

- [docs/launchdeck/USAGE.md](docs/launchdeck/USAGE.md) - LaunchDeck operator workflow
- [docs/launchdeck/LAUNCHPADS.md](docs/launchdeck/LAUNCHPADS.md) - Pump, Bonk, Bagsapp support matrix
- [docs/launchdeck/STRATEGIES.md](docs/launchdeck/STRATEGIES.md) - dev buys, snipes, dev sells, follow sells
- [docs/launchdeck/METADATA_AND_VANITY.md](docs/launchdeck/METADATA_AND_VANITY.md) - metadata/IPFS uploads, Pinata, and vanity mint queues
- [docs/launchdeck/FOLLOW_DAEMON.md](docs/launchdeck/FOLLOW_DAEMON.md) - watcher ownership, triggers, and follow timing
- [docs/launchdeck/REPORTING.md](docs/launchdeck/REPORTING.md) - reports, history, and local state

Contributor/internal reference:

- [docs/internal/EXECUTION_DOS_AND_DONTS.md](docs/internal/EXECUTION_DOS_AND_DONTS.md)
- [docs/internal/ROUTE_SOURCE_POLICY.md](docs/internal/ROUTE_SOURCE_POLICY.md)

## Security

Keep the runtime private by default:

- do not share `.env`
- do not paste real private keys, API keys, JWTs, or auth tokens into issues, screenshots, Discord, or support messages
- do not expose raw local ports to the public internet
- use the SSH-tunnel VPS pattern in [docs/VPS_SETUP.md](docs/VPS_SETUP.md)
- use HTTPS and browser host-permission grants if you intentionally point the extension at non-loopback hosts

Read [SECURITY.md](SECURITY.md) before running this with real wallets.

## Licensing

Trench.Tools core software, including the execution engine, browser extension, LaunchDeck, and shared runtime crates, is licensed under the GNU Affero General Public License v3.0 only. See [LICENSE](LICENSE) for the full license text and [NOTICE](NOTICE) for project notices.

You are free to use, study, modify, self-host, and redistribute the software under the terms of the AGPLv3. If you modify the software and make it available to others, including through a hosted service or network-accessible product, you must make the corresponding source code available under the same license.

## Branding

The Trench.Tools name, logo, domain, visual identity, and related branding are not licensed under the AGPLv3. You may not use Trench.Tools branding to present a fork, modified version, commercial service, hosted service, extension package, or unrelated product as official, endorsed, sponsored, or affiliated with Trench.Tools without written permission.

See [TRADEMARK.md](TRADEMARK.md) for trademark and branding guidelines.
