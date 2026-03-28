# Configuration

## Environment

Provider and launchpad credentials stay env-only.

Key variables:

- `SOLANA_PRIVATE_KEY`
- `HELIUS_RPC_URL`
- `HELIUS_API_KEY`
- `ASTRALANE_API_KEY`
- `BAGS_API_KEY`
- `BLOXROUTE_AUTH_HEADER`
- `HELLOMOON_API_KEY`
- `HELLOMOON_RPC_URL`
- `JITO_BUNDLE_BASE_URLS`
- `JITO_SEND_BUNDLE_ENDPOINT`
- `JITO_BUNDLE_STATUS_ENDPOINT`

## Persisted App Config

The UI settings file lives at:

`LaunchDeck/.local/launchdeck/app-config.json`

It stores:

- default launch execution settings
- default buy execution settings
- launch presets
- sniper presets
- default post-launch strategy
- default automatic dev-sell settings

## Launch Config

`launch.example.yml` shows the build/simulate/send launch config shape.

Important runtime fields:

- `launchpad`
- `mode`
- `execution.provider`
- `execution.policy`
- `execution.autoGas`
- `execution.buyProvider`
- `execution.buyPolicy`
- `postLaunch.strategy`
- `presets.selectedLaunchPresetId`
- `presets.selectedSniperPresetId`

## Safe Defaults

The app defaults are designed so a normal user can open the UI and get a sensible baseline without manually tuning every field:

- launch provider: `auto`
- launch policy: `fast`
- buy provider: `auto`
- buy policy: `fast`
- post-launch strategy: `none`
- automatic dev sell: off
