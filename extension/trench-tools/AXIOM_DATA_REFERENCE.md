# Axiom Data Reference

This is a contributor reference for Axiom adapter data. It is not an operator guide. Operator setup lives in [../../docs/EXTENSION.md](../../docs/EXTENSION.md).

## Normalized Token Fields

The content adapter tries to normalize Axiom page data into a route/token context with fields such as:

- `mint`
- `pairAddress`
- `tokenAddress`
- `routeAddress`
- `quotedPrice`
- `marketCapUsd`
- `liquidityUsd`
- `volumeUsd`
- `holders`
- `tokenTicker`
- `tokenName`
- `tokenImage`
- `sourceUrl`
- `originSurface`
- `canonicalSurface`
- `observedAtUnixMs`
- `detectedFrom`
- `pulseCardId`
- `viewTimestamp`
- `warmKey`
- `buyWarmKey`
- `sellWarmKey`

`pairAddress` and `routeAddress` are route hints. They are forwarded to the execution engine so Rust can classify the actual owner/layout. They are not browser-side execution authority.

## Surfaces

Current Axiom surfaces include:

- token detail page
- token-page instant trade panel
- floating token panel
- Pulse cards
- Pulse manual panel controls
- Pulse LaunchDeck toolbar entry
- Pulse and token-page Vamp helpers
- Pulse and token-page DexScreener shortcuts
- watchlist rows
- wallet tracker rows

`originSurface` should describe where the adapter first observed the data. `canonicalSurface` should normalize equivalent surfaces for host requests and metrics.

## Pulse Data

Pulse data can come from several places:

- rendered card links and row text
- copy-button/card ancestry
- Axiom page-world captures from `axiom-override.js`
- WebSocket or fetch response metadata captured by the override bridge
- React props or rendered-row data when available
- `localStorage` cache entries under Axiom-specific Pulse keys such as `axiom.pulse`

Useful Pulse fields include:

- card route link or pair route
- token mint/address
- pair address
- ticker/name/image
- quoted price and market-cap hints
- card id
- source URL

Pulse rows are dynamic. Adapter code should re-resolve live card data at click time rather than relying only on the first mounted snapshot.

## Token Detail Page

Token detail data can come from:

- route URL/path parameters
- page text
- instant-panel state
- Axiom localStorage/sessionStorage
- page-world bridge snapshots
- relevant links and script data collected by inspector/debug paths

Useful token detail fields include:

- token mint/address
- pair address
- ticker/name/image
- price/liquidity/volume/holder hints
- panel button mode
- instant trade panel modal position and size
- Trench Tools panel size/position
- selected Axiom wallet menu state where available

## Watchlist And Wallet Tracker

Watchlist and wallet-tracker controls are mounted from rendered rows and links. The adapter may derive:

- `mint`
- `tokenAddress`
- `pairAddress`
- `sourceUrl`
- `tokenTicker`
- `tokenName`
- `quotedPrice`
- row/card text hints

These surfaces are more heuristic than token detail pages. Always keep click-time re-resolution and clear fallback errors.

## Wallet Selection Storage

The extension stores shared quick-trade preferences under Trench Tools keys, especially:

- `trenchTools.panelPreferences`
- active preset id
- selection source
- active wallet group id
- manual wallet keys
- selection target
- quick-buy amount

Axiom token-detail wallet menu helpers mirror this selection so native-looking wallet controls, the popup, and the Trench Tools panel stay aligned.

## Prewarm Keys

The adapter may attach warm keys to route contexts:

- `warmKey`
- `buyWarmKey`
- `sellWarmKey`

Buy and sell can need different warm entries for migrated or quote-specific routes. Warm keys are hints for `/api/extension/prewarm`; final build still verifies route state.

## Site Feature Toggles

Axiom features are controlled through the extension site settings. Current feature groups include:

- trading buttons
- token-page panel/instant controls
- Pulse quick buy
- Pulse manual panel
- watchlist quick buy
- wallet-tracker quick buy
- LaunchDeck popout/shell entry
- Vamp helpers
- DexScreener shortcuts

Keep feature checks close to adapter mount code so disabled surfaces clean up their injected controls.

## Inspector And Debug Data

Inspector-only structures may include:

- `currentCandidate`
- `activeTokenContext`
- `platformSnapshot.page`
- `platformSnapshot.storage.local`
- `platformSnapshot.storage.session`
- `platformSnapshot.scripts`
- `platformSnapshot.pageSignals.metricHints`
- `platformSnapshot.pageSignals.keywordContexts`
- `platformSnapshot.pageSignals.relevantLinks`
- `platformSnapshot.pageSignals.addresses`
- `platformSnapshot.pageSignals.pageTextSample`
- `lastHostPayloads["trench:resolve-token"]`
- `lastHostPayloads["trench:preview-batch"]`
- `lastHostPayloads["trench:buy"]`
- `lastHostPayloads["trench:sell"]`

Do not document private reverse-engineering notes or sensitive platform details here. Keep this file focused on the normalized data contract and adapter-owned storage/surface behavior.
