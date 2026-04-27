# Axiom Usable Data

## Normalized fields

- `mint`
- `pairAddress`
- `tokenAddress`
- `quotedPrice`
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

## Token detail page

- page route value: `pairAddress`
- page text: `quotedPrice`
- page text: `liquidityUsd`
- page text: `volumeUsd`
- page text: `holders`
- page text: `tokenAddress`
- page text: `pairAddress`
- page text: `tokenTicker`
- page text: `tokenName`

## Pulse page

- pulse card link: `mint`
- pulse card link: `tokenAddress`
- pulse card text: `quotedPrice`
- pulse card text: `marketCapUsd`
- pulse card text: `tokenTicker`
- pulse card text: `tokenName`
- pulse card id: `pulseCardId`
- pulse/watchlist link: `sourceUrl`

## Watchlist

- watchlist link: `mint`
- watchlist link: `tokenAddress`
- watchlist link: `sourceUrl`
- watchlist row text: `tokenTicker`
- watchlist row text: `quotedPrice`

## Wallet tracker

- wallet tracker link: `mint`
- wallet tracker link: `tokenAddress`
- wallet tracker link: `sourceUrl`
- wallet tracker row text: `tokenTicker`
- wallet tracker row text: `tokenName`

## localStorage

- `recentTickerSol[].pairAddress`
- `recentTickerSol[].tokenAddress`
- `recentTickerSol[].tokenTicker`
- `recentTickerSol[].tokenName`
- `recentTickerSol[].tokenImage`
- `recentTickerSol[].viewTimestamp`
- `LocalStorageSaveLoadAdapter_charts_v4[].content.short_name`
- `LocalStorageSaveLoadAdapter_charts_v4[].content.symbol`
- `LocalStorageSaveLoadAdapter_charts_v4[].content.resolution`
- `LocalStorageSaveLoadAdapter_charts_v4[].timestamp`

## Inline page scripts

- `params.pairAddress`
- page route key

## Inspector-only captured structures

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
