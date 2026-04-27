import { postBalancePresence } from "./execution-client.js";

const HEARTBEAT_MS = 20_000;
const IDLE_GRACE_MS = 60_000;

const state = {
  sources: new Set(),
  active: false,
  heartbeatTimer: null,
  inactiveTimer: null,
  lastReason: ""
};

export function markBalanceDemand(reason = "transient") {
  state.lastReason = String(reason || "transient");
  activate();
  scheduleInactiveIfIdle();
}

export function setBalanceDemandSource(sourceId, active, reason = sourceId) {
  const normalized = String(sourceId || "").trim();
  if (!normalized) return;
  if (active) {
    state.sources.add(normalized);
    state.lastReason = String(reason || normalized);
    activate();
    clearInactiveTimer();
  } else if (state.sources.delete(normalized)) {
    state.lastReason = String(reason || normalized);
    scheduleInactiveIfIdle();
  }
}

function activate() {
  if (state.active) {
    ensureHeartbeat();
    return;
  }
  state.active = true;
  sendPresence(true);
  ensureHeartbeat();
}

function deactivate() {
  if (!state.active || state.sources.size > 0) return;
  state.active = false;
  clearHeartbeat();
  sendPresence(false);
}

function scheduleInactiveIfIdle() {
  if (state.sources.size > 0) return;
  clearInactiveTimer();
  state.inactiveTimer = setTimeout(() => {
    state.inactiveTimer = null;
    deactivate();
  }, IDLE_GRACE_MS);
}

function ensureHeartbeat() {
  if (state.heartbeatTimer) return;
  state.heartbeatTimer = setInterval(() => {
    if (!state.active) return;
    sendPresence(true);
  }, HEARTBEAT_MS);
}

function clearHeartbeat() {
  if (!state.heartbeatTimer) return;
  clearInterval(state.heartbeatTimer);
  state.heartbeatTimer = null;
}

function clearInactiveTimer() {
  if (!state.inactiveTimer) return;
  clearTimeout(state.inactiveTimer);
  state.inactiveTimer = null;
}

function sendPresence(active) {
  postBalancePresence({ active, reason: state.lastReason }).catch((error) => {
    console.warn("[balance-demand] presence update failed", error);
  });
}
