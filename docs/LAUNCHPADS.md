# Launchpads

## Pump

Pump is the verified launch flow currently migrated into `LaunchDeck`.

The current implementation supports:

- `regular`
- `cashback`
- `agent-custom`
- `agent-unlocked`
- `agent-locked`

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
