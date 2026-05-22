# VPS Setup

This guide walks through a fresh Ubuntu VPS setup for Trench Tools. From a clean start, including the quick account, payment, SSH key, firewall, startup script, and Helius steps, the install can be up and running in about 5-10 minutes.

Recommended pattern:

- run Trench Tools on the VPS
- use the VPS path for the best mix of performance and security
- keep raw hosts bound privately
- access browser-facing ports through SSH forwards
- use the extension locally against loopback unless you intentionally set up HTTPS remote access

If you are not technical, use [Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI), Codex, Claude Computer Use, or another trusted computer assistant to walk through this guide with you. These tools are good at checking SSH commands, editing config files, following provider dashboards, and reading logs.

## Fast Path

If you want the shortest working path, do this:

1. Create a Vultr account, confirm your email, and add a payment method.
2. Create or find an SSH key on the computer where you run your browser.
3. Add the public key at [Vultr SSH Keys](https://console.vultr.com/sshkeys/).
4. Add the bootstrap script at [Vultr Startup Scripts](https://console.vultr.com/startup/).
5. Create a firewall group at [Vultr Firewall](https://console.vultr.com/firewall/) that allows SSH on port `22` and drops other inbound traffic.
6. Deploy Ubuntu `24.04` with the SSH key, startup script, and firewall group selected.
7. Add an SSH config entry with local forwards for `8788` and `8789`, then connect with `ssh Trenchtools-vps`.
8. Put your wallet, Helius RPC, and Helius websocket values into `/root/trench.tools/.env`.
9. Restart with `systemctl restart trenchtools`.
10. Copy the shared token, install the extension on your local PC, paste the token into extension Options, and test the connection.

That is the path that should get most users running in about 5-10 minutes after the VPS boots. The rest of this guide explains each step.

## What Runs On The VPS

The default `.env` value `TRENCH_TOOLS_MODE=` means `both`. `TRENCH_TOOLS_MODE=both` starts:

- `execution-engine` on `127.0.0.1:8788`
- `launchdeck-engine` on `127.0.0.1:8789`
- `launchdeck-follow-daemon` on `127.0.0.1:8790`

The shared auth token lives at:

```text
/root/trench.tools/.local/trench-tools/default-engine-token.txt
```

The default install path is `/root/trench.tools`. The default service name is `trenchtools`.

## Recommended Location

Place the VPS near the provider endpoints and RPCs you actually plan to use.

Good starting points:

- EU: Frankfurt or Amsterdam
- US East: New York / Newark area for the default east-side endpoints
- US West / central fallback: Salt Lake City area if you set the Salt Lake provider endpoint
- Asia: Singapore or Tokyo

If you use a grouped `USER_REGION` like `us` or `asia`, remember those metros are far apart. In practice, pick a server near the side you care about and use the exact metro token (`ewr`, `slc`, `sg`, `tyo`, etc.).

## Recommended Server Shape

Start simple:

- Ubuntu `24.04`
- Dedicated CPU type with the CPU Optimized plan category for real trading
- shared CPU only for testing, learning, or very light use
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

This guide uses Vultr as the worked example because it is easy to deploy quickly across a wide range of regions, supports normal card/fiat-style payments as well as crypto, and has been reliable in long-term use.

If you use Vultr, use the referral link in [Create Or Prepare The Vultr Account](#21-create-or-prepare-the-vultr-account). Any other VPS provider is fine as long as you place the server close to the provider endpoints and RPCs you actually plan to use.

The detailed steps below create those provider-level items before deploy: SSH key, startup script, and firewall group. That is the easiest path because the server starts installing Trench Tools automatically as soon as it boots.

Personal note: I have used Vultr for 5+ years and have not had issues with it.

## 1. Create Or Find An SSH Key

SSH is how your computer proves it is allowed to log in to the VPS. You use the same SSH connection for three things:

- logging in to install and update Trench Tools
- opening the server in [Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI) Remote SSH, if you use [Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI) for editing
- forwarding private VPS ports back to your local browser so Chrome/Edge can talk to `127.0.0.1:8788` and `127.0.0.1:8789`

An SSH key has two files:

- private key: stays on your computer, usually `~/.ssh/id_ed25519`
- public key: safe to paste into the VPS provider, usually `~/.ssh/id_ed25519.pub`

Create or choose the key on the computer where you run your browser and [Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI).

1. Open a terminal on your own computer.

On Windows, open PowerShell. On Linux, open Terminal.

2. Check whether you already have an Ed25519 SSH key.

Windows PowerShell:

```powershell
Test-Path $env:USERPROFILE\.ssh\id_ed25519.pub
Get-ChildItem $env:USERPROFILE\.ssh
```

Linux:

```bash
test -f ~/.ssh/id_ed25519.pub && echo "SSH key exists"
ls -la ~/.ssh
```

3. Decide whether to use the existing key or create a new one.

If `id_ed25519.pub` already exists and you recognize it, use that key and skip to step 5. If it does not exist, or you are not sure what it is, create a new one in step 4.

4. Create a new SSH key if you need one.

Windows PowerShell:

```powershell
ssh-keygen -t ed25519 -C "you@example.com"
```

Linux:

```bash
ssh-keygen -t ed25519 -C "you@example.com"
```

When `ssh-keygen` asks where to save the key, press `Enter` to accept the default path. When it asks for a passphrase, you can press `Enter` for no passphrase or set one if you know you want that.

5. Copy the public key to your clipboard.

Windows PowerShell:

```powershell
Get-Content $env:USERPROFILE\.ssh\id_ed25519.pub | Set-Clipboard
```

Linux:

```bash
cat ~/.ssh/id_ed25519.pub
```

Copy the full line manually.

6. Check what you copied before pasting it into [Vultr SSH Keys](https://console.vultr.com/sshkeys/).

It should be one line that starts with `ssh-ed25519` and ends with the label you used, for example `you@example.com`. Only copy the `.pub` public key. Do not copy or share the private key.

## 2. Create The VPS

For Vultr, create the account-level SSH key, startup script, and firewall group before creating the server. Then they appear as selectable options on the deploy page.

### 2.1 Create Or Prepare The Vultr Account

What this does: the VPS cannot be deployed until the account is active and has billing set up.

1. Open [Vultr](https://www.vultr.com/?ref=9589308).
2. Sign up or log in.
3. Confirm the email address on the account.
4. Add a payment method.
5. Keep the dashboard open.

If Vultr does not show deploy options yet, the account email or payment method is usually not finished.

### 2.2 Add Your SSH Key To Vultr

What this does: this lets your computer log in to the VPS as `root` without using a password. Vultr only needs your public key. Your private key stays on your computer.

1. On your local computer, copy your public key again if it is not already copied.

Windows PowerShell:

```powershell
Get-Content $env:USERPROFILE\.ssh\id_ed25519.pub | Set-Clipboard
```

Linux:

```bash
cat ~/.ssh/id_ed25519.pub
```

Copy the full line manually.

If the file does not exist, go back to [Create Or Find An SSH Key](#1-create-or-find-an-ssh-key), create the key, then return here.

2. Open [Vultr SSH Keys](https://console.vultr.com/sshkeys/).
3. Click `Add SSH Key`.
4. Paste your copied public key into the `SSH Key` field.
5. Confirm it starts with `ssh-ed25519`.
6. Put `trench-tools` in the name/label field.
7. Save the SSH key.

### 2.3 Add The Startup Script

What this does: when the VPS boots for the first time, Vultr runs this script automatically. The script downloads the real Trench Tools bootstrap script and starts the install, so you do not have to manually paste install commands after deploying.

1. Open [Vultr Startup Scripts](https://console.vultr.com/startup/).
2. Click `Add Startup Script`.
3. If Vultr asks for a script type, choose the boot-time/startup option.
4. Name the script `trench-tools-bootstrap`.
5. Paste this exact script into the script/body field:

```bash
#!/usr/bin/env bash
set -euo pipefail
apt-get update
apt-get install -y ca-certificates curl
curl -fsSL https://raw.githubusercontent.com/0xD3bt/Trench-Tools/master/scripts/vps-bootstrap.sh -o /root/vps-bootstrap.sh
bash /root/vps-bootstrap.sh
```

6. Save the startup script.

### 2.4 Create The Firewall Group

What this does: the firewall keeps the VPS closed to the public internet except for SSH. Trench Tools ports stay private and are reached later through your SSH tunnel.

1. Open [Vultr Firewall](https://console.vultr.com/firewall/).
2. Create a new firewall group.
3. Name it `trench-tools-ssh-only`.
4. Add one inbound IPv4 rule with these values:

```text
Action: accept
Protocol: SSH
Port: 22
Source: Anywhere
Value: 0.0.0.0/0
```

5. Add the same SSH-only rule under IPv6 too:

```text
Action: accept
Protocol: SSH
Port: 22
Source: Anywhere
Value: ::/0
```

6. Leave the default inbound rule as `drop any 0-65535`.
7. Do not add public rules for `8788`, `8789`, or `8790`.

Only SSH port `22` should be open in the Vultr firewall. The Trench Tools browser ports `8788`, `8789`, and `8790` stay private on the VPS and are reached later through SSH local forwards from your own computer.

### 2.5 Deploy The Server

What this does: this creates the actual VPS and attaches the SSH key, startup script, and firewall group you prepared above.

1. Open the [Vultr deploy page](https://console.vultr.com/deploy-beta/).
2. In `Choose Type`, choose `Dedicated CPU`.
3. In the plans table, choose the `CPU Optimized` category.
4. Use shared CPU only for testing or very light use.
5. Choose Ubuntu `24.04`.
6. Choose at least `2 vCPU / 4 GB RAM`.
7. Choose the region closest to your target RPC/provider endpoints.
8. For EU, start with Frankfurt or Amsterdam.
9. For US East, start with New York / Newark for the default east-side endpoints.
10. For US West or central fallback, start with Salt Lake City and set the Salt Lake provider endpoint.
11. In the SSH key section, select `trench-tools`.
12. In the startup script/user-data section, select `trench-tools-bootstrap`.
13. In the firewall section, select `trench-tools-ssh-only`.
14. Create/deploy the server.

If you skip the startup script, the server still works, but you will need to run the bootstrap commands manually after first SSH login.

If you forgot to select the SSH key while deploying, the easiest fix is usually to destroy the empty fresh server and redeploy with the key selected. Advanced users can add the public key later to `/root/.ssh/authorized_keys`.

If you forgot to attach the firewall group, attach it from the server's settings before you start using the VPS. The only inbound port that needs to be open publicly is SSH `22`.

After the server finishes booting, continue with [Connect To The VPS](#3-connect-to-the-vps). If your VPS provider does not support startup scripts, skip the startup-script part and run the bootstrap manually in that section.

## 3. Connect To The VPS

After the VPS is ready, copy its public IP from the provider dashboard. You can do this section from any terminal, or you can connect with VS Code Remote SSH, [Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI) Remote SSH, Codex, or another computer assistant if you want help editing files and reading logs.

1. Test direct SSH from your local machine:

```bash
ssh root@YOUR_SERVER_IP
```

If SSH says `Permission denied (publickey)`, the provider did not receive or attach the public key you generated. Go back to the provider `SSH Keys` setting and make sure the public key is added and selected for the server.

2. Add a named host to your local SSH config.

This lets you reconnect by name and automatically forward the browser ports. It is the most important quality-of-life step for VPS use.

SSH config location:

- Windows: `C:\Users\<user>\.ssh\config`
- Linux: `~/.ssh/config`

On Windows, run this in PowerShell:

```powershell
New-Item -ItemType Directory -Force $env:USERPROFILE\.ssh
notepad $env:USERPROFILE\.ssh\config
```

On Linux, run this:

```bash
mkdir -p ~/.ssh
nano ~/.ssh/config
```

3. Paste this block into the SSH config file.

Replace `YOUR_SERVER_IP` with the VPS IPv4 address from Vultr.

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

4. Save the SSH config file.

In Notepad, use `File` -> `Save`. In `nano`, press `Ctrl+O`, press `Enter`, then press `Ctrl+X`.

5. Connect with the named host:

```bash
ssh Trenchtools-vps
```

That one command logs in and opens both browser-facing ports on your computer:

- local `127.0.0.1:8788` forwards to the VPS execution engine
- local `127.0.0.1:8789` forwards to the VPS LaunchDeck engine

[Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI) Remote SSH can use the same `Host Trenchtools-vps` entry. In [Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI), choose Remote SSH and select `Trenchtools-vps`. When [Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI) connects, SSH opens the same local forwards, so your browser can reach the private VPS services at `127.0.0.1:8788` and `127.0.0.1:8789`.

Keep one SSH/[Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI) connection open while using the extension or LaunchDeck. The local forwards exist only while the SSH session is connected.

If you did not use the deploy-time startup script, run the bootstrap manually:

```bash
apt-get update
apt-get install -y ca-certificates curl
curl -fsSL https://raw.githubusercontent.com/0xD3bt/Trench-Tools/master/scripts/vps-bootstrap.sh -o /root/vps-bootstrap.sh
bash /root/vps-bootstrap.sh
```

If you did use the deploy-time startup script, check that it completed:

```bash
systemctl status trenchtools
journalctl -u trenchtools -n 100 --no-pager
```

What the script does:

- installs base packages
- installs Rust stable
- installs Node.js 20
- clones the repo to `/root/trench.tools`
- runs `npm install`
- copies `.env.example` to `.env` if needed
- installs and enables a `systemd` service
- enables `ufw` for OpenSSH
- enables `fail2ban`

Optional overrides:

```bash
TRENCH_TOOLS_REPO_BRANCH=master \
TRENCH_TOOLS_DIR=/root/trench.tools \
TRENCH_TOOLS_SERVICE_NAME=trenchtools \
bash /root/vps-bootstrap.sh
```

Older `LAUNCHDECK_*` override names still work for existing automation, but new setups should use `TRENCH_TOOLS_*`.

## 4. Fill `.env`

If you are using a terminal on the VPS:

```bash
cd /root/trench.tools
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
- `LAUNCHDECK_METADATA_UPLOAD_PROVIDER=pinata` and `PINATA_JWT` for the recommended Pump/Bonk metadata upload path

Recommended URL examples:

```bash
SOLANA_RPC_URL=https://beta.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
SOLANA_WS_URL=wss://mainnet.helius-rpc.com/?api-key=YOUR_HELIUS_API_KEY
WARM_RPC_URL=https://rpc.shyft.to?api_key=YOUR_SHYFT_API_KEY
WARM_WS_URL=wss://rpc.shyft.to?api_key=YOUR_SHYFT_API_KEY
```

For LaunchDeck Pump/Bonk metadata uploads, Pinata is recommended and the free tier is enough for normal use. Create a free account at [pinata.cloud](https://pinata.cloud/), create an API key, copy the JWT, and set:

```bash
LAUNCHDECK_METADATA_UPLOAD_PROVIDER=pinata
PINATA_JWT=YOUR_PINATA_JWT
```

After saving `.env`, restart Trench Tools:

```bash
systemctl restart trenchtools
```

Then copy the shared access token. You will paste this into the browser extension on your local PC in the next step.

```bash
cat /root/trench.tools/.local/trench-tools/default-engine-token.txt
```

## 5. Install The Extension And Add The Token

Do this on the local PC that runs Chrome/Edge, not inside the VPS.

1. Download `trench-tools-extension.zip` from the [`extension-latest` GitHub release](https://github.com/0xD3bt/Trench-Tools/releases/tag/extension-latest).
2. Unzip it locally.
3. Load the unzipped `trench-tools-extension` folder as an unpacked Chrome/Edge extension.
4. Open the Trench Tools extension Options page.
5. Open `Global settings`.
6. Paste the token from the VPS into `Shared access token`.
7. Use these host URLs:

```text
Execution host URL: http://127.0.0.1:8788
LaunchDeck host URL: http://127.0.0.1:8789
```

8. Click `Save token`.
9. Click `Test execution host`.
10. Click `Test LaunchDeck connection` if you are running `both` or `ld`.
11. Reload the supported trading site.

At this point the normal setup is done. Keep the `ssh Trenchtools-vps` connection open while using the extension or LaunchDeck.

Even though Trench Tools runs on a VPS, the extension host fields still use `127.0.0.1` because the SSH tunnel makes those VPS services appear local to your browser. See [EXTENSION.md](EXTENSION.md) for the full extension install guide.

## Troubleshooting And Reference

The normal setup is complete after the extension connects. Use the sections below only if something does not work, or when updating/debugging later.

### Optional Service Check

If setup worked and the extension connects, you can skip this section. Use it if the service does not seem to start or you want to inspect logs.

```bash
systemctl status trenchtools
journalctl -u trenchtools -n 100 --no-pager
```

The first start may take a while because Rust builds release binaries. Later restarts should be faster.

The bootstrap installs a `systemd` service so the VPS can start Trench Tools automatically after reboots. That service runs `npm start`, and `npm start` reads `TRENCH_TOOLS_MODE` from `.env`.

### Optional SSH Tunnel Check

If Chrome/Edge runs on your own computer and Trench Tools runs on the VPS, your browser cannot see the VPS's `127.0.0.1` by itself. `127.0.0.1` always means "this computer".

The fix is an SSH tunnel:

```text
your browser -> 127.0.0.1 on your computer -> SSH tunnel -> 127.0.0.1 on the VPS
```

The SSH config in [Connect To The VPS](#3-connect-to-the-vps) already opens this tunnel with `LocalForward` lines. If you added that config and connect with `ssh Trenchtools-vps`, you do not need to run more port commands. Use this section only for verification, daily reconnects, or the manual fallback if you skipped the SSH config.

Port meanings:

- `8788` - execution engine: extension trades, execution presets, wallets, PnL
- `8789` - LaunchDeck engine: LaunchDeck UI, LaunchDeck presets, reports
- `8790` - LaunchDeck follow daemon; internal to the VPS, normally not forwarded to the browser

Recommended daily flow:

1. start or reconnect SSH/[Cursor](https://cursor.com/referral?code=5M7HRMNQT5VI):

```bash
ssh Trenchtools-vps
```

2. keep that SSH session open
3. open the local URLs in your browser:

```text
http://127.0.0.1:8788
http://127.0.0.1:8789
```

Only use this manual fallback if you did not add `LocalForward` to SSH config:

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

Verify from Linux on the same machine as the browser:

```bash
curl http://127.0.0.1:8788/api/extension/auth/bootstrap
curl http://127.0.0.1:8789/health
```

If `8788` works but `8789` fails, extension trading can work while LaunchDeck is offline. Add or fix the `LocalForward 8789 127.0.0.1:8789` line and reconnect SSH.

### Optional Manual Runtime Commands

For manual control inside the repo, use:

```bash
cd /root/trench.tools
npm start
npm stop
npm restart
```

You can still override the mode for a one-off manual run:

```bash
cd /root/trench.tools
./trench-tools-start.sh --mode both
```

Modes:

- `ee` - only `execution-engine` on `8788`; extension trading and PnL
- `ld` - LaunchDeck engine and follow daemon on `8789/8790`
- `both` - all services; normal VPS mode

For the easiest VPS setup, set `TRENCH_TOOLS_MODE` in `.env`, use `systemctl restart trenchtools` after `.env` changes, and let the service own startup. Use the direct start script only when you are testing a one-off mode in an SSH session.

Do not expose plain HTTP `8788`, `8789`, or `8790` to the public internet. Use SSH tunnels unless you intentionally manage your own HTTPS reverse proxy and access controls.

### Auth Token

Default VPS token path:

```text
/root/trench.tools/.local/trench-tools/default-engine-token.txt
```

Use it in extension Options -> Global settings -> `Shared access token`.

Never paste a real token into docs, screenshots, public issues, Discord, or support messages.

### Verification Checklist

- `systemctl status trenchtools` is healthy
- `/root/trench.tools/.env` is filled with placeholder-free values
- token file exists at `/root/trench.tools/.local/trench-tools/default-engine-token.txt`
- SSH config or a manual tunnel forwards `8788` and `8789` to your local browser machine
- LaunchDeck opens locally at `http://127.0.0.1:8789`
- extension Options -> Global settings tests the expected host(s)
- first trade/launch uses a small test amount and `Helius Sender` or `Hello Moon`

### Updating Later

```bash
cd /root/trench.tools
git pull --ff-only
npm install
systemctl restart trenchtools
systemctl status trenchtools
```

If you use the browser extension, reload the unpacked extension in Chrome/Edge after updating:

1. open `chrome://extensions` or `edge://extensions`
2. find Trench Tools
3. click reload
4. re-test Options -> Global settings

If you installed from the packaged zip, download the newest `trench-tools-extension.zip` from the [`extension-latest` GitHub release](https://github.com/0xD3bt/Trench-Tools/releases/tag/extension-latest) on your local PC, unzip it over or beside the old extension folder, then reload Trench Tools in Chrome/Edge. If you loaded `extension/trench-tools` from a git checkout, update that checkout with `git pull --ff-only` first.

### Useful Commands

Status:

```bash
systemctl status trenchtools
```

Restart:

```bash
systemctl restart trenchtools
```

Logs:

```bash
journalctl -u trenchtools -f
```

Stop:

```bash
systemctl stop trenchtools
```

### Security Notes

- do not share `.env`
- do not open `8788`, `8789`, or `8790` directly to the public internet
- keep SSH keys private
- keep `ufw` and `fail2ban` enabled unless you know what you are changing
- use SSH tunnels or HTTPS with your own access controls
- read [../SECURITY.md](../SECURITY.md)
