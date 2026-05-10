// balances-store.js
// Single source of truth for wallet SOL / USD1 balances across all extension surfaces.
// Stream-first: the background runs a persistent SSE consumer (events-client.js) and
// feeds balance / connection-state / trade events into this store. The HTTP path
// (fetchWalletStatus) is used only for cold-start and as a fallback when the stream is
// not live. Surfaces subscribe via chrome.storage change events on three revision keys.

import { fetchWalletStatus } from "./execution-client.js";

const WALLET_STATUS_REVISION_KEY = "trenchTools.walletStatusRevision";
const WALLET_STATUS_META_KEY = "trenchTools.walletStatusMeta";
const WALLET_STATUS_DIFF_KEY = "trenchTools.walletStatusDiff";
const WALLET_STATUS_MARK_REVISION_KEY = "trenchTools.walletStatusMarkRevision";
const WALLET_STATUS_MARK_DIFF_KEY = "trenchTools.walletStatusMarkDiff";
const LAST_TRADE_EVENT_KEY = "trenchTools.lastTradeEvent";
const BATCH_STATUS_EVENT_KEY = "trenchTools.lastBatchStatusEvent";
const BATCH_STATUS_REVISION_KEY = "trenchTools.batchStatusRevision";

const STORE = {
  balances: new Map(),
  tokenBalances: new Map(),
  tokenBalanceRaws: new Map(),
  tokenDecimals: new Map(),
  balanceSlots: new Map(),
  tokenSlots: new Map(),
  markRevisions: new Map(),
  storageRevision: Date.now(),
  lastFetchAt: 0,
  lastEventAt: 0,
  lastError: null,
  inFlight: null,
  connection: {
    state: "connecting",
    error: null,
    sinceMs: Date.now(),
  },
  scheduledRetries: new Set(),
};

function numberOrNull(value) {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function slotOrNull(value) {
  return Number.isInteger(value) && value >= 0 ? value : null;
}

function revisionOrNull(value) {
  return Number.isInteger(value) && value >= 0 ? value : null;
}

function nextStorageRevision() {
  STORE.storageRevision = Math.max(Date.now(), STORE.storageRevision + 1);
  return STORE.storageRevision;
}

function normalizeEntry(row) {
  return {
    balanceSol: numberOrNull(row?.balanceSol),
    balanceLamports: numberOrNull(row?.balanceLamports),
    usd1Balance: numberOrNull(row?.usd1Balance),
    balanceError:
      typeof row?.balanceError === "string" && row.balanceError.length > 0
        ? row.balanceError
        : null,
    publicKey: typeof row?.publicKey === "string" ? row.publicKey : null,
    commitment: typeof row?.commitment === "string" ? row.commitment : null,
    source: typeof row?.source === "string" ? row.source : null,
    slot: slotOrNull(row?.slot),
    fetchedAt: Date.now(),
  };
}

function mergeEntry(prev, next) {
  if (!prev) return { ...next };
  const merged = { ...prev };
  if (next.balanceSol != null) merged.balanceSol = next.balanceSol;
  if (next.balanceLamports != null) merged.balanceLamports = next.balanceLamports;
  if (next.usd1Balance != null) merged.usd1Balance = next.usd1Balance;
  if (next.publicKey != null) merged.publicKey = next.publicKey;
  if (next.commitment != null) merged.commitment = next.commitment;
  if (next.source != null) merged.source = next.source;
  if (next.slot != null) merged.slot = next.slot;
  if (next.balanceError !== undefined) merged.balanceError = next.balanceError;
  merged.fetchedAt = next.fetchedAt || Date.now();
  return merged;
}

function tokenBalanceKey(envKey, mint) {
  return `${String(envKey || "").trim()}::${String(mint || "").trim()}`;
}

function balanceSlotKey(envKey, field) {
  return `${String(envKey || "").trim()}::${field}`;
}

function shouldApplySlot(slotMap, key, slot) {
  if (slot == null) return true;
  const previous = slotMap.get(key);
  if (previous != null && slot < previous) {
    return false;
  }
  slotMap.set(key, slot);
  return true;
}

function tokenBalanceFromRow(row) {
  return numberOrNull(
    row?.tokenBalance ??
    row?.mintBalanceUi ??
    row?.mintBalance
  );
}

function tokenBalanceRawFromRow(row) {
  return numberOrNull(row?.tokenBalanceRaw ?? row?.mintBalanceRaw);
}

function tokenDecimalsFromRow(row) {
  const decimals = row?.tokenDecimals ?? row?.mintDecimals;
  return Number.isInteger(decimals) && decimals >= 0 ? decimals : null;
}

function walletBalanceDiffEntry(envKey, entry) {
  const diff = { envKey };
  if (entry?.balanceSol != null) diff.balanceSol = entry.balanceSol;
  if (entry?.balanceLamports != null) diff.balanceLamports = entry.balanceLamports;
  if (entry?.usd1Balance != null) diff.usd1Balance = entry.usd1Balance;
  diff.balanceError = entry?.balanceError ?? null;
  if (entry?.commitment) diff.commitment = entry.commitment;
  if (entry?.source) diff.source = entry.source;
  if (entry?.slot != null) diff.slot = entry.slot;
  return diff;
}

function markSlotKey(event) {
  const surface = String(event?.surfaceId || "").trim();
  const mint = String(event?.mint || "").trim();
  const walletKeys = Array.isArray(event?.walletKeys)
    ? event.walletKeys.map((key) => String(key || "").trim()).filter(Boolean).sort().join(",")
    : "";
  const group = String(event?.walletGroupId || "").trim();
  return `${surface}::${mint}::${group}::${walletKeys}`;
}

function shouldApplyMarkRevision(revisionMap, key, revision) {
  if (revision == null) return true;
  const previous = revisionMap.get(key);
  if (previous != null && revision <= previous) {
    return false;
  }
  revisionMap.set(key, revision);
  return true;
}

function snapshotFromStore() {
  const entries = [];
  for (const [envKey, entry] of STORE.balances.entries()) {
    entries.push({ envKey, ...entry });
  }
  return {
    balances: entries,
    lastFetchAt: STORE.lastFetchAt,
    lastEventAt: STORE.lastEventAt,
    lastError: STORE.lastError,
    meta: getMeta(),
  };
}

export function getBalancesSnapshot() {
  return snapshotFromStore();
}

export function getMeta() {
  return {
    state: STORE.connection.state,
    error: STORE.connection.error,
    sinceMs: STORE.connection.sinceMs,
    lastEventAt: STORE.lastEventAt,
    connected: STORE.connection.state === "live",
  };
}

/**
 * Called by events-client when the SSE endpoint delivers its initial
 * `snapshot` event. Replaces the entire balance map and broadcasts.
 */
export function applyServerSnapshot(snapshot) {
  if (!snapshot || typeof snapshot !== "object") return;
  const wallets = Array.isArray(snapshot.wallets) ? snapshot.wallets : [];
  const nextMap = new Map();
  for (const wallet of wallets) {
    const key = wallet?.envKey || wallet?.key;
    if (!key) continue;
    nextMap.set(key, normalizeEntry(wallet));
  }
  STORE.balances = nextMap;
  STORE.tokenBalances.clear();
  STORE.tokenBalanceRaws.clear();
  STORE.tokenDecimals.clear();
  STORE.balanceSlots.clear();
  STORE.tokenSlots.clear();
  STORE.markRevisions.clear();
  STORE.lastEventAt = Date.now();
  STORE.lastError = null;
  const changedKeys = Array.from(nextMap.keys());
  broadcastUpdated({
    changedKeys,
    walletBalanceEntries: changedKeys.map((key) => walletBalanceDiffEntry(key, nextMap.get(key))),
  });
  broadcastMeta();
}

/**
 * Called by events-client for every live `balance` event. Merges the partial
 * update into the existing entry (so a SOL-only notification doesn't wipe
 * USD1 and vice versa) and broadcasts the changed key.
 */
export function applyServerBalanceEvent(event) {
  if (!event || typeof event !== "object") return;
  const envKey = event.envKey;
  if (!envKey) return;
  const tokenMint = typeof event.tokenMint === "string" ? event.tokenMint.trim() : "";
  const tokenBalance = numberOrNull(event.tokenBalance);
  const tokenBalanceRaw = numberOrNull(event.tokenBalanceRaw);
  const tokenDecimals = tokenDecimalsFromRow(event);
  const eventSlot = slotOrNull(event.slot);
  const eventCommitment = typeof event.commitment === "string" ? event.commitment : null;
  const eventSource = typeof event.source === "string" ? event.source : null;
  const prev = STORE.balances.get(envKey);
  const balanceSol = numberOrNull(event.balanceSol);
  const balanceLamports = numberOrNull(event.balanceLamports);
  const usd1Balance = numberOrNull(event.usd1Balance);
  const hasSolUpdate = balanceSol != null || balanceLamports != null;
  const solSlotOk =
    !hasSolUpdate ||
    shouldApplySlot(STORE.balanceSlots, balanceSlotKey(envKey, "sol"), eventSlot);
  const usd1SlotOk =
    usd1Balance == null ||
    shouldApplySlot(STORE.balanceSlots, balanceSlotKey(envKey, "usd1"), eventSlot);
  const tokenSlotOk =
    !tokenMint ||
    (tokenBalance == null && tokenBalanceRaw == null && tokenDecimals == null) ||
    shouldApplySlot(STORE.tokenSlots, tokenBalanceKey(envKey, tokenMint), eventSlot);
  const balanceKey = tokenBalanceKey(envKey, tokenMint);
  const previousTokenBalance =
    tokenMint && tokenBalance != null && tokenSlotOk
      ? STORE.tokenBalances.get(balanceKey)
      : undefined;
  const previousTokenBalanceRaw =
    tokenMint && tokenBalanceRaw != null && tokenSlotOk
      ? STORE.tokenBalanceRaws.get(balanceKey)
      : undefined;
  const previousTokenDecimals =
    tokenMint && tokenDecimals != null && tokenSlotOk
      ? STORE.tokenDecimals.get(balanceKey)
      : undefined;
  const tokenChanged =
    tokenMint &&
    tokenSlotOk &&
    (
      (tokenBalance != null && previousTokenBalance !== tokenBalance) ||
      (tokenBalanceRaw != null && previousTokenBalanceRaw !== tokenBalanceRaw) ||
      (tokenDecimals != null && previousTokenDecimals !== tokenDecimals)
    );
  const hasAppliedBalanceField =
    (solSlotOk && hasSolUpdate) || (usd1SlotOk && usd1Balance != null);
  const partial = {
    balanceSol: solSlotOk ? balanceSol : null,
    balanceLamports: solSlotOk ? balanceLamports : null,
    usd1Balance: usd1SlotOk ? usd1Balance : null,
    publicKey: prev?.publicKey ?? null,
    commitment: hasAppliedBalanceField ? eventCommitment : null,
    source: hasAppliedBalanceField ? eventSource : null,
    slot: hasAppliedBalanceField ? eventSlot : null,
    balanceError: hasAppliedBalanceField ? null : undefined,
    fetchedAt: Date.now(),
  };
  const next = mergeEntry(prev, partial);
  const balanceChanged =
    hasAppliedBalanceField &&
    (!prev || !sameTrackedFields(prev, next) || prev.publicKey !== next.publicKey);
  STORE.lastEventAt = Date.now();
  // Skip the broadcast when the incoming notification didn't actually change
  // any tracked field. Solana resends state on reconnect / subscription ack
  // and the UI shouldn't tear down rows for a no-op.
  if (!balanceChanged && !tokenChanged) {
    if (hasAppliedBalanceField || prev) {
      STORE.balances.set(envKey, next);
    }
    return;
  }
  if (tokenMint && tokenBalance != null && tokenSlotOk) {
    STORE.tokenBalances.set(balanceKey, tokenBalance);
  }
  if (tokenMint && tokenBalanceRaw != null && tokenSlotOk) {
    STORE.tokenBalanceRaws.set(balanceKey, tokenBalanceRaw);
  }
  if (tokenMint && tokenDecimals != null && tokenSlotOk) {
    STORE.tokenDecimals.set(balanceKey, tokenDecimals);
  }
  STORE.balances.set(envKey, next);
  broadcastUpdated({
    changedKeys: [envKey],
    changedMints: tokenChanged ? [tokenMint] : [],
    tokenBalanceEntries: tokenChanged
      ? [
          {
            envKey,
            mint: tokenMint,
            tokenBalance,
            tokenBalanceRaw,
            tokenDecimals,
            commitment: eventCommitment,
            source: eventSource,
            slot: eventSlot,
          },
        ]
      : [],
    walletBalanceEntries: balanceChanged ? [walletBalanceDiffEntry(envKey, next)] : [],
  });
}

export function applyServerMarkEvent(event) {
  if (!event || typeof event !== "object") return;
  const mint = String(event.mint || "").trim();
  if (!mint) return;
  const eventSlot = slotOrNull(event.slot);
  const markRevision = revisionOrNull(event.markRevision);
  if (!shouldApplyMarkRevision(STORE.markRevisions, markSlotKey(event), markRevision)) {
    return;
  }
  STORE.lastEventAt = Date.now();
  try {
    chrome.storage.local.set({
      [WALLET_STATUS_MARK_REVISION_KEY]: nextStorageRevision(),
      [WALLET_STATUS_MARK_DIFF_KEY]: {
        surfaceId: typeof event.surfaceId === "string" ? event.surfaceId : null,
        markRevision,
        mint,
        walletKeys: Array.isArray(event.walletKeys) ? event.walletKeys : [],
        walletGroupId: typeof event.walletGroupId === "string" ? event.walletGroupId : null,
        tokenBalance: numberOrNull(event.tokenBalance),
        tokenBalanceRaw: numberOrNull(event.tokenBalanceRaw),
        holdingValueSol: numberOrNull(event.holdingValueSol),
        holding: numberOrNull(event.holding),
        pnlGross: numberOrNull(event.pnlGross),
        pnlNet: numberOrNull(event.pnlNet),
        pnlPercentGross: numberOrNull(event.pnlPercentGross),
        pnlPercentNet: numberOrNull(event.pnlPercentNet),
        quoteSource: typeof event.quoteSource === "string" ? event.quoteSource : null,
        commitment: typeof event.commitment === "string" ? event.commitment : null,
        slot: eventSlot,
        at: Date.now(),
      },
    });
  } catch (_error) {}
}

function sameTrackedFields(a, b) {
  return (
    a.balanceSol === b.balanceSol &&
    a.balanceLamports === b.balanceLamports &&
    a.usd1Balance === b.usd1Balance &&
    a.balanceError === b.balanceError
  );
}

/**
 * Called by events-client whenever the stream transitions between
 * connecting / live / disconnected states. Surfaces read this via the
 * walletStatusMeta storage key.
 */
export function applyServerConnectionState({ state, error }) {
  const nextState = typeof state === "string" ? state : "connecting";
  const nextError = typeof error === "string" ? error : null;
  if (
    STORE.connection.state === nextState &&
    STORE.connection.error === nextError
  ) {
    return;
  }
  STORE.connection = {
    state: nextState,
    error: nextError,
    sinceMs: Date.now(),
  };
  broadcastMeta();
  if (nextState === "disconnected") {
    // Kick a single HTTP fallback so surfaces aren't left with stale data
    // while the stream is reconnecting.
    void fetchBalances({ force: false });
  }
}

/**
 * Called by events-client when a `trade` event arrives. Writes the latest
 * event to chrome.storage so the content panel (the only surface that cares
 * about trade transitions) can match by clientRequestId.
 */
export function handleTradeEvent(event) {
  if (!event || typeof event !== "object") return;
  try {
    chrome.storage.local.set({
      [LAST_TRADE_EVENT_KEY]: {
        ...event,
        receivedAt: Date.now(),
      },
    });
  } catch (_error) {}
}

export function handleBatchStatusEvent(event) {
  if (!event || typeof event !== "object") return;
  const snapshot = event.snapshot && typeof event.snapshot === "object"
    ? event.snapshot
    : event;
  const batchId = String(snapshot?.batchId || event.batchId || "").trim();
  if (!batchId) return;
  const receivedAt = Date.now();
  try {
    chrome.storage.local.set({
      [BATCH_STATUS_EVENT_KEY]: {
        ...snapshot,
        batchId,
        clientRequestId: snapshot?.clientRequestId || event.clientRequestId || "",
        revision: Number.isInteger(event.revision) ? event.revision : snapshot?.revision,
        streamReason: snapshot?.streamReason || event.reason || "",
        streamReceivedAtUnixMs: receivedAt,
        streamEmittedAtUnixMs: snapshot?.streamEmittedAtUnixMs || event.atMs || null
      },
      [BATCH_STATUS_REVISION_KEY]: nextStorageRevision()
    });
    const emittedAt = Number(snapshot?.streamEmittedAtUnixMs || event.atMs || 0);
    if (emittedAt > 0) {
      console.debug("[trench][latency] phase=background-sse-batch batch=%s revision=%s stream_to_background_ms=%s",
        batchId,
        Number.isInteger(event.revision) ? event.revision : snapshot?.revision,
        Math.max(0, receivedAt - emittedAt)
      );
    }
  } catch (_error) {}
}

/**
 * Seed the store from any `fetchWalletStatus` response (e.g. a mint-specific
 * content-panel fetch). Partial rows only overwrite fields they carry.
 */
export function hydrateFromWalletStatus(response) {
  const wallets = Array.isArray(response?.wallets) ? response.wallets : [];
  if (wallets.length === 0) return;
  const changedKeys = [];
  const changedMints = new Set();
  const tokenBalanceEntries = [];
  const walletBalanceEntries = [];
  for (const wallet of wallets) {
    const key = wallet?.envKey || wallet?.key;
    if (!key) continue;
    const prev = STORE.balances.get(key);
    const next = mergeEntry(prev, normalizeEntry(wallet));
    STORE.balances.set(key, next);
    if (!prev || !sameTrackedFields(prev, next) || prev.publicKey !== next.publicKey) {
      changedKeys.push(key);
      walletBalanceEntries.push(walletBalanceDiffEntry(key, next));
    }
    const mint = typeof wallet?.mint === "string" ? wallet.mint.trim() : "";
    const tokenBalance = tokenBalanceFromRow(wallet);
    const tokenBalanceRaw = tokenBalanceRawFromRow(wallet);
    const tokenDecimals = tokenDecimalsFromRow(wallet);
    if (mint && (tokenBalance != null || tokenBalanceRaw != null || tokenDecimals != null)) {
      const balanceKey = tokenBalanceKey(key, mint);
      const changed =
        (tokenBalance != null && STORE.tokenBalances.get(balanceKey) !== tokenBalance) ||
        (tokenBalanceRaw != null && STORE.tokenBalanceRaws.get(balanceKey) !== tokenBalanceRaw) ||
        (tokenDecimals != null && STORE.tokenDecimals.get(balanceKey) !== tokenDecimals);
      if (changed) {
        if (tokenBalance != null) STORE.tokenBalances.set(balanceKey, tokenBalance);
        if (tokenBalanceRaw != null) STORE.tokenBalanceRaws.set(balanceKey, tokenBalanceRaw);
        if (tokenDecimals != null) STORE.tokenDecimals.set(balanceKey, tokenDecimals);
        changedMints.add(mint);
        tokenBalanceEntries.push({ envKey: key, mint, tokenBalance, tokenBalanceRaw, tokenDecimals });
        if (!changedKeys.includes(key)) {
          changedKeys.push(key);
        }
      }
    }
  }
  if (changedKeys.length > 0) {
    broadcastUpdated({
      changedKeys,
      changedMints: Array.from(changedMints),
      tokenBalanceEntries,
      walletBalanceEntries,
    });
  }
}

async function runFetch({ force }) {
  const startedAt = Date.now();
  try {
    const payload = await fetchWalletStatus({
      includeDisabled: true,
      force: Boolean(force),
    });
    const wallets = Array.isArray(payload?.wallets) ? payload.wallets : [];
    const changedKeys = [];
    const changedMints = new Set();
    const tokenBalanceEntries = [];
    const walletBalanceEntries = [];
    for (const wallet of wallets) {
      const key = wallet?.envKey || wallet?.key;
      if (!key) continue;
      const prev = STORE.balances.get(key);
      const next = mergeEntry(prev, normalizeEntry(wallet));
      STORE.balances.set(key, next);
      if (!prev || !sameTrackedFields(prev, next) || prev.publicKey !== next.publicKey) {
        changedKeys.push(key);
        walletBalanceEntries.push(walletBalanceDiffEntry(key, next));
      }
      const mint = typeof wallet?.mint === "string" ? wallet.mint.trim() : "";
      const tokenBalance = tokenBalanceFromRow(wallet);
      const tokenBalanceRaw = tokenBalanceRawFromRow(wallet);
      const tokenDecimals = tokenDecimalsFromRow(wallet);
      if (mint && (tokenBalance != null || tokenBalanceRaw != null || tokenDecimals != null)) {
        const balanceKey = tokenBalanceKey(key, mint);
        const changed =
          (tokenBalance != null && STORE.tokenBalances.get(balanceKey) !== tokenBalance) ||
          (tokenBalanceRaw != null && STORE.tokenBalanceRaws.get(balanceKey) !== tokenBalanceRaw) ||
          (tokenDecimals != null && STORE.tokenDecimals.get(balanceKey) !== tokenDecimals);
        if (changed) {
          if (tokenBalance != null) STORE.tokenBalances.set(balanceKey, tokenBalance);
          if (tokenBalanceRaw != null) STORE.tokenBalanceRaws.set(balanceKey, tokenBalanceRaw);
          if (tokenDecimals != null) STORE.tokenDecimals.set(balanceKey, tokenDecimals);
          changedMints.add(mint);
          tokenBalanceEntries.push({ envKey: key, mint, tokenBalance, tokenBalanceRaw, tokenDecimals });
          if (!changedKeys.includes(key)) {
            changedKeys.push(key);
          }
        }
      }
    }
    STORE.lastFetchAt = startedAt;
    STORE.lastError = null;
    if (changedKeys.length > 0) {
      broadcastUpdated({
        changedKeys,
        changedMints: Array.from(changedMints),
        tokenBalanceEntries,
        walletBalanceEntries,
      });
    }
  } catch (error) {
    STORE.lastError = error?.message || String(error);
  }
}

function fetchBalances({ force = false } = {}) {
  if (STORE.inFlight) {
    return STORE.inFlight;
  }
  const task = runFetch({ force }).finally(() => {
    STORE.inFlight = null;
  });
  STORE.inFlight = task;
  return task;
}

function broadcastUpdated({
  changedKeys = [],
  changedMints = [],
  tokenBalanceEntries = [],
  walletBalanceEntries = [],
} = {}) {
  try {
    const payload = {
      [WALLET_STATUS_REVISION_KEY]: nextStorageRevision(),
    };
    if (changedKeys.length > 0) {
      payload[WALLET_STATUS_DIFF_KEY] = {
        changedKeys,
        changedMints,
        tokenBalanceEntries,
        walletBalanceEntries,
        at: Date.now(),
      };
    }
    chrome.storage.local.set(payload);
  } catch (_error) {}
}

function broadcastMeta() {
  try {
    chrome.storage.local.set({
      [WALLET_STATUS_META_KEY]: getMeta(),
    });
  } catch (_error) {}
}

/**
 * Read the current snapshot; trigger an HTTP fetch only if the stream is not
 * live (cold-start, fallback, or explicit force). With the SSE stream connected,
 * this is a pure read — no network call is made.
 */
export function ensureFreshBalances({ force = false } = {}) {
  if (force || STORE.connection.state !== "live") {
    void fetchBalances({ force });
  }
  return snapshotFromStore();
}

/**
 * Kick an HTTP refresh when an action (CRUD, trade) needs a guaranteed fresh
 * read. With the stream live, this is mostly a no-op — the server pushes the
 * new state via signatureSubscribe / accountSubscribe. If disconnected, we
 * force a single HTTP fetch so surfaces don't stall.
 */
export function invalidateBalances({ reason = "manual" } = {}) {
  if (STORE.connection.state === "live") {
    // Stream will push the authoritative update; avoid piling on HTTP.
    return;
  }
  void fetchBalances({ force: true });
}

/**
 * Post-trade safety net. With signatureSubscribe on the server, the confirmed
 * balance normally arrives via push. We schedule a single 8s belt-and-suspenders
 * fallback refetch in case the stream is momentarily degraded.
 */
export function invalidateBalancesAfterTrade() {
  if (STORE.connection.state !== "live") {
    void fetchBalances({ force: true });
  }
  const token = setTimeout(() => {
    STORE.scheduledRetries.delete(token);
    if (STORE.connection.state !== "live") {
      void fetchBalances({ force: true });
    }
  }, 8_000);
  STORE.scheduledRetries.add(token);
}
