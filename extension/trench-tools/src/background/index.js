import {
  buy,
  claimRewards,
  consolidateTokens,
  createAuthToken,
  fetchHealth,
  fetchAuthBootstrap,
  fetchBootstrap,
  fetchRewardsSummary,
  fetchRuntimeStatus,
  fetchWalletStatus,
  fetchSettings,
  fetchCanonicalConfig,
  getBatchStatus,
  exportPnlHistory,
  listBatches,
  listAuthTokens,
  listPresets,
  createPreset,
  updatePreset,
  deletePreset,
  listWallets,
  createWallet,
  updateWallet,
  deleteWallet,
  reorderWallets,
  listWalletGroups,
  createWalletGroup,
  updateWalletGroup,
  deleteWalletGroup,
  postActiveMark,
  postPrewarm,
  previewBatch,
  resetPnlHistory,
  refreshHostConnectionState,
  resolveToken,
  resyncPnlHistory,
  revokeAuthToken,
  saveCanonicalConfig,
  saveSettings,
  sell,
  serializeHostError,
  splitTokens,
  wipePnlHistory
} from "./execution-client.js";
import {
  ensureFreshBalances,
  getBalancesSnapshot,
  getMeta,
  hydrateFromWalletStatus,
  invalidateBalances,
  invalidateBalancesAfterTrade
} from "./balances-store.js";
import { startEventsClient } from "./events-client.js";
import {
  applyRuntimeDiagnostics,
  dismissDiagnostic,
  getDiagnosticsSnapshot
} from "./diagnostics-store.js";
import { clearActiveMints, setActiveMints } from "./active-mints.js";
import { markBalanceDemand, setBalanceDemandSource } from "./balance-demand.js";
import { setTradeReadinessSurface } from "./trade-readiness.js";
import {
  getHostAuthToken,
  getLaunchdeckHostBase,
  isLoopbackHost,
  originPatternFromHostBase
} from "../shared/host-client.js";
import {
  OPTIONS_TARGET_SECTION_KEY
} from "../shared/constants.js";
// Side-effect import: the shared module attaches its public API to
// `globalThis.__trenchToolsStorageMigrations` so the popout (a classic
// <script> context) and this service worker (an ES module) both consume the
// same implementation.
import "../shared/storage-migrations.js";

// Kick off the SSE consumer eagerly. The service worker may be torn down and
// re-spawned; on every cold start we reconnect and refetch a snapshot. Message
// handlers await this promise so upgraded profiles cannot serve requests before
// the one-shot storage migration updates stale host settings.
const backgroundRuntimeReady = initializeBackgroundRuntime();
const LAUNCHDECK_RUNTIME_STATUS_TIMEOUT_MS = 2500;

async function initializeBackgroundRuntime() {
  try {
    const migrations = globalThis.__trenchToolsStorageMigrations;
    if (!migrations) {
      throw new Error("Trench Tools storage migrations module not initialized.");
    }
    await migrations.migrateStoredConnectionSettings({
      onMergeWarning: () => {
        console.warn(
          "Trench Tools migration: found separate execution-host and LaunchDeck-host access tokens. Kept the execution-host token as the shared bearer and left the older LaunchDeck token in storage for manual recovery; re-enter the shared token in Options if the LaunchDeck host now rejects requests."
        );
      }
    });
  } catch (error) {
    console.warn("Trench Tools host migration failed", error);
  }
  startEventsClient();
}

const OFFSCREEN_AUDIO_PATH = "src/offscreen/audio.html";
let ensureOffscreenAudioPromise = null;

function launchdeckConnectionError(message) {
  const error = new Error(message);
  error.code = "LAUNCHDECK_NOT_CONFIGURED";
  return error;
}

async function assertStoredHostPermission(baseUrl) {
  if (isLoopbackHost(baseUrl)) {
    return;
  }
  const originPattern = originPatternFromHostBase(baseUrl);
  const granted = await chrome.permissions.contains({ origins: [originPattern] });
  if (!granted) {
    throw launchdeckConnectionError(
      `Remote host permission is missing for ${new URL(baseUrl).origin}. Open Global Settings and grant access first.`
    );
  }
}

function assertSecureTransport(baseUrl, label) {
  if (isLoopbackHost(baseUrl)) {
    return;
  }
  let parsed;
  try {
    parsed = new URL(baseUrl);
  } catch {
    throw launchdeckConnectionError(`Configured ${label} URL is invalid.`);
  }
  if (parsed.protocol !== "https:") {
    throw launchdeckConnectionError(`${label} must use HTTPS when it is not loopback.`);
  }
}

async function loadLaunchdeckConnection() {
  const baseUrl = await getLaunchdeckHostBase();
  const authToken = await getHostAuthToken();
  assertSecureTransport(baseUrl, "LaunchDeck host");
  await assertStoredHostPermission(baseUrl);
  if (!authToken) {
    throw launchdeckConnectionError("LaunchDeck host requires the shared access token.");
  }
  return { baseUrl, authToken };
}

async function fetchLaunchdeckSettingsPayload() {
  const { baseUrl, authToken } = await loadLaunchdeckConnection();
  const response = await fetch(new URL("/api/settings", baseUrl).toString(), {
    method: "GET",
    headers: { authorization: `Bearer ${authToken}` },
    credentials: "omit",
    cache: "no-cache"
  });
  const contentType = response.headers.get("content-type") || "";
  const payload = contentType.includes("application/json")
    ? await response.json().catch(() => ({}))
    : await response.text().catch(() => "");
  if (!response.ok) {
    const message = typeof payload === "string"
      ? payload
      : payload?.error || `${response.status} ${response.statusText}`.trim();
    throw new Error(message || "LaunchDeck request failed.");
  }
  return payload;
}

async function fetchLaunchdeckRuntimeStatusPayload() {
  const { baseUrl, authToken } = await loadLaunchdeckConnection();
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), LAUNCHDECK_RUNTIME_STATUS_TIMEOUT_MS);
  let response;
  try {
    response = await fetch(new URL("/api/runtime-status", baseUrl).toString(), {
      method: "GET",
      headers: { authorization: `Bearer ${authToken}` },
      credentials: "omit",
      cache: "no-cache",
      signal: controller.signal
    });
  } catch (error) {
    if (error?.name === "AbortError") {
      throw new Error(`LaunchDeck runtime status timed out after ${LAUNCHDECK_RUNTIME_STATUS_TIMEOUT_MS}ms.`);
    }
    throw error;
  } finally {
    clearTimeout(timer);
  }
  const contentType = response.headers.get("content-type") || "";
  const payload = contentType.includes("application/json")
    ? await response.json().catch(() => ({}))
    : await response.text().catch(() => "");
  if (!response.ok) {
    const message = typeof payload === "string"
      ? payload
      : payload?.error || `${response.status} ${response.statusText}`.trim();
    throw new Error(message || "LaunchDeck runtime status request failed.");
  }
  return payload;
}

function launchdeckUnavailableDiagnostic(error) {
  const message = String(error?.message || "LaunchDeck runtime status unavailable.").trim();
  return {
    fingerprint: "launchdeck-engine:runtime:::launchdeck_status_unreachable",
    severity: "warning",
    source: "launchdeck-engine",
    code: "launchdeck_status_unreachable",
    message: "LaunchDeck runtime status is unavailable.",
    detail: message,
    active: true,
    restartRequired: false,
    atMs: Date.now()
  };
}

async function ensureOffscreenAudioDocument() {
  if (!chrome.offscreen?.createDocument) {
    return false;
  }
  if (ensureOffscreenAudioPromise) {
    return ensureOffscreenAudioPromise;
  }
  ensureOffscreenAudioPromise = (async () => {
    try {
      if (chrome.offscreen.hasDocument) {
        const has = await chrome.offscreen.hasDocument();
        if (has) return true;
      }
      await chrome.offscreen.createDocument({
        url: OFFSCREEN_AUDIO_PATH,
        reasons: ["AUDIO_PLAYBACK"],
        justification: "Play buy-confirmation sound at the same time as the confirmed toast."
      });
      return true;
    } catch (error) {
      const message = String(error?.message || "");
      if (message.includes("Only a single offscreen document") || message.includes("already exists")) {
        return true;
      }
      console.warn("Trench Tools offscreen document creation failed", error);
      return false;
    } finally {
      ensureOffscreenAudioPromise = null;
    }
  })();
  return ensureOffscreenAudioPromise;
}

async function playSoundViaOffscreen(payload) {
  const ok = await ensureOffscreenAudioDocument();
  if (!ok) {
    return { ok: false, error: "offscreen unavailable" };
  }
  try {
    return await chrome.runtime.sendMessage({
      type: "trench:offscreen-play-sound",
      payload
    });
  } catch (error) {
    console.warn("Trench Tools offscreen audio dispatch failed", error);
    return { ok: false, error: error?.message || "dispatch failed" };
  }
}

const CONTENT_REINJECTION_TARGETS = [
  {
    matches: ["https://axiom.trade/*"],
    loader: "src/content/loaders/axiom-loader.js"
  },
  {
    matches: ["https://j7tracker.io/*"],
    loader: "src/content/loaders/j7-loader.js"
  }
];

async function openExternalUrl(payload = {}) {
  const url = String(payload.url || "").trim();
  const mode = String(payload.mode || "tab").trim().toLowerCase();
  let parsed;
  try {
    parsed = new URL(url);
  } catch (_error) {
    throw new Error("Valid URL required.");
  }
  if (parsed.protocol !== "https:" && parsed.protocol !== "http:") {
    throw new Error("Only http and https URLs can be opened.");
  }
  if (mode === "window") {
    const width = Math.max(420, Math.min(1280, Number(payload.width) || 1100));
    const height = Math.max(420, Math.min(1000, Number(payload.height) || 760));
    await chrome.windows.create({
      url: parsed.toString(),
      type: "popup",
      focused: true,
      width,
      height
    });
    return { opened: true, mode: "window" };
  }
  await chrome.tabs.create({ url: parsed.toString(), active: true });
  return { opened: true, mode: "tab" };
}

async function handleMessage(message) {
  await backgroundRuntimeReady;
  switch (message?.type) {
    case "trench:get-health":
      return fetchHealth();
    case "trench:get-auth-bootstrap":
      return fetchAuthBootstrap();
    case "trench:refresh-host-connection":
      await refreshHostConnectionState();
      return { ok: true };
    case "trench:get-bootstrap":
      return fetchBootstrap();
    case "trench:get-runtime-status": {
      const status = await fetchRuntimeStatus();
      await applyRuntimeDiagnostics(status?.diagnostics || [], "execution-engine");
      try {
        const launchdeckStatus = await fetchLaunchdeckRuntimeStatusPayload();
        await applyRuntimeDiagnostics(launchdeckStatus?.diagnostics || [], "launchdeck-engine");
      } catch (error) {
        await applyRuntimeDiagnostics([launchdeckUnavailableDiagnostic(error)], "launchdeck-engine");
      }
      return status;
    }
    case "trench:get-runtime-diagnostics":
      return getDiagnosticsSnapshot();
    case "trench:dismiss-runtime-diagnostic":
      return dismissDiagnostic(message.payload?.fingerprint);
    case "trench:get-launchdeck-host-settings":
    case "trench:get-launchdeck-settings":
      return fetchLaunchdeckSettingsPayload();
    case "trench:get-wallet-status": {
      markBalanceDemand("wallet-status");
      const response = await fetchWalletStatus(message.payload);
      // Piggy-back: every successful wallet-status response hydrates the
      // shared store so mint-specific panel fetches keep the global snapshot warm.
      hydrateFromWalletStatus(response);
      return response;
    }
    case "trench:get-balances-snapshot": {
      markBalanceDemand("balances-snapshot");
      const force = Boolean(message.payload?.force);
      ensureFreshBalances({ force });
      return getBalancesSnapshot();
    }
    case "trench:get-balances-meta":
      return getMeta();
    case "trench:set-trade-readiness": {
      const surfaceId = String(message.payload?.surfaceId ?? "").trim();
      if (!surfaceId) {
        return { ok: false, error: "surfaceId required" };
      }
      setTradeReadinessSurface(
        surfaceId,
        Boolean(message.payload?.active),
        String(message.payload?.surface ?? surfaceId)
      );
      return { ok: true };
    }
    case "trench:set-active-mints": {
      const surfaceId = String(message.payload?.surfaceId ?? "").trim();
      if (!surfaceId) {
        return { ok: false, error: "surfaceId required" };
      }
      const mints = Array.isArray(message.payload?.mints) ? message.payload.mints : [];
      if (mints.length === 0) {
        clearActiveMints(surfaceId);
      } else {
        setActiveMints(surfaceId, mints);
      }
      return { ok: true };
    }
    case "trench:set-active-mark": {
      const payload = message.payload || {};
      const surfaceId = String(payload.surfaceId || "default").trim() || "default";
      const response = await postActiveMark(payload);
      setBalanceDemandSource(
        `active-mark:${surfaceId}`,
        Boolean(payload.active),
        "active-mark"
      );
      return response;
    }
    case "trench:invalidate-balances": {
      const payload = message.payload || {};
      if (payload.afterTrade) {
        invalidateBalancesAfterTrade();
      } else {
        invalidateBalances({
          reason: typeof payload.reason === "string" ? payload.reason : "manual",
          delays: Array.isArray(payload.delays) ? payload.delays : null
        });
      }
      return { ok: true };
    }
    case "trench:get-settings":
      return fetchSettings();
    case "trench:get-execution-canonical-config":
    case "trench:get-canonical-config":
      return fetchCanonicalConfig();
    case "trench:save-execution-canonical-config":
    case "trench:save-canonical-config":
      return saveCanonicalConfig(message.payload);
    case "trench:save-settings":
      return saveSettings(message.payload);
    case "trench:resync-pnl-history":
      return resyncPnlHistory(message.payload);
    case "trench:reset-pnl-history":
      return resetPnlHistory(message.payload);
    case "trench:export-pnl-history":
      return exportPnlHistory();
    case "trench:wipe-pnl-history":
      return wipePnlHistory();
    case "trench:list-presets":
      return listPresets();
    case "trench:create-preset":
      return createPreset(message.payload);
    case "trench:update-preset":
      return updatePreset(message.payload.presetId, message.payload.preset);
    case "trench:delete-preset":
      return deletePreset(message.payload.presetId);
    case "trench:list-wallets":
      return listWallets();
    case "trench:create-wallet": {
      const result = await createWallet(message.payload);
      invalidateBalances({ reason: "wallet-created" });
      return result;
    }
    case "trench:update-wallet": {
      const result = await updateWallet(message.payload.walletKey, message.payload.wallet);
      invalidateBalances({ reason: "wallet-updated" });
      return result;
    }
    case "trench:delete-wallet": {
      const result = await deleteWallet(message.payload.walletKey);
      invalidateBalances({ reason: "wallet-deleted" });
      return result;
    }
    case "trench:reorder-wallets":
      return reorderWallets(message.payload?.walletKeys || []);
    case "trench:list-auth-tokens":
      return listAuthTokens();
    case "trench:create-auth-token":
      return createAuthToken(message.payload);
    case "trench:revoke-auth-token":
      return revokeAuthToken(message.payload.tokenId);
    case "trench:list-wallet-groups":
      return listWalletGroups();
    case "trench:create-wallet-group":
      return createWalletGroup(message.payload);
    case "trench:update-wallet-group":
      return updateWalletGroup(message.payload.groupId, message.payload.group);
    case "trench:delete-wallet-group":
      return deleteWalletGroup(message.payload.groupId);
    case "trench:resolve-token":
      return resolveToken(message.payload);
    case "trench:prewarm-mint": {
      // Intent-driven prewarm. Best-effort: swallow errors so they
      // never surface as visible failures in the panel UI — a failed
      // prewarm just means the subsequent trade click hits the cold
      // path. The host logs the real reason.
      try {
        const response = await postPrewarm(message.payload || {});
        return { ok: true, data: response };
      } catch (error) {
        const serialized = serializeHostError(error);
        return {
          ok: false,
          error: serialized.message,
          errorCode: serialized.code
        };
      }
    }
    case "trench:list-batches":
      return listBatches();
    case "trench:preview-batch":
      return previewBatch(message.payload);
    case "trench:buy": {
      const result = await buy(message.payload);
      invalidateBalancesAfterTrade();
      return result;
    }
    case "trench:sell": {
      const result = await sell(message.payload);
      invalidateBalancesAfterTrade();
      return result;
    }
    case "trench:token-split": {
      const result = await splitTokens(message.payload);
      invalidateBalances({ reason: "token-split" });
      return result;
    }
    case "trench:token-consolidate": {
      const result = await consolidateTokens(message.payload);
      invalidateBalances({ reason: "token-consolidate" });
      return result;
    }
    case "trench:get-rewards-summary":
      return fetchRewardsSummary(message.payload);
    case "trench:claim-rewards":
      return claimRewards(message.payload);
    case "trench:get-batch-status":
      return getBatchStatus(message.payload.batchId);
    case "trench:open-options":
      if (message.payload?.section) {
        await chrome.storage.local.set({ [OPTIONS_TARGET_SECTION_KEY]: String(message.payload.section).trim() });
      }
      await chrome.runtime.openOptionsPage();
      return { opened: true };
    case "trench:open-external-url":
      return openExternalUrl(message.payload || {});
    case "trench:play-sound":
      return playSoundViaOffscreen(message.payload || {});
    default:
      throw new Error(`Unknown message type: ${message?.type ?? "undefined"}`);
  }
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  // The offscreen document relays playback requests via chrome.runtime.sendMessage,
  // which also hits this listener. Ignore its own envelope so the background doesn't
  // reply to itself (and so it doesn't bounce through handleMessage).
  if (message?.type === "trench:offscreen-play-sound") {
    return false;
  }
  handleMessage(message)
    .then((data) => sendResponse({ ok: true, data }))
    .catch((error) => {
      const serialized = serializeHostError(error);
      sendResponse({
        ok: false,
        error: serialized.message,
        errorCode: serialized.code,
        errorStatus: serialized.status,
        errorRetryable: serialized.retryable,
        errorTimeout: serialized.timeout
      });
    });
  return true;
});

async function reinjectSupportedTabs(reason = "runtime") {
  if (!chrome.scripting?.executeScript || !chrome.tabs?.query) {
    return;
  }

  for (const target of CONTENT_REINJECTION_TARGETS) {
    let tabs = [];
    try {
      tabs = await chrome.tabs.query({ url: target.matches });
    } catch (error) {
      console.warn(`Failed to enumerate tabs for ${target.loader}`, { reason, error });
      continue;
    }

    for (const tab of tabs) {
      if (!Number.isInteger(tab?.id) || tab.discarded) {
        continue;
      }
      try {
        await chrome.scripting.executeScript({
          target: { tabId: tab.id },
          files: [target.loader]
        });
      } catch (error) {
        console.warn(`Failed to reinject ${target.loader}`, {
          reason,
          tabId: tab.id,
          url: tab.url,
          error
        });
      }
    }
  }
}

chrome.runtime.onInstalled.addListener((details) => {
  void reinjectSupportedTabs(`installed:${details?.reason || "unknown"}`);
});
