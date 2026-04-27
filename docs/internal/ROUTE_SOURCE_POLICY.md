# Route Source Policy

Contributor reference for route discovery and config resolution.

Pump, Bonk, Bags, and explicit Raydium AMM v4 execution routing must be canonical and RPC-first.

Trade-path route discovery may use only:

- configured JSON-RPC endpoint
- deterministic PDA derivation
- shipped constants
- local caches whose entries were originally verified from RPC

## Disallowed In Trade-path Routing

- Raydium REST pool search for Pump/Bonk/Bags/Raydium v4 routing
- LaunchLab config endpoints for Pump/Bonk/Bags route discovery
- DexScreener, metadata APIs, or other third-party route/config sources
- live "best pool" scans that select by liquidity instead of canonical verification
- operator settings that silently permit non-canonical Pump, Bonk, or Bags pools

## Allowed Outside Route Discovery

- Bags public API launch/setup/fee-share/transaction flows
- Explicit Raydium AMM v4 pool inputs when the submitted address is verified by RPC owner/layout/mint/status checks
- transport providers such as Helius Sender, Hello Moon, and deferred provider paths
- Fee feeds such as Helius priority fee estimates and Jito tip feeds
- follow-daemon SOL/USD price sources
- VAMP/import enrichment, metadata/image upload, GitHub, IPFS, Arweave, and LaunchDeck metadata flows

## Trusted Stable Swaps

Trusted stable swaps are separate from route discovery.

They stay exact-pool allowlisted only:

- no hops
- no fallback pools
- no liquidity-based substitution

## Non-canonical Pool Handling

Non-canonical pool support must stay explicit. If a path allows a pinned non-canonical pool for research or specialized execution, it must:

- be opt-in
- be visible in settings/request state
- not change canonical default routing
- fail closed when verification is missing
