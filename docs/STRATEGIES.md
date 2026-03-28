# Strategies

## Shared Post-Launch Strategies

- `none`
- `dev-buy`
- `snipe-own-launch`
- `automatic-dev-sell`

## Snipe Own Launch

Intent:

- submit separate follow-up buy transactions after launch
- target roughly `1-2` blocks after launch
- keep it separate from launch creation routing

This strategy should only be enabled where the selected launchpad supports the required follow-up buy flow.

## Automatic Dev Sell

Intent:

- sell a configured percentage of the dev wallet
- allow a small delay after launch

Current UI fields:

- `automaticDevSellEnabled`
- `automaticDevSellPercent`
- `automaticDevSellDelaySeconds`

Validation targets:

- percent: `0-100`
- delay: `0-10`
