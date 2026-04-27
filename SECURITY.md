# Security

Trench Tools is local-first, self-hosted trading software. You own the machine, wallets, private keys, provider accounts, dependencies, configuration, trades, and outcomes.

Do not treat this project as audited or production-safe by default.

## Security Model

Default local hosts:

- `execution-engine`: `http://127.0.0.1:8788`
- `launchdeck-engine`: `http://127.0.0.1:8789`
- `launchdeck-follow-daemon`: `http://127.0.0.1:8790`

Default posture:

- hosts are intended to stay on loopback/private access
- browser-facing routes require a shared bearer token
- private keys live in `.env`
- runtime state lives under `.local/trench-tools`
- raw ports should not be exposed publicly

## Private Keys

Wallet private keys are configured with:

```bash
SOLANA_PRIVATE_KEY=
SOLANA_PRIVATE_KEY2=
SOLANA_PRIVATE_KEY3=
```

Rules:

- never share `.env`
- never commit `.env`
- never paste real private keys into issues, screenshots, Discord, docs, or support messages
- keep keys only on the machine or VPS that actually runs Trench Tools
- use small first-run amounts until you trust your setup

## Shared Bearer Token

Default token path:

```text
.local/trench-tools/default-engine-token.txt
```

Default VPS path:

```text
/opt/launchdeck/.local/trench-tools/default-engine-token.txt
```

The same token authenticates the extension to:

- `execution-engine`
- `launchdeck-engine`

The extension Options page uses it as `Shared access token`.

Rules:

- do not share the token
- do not put the token in URLs, screenshots, logs, or support messages
- paste it only into the extension Options page or your own trusted local tooling
- reload open browser pages after changing the saved token

## Local Hosts

Keep these private:

- `8788` - extension trading/API host
- `8789` - LaunchDeck host
- `8790` - follow daemon

Do not open these raw ports to the public internet.

## VPS Access

Recommended VPS pattern:

```bash
ssh -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP
```

For extension + LaunchDeck over an SSH tunnel:

```bash
ssh -L 8788:127.0.0.1:8788 -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP
```

The bootstrap script enables `ufw` for OpenSSH and enables `fail2ban`. Keep that posture unless you know what you are changing.

## Remote Extension Hosts

Loopback is the normal extension setup.

If you intentionally point the extension at a non-loopback host:

- use HTTPS
- use browser host-permission grants
- keep the shared bearer token private
- put the hosts behind your own access controls
- do not use plain HTTP over the public internet

## Third-party Trust Boundary

Your setup can depend on:

- RPC providers such as Helius and Shyft
- execution providers such as Helius Sender and Hello Moon
- metadata providers such as Pinata
- VPS providers
- browser extension runtime behavior
- npm and Rust dependencies
- Solana network behavior

Treat those as part of your trust boundary.

## What Not To Share

Do not share:

- `.env`
- private keys
- API keys
- Pinata JWTs
- bearer tokens
- full local logs if they contain keys/tokens
- screenshots with Options -> Global settings visible
- local report files if they expose sensitive addresses, token plans, or infra details

When asking for help, redact first.

## Reporting Security Issues

Please do not open a public issue with exploit details or live secrets.

Report privately to the maintainer through a private contact method you already have available. Include:

- short description
- affected files, routes, or flows
- reproduction steps if safe
- impact
- suggested mitigation if you have one

No SLA is promised, but private reports are appreciated.

## Trading Risk

This software does not guarantee:

- profitable trades
- successful inclusion
- provider uptime
- RPC/websocket reliability
- protection from bad settings
- protection from malicious tokens/sites/pools
- protection from user mistakes

Use at your own risk.
