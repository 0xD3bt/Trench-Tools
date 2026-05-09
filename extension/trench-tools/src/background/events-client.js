// events-client.js
// SSE consumer for the /api/extension/events/stream endpoint. MV3 service
// workers don't expose EventSource, so we run our own fetch+ReadableStream
// parser. Balance + trade events are routed to balances-store; connection
// state changes drive the `meta` dot on every surface.

import { ensureSecureTransport, getCachedHostBase } from "./execution-client.js";
import {
  applyServerSnapshot,
  applyServerBalanceEvent,
  applyServerMarkEvent,
  applyServerConnectionState,
  handleTradeEvent,
} from "./balances-store.js";
import { applyServerDiagnosticEvent } from "./diagnostics-store.js";

const SSE_PATH = "/api/extension/events/stream";
const HOST_AUTH_TOKEN_STORAGE_KEY = "trenchTools.hostAuthToken";

const BACKOFF_BASE_MS = 1_000;
const BACKOFF_CAP_MS = 30_000;
const CONNECT_TIMEOUT_MS = 10_000;

const state = {
  running: false,
  attempt: 0,
  abortController: null,
  reconnectTimer: null,
};

/**
 * Start the stream consumer. Safe to call multiple times; only one session
 * runs at a time. Returns a stop() function.
 */
export function startEventsClient() {
  if (state.running) return stopEventsClient;
  state.running = true;
  applyServerConnectionState({ state: "connecting", error: null });
  runSessionLoop();
  return stopEventsClient;
}

export function stopEventsClient() {
  state.running = false;
  if (state.abortController) {
    try {
      state.abortController.abort();
    } catch (_error) {}
    state.abortController = null;
  }
  if (state.reconnectTimer) {
    clearTimeout(state.reconnectTimer);
    state.reconnectTimer = null;
  }
  applyServerConnectionState({ state: "disconnected", error: null });
}

async function runSessionLoop() {
  while (state.running) {
    let reason = "closed";
    try {
      await runSession();
    } catch (error) {
      reason = error?.message || String(error);
    }
    if (!state.running) break;
    applyServerConnectionState({ state: "disconnected", error: reason });
    const delayMs = computeBackoffMs(state.attempt);
    state.attempt = Math.min(state.attempt + 1, 10);
    await waitForReconnect(delayMs);
  }
}

function computeBackoffMs(attempt) {
  const capped = Math.min(attempt, 10);
  const delay = BACKOFF_BASE_MS * Math.pow(2, capped);
  return Math.min(delay, BACKOFF_CAP_MS);
}

function waitForReconnect(delayMs) {
  return new Promise((resolve) => {
    state.reconnectTimer = setTimeout(() => {
      state.reconnectTimer = null;
      resolve();
    }, delayMs);
  });
}

async function runSession() {
  const [baseUrl, authToken] = await Promise.all([
    getCachedHostBase(),
    readAuthToken(),
  ]);
  if (!authToken) {
    throw new Error("no auth token");
  }
  ensureSecureTransport(baseUrl);
  const url = `${baseUrl}${SSE_PATH}`;
  const controller = new AbortController();
  state.abortController = controller;

  applyServerConnectionState({ state: "connecting", error: null });

  const connectTimeout = setTimeout(() => {
    try {
      controller.abort();
    } catch (_error) {}
  }, CONNECT_TIMEOUT_MS);

  let response;
  try {
    response = await fetch(url, {
      method: "GET",
      headers: {
        accept: "text/event-stream",
        "cache-control": "no-cache",
        authorization: `Bearer ${authToken}`,
      },
      signal: controller.signal,
    });
  } finally {
    clearTimeout(connectTimeout);
  }

  if (!response.ok || !response.body) {
    throw new Error(`stream http ${response.status}`);
  }

  applyServerConnectionState({ state: "live", error: null });
  state.attempt = 0;

  const reader = response.body.getReader();
  const decoder = new TextDecoder("utf-8");
  let buffer = "";

  try {
    while (state.running) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      let separatorIndex;
      while ((separatorIndex = buffer.indexOf("\n\n")) !== -1) {
        const rawEvent = buffer.slice(0, separatorIndex);
        buffer = buffer.slice(separatorIndex + 2);
        const parsed = parseSseEvent(rawEvent);
        if (parsed) {
          dispatchEvent(parsed);
        }
      }
    }
  } finally {
    try {
      reader.releaseLock();
    } catch (_error) {}
    if (state.abortController === controller) {
      state.abortController = null;
    }
  }
}

function parseSseEvent(rawEvent) {
  if (!rawEvent) return null;
  let eventName = "message";
  const dataLines = [];
  for (const line of rawEvent.split("\n")) {
    if (line.startsWith(":")) continue;
    if (line.startsWith("event:")) {
      eventName = line.slice(6).trim();
    } else if (line.startsWith("data:")) {
      dataLines.push(line.slice(5).trim());
    }
  }
  if (dataLines.length === 0) return null;
  const dataRaw = dataLines.join("\n");
  let data;
  try {
    data = JSON.parse(dataRaw);
  } catch (_error) {
    return null;
  }
  return { event: eventName, data };
}

function dispatchEvent({ event, data }) {
  switch (event) {
    case "snapshot":
      applyServerSnapshot(data);
      break;
    case "balance":
      applyServerBalanceEvent(data);
      break;
    case "trade":
      handleTradeEvent(data);
      break;
    case "mark":
      applyServerMarkEvent(data);
      break;
    case "connectionState":
      if (data && typeof data === "object") {
        applyServerConnectionState({
          state: typeof data.state === "string" ? data.state : "connecting",
          error: typeof data.error === "string" ? data.error : null,
        });
      }
      break;
    case "diagnostic":
      void applyServerDiagnosticEvent(data);
      break;
    default:
      break;
  }
}

async function readAuthToken() {
  try {
    const stored = await chrome.storage.local.get([HOST_AUTH_TOKEN_STORAGE_KEY]);
    const value = stored[HOST_AUTH_TOKEN_STORAGE_KEY];
    return typeof value === "string" ? value.trim() : "";
  } catch (_error) {
    return "";
  }
}
