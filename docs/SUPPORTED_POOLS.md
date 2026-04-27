# Supported Pools And Routes

This page describes what the local `execution-engine` can trade today through native execution. It is intentionally current-state documentation, not a permanent limit. More pools, venues, and route types will be added over time.

## What "Supported" Means

The engine only trades a token or pool after it can verify the on-chain account owner and account layout for a known route family.

An address shown by a site as a `pair` is not automatically executable. On Axiom, `pair` usually means "the pool address Axiom is displaying", and that pool may be Raydium, Orca, Meteora, Pump, Bags, Bonk, or something else. The engine still has to classify it as one of the supported route families below.

If the UI says:

```text
Address ... is not a token mint or supported pool/pair account.
```

then the address either was not found on-chain, was not a token mint, or was a pool type the engine does not currently execute.

## Supported Mint Inputs

Mint input means the user or extension provides the token mint, and the engine derives or discovers the supported route.

- Pump.fun bonding curve tokens.
- Pump AMM post-migration tokens.
- LetsBonk launchpad tokens.
- LetsBonk post-migration Raydium-style routes.
- BagsApp Meteora DBC pre-migration tokens.
- BagsApp Meteora DAMM v2 post-migration tokens.

Raydium AMM v4 is not currently discovered from mint input alone. Submit the verified Raydium AMM v4 pool address for that route.

## Supported Pool Or Pair Inputs

Pool input means the user or extension provides the actual pool, pair, curve, or market account.

- Pump.fun bonding curve accounts.
- Pump AMM pools.
- LetsBonk launchpad pools.
- LetsBonk post-migration Raydium-style pools, including supported CLMM/CPMM variants used by Bonk routes.
- BagsApp Meteora DBC pools.
- BagsApp Meteora DAMM v2 pools.
- Raydium AMM v4 pools, when the pool is a supported WSOL pair and uses the expected Raydium/OpenBook account layout.

For Raydium AMM v4, the pool account must be owned by:

```text
675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8
```

The engine validates the pool mints, vaults, OpenBook market, market vault signer, and SPL Token program before compiling a trade.

## Trusted Stable Routes

The engine also has a small trusted stable allowlist for stable swaps. These are not generic pool support; they are fixed routes used for stable settlement/top-up flows.

Current trusted stable coverage includes:

- Orca Whirlpool SOL/USDC.
- Specific Raydium CLMM stable pools for SOL, USDC, USDT, and USD1 routes.

Arbitrary Orca Whirlpool pools are not supported just because the trusted stable path can invoke one known Orca pool.

## Not Currently Supported

These may appear as `pair` addresses on Axiom or other sites, but they are not generally executable by the engine today:

- Generic Orca Whirlpool token pools.
- Generic Raydium CLMM pools outside the supported Bonk and trusted stable paths.
- Generic Raydium CPMM pools outside the supported Bonk paths.
- Generic Meteora pools that are not BagsApp DBC or BagsApp DAMM v2.
- Unverified pool addresses whose owner/layout does not match a supported classifier.
- Token-2022 route variants unless the specific route family explicitly supports that mint program.

Support for more of these is planned. When adding a new pool family, the engine should add on-chain classification, route planning, native compilation, wrapper compatibility, and focused tests together.

## Axiom Pair Caveats

Axiom can provide a useful pair address, but the label does not tell us the executable family by itself.

Examples:

- A Raydium AMM v4 pair can be supported if it is the actual AMM v4 pool account.
- An Orca Whirlpool pair may be valid on-chain but unsupported for arbitrary token trading.
- A migrated launchpad token may have several pools; the engine should only trade the pool it can verify.

When debugging a pair, check the account owner first. The owner usually explains why the engine accepted or rejected the route.

## Adding More Support

The goal is to expand coverage over time without weakening route safety. New pool support should preserve:

- deterministic route classification;
- on-chain account and mint validation;
- exact account ordering expected by the venue program;
- wrapper behavior for fees and SOL/WSOL handling;
- focused buy and sell tests;
- clear operator errors when a pool is unsupported.
