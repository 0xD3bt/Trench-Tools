import {
  DEFAULT_HOST_BASE,
  HOST_AUTH_TOKEN_STORAGE_KEY
} from "../shared/constants.js";

const ROUTE_TIMEOUT_MS = {
  health: 1500,
  bootstrap: 2500,
  runtimeStatus: 1500,
  settings: 2500,
  canonicalConfig: 2500,
  saveCanonicalConfig: 4000,
  saveSettings: 4000,
  listPresets: 2500,
  createPreset: 4000,
  updatePreset: 4000,
  deletePreset: 3000,
  listWallets: 2500,
  createWallet: 4000,
  updateWallet: 4000,
  deleteWallet: 3000,
  reorderWallets: 4000,
  listWalletGroups: 2500,
  createWalletGroup: 4000,
  updateWalletGroup: 4000,
  deleteWalletGroup: 3000,
  authBootstrap: 2500,
  listAuthTokens: 2500,
  createAuthToken: 4000,
  revokeAuthToken: 3000,
  listBatches: 2500,
  resolveToken: 2500,
  tradeReadiness: 1500,
  previewBatch: 3500,
  walletStatus: 3500,
  setActiveMark: 1500,
  balancePresence: 1500,
  resyncPnlHistory: 15000,
  resetPnlHistory: 5000,
  exportPnlHistory: 10000,
  wipePnlHistory: 5000,
  buy: 15000,
  sell: 15000,
  tokenSplit: 10000,
  tokenConsolidate: 10000,
  rewardsSummary: 12000,
  rewardsClaim: 10000,
  batchStatus: 2500
};

const FIRE_AND_FORGET_ROUTES = new Set(["tokenSplit", "tokenConsolidate", "rewardsClaim"]);

const SAFE_INFLIGHT_DEDUPE_ROUTES = new Set([
  "health",
  "bootstrap",
  "runtimeStatus",
  "settings",
  "canonicalConfig",
  "listPresets",
  "listWallets",
  "listWalletGroups",
  "authBootstrap",
  "listAuthTokens",
  "listBatches",
  "resolveToken",
  "previewBatch",
  "walletStatus",
  "batchStatus"
]);

const hostState = {
  ready: false,
  value: DEFAULT_HOST_BASE,
  authToken: "",
  promise: null
};

let latestBootstrapRevision = "";

const inflightRequests = new Map();
const walletStatusCache = new Map();
const WALLET_STATUS_CACHE_TTL_MS = 5000;

class HostRequestError extends Error {
  constructor(message, options = {}) {
    super(message);
    this.name = "HostRequestError";
    this.code = options.code || "HOST_REQUEST_FAILED";
    this.status = Number.isInteger(options.status) ? options.status : null;
    this.retryable = Boolean(options.retryable);
    this.timeout = Boolean(options.timeout);
    this.routeKey = options.routeKey || null;
  }
}

function normalizeHostBase(hostBase) {
  const normalized = String(hostBase || DEFAULT_HOST_BASE).trim();
  return (normalized || DEFAULT_HOST_BASE).replace(/\/+$/, "");
}

function isLoopbackHost(baseUrl) {
  try {
    const url = new URL(baseUrl);
    return ["127.0.0.1", "localhost"].includes(url.hostname);
  } catch {
    return false;
  }
}

export function ensureSecureTransport(baseUrl) {
  if (isLoopbackHost(baseUrl)) {
    return;
  }
  let protocol = "";
  try {
    protocol = new URL(baseUrl).protocol;
  } catch {
    throw new HostRequestError("Configured host URL is invalid.", {
      code: "HOST_INVALID_URL",
      retryable: false
    });
  }
  if (protocol !== "https:") {
    throw new HostRequestError(
      "Remote execution hosts must use HTTPS. Non-local connections over plain HTTP are blocked.",
      {
        code: "HOST_INSECURE_TRANSPORT",
        retryable: false
      }
    );
  }
}

async function maybeSyncBootstrapRevision(payload) {
  const nextRevision = String(payload?.bootstrapRevision || "").trim();
  if (!nextRevision || nextRevision === latestBootstrapRevision) {
    return;
  }
  latestBootstrapRevision = nextRevision;
  await chrome.storage.local.set({
    "trenchTools.bootstrapHostRevision": nextRevision,
    "trenchTools.bootstrapRevision": Date.now()
  });
}

function stableStringify(value) {
  if (Array.isArray(value)) {
    return `[${value.map((item) => stableStringify(item)).join(",")}]`;
  }
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

async function loadHostBaseFromStorage() {
  const stored = await chrome.storage.local.get(HOST_AUTH_TOKEN_STORAGE_KEY);
  hostState.value = DEFAULT_HOST_BASE;
  hostState.authToken = typeof stored[HOST_AUTH_TOKEN_STORAGE_KEY] === "string"
    ? stored[HOST_AUTH_TOKEN_STORAGE_KEY].trim()
    : "";
  hostState.ready = true;
  hostState.promise = null;
  return hostState.value;
}

export async function getCachedHostBase() {
  if (hostState.ready) {
    return hostState.value;
  }
  if (!hostState.promise) {
    hostState.promise = loadHostBaseFromStorage();
  }
  return hostState.promise;
}

export async function refreshHostConnectionState() {
  return loadHostBaseFromStorage();
}

chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName !== "local") {
    return;
  }
  if (Object.prototype.hasOwnProperty.call(changes, HOST_AUTH_TOKEN_STORAGE_KEY)) {
    hostState.authToken = typeof changes[HOST_AUTH_TOKEN_STORAGE_KEY]?.newValue === "string"
      ? changes[HOST_AUTH_TOKEN_STORAGE_KEY].newValue.trim()
      : "";
    hostState.ready = true;
    hostState.promise = null;
    walletStatusCache.clear();
  }
});

function timeoutForRoute(routeKey) {
  return ROUTE_TIMEOUT_MS[routeKey] || 4000;
}

function createRequestKey(routeKey, baseUrl, options = {}) {
  if (options.dedupeKey) {
    return `${routeKey}:${baseUrl}:${options.dedupeKey}`;
  }
  const bodyKey = options.body ? stableStringify(options.body) : "";
  return `${routeKey}:${baseUrl}:${options.method || "GET"}:${bodyKey}`;
}

function buildTimeoutMessage(routeKey, timeoutMs) {
  if (FIRE_AND_FORGET_ROUTES.has(routeKey)) {
    return routeKey === "rewardsClaim"
      ? "Rewards claim request is stale after 10s."
      : "Token distribution request is stale after 10s.";
  }
  return `Timed out contacting the execution host for ${routeKey} after ${timeoutMs}ms.`;
}

async function performJsonRequest(routeKey, path, options = {}) {
  const baseUrl = await getCachedHostBase();
  ensureSecureTransport(baseUrl);
  const timeoutMs = options.timeoutMs || timeoutForRoute(routeKey);
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), timeoutMs);
  const authToken = hostState.authToken || "";

  try {
    const response = await fetch(`${baseUrl}${path}`, {
      method: options.method || "GET",
      headers: {
        "content-type": "application/json",
        ...(authToken && !options.skipAuth ? { authorization: `Bearer ${authToken}` } : {}),
        ...(options.headers || {})
      },
      body: options.body ? JSON.stringify(options.body) : undefined,
      signal: controller.signal
    });

    if (!response.ok) {
      const text = await response.text();
      throw new HostRequestError(text || `Host request failed with ${response.status}`, {
        code:
          response.status === 401
            ? "HOST_UNAUTHORIZED"
            : response.status === 429
              ? "HOST_OVERLOADED"
              : "HOST_HTTP_ERROR",
        status: response.status,
        retryable:
          !FIRE_AND_FORGET_ROUTES.has(routeKey) &&
          (response.status >= 500 || response.status === 429),
        routeKey
      });
    }

    const payload = await response.json();
    await maybeSyncBootstrapRevision(payload);
    return payload;
  } catch (error) {
    if (error instanceof HostRequestError) {
      throw error;
    }
    if (error?.name === "AbortError") {
      throw new HostRequestError(buildTimeoutMessage(routeKey, timeoutMs), {
        code: "HOST_TIMEOUT",
        retryable: !FIRE_AND_FORGET_ROUTES.has(routeKey),
        timeout: true,
        routeKey
      });
    }
    throw new HostRequestError(error?.message || "Failed to contact execution host", {
      code: "HOST_UNREACHABLE",
      retryable: !FIRE_AND_FORGET_ROUTES.has(routeKey),
      routeKey
    });
  } finally {
    clearTimeout(timeoutId);
  }
}

async function requestJson(routeKey, path, options = {}) {
  const baseUrl = await getCachedHostBase();
  const shouldDedupe = Boolean(options.dedupeKey) || SAFE_INFLIGHT_DEDUPE_ROUTES.has(routeKey);
  if (!shouldDedupe) {
    return performJsonRequest(routeKey, path, options);
  }

  const requestKey = createRequestKey(routeKey, baseUrl, options);
  if (inflightRequests.has(requestKey)) {
    return inflightRequests.get(requestKey);
  }

  const promise = performJsonRequest(routeKey, path, options).finally(() => {
    inflightRequests.delete(requestKey);
  });
  inflightRequests.set(requestKey, promise);
  return promise;
}

function pruneExpiredWalletStatusCache(now = Date.now()) {
  for (const [key, entry] of walletStatusCache.entries()) {
    if (!entry || entry.expiresAt <= now) {
      walletStatusCache.delete(key);
    }
  }
}

function withClientRequestId(payload) {
  const clientRequestId = String(payload?.clientRequestId || "").trim();
  if (clientRequestId) {
    return { ...payload, clientRequestId };
  }
  return { ...payload, clientRequestId: crypto.randomUUID() };
}

export function serializeHostError(error) {
  if (error instanceof HostRequestError) {
    return {
      message: error.message,
      code: error.code,
      status: error.status,
      retryable: error.retryable,
      timeout: error.timeout
    };
  }

  return {
    message: error?.message || "Unknown extension error",
    code: typeof error?.code === "string" && error.code ? error.code : "EXTENSION_ERROR",
    status: Number.isInteger(error?.status) ? error.status : null,
    retryable: typeof error?.retryable === "boolean" ? error.retryable : false,
    timeout: typeof error?.timeout === "boolean" ? error.timeout : false
  };
}

export function fetchHealth() {
  return requestJson("health", "/api/extension/health");
}

export function postActiveMints(entries) {
  return requestJson("setActiveMints", "/api/extension/events/active-mint", {
    method: "POST",
    body: { entries: Array.isArray(entries) ? entries : [] }
  });
}

export function postActiveMark(payload = {}) {
  return requestJson("setActiveMark", "/api/extension/events/active-mark", {
    method: "POST",
    body: payload && typeof payload === "object" ? payload : {}
  });
}

export function postBalancePresence({ active, reason } = {}) {
  return requestJson("balancePresence", "/api/extension/events/presence", {
    method: "POST",
    body: {
      active: Boolean(active),
      reason: typeof reason === "string" ? reason : ""
    }
  });
}

export function postTradeReadiness({ active, surface } = {}) {
  return requestJson("tradeReadiness", "/api/extension/trade-readiness", {
    method: "POST",
    body: {
      active: Boolean(active),
      surface: typeof surface === "string" ? surface : ""
    }
  });
}

export function fetchAuthBootstrap() {
  return requestJson("authBootstrap", "/api/extension/auth/bootstrap", {
    skipAuth: true
  });
}

export function fetchBootstrap() {
  return requestJson("bootstrap", "/api/extension/bootstrap");
}

export function fetchRuntimeStatus() {
  return requestJson("runtimeStatus", "/api/extension/runtime-status");
}

export function fetchSettings() {
  return requestJson("settings", "/api/extension/settings");
}

export function fetchCanonicalConfig() {
  return requestJson("canonicalConfig", "/api/extension/config");
}

export function saveCanonicalConfig(payload) {
  return requestJson("saveCanonicalConfig", "/api/extension/config", {
    method: "PUT",
    body: payload
  });
}

export function saveSettings(payload) {
  return requestJson("saveSettings", "/api/extension/settings", {
    method: "PUT",
    body: payload
  });
}

export function listPresets() {
  return requestJson("listPresets", "/api/extension/presets");
}

export function createPreset(payload) {
  return requestJson("createPreset", "/api/extension/presets", {
    method: "POST",
    body: payload
  });
}

export function updatePreset(presetId, payload) {
  return requestJson("updatePreset", `/api/extension/presets/${encodeURIComponent(presetId)}`, {
    method: "PUT",
    body: payload
  });
}

export function deletePreset(presetId) {
  return requestJson("deletePreset", `/api/extension/presets/${encodeURIComponent(presetId)}`, {
    method: "DELETE"
  });
}

export function listWallets() {
  return requestJson("listWallets", "/api/extension/wallets");
}

export function createWallet(payload) {
  return requestJson("createWallet", "/api/extension/wallets", {
    method: "POST",
    body: payload
  });
}

export function updateWallet(walletKey, payload) {
  return requestJson("updateWallet", `/api/extension/wallets/${encodeURIComponent(walletKey)}`, {
    method: "PUT",
    body: payload
  });
}

export function deleteWallet(walletKey) {
  return requestJson("deleteWallet", `/api/extension/wallets/${encodeURIComponent(walletKey)}`, {
    method: "DELETE"
  });
}

export function reorderWallets(walletKeys) {
  return requestJson("reorderWallets", "/api/extension/wallets/reorder", {
    method: "POST",
    body: { walletKeys: Array.isArray(walletKeys) ? walletKeys : [] }
  });
}

export function listWalletGroups() {
  return requestJson("listWalletGroups", "/api/extension/wallet-groups");
}

export function createWalletGroup(payload) {
  return requestJson("createWalletGroup", "/api/extension/wallet-groups", {
    method: "POST",
    body: payload
  });
}

export function updateWalletGroup(groupId, payload) {
  return requestJson("updateWalletGroup", `/api/extension/wallet-groups/${encodeURIComponent(groupId)}`, {
    method: "PUT",
    body: payload
  });
}

export function deleteWalletGroup(groupId) {
  return requestJson("deleteWalletGroup", `/api/extension/wallet-groups/${encodeURIComponent(groupId)}`, {
    method: "DELETE"
  });
}

export function listAuthTokens() {
  return requestJson("listAuthTokens", "/api/extension/auth/tokens");
}

export function createAuthToken(payload) {
  return requestJson("createAuthToken", "/api/extension/auth/tokens", {
    method: "POST",
    body: payload
  });
}

export function revokeAuthToken(tokenId) {
  return requestJson("revokeAuthToken", `/api/extension/auth/tokens/${encodeURIComponent(tokenId)}`, {
    method: "DELETE"
  });
}

export function resolveToken(payload) {
  return requestJson("resolveToken", "/api/extension/resolve-token", {
    method: "POST",
    body: payload
  });
}

export function postPrewarm(payload) {
  return requestJson("prewarm", "/api/extension/prewarm", {
    method: "POST",
    body: payload
  });
}

export function listBatches() {
  return requestJson("listBatches", "/api/extension/batches");
}

export function previewBatch(payload) {
  return requestJson("previewBatch", "/api/extension/batch/preview", {
    method: "POST",
    body: payload
  });
}

export async function fetchWalletStatus(payload = {}) {
  const normalizedPayload = payload && typeof payload === "object"
    ? { ...payload }
    : {};
  const force = Boolean(normalizedPayload.force);
  const cachePayload = { ...normalizedPayload };
  delete cachePayload.force;
  const baseUrl = await getCachedHostBase();
  const cacheKey = createRequestKey("walletStatus", baseUrl, {
    method: "POST",
    body: cachePayload
  });
  pruneExpiredWalletStatusCache();
  if (!force) {
    const cached = walletStatusCache.get(cacheKey);
    if (cached && cached.expiresAt > Date.now()) {
      return cached.payload;
    }
  }
  const payloadResult = await requestJson("walletStatus", "/api/extension/wallet-status", {
    method: "POST",
    body: normalizedPayload
  });
  walletStatusCache.set(cacheKey, {
    payload: payloadResult,
    expiresAt: Date.now() + WALLET_STATUS_CACHE_TTL_MS
  });
  return payloadResult;
}

export function resyncPnlHistory(payload) {
  return requestJson("resyncPnlHistory", "/api/extension/pnl/resync", {
    method: "POST",
    body: payload
  });
}

export function resetPnlHistory(payload) {
  return requestJson("resetPnlHistory", "/api/extension/pnl/reset", {
    method: "POST",
    body: payload
  });
}

export function exportPnlHistory() {
  return requestJson("exportPnlHistory", "/api/extension/pnl/export", {
    method: "POST",
    body: {}
  });
}

export function wipePnlHistory() {
  return requestJson("wipePnlHistory", "/api/extension/pnl/wipe", {
    method: "POST",
    body: {}
  });
}

export function buy(payload) {
  const request = withClientRequestId(payload);
  return requestJson("buy", "/api/extension/buy", {
    method: "POST",
    body: request,
    dedupeKey: request.clientRequestId
  });
}

export function sell(payload) {
  const request = withClientRequestId(payload);
  return requestJson("sell", "/api/extension/sell", {
    method: "POST",
    body: request,
    dedupeKey: request.clientRequestId
  });
}

export function splitTokens(payload) {
  const request = withClientRequestId(payload);
  return requestJson("tokenSplit", "/api/extension/token-distribution/split", {
    method: "POST",
    body: request,
    dedupeKey: request.clientRequestId
  });
}

export function consolidateTokens(payload) {
  const request = withClientRequestId(payload);
  return requestJson("tokenConsolidate", "/api/extension/token-distribution/consolidate", {
    method: "POST",
    body: request,
    dedupeKey: request.clientRequestId
  });
}

export function fetchRewardsSummary(payload) {
  return requestJson("rewardsSummary", "/api/extension/rewards/summary", {
    method: "POST",
    body: payload || {}
  });
}

export function claimRewards(payload) {
  const request = withClientRequestId(payload);
  return requestJson("rewardsClaim", "/api/extension/rewards/claim", {
    method: "POST",
    body: request,
    dedupeKey: request.clientRequestId
  });
}

export function getBatchStatus(batchId) {
  return requestJson("batchStatus", `/api/extension/batch/${encodeURIComponent(batchId)}`);
}
