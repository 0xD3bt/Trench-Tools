"use strict";

const ENGINE_HOST = "127.0.0.1";
const ENGINE_PORT = Number(process.env.LAUNCHDECK_ENGINE_PORT || 8790);
const ENGINE_BASE_URL = String(process.env.LAUNCHDECK_ENGINE_URL || `http://${ENGINE_HOST}:${ENGINE_PORT}`).replace(/\/+$/, "");
const ENGINE_BACKEND = String(process.env.LAUNCHDECK_ENGINE_BACKEND || "rust").trim().toLowerCase();
const ENGINE_AUTH_TOKEN = String(process.env.LAUNCHDECK_ENGINE_AUTH_TOKEN || "").trim();

function getEngineBackendMode() {
  return ENGINE_BACKEND === "rust" ? "rust" : "rust";
}

function buildEngineHeaders(extraHeaders = {}) {
  const headers = {
    ...extraHeaders,
  };
  if (ENGINE_AUTH_TOKEN) {
    headers["x-launchdeck-engine-auth"] = ENGINE_AUTH_TOKEN;
  }
  return headers;
}

async function requestEngine(pathname, payload, { method = "POST" } = {}) {
  const upperMethod = String(method || "POST").toUpperCase();
  const response = await fetch(`${ENGINE_BASE_URL}${pathname}`, {
    method: upperMethod,
    headers: buildEngineHeaders(
      upperMethod === "GET"
        ? {}
        : { "content-type": "application/json" }
    ),
    body: upperMethod === "GET" ? undefined : JSON.stringify(payload || {}),
  });
  const rawText = await response.text();
  const data = rawText ? JSON.parse(rawText) : {};
  if (!response.ok) {
    const message = data && data.error ? data.error : `Engine request failed (${response.status}).`;
    throw new Error(message);
  }
  return data;
}

function getEngineHealth() {
  return requestEngine("/health", undefined, { method: "GET" });
}

function runEngineAction(action, payload) {
  return requestEngine(`/engine/${encodeURIComponent(action)}`, payload, { method: "POST" });
}

function startEngineRuntime(worker, config = {}) {
  return requestEngine("/engine/runtime/start", { worker, config }, { method: "POST" });
}

function stopEngineRuntime(worker, note = "") {
  return requestEngine("/engine/runtime/stop", { worker, note }, { method: "POST" });
}

function heartbeatEngineRuntime(worker, note = "") {
  return requestEngine("/engine/runtime/heartbeat", { worker, note }, { method: "POST" });
}

function failEngineRuntime(worker, note = "") {
  return requestEngine("/engine/runtime/fail", { worker, note }, { method: "POST" });
}

function getEngineRuntimeStatus(worker = "") {
  return requestEngine("/engine/runtime/status", { worker }, { method: "POST" });
}

module.exports = {
  ENGINE_BASE_URL,
  ENGINE_PORT,
  failEngineRuntime,
  getEngineBackendMode,
  getEngineHealth,
  getEngineRuntimeStatus,
  heartbeatEngineRuntime,
  runEngineAction,
  startEngineRuntime,
  stopEngineRuntime,
};
