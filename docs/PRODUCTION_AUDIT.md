# Production Audit

> Note: This is an internal/supporting audit document, not part of the primary operator documentation path. Use `README.md` and the core docs in `docs/` for current setup and usage guidance.

## Scope

This audit focuses on the production-readiness of the LaunchDeck backend, follow daemon, and launchpad helper bridge.

Primary files reviewed:

- `rust/launchdeck-engine/src/main.rs`
- `rust/launchdeck-engine/src/bin/launchdeck-follow-daemon.rs`
- `rust/launchdeck-engine/src/follow.rs`
- `rust/launchdeck-engine/src/rpc.rs`
- `rust/launchdeck-engine/src/report.rs`
- `rust/launchdeck-engine/src/observability.rs`
- `rust/launchdeck-engine/src/runtime.rs`
- `rust/launchdeck-engine/src/launchpads.rs`
- `rust/launchdeck-engine/src/bonk_native.rs`
- `rust/launchdeck-engine/src/bags_native.rs`
- `scripts/bonk-launchpad.js`
- `scripts/bags-launchpad.js`

## Assumptions

This report is written against the current product intent:

- LaunchDeck is primarily a trusted localhost tool, not a hardened multi-tenant service.
- Sending sensitive runtime values from the frontend to the backend is acceptable when needed for speed and UX.
- The main concern is not that the backend temporarily sees secrets, but that it should avoid unnecessarily echoing, persisting, or reporting them.

Because of that, findings are prioritized around correctness, reliability, performance, and operational safety first. Security items that only matter under a stronger trust model are called out separately.

## Executive Summary

The highest-value work is not a rewrite. It is a focused hardening pass on the execution lifecycle:

1. Fix send/result semantics so the API clearly reports what succeeded on-chain and what failed after the fact.
2. Fix follow-daemon watcher and cancellation edge cases that can leave jobs hanging or cancel more work than intended.
3. Make state and report persistence crash-safe and explicit on failure.
4. Add proper timeout and concurrency discipline around the JS helper bridge and hot polling paths.
5. Redact sensitive runtime fields from responses and reports even if frontend-to-backend secret flow remains allowed.

The current codebase is functional, but several behaviors are still prototype-grade for a real production product:

- launches can be submitted successfully while the API still returns failure
- follow jobs can hang forever on some watcher failures
- targeted action cancellation can behave like whole-job cancellation
- JSON persistence can be torn or silently reset on corruption
- helper subprocesses and market polling paths are more expensive and less isolated than they should be

## Production-Blocking Or Near-Blocking Findings

### 1. Post-send failures are reported as top-level request failures

Severity: Critical

Problem:

In `rust/launchdeck-engine/src/main.rs`, the send path can successfully submit the launch transaction and then still return an error if a later step fails, such as:

- follow-daemon arming
- final persisted report update

Why this matters:

- users or the UI can interpret the request as a failed launch
- retries can create duplicate sends, duplicate launches, duplicate dev-buys, or duplicate automation
- the API contract becomes misleading at the exact point where the operation is least reversible

Recommended fix:

- once any irreversible send has succeeded, return success at the top level
- attach raw structured details for each phase:
  - launch built
  - launch sent
  - launch signatures
  - report persisted
  - follow daemon reserved
  - follow daemon armed
- also include a plain-language summary such as:
  - `Launch was submitted successfully, but follow actions were not armed.`

Suggested response shape:

```json
{
  "ok": true,
  "launchSubmitted": true,
  "launchSignature": "5abc...",
  "followDaemonArmed": false,
  "warnings": [
    "Launch was sent successfully, but follow automation could not be armed."
  ],
  "plainSummary": "Launch succeeded. Follow automation failed."
}
```

### 2. Some follow-daemon watcher failures can leave actions hanging forever

Severity: Critical

Problem:

In `rust/launchdeck-engine/src/bin/launchdeck-follow-daemon.rs`, slot and market watcher loops can stop without publishing a terminal failure into the watch channel. That means waiting actions can block indefinitely instead of failing clearly.

Why this matters:

- jobs appear stuck rather than failed
- users get poor feedback and may not know whether to retry or cancel
- production monitoring becomes much harder because work is neither progressing nor exiting

Recommended fix:

- make all watcher channels carry terminal `Result`-style state, not just live values
- publish terminal failure before watcher exit
- ensure waiters fail fast with a clear reason
- remove or replace dead watcher hubs after terminal failure so later waits do not inherit stale state

### 3. Action-level cancellation is not isolated enough

Severity: High

Problem:

In `rust/launchdeck-engine/src/follow.rs`, canceling a single action can escalate into whole-job cancellation because job-level cancellation state is reused too broadly.

Why this matters:

- one bad action can stop unrelated sibling actions
- targeted remediation becomes unsafe
- users lose trust in cancel behavior because it is broader than requested

Recommended fix:

- separate whole-job cancellation from action-scoped cancellation
- keep action cancellation local to the targeted action
- only set `job.cancelRequested` for true job-wide cancel operations

### 4. Persistence is not crash-safe

Severity: High

Problem:

Several persistence paths write JSON directly to the live file with `fs::write`, including state/config/report related modules such as:

- `rust/launchdeck-engine/src/follow.rs`
- `rust/launchdeck-engine/src/runtime.rs`
- `rust/launchdeck-engine/src/ui_config.rs`
- `rust/launchdeck-engine/src/observability.rs`
- `rust/launchdeck-engine/src/main.rs` Bags credential persistence

Some load paths also silently fall back to empty/default state on corruption.

Why this matters:

- torn writes can corrupt state on crash or process kill
- silent reset on parse failure can erase jobs or state without an explicit alert
- operators may think the system recovered cleanly when it actually dropped important data

Recommended fix:

- switch to temp-file write plus rename for all persisted JSON
- preserve corrupt files for inspection instead of replacing them silently
- surface degraded startup state when persistence restore fails
- stop silently swallowing write errors in runtime/state persistence paths

## High-Value Reliability And Performance Findings

### 5. The JS helper bridge blocks more than it should

Severity: High

Problem:

`rust/launchdeck-engine/src/bonk_native.rs` and `rust/launchdeck-engine/src/bags_native.rs` spawn `node` and synchronously wait for completion from async code, with no hard timeout and no kill-on-timeout path.

Why this matters:

- hung helper calls can pin runtime threads
- upstream RPC/SDK/API stalls can turn into backend stalls
- this creates weak failure isolation between one launchpad integration and the rest of the engine

Recommended fix:

- move helper execution to `tokio::process`
- enforce per-action timeouts
- kill helpers on timeout
- add concurrency limits around helper invocations

Longer term:

- either move hot paths to Rust or use a warm long-lived helper process with a typed RPC contract rather than spawn-per-call

### 6. Market-watcher hot paths are expensive and fragile

Severity: High

Problem:

The follow daemon repeatedly shells out into helper logic for some market snapshot paths, especially on Bonk and Bags market polling.

Why this matters:

- process startup overhead accumulates on the hottest loops
- helper bootstrapping repeats work constantly
- API/RPC rate-limit pressure is higher than necessary
- this makes latency and reliability worse exactly where automation needs to be stable

Recommended fix:

- remove process-per-poll behavior
- poll directly in Rust where practical, or keep a warm helper runtime
- centralize market polling cadence and backoff
- add per-job and global budgets for hot refresh/poll loops

### 7. Network client usage is not optimized for stability

Severity: Medium-High

Problem:

Multiple paths build fresh `reqwest::Client` instances repeatedly rather than reusing shared clients with clear timeout discipline, especially in:

- `rust/launchdeck-engine/src/rpc.rs`
- `rust/launchdeck-engine/src/follow.rs`
- `rust/launchdeck-engine/src/main.rs` Bags API helper functions

Why this matters:

- extra connection churn
- poorer behavior under latency spikes
- harder control over timeouts and retry behavior

Recommended fix:

- create shared clients per upstream class:
  - RPC
  - Jito
  - follow-daemon client
  - Bags API
- configure connect, read, and total request timeouts
- standardize retry/backoff rules

### 8. Concurrency controls are incomplete

Severity: Medium-High

Problem:

The follow daemon has concurrency knobs, but some heavy paths still bypass or weaken them:

- reservation/admission can oversubscribe in racey edge cases
- hot follow-buy refresh loops can multiply per job
- some precompute/cache warmup paths are not gated by the same compile/send limits

Why this matters:

- load spikes can bypass intended ceilings
- CPU and RPC pressure become less predictable
- the system becomes harder to tune safely

Recommended fix:

- enforce admission limits inside the serialized reservation/store path
- put cache warmups and hot refresh loops behind semaphores or a shared scheduler
- add metrics for active jobs, active watchers, active refresh loops, and helper invocations

### 9. Wallet locking is broader than necessary

Severity: Medium

Problem:

Some wallet locks are held across long network paths including compile, send, and confirm phases.

Why this matters:

- one slow action can block unrelated work for the same wallet for too long
- this reduces throughput and increases tail latency

Recommended fix:

- narrow the critical section where possible
- if broad locking is required to avoid duplicate or conflicting sends, add tighter timeout and cancellation handling inside the locked section

### 10. Job lookup and watcher bookkeeping are heavier than necessary

Severity: Medium

Problem:

Some hot paths repeatedly clone job lists or use global watcher health in ways that do not scale cleanly under load.

Why this matters:

- extra allocator and lock churn in latency-sensitive loops
- daemon-level health can look degraded due to one noisy job

Recommended fix:

- add indexed job lookup by `traceId`
- reduce full-list cloning in hot loops
- track watcher health per job and per subsystem more explicitly

## Product-Model-Compatible Secret Handling Findings

These are intentionally framed around your current trust model.

### 11. Sensitive runtime values are echoed back more than necessary

Severity: Medium

Problem:

Even if frontend-to-backend secret flow is acceptable, the backend currently reflects sensitive fields more than necessary:

- `normalizedConfig` is returned from execution paths in `rust/launchdeck-engine/src/main.rs`
- Bags identity endpoints return `authToken`
- Bags credentials are stored on disk in plaintext JSON

Why this still matters:

- it is unnecessary exposure, even on a trusted localhost product
- it increases accidental leakage into browser tooling, logs, screenshots, bug reports, and future integrations
- it makes later hardening harder because the API contract already assumes secret reflection

Recommended fix:

- keep accepting runtime secrets from the frontend if needed
- stop returning them back in normal responses
- redact sensitive fields from:
  - `normalizedConfig`
  - execution responses
  - reports/history payloads
  - debug/log output
- keep Bags auth/API material out of persisted launch reports and history cards

This is the right middle ground for the current product model:

- backend may temporarily receive these values
- backend should not unnecessarily re-emit them

### 12. Auth on localhost APIs is weak for any future expansion

Severity: Conditional

Problem:

The current auth posture relies on localhost trust and permissive API exposure. That may be acceptable today, but it is not ready for:

- remote access
- team/shared workstation use
- browser extensions or local malware threat models
- reverse proxy or container exposure

Recommended fix:

- not urgent if the tool stays strictly single-user localhost
- important before any broader deployment model

## Helper-Specific Findings

### 13. Helper input validation fails open in a few places

Severity: Medium

Problem:

Some helper paths silently coerce or default invalid enum-like inputs rather than rejecting them:

- unknown quote assets or modes can fall back to defaults
- some Bags mode mapping falls back to a default config type

Why this matters:

- it hides contract drift between UI, Rust, and JS
- wrong behavior can look like “successful execution”

Recommended fix:

- reject unknown launchpad-specific enums explicitly
- make helper input validation strict and fail early

### 14. Numeric handling in some helper flows is brittle

Severity: Medium

Problem:

Some helper numeric conversions use number-like intermediate handling where large values or unusual market conditions could cause precision issues or failure.

Recommended fix:

- keep large values as `BN` or `BigInt` end-to-end
- remove avoidable `toNumber()` style conversions in critical market/trade calculations

## Recommended Remediation Tracks

### Track 1: Result Semantics And UX

- redesign send/build/simulate responses to be phase-based
- show both raw machine-readable status and plain-language summary
- never report a post-send partial failure as if the send itself failed

### Track 2: Follow-Daemon Correctness

- fix watcher terminal-state propagation
- isolate action cancellation from job cancellation
- improve trigger anchoring and watcher lifecycle cleanup
- add failure-injection tests around stuck or failed watcher paths

### Track 3: Persistence Hardening

- atomic writes for all JSON persistence
- preserve corrupt files for inspection
- explicit degraded startup state on restore failure
- stop swallowing write failures

### Track 4: Bridge And Hot-Path Performance

- timeouts and kill-on-timeout for helpers
- `tokio::process` instead of blocking process waits
- shared HTTP clients and consistent timeout policy
- remove process-per-poll market snapshot paths
- enforce global budgets around refreshers and watchers

### Track 5: Redaction Without Changing Product UX

- allow frontend-provided vanity/Bags values at runtime
- redact them from normal responses
- keep them out of reports/history/logging
- avoid reflecting full `normalizedConfig` back to clients

## Suggested Prioritization

### Immediate

1. Fix send/result semantics.
2. Fix watcher terminal failure handling.
3. Fix action-vs-job cancellation behavior.
4. Make persistence atomic and explicit on failure.

### Next

5. Add helper timeouts and async-safe process handling.
6. Reduce process-per-poll overhead in market watcher paths.
7. Reuse shared network clients with stronger timeout discipline.

### After That

8. Redact sensitive runtime fields from responses and reports.
9. Tighten localhost auth posture if the deployment model expands.
10. Optimize hot-path job lookup and watcher health accounting.

## Open Questions Requiring Live Validation

- Exact Bags auth request/response behavior under real account flows.
- Provider behavior when launch submission succeeds but confirmation/report/follow-daemon steps fail.
- Real CPU and RPC cost of current hot refresh and market polling under concurrent live usage.
- Timeout and retry behavior during Helius, Jito, Bags, or websocket degradation.
- Whether any third-party SDK exceptions include data that should be additionally redacted before surfacing.

## Practical Bottom Line

The most important work is not “enterprise security theater.” It is about making LaunchDeck honest and predictable under stress:

- if a launch went out, the API must say so
- if automation failed, the API must say exactly which part failed
- if a watcher dies, jobs must fail clearly instead of hanging
- if the process crashes during a write, state should recover safely

Those changes materially improve production quality even if the product remains a fast localhost-first tool with frontend-supplied sensitive runtime values.
