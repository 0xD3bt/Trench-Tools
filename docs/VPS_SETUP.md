# VPS Setup

This guide walks through a fresh Ubuntu VPS setup for Trench Tools. With a VPS provider account, the startup script, and Helius Developer tier ready, the install can be up and running in about 5 minutes.

Recommended pattern:

- run Trench Tools on the VPS
- use the VPS path for the best mix of performance and security
- keep raw hosts bound privately
- access browser-facing ports through SSH forwards
- use the extension locally against loopback unless you intentionally set up HTTPS remote access

AI help is fine here. Cursor, Codex, Claude, and similar tools can walk you through SSH, package installs, `.env` editing, `systemctl`, and logs. Do not paste real private keys, API keys, server secrets, or auth tokens into any AI/chat tool.

## Fast Path

If you want the shortest working path, do this:

1. Create a Vultr account or log in.
2. Create an SSH key on the computer where you run your browser.
3. Add the public key at [Vultr SSH Keys](https://console.vultr.com/sshkeys/).
4. Add the bootstrap script at [Vultr Startup Scripts](https://console.vultr.com/startup/).
5. Deploy Ubuntu `24.04` with the SSH key and startup script selected.
6. Put your wallet, Helius RPC, and Helius websocket values into `/opt/launchdeck/.env`.
7. Restart with `systemctl restart launchdeck`.
8. Connect with SSH port forwards, then paste the shared token into the extension.

That is the path that should get most users running in about 5 minutes after the VPS boots. The rest of this guide explains each step.

## What Runs On The VPS

The default `.env` value `TRENCH_TOOLS_MODE=` means `both`. `TRENCH_TOOLS_MODE=both` starts:

- `execution-engine` on `127.0.0.1:8788`
- `launchdeck-engine` on `127.0.0.1:8789`
- `launchdeck-follow-daemon` on `127.0.0.1:8790`

The shared auth token lives at:

```text
/opt/launchdeck/.local/trench-tools/default-engine-token.txt
```

The default install path and service name still use `launchdeck` for upgrade compatibility. The product is Trench Tools.

## Recommended Location

Place the VPS near the provider endpoints and RPCs you actually plan to use.

Good starting points:

- EU: Frankfurt or Amsterdam
- US: New York / Newark area or Salt Lake City area
- Asia: Singapore or Tokyo

If you use a grouped `USER_REGION` like `us` or `asia`, remember those metros are far apart. In practice, pick a server near the side you care about and use the exact metro token (`ewr`, `slc`, `sg`, `tyo`, etc.).

## Recommended Server Shape

Start simple:

- Ubuntu `24.04`
- 2 vCPU minimum
- 4 GB RAM minimum
- enough SSD for Rust builds, `node_modules`, uploads, logs, and reports

## Recommended Stack

- `SOLANA_RPC_URL`: Helius Gatekeeper HTTP
- `SOLANA_WS_URL`: Helius standard websocket
- `WARM_RPC_URL`: Shyft or another low-priority RPC for compatible warm/cache traffic
- provider: `Helius Sender` or `Hello Moon`

Helius Developer tier, about $50/month, is strongly recommended if you care about watcher quality, multiple snipes, or follow automation.

## VPS Provider

This guide uses [Vultr](https://www.vultr.com/?ref=9589308) as the worked example because it is easy to deploy quickly across a wide range of regions, supports normal card/fiat-style payments as well as crypto, and has been reliable in long-term use.

If you use Vultr, please use [my referral link](https://www.vultr.com/?ref=9589308). Any other VPS provider is fine as long as you place the server close to the provider endpoints and RPCs you actually plan to use.

Recommended Vultr flow:

1. create or log in to your Vultr account
2. create your SSH key on your own computer
3. add the public key in Vultr at [SSH Keys](https://console.vultr.com/sshkeys/)
4. add the Trench Tools bootstrap in Vultr at [Startup Scripts](https://console.vultr.com/startup/)
5. deploy the server and select both the SSH key and startup script during deploy

This is the easiest path. The SSH key lets you log in afterward, and the startup script boots the Trench Tools install automatically in about 5 minutes on a normal fresh VPS.

Personal note: I have used Vultr for 5+ years and have not had issues with it.

## 1. Create An SSH Key

SSH is how your computer proves it is allowed to log in to the VPS. You use the same SSH connection for three things:

- logging in to install and update Trench Tools
- opening the server in Cursor Remote SSH, if you use Cursor for editing
- forwarding private VPS ports back to your local browser so Chrome/Edge can talk to `127.0.0.1:8788` and `127.0.0.1:8789`

An SSH key has two files:

- private key: stays on your computer, usually `~/.ssh/id_ed25519`
- public key: safe to paste into the VPS provider, usually `~/.ssh/id_ed25519.pub`

Create the key on the computer where you run your browser and Cursor.

Linux/macOS:

```bash
ssh-keygen -t ed25519 -C "you@example.com"
```

Windows PowerShell:

```powershell
ssh-keygen -t ed25519 -C "you@example.com"
```

Show the public key:

```bash
cat ~/.ssh/id_ed25519.pub
```

Windows PowerShell:

```powershell
Get-Content $env:USERPROFILE\.ssh\id_ed25519.pub
```

Do not share the private key.

Copy the full public key line. It starts with `ssh-ed25519` and ends with the label you used, for example `you@example.com`.

## 2. Create The VPS

For Vultr, set up the account-level SSH key and startup script before creating the server. Then both will appear as selectable options on the deploy page.

First, add the SSH key:

1. open [Vultr SSH Keys](https://console.vultr.com/sshkeys/)
2. click `Add SSH Key`
3. paste the full public key line from `id_ed25519.pub`
4. give it a recognizable name like `trench-tools-laptop`
5. save it

Only the public key goes into Vultr. The private key stays on your computer and is used automatically by `ssh`, Cursor Remote SSH, and any tunnels you open.

Second, add the optional startup script. We highly recommend this because it boots the install automatically instead of making the user copy commands after deploy.

1. open [Vultr Startup Scripts](https://console.vultr.com/startup/)
2. click `Add Startup Script`
3. choose a boot-time script type, if Vultr asks
4. name it `trench-tools-bootstrap`
5. paste the script below
6. save it

```bash
#!/usr/bin/env bash
set -euo pipefail
apt-get update
apt-get install -y ca-certificates curl
curl -fsSL https://raw.githubusercontent.com/0xD3bt/Trench-Tools/master/scripts/vps-bootstrap.sh -o /root/vps-bootstrap.sh
bash /root/vps-bootstrap.sh
```

Now deploy the server from the normal Vultr deploy page:

1. choose Cloud Compute or equivalent
2. choose Ubuntu `24.04`
3. choose at least `2 vCPU / 4 GB RAM`
4. choose the region closest to your target RPC/provider endpoints
5. in the deploy settings, select the SSH key you added
6. in the startup script/user-data option, select `trench-tools-bootstrap`
7. create the server

If you skip the startup script, the server still works, but you will need to run the bootstrap commands manually after first SSH login.

If you forgot to select the SSH key while deploying, the easiest fix is usually to destroy the empty fresh server and redeploy with the key selected. Advanced users can add the public key later to `/root/.ssh/authorized_keys`.

After the server finishes booting, SSH in and continue at [Fill `.env`](#4-fill-env). If your VPS provider does not support startup scripts, skip the startup-script part and run the bootstrap manually in the next section.

## 3. Bootstrap The Server

After the VPS is ready, copy its public IP from the provider dashboard.

First test direct SSH from your local machine:

```bash
ssh root@YOUR_SERVER_IP
```

If SSH says `Permission denied (publickey)`, the provider did not receive or attach the public key you generated. Go back to the provider `SSH Keys` setting and make sure the public key is added and selected for the server.

Recommended: add a named host to your local SSH config so you can reconnect by name and automatically forward the browser ports. This is the most important quality-of-life step for VPS use.

SSH config location:

- Windows: `C:\Users\<user>\.ssh\config`
- macOS/Linux: `~/.ssh/config`

Create the file if it does not exist.

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

After saving the config, connect with:

```bash
ssh Trenchtools-vps
```

Cursor Remote SSH can use the same `Host Trenchtools-vps` entry. In Cursor, choose Remote SSH and select `Trenchtools-vps`. When Cursor connects, SSH opens the same local forwards, so your browser can reach the private VPS services at `127.0.0.1:8788` and `127.0.0.1:8789`.

Keep one SSH/Cursor connection open while using the extension or LaunchDeck. The local forwards exist only while the SSH session is connected.

If you did not use the deploy-time startup script, run the bootstrap manually:

```bash
apt-get update
apt-get install -y ca-certificates curl
curl -fsSL https://raw.githubusercontent.com/0xD3bt/Trench-Tools/master/scripts/vps-bootstrap.sh -o /root/vps-bootstrap.sh
bash /root/vps-bootstrap.sh
```

If you did use the deploy-time startup script, check that it completed:

```bash
systemctl status launchdeck
journalctl -u launchdeck -n 100 --no-pager
```

What the script does:

- installs base packages
- installs Rust stable
- installs Node.js 20
- clones the repo to `/opt/launchdeck`
- runs `npm install`
- copies `.env.example` to `.env` if needed
- installs and enables a `systemd` service
- enables `ufw` for OpenSSH
- enables `fail2ban`

Optional overrides:

```bash
LAUNCHDECK_REPO_BRANCH=master \
LAUNCHDECK_DIR=/opt/launchdeck \
LAUNCHDECK_SERVICE_NAME=launchdeck \
bash /root/vps-bootstrap.sh
```

Those env names are kept for compatibility.

## 4. Fill `.env`

On the VPS:

```bash
cd /opt/launchdeck
nano .env
```

Fill the starter values:

- `SOLANA_PRIVATE_KEY` or your `SOLANA_PRIVATE_KEY*` wallet slots
- `SOLANA_RPC_URL`
- `SOLANA_WS_URL`
- `USER_REGION`
- `TRENCH_TOOLS_MODE` if you want something other than the normal full stack
- `TRENCH_TOOL_FEE` only if you want to turn the voluntary fee off or increase it
- `WARM_RPC_URL` moves compatible warm/cache traffic off the primary RPC
- `HELLOMOON_API_KEY` only if using Hello Moon
- `BAGS_API_KEY` only if using Bags launchpad flows
- `PINATA_JWT` only if using Pinata metadata uploads

Recommended URL examples:

```bash
SOLANA_RPC_URL=https://beta.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
SOLANA_WS_URL=wss://mainnet.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
WARM_RPC_URL=https://rpc.shyft.to?api_key=YOUR_SHYFT_API_KEY
WARM_WS_URL=wss://rpc.shyft.to?api_key=YOUR_SHYFT_API_KEY
```

Restart after editing:

```bash
systemctl restart launchdeck
```

The bootstrap installs a `systemd` service so the VPS can start Trench Tools automatically after reboots. That service runs `npm start`, and `npm start` reads `TRENCH_TOOLS_MODE` from `.env`.

For manual control inside the repo, use:

```bash
cd /opt/launchdeck
npm start
npm stop
npm restart
```

You can still override the mode for a one-off manual run:

```bash
cd /opt/launchdeck
./trench-tools-start.sh --mode both
```

Modes:

- `ee` - only `execution-engine` on `8788`; extension trading and PnL
- `ld` - LaunchDeck engine and follow daemon on `8789/8790`
- `both` - all services; normal VPS mode

For the easiest VPS setup, set `TRENCH_TOOLS_MODE` in `.env`, use `systemctl restart launchdeck` after `.env` changes, and let the service own startup. Use the direct start script only when you are testing a one-off mode in an SSH session.

## 5. Check The Service

```bash
systemctl status launchdeck
journalctl -u launchdeck -n 100 --no-pager
```

The first start may take a while because Rust builds release binaries. Later restarts should be faster.

## 6. Open Browser Ports Through SSH

If Chrome/Edge runs on your own computer and Trench Tools runs on the VPS, your browser cannot see the VPS's `127.0.0.1` by itself. `127.0.0.1` always means "this computer".

The fix is an SSH tunnel:

```text
your browser -> 127.0.0.1 on your computer -> SSH tunnel -> 127.0.0.1 on the VPS
```

That is why the SSH config above includes `LocalForward` lines.

Port meanings:

- `8788` - execution engine: extension trades, execution presets, wallets, PnL
- `8789` - LaunchDeck engine: LaunchDeck UI, LaunchDeck presets, reports
- `8790` - LaunchDeck follow daemon; internal to the VPS, normally not forwarded to the browser

Recommended daily flow:

1. start or reconnect SSH/Cursor:

```bash
ssh Trenchtools-vps
```

2. keep that SSH session open
3. open the local URLs in your browser:

```text
http://127.0.0.1:8788
http://127.0.0.1:8789
```

Manual fallback if you did not add `LocalForward` to SSH config:

```bash
ssh -L 8788:127.0.0.1:8788 -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP
```

Manual LaunchDeck-only tunnel:

```bash
ssh -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP
```

This keeps the runtime private instead of exposing the raw ports to the internet.

Verify from Windows on the same machine as Chrome/Edge:

```powershell
Test-NetConnection 127.0.0.1 -Port 8788
Test-NetConnection 127.0.0.1 -Port 8789
```

Verify from macOS/Linux on the same machine as the browser:

```bash
curl http://127.0.0.1:8788/api/extension/auth/bootstrap
curl http://127.0.0.1:8789/health
```

If `8788` works but `8789` fails, extension trading can work while LaunchDeck is offline. Add or fix the `LocalForward 8789 127.0.0.1:8789` line and reconnect SSH.

## 7. Connect The Extension

Open the Trench Tools extension Options page, then open `Global settings`.

Use these values when you are using SSH forwards:

```text
Execution host URL: http://127.0.0.1:8788
LaunchDeck host URL: http://127.0.0.1:8789
Shared access token: contents of /opt/launchdeck/.local/trench-tools/default-engine-token.txt
```

Then:

1. click `Save token`
2. click `Test execution host`
3. click `Test LaunchDeck connection` if you are running `both` or `ld`
4. reload the supported trading site

Even though Trench Tools runs on a VPS, the extension host fields still use `127.0.0.1` because the SSH tunnel makes those VPS services appear local to your browser.

Do not expose plain HTTP `8788`, `8789`, or `8790` to the public internet. Use SSH tunnels unless you intentionally manage your own HTTPS reverse proxy and access controls.

## Auth Token

Default VPS token path:

```text
/opt/launchdeck/.local/trench-tools/default-engine-token.txt
```

Use it in extension Options -> Global settings -> `Shared access token`.

Never paste a real token into docs, screenshots, public issues, Discord, or support messages.

## Verification Checklist

- `systemctl status launchdeck` is healthy
- `/opt/launchdeck/.env` is filled with placeholder-free values
- token file exists at `/opt/launchdeck/.local/trench-tools/default-engine-token.txt`
- SSH config or a manual tunnel forwards `8788` and `8789` to your local browser machine
- LaunchDeck opens locally at `http://127.0.0.1:8789`
- extension Options -> Global settings tests the expected host(s)
- first trade/launch uses a small test amount and `Helius Sender` or `Hello Moon`

## Updating Later

```bash
cd /opt/launchdeck
git pull --ff-only
npm install
systemctl restart launchdeck
systemctl status launchdeck
```

If you use the browser extension, reload the unpacked extension in Chrome/Edge after updating:

1. open `chrome://extensions` or `edge://extensions`
2. find Trench Tools
3. click reload
4. re-test Options -> Global settings

## Useful Commands

Status:

```bash
systemctl status launchdeck
```

Restart:

```bash
systemctl restart launchdeck
```

Logs:

```bash
journalctl -u launchdeck -f
```

Stop:

```bash
systemctl stop launchdeck
```

## Security Notes

- do not share `.env`
- do not open `8788`, `8789`, or `8790` directly to the public internet
- keep SSH keys private
- keep `ufw` and `fail2ban` enabled unless you know what you are changing
- use SSH tunnels or HTTPS with your own access controls
- read [../SECURITY.md](../SECURITY.md)
