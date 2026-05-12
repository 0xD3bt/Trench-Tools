const CHANNEL_OUT = "trench-tools-panel";
const CHANNEL_IN = "trench-tools-content";
const EXPECTED_PARENT_ORIGIN = (() => {
  try {
    return new URLSearchParams(window.location.search).get("parentOrigin") || "";
  } catch {
    return "";
  }
})();
const PANEL_MODE = (() => {
  try {
    return new URLSearchParams(window.location.search).get("mode") || "persistent";
  } catch {
    return "persistent";
  }
})();
const IS_QUICK_MODE = PANEL_MODE === "quick";
const REDUCED_MEV_ICON_SRC = "../../assets/MEV-icon.png";
const SECURE_MEV_ICON_SRC = "../../assets/mevsecure-icon.png";
const NO_MEV_ICON_SRC = "../../assets/NOMEV-icon.png";
const SOL_ICON_SRC = "../../assets/sol-icon-white.png";
const PERCENT_ICON_SRC = "../../assets/percent-icon.png";
const FUEL_ICON_SRC = "../../assets/fuel-icon.png";
const SLIPPAGE_ICON_SRC = "../../assets/slippage-icon.png";
const TIP_ICON_SRC = "../../assets/tip-icon.png";
const AUTO_ICON_SRC = "../../assets/lighting-icon.png";

const renderSignatures = new Map();

function memoizedRender(key, signature, renderFn) {
  const prev = renderSignatures.get(key);
  if (prev === signature) {
    return;
  }
  renderSignatures.set(key, signature);
  renderFn();
}

const state = {
  bootstrap: null,
  walletStatus: null,
  tokenContext: null,
  preferences: {
    presetId: "",
    selectionSource: "group",
    activeWalletGroupId: "",
    manualWalletKeys: [],
    selectionRevision: 0,
    selectionTarget: {
      type: "wallet_group",
      walletKey: "",
      walletGroupId: "",
      walletKeys: []
    },
    selectionMode: "wallet_group",
    walletKey: "",
    walletGroupId: "",
    walletKeys: [],
    includeFees: null,
    buyAmountSol: "",
    customSellPercent: "",
    customSellSol: "",
    hideWalletGroupRow: false,
    hidePresetChipRow: false
  },
  preview: null,
  batchStatus: null,
  tokenDistributionPending: "",
  hostError: "",
  panelNotice: null,
  walletPopoverOpen: false,
  settingsMenuOpen: false
};

const elements = {
  dragHandle: document.getElementById("drag-handle"),
  versionText: document.getElementById("version-text"),
  walletButton: document.getElementById("wallet-button"),
  settingsButton: document.getElementById("settings-button"),
  settingsMenu: document.getElementById("settings-menu"),
  toggleFeesButton: document.getElementById("toggle-fees-button"),
  resyncCoinButton: document.getElementById("resync-coin-button"),
  resetCoinButton: document.getElementById("reset-coin-button"),
  openGlobalSettingsButton: document.getElementById("open-global-settings-button"),
  toggleWalletGroupsButton: document.getElementById("toggle-wallet-groups-button"),
  togglePresetRowButton: document.getElementById("toggle-preset-row-button"),
  walletGroupsFlyoutHost: document.querySelector(
    '.panel-settings-menu-flyout-host[data-flyout="wallet-groups"]'
  ),
  presetRowFlyoutHost: document.querySelector(
    '.panel-settings-menu-flyout-host[data-flyout="preset-row"]'
  ),
  walletModal: document.getElementById("wallet-modal"),
  walletModalCloseButtons: Array.from(
    document.querySelectorAll("[data-wallet-modal-close]")
  ),
  walletPickerList: document.getElementById("wallet-picker-list"),
  walletSelectAll: document.getElementById("wallet-select-all"),
  walletSelectBalance: document.getElementById("wallet-select-balance"),
  walletHoldersSection: document.getElementById("wallet-holders-section"),
  walletHoldersList: document.getElementById("wallet-holders-list"),
  walletHoldersSelectAll: document.getElementById("wallet-holders-select-all"),
  walletConsolidate: document.getElementById("wallet-consolidate"),
  walletSplit: document.getElementById("wallet-split"),
  engineStatusRow: document.getElementById("engine-status-row"),
  engineStatusTitle: document.getElementById("engine-status-title"),
  engineStatusMessage: document.getElementById("engine-status-message"),
  engineStatusRetry: document.getElementById("engine-status-retry"),
  walletGroupChips: document.getElementById("wallet-group-chips"),
  presetChipRow: document.getElementById("preset-chip-row"),
  executionSummary: document.getElementById("execution-summary"),
  quickBuyPresets: document.getElementById("quick-buy-presets"),
  quickSellShortcuts: document.getElementById("quick-sell-shortcuts"),
  buyOptionRow: document.getElementById("buy-option-row"),
  sellOptionRow: document.getElementById("sell-option-row"),
  customBuyInput: document.getElementById("custom-buy-input"),
  customBuyButton: document.getElementById("custom-buy-button"),
  customSellPercentInput: document.getElementById("custom-sell-percent-input"),
  customSellPercentButton: document.getElementById("custom-sell-percent-button"),
  customSellSolInput: document.getElementById("custom-sell-sol-input"),
  customSellSolButton: document.getElementById("custom-sell-sol-button"),
  balanceBuy: document.getElementById("balance-buy"),
  balanceSell: document.getElementById("balance-sell"),
  balanceHold: document.getElementById("balance-hold"),
  balancePnl: document.getElementById("balance-pnl"),
  balancePnlDetail: document.getElementById("balance-pnl-detail"),
  minimizeButton: document.getElementById("minimize-button")
};

if (IS_QUICK_MODE) {
  document.documentElement.classList.add("quick-mode");
  document.body.classList.add("quick-mode");
  document.querySelector(".panel-shell")?.classList.add("quick-mode-shell");
}

let panelResizeScheduled = false;
let lastPanelResizeKey = "";

function schedulePanelResize() {
  if (panelResizeScheduled) {
    return;
  }
  panelResizeScheduled = true;
  requestAnimationFrame(() => {
    panelResizeScheduled = false;
    const shell = document.querySelector(".panel-shell");
    if (!(shell instanceof HTMLElement)) {
      return;
    }
    // `.panel-shell` uses `height: max-content`, so its rect/scroll metrics
    // reflect the natural content height regardless of the iframe size. We
    // intentionally do NOT include `document.body.scrollHeight` /
    // `documentElement.scrollHeight` because those track the iframe height
    // (body has `height: 100%`) and would prevent the panel from ever
    // shrinking when rows are hidden.
    const rect = shell.getBoundingClientRect();
    const width = Math.ceil(Math.max(rect.width, shell.scrollWidth));
    const height = Math.ceil(Math.max(shell.scrollHeight, rect.height));
    const sizeKey = `${width}x${height}`;
    if (sizeKey === lastPanelResizeKey) {
      return;
    }
    lastPanelResizeKey = sizeKey;
    window.parent.postMessage(
      {
        channel: CHANNEL_OUT,
        type: "panel-resize",
        payload: { width, height, mode: PANEL_MODE }
      },
      EXPECTED_PARENT_ORIGIN || "*"
    );
  });
}

window.addEventListener("message", (event) => {
  if (!event.data || event.data.channel !== CHANNEL_IN) {
    return;
  }
  if (event.source !== window.parent) {
    return;
  }
  if (EXPECTED_PARENT_ORIGIN && event.origin !== EXPECTED_PARENT_ORIGIN) {
    return;
  }

  switch (event.data.type) {
    case "panel-state":
      applyState(event.data.payload);
      break;
    case "panel-preview":
      state.preview = event.data.payload;
      render();
      break;
    case "panel-batch-status":
      state.batchStatus = event.data.payload;
      render();
      break;
    case "panel-error":
      applyPanelNotice(event.data.payload);
      render();
      break;
    case "panel-flyout-dismiss-menu":
      // The host-page flyout overlay applied a chip selection. Close the
      // settings dropdown so the user gets the same "menu collapses after
      // pick" behaviour as our other dropdown items.
      if (state.settingsMenuOpen) {
        state.settingsMenuOpen = false;
        render();
      }
      break;
    default:
      break;
  }
});

elements.customBuyInput.addEventListener("input", syncLocalCustomInputs);
elements.customSellPercentInput.addEventListener("input", syncLocalCustomInputs);
elements.customSellSolInput.addEventListener("input", syncLocalCustomInputs);

attachDragScroll(elements.presetChipRow);
attachDragScroll(elements.walletGroupChips);

function attachDragScroll(slider) {
  if (!slider) return;

  const viewport = document.createElement("div");
  viewport.className = "slider-viewport";
  slider.parentNode.insertBefore(viewport, slider);
  viewport.appendChild(slider);

  const leftArrow = document.createElement("span");
  leftArrow.className = "slider-arrow slider-arrow-left";
  leftArrow.setAttribute("aria-hidden", "true");
  const rightArrow = document.createElement("span");
  rightArrow.className = "slider-arrow slider-arrow-right";
  rightArrow.setAttribute("aria-hidden", "true");
  viewport.appendChild(leftArrow);
  viewport.appendChild(rightArrow);

  const updateArrows = () => {
    const maxScroll = slider.scrollWidth - slider.clientWidth;
    const canLeft = slider.scrollLeft > 1;
    const canRight = slider.scrollLeft < maxScroll - 1;
    viewport.classList.toggle("can-scroll-left", canLeft);
    viewport.classList.toggle("can-scroll-right", canRight);
  };

  slider.addEventListener("scroll", updateArrows, { passive: true });
  window.addEventListener("resize", updateArrows);
  new MutationObserver(updateArrows).observe(slider, { childList: true, subtree: true });
  queueMicrotask(updateArrows);

  const DRAG_THRESHOLD = 4;
  let pointerId = null;
  let startX = 0;
  let startScrollLeft = 0;
  let dragging = false;
  let moved = false;
  let capturedPointerId = null;

  slider.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    pointerId = event.pointerId;
    startX = event.clientX;
    startScrollLeft = slider.scrollLeft;
    dragging = false;
    moved = false;
  });

  slider.addEventListener("pointermove", (event) => {
    if (pointerId === null || event.pointerId !== pointerId) return;
    const dx = event.clientX - startX;
    if (!dragging && Math.abs(dx) < DRAG_THRESHOLD) return;
    if (!dragging) {
      dragging = true;
      slider.classList.add("is-dragging");
      try {
        slider.setPointerCapture(pointerId);
        capturedPointerId = pointerId;
      } catch (err) {
        capturedPointerId = null;
      }
    }
    moved = true;
    slider.scrollLeft = startScrollLeft - dx;
  });

  const endDrag = (event) => {
    if (pointerId === null) return;
    if (event && event.pointerId !== pointerId) return;
    if (capturedPointerId !== null) {
      try {
        slider.releasePointerCapture(capturedPointerId);
      } catch (err) {
        // no-op
      }
    }
    pointerId = null;
    capturedPointerId = null;
    if (dragging) {
      requestAnimationFrame(() => slider.classList.remove("is-dragging"));
    }
    dragging = false;
  };

  slider.addEventListener("pointerup", endDrag);
  slider.addEventListener("pointercancel", endDrag);

  slider.addEventListener(
    "click",
    (event) => {
      if (moved) {
        moved = false;
        event.preventDefault();
        event.stopPropagation();
      }
    },
    true
  );

  slider.addEventListener(
    "wheel",
    (event) => {
      if (event.deltaY === 0) return;
      if (slider.scrollWidth <= slider.clientWidth) return;
      event.preventDefault();
      slider.scrollLeft += event.deltaY;
    },
    { passive: false }
  );
}
elements.walletButton.addEventListener("click", () => {
  state.walletPopoverOpen = !state.walletPopoverOpen;
  renderWalletPopover();
});
elements.walletModalCloseButtons.forEach((button) => {
  button.addEventListener("click", () => {
    state.walletPopoverOpen = false;
    renderWalletPopover();
  });
});
elements.walletModal.addEventListener("click", (event) => {
  if (event.target === elements.walletModal) {
    state.walletPopoverOpen = false;
    renderWalletPopover();
  }
});
bindImmediateButtonAction(elements.walletSelectAll, () => {
  const nonHolders = getWalletSourceWallets().filter((wallet) => getWalletTokenBalanceNumber(wallet) <= 0);
  if (!nonHolders.length) {
    return;
  }
  const nonHolderKeys = nonHolders.map((wallet) => wallet.key);
  const selected = new Set(getSelectedWalletKeys());
  const allSelected = nonHolderKeys.every((key) => selected.has(key));
  if (allSelected) {
    applyWalletSelection(getSelectedWalletKeys().filter((key) => !nonHolderKeys.includes(key)));
  } else {
    applyWalletSelection(Array.from(new Set([...getSelectedWalletKeys(), ...nonHolderKeys])));
  }
});
bindImmediateButtonAction(elements.walletSelectBalance, () => {
  const nonHolders = getWalletSourceWallets().filter((wallet) => getWalletTokenBalanceNumber(wallet) <= 0);
  const keysWithBalance = nonHolders
    .filter((wallet) => getWalletBalanceNumber(wallet) > 0)
    .map((wallet) => wallet.key);
  if (!keysWithBalance.length) {
    return;
  }
  applyWalletSelection(Array.from(new Set([...getSelectedWalletKeys(), ...keysWithBalance])));
});
bindImmediateButtonAction(elements.walletHoldersSelectAll, () => {
  const holders = getWalletSourceWallets().filter((wallet) => getWalletTokenBalanceNumber(wallet) > 0);
  if (!holders.length) {
    return;
  }
  const holderKeys = holders.map((wallet) => wallet.key);
  const selected = new Set(getSelectedWalletKeys());
  const allHoldersSelected = holderKeys.every((key) => selected.has(key));
  if (allHoldersSelected) {
    applyWalletSelection(getSelectedWalletKeys().filter((key) => !holderKeys.includes(key)));
  } else {
    applyWalletSelection(Array.from(new Set([...getSelectedWalletKeys(), ...holderKeys])));
  }
});
bindImmediateButtonAction(elements.walletConsolidate, () => {
  if (state.tokenDistributionPending) {
    return;
  }
  emit("request-token-consolidate", collectPreferences());
});
bindImmediateButtonAction(elements.walletSplit, () => {
  if (state.tokenDistributionPending) {
    return;
  }
  emit("request-token-split", {
    ...collectPreferences(),
    sourceWalletKeys: getSelectedHolderWalletKeys()
  });
});
elements.settingsButton.addEventListener("click", (event) => {
  event.stopPropagation();
  state.settingsMenuOpen = !state.settingsMenuOpen;
  renderSettingsMenu();
});
elements.toggleFeesButton?.addEventListener("click", () => {
  const defaultIncludeFees = resolveDefaultIncludeFeesPreference();
  const nextIncludeFees = !resolveIncludeFeesPreference();
  if (nextIncludeFees === defaultIncludeFees) {
    delete state.preferences.includeFees;
  } else {
    state.preferences.includeFees = nextIncludeFees;
  }
  state.settingsMenuOpen = false;
  persistPreferences();
});
elements.resyncCoinButton?.addEventListener("click", () => {
  state.settingsMenuOpen = false;
  renderSettingsMenu();
  emit("resync-pnl-history");
});
elements.resetCoinButton?.addEventListener("click", () => {
  state.settingsMenuOpen = false;
  renderSettingsMenu();
  if (!window.confirm("Reset PnL history for this coin and start fresh from here? This only works when the position is fully closed.")) {
    return;
  }
  emit("reset-pnl-history");
});
elements.openGlobalSettingsButton?.addEventListener("click", () => {
  state.settingsMenuOpen = false;
  renderSettingsMenu();
  emit("open-options", { section: "global" });
});
elements.toggleWalletGroupsButton?.addEventListener("click", () => {
  state.preferences.hideWalletGroupRow = !state.preferences.hideWalletGroupRow;
  if (!state.preferences.hideWalletGroupRow) {
    requestFlyoutCancel();
  }
  persistPreferences();
});
elements.togglePresetRowButton?.addEventListener("click", () => {
  state.preferences.hidePresetChipRow = !state.preferences.hidePresetChipRow;
  if (!state.preferences.hidePresetChipRow) {
    requestFlyoutCancel();
  }
  persistPreferences();
});
if (elements.walletGroupsFlyoutHost) {
  elements.walletGroupsFlyoutHost.addEventListener("mouseenter", () => {
    requestFlyoutShow(elements.walletGroupsFlyoutHost, "wallet-groups");
  });
  elements.walletGroupsFlyoutHost.addEventListener("mouseleave", () => {
    requestFlyoutHostLeave("wallet-groups");
  });
}
if (elements.presetRowFlyoutHost) {
  elements.presetRowFlyoutHost.addEventListener("mouseenter", () => {
    requestFlyoutShow(elements.presetRowFlyoutHost, "preset-row");
  });
  elements.presetRowFlyoutHost.addEventListener("mouseleave", () => {
    requestFlyoutHostLeave("preset-row");
  });
}
document.addEventListener("pointerdown", (event) => {
  if (!state.settingsMenuOpen) {
    return;
  }
  if (event.target instanceof Node && elements.settingsMenu?.contains(event.target)) {
    return;
  }
  if (event.target instanceof Node && elements.settingsButton?.contains(event.target)) {
    return;
  }
  state.settingsMenuOpen = false;
  renderSettingsMenu();
});
elements.dragHandle.addEventListener("pointerdown", (event) => {
  if (IS_QUICK_MODE) {
    return;
  }
  if (event.target.closest("button, input, select")) {
    return;
  }
  event.preventDefault();
  window.parent.postMessage(
    {
      channel: CHANNEL_OUT,
      type: "start-drag",
      payload: { clientX: event.clientX, clientY: event.clientY }
    },
    EXPECTED_PARENT_ORIGIN || "*"
  );
});
bindImmediateButtonAction(elements.customBuyButton, () => {
  const value = elements.customBuyInput.value.trim();
  if (!isPositiveNumericInput(value)) {
    flagEmptyCustomInput(elements.customBuyInput, "Enter a SOL amount before buying.");
    return;
  }
  emit("request-buy", {
    ...collectPreferences(),
    buyAmountSol: value
  });
});
bindImmediateButtonAction(elements.customSellPercentButton, () => {
  const value = elements.customSellPercentInput.value.trim();
  if (!isPositiveNumericInput(value)) {
    flagEmptyCustomInput(elements.customSellPercentInput, "Enter a sell percentage before selling.");
    return;
  }
  emit("request-sell", {
    ...collectPreferences(),
    sellPercent: value
  });
});
bindImmediateButtonAction(elements.customSellSolButton, () => {
  const value = elements.customSellSolInput.value.trim();
  if (!isPositiveNumericInput(value)) {
    flagEmptyCustomInput(elements.customSellSolInput, "Enter a SOL amount before selling.");
    return;
  }
  emit("request-sell", {
    ...collectPreferences(),
    sellOutputSol: value
  });
});
elements.engineStatusRetry?.addEventListener("click", () => emit("refresh-panel"));
elements.minimizeButton.addEventListener("click", () => emit("minimize-panel"));

if (IS_QUICK_MODE) {
  elements.minimizeButton.style.display = "none";
  elements.dragHandle.style.cursor = "default";
}

schedulePanelResize();
if (typeof ResizeObserver === "function") {
  const panelResizeObserver = new ResizeObserver(() => {
    schedulePanelResize();
  });
  const shell = document.querySelector(".panel-shell");
  if (shell instanceof HTMLElement) {
    panelResizeObserver.observe(shell);
  }
  panelResizeObserver.observe(document.body);
}
window.addEventListener("resize", () => {
  schedulePanelResize();
});

window.parent.postMessage(
  { channel: CHANNEL_OUT, type: "panel-ready" },
  EXPECTED_PARENT_ORIGIN || "*"
);

function emit(type, payload = collectPreferences()) {
  window.parent.postMessage(
    {
      channel: CHANNEL_OUT,
      type,
      payload
    },
    EXPECTED_PARENT_ORIGIN || "*"
  );
}

function isPositiveNumericInput(value) {
  if (typeof value !== "string") return false;
  const trimmed = value.trim();
  if (!trimmed) return false;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) && parsed > 0;
}

let inputValidationNoticeTimer = null;

function flagEmptyCustomInput(input, message) {
  if (input instanceof HTMLInputElement) {
    input.focus();
    input.classList.add("input-validation-error");
    window.setTimeout(() => {
      input.classList.remove("input-validation-error");
    }, 1200);
  }
  applyPanelNotice({
    source: "input-validation",
    title: "Input required",
    message,
    kind: "warning"
  });
  render();
  if (inputValidationNoticeTimer) {
    clearTimeout(inputValidationNoticeTimer);
  }
  inputValidationNoticeTimer = window.setTimeout(() => {
    applyPanelNotice({ source: "input-validation", message: "" });
    render();
    inputValidationNoticeTimer = null;
  }, 2800);
}

function bindImmediateButtonAction(button, action) {
  if (!button) {
    return;
  }
  // The panel re-renders frequently while wallet/batch status updates stream in.
  // Firing sell actions on pointerdown avoids losing the interaction when a
  // DOM refresh lands between pointerdown and the later click event.
  button.addEventListener("pointerdown", (event) => {
    if (button.disabled) {
      return;
    }
    if (typeof event.button === "number" && event.button !== 0) {
      return;
    }
    action();
  });
  button.addEventListener("click", (event) => {
    if (button.disabled) {
      return;
    }
    if (event.detail !== 0) {
      return;
    }
    action();
  });
}

function collectPreferences() {
  syncLocalCustomInputs();
  const activePreset = getActivePreset();
  mirrorWalletSelectionPreferenceOntoPreferences(state.preferences);
  const selectionTarget = selectionTargetFromWalletSelectionPreference(state.preferences);
  return {
    presetId: state.preferences.presetId || activePreset?.id || "",
    selectionSource: state.preferences.selectionSource || "group",
    activeWalletGroupId: state.preferences.activeWalletGroupId || "",
    manualWalletKeys: [...(state.preferences.manualWalletKeys || [])],
    selectionRevision: Math.max(0, Number(state.preferences.selectionRevision || 0) || 0),
    selectionTarget,
    selectionMode: selectionTarget.type,
    walletKey: selectionTarget.walletKey || "",
    walletGroupId: selectionTarget.walletGroupId || "",
    walletKeys: [...(selectionTarget.walletKeys || [])],
    includeFees: typeof state.preferences.includeFees === "boolean"
      ? state.preferences.includeFees
      : undefined,
    customSellPercent: elements.customSellPercentInput.value.trim(),
    customSellSol: elements.customSellSolInput.value.trim()
  };
}

function applyState(payload) {
  state.bootstrap = payload.bootstrap || null;
  state.walletStatus = Object.prototype.hasOwnProperty.call(payload, "walletStatus")
    ? (payload.walletStatus || null)
    : state.walletStatus;
  state.tokenContext = payload.tokenContext || null;
  state.tokenDistributionPending = String(payload.tokenDistributionPending || "");
  const localSelectionState = {
    selectionSource: state.preferences.selectionSource || "group",
    activeWalletGroupId: state.preferences.activeWalletGroupId || "",
    manualWalletKeys: [...(state.preferences.manualWalletKeys || [])],
    selectionRevision: Math.max(0, Number(state.preferences.selectionRevision || 0) || 0),
    selectionTarget: normalizeSelectionTarget(state.preferences.selectionTarget || state.preferences),
    selectionMode: state.preferences.selectionMode || "wallet_group",
    walletKey: state.preferences.walletKey || "",
    walletGroupId: state.preferences.walletGroupId || "",
    walletKeys: [...(state.preferences.walletKeys || [])]
  };
  const active = document.activeElement;
  const focusedInputOverrides = {};
  if (active === elements.customBuyInput) {
    focusedInputOverrides.buyAmountSol = elements.customBuyInput.value.trim();
  }
  if (active === elements.customSellPercentInput) {
    focusedInputOverrides.customSellPercent = elements.customSellPercentInput.value.trim();
  }
  if (active === elements.customSellSolInput) {
    focusedInputOverrides.customSellSol = elements.customSellSolInput.value.trim();
  }
  const incomingPreferences = payload.preferences || {};
  const incomingSelectionRevision = Math.max(0, Number(incomingPreferences.selectionRevision || 0) || 0);
  state.preferences = {
    ...state.preferences,
    ...incomingPreferences,
    ...focusedInputOverrides
  };
  clearIncludeFeesOverrideWhenMatchingDefault();
  if (incomingSelectionRevision >= localSelectionState.selectionRevision) {
    mirrorWalletSelectionPreferenceOntoPreferences(state.preferences);
    state.preferences.selectionRevision = incomingSelectionRevision;
  } else {
    Object.assign(state.preferences, localSelectionState);
  }
  state.preview = Object.prototype.hasOwnProperty.call(payload, "preview")
    ? (payload.preview || null)
    : state.preview;
  state.batchStatus = Object.prototype.hasOwnProperty.call(payload, "batchStatus")
    ? (payload.batchStatus || null)
    : state.batchStatus;
  state.hostError = payload.hostError || "";
  if (state.hostError) {
    state.panelNotice = {
      title: "Execution engine unavailable",
      message: state.hostError,
      kind: "error",
      source: "host"
    };
  } else if (Object.prototype.hasOwnProperty.call(payload, "runtimeDiagnosticNotice")) {
    state.panelNotice = payload.runtimeDiagnosticNotice?.source === "runtime-diagnostic"
      ? {
          title: String(payload.runtimeDiagnosticNotice.title || "Runtime diagnostic"),
          message: String(payload.runtimeDiagnosticNotice.message || ""),
          kind: String(payload.runtimeDiagnosticNotice.kind || "info"),
          source: "runtime-diagnostic"
        }
      : null;
  } else if (state.panelNotice?.source !== "runtime-diagnostic") {
    state.panelNotice = null;
  }
  render();
}

function applyPanelNotice(payload) {
  const message = String(payload?.message || "").trim();
  const source = String(payload?.source || "notice").trim() || "notice";
  if (!message) {
    if (!source || state.panelNotice?.source === source) {
      state.panelNotice = null;
    }
    return;
  }
  const title = String(payload?.title || "").trim()
    || (source === "host" ? "Execution engine unavailable" : "Panel notice");
  state.panelNotice = {
    title,
    message,
    kind: String(payload?.kind || "error").trim() || "error",
    source
  };
  if (source === "host") {
    state.hostError = message;
  }
}

function syncLocalCustomInputs() {
  state.preferences.buyAmountSol = elements.customBuyInput.value.trim();
  state.preferences.customSellPercent = elements.customSellPercentInput.value.trim();
  state.preferences.customSellSol = elements.customSellSolInput.value.trim();
}

function resolveDefaultIncludeFeesPreference() {
  return Boolean(state.walletStatus?.includeFees);
}

function clearIncludeFeesOverrideWhenMatchingDefault() {
  if (typeof state.preferences.includeFees !== "boolean") {
    return;
  }
  if (state.preferences.includeFees === resolveDefaultIncludeFeesPreference()) {
    delete state.preferences.includeFees;
  }
}

function render() {
  syncPanelDebugState();
  renderHeader();
  renderEngineStatus();
  renderSelectors();
  renderWalletGroupChips();
  renderPresetChips();
  renderHiddenRowVisibility();
  renderExecutionSummary();
  renderShortcutRows();
  renderOptionRows();
  renderBalanceSection();
  renderWalletPopover();
  renderSettingsMenu();
  schedulePanelResize();
}

function renderHiddenRowVisibility() {
  const presetHidden = Boolean(state.preferences.hidePresetChipRow);
  const walletHidden = Boolean(state.preferences.hideWalletGroupRow);
  if (elements.presetChipRow) {
    elements.presetChipRow.classList.toggle("is-hidden", presetHidden);
  }
  if (elements.walletGroupChips) {
    elements.walletGroupChips.classList.toggle("is-hidden", walletHidden);
  }
  const presetDivider = document.querySelector('[data-section-divider="preset-chip-row"]');
  const walletDivider = document.querySelector('[data-section-divider="wallet-group-chips"]');
  if (presetDivider) {
    presetDivider.classList.toggle("is-hidden", presetHidden);
  }
  if (walletDivider) {
    walletDivider.classList.toggle("is-hidden", walletHidden);
  }
}

function syncPanelDebugState() {
  const activeMint = String(state.tokenContext?.mint || "").trim();
  const activeSurface = String(state.tokenContext?.surface || "").trim();
  const activeUrl = String(state.tokenContext?.url || state.tokenContext?.sourceUrl || "").trim();
  document.documentElement.dataset.trenchActiveMint = activeMint;
  document.documentElement.dataset.trenchActiveSurface = activeSurface;
  document.documentElement.dataset.trenchActiveUrl = activeUrl;
  window.__trenchToolsPanelDebug = {
    tokenContext: state.tokenContext ? { ...state.tokenContext } : null,
    preferences: {
      selectionSource: state.preferences.selectionSource || "",
      activeWalletGroupId: state.preferences.activeWalletGroupId || "",
      manualWalletKeys: [...(state.preferences.manualWalletKeys || [])],
      selectionMode: state.preferences.selectionMode || "",
      walletKey: state.preferences.walletKey || "",
      walletGroupId: state.preferences.walletGroupId || "",
      walletKeys: [...(state.preferences.walletKeys || [])]
    },
    walletStatus: state.walletStatus
      ? {
          selectionMode: state.walletStatus.selectionMode || "",
          walletKeys: [...(state.walletStatus.walletKeys || [])],
          selectedWalletKey: state.walletStatus.selectedWalletKey || "",
          mint: state.walletStatus.mint || ""
        }
      : null
  };
}

function getExtensionManifestVersion() {
  try {
    if (typeof chrome !== "undefined" && chrome?.runtime?.getManifest) {
      return chrome.runtime.getManifest()?.version || "";
    }
  } catch {
    // chrome.runtime may not be reachable in all surfaces; fall through.
  }
  return "";
}

function renderHeader() {
  if (!elements.versionText) {
    return;
  }
  if (state.hostError) {
    elements.versionText.textContent = "offline";
    elements.versionText.classList.add("is-waiting");
    return;
  }
  // Display the extension's manifest version (the user-facing product
  // version bumped in `extension/trench-tools/manifest.json`), not the
  // backend's `CURRENT_ENGINE_STATE_VERSION` — that one is an internal
  // state-schema marker used for preset migrations and shouldn't surface
  // in the header. Fall back to the bootstrap value only if the manifest
  // is unavailable for some reason.
  const version = getExtensionManifestVersion() || state.bootstrap?.version || "";
  if (version) {
    elements.versionText.textContent = `v${version}`;
    elements.versionText.classList.remove("is-waiting");
  } else {
    elements.versionText.textContent = "…";
    elements.versionText.classList.add("is-waiting");
  }
}

function renderEngineStatus() {
  if (!elements.engineStatusRow || !elements.engineStatusTitle || !elements.engineStatusMessage) {
    return;
  }
  const notice = state.hostError
    ? {
        title: "Execution engine unavailable",
        message: state.hostError,
        source: "host"
      }
    : state.panelNotice;
  const message = String(notice?.message || "").trim();
  const source = String(notice?.source || "").trim();
  elements.engineStatusRow.classList.toggle("hidden", !message);
  elements.engineStatusTitle.textContent = String(notice?.title || "Panel notice").trim() || "Panel notice";
  elements.engineStatusMessage.textContent = message || "Start the execution engine, then try again.";
  elements.engineStatusRetry?.classList.toggle("hidden", source !== "host");
}

function resolveIncludeFeesPreference() {
  if (typeof state.preferences.includeFees === "boolean") {
    return state.preferences.includeFees;
  }
  return resolveDefaultIncludeFeesPreference();
}

function renderSettingsMenu() {
  if (!elements.settingsMenu) {
    return;
  }
  const includeFees = resolveIncludeFeesPreference();
  const hasMint = Boolean(String(state.walletStatus?.mint || state.tokenContext?.mint || "").trim());
  const rawBalance = Number(state.walletStatus?.tokenBalanceRaw);
  const uiBalance = Number(
    state.walletStatus?.holdingAmount ??
    state.walletStatus?.mintBalanceUi ??
    state.walletStatus?.tokenBalance
  );
  const hasOpenBalance = Number.isFinite(rawBalance)
    ? rawBalance > 0
    : Number.isFinite(uiBalance) && uiBalance > 0;
  elements.settingsMenu.classList.toggle("hidden", !state.settingsMenuOpen);
  if (!state.settingsMenuOpen) {
    closeAllFlyouts();
  }
  if (elements.toggleFeesButton) {
    elements.toggleFeesButton.textContent = includeFees ? "Show gross PnL" : "Show net PnL";
  }
  if (elements.resyncCoinButton) {
    elements.resyncCoinButton.disabled = !hasMint;
  }
  if (elements.resetCoinButton) {
    elements.resetCoinButton.disabled = !hasMint || hasOpenBalance;
    elements.resetCoinButton.title =
      !hasMint || hasOpenBalance
        ? "Reset is only available when the current position is fully closed."
        : "";
  }
  const walletGroupRowHidden = Boolean(state.preferences.hideWalletGroupRow);
  const presetChipRowHidden = Boolean(state.preferences.hidePresetChipRow);
  if (elements.toggleWalletGroupsButton) {
    const label = elements.toggleWalletGroupsButton.querySelector(
      ".panel-settings-menu-item-label"
    );
    if (label) {
      label.textContent = walletGroupRowHidden ? "Show wallet groups" : "Hide wallet groups";
    }
  }
  if (elements.togglePresetRowButton) {
    const label = elements.togglePresetRowButton.querySelector(
      ".panel-settings-menu-item-label"
    );
    if (label) {
      label.textContent = presetChipRowHidden ? "Show preset row" : "Hide preset row";
    }
  }
  if (elements.walletGroupsFlyoutHost) {
    elements.walletGroupsFlyoutHost.classList.toggle("is-hideable", walletGroupRowHidden);
  }
  if (elements.presetRowFlyoutHost) {
    elements.presetRowFlyoutHost.classList.toggle("is-hideable", presetChipRowHidden);
  }
  // The flyout itself is rendered as a host-page overlay (see content/index.js
  // panel-flyout-* handlers) so it can extend beyond the iframe boundary
  // without being clipped. We only manage the host's "is-open" state here for
  // the in-iframe arrow/highlight styling.
}

function flyoutKindForHost(host) {
  if (!host) return null;
  const kind = host.dataset?.flyout;
  if (kind === "wallet-groups" || kind === "preset-row") {
    return kind;
  }
  return null;
}

function postFlyoutMessage(type, payload) {
  window.parent.postMessage(
    {
      channel: CHANNEL_OUT,
      type,
      payload: payload || {}
    },
    EXPECTED_PARENT_ORIGIN || "*"
  );
}

function requestFlyoutShow(host, kind) {
  if (!host || !host.classList.contains("is-hideable")) {
    return;
  }
  const rect = host.getBoundingClientRect();
  host.classList.add("is-open");
  postFlyoutMessage("panel-flyout-show", {
    kind,
    anchor: {
      left: rect.left,
      top: rect.top,
      width: rect.width,
      height: rect.height
    }
  });
}

function requestFlyoutHostLeave(kind) {
  // Notify the parent that the cursor left the in-iframe host. The parent
  // overlay manages its own debounce and will keep the flyout open while the
  // cursor is over the overlay itself.
  const host = kind === "wallet-groups"
    ? elements.walletGroupsFlyoutHost
    : kind === "preset-row"
      ? elements.presetRowFlyoutHost
      : null;
  if (host) {
    host.classList.remove("is-open");
  }
  postFlyoutMessage("panel-flyout-host-leave", { kind });
}

function requestFlyoutCancel() {
  if (elements.walletGroupsFlyoutHost) {
    elements.walletGroupsFlyoutHost.classList.remove("is-open");
  }
  if (elements.presetRowFlyoutHost) {
    elements.presetRowFlyoutHost.classList.remove("is-open");
  }
  postFlyoutMessage("panel-flyout-cancel", {});
}

function closeAllFlyouts() {
  requestFlyoutCancel();
}

function renderShortcutRows() {
  const preset = getActivePreset();
  const buyAmounts = getPresetBuyAmounts(preset);
  const sellAmounts = getPresetSellAmounts(preset);

  memoizedRender("quickBuyShortcuts", JSON.stringify(buyAmounts), () => {
    elements.quickBuyPresets.innerHTML = "";
    for (const amount of buyAmounts) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "shortcut-button buy-shortcut";
      button.disabled = !amount;
      button.innerHTML = `
        <span class="shortcut-value-with-icon">
          <img class="unit-icon" src="${SOL_ICON_SRC}" alt="" aria-hidden="true" />
          <span>${amount || "--"}</span>
        </span>
      `;
      if (amount) {
        bindImmediateButtonAction(button, () =>
          emit("request-buy", {
            ...collectPreferences(),
            buyAmountSol: amount
          })
        );
      }
      elements.quickBuyPresets.appendChild(button);
    }
  });

  memoizedRender("quickSellShortcuts", JSON.stringify(sellAmounts), () => {
    elements.quickSellShortcuts.innerHTML = "";
    for (const value of sellAmounts) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "shortcut-button sell-shortcut";
      button.disabled = !value;
      button.innerHTML = value
        ? `
        <span class="shortcut-value-with-icon">
          <img class="unit-icon" src="${PERCENT_ICON_SRC}" alt="" aria-hidden="true" />
          <span>${value}</span>
        </span>
      `
        : `<span class="shortcut-value-with-icon"><img class="unit-icon" src="${PERCENT_ICON_SRC}" alt="" aria-hidden="true" /><span>--</span></span>`;
      if (value) {
        bindImmediateButtonAction(button, () =>
          emit("request-sell", {
            ...collectPreferences(),
            sellPercent: value
          })
        );
      }
      elements.quickSellShortcuts.appendChild(button);
    }
  });
}

function renderSelectors() {
  const activePreset = getActivePreset();
  state.preferences.presetId = activePreset?.id || "";
  normalizeWalletSelection();
  const active = document.activeElement;
  if (active !== elements.customBuyInput) {
    const next = state.preferences.buyAmountSol || "";
    if (elements.customBuyInput.value !== next) {
      elements.customBuyInput.value = next;
    }
  }
  if (active !== elements.customSellPercentInput) {
    const next = state.preferences.customSellPercent || "";
    if (elements.customSellPercentInput.value !== next) {
      elements.customSellPercentInput.value = next;
    }
  }
  if (active !== elements.customSellSolInput) {
    const next = state.preferences.customSellSol || "";
    if (elements.customSellSolInput.value !== next) {
      elements.customSellSolInput.value = next;
    }
  }
}

function renderWalletPopover() {
  elements.walletModal.classList.toggle("hidden", !state.walletPopoverOpen);
  renderWalletPicker();
}

function renderOptionRows() {
  const preset = getActivePreset();
  const buyMevMode = getBuyMevMode(preset);
  const sellMevMode = getSellMevMode(preset);
  const buyMevLabel = buyMevMode ? formatLabel(buyMevMode) : "Preset";
  const sellMevLabel = sellMevMode ? formatLabel(sellMevMode) : "Preset";
  const buySlippageLabel = getBuySlippagePercent(preset) ? `${getBuySlippagePercent(preset)}%` : "Preset";
  const sellSlippageLabel = getSellSlippagePercent(preset)
    ? `${getSellSlippagePercent(preset)}%`
    : "Preset";
  const buyFeeLabel = preset?.buyFeeSol ? formatValueWithUnitIcon(preset.buyFeeSol) : "Preset";
  const buyTipLabel = preset?.buyTipSol ? formatValueWithUnitIcon(preset.buyTipSol) : "Preset";
  const sellFeeLabel = preset?.sellFeeSol ? formatValueWithUnitIcon(preset.sellFeeSol) : "Preset";
  const sellTipLabel = preset?.sellTipSol ? formatValueWithUnitIcon(preset.sellTipSol) : "Preset";
  const buyAutoFeeLabel = preset?.buyAutoTipEnabled ? "On" : "Off";
  const sellAutoFeeLabel = preset?.sellAutoTipEnabled ? "On" : "Off";
  const buyMevIconSrc = getMevIconSrc(buyMevMode);
  const sellMevIconSrc = getMevIconSrc(sellMevMode);

  const buyItems = [
    {
      imageSrc: AUTO_ICON_SRC,
      imageAlt: "Auto fee",
      label: "Auto fee",
      value: buyAutoFeeLabel
    },
    { imageSrc: FUEL_ICON_SRC, imageAlt: "Gas fee", label: "Gas fee", value: buyFeeLabel, valueIsHtml: true },
    { imageSrc: TIP_ICON_SRC, imageAlt: "Tip", label: "Tip", value: buyTipLabel, valueIsHtml: true },
    { imageSrc: SLIPPAGE_ICON_SRC, imageAlt: "Slippage", label: "Slippage", value: buySlippageLabel },
    { imageSrc: buyMevIconSrc, imageAlt: "MEV mode", label: "MEV mode", value: buyMevLabel }
  ];
  const sellItems = [
    {
      imageSrc: AUTO_ICON_SRC,
      imageAlt: "Auto fee",
      label: "Auto fee",
      value: sellAutoFeeLabel
    },
    { imageSrc: FUEL_ICON_SRC, imageAlt: "Gas fee", label: "Gas fee", value: sellFeeLabel, valueIsHtml: true },
    { imageSrc: TIP_ICON_SRC, imageAlt: "Tip", label: "Tip", value: sellTipLabel, valueIsHtml: true },
    { imageSrc: SLIPPAGE_ICON_SRC, imageAlt: "Slippage", label: "Slippage", value: sellSlippageLabel },
    { imageSrc: sellMevIconSrc, imageAlt: "MEV mode", label: "MEV mode", value: sellMevLabel }
  ];

  memoizedRender("buyOptionRow", JSON.stringify(buyItems), () => {
    renderOptionRow(elements.buyOptionRow, buyItems);
  });
  memoizedRender("sellOptionRow", JSON.stringify(sellItems), () => {
    renderOptionRow(elements.sellOptionRow, sellItems);
  });
}

function renderOptionRow(container, items) {
  container.innerHTML = "";
  items.forEach((item, index) => {
    const entry = document.createElement("span");
    entry.className = `option-inline${item.iconOnly ? " icon-only" : ""}`;
    const stateSuffix = item.iconOnly ? (item.isEnabled ? " enabled" : " disabled") : "";
    entry.setAttribute("data-tooltip", `${item.label}${stateSuffix}`);
    entry.setAttribute("data-tooltip-position", "top");
    entry.setAttribute("aria-label", `${item.label}${stateSuffix}`);
    const iconMarkup = item.imageSrc
      ? `<img class="option-inline-icon${item.iconOnly ? ` ${item.isEnabled ? "is-enabled" : "is-disabled"}` : ""}" src="${item.imageSrc}" alt="${item.imageAlt || ""}" />`
      : `<span class="option-inline-icon-text">${item.icon}</span>`;
    const valueMarkup = item.valueIsHtml ? item.value : escapeHtml(item.value);
    entry.innerHTML = `
      ${iconMarkup}
      ${item.iconOnly ? "" : `<span class="option-inline-value">${valueMarkup}</span>`}
    `;
    container.appendChild(entry);

    if (index < items.length - 1) {
      const separator = document.createElement("span");
      separator.className = "option-separator";
      separator.textContent = "|";
      container.appendChild(separator);
    }
  });
}

function renderBalanceSection() {
  const walletStatus = state.walletStatus || {};
  const includeFees = resolveIncludeFeesPreference();
  const pnlRequiresQuote = walletStatus.pnlRequiresQuote === true;
  const buyValue = normalizeMetricValue(walletStatus.trackedBoughtSol);
  const sellValue = normalizeMetricValue(walletStatus.trackedSoldSol);
  const holdValue = normalizeMetricValue(walletStatus.holdingValueSol);
  const pnlMetric = includeFees ? walletStatus.pnlNet : walletStatus.pnlGross;
  const pnlValue = formatPnlMetric(pnlMetric);
  const pnlPercent = formatPercentMetric(
    includeFees ? walletStatus.pnlPercentNet : walletStatus.pnlPercentGross
  );

  elements.balanceBuy.textContent = buyValue;
  elements.balanceSell.textContent = sellValue;
  elements.balanceHold.textContent = holdValue;
  elements.balancePnl.textContent = pnlValue;
  if (elements.balancePnlDetail) {
    const quoteError = String(walletStatus.holdingQuoteError || "").trim();
    const shortQuoteError =
      quoteError.length > 72 ? `${quoteError.slice(0, 69)}...` : quoteError;
    const pnlDetail = pnlRequiresQuote
      ? (shortQuoteError ? `Quote error: ${shortQuoteError}` : "Live quote needed")
      : pnlPercent;
    elements.balancePnlDetail.textContent =
      pnlDetail + (walletStatus.needsResync ? " • Needs resync" : "");
    elements.balancePnlDetail.title = quoteError;
    elements.balancePnlDetail.classList.toggle("is-negative", pnlValue.startsWith("-"));
  }
  elements.balancePnl.classList.toggle("is-negative", pnlValue.startsWith("-"));
  elements.balancePnl.closest(".balance-stat")?.classList.toggle("is-negative", pnlValue.startsWith("-"));
}

function persistPreferences() {
  syncLocalCustomInputs();
  clearIncludeFeesOverrideWhenMatchingDefault();
  const activePreset = getActivePreset();
  mirrorWalletSelectionPreferenceOntoPreferences(state.preferences);
  const selectionTarget = selectionTargetFromWalletSelectionPreference(state.preferences);
  const payload = {
    presetId: state.preferences.presetId || activePreset?.id || "",
    selectionSource: state.preferences.selectionSource || "group",
    activeWalletGroupId: state.preferences.activeWalletGroupId || "",
    manualWalletKeys: [...(state.preferences.manualWalletKeys || [])],
    selectionTarget,
    selectionMode: selectionTarget.type,
    walletKey: selectionTarget.walletKey || "",
    walletGroupId: selectionTarget.walletGroupId || "",
    walletKeys: [...(selectionTarget.walletKeys || [])],
    includeFees: typeof state.preferences.includeFees === "boolean"
      ? state.preferences.includeFees
      : undefined,
    hideWalletGroupRow: Boolean(state.preferences.hideWalletGroupRow),
    hidePresetChipRow: Boolean(state.preferences.hidePresetChipRow)
  };
  state.preferences = {
    ...state.preferences,
    ...payload
  };
  render();
  window.parent.postMessage(
    {
      channel: CHANNEL_OUT,
      type: "persist-preferences",
      payload
    },
    EXPECTED_PARENT_ORIGIN || "*"
  );
}

function getMevIconSrc(mode) {
  const normalizedMode = String(mode || "off").trim().toLowerCase();
  if (normalizedMode === "off") {
    return NO_MEV_ICON_SRC;
  }
  if (normalizedMode === "reduced") {
    return REDUCED_MEV_ICON_SRC;
  }
  return SECURE_MEV_ICON_SRC;
}

function fillSelect(select, items, currentValue, projector) {
  const previousValue = currentValue || select.value;
  select.innerHTML = "";

  if (!items.length) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "No options";
    select.appendChild(option);
    return;
  }

  for (const item of items) {
    const { value, label } = projector(item);
    const option = document.createElement("option");
    option.value = value;
    option.textContent = label;
    select.appendChild(option);
  }

  if (previousValue && items.some((item) => projector(item).value === previousValue)) {
    select.value = previousValue;
  }
}

function fillMultiSelect(select, items, currentValues) {
  const selected = new Set(currentValues || []);
  select.innerHTML = "";
  for (const item of items) {
    const option = document.createElement("option");
    option.value = item.key;
    option.textContent = item.label;
    option.selected = selected.has(item.key);
    select.appendChild(option);
  }
}

function getActivePreset() {
  const presets = state.bootstrap?.presets || [];
  return presets.find((preset) => preset.id === state.preferences.presetId) || presets[0] || null;
}

function getActiveWalletGroup() {
  const groups = state.bootstrap?.walletGroups || [];
  const selection = normalizeWalletSelectionPreference(state.preferences);
  return groups.find((group) => group.id === selection.activeWalletGroupId) || groups[0] || null;
}

function buildWalletGroupChips(container) {
  if (!container) {
    return;
  }
  const groups = state.bootstrap?.walletGroups || [];
  const selection = normalizeWalletSelectionPreference(state.preferences);
  container.innerHTML = "";
  if (!groups.length) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "compact-chip-button overflow-chip";
    button.textContent = "Set up groups";
    button.addEventListener("click", () => emit("open-options", { section: "wallets" }));
    container.appendChild(button);
    return;
  }
  groups.forEach((group) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `compact-chip-button${selection.selectionSource === "group" && selection.activeWalletGroupId === group.id ? " active-chip" : ""}`;
    button.textContent = group.label || group.id;
    button.addEventListener("click", () => {
      bumpSelectionRevision();
      applyWalletSelectionPreference({
        selectionSource: "group",
        activeWalletGroupId: group.id,
        manualWalletKeys: [...(state.preferences.manualWalletKeys || [])]
      });
      persistPreferences();
    });
    container.appendChild(button);
  });
}

function buildPresetChips(container) {
  if (!container) {
    return;
  }
  const presets = state.bootstrap?.presets || [];
  const activePresetId = state.preferences.presetId || presets[0]?.id || "";
  container.innerHTML = "";
  if (!presets.length) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "compact-chip-button overflow-chip";
    button.textContent = "Create preset";
    button.addEventListener("click", () => emit("open-options", { section: "presets" }));
    container.appendChild(button);
    return;
  }
  presets.forEach((preset) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `compact-chip-button${preset.id === activePresetId ? " active-chip" : ""}`;
    button.textContent = preset.label || preset.id;
    button.addEventListener("click", () => {
      state.preferences.presetId = preset.id;
      persistPreferences();
    });
    container.appendChild(button);
  });
}

function renderWalletGroupChips() {
  const groups = state.bootstrap?.walletGroups || [];
  const selection = normalizeWalletSelectionPreference(state.preferences);
  const hidden = Boolean(state.preferences.hideWalletGroupRow);
  const signature = JSON.stringify({
    surface: "panel",
    hidden,
    groups: groups.map((group) => ({ id: group.id, label: group.label })),
    activeId: selection.selectionSource === "group" ? selection.activeWalletGroupId : null
  });
  memoizedRender("walletGroupChips", signature, () => {
    buildWalletGroupChips(elements.walletGroupChips);
  });
}

function renderPresetChips() {
  const presets = state.bootstrap?.presets || [];
  const activePresetId = state.preferences.presetId || presets[0]?.id || "";
  const hidden = Boolean(state.preferences.hidePresetChipRow);
  const signature = JSON.stringify({
    surface: "panel",
    hidden,
    presets: presets.map((preset) => ({ id: preset.id, label: preset.label })),
    activePresetId,
    empty: presets.length === 0
  });
  memoizedRender("presetChips", signature, () => {
    buildPresetChips(elements.presetChipRow);
  });
}

function renderExecutionSummary() {
  if (!elements.executionSummary) {
    return;
  }
  const selection = normalizeWalletSelectionPreference(state.preferences);
  const activeGroup = getActiveWalletGroup();
  const walletCount = selection.selectionSource === "group"
    ? (activeGroup?.walletKeys?.length || 0)
    : getSelectedWalletKeys().length;
  const selectedWalletKeys = getSelectedWalletKeys();
  const batchPolicy = activeGroup?.batchPolicy || null;
  const currentBuyAmount = elements.customBuyInput.value.trim()
    || quickAmountForSummary(getActivePreset())
    || state.preferences.buyAmountSol
    || "";
  const matchingBuyPreview = matchingPreviewForBuySummary(state.preview, {
    buyAmountSol: currentBuyAmount,
    walletCount,
    selection,
    activeGroup,
    selectedWalletKeys
  });
  const planningItems = [
    {
      label: "Group",
      value: selection.selectionSource === "group" ? (activeGroup?.label || "None") : "Ad hoc"
    },
    {
      label: "Wallets",
      value: String(walletCount || 0)
    },
    {
      label: "Per wallet",
      value: formatPerWalletBuyAmount(currentBuyAmount, walletCount, batchPolicy, matchingBuyPreview)
    },
    {
      label: "Batch total",
      value: formatBatchSpend(currentBuyAmount, walletCount, batchPolicy, matchingBuyPreview)
    },
    {
      label: "Variance",
      value: batchPolicy?.buyVariancePercent ? `${batchPolicy.buyVariancePercent}%` : "Off"
    },
    {
      label: "Delay",
      value: formatDelaySummary(batchPolicy)
    },
    {
      label: "Fee",
      value: formatWrapperFeeSummary(state.preview)
    }
  ];
  const batchItems = buildBatchSummaryItems(state.batchStatus, walletCount);
  const summaryItems = [...batchItems, ...planningItems];
  memoizedRender("executionSummary", JSON.stringify(summaryItems), () => {
    elements.executionSummary.innerHTML = summaryItems
      .map(
        (item) => `
          <div class="execution-summary-item${item.tone ? ` ${item.tone}` : ""}">
            <span>${escapeHtml(item.label)}</span>
            <strong>${escapeHtml(item.value)}</strong>
          </div>
        `
      )
      .join("");
  });
}

function buildBatchSummaryItems(batchStatus, fallbackWalletCount = 0) {
  if (!batchStatus?.batchId) {
    return [];
  }
  const normalizedStatus = String(batchStatus.status || "").trim().toLowerCase();
  const summary = batchStatus.summary || {};
  const wallets = Array.isArray(batchStatus.wallets) ? batchStatus.wallets : [];
  const totalWallets = Number(summary.totalWallets || wallets.length || fallbackWalletCount || 0);
  const firstSignature = wallets.find((wallet) => typeof wallet?.txSignature === "string" && wallet.txSignature)?.txSignature || "";
  return [
    {
      label: "Last Action",
      value: formatLabel(batchStatus.side || "trade")
    },
    {
      label: "Status",
      value: formatLabel(normalizedStatus || "queued"),
      tone: batchStatusTone(normalizedStatus)
    },
    {
      label: "Progress",
      value: formatBatchProgress(batchStatus, totalWallets),
      tone: batchStatusTone(normalizedStatus)
    },
    {
      label: "Signature",
      value: compactSignature(firstSignature || batchStatus.batchId)
    }
  ];
}

function formatBatchProgress(batchStatus, totalWallets = 0) {
  const summary = batchStatus?.summary || {};
  const total = Number(totalWallets || summary.totalWallets || 0);
  const confirmed = Number(summary.confirmedWallets || 0);
  const failed = Number(summary.failedWallets || 0);
  const submitted = Number(summary.submittedWallets || 0);
  const queued = Number(summary.queuedWallets || 0);
  if (confirmed > 0 && failed > 0) {
    return `${confirmed}/${total} ok, ${failed} failed`;
  }
  if (confirmed > 0) {
    return `${confirmed}/${total} confirmed`;
  }
  if (failed > 0) {
    return `${failed}/${total} failed`;
  }
  if (submitted > 0) {
    return `${submitted}/${total} submitted`;
  }
  if (queued > 0) {
    return `${queued}/${total} queued`;
  }
  return total ? `0/${total}` : "--";
}

function batchStatusTone(status) {
  const normalized = String(status || "").trim().toLowerCase();
  if (normalized === "failed") {
    return "is-bad";
  }
  if (["confirmed", "partially_confirmed"].includes(normalized)) {
    return "is-good";
  }
  if (["queued", "submitted"].includes(normalized)) {
    return "is-pending";
  }
  return "";
}

function compactSignature(value) {
  const normalized = String(value || "").trim();
  if (!normalized) {
    return "--";
  }
  if (normalized.length <= 14) {
    return normalized;
  }
  return `${normalized.slice(0, 6)}...${normalized.slice(-6)}`;
}

function getSelectedWallet() {
  const wallets = getWalletSourceWallets();
  return wallets.find((wallet) => wallet.key === state.preferences.walletKey) || null;
}

function getWalletSourceWallets() {
  if (Array.isArray(state.walletStatus?.wallets) && state.walletStatus.wallets.length) {
    return state.walletStatus.wallets;
  }
  return state.bootstrap?.wallets || [];
}

function formatLabel(value) {
  return String(value || "")
    .replace(/_/g, " ")
    .replace(/\b\w/g, (character) => character.toUpperCase());
}

function normalizeMetricValue(value, suffix = "") {
  if (value == null || value === "") {
    return suffix ? `0.000${suffix}` : "0.000";
  }

  const number = Number(value);
  if (Number.isFinite(number)) {
    return `${number.toFixed(3)}${suffix}`;
  }

  return `${String(value)}${suffix}`;
}

function formatPnlMetric(value) {
  if (value == null || value === "") {
    return "0.000";
  }

  const number = Number(value);
  if (Number.isFinite(number)) {
    if (number === 0) {
      return "0.000";
    }
    return `${number > 0 ? "+" : ""}${number.toFixed(3)}`;
  }

  return String(value);
}

function formatPercentMetric(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) {
    return "--%";
  }
  if (number === 0) {
    return "0.0%";
  }
  return `${number > 0 ? "+" : ""}${number.toFixed(1)}%`;
}

function formatValueWithUnitIcon(value) {
  return `
    <span class="value-with-unit-icon">
      <img class="unit-icon" src="${SOL_ICON_SRC}" alt="" aria-hidden="true" />
      <span>${escapeHtml(String(value))}</span>
    </span>
  `;
}

function getPresetBuyAmountRows(preset) {
  if (!preset) return 1;
  const explicit = Number(preset.buyAmountRows);
  if (explicit === 2) return 2;
  if (Array.isArray(preset.buyAmountsSol) && preset.buyAmountsSol.length > 4) {
    const tail = preset.buyAmountsSol.slice(4, 8);
    if (tail.some((value) => String(value || "").trim() !== "")) {
      return 2;
    }
  }
  return 1;
}

function getPresetSellPercentRows(preset) {
  if (!preset) return 1;
  const explicit = Number(preset.sellPercentRows);
  if (explicit === 2) return 2;
  if (Array.isArray(preset.sellAmountsPercent) && preset.sellAmountsPercent.length > 4) {
    const tail = preset.sellAmountsPercent.slice(4, 8);
    if (tail.some((value) => String(value || "").trim() !== "")) {
      return 2;
    }
  }
  return 1;
}

function getPresetBuyAmounts(preset) {
  const rows = getPresetBuyAmountRows(preset);
  return normalizeShortcutValues(preset?.buyAmountsSol, preset?.buyAmountSol, rows * 4);
}

function getPresetSellAmounts(preset) {
  const rows = getPresetSellPercentRows(preset);
  return normalizeShortcutValues(preset?.sellAmountsPercent, preset?.sellPercent, rows * 4);
}

function normalizeShortcutValues(values, fallbackValue, length = 4) {
  const targetLength = Number.isFinite(length) && length > 0 ? Math.floor(length) : 4;
  const normalized = Array.isArray(values)
    ? values.slice(0, targetLength).map((value) => String(value || "").trim())
    : [];

  while (normalized.length < targetLength) {
    normalized.push("");
  }

  if (!normalized.some(Boolean) && fallbackValue) {
    normalized[0] = String(fallbackValue).trim();
  }

  return normalized.slice(0, targetLength);
}

function getMetricValue(sources, keys) {
  for (const source of sources) {
    if (!source) {
      continue;
    }

    for (const key of keys) {
      if (source[key] != null && source[key] !== "") {
        return source[key];
      }
    }
  }

  return null;
}

function getHoldingValueSol(sources) {
  const direct = getMetricValue(sources, ["holdingValueSol"]);
  const directNumber = Number(direct);
  if (Number.isFinite(directNumber)) {
    return directNumber;
  }
  return null;
}

function renderWalletPicker() {
  const wallets = getWalletSourceWallets();
  const selection = normalizeWalletSelectionPreference(state.preferences);
  const activeGroup = getActiveWalletGroup();
  const activeWalletSet = selection.selectionSource === "group"
    ? new Set(activeGroup?.walletKeys || [])
    : new Set();
  const selectedKeys = new Set(getSelectedWalletKeys());

  const settingsOrder = new Map(
    (state.bootstrap?.wallets || []).map((wallet, index) => [wallet.key, index])
  );
  const orderWallets = (list) => [...list].sort((left, right) => {
    const leftIndex = settingsOrder.has(left.key) ? settingsOrder.get(left.key) : Number.MAX_SAFE_INTEGER;
    const rightIndex = settingsOrder.has(right.key) ? settingsOrder.get(right.key) : Number.MAX_SAFE_INTEGER;
    if (leftIndex !== rightIndex) {
      return leftIndex - rightIndex;
    }
    return formatWalletDisplayLabel(left).localeCompare(
      formatWalletDisplayLabel(right),
      undefined,
      { numeric: true, sensitivity: "base" }
    );
  });

  const holders = wallets.filter((wallet) => getWalletTokenBalanceNumber(wallet) > 0);
  const nonHolders = wallets.filter((wallet) => getWalletTokenBalanceNumber(wallet) <= 0);
  const orderedHolders = orderWallets(holders);
  const orderedNonHolders = orderWallets(nonHolders);
  const distributionPending = Boolean(state.tokenDistributionPending);

  const hasHolders = orderedHolders.length > 0;
  elements.walletHoldersSection.classList.toggle("is-hidden", !hasHolders);
  // The close button lives inline in each section's actions row. Only the
  // topmost visible section should render it so the UX matches a single
  // close affordance regardless of whether the holders section is shown.
  elements.walletModalCloseButtons.forEach((button) => {
    const section = button.closest(".wallet-section");
    const isHoldersSection = section?.id === "wallet-holders-section";
    const shouldShow = hasHolders ? isHoldersSection : !isHoldersSection;
    button.hidden = !shouldShow;
  });
  if (hasHolders) {
    const holderKeys = orderedHolders.map((wallet) => wallet.key);
    const allHoldersSelected = holderKeys.every((key) => selectedKeys.has(key));
    elements.walletHoldersSelectAll.textContent = allHoldersSelected ? "Unselect All" : "Select All";
    renderWalletRows(elements.walletHoldersList, orderedHolders, {
      selectedKeys,
      activeWalletSet,
      showTokenBalance: true
    });
  } else {
    elements.walletHoldersList.innerHTML = "";
  }
  elements.walletConsolidate.disabled = distributionPending || !hasHolders;
  elements.walletSplit.disabled = distributionPending || !hasHolders;

  elements.walletPickerList.innerHTML = "";
  const nonHolderKeys = orderedNonHolders.map((wallet) => wallet.key);
  const allNonHoldersSelected =
    nonHolderKeys.length > 0 && nonHolderKeys.every((key) => selectedKeys.has(key));
  elements.walletSelectAll.textContent = allNonHoldersSelected ? "Unselect All" : "Select All";
  elements.walletSelectAll.disabled = nonHolderKeys.length === 0;
  elements.walletSelectBalance.disabled = !orderedNonHolders.some((wallet) => getWalletBalanceNumber(wallet) > 0);

  if (!wallets.length) {
    const empty = document.createElement("div");
    empty.className = "wallet-picker-empty";
    empty.textContent = "No imported wallets yet.";
    elements.walletPickerList.appendChild(empty);
    return;
  }
  if (!orderedNonHolders.length) {
    const empty = document.createElement("div");
    empty.className = "wallet-picker-empty";
    empty.textContent = "All active wallets are holding this token.";
    elements.walletPickerList.appendChild(empty);
    return;
  }

  renderWalletRows(elements.walletPickerList, orderedNonHolders, {
    selectedKeys,
    activeWalletSet,
    showTokenBalance: false
  });
}

function renderWalletRows(container, wallets, { selectedKeys, activeWalletSet, showTokenBalance }) {
  container.innerHTML = "";
  for (const wallet of wallets) {
    const row = document.createElement("button");
    row.type = "button";
    const tokenBalance = getWalletTokenBalanceNumber(wallet);
    const classes = ["wallet-picker-row"];
    if (selectedKeys.has(wallet.key)) classes.push("is-selected");
    if (activeWalletSet.has(wallet.key)) classes.push("is-prioritized");
    if (showTokenBalance && tokenBalance > 0) classes.push("has-token");
    row.className = classes.join(" ");
    const walletLabel = formatWalletDisplayLabel(wallet);
    const balanceLabel = formatWalletBalance(wallet);
    const tokenPill = showTokenBalance && tokenBalance > 0
      ? `<span class="wallet-picker-token">
          <span class="wallet-token-icon" aria-hidden="true"></span>
          ${escapeHtml(formatCompactAmount(tokenBalance))}
        </span>`
      : "";
    row.innerHTML = `
      <span class="wallet-picker-checkbox${selectedKeys.has(wallet.key) ? " is-selected" : ""}">
        <span class="wallet-picker-checkbox-dot"></span>
      </span>
      <span class="wallet-picker-copy">
        <span class="wallet-picker-name">${escapeHtml(walletLabel)}</span>
        <span class="wallet-picker-meta">${escapeHtml(shortenWalletKey(wallet.publicKey || wallet.key))}</span>
      </span>
      <span class="wallet-picker-balance${getWalletBalanceNumber(wallet) > 0 ? " has-balance" : ""}">
        <img class="wallet-balance-sol-icon" src="${SOL_ICON_SRC}" alt="" aria-hidden="true" />
        ${escapeHtml(balanceLabel)}
      </span>
      ${tokenPill}
    `;
    bindImmediateButtonAction(row, () => {
      toggleWalletSelection(wallet.key);
    });
    container.appendChild(row);
  }
}

function toggleWalletSelection(walletKey) {
  const selected = getSelectedWalletKeys();
  const exists = selected.includes(walletKey);
  const nextKeys = exists ? selected.filter((key) => key !== walletKey) : [...selected, walletKey];
  applyWalletSelection(nextKeys);
}

function applyWalletSelection(walletKeys) {
  const wallets = state.bootstrap?.wallets || [];
  const knownWalletKeys = new Set(wallets.map((wallet) => wallet.key));
  const nextKeys = Array.from(new Set(walletKeys.filter((key) => knownWalletKeys.has(key))));
  bumpSelectionRevision();
  applyWalletSelectionPreference({
    selectionSource: "manual",
    activeWalletGroupId: state.preferences.activeWalletGroupId || "",
    manualWalletKeys: nextKeys
  });
  persistPreferences();
}

function normalizeWalletSelection() {
  const wallets = state.bootstrap?.wallets || [];
  const knownWalletKeys = new Set(wallets.map((wallet) => wallet.key));
  const groups = state.bootstrap?.walletGroups || [];
  const selection = normalizeWalletSelectionPreference(state.preferences);
  if (groups[0] && !selection.activeWalletGroupId) {
    selection.activeWalletGroupId = groups[0].id;
  }
  if (selection.selectionSource === "group") {
    const knownGroupIds = new Set(groups.map((group) => group.id));
    if (!knownGroupIds.has(selection.activeWalletGroupId) && groups[0]) {
      selection.activeWalletGroupId = groups[0].id;
    } else if (!knownGroupIds.has(selection.activeWalletGroupId) && wallets[0]) {
      selection.selectionSource = "manual";
      selection.manualWalletKeys = [wallets[0].key];
    }
  } else {
    selection.manualWalletKeys = (selection.manualWalletKeys || []).filter((key) => knownWalletKeys.has(key));
  }
  applyWalletSelectionPreference(selection);
}

function getSelectedWalletKeys() {
  const selection = normalizeWalletSelectionPreference(state.preferences);
  if (selection.selectionSource === "group") {
    const activeGroup = getActiveWalletGroup();
    return Array.from(new Set((activeGroup?.walletKeys || []).filter(Boolean)));
  }
  return Array.from(new Set((selection.manualWalletKeys || []).filter(Boolean)));
}

function getSelectedHolderWalletKeys() {
  const selectedKeys = new Set(getSelectedWalletKeys());
  return getWalletSourceWallets()
    .filter((wallet) => selectedKeys.has(wallet.key) && getWalletTokenBalanceNumber(wallet) > 0)
    .map((wallet) => wallet.key);
}

function normalizeSelectionTarget(value) {
  const type = String(value?.type || value?.selectionMode || "wallet_group").trim() || "wallet_group";
  return {
    type: ["wallet_group", "wallet_list", "single_wallet"].includes(type) ? type : "wallet_group",
    walletKey: String(value?.walletKey || value?.selectionTarget?.walletKey || "").trim(),
    walletGroupId: String(value?.walletGroupId || value?.selectionTarget?.walletGroupId || "").trim(),
    walletKeys: Array.isArray(value?.walletKeys)
      ? value.walletKeys.map((entry) => String(entry || "").trim()).filter(Boolean)
      : Array.isArray(value?.selectionTarget?.walletKeys)
        ? value.selectionTarget.walletKeys.map((entry) => String(entry || "").trim()).filter(Boolean)
        : []
  };
}

function mirrorSelectionTargetOntoPreferences(preferences) {
  const target = normalizeSelectionTarget(preferences.selectionTarget || preferences);
  preferences.selectionTarget = target;
  preferences.selectionMode = target.type;
  preferences.walletKey = target.walletKey;
  preferences.walletGroupId = target.walletGroupId;
  preferences.walletKeys = [...target.walletKeys];
}

function applySelectionTarget(target) {
  state.preferences.selectionTarget = normalizeSelectionTarget(target);
  mirrorSelectionTargetOntoPreferences(state.preferences);
}

function normalizeWalletSelectionPreference(value) {
  const selectionSource = String(value?.selectionSource || "").trim().toLowerCase();
  const activeWalletGroupId = String(
    value?.activeWalletGroupId ||
    value?.selectionTarget?.activeWalletGroupId ||
    value?.walletGroupId ||
    ""
  ).trim();
  const directManualWalletKeys = Array.isArray(value?.manualWalletKeys)
    ? value.manualWalletKeys
    : Array.isArray(value?.selectionTarget?.manualWalletKeys)
      ? value.selectionTarget.manualWalletKeys
      : null;
  const normalizedManualWalletKeys = (directManualWalletKeys || []).map((entry) => String(entry || "").trim()).filter(Boolean);

  if (selectionSource === "group" || selectionSource === "manual") {
    return {
      selectionSource,
      activeWalletGroupId,
      manualWalletKeys: Array.from(new Set(normalizedManualWalletKeys))
    };
  }

  const target = normalizeSelectionTarget(value?.selectionTarget || value);
  if (target.type === "wallet_group") {
    return {
      selectionSource: "group",
      activeWalletGroupId: target.walletGroupId,
      manualWalletKeys: []
    };
  }
  const manualWalletKeys = target.type === "wallet_list"
    ? target.walletKeys
    : target.walletKey
      ? [target.walletKey]
      : [];
  return {
    selectionSource: "manual",
    activeWalletGroupId,
    manualWalletKeys: Array.from(new Set(manualWalletKeys.map((entry) => String(entry || "").trim()).filter(Boolean)))
  };
}

function selectionTargetFromWalletSelectionPreference(selection) {
  if (selection.selectionSource === "group") {
    return {
      type: "wallet_group",
      walletKey: "",
      walletGroupId: String(selection.activeWalletGroupId || "").trim(),
      walletKeys: []
    };
  }
  const manualWalletKeys = Array.from(new Set((selection.manualWalletKeys || []).map((entry) => String(entry || "").trim()).filter(Boolean)));
  return {
    type: manualWalletKeys.length === 1 ? "single_wallet" : "wallet_list",
    walletKey: manualWalletKeys[0] || "",
    walletGroupId: "",
    walletKeys: manualWalletKeys
  };
}

function mirrorWalletSelectionPreferenceOntoPreferences(preferences, sourceValue = preferences) {
  const selection = normalizeWalletSelectionPreference(sourceValue);
  preferences.selectionSource = selection.selectionSource;
  preferences.activeWalletGroupId = selection.activeWalletGroupId;
  preferences.manualWalletKeys = [...selection.manualWalletKeys];
  const target = selectionTargetFromWalletSelectionPreference(selection);
  preferences.selectionTarget = target;
  preferences.selectionMode = target.type;
  preferences.walletKey = target.walletKey;
  preferences.walletGroupId = target.walletGroupId;
  preferences.walletKeys = [...target.walletKeys];
}

function applyWalletSelectionPreference(selection) {
  mirrorWalletSelectionPreferenceOntoPreferences(state.preferences, selection);
}

function bumpSelectionRevision() {
  state.preferences.selectionRevision = Math.max(0, Number(state.preferences.selectionRevision || 0) || 0) + 1;
}

function quickAmountForSummary(preset) {
  return getPresetBuyAmounts(preset).find(Boolean) || "";
}

function numericStringsMatch(left, right) {
  const leftNumber = Number(left || 0);
  const rightNumber = Number(right || 0);
  return Number.isFinite(leftNumber) &&
    Number.isFinite(rightNumber) &&
    leftNumber > 0 &&
    Math.abs(leftNumber - rightNumber) < 0.000000001;
}

function walletKeySetsMatch(left, right) {
  const normalize = (items) => Array.from(new Set(
    (Array.isArray(items) ? items : [])
      .map((entry) => String(entry || "").trim())
      .filter(Boolean)
  )).sort();
  const leftKeys = normalize(left);
  const rightKeys = normalize(right);
  return leftKeys.length === rightKeys.length &&
    leftKeys.every((key, index) => key === rightKeys[index]);
}

function matchingPreviewForBuySummary(preview, {
  buyAmountSol,
  walletCount,
  selection,
  activeGroup,
  selectedWalletKeys
}) {
  if (!preview || preview.side !== "buy") {
    return null;
  }
  const plan = Array.isArray(preview.executionPlan) ? preview.executionPlan : [];
  if (!plan.length || plan.length !== walletCount) {
    return null;
  }
  if (!numericStringsMatch(preview.policy?.buyAmountSol, buyAmountSol)) {
    return null;
  }
  const target = preview.target || {};
  if (selection.selectionSource === "group") {
    const activeGroupId = String(activeGroup?.id || "").trim();
    return activeGroupId && String(target.walletGroupId || "").trim() === activeGroupId
      ? preview
      : null;
  }
  return walletKeySetsMatch(target.walletKeys, selectedWalletKeys) ? preview : null;
}

function previewBuyAmountNumbers(preview) {
  const plan = Array.isArray(preview?.executionPlan) ? preview.executionPlan : [];
  return plan
    .map((entry) => Number(entry?.buyAmountSol || 0))
    .filter((value) => Number.isFinite(value) && value > 0);
}

function formatSolAmount(value) {
  return `${value.toFixed(3)} SOL`;
}

function formatPerWalletBuyAmount(buyAmountSol, walletCount, batchPolicy, preview = null) {
  const plannedAmounts = previewBuyAmountNumbers(preview);
  if (plannedAmounts.length) {
    const min = Math.min(...plannedAmounts);
    const max = Math.max(...plannedAmounts);
    return min === max
      ? formatSolAmount(min)
      : `${min.toFixed(3)}-${max.toFixed(3)} SOL`;
  }
  const numericAmount = Number(buyAmountSol || 0);
  if (!Number.isFinite(numericAmount) || numericAmount <= 0 || walletCount <= 0) {
    return "--";
  }
  const distributionMode = batchPolicy?.distributionMode || "each";
  const amount = distributionMode === "split" ? numericAmount / walletCount : numericAmount;
  const varianceSuffix = batchPolicy?.buyVariancePercent ? ` ±${batchPolicy.buyVariancePercent}%` : "";
  return `${amount.toFixed(3)} SOL${varianceSuffix}`;
}

function formatBatchSpend(buyAmountSol, walletCount, batchPolicy, preview = null) {
  const plannedAmounts = previewBuyAmountNumbers(preview);
  if (plannedAmounts.length) {
    const total = plannedAmounts.reduce((sum, value) => sum + value, 0);
    return formatSolAmount(total);
  }
  const numericAmount = Number(buyAmountSol || 0);
  if (!Number.isFinite(numericAmount) || numericAmount <= 0 || walletCount <= 0) {
    return "--";
  }
  const distributionMode = batchPolicy?.distributionMode || "each";
  const total = distributionMode === "split" ? numericAmount : numericAmount * walletCount;
  const approximatePrefix = batchPolicy?.buyVariancePercent ? "~" : "";
  return `${approximatePrefix}${total.toFixed(3)} SOL`;
}

function formatDelaySummary(batchPolicy) {
  if (!batchPolicy || batchPolicy.transactionDelayMode === "off") {
    return "Off";
  }
  const prefix = batchPolicy.transactionDelayMode === "first_buy_only" ? "First buys: " : "";
  if (batchPolicy.transactionDelayStrategy === "random") {
    return `${prefix}${batchPolicy.transactionDelayMinMs || 0}-${batchPolicy.transactionDelayMaxMs || 0}ms`;
  }
  return `${prefix}${batchPolicy.transactionDelayMs || 0}ms`;
}

// Map a bps integer to the customer-facing percent string. Kept inline
// (instead of pulling shared-constants.js) because the panel is loaded
// as an ES module and the shared file is an IIFE attached to `window`.
function formatWrapperFeeBpsPercent(bps) {
  const value = Number.isFinite(Number(bps)) ? Number(bps) : 0;
  if (value <= 0) return "0%";
  if (value === 10) return "0.1%";
  if (value === 20) return "0.2%";
  return `${(value / 100).toFixed(2)}%`;
}

function formatWrapperFeeSummary(preview) {
  const plan = Array.isArray(preview?.executionPlan) ? preview.executionPlan : [];
  const firstWithRoute = plan.find(
    (entry) => entry && entry.wrapperRoute && entry.wrapperRoute !== "no_sol"
  );
  if (!firstWithRoute) {
    return plan.length ? "No-SOL route" : "--";
  }
  const percent = formatWrapperFeeBpsPercent(firstWithRoute.wrapperFeeBps);
  const timing = firstWithRoute.wrapperRoute === "sol_out" ? "post-swap" : "pre-swap";
  const feeSol = String(firstWithRoute.wrapperFeeSol || "").trim();
  if (feeSol && feeSol !== "0") {
    return `${percent} (${timing}, ~${feeSol} SOL)`;
  }
  return `${percent} (${timing})`;
}

function shortenWalletKey(value) {
  const text = String(value || "").trim();
  if (text.length <= 10) {
    return text || "No address";
  }
  return `${text.slice(0, 4)}...${text.slice(-4)}`;
}

function formatWalletDisplayLabel(wallet) {
  const label = String(wallet?.label || wallet?.customName || wallet?.key || "").trim();
  if (!label) {
    return "Unnamed wallet";
  }
  const genericMatch = label.match(/^SOLANA_PRIVATE_KEY(\d+)?$/i);
  if (!genericMatch) {
    return label;
  }
  return `#${genericMatch[1] || "1"}`;
}

function getWalletBalanceNumber(wallet) {
  const raw =
    wallet?.balanceSol ??
    wallet?.balance_sol ??
    wallet?.solBalance ??
    wallet?.balance ??
    null;
  const number = Number(raw);
  return Number.isFinite(number) ? number : 0;
}

function formatWalletBalance(wallet) {
  const number = getWalletBalanceNumber(wallet);
  return number > 0 ? number.toFixed(2) : "0.00";
}

function getWalletTokenBalanceNumber(wallet) {
  const raw =
    wallet?.tokenBalance ??
    wallet?.mintBalanceUi ??
    wallet?.mintBalance ??
    wallet?.holdingAmount ??
    null;
  const number = Number(raw);
  return Number.isFinite(number) && number > 0 ? number : 0;
}

function formatCompactAmount(value) {
  const number = Number(value);
  if (!Number.isFinite(number) || number <= 0) {
    return "0";
  }
  const abs = Math.abs(number);
  if (abs >= 1_000_000_000) {
    return `${(number / 1_000_000_000).toFixed(abs >= 10_000_000_000 ? 1 : 2)}B`;
  }
  if (abs >= 1_000_000) {
    return `${(number / 1_000_000).toFixed(abs >= 10_000_000 ? 1 : 2)}M`;
  }
  if (abs >= 1_000) {
    return `${(number / 1_000).toFixed(abs >= 10_000 ? 1 : 2)}K`;
  }
  if (abs >= 1) {
    return number.toFixed(2);
  }
  return number.toPrecision(2);
}

function getBuySlippagePercent(preset) {
  return String(preset?.buySlippagePercent ?? preset?.slippagePercent ?? "").trim();
}

function getSellSlippagePercent(preset) {
  return String(preset?.sellSlippagePercent ?? preset?.slippagePercent ?? "").trim();
}

function getBuyMevMode(preset) {
  return String(preset?.buyMevMode ?? preset?.mevMode ?? "off").trim() || "off";
}

function getSellMevMode(preset) {
  return String(preset?.sellMevMode ?? preset?.mevMode ?? "off").trim() || "off";
}

function escapeHtml(value) {
  return String(value || "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll("\"", "&quot;");
}
