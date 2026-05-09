const fs = require("fs");
const path = require("path");
const WebSocket = require("ws");

const DEFAULT_EXECUTION_PORT = 8788;
const DEFAULT_LAUNCHDECK_PORT = 8789;
const DEFAULT_FOLLOW_PORT = 8790;
const FETCH_TIMEOUT_MS = 3_000;
const RPC_PROBE_TIMEOUT_MS = 3_000;
const WS_PROBE_TIMEOUT_MS = 4_000;

function projectRoot() {
  return path.resolve(__dirname, "..");
}

function supportsColor() {
  return !process.env.NO_COLOR && Boolean(process.stdout.isTTY);
}

const colorEnabled = supportsColor();
const ansi = {
  green: "\u001b[32m",
  yellow: "\u001b[33m",
  red: "\u001b[31m",
  cyan: "\u001b[36m",
  reset: "\u001b[0m"
};

function color(value, name) {
  if (!colorEnabled) return value;
  return `${ansi[name] || ""}${value}${ansi.reset}`;
}

function statusLabel(status) {
  const normalized = String(status || "").toUpperCase();
  if (normalized === "OK") return color("OK", "green");
  if (normalized === "WARN") return color("WARN", "yellow");
  if (normalized === "FAIL" || normalized === "CRITICAL") return color(normalized, "red");
  return normalized;
}

function section(title) {
  console.log("");
  console.log(title);
}

function row(status, name, detail = "", note = "") {
  const normalized = String(status || "").toUpperCase();
  const statusText = `${statusLabel(normalized)}${" ".repeat(Math.max(1, 8 - normalized.length))}`;
  const nameText = String(name || "").padEnd(22);
  const pieces = [`  ${statusText}`, nameText];
  if (detail) pieces.push(detail);
  if (note) pieces.push(note);
  console.log(pieces.join("  ").replace(/\s+$/, ""));
}

function resolveProjectPath(rawPath) {
  const value = String(rawPath || "").trim();
  if (!value) return "";
  return path.isAbsolute(value) ? value : path.join(projectRoot(), value);
}

function defaultDataRoot(env) {
  return resolveProjectPath(env.TRENCH_TOOLS_DATA_ROOT || ".local/trench-tools");
}

function tokenPathFromEnv(env) {
  return resolveProjectPath(
    env.LAUNCHDECK_EXECUTION_ENGINE_TOKEN_FILE ||
      path.join(defaultDataRoot(env), "default-engine-token.txt")
  );
}

function readDefaultToken(env) {
  if (env.LAUNCHDECK_EXECUTION_ENGINE_TOKEN) {
    return String(env.LAUNCHDECK_EXECUTION_ENGINE_TOKEN).trim();
  }
  const tokenPath = tokenPathFromEnv(env);
  try {
    return fs.readFileSync(tokenPath, "utf8").trim();
  } catch {
    return "";
  }
}

function parseEnvFile() {
  const envPath = path.join(projectRoot(), ".env");
  const values = {};
  let contents = "";
  try {
    contents = fs.readFileSync(envPath, "utf8");
  } catch {
    return values;
  }
  for (const rawLine of contents.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    const index = line.indexOf("=");
    if (index <= 0) continue;
    const key = line.slice(0, index).trim().replace(/^export\s+/, "");
    let value = line.slice(index + 1).trim();
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }
    values[key] = value;
  }
  return values;
}

function endpointHost(value) {
  const trimmed = String(value || "").trim();
  if (!trimmed) return "";
  try {
    return new URL(trimmed).host;
  } catch {
    return "<invalid-url>";
  }
}

function sanitizeError(error) {
  const message = String(error?.message || error || "unknown error");
  return message
    .replace(
      /([?&](?:api[-_]?key|apikey|key|token|access[_-]?token|auth|authorization|private[_-]?key)=)[^&\s)]+/gi,
      "$1<redacted>"
    )
    .replace(
      /\b(api[-_]?key|apikey|access[_-]?token|token|authorization|private[_-]?key)=([^&\s)]+)/gi,
      "$1=<redacted>"
    )
    .slice(0, 180);
}

async function fetchJson(url, token, timeoutMs = FETCH_TIMEOUT_MS) {
  const headers = { accept: "application/json" };
  if (token) headers.authorization = `Bearer ${token}`;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(url, { headers, cache: "no-store", signal: controller.signal });
    const text = await response.text();
    let payload = null;
    try {
      payload = text ? JSON.parse(text) : null;
    } catch {
      payload = { error: text.slice(0, 160) };
    }
    if (!response.ok) {
      const message = payload?.error || `${response.status} ${response.statusText}`.trim();
      throw new Error(message);
    }
    return payload;
  } catch (error) {
    if (error?.name === "AbortError") {
      throw new Error(`timed out after ${timeoutMs}ms`);
    }
    throw error;
  } finally {
    clearTimeout(timer);
  }
}

async function postJson(url, payload, timeoutMs = RPC_PROBE_TIMEOUT_MS) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json", accept: "application/json" },
      body: JSON.stringify(payload),
      cache: "no-store",
      signal: controller.signal
    });
    const text = await response.text();
    let parsed = null;
    try {
      parsed = text ? JSON.parse(text) : null;
    } catch {
      parsed = { error: { message: text.slice(0, 160) } };
    }
    if (!response.ok) {
      throw new Error(`${response.status} ${response.statusText}`.trim());
    }
    return parsed;
  } catch (error) {
    if (error?.name === "AbortError") {
      throw new Error(`timed out after ${timeoutMs}ms`);
    }
    throw error;
  } finally {
    clearTimeout(timer);
  }
}

async function checkFollowDaemonHealth(url) {
  try {
    const payload = await fetchJson(`${url}/health`, "");
    if (!payload || typeof payload !== "object" || !Object.prototype.hasOwnProperty.call(payload, "activeJobs")) {
      throw new Error("unexpected follow daemon health response");
    }
    return { ok: true, error: "" };
  } catch (error) {
    return { ok: false, error: error.message };
  }
}

async function collectStartupDiagnostics({ includeExecution = true, includeLaunchdeck = true } = {}) {
  const env = { ...parseEnvFile(), ...process.env };
  const token = readDefaultToken(env);
  const executionPort = Number(env.EXECUTION_ENGINE_PORT || DEFAULT_EXECUTION_PORT);
  const launchdeckPort = Number(env.LAUNCHDECK_PORT || DEFAULT_LAUNCHDECK_PORT);
  const followPort = Number(env.LAUNCHDECK_FOLLOW_DAEMON_PORT || DEFAULT_FOLLOW_PORT);
  const executionUrl = `http://127.0.0.1:${executionPort}`;
  const launchdeckUrl = `http://127.0.0.1:${launchdeckPort}`;
  const followUrl = String(env.LAUNCHDECK_FOLLOW_DAEMON_URL || `http://127.0.0.1:${followPort}`).replace(/\/+$/, "");
  const diagnostics = [];
  const services = [];

  if (includeExecution) {
    try {
      const status = await fetchJson(`${executionUrl}/api/extension/runtime-status`, token);
      diagnostics.push(...(Array.isArray(status?.diagnostics) ? status.diagnostics : []));
      services.push({ name: "Execution Engine", ok: true, url: executionUrl });
    } catch (error) {
      services.push({ name: "Execution Engine", ok: false, url: executionUrl, error: error.message });
      diagnostics.push({
        severity: "critical",
        message: "Execution engine host unreachable or rejected local auth.",
        source: "startup",
        code: "execution_host_unreachable"
      });
    }
  }

  if (includeLaunchdeck) {
    try {
      const status = await fetchJson(`${launchdeckUrl}/api/runtime-status`, token);
      diagnostics.push(...(Array.isArray(status?.diagnostics) ? status.diagnostics : []));
      services.push({ name: "LaunchDeck Engine", ok: true, url: launchdeckUrl });
    } catch (error) {
      services.push({ name: "LaunchDeck Engine", ok: false, url: launchdeckUrl, error: error.message });
      diagnostics.push({
        severity: "critical",
        message: "LaunchDeck engine host unreachable or rejected local auth.",
        source: "startup",
        code: "launchdeck_host_unreachable"
      });
    }
  }

  if (includeLaunchdeck) {
    const followCheck = await checkFollowDaemonHealth(followUrl);
    services.push({
      name: "Follow Daemon",
      ok: followCheck.ok,
      url: followUrl,
      error: followCheck.error
    });
    if (!followCheck.ok) {
      diagnostics.push({
        severity: "warning",
        message: "Follow daemon health check failed.",
        source: "startup",
        code: "follow_daemon_unreachable"
      });
    }
  }

  const rpcHealth = await probeRpcHealth(env);
  const wsHealth = await probeWsHealth(env);
  return { services, diagnostics, env, rpcHealth, wsHealth, tokenPath: tokenPathFromEnv(env) };
}

async function probeHttpRpc(envVar, url) {
  const host = endpointHost(url);
  if (!url) {
    return { envVar, status: "WARN", host: "", detail: "not set" };
  }
  const started = Date.now();
  try {
    const payload = await postJson(url, {
      jsonrpc: "2.0",
      id: 1,
      method: "getLatestBlockhash",
      params: [{ commitment: "confirmed" }]
    });
    if (payload?.error) {
      throw new Error(payload.error.message || JSON.stringify(payload.error));
    }
    if (!payload?.result) {
      throw new Error("missing result");
    }
    return {
      envVar,
      status: "OK",
      host,
      detail: `getLatestBlockhash ${Date.now() - started}ms`
    };
  } catch (error) {
    return { envVar, status: "FAIL", host, detail: sanitizeError(error) };
  }
}

async function probeRpcHealth(env) {
  const primaryUrl = String(env.SOLANA_RPC_URL || "").trim();
  const warmUrl = String(env.WARM_RPC_URL || "").trim();
  const primary = await probeHttpRpc("SOLANA_RPC_URL", primaryUrl);
  const probes = [primary];

  if (warmUrl) {
    if (warmUrl === primaryUrl) {
      return probes;
    } else {
      const warm = await probeHttpRpc("WARM_RPC_URL", warmUrl);
      if (warm.status === "FAIL" && primary.status === "OK") {
        warm.status = "WARN";
        warm.detail = `${warm.detail}, optional warm RPC unavailable; primary remains active`;
      }
      probes.push(warm);
    }
  } else {
    probes.push({
      envVar: "WARM_RPC_URL",
      status: primary.status === "OK" ? "OK" : "WARN",
      host: primary.host,
      detail: "not set, using primary"
    });
  }
  return probes;
}

function probeWebSocket(envVar, url) {
  const host = endpointHost(url);
  if (!url) {
    return Promise.resolve({ envVar, status: "WARN", host: "", detail: "not set" });
  }
  const started = Date.now();
  return new Promise((resolve) => {
    let settled = false;
    let socket;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try {
        if (socket && socket.readyState === WebSocket.OPEN) socket.close();
      } catch {
      }
      resolve(result);
    };
    const timer = setTimeout(() => {
      finish({ envVar, status: "FAIL", host, detail: `timed out after ${WS_PROBE_TIMEOUT_MS}ms` });
    }, WS_PROBE_TIMEOUT_MS);

    try {
      socket = new WebSocket(url);
      socket.on("open", () => {
        socket.send(JSON.stringify({ jsonrpc: "2.0", id: 1, method: "slotSubscribe" }));
      });
      socket.on("message", (raw) => {
        let payload;
        try {
          payload = JSON.parse(String(raw));
        } catch {
          return;
        }
        if (payload?.id === 1) {
          const latency = Date.now() - started;
          const subscriptionId = payload.result;
          if (subscriptionId !== undefined) {
            try {
              socket.send(
                JSON.stringify({
                  jsonrpc: "2.0",
                  id: 2,
                  method: "slotUnsubscribe",
                  params: [subscriptionId]
                })
              );
            } catch {
            }
            finish({ envVar, status: "OK", host, detail: `slotSubscribe ${latency}ms` });
          } else {
            finish({ envVar, status: "FAIL", host, detail: "subscription rejected" });
          }
        }
      });
      socket.on("error", (error) => {
        finish({ envVar, status: "FAIL", host, detail: sanitizeError(error) });
      });
      socket.on("close", () => {
        finish({ envVar, status: "FAIL", host, detail: "connection closed before subscription" });
      });
    } catch (error) {
      finish({ envVar, status: "FAIL", host, detail: sanitizeError(error) });
    }
  });
}

async function probeWsHealth(env) {
  const primaryUrl = String(env.SOLANA_WS_URL || "").trim();
  const warmUrl = String(env.WARM_WS_URL || "").trim();
  const primary = await probeWebSocket("SOLANA_WS_URL", primaryUrl);
  const probes = [primary];

  if (warmUrl) {
    if (warmUrl === primaryUrl) {
      if (primary.status === "FAIL") {
        primary.status = "CRITICAL";
      }
      return probes;
    } else {
      const warm = await probeWebSocket("WARM_WS_URL", warmUrl);
      if (warm.status === "FAIL" && primary.status === "OK") {
        warm.status = "WARN";
        warm.detail = `${warm.detail}, optional warm WS unavailable; primary remains active`;
      }
      probes.push(warm);
    }
  } else {
    probes.push({
      envVar: "WARM_WS_URL",
      status: primary.status === "OK" ? "OK" : "WARN",
      host: primary.host,
      detail: "not set, using primary"
    });
  }

  const primaryFailed = primary.status === "FAIL";
  const warmFailed = probes.some((probe) => probe.envVar === "WARM_WS_URL" && probe.status === "FAIL");
  if (primaryFailed && (!warmUrl || warmFailed)) {
    primary.status = "CRITICAL";
  }
  return probes;
}

function diagnosticSeverity(entry) {
  const raw = String(entry.severity || "info").toLowerCase();
  if (raw === "critical" || raw === "error") return "CRITICAL";
  if (raw === "warning" || raw === "warn") return "WARN";
  return "WARN";
}

function probeMap(result) {
  const probes = new Map(
    [...result.rpcHealth, ...result.wsHealth].map((probe) => [probe.envVar, probe])
  );
  if (result.env.WARM_RPC_URL && result.env.WARM_RPC_URL === result.env.SOLANA_RPC_URL) {
    probes.set("WARM_RPC_URL", probes.get("SOLANA_RPC_URL"));
  }
  if (result.env.WARM_WS_URL && result.env.WARM_WS_URL === result.env.SOLANA_WS_URL) {
    probes.set("WARM_WS_URL", probes.get("SOLANA_WS_URL"));
  }
  return probes;
}

function diagnosticIsCoveredByProbe(diagnostic, probes) {
  if (!diagnostic.envVar || diagnostic.restartRequired) return false;
  const probe = probes.get(diagnostic.envVar);
  if (!probe || probe.status === "OK") return false;
  if (diagnostic.host && probe.host && diagnostic.host !== probe.host) return false;
  return true;
}

function dedupeActions(items) {
  const seen = new Set();
  return items.filter((item) => {
    const key = `${item.severity}:${item.message}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function actionItems(result) {
  const items = [];
  const probes = probeMap(result);
  for (const service of result.services) {
    if (!service.ok) {
      items.push({
        severity: "CRITICAL",
        message: `${service.name} is not reachable. Check its log and restart the launcher.`
      });
    }
  }
  for (const probe of [...result.rpcHealth, ...result.wsHealth]) {
    if (probe.status === "WARN" || probe.status === "FAIL" || probe.status === "CRITICAL") {
      const host = probe.host ? ` (${probe.host})` : "";
      items.push({
        severity: probe.status,
        message: `${probe.envVar}${host}: ${probe.detail}`
      });
    }
  }
  for (const diagnostic of result.diagnostics) {
    const severity = diagnosticSeverity(diagnostic);
    if (severity === "CRITICAL" || severity === "WARN") {
      if (diagnosticIsCoveredByProbe(diagnostic, probes)) {
        continue;
      }
      const envVar = diagnostic.envVar ? `${diagnostic.envVar}: ` : "";
      items.push({
        severity,
        message: `${envVar}${diagnostic.message || diagnostic.code || "runtime diagnostic"}`
      });
    }
  }
  return dedupeActions(items);
}

async function printStartupDiagnostics(options = {}) {
  const result = await collectStartupDiagnostics(options);
  const actions = actionItems(result);

  console.log("");
  console.log("Trench Tools startup");

  section("Services");
  for (const service of result.services) {
    row(service.ok ? "OK" : "CRITICAL", service.name, service.ok ? service.url : sanitizeError(service.error));
  }

  section("RPC health");
  for (const probe of [...result.rpcHealth, ...result.wsHealth]) {
    const host = probe.host || "-";
    row(probe.status, probe.envVar, host, probe.detail);
  }

  const probes = probeMap(result);
  const diagnosticKeys = new Set();
  const visibleDiagnostics = result.diagnostics.filter((entry) => {
    if (!["CRITICAL", "WARN"].includes(diagnosticSeverity(entry))) return false;
    if (diagnosticIsCoveredByProbe(entry, probes)) return false;
    const key = `${entry.envVar || ""}:${entry.source || ""}:${entry.message || entry.code || ""}:${entry.host || ""}`;
    if (diagnosticKeys.has(key)) return false;
    diagnosticKeys.add(key);
    return true;
  });
  if (visibleDiagnostics.length) {
    section("Details");
    for (const diagnostic of visibleDiagnostics) {
      const envVar = diagnostic.envVar ? `${diagnostic.envVar}: ` : "";
      const host = diagnostic.host ? ` (${diagnostic.host})` : "";
      row(
        diagnosticSeverity(diagnostic),
        diagnostic.source || "runtime",
        "",
        `${envVar}${diagnostic.message || diagnostic.code || "diagnostic"}${host}`
      );
    }
  }

  if (actions.length) {
    section("Action needed");
    for (const item of actions) {
      console.log(`  ${statusLabel(item.severity)}  ${item.message}`);
    }
  }

  section("Extension auth");
  if (options.includeExecution === false) {
    console.log("  No Execution Engine auth token is needed for LaunchDeck-only mode.");
  } else {
    console.log(`  Token file: ${color(result.tokenPath, "cyan")}`);
    console.log("  Paste this token into the extension. Keep it private.");
  }
}

module.exports = {
  collectStartupDiagnostics,
  printStartupDiagnostics
};
