# Launchpads

## Pump

Pump is an active verified launch flow in `LaunchDeck`.

The current implementation supports:

- `regular`
- `cashback`
- `agent-custom`
- `agent-unlocked`
- `agent-locked`

Current runtime notes for Pump:

- the Rust engine builds these launch modes natively
- launch transactions use measured versioned transaction selection rather than fixed legacy formatting
- default lookup tables are warmed and persisted locally to reduce compile latency
- agent launch modes keep `AgentInitialize` in the creation transaction when size allows, with follow-up transactions only where the flow still requires them
- regular Pump launches with an immediate dev buy can include the expected Pump account-extension and Token-2022 ATA setup instructions before the buy executes

### Pump Mode Semantics

#### `regular`

- standard Pump token creation path
- creator fee routing stays on the deployer unless deferred fee-sharing is explicitly enabled
- may optionally include an immediate dev buy in the same launch transaction

#### `cashback`

- same core creation flow as `regular`
- creation marks cashback behavior in the Pump `CreateV2` payload
- deferred fee-sharing follow-up can still be generated when configured

#### `agent-unlocked`

- initializes the agent during the creation transaction
- keeps creator reward distribution untouched on launch
- does not generate an agent follow-up transaction
- buyback percentage is configurable and not hard-forced to `100%`

#### `agent-custom`

- initializes the agent during the creation transaction
- defers final custom reward distribution setup to a follow-up `agent-setup` transaction
- follow-up applies the configured recipient split after launch

#### `agent-locked`

- initializes the agent during the creation transaction
- locks creator rewards to the agent payments escrow model
- emits a follow-up `agent-setup` transaction for the final fee-sharing setup path

### Transaction Shape By Mode

The current verified native Pump path uses these high-level transaction shapes:

- `regular` / `cashback`
  - one launch transaction by default
  - optional deferred `follow-up` transaction only when `generateLaterSetup` fee sharing is enabled
- `agent-unlocked`
  - one launch transaction only
- `agent-custom`
  - launch transaction
  - `agent-setup` follow-up transaction for custom recipient setup
- `agent-locked`
  - launch transaction
  - `agent-setup` follow-up transaction for locked agent escrow fee setup

When a Pump launch also includes an immediate dev buy, the launch transaction can additionally include:

- Pump `ExtendAccount`
- Token-2022 associated token account creation via `create_associated_token_account_idempotent`
- Pump `Buy`

## Bonk

Bonk is an active verified launch target.

Current Bonk coverage:

- `regular`
- `bonkers`
- `sol` quote asset
- `usd1` quote asset with automatic SOL -> USD1 top-up when needed
- immediate dev buy
- same-time sniper buys
- follow buys
- follow sells
- automatic dev sell

Current runtime notes for Bonk:

- the Rust engine owns Bonk validation, transport planning, reporting, simulation/send orchestration, and follow-daemon integration
- Bonk launch assembly uses the Raydium LaunchLab SDK-backed helper bridge
- `regular` routes through LetsBonk
- `bonkers` routes through the Bonkers platform config on Raydium LaunchLab
- same-time Bonk sniper buys use a non-bundle safeguard so the launch tx lands before the buy path is sent
- `usd1` mode prefers Raydium route-based top-up handling before Bonk buys when the wallet is short on USD1

Current Bonk restrictions:

- Pump-only modes such as `cashback`, `agent-custom`, `agent-unlocked`, and `agent-locked` are rejected
- fee-sharing setup is rejected
- `mayhem` is rejected
- per-sniper `postBuySell` chaining is not shipped yet

## Bagsapp

Bagsapp is not active in the current initial version.

Do not treat Bagsapp as a supported launch target yet.
