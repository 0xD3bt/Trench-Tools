# Execution Do's And Don'ts

Contributor reference for changing Trench Tools execution paths.

This is not a first-run operator guide. Operator provider guidance lives in [../PROVIDERS.md](../PROVIDERS.md).

## Global Rules

Do:

- Treat transaction sending and confirmation as separate concerns.
- Build and sign transactions locally whenever the provider supports raw submission.
- Prefer `base64` when a provider allows it.
- Keep retry logic app-owned and blockhash-aware.
- Validate provider-specific tip/priority-fee requirements before submission.
- Keep endpoint selection region-aware.
- Keep read/confirm RPC separate from specialized send transport.
- Make MEV/protection modes explicit and route-specific.
- Reuse warm connections where the transport benefits from connection reuse.

Do not:

- Treat "request accepted" as "landed on-chain".
- Silently downgrade from a specialized provider into a weaker path.
- Use one provider's tip accounts with another provider.
- Assume batch APIs are bundle APIs.
- Reconnect for every send on connection-oriented transports.
- Emit transactions missing a tip or priority fee when the selected provider requires them.
- Re-sign a still-valid transaction just because confirmation is slow.

## Current Recommended Provider Work

The user-facing recommended providers are:

- `Helius Sender`
- `Hello Moon`

`Standard RPC` and `Jito Bundle` code paths are deferred until re-tested. Contributor changes may touch them, but setup docs should not recommend them as default operator paths.

## Helius Sender

Rules:

- Use Sender for low-latency execution.
- Include a compute-unit price instruction and provider tip where required.
- Set `skipPreflight=true`.
- Set `maxRetries=0`.
- Keep Sender warm with the supported ping path.
- Confirm through explicit RPC strategy after send.

Do not treat Sender as a generic RPC endpoint.

## Hello Moon

Rules:

- Reuse QUIC connections.
- Use one transaction per unidirectional stream.
- Treat QUIC as fire-and-forget and confirm through RPC afterward.
- Keep MEV protection route/connection-specific.
- Require a valid API key.

Do not expect a per-stream response body from QUIC.

## Deferred Provider Notes

`Standard RPC` and `Jito Bundle` remain implementation paths, but any public recommendation should stay deferred until they are re-tested.

If changing these paths:

- keep behavior explicit in UI/runtime
- keep provider requirements observable in reports/logs
- do not mix tip accounts between providers
- keep bundle semantics distinct from batch send semantics
- document any re-validation before moving them back into user-facing recommendations

## Fee Handling

- Distinguish provider tip, priority fee, Auto Fee cap, and voluntary support fee.
- Keep provider minimums centralized.
- Randomize provider-specific tip accounts where applicable.
- Do not use one universal minimum fee table for every provider.

## Confirmation And Observability

Every fire-and-forget path needs:

- selected provider/endpoint recorded
- sent signature recorded when available
- confirmation path recorded
- error/rejection reason surfaced
- enough detail for the operator to understand whether a request was accepted, sent, or landed

## Wrapper And Route Families

Wrapper v3 route-mode execution is the current wrapper path. When changing or adding a venue family:

- keep planner family, lifecycle, quote asset, and wrapper action in sync
- validate all inner programs through the wrapper allowlist
- keep ALT usage observable through diagnostics when relevant
- update native compile, wrapper compile, and route metrics together
- add focused buy/sell tests for the new route family

Do not add a route that can build natively but cannot pass wrapper validation on supported fee paths.

## Hard Don'ts

- Do not hide transport requirements behind UI defaults.
- Do not make fallback behavior invisible.
- Do not put private provider notes or reverse-engineering details in public docs.
- Do not add new required HTTP/IPC hops to hot paths unless there is a clear execution-safety reason.
