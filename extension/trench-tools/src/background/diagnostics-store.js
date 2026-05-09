import {
  RUNTIME_DIAGNOSTICS_DISMISSED_KEY,
  RUNTIME_DIAGNOSTICS_REVISION_KEY,
  RUNTIME_DIAGNOSTICS_SNAPSHOT_KEY
} from "../shared/constants.js";

const ACTIVE = new Map();
let lastPersistSignature = "";

function nowMs() {
  return Date.now();
}

function normalizeSeverity(value) {
  const severity = String(value || "").trim().toLowerCase();
  return ["info", "warning", "critical"].includes(severity) ? severity : "info";
}

function normalizedFingerprint(entry) {
  const explicit = String(entry?.fingerprint || entry?.key || "").trim();
  if (explicit) return explicit;
  return [
    entry?.source || "runtime",
    entry?.endpointKind || "runtime",
    entry?.envVar || "",
    entry?.host || "",
    entry?.code || "diagnostic"
  ].map((part) => String(part || "").trim()).join(":");
}

function diagnosticDismissalSignature(entry) {
  return [
    entry?.severity || "",
    entry?.source || "",
    entry?.code || "",
    entry?.message || "",
    entry?.detail || "",
    entry?.envVar || "",
    entry?.endpointKind || "",
    entry?.host || "",
    entry?.restartRequired ? "restart" : ""
  ].map((part) => String(part || "").trim()).join("|");
}

export function normalizeDiagnostic(entry, sourceFallback = "runtime") {
  if (!entry || typeof entry !== "object") return null;
  const fingerprint = normalizedFingerprint(entry);
  if (!fingerprint) return null;
  const message = String(entry.message || entry.detail || entry.code || "Runtime diagnostic").trim();
  if (!message) return null;
  return {
    key: fingerprint,
    fingerprint,
    severity: normalizeSeverity(entry.severity),
    source: String(entry.source || sourceFallback).trim() || sourceFallback,
    code: String(entry.code || "diagnostic").trim() || "diagnostic",
    message,
    detail: typeof entry.detail === "string" && entry.detail.trim() ? entry.detail.trim() : null,
    envVar: typeof entry.envVar === "string" && entry.envVar.trim() ? entry.envVar.trim() : null,
    endpointKind: typeof entry.endpointKind === "string" && entry.endpointKind.trim() ? entry.endpointKind.trim() : null,
    host: typeof entry.host === "string" && entry.host.trim() ? entry.host.trim() : null,
    active: entry.active !== false,
    restartRequired: Boolean(entry.restartRequired),
    firstSeenAtMs: Number(entry.firstSeenAtMs || entry.firstSeenAt || entry.atMs || nowMs()),
    lastSeenAtMs: Number(entry.lastSeenAtMs || entry.lastSeenAt || entry.atMs || nowMs())
  };
}

async function readDismissed() {
  try {
    const stored = await chrome.storage.local.get(RUNTIME_DIAGNOSTICS_DISMISSED_KEY);
    const dismissed = stored[RUNTIME_DIAGNOSTICS_DISMISSED_KEY];
    return dismissed && typeof dismissed === "object" ? dismissed : {};
  } catch (_error) {
    return {};
  }
}

function stableDismissed(dismissed) {
  return Object.fromEntries(
    Object.entries(dismissed || {}).sort(([left], [right]) => left.localeCompare(right))
  );
}

function storageSignatureDiagnostic(entry) {
  return {
    key: entry.key,
    fingerprint: entry.fingerprint,
    severity: entry.severity,
    source: entry.source,
    code: entry.code,
    message: entry.message,
    detail: entry.detail,
    envVar: entry.envVar,
    endpointKind: entry.endpointKind,
    host: entry.host,
    active: entry.active,
    restartRequired: entry.restartRequired,
    firstSeenAtMs: entry.firstSeenAtMs
  };
}

async function hydrateFromStorage() {
  if (ACTIVE.size > 0) return;
  try {
    const stored = await chrome.storage.local.get(RUNTIME_DIAGNOSTICS_SNAPSHOT_KEY);
    const snapshot = stored[RUNTIME_DIAGNOSTICS_SNAPSHOT_KEY];
    const diagnostics = Array.isArray(snapshot?.diagnostics) ? snapshot.diagnostics : [];
    for (const entry of diagnostics) {
      const normalized = normalizeDiagnostic(entry, entry?.source || "runtime");
      if (normalized?.active) ACTIVE.set(normalized.fingerprint, normalized);
    }
  } catch (_error) {}
}

async function persist(dismissedOverride = null) {
  const dismissed = stableDismissed(dismissedOverride || await readDismissed());
  const diagnostics = Array.from(ACTIVE.values())
    .filter((entry) => entry.active)
    .sort((left, right) => {
      const rank = { critical: 0, warning: 1, info: 2 };
      return (rank[left.severity] ?? 9) - (rank[right.severity] ?? 9)
        || left.source.localeCompare(right.source)
        || left.message.localeCompare(right.message);
    });
  const signature = JSON.stringify({
    diagnostics: diagnostics.map(storageSignatureDiagnostic),
    dismissed
  });
  if (signature === lastPersistSignature) return;
  lastPersistSignature = signature;
  await chrome.storage.local.set({
    [RUNTIME_DIAGNOSTICS_SNAPSHOT_KEY]: {
      diagnostics,
      dismissed,
      updatedAtMs: nowMs()
    },
    [RUNTIME_DIAGNOSTICS_DISMISSED_KEY]: dismissed,
    [RUNTIME_DIAGNOSTICS_REVISION_KEY]: nowMs()
  });
}

export async function applyRuntimeDiagnostics(entries, sourceFallback = "runtime") {
  await hydrateFromStorage();
  const dismissed = await readDismissed();
  const previousForSource = Array.from(ACTIVE.values())
    .filter((entry) => entry.source === sourceFallback)
    .map((entry) => entry.fingerprint);
  const next = new Map();
  for (const entry of Array.isArray(entries) ? entries : []) {
    const normalized = normalizeDiagnostic(entry, sourceFallback);
    if (!normalized || !normalized.active) continue;
    const previous = ACTIVE.get(normalized.fingerprint);
    if (
      previous &&
      diagnosticDismissalSignature(previous) !== diagnosticDismissalSignature(normalized)
    ) {
      delete dismissed[normalized.fingerprint];
    }
    next.set(normalized.fingerprint, {
      ...normalized,
      firstSeenAtMs: previous?.firstSeenAtMs || normalized.firstSeenAtMs
    });
  }
  for (const [key, value] of ACTIVE.entries()) {
    if (value.source !== sourceFallback) {
      next.set(key, value);
    }
  }
  for (const fingerprint of previousForSource) {
    if (!next.has(fingerprint)) {
      delete dismissed[fingerprint];
    }
  }
  ACTIVE.clear();
  for (const [key, value] of next.entries()) ACTIVE.set(key, value);
  await persist(dismissed);
}

export async function applyServerDiagnosticEvent(entry) {
  await hydrateFromStorage();
  const normalized = normalizeDiagnostic(entry, entry?.source || "execution-engine");
  if (!normalized) return;
  const dismissed = await readDismissed();
  if (normalized.active) {
    const previous = ACTIVE.get(normalized.fingerprint);
    if (
      previous &&
      diagnosticDismissalSignature(previous) !== diagnosticDismissalSignature(normalized)
    ) {
      delete dismissed[normalized.fingerprint];
    }
    ACTIVE.set(normalized.fingerprint, {
      ...normalized,
      firstSeenAtMs: previous?.firstSeenAtMs || normalized.firstSeenAtMs
    });
  } else {
    ACTIVE.delete(normalized.fingerprint);
  }
  if (!normalized.active) {
    delete dismissed[normalized.fingerprint];
  }
  await persist(dismissed);
}

export async function dismissDiagnostic(fingerprint) {
  await hydrateFromStorage();
  const key = String(fingerprint || "").trim();
  if (!key) return { ok: false, error: "diagnostic fingerprint required" };
  const dismissed = await readDismissed();
  dismissed[key] = nowMs();
  await persist(dismissed);
  return { ok: true };
}

export async function getDiagnosticsSnapshot() {
  await hydrateFromStorage();
  const dismissed = await readDismissed();
  return {
    diagnostics: Array.from(ACTIVE.values()).filter((entry) => entry.active),
    dismissed,
    updatedAtMs: nowMs()
  };
}
