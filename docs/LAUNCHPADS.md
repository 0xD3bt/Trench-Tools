# Launchpads

## Pump

Pump is the verified launch flow currently migrated into `LaunchDeck`.

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

Bonk is modeled as a Raydium-backed launchpad integration.

Rules:

- use official Raydium SDK surfaces
- prefer SDK v2 when it exposes the required flow cleanly
- do not copy community bot code as the implementation source

Bonk is currently marked unverified until live validation is complete.

## Bagsapp

Bagsapp is modeled using the official Bags launch flow.

Important constraints:

- creator BPS must be explicit
- total fee-claimer BPS must equal `10000`
- LUT-aware config creation may be required for larger fee-claimer sets
- `BAGS_API_KEY` is treated as a launchpad credential, not a provider credential

Bagsapp is currently marked unverified until live validation is complete.
