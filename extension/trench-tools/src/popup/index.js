import { callBackground } from "../shared/background-rpc.js";
import {
  HOST_AUTH_TOKEN_STORAGE_KEY,
  OPTIONS_TARGET_SECTION_KEY
} from "../shared/constants.js";

const PREFERENCES_KEY = "trenchTools.panelPreferences";
const BOOTSTRAP_REVISION_KEY = "trenchTools.bootstrapRevision";
const statusPill = document.getElementById("status-pill");
const presetCount = document.getElementById("preset-count");
const walletCount = document.getElementById("wallet-count");
const groupCount = document.getElementById("group-count");
const authGate = document.getElementById("auth-gate");
const mainView = document.getElementById("main-view");
const authTokenForm = document.getElementById("auth-token-form");
const authTokenInput = document.getElementById("auth-token-input");
const authTokenSubmit = document.getElementById("auth-token-submit");
const authTokenToggle = document.getElementById("auth-token-toggle");
const authTokenToggleEyeOn = authTokenToggle?.querySelector('[data-icon="eye"]');
const authTokenToggleEyeOff = authTokenToggle?.querySelector('[data-icon="eye-off"]');
const authTokenStatus = document.getElementById("auth-token-status");
const openOptionsButton = document.getElementById("open-options-button");
const connectionButton = document.getElementById("connection-button");
const quickBuyAmountInput = document.getElementById("quick-buy-amount");
const state = {
  hasStoredHost: false,
  hasAuthToken: false,
  isConnected: false
};

function normalizeQuickBuyAmountInput(value) {
  const trimmed = String(value || "").trim();
  if (!trimmed) {
    return "";
  }

  let normalized = trimmed.replace(/[^\d.]/g, "");
  const firstDotIndex = normalized.indexOf(".");
  if (firstDotIndex >= 0) {
    normalized =
      normalized.slice(0, firstDotIndex + 1) +
      normalized.slice(firstDotIndex + 1).replace(/\./g, "");
  }

  if (normalized.startsWith(".")) {
    normalized = `0${normalized}`;
  }

  if (normalized.includes(".")) {
    const [whole, fractional] = normalized.split(".");
    normalized = `${whole.replace(/^0+(?=\d)/, "") || "0"}.${fractional}`;
  } else {
    normalized = normalized.replace(/^0+(?=\d)/, "");
  }

  return normalized;
}

openOptionsButton.addEventListener("click", async () => {
  await chrome.storage.local.set({ [OPTIONS_TARGET_SECTION_KEY]: "presets" });
  await chrome.runtime.openOptionsPage();
});

connectionButton.addEventListener("click", async () => {
  if (state.hasAuthToken) {
    await chrome.storage.local.remove(HOST_AUTH_TOKEN_STORAGE_KEY);
    await init();
    return;
  }
  await chrome.storage.local.set({ [OPTIONS_TARGET_SECTION_KEY]: "global" });
  await chrome.runtime.openOptionsPage();
});

quickBuyAmountInput.addEventListener("input", async () => {
  const stored = await chrome.storage.local.get(PREFERENCES_KEY);
  const normalizedQuickBuyAmount = normalizeQuickBuyAmountInput(quickBuyAmountInput.value);
  quickBuyAmountInput.value = normalizedQuickBuyAmount;
  const preferences = {
    ...(stored[PREFERENCES_KEY] || {}),
    quickBuyAmount: normalizedQuickBuyAmount
  };
  await chrome.storage.local.set({ [PREFERENCES_KEY]: preferences });
});

authTokenForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  const token = String(authTokenInput.value || "").trim();
  if (!token) {
    setAuthStatus("Paste your auth token to connect.", "error");
    authTokenInput.focus();
    return;
  }

  authTokenSubmit.disabled = true;
  authTokenSubmit.textContent = "Connecting...";
  setAuthStatus("", "info");
  try {
    await chrome.storage.local.set({ [HOST_AUTH_TOKEN_STORAGE_KEY]: token });
    await callBackground("trench:refresh-host-connection");
    await init();
  } catch (error) {
    setAuthStatus(error?.message || "Could not save auth token.", "error");
  } finally {
    authTokenSubmit.disabled = false;
    authTokenSubmit.textContent = "Connect";
  }
});

authTokenInput.addEventListener("input", () => {
  if (authTokenInput.hasAttribute("aria-invalid")) {
    setAuthStatus("", "info");
  }
});

if (authTokenToggle) {
  authTokenToggle.addEventListener("click", () => {
    const showing = authTokenInput.type === "text";
    authTokenInput.type = showing ? "password" : "text";
    authTokenToggle.setAttribute("aria-pressed", showing ? "false" : "true");
    authTokenToggle.setAttribute("aria-label", showing ? "Show auth token" : "Hide auth token");
    authTokenToggleEyeOn?.classList.toggle("hidden", !showing);
    authTokenToggleEyeOff?.classList.toggle("hidden", showing);
  });
}

function setAuthStatus(message, tone = "info") {
  authTokenStatus.textContent = message;
  authTokenStatus.dataset.tone = tone;
  if (tone === "error" && message) {
    authTokenInput.setAttribute("aria-invalid", "true");
  } else {
    authTokenInput.removeAttribute("aria-invalid");
  }
}

function showAuthGate({ message, focus = false } = {}) {
  state.isConnected = false;
  authGate.classList.remove("hidden");
  mainView.classList.add("hidden");
  statusPill.textContent = "Needs auth token";
  statusPill.dataset.state = "offline";
  if (message !== undefined) {
    setAuthStatus(message, message ? "error" : "info");
  }
  presetCount.textContent = "0";
  walletCount.textContent = "0";
  groupCount.textContent = "0";
  if (focus) {
    setTimeout(() => authTokenInput.focus(), 0);
  }
}

function showMainView() {
  authGate.classList.add("hidden");
  mainView.classList.remove("hidden");
  setAuthStatus("", "info");
}

async function init() {
  const storedHost = await chrome.storage.local.get(HOST_AUTH_TOKEN_STORAGE_KEY);
  const configuredToken = String(storedHost[HOST_AUTH_TOKEN_STORAGE_KEY] || "").trim();
  state.hasStoredHost = true;
  state.hasAuthToken = Boolean(configuredToken);

  if (!state.hasAuthToken) {
    showAuthGate();
    return;
  }

  try {
    const [health, bootstrap, stored] = await Promise.all([
      callBackground("trench:get-health"),
      callBackground("trench:get-bootstrap"),
      chrome.storage.local.get(PREFERENCES_KEY)
    ]);
    const preferences = stored[PREFERENCES_KEY] || {};
    state.isConnected = true;
    authTokenInput.value = "";
    statusPill.textContent = `Host ${health.engineVersion}`;
    statusPill.dataset.state = "online";
    presetCount.textContent = String(bootstrap.presets.length);
    walletCount.textContent = String(bootstrap.wallets.length);
    groupCount.textContent = String(bootstrap.walletGroups.length);
    quickBuyAmountInput.value = normalizeQuickBuyAmountInput(preferences.quickBuyAmount || "");
    showMainView();
  } catch (error) {
    if (error?.code === "HOST_UNAUTHORIZED" || error?.status === 401) {
      await chrome.storage.local.remove(HOST_AUTH_TOKEN_STORAGE_KEY);
      showAuthGate({
        message: "Auth token rejected. Paste the current token from the launcher.",
        focus: true
      });
      return;
    }
    const stored = await chrome.storage.local.get(PREFERENCES_KEY);
    const preferences = stored[PREFERENCES_KEY] || {};
    state.isConnected = false;
    statusPill.textContent = state.hasStoredHost ? "Host unavailable" : "No host set";
    statusPill.dataset.state = "offline";
    presetCount.textContent = "0";
    walletCount.textContent = "0";
    groupCount.textContent = "0";
    quickBuyAmountInput.value = normalizeQuickBuyAmountInput(preferences.quickBuyAmount || "");
    showMainView();
  }
  connectionButton.textContent = "Disconnect";
}

chrome.storage.onChanged.addListener((changes, areaName) => {
  if (
    areaName === "local" &&
    (changes[HOST_AUTH_TOKEN_STORAGE_KEY] || changes[BOOTSTRAP_REVISION_KEY])
  ) {
    init();
  }
});

setInterval(() => {
  if (state.hasAuthToken) {
    void callBackground("trench:get-runtime-status").catch(() => {});
  }
}, 15000);

init();
