# Trench Tools Extension

This folder contains the Chrome/Edge extension surface for Trench Tools.

Operator setup lives in the root docs:

- [../../docs/EXTENSION.md](../../docs/EXTENSION.md) - install, connect, auth token, presets, sites, updates
- [../../docs/QUICKSTART.md](../../docs/QUICKSTART.md) - local backend setup
- [../../docs/VPS_SETUP.md](../../docs/VPS_SETUP.md) - VPS setup

## Layout

- `src/background/` - MV3 service worker, HTTP client, balance stream, active-mint registry
- `src/content/` - content-script runtime and platform adapters
- `src/content/platforms/` - terminal-specific adapters such as Axiom and J7
- `src/panel/` - floating iframe trading panel
- `src/options/` - Options page for connection, presets, wallets, wallet groups, sites, rewards, appearance
- `src/popup/` - toolbar popup
- `src/shared/` - constants and shared client utilities
- `launchdeck/` - packaged LaunchDeck shell used by the extension popout
- `tests/` - stable extension tests
- `scripts/` - package/check scripts that are part of the repo

## Useful Commands

From this folder:

```bash
npm run check
npm run package:launchdeck-shell
npm run test:layout
```

Local smoke/debug helpers should stay untracked when they are tied to a developer machine or browser session. Stable scripts used by contributors should be named clearly and wired into `package.json`.

## Scaffold Rules

- The extension trading side talks to `execution-engine` for trades, wallets, presets, PnL, and the live event stream.
- The embedded LaunchDeck shell talks to `launchdeck-engine` for Launch, Snipe, Reports, and LaunchDeck-native routes.
- Do not import LaunchDeck UI internals directly into the trading-panel side.
- Keep terminal adapters as the only place that scrapes platform DOM.
- Keep storage keys product-owned (`trenchTools.*`) and runtime messages namespaced (`trench:*`).
- Keep private reverse-engineering notes out of the repo.
