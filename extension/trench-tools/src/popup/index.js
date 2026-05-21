import { callBackground } from "../shared/background-rpc.js";
import {
  HOST_AUTH_TOKEN_STORAGE_KEY,
  OPTIONS_TARGET_SECTION_KEY
} from "../shared/constants.js";
import {
  normalizeTradePreferences,
  normalizeQuickBuyAmountInput,
  normalizeWalletSelectionPreference,
  mirrorWalletSelectionPreferenceOntoPreferences,
  tradePreferencePatchFromValue
} from "../shared/trade-preferences.js";

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
const quickBuyAmount2Input = document.getElementById("quick-buy-amount-2");
const presetSelect = document.getElementById("preset-select");
const walletDropdownButton = document.getElementById("wallet-dropdown-button");
const walletDropdownLabel = document.getElementById("wallet-dropdown-label");
const walletDropdownMenu = document.getElementById("wallet-dropdown-menu");
const state = {
  hasStoredHost: false,
  hasAuthToken: false,
  isConnected: false,
  bootstrap: {
    presets: [],
    wallets: [],
    walletGroups: []
  },
  preferences: {}
};
let authTokenSaveInProgress = false;

function normalizeWalletKeys(keys) {
  return Array.from(
    new Set((Array.isArray(keys) ? keys : []).map((key) => String(key || "").trim()).filter(Boolean))
  );
}

function mirrorWalletSelectionPreference(preferences, selection) {
  const next = { ...preferences };
  mirrorWalletSelectionPreferenceOntoPreferences(next, selection);
  return next;
}

function enabledWallets() {
  return (Array.isArray(state.bootstrap.wallets) ? state.bootstrap.wallets : []).filter(
    (wallet) => wallet?.enabled !== false
  );
}

function compactEnvWalletLabel(value) {
  const label = String(value || "").trim();
  const genericMatch = label.match(/^SOLANA_PRIVATE_KEY(\d+)?$/i);
  if (!genericMatch) {
    return label;
  }
  return `#${genericMatch[1] || "1"}`;
}

function walletLabel(wallet, index) {
  const raw = compactEnvWalletLabel(wallet?.label || wallet?.name || wallet?.key || `Wallet ${index + 1}`);
  if (raw.length <= 16) {
    return raw;
  }
  return `${raw.slice(0, 7)}…${raw.slice(-5)}`;
}

function selectedWalletKeys(selection = normalizeWalletSelectionPreference(state.preferences)) {
  if (selection.selectionSource === "group") {
    const group = state.bootstrap.walletGroups.find(
      (entry) => String(entry?.id || "").trim() === selection.activeWalletGroupId
    );
    return normalizeWalletKeys(group?.walletKeys);
  }
  return normalizeWalletKeys(selection.manualWalletKeys);
}

function fallbackSelection(selection) {
  const wallets = enabledWallets();
  const groups = Array.isArray(state.bootstrap.walletGroups) ? state.bootstrap.walletGroups : [];
  const knownWalletKeys = new Set(wallets.map((wallet) => wallet.key).filter(Boolean));
  if (selection.selectionSource === "group") {
    const knownGroupIds = new Set(groups.map((group) => group.id).filter(Boolean));
    if (knownGroupIds.has(selection.activeWalletGroupId)) {
      return selection;
    }
    if (groups[0]) {
      return {
        selectionSource: "group",
        activeWalletGroupId: groups[0].id,
        manualWalletKeys: []
      };
    }
  }
  const manualWalletKeys = normalizeWalletKeys(selection.manualWalletKeys).filter((key) =>
    knownWalletKeys.has(key)
  );
  return {
    selectionSource: "manual",
    activeWalletGroupId: selection.activeWalletGroupId,
    manualWalletKeys: manualWalletKeys.length ? manualWalletKeys : normalizeWalletKeys([wallets[0]?.key])
  };
}

function currentSelection() {
  return fallbackSelection(normalizeWalletSelectionPreference(state.preferences));
}

async function savePreferences(nextPreferences) {
  state.preferences = normalizeTradePreferences(nextPreferences);
  renderTradeControls();
}

async function updatePreferences(updater) {
  const current = normalizeTradePreferences(state.preferences);
  const next = updater(current);
  await savePreferences(next);
  const changedFields = Object.keys(next).filter((key) => {
    const currentValue = current[key];
    const nextValue = next[key];
    return Array.isArray(nextValue)
      ? JSON.stringify(currentValue || []) !== JSON.stringify(nextValue)
      : currentValue !== nextValue;
  });
  const patch = tradePreferencePatchFromValue(next, changedFields);
  if (Object.keys(patch).length) {
    try {
      state.preferences = normalizeTradePreferences(
        await callBackground("trench:update-quick-trade-preferences", { patch })
      );
    } catch (error) {
      if (isStaleQuickTradePreferencesError(error)) {
        state.preferences = normalizeTradePreferences(
          await callBackground("trench:get-quick-trade-preferences")
        );
      } else {
        state.preferences = current;
        renderTradeControls();
        throw error;
      }
    }
    renderTradeControls();
  }
}

function isStaleQuickTradePreferencesError(error) {
  if (error?.status !== 400) {
    return false;
  }
  return /stale wallet selection update/i.test(String(error?.message || error || ""));
}

function renderPresetSelect() {
  const presets = Array.isArray(state.bootstrap.presets) ? state.bootstrap.presets : [];
  const activePresetId = String(state.preferences.presetId || presets[0]?.id || "").trim();
  presetSelect.innerHTML = "";
  if (!presets.length) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "No presets";
    presetSelect.appendChild(option);
    presetSelect.disabled = true;
    return;
  }
  presetSelect.disabled = false;
  presets.forEach((preset, index) => {
    const option = document.createElement("option");
    option.value = preset.id || "";
    option.textContent = preset.label || preset.id || `Preset ${index + 1}`;
    presetSelect.appendChild(option);
  });
  presetSelect.value = activePresetId;
}

function walletDropdownSummary(selection) {
  const wallets = enabledWallets();
  if (!wallets.length) {
    return { text: "No wallets", placeholder: true };
  }
  if (selection.selectionSource === "group" && selection.activeWalletGroupId) {
    const group = state.bootstrap.walletGroups.find(
      (entry) => String(entry?.id || "").trim() === selection.activeWalletGroupId
    );
    if (group) {
      return { text: group.label || group.id || "Group", placeholder: false };
    }
  }
  const selectedKeys = selectedWalletKeys(selection);
  if (!selectedKeys.length) {
    return { text: "Select wallets", placeholder: true };
  }
  if (selectedKeys.length === 1) {
    const walletIndex = wallets.findIndex((wallet) => wallet.key === selectedKeys[0]);
    if (walletIndex >= 0) {
      return { text: walletLabel(wallets[walletIndex], walletIndex), placeholder: false };
    }
  }
  return { text: `${selectedKeys.length} selected`, placeholder: false };
}

function buildDropdownItem({ label, title, active, disabled, onClick }) {
  const item = document.createElement("button");
  item.type = "button";
  item.className = `wallet-dropdown-item${active ? " active" : ""}`;
  item.setAttribute("role", "option");
  item.setAttribute("aria-selected", active ? "true" : "false");
  if (title) {
    item.title = title;
  }
  if (disabled) {
    item.disabled = true;
  }

  const mark = document.createElement("span");
  mark.className = "wallet-dropdown-item-mark";
  mark.setAttribute("aria-hidden", "true");
  mark.textContent = active ? "\u2713" : "";

  const text = document.createElement("span");
  text.className = "wallet-dropdown-item-label";
  text.textContent = label;

  item.appendChild(mark);
  item.appendChild(text);
  if (!disabled && typeof onClick === "function") {
    item.addEventListener("click", onClick);
  }
  return item;
}

function renderWalletDropdown(selection) {
  const wallets = enabledWallets();
  const groups = Array.isArray(state.bootstrap.walletGroups) ? state.bootstrap.walletGroups : [];
  const selectedKeys = new Set(selectedWalletKeys(selection));
  const summary = walletDropdownSummary(selection);

  walletDropdownLabel.textContent = summary.text;
  walletDropdownLabel.classList.toggle("placeholder", Boolean(summary.placeholder));
  walletDropdownButton.disabled = !wallets.length;
  if (!wallets.length) {
    closeWalletDropdown();
  }

  walletDropdownMenu.innerHTML = "";

  if (groups.length) {
    const groupSection = document.createElement("div");
    groupSection.className = "wallet-dropdown-section";
    const groupLabel = document.createElement("div");
    groupLabel.className = "wallet-dropdown-section-label";
    groupLabel.textContent = "Groups";
    groupSection.appendChild(groupLabel);
    groups.forEach((group, index) => {
      const groupId = String(group?.id || "").trim();
      if (!groupId) {
        return;
      }
      const active = selection.selectionSource === "group" && selection.activeWalletGroupId === groupId;
      groupSection.appendChild(
        buildDropdownItem({
          label: group.label || group.id || `Group ${index + 1}`,
          title: group.label || group.id,
          active,
          onClick: () => selectWalletGroup(groupId)
        })
      );
    });
    walletDropdownMenu.appendChild(groupSection);
  }

  const walletSection = document.createElement("div");
  walletSection.className = "wallet-dropdown-section";
  const walletLabelEl = document.createElement("div");
  walletLabelEl.className = "wallet-dropdown-section-label";
  walletLabelEl.textContent = groups.length ? "Wallets" : "Select wallets";
  walletSection.appendChild(walletLabelEl);

  if (!wallets.length) {
    walletSection.appendChild(
      buildDropdownItem({
        label: "No wallets",
        active: false,
        disabled: true
      })
    );
  } else {
    wallets.forEach((wallet, index) => {
      const key = String(wallet?.key || "").trim();
      if (!key) {
        return;
      }
      const active = selection.selectionSource !== "group" && selectedKeys.has(key);
      walletSection.appendChild(
        buildDropdownItem({
          label: walletLabel(wallet, index),
          title: wallet.label || wallet.key,
          active,
          onClick: () => toggleManualWallet(key)
        })
      );
    });
  }
  walletDropdownMenu.appendChild(walletSection);
}

async function selectWalletGroup(groupId) {
  await updatePreferences((preferences) =>
    mirrorWalletSelectionPreference(
      {
        ...preferences,
        selectionRevision: Math.max(0, Number(preferences.selectionRevision || 0) || 0) + 1
      },
      {
        selectionSource: "group",
        activeWalletGroupId: groupId,
        manualWalletKeys: normalizeWalletSelectionPreference(state.preferences).manualWalletKeys
      }
    )
  );
  closeWalletDropdown();
}

async function toggleManualWallet(key) {
  const selection = currentSelection();
  const baseKeys =
    selection.selectionSource === "group" ? [] : selectedWalletKeys(selection);
  const nextKeys = new Set(baseKeys);
  if (nextKeys.has(key)) {
    if (nextKeys.size <= 1) {
      return;
    }
    nextKeys.delete(key);
  } else {
    nextKeys.add(key);
  }
  await updatePreferences((preferences) =>
    mirrorWalletSelectionPreference(
      {
        ...preferences,
        selectionRevision: Math.max(0, Number(preferences.selectionRevision || 0) || 0) + 1
      },
      {
        selectionSource: "manual",
        activeWalletGroupId: selection.activeWalletGroupId,
        manualWalletKeys: normalizeWalletKeys([...nextKeys])
      }
    )
  );
}

function isWalletDropdownOpen() {
  return !walletDropdownMenu.hasAttribute("hidden");
}

function openWalletDropdown() {
  if (walletDropdownButton.disabled || isWalletDropdownOpen()) {
    return;
  }
  walletDropdownMenu.removeAttribute("hidden");
  walletDropdownButton.setAttribute("aria-expanded", "true");
  document.addEventListener("mousedown", handleWalletDropdownOutside, true);
  document.addEventListener("keydown", handleWalletDropdownKey, true);
}

function closeWalletDropdown() {
  if (!isWalletDropdownOpen()) {
    return;
  }
  walletDropdownMenu.setAttribute("hidden", "");
  walletDropdownButton.setAttribute("aria-expanded", "false");
  document.removeEventListener("mousedown", handleWalletDropdownOutside, true);
  document.removeEventListener("keydown", handleWalletDropdownKey, true);
}

function handleWalletDropdownOutside(event) {
  if (
    walletDropdownMenu.contains(event.target) ||
    walletDropdownButton.contains(event.target)
  ) {
    return;
  }
  closeWalletDropdown();
}

function handleWalletDropdownKey(event) {
  if (event.key === "Escape") {
    closeWalletDropdown();
    walletDropdownButton.focus();
  }
}

function setQuickBuyInputValue(input, value) {
  if (!input || document.activeElement === input) {
    return;
  }
  input.value = value;
}

function renderTradeControls() {
  const selection = currentSelection();
  setQuickBuyInputValue(
    quickBuyAmountInput,
    normalizeQuickBuyAmountInput(state.preferences.quickBuyAmount || "")
  );
  if (quickBuyAmount2Input) {
    setQuickBuyInputValue(
      quickBuyAmount2Input,
      normalizeQuickBuyAmountInput(state.preferences.quickBuyAmount2 || "")
    );
  }
  renderPresetSelect();
  renderWalletDropdown(selection);
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
  const normalizedQuickBuyAmount = normalizeQuickBuyAmountInput(quickBuyAmountInput.value);
  quickBuyAmountInput.value = normalizedQuickBuyAmount;
  await updatePreferences((preferences) => ({
    ...preferences,
    quickBuyAmount: normalizedQuickBuyAmount
  }));
});
quickBuyAmountInput.addEventListener("blur", renderTradeControls);

if (quickBuyAmount2Input) {
  quickBuyAmount2Input.addEventListener("input", async () => {
    const normalizedQuickBuyAmount2 = normalizeQuickBuyAmountInput(quickBuyAmount2Input.value);
    quickBuyAmount2Input.value = normalizedQuickBuyAmount2;
    await updatePreferences((preferences) => ({
      ...preferences,
      quickBuyAmount2: normalizedQuickBuyAmount2
    }));
  });
  quickBuyAmount2Input.addEventListener("blur", renderTradeControls);
}

presetSelect.addEventListener("change", async () => {
  await updatePreferences((preferences) => ({
    ...preferences,
    presetId: presetSelect.value
  }));
});

walletDropdownButton.addEventListener("click", () => {
  if (isWalletDropdownOpen()) {
    closeWalletDropdown();
  } else {
    openWalletDropdown();
  }
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
  authTokenSaveInProgress = true;
  try {
    await chrome.storage.local.set({ [HOST_AUTH_TOKEN_STORAGE_KEY]: token });
    await callBackground("trench:refresh-host-connection");
    await init();
  } catch (error) {
    setAuthStatus(error?.message || "Could not save auth token.", "error");
  } finally {
    authTokenSaveInProgress = false;
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
    await callBackground("trench:refresh-host-connection");
    const [health, bootstrap, preferencesResult] = await Promise.all([
      callBackground("trench:get-health"),
      callBackground("trench:get-bootstrap"),
      callBackground("trench:get-quick-trade-preferences").then(
        (preferences) => ({ ok: true, preferences }),
        (error) => ({ ok: false, error })
      )
    ]);
    state.isConnected = true;
    state.bootstrap = bootstrap;
    state.preferences = normalizeTradePreferences(
      preferencesResult.ok ? preferencesResult.preferences : state.preferences
    );
    authTokenInput.value = "";
    statusPill.textContent = `Host ${health.engineVersion}`;
    statusPill.dataset.state = "online";
    presetCount.textContent = String(bootstrap.presets.length);
    walletCount.textContent = String(bootstrap.wallets.length);
    groupCount.textContent = String(bootstrap.walletGroups.length);
    renderTradeControls();
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
    state.bootstrap = {
      presets: [],
      wallets: [],
      walletGroups: []
    };
    state.preferences = normalizeTradePreferences(state.preferences);
    state.isConnected = false;
    statusPill.textContent = state.hasStoredHost ? "Host unavailable" : "No host set";
    statusPill.dataset.state = "offline";
    presetCount.textContent = "0";
    walletCount.textContent = "0";
    groupCount.textContent = "0";
    renderTradeControls();
    showMainView();
  }
  connectionButton.textContent = "Disconnect";
}

chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName !== "local") {
    return;
  }
  if (changes[HOST_AUTH_TOKEN_STORAGE_KEY] || changes[BOOTSTRAP_REVISION_KEY]) {
    if (authTokenSaveInProgress && changes[HOST_AUTH_TOKEN_STORAGE_KEY]) {
      return;
    }
    void init();
    return;
  }
});

chrome.runtime.onMessage.addListener((message) => {
  if (message?.type !== "trench:quick-trade-preferences-updated") {
    return;
  }
  const incomingPreferences = normalizeTradePreferences(message.payload || {});
  const currentSelectionRevision = Math.max(0, Number(state.preferences?.selectionRevision || 0) || 0);
  const incomingSelectionRevision = Math.max(0, Number(incomingPreferences.selectionRevision || 0) || 0);
  state.preferences = normalizeTradePreferences({
    ...state.preferences,
    ...incomingPreferences,
    ...(incomingSelectionRevision < currentSelectionRevision
      ? {
          selectionSource: state.preferences.selectionSource,
          activeWalletGroupId: state.preferences.activeWalletGroupId,
          manualWalletKeys: [...(state.preferences.manualWalletKeys || [])],
          selectionRevision: currentSelectionRevision
        }
      : {})
  });
  renderTradeControls();
});

setInterval(() => {
  if (state.hasAuthToken) {
    void callBackground("trench:get-runtime-status").catch(() => {});
  }
}, 15000);

init();
