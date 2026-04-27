import { postActiveMints } from "./execution-client.js";
import { setBalanceDemandSource } from "./balance-demand.js";

/**
 * Active-mint registry.
 *
 * Extension surfaces (content panel, launchdeck, options) each declare which
 * mints they're currently interested in. The registry unions all surfaces,
 * debounces on change, and POSTs the union to Rust so the SSE stream only
 * subscribes to ATAs for mints someone is actually viewing.
 */

const DEBOUNCE_MS = 200;

const registry = {
  // surfaceId -> Set<string>
  bySurface: new Map(),
  lastSent: "",
  timer: null,
  inflight: null
};

function computeUnion() {
  const merged = new Set();
  for (const set of registry.bySurface.values()) {
    for (const mint of set) {
      merged.add(mint);
    }
  }
  return [...merged].sort();
}

function schedulePush() {
  if (registry.timer) {
    return;
  }
  registry.timer = setTimeout(() => {
    registry.timer = null;
    flushPush().catch((error) => {
      console.warn("[active-mints] push failed", error);
    });
  }, DEBOUNCE_MS);
}

async function flushPush() {
  const mints = computeUnion();
  const signature = mints.join(",");
  if (signature === registry.lastSent) {
    return;
  }
  registry.lastSent = signature;
  setBalanceDemandSource("active-mints", mints.length > 0, "active-mints");
  const entries = mints.map((mint) => ({ walletKey: "*", mint }));
  // Wait for any in-flight request before issuing a new one so the server
  // always sees the latest intent last.
  if (registry.inflight) {
    try {
      await registry.inflight;
    } catch (_) {
      // ignore; we'll still try to push the latest intent.
    }
  }
  const pending = postActiveMints(entries).catch((error) => {
    // If the call failed, clear lastSent so the next change forces a retry.
    registry.lastSent = "";
    throw error;
  });
  registry.inflight = pending;
  try {
    await pending;
  } finally {
    if (registry.inflight === pending) {
      registry.inflight = null;
    }
  }
}

/**
 * Replace the set of active mints for a given surface. Passing an empty list
 * clears this surface's contribution (e.g. the content panel is closed).
 */
export function setActiveMints(surfaceId, mints) {
  const normalized = new Set(
    (Array.isArray(mints) ? mints : [])
      .map((value) => (typeof value === "string" ? value.trim() : ""))
      .filter((value) => value.length > 0)
  );
  if (normalized.size === 0) {
    if (!registry.bySurface.has(surfaceId)) {
      return;
    }
    registry.bySurface.delete(surfaceId);
  } else {
    registry.bySurface.set(surfaceId, normalized);
  }
  schedulePush();
}

export function clearActiveMints(surfaceId) {
  if (!registry.bySurface.has(surfaceId)) {
    return;
  }
  registry.bySurface.delete(surfaceId);
  schedulePush();
}
