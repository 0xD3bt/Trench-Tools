import {
  DEFAULT_HOST_BASE,
  DEFAULT_LAUNCHDECK_HOST_BASE,
  HOST_AUTH_TOKEN_STORAGE_KEY,
  HOST_STORAGE_KEY,
  LAUNCHDECK_HOST_STORAGE_KEY
} from "./constants.js";

export function normalizeHostBase(hostBase) {
  return (String(hostBase || DEFAULT_HOST_BASE).trim() || DEFAULT_HOST_BASE).replace(/\/+$/, "");
}

export function normalizeLaunchdeckHostBase(hostBase) {
  return (
    String(hostBase || DEFAULT_LAUNCHDECK_HOST_BASE).trim() || DEFAULT_LAUNCHDECK_HOST_BASE
  ).replace(/\/+$/, "");
}

export function isLoopbackHost(baseUrl) {
  try {
    const url = new URL(baseUrl);
    return ["127.0.0.1", "localhost"].includes(url.hostname);
  } catch {
    return false;
  }
}

export function originPatternFromHostBase(baseUrl) {
  const url = new URL(baseUrl);
  return `${url.origin}/*`;
}

async function getStoredToken(storageKey) {
  const stored = await chrome.storage.local.get(storageKey);
  const value = stored[storageKey];
  return typeof value === "string" ? value.trim() : "";
}

async function setStoredToken(storageKey, token) {
  const normalized = String(token || "").trim();
  await chrome.storage.local.set({ [storageKey]: normalized });
  return normalized;
}

export async function getHostBase() {
  return DEFAULT_HOST_BASE;
}

export async function setHostBase(_hostBase) {
  await chrome.storage.local.remove(HOST_STORAGE_KEY);
  return DEFAULT_HOST_BASE;
}

export async function getHostAuthToken() {
  return getStoredToken(HOST_AUTH_TOKEN_STORAGE_KEY);
}

export async function setHostAuthToken(token) {
  return setStoredToken(HOST_AUTH_TOKEN_STORAGE_KEY, token);
}

export async function getLaunchdeckHostBase() {
  return DEFAULT_LAUNCHDECK_HOST_BASE;
}

export async function setLaunchdeckHostBase(_hostBase) {
  await chrome.storage.local.remove(LAUNCHDECK_HOST_STORAGE_KEY);
  return DEFAULT_LAUNCHDECK_HOST_BASE;
}

export async function ensureHostPermission(baseUrl) {
  const normalized = normalizeHostBase(baseUrl);
  if (isLoopbackHost(normalized)) {
    return true;
  }
  const originPattern = originPatternFromHostBase(normalized);
  const granted = await chrome.permissions.contains({ origins: [originPattern] });
  if (granted) {
    return true;
  }
  return chrome.permissions.request({ origins: [originPattern] });
}
