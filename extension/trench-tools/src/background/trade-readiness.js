import { postTradeReadiness } from "./execution-client.js";

const HEARTBEAT_MS = 60_000;

const state = {
  surfaces: new Map(),
  heartbeatTimer: null,
  lastSignature: ""
};

export function setTradeReadinessSurface(surfaceId, active, surface = surfaceId) {
  const normalized = String(surfaceId || "").trim();
  if (!normalized) return;
  if (active) {
    state.surfaces.set(normalized, String(surface || normalized));
  } else {
    state.surfaces.delete(normalized);
  }
  flushTradeReadiness();
}

function flushTradeReadiness() {
  const active = state.surfaces.size > 0;
  const surface = [...state.surfaces.values()].sort().join(",");
  const signature = `${active}:${surface}`;
  if (signature !== state.lastSignature) {
    state.lastSignature = signature;
    postTradeReadiness({ active, surface }).catch((error) => {
      console.warn("[trade-readiness] update failed", error);
    });
  }
  if (active) {
    ensureHeartbeat();
  } else {
    clearHeartbeat();
  }
}

function ensureHeartbeat() {
  if (state.heartbeatTimer) return;
  state.heartbeatTimer = setInterval(() => {
    if (state.surfaces.size === 0) return;
    const surface = [...state.surfaces.values()].sort().join(",");
    postTradeReadiness({ active: true, surface }).catch((error) => {
      console.warn("[trade-readiness] heartbeat failed", error);
    });
  }, HEARTBEAT_MS);
}

function clearHeartbeat() {
  if (!state.heartbeatTimer) return;
  clearInterval(state.heartbeatTimer);
  state.heartbeatTimer = null;
}
