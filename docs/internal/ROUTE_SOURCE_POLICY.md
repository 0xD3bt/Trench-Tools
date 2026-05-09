# Route Source Policy

Contributor reference for route discovery and config resolution.

Pump, Bonk, LaunchLab, Raydium AMM v4, Raydium CPMM, Meteora DBC/DAMM v2, and trusted stable execution routing must be canonical and RPC-first.

Trade-path route discovery may use only:

- configured JSON-RPC endpoint
- deterministic PDA derivation
- shipped constants
- local caches whose entries were originally verified from RPC
- explicit user/site pool inputs after RPC owner/layout/mint validation

## Disallowed In Trade-path Routing

- Raydium REST pool search for Pump/Bonk/LaunchLab/Meteora/Raydium routing
- LaunchLab config endpoints as executable-market authority
- DexScreener, metadata APIs, or other third-party route/config sources
- live "best pool" scans that select by liquidity instead of canonical verification
- operator settings that silently permit non-canonical Pump, Bonk, LaunchLab, Raydium, or Meteora pools

## Allowed Outside Route Discovery

- Bags public API launch/setup/fee-share/transaction flows
- Explicit Raydium AMM v4 pool inputs when the submitted address is verified by RPC owner/layout/mint/status checks
- Explicit Raydium CPMM pool inputs when the submitted address is verified by RPC owner/layout/mint/status checks
- LaunchLab pool inputs and migrated LaunchLab routes when canonical state can be verified from RPC
- Meteora DBC/DAMM v2 launchpad state after RPC-derived market verification
- Bags API metadata, setup, and fee-share enrichment, as long as executable market selection remains RPC-derived
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

Trusted stable routes must remain an explicit allowlist. Adding a stable route means adding the exact route accounts, validation, wrapper compatibility, and tests together.

## Non-canonical Pool Handling

Non-canonical pool support must stay explicit. If a path allows a pinned non-canonical pool for research or specialized execution, it must:

- be opt-in
- be visible in settings/request state
- not change canonical default routing
- fail closed when verification is missing

Current operator-facing behavior should be described as fail-closed unless the source path and setting are both confirmed to permit the non-canonical route.
