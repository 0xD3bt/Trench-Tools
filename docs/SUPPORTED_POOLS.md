# Supported Pools And Routes

This page describes what the local `execution-engine` can trade today through native execution. It is intentionally current-state documentation, not a permanent limit. More pools, venues, and route types will be added over time.

## What "Supported" Means

The engine only trades a token or pool after it can verify the on-chain account owner and account layout for a known route family.

An address shown by a site as a `pair` is not automatically executable. On Axiom, `pair` usually means "the pool address Axiom is displaying", and that pool may be Raydium, Orca, Meteora, Pump, Bags, Bonk, LaunchLab, or something else. The engine still has to classify it as one of the supported route families below.

If the UI says:

```text
Address ... is not a token mint or supported pool/pair account.
```

then the address either was not found on-chain, was not a token mint, or was a pool type the engine does not currently execute.

## Supported Mint Inputs

Mint input means the user or extension provides the token mint, and the engine derives or discovers the supported canonical route.

- Pump.fun bonding curve tokens.
- Pump AMM post-migration tokens.
- LetsBonk launchpad tokens.
- LetsBonk post-migration Raydium-style routes, including supported CLMM/CPMM variants used by Bonk.
- Raydium LaunchLab SOL launch pools while active.
- Migrated LaunchLab tokens when the engine can prove the canonical Raydium AMM v4 or CPMM SOL pool.
- Meteora DBC pre-migration launchpad routes.
- Meteora DAMM v2 post-migration launchpad routes.

Raydium AMM v4 and CPMM are not treated as generic "discover any best pool" mint routes. When you want a specific standalone Raydium pool, submit the verified pool address.

## Supported Pool Or Pair Inputs

Pool input means the user or extension provides the actual pool, pair, curve, or market account.

- Pump.fun bonding curve accounts.
- Pump AMM pools.
- LetsBonk launchpad pools.
- LetsBonk post-migration Raydium-style pools, including supported CLMM/CPMM variants used by Bonk routes.
- Raydium LaunchLab SOL pools.
- Raydium AMM v4 pools, when the pool is a supported WSOL pair and uses the expected Raydium/OpenBook account layout.
- Raydium CPMM pools, when the pool is a supported WSOL pair and passes RPC owner/layout/mint validation.
- Meteora DBC launchpad pools.
- Meteora DAMM v2 launchpad pools.

For Raydium AMM v4, the pool account must be owned by:

```text
675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8
```

The engine validates the pool mints, vaults, market accounts, token programs, and route-specific account layout before compiling a trade.

## Canonical And Pinned Pools

Default routing is canonical. If the engine can derive or prove the canonical market for a token, it uses that market rather than selecting a pool by liquidity or a third-party "best pool" result.

A pair/pool address from Axiom can be useful because it lets the engine classify the exact account. That does not bypass safety checks:

- a pinned Pump AMM pool must match the canonical Pump AMM pool unless the non-canonical policy explicitly allows it
- migrated LaunchLab routes must prove the canonical migrated Raydium AMM v4 or CPMM pool
- Meteora DBC/DAMM v2 routes must be verified from RPC-derived state
- unverified or unsupported pair accounts fail closed

The current operator-safe behavior is fail-closed for non-canonical pools. Do not rely on non-canonical pool trading unless the setting and source path are both explicitly documented as enabled.

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
- Generic Raydium CPMM pools that are not supported WSOL pool inputs or verified launchpad/migration routes.
- Generic Meteora pools that are not supported DBC or DAMM v2 launchpad routes.
- Unverified pool addresses whose owner/layout does not match a supported classifier.
- Token-2022 route variants unless the specific route family explicitly supports that mint program.

Support for more of these is planned. When adding a new pool family, the engine should add on-chain classification, route planning, native compilation, wrapper compatibility, and focused tests together.

## Route Diagnostics

Execution logs include route metrics when planning or compiling routes. Useful log prefixes include:

```text
[execution-engine][route-metrics] phase=plan
[execution-engine][route-metrics] phase=compile
```

Those lines can show elapsed time, RPC method counts, and route family/lifecycle labels. Warm/prewarm behavior is best-effort; if a warm route is stale or invalidated after a trade, the next trade can plan fresh.

## Axiom Pair Caveats

Axiom can provide a useful pair address, but the label does not tell us the executable family by itself.

Examples:

- A Raydium AMM v4 pair can be supported if it is the actual AMM v4 pool account.
- A Raydium CPMM pair can be supported if it is a verified WSOL CPMM pool account.
- A LaunchLab pair can be supported while active, or after migration if the canonical Raydium pool can be proven.
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
