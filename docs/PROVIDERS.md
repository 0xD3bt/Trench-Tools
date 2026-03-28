# Providers

## Supported Provider IDs

- `auto`
- `helius`
- `jito`
- `astralane`
- `bloxroute`
- `hellomoon`

## Verified vs Unverified

Current verified providers in the app model:

- `helius`
- `jito`
- `astralane`

Currently modeled but still marked unverified until live validation:

- `bloxroute`
- `hellomoon`

## Intent

- `auto`: choose the best verified path for the current execution shape
- `helius`: default fast single-tx path
- `jito`: native bundle path
- `astralane`: advanced low-latency path
- `bloxroute`: modeled low-latency alternative
- `hellomoon`: modeled Lunar Lander alternative

## Current Runtime Behavior

The app exposes provider availability and support state through `/api/status` and `/api/settings`.

The provider adapter layer resolves:

- requested provider
- resolved provider
- execution class: `single`, `sequential`, or `bundle`

At this stage, Jito remains the main live bundle path in the code.
