import {
  ensureHostPermission,
  getHostAuthToken,
  getHostBase,
  getLaunchdeckHostBase,
  isLoopbackHost,
  normalizeHostBase,
  normalizeLaunchdeckHostBase,
  originPatternFromHostBase,
  setHostAuthToken,
  setHostBase
} from "../shared/host-client.js";
import { callBackground } from "../shared/background-rpc.js";
import { getSiteFeatures, saveSiteFeatures } from "../shared/site-features.js";
import {
  SOUND_CUSTOM_ID,
  SOUND_CUSTOM_MAX_BYTES,
  SOUND_TEMPLATES,
  defaultAppearance,
  getAppearance,
  normalizeAppearance,
  resolveSoundUrl,
  saveAppearance
} from "../shared/appearance.js";
import {
  APPEARANCE_STORAGE_KEY,
  OPTIONS_TARGET_SECTION_KEY,
  RUNTIME_DIAGNOSTICS_REVISION_KEY,
  RUNTIME_DIAGNOSTICS_SNAPSHOT_KEY
} from "../shared/constants.js";

const BOOTSTRAP_REVISION_KEY = "trenchTools.bootstrapRevision";
const WALLET_STATUS_REVISION_KEY = "trenchTools.walletStatusRevision";
const HOST_AUTH_TOKEN_MERGE_WARNING_KEY = "trenchTools.hostAuthTokenMergeV1Warning";
const OPTIONS_TOAST_SUCCESS_ICON_URL = chrome.runtime.getURL("assets/confirmed-icon.png");
const OPTIONS_TOAST_FAIL_ICON_URL = chrome.runtime.getURL("assets/fail-icon.png");

const state = {
  health: null,
  bootstrap: emptyBootstrap(),
  settings: emptySettings(),
  presets: [],
  launchdeckConfig: createDefaultLaunchdeckConfig(),
  launchdeckSettingsPayload: null,
  wallets: [],
  walletGroups: [],
  walletBalances: new Map(),
  authBootstrap: null,
  authTokens: [],
  siteFeatures: null,
  appearance: defaultAppearance(),
  appearancePreview: { side: null, audio: null },
  appearanceStatusTimers: { buy: null, sell: null },
  rewards: {
    loading: false,
    claiming: false,
    lastRefreshedAt: null,
    providers: [],
    errors: []
  },
  activeSection: "presets",
  presetModalOpen: false,
  editingPresetId: null,
  launchdeckPresetModalOpen: false,
  editingLaunchdeckPresetId: null,
  walletEditModal: createEmptyWalletEditModalState(),
  createGroupModal: createEmptyCreateGroupModalState(),
  engineSettingsDirty: false,
  runtimeDiagnosticToastKeys: new Set()
};

const elements = {
  navButtons: Array.from(document.querySelectorAll("[data-section-target]")),
  hostInput: document.getElementById("host-input"),
  hostAuthTokenInput: document.getElementById("host-auth-token-input"),
  saveHostButton: document.getElementById("save-host-button"),
  testHostButton: document.getElementById("test-host-button"),
  launchdeckHostInput: document.getElementById("launchdeck-host-input"),
  testLaunchdeckHostButton: document.getElementById("test-launchdeck-host-button"),
  reloadHostButton: document.getElementById("reload-host-button"),
  addExecutionPresetButton: document.getElementById("add-execution-preset-button"),
  addLaunchdeckPresetButton: document.getElementById("add-launchdeck-preset-button"),
  addWalletButton: document.getElementById("add-wallet-button"),
  saveEngineSettingsButton: document.getElementById("save-engine-settings-button"),
  engineSettingsDirtyBadge: document.getElementById("engine-settings-dirty-badge"),
  activeWalletsGrid: document.getElementById("active-wallets-grid"),
  walletGroupsList: document.getElementById("wallet-groups-list"),
  openCreateGroupModalButton: document.getElementById("open-create-group-modal"),
  buyDistributionSplit: document.getElementById("buy-distribution-split"),
  buyDistributionEach: document.getElementById("buy-distribution-each"),
  walletEditModal: document.getElementById("wallet-edit-modal"),
  walletEditModalTitle: document.getElementById("wallet-edit-modal-title"),
  walletEditModalClose: document.getElementById("wallet-edit-modal-close"),
  walletEditModalCancel: document.getElementById("wallet-edit-modal-cancel"),
  walletEditModalSave: document.getElementById("wallet-edit-modal-save"),
  walletEditModalDelete: document.getElementById("wallet-edit-modal-delete"),
  walletEditLabel: document.getElementById("wallet-edit-label"),
  walletEditPrivateKey: document.getElementById("wallet-edit-private-key"),
  walletEditPrivateKeyLabel: document.getElementById("wallet-edit-private-key-label"),
  walletEditPrivateKeyRow: document.getElementById("wallet-edit-private-key-row"),
  walletEditPublicKey: document.getElementById("wallet-edit-public-key"),
  walletEditPublicKeyRow: document.getElementById("wallet-edit-public-key-row"),
  walletEditEnabled: document.getElementById("wallet-edit-enabled"),
  walletEditEmojiButton: document.getElementById("wallet-edit-emoji-button"),
  walletEditEmojiGlyph: document.getElementById("wallet-edit-emoji-glyph"),
  walletEditEmojiPopover: document.getElementById("wallet-edit-emoji-popover"),
  walletEditPrivateKeySummaryLabel: document.getElementById("wallet-edit-private-key-summary-label"),
  createGroupModal: document.getElementById("create-group-modal"),
  createGroupModalTitle: document.getElementById("create-group-modal-title"),
  createGroupModalClose: document.getElementById("create-group-modal-close"),
  createGroupModalCancel: document.getElementById("create-group-modal-cancel"),
  createGroupModalSave: document.getElementById("create-group-modal-save"),
  createGroupModalDelete: document.getElementById("create-group-modal-delete"),
  createGroupNameInput: document.getElementById("create-group-name-input"),
  createGroupEmojiButton: document.getElementById("create-group-emoji-button"),
  createGroupEmojiGlyph: document.getElementById("create-group-emoji-glyph"),
  createGroupEmojiPopover: document.getElementById("create-group-emoji-popover"),
  createGroupWalletList: document.getElementById("create-group-wallet-list"),
  createGroupVarianceToggle: document.getElementById("create-group-variance-toggle"),
  createGroupVarianceSliderRow: document.getElementById("create-group-variance-slider-row"),
  createGroupVarianceSlider: document.getElementById("create-group-variance-slider"),
  createGroupVarianceInput: document.getElementById("create-group-variance-input"),
  createGroupStaggerToggle: document.getElementById("create-group-stagger-toggle"),
  createGroupStaggerFields: document.getElementById("create-group-stagger-fields"),
  createGroupStaggerStrategyButtons: Array.from(
    document.querySelectorAll("[data-stagger-strategy]")
  ),
  createGroupStaggerFixed: document.getElementById("create-group-stagger-fixed"),
  createGroupStaggerRandom: document.getElementById("create-group-stagger-random"),
  createGroupStaggerMs: document.getElementById("create-group-stagger-ms"),
  createGroupStaggerMinMs: document.getElementById("create-group-stagger-min-ms"),
  createGroupStaggerMaxMs: document.getElementById("create-group-stagger-max-ms"),
  engineBuySlippage: document.getElementById("engine-buy-slippage"),
  engineSellSlippage: document.getElementById("engine-sell-slippage"),
  engineBuyMevMode: document.getElementById("engine-buy-mev-mode"),
  engineSellMevMode: document.getElementById("engine-sell-mev-mode"),
  engineProvider: document.getElementById("engine-provider"),
  engineEndpointProfile: document.getElementById("engine-endpoint-profile"),
  engineEndpointProfileCustom: document.getElementById("engine-endpoint-profile-custom"),
  engineEndpointProfileCustomRow: document.getElementById("engine-endpoint-profile-custom-row"),
  engineCommitment: document.getElementById("engine-commitment"),
  engineSkipPreflight: document.getElementById("engine-skip-preflight"),
  engineTrackSendBlockHeight: document.getElementById("engine-track-send-block-height"),
  engineAllowNonCanonicalPoolTrades: document.getElementById("engine-allow-non-canonical-pool-trades"),
  enginePnlTrackingMode: document.getElementById("engine-pnl-tracking-mode"),
  enginePnlIncludeFees: document.getElementById("engine-pnl-include-fees"),
  engineWrapperDefaultFeeBps: document.getElementById("engine-wrapper-default-fee-bps"),
  exportPnlHistoryButton: document.getElementById("export-pnl-history-button"),
  wipePnlHistoryButton: document.getElementById("wipe-pnl-history-button"),
  engineMaxActiveBatches: document.getElementById("engine-max-active-batches"),
  engineRpcUrl: document.getElementById("engine-rpc-url"),
  engineWsUrl: document.getElementById("engine-ws-url"),
  engineWarmRpcUrl: document.getElementById("engine-warm-rpc-url"),
  engineWarmWsUrl: document.getElementById("engine-warm-ws-url"),
  engineSharedRegion: document.getElementById("engine-shared-region"),
  engineHeliusRpcUrl: document.getElementById("engine-helius-rpc-url"),
  engineHeliusWsUrl: document.getElementById("engine-helius-ws-url"),
  engineStandardRpcSendUrls: document.getElementById("engine-standard-rpc-send-urls"),
  engineHeliusSenderRegion: document.getElementById("engine-helius-sender-region"),
  presetModal: document.getElementById("preset-modal"),
  presetModalTitle: document.getElementById("preset-modal-title"),
  presetModalClose: document.getElementById("preset-modal-close"),
  presetModalCancel: document.getElementById("preset-modal-cancel"),
  presetModalSave: document.getElementById("preset-modal-save"),
  presetModalId: document.getElementById("preset-modal-id"),
  presetModalLabel: document.getElementById("preset-modal-label"),
  presetModalBuyAmounts: Array.from(document.querySelectorAll("[data-buy-amount-input]")),
  presetModalBuyAutoTip: document.getElementById("preset-modal-buy-auto-tip"),
  presetModalBuyAutoFee: document.getElementById("preset-modal-buy-auto-fee"),
  presetModalBuyMaxFee: document.getElementById("preset-modal-buy-max-fee"),
  presetModalBuyFee: document.getElementById("preset-modal-buy-fee"),
  presetModalBuyTip: document.getElementById("preset-modal-buy-tip"),
  presetModalBuySlippage: document.getElementById("preset-modal-buy-slippage"),
  presetModalBuyMevMode: document.getElementById("preset-modal-buy-mev-mode"),
  presetModalBuyProvider: document.getElementById("preset-modal-buy-provider"),
  presetModalBuyEndpointProfile: document.getElementById("preset-modal-buy-endpoint-profile"),
  presetModalSellPercents: Array.from(document.querySelectorAll("[data-sell-percent-input]")),
  presetModalSellAutoTip: document.getElementById("preset-modal-sell-auto-tip"),
  presetModalSellAutoFee: document.getElementById("preset-modal-sell-auto-fee"),
  presetModalSellMaxFee: document.getElementById("preset-modal-sell-max-fee"),
  presetModalSellFee: document.getElementById("preset-modal-sell-fee"),
  presetModalSellTip: document.getElementById("preset-modal-sell-tip"),
  presetModalSellSlippage: document.getElementById("preset-modal-sell-slippage"),
  presetModalSellMevMode: document.getElementById("preset-modal-sell-mev-mode"),
  presetModalSellProvider: document.getElementById("preset-modal-sell-provider"),
  presetModalSellEndpointProfile: document.getElementById("preset-modal-sell-endpoint-profile"),
  hostStatusBadge: document.getElementById("host-status-badge"),
  connectionStatus: document.getElementById("connection-status"),
  launchdeckConnectionStatus: document.getElementById("launchdeck-connection-status"),
  authBootstrapHint: document.getElementById("auth-bootstrap-hint"),
  hostAuthTokenMergeNotice: document.getElementById("host-auth-token-merge-notice"),
  hostAuthTokenMergeNoticeDismiss: document.getElementById("host-auth-token-merge-notice-dismiss"),
  bootstrapJson: document.getElementById("bootstrap-json"),
  siteAxiomEnabled: document.getElementById("site-axiom-enabled"),
  siteAxiomLauncher: document.getElementById("site-axiom-launcher"),
  siteAxiomInstantTrade: document.getElementById("site-axiom-instant-trade"),
  siteAxiomInstantTradeButtonModeCount: document.getElementById("site-axiom-instant-trade-button-mode-count"),
  siteAxiomLaunchdeck: document.getElementById("site-axiom-launchdeck"),
  siteAxiomPulseQb: document.getElementById("site-axiom-pulse-qb"),
  siteAxiomPulsePanel: document.getElementById("site-axiom-pulse-panel"),
  siteAxiomPulseVamp: document.getElementById("site-axiom-pulse-vamp"),
  siteAxiomVampIconMode: document.getElementById("site-axiom-vamp-icon-mode"),
  siteAxiomPulseVampMode: document.getElementById("site-axiom-pulse-vamp-mode"),
  siteAxiomDexScreenerIconMode: document.getElementById("site-axiom-dexscreener-icon-mode"),
  siteAxiomPostDeployAction: document.getElementById("site-axiom-post-deploy-action"),
  siteAxiomWalletTracker: document.getElementById("site-axiom-wallet-tracker"),
  siteAxiomWatchlist: document.getElementById("site-axiom-watchlist"),
  siteJ7Enabled: document.getElementById("site-j7-enabled"),
  executionPresetList: document.getElementById("execution-preset-list"),
  launchdeckPresetList: document.getElementById("launchdeck-preset-list"),
  launchdeckPresetModal: document.getElementById("launchdeck-preset-modal"),
  launchdeckPresetModalTitle: document.getElementById("launchdeck-preset-modal-title"),
  launchdeckPresetModalClose: document.getElementById("launchdeck-preset-modal-close"),
  launchdeckPresetModalDelete: document.getElementById("launchdeck-preset-modal-delete"),
  launchdeckPresetModalCancel: document.getElementById("launchdeck-preset-modal-cancel"),
  launchdeckPresetModalSave: document.getElementById("launchdeck-preset-modal-save"),
  launchdeckPresetId: document.getElementById("launchdeck-preset-id"),
  launchdeckPresetLabel: document.getElementById("launchdeck-preset-label"),
  launchdeckPresetDevBuy: document.getElementById("launchdeck-preset-dev-buy"),
  launchdeckPresetCreationProvider: document.getElementById("launchdeck-preset-creation-provider"),
  launchdeckPresetCreationMev: document.getElementById("launchdeck-preset-creation-mev"),
  launchdeckPresetCreationFee: document.getElementById("launchdeck-preset-creation-fee"),
  launchdeckPresetCreationTip: document.getElementById("launchdeck-preset-creation-tip"),
  launchdeckPresetCreationAutoFee: document.getElementById("launchdeck-preset-creation-auto-fee"),
  launchdeckPresetCreationMaxFee: document.getElementById("launchdeck-preset-creation-max-fee"),
  launchdeckPresetBuyAmounts: Array.from(document.querySelectorAll("[data-launchdeck-buy-amount-input]")),
  launchdeckPresetBuyProvider: document.getElementById("launchdeck-preset-buy-provider"),
  launchdeckPresetBuyFee: document.getElementById("launchdeck-preset-buy-fee"),
  launchdeckPresetBuyTip: document.getElementById("launchdeck-preset-buy-tip"),
  launchdeckPresetBuySlippage: document.getElementById("launchdeck-preset-buy-slippage"),
  launchdeckPresetBuyMev: document.getElementById("launchdeck-preset-buy-mev"),
  launchdeckPresetBuyAutoFee: document.getElementById("launchdeck-preset-buy-auto-fee"),
  launchdeckPresetBuyMaxFee: document.getElementById("launchdeck-preset-buy-max-fee"),
  launchdeckPresetSnipeBuy: document.getElementById("launchdeck-preset-snipe-buy"),
  launchdeckPresetSellPercents: Array.from(document.querySelectorAll("[data-launchdeck-sell-percent-input]")),
  launchdeckPresetSellProvider: document.getElementById("launchdeck-preset-sell-provider"),
  launchdeckPresetSellFee: document.getElementById("launchdeck-preset-sell-fee"),
  launchdeckPresetSellTip: document.getElementById("launchdeck-preset-sell-tip"),
  launchdeckPresetSellSlippage: document.getElementById("launchdeck-preset-sell-slippage"),
  launchdeckPresetSellMev: document.getElementById("launchdeck-preset-sell-mev"),
  launchdeckPresetSellAutoFee: document.getElementById("launchdeck-preset-sell-auto-fee"),
  launchdeckPresetSellMaxFee: document.getElementById("launchdeck-preset-sell-max-fee"),
  rewardsRefreshButton: document.getElementById("rewards-refresh-button"),
  rewardsTotalClaimable: document.getElementById("rewards-total-claimable"),
  rewardsTotalLegend: document.getElementById("rewards-total-legend"),
  rewardsLastRefreshed: document.getElementById("rewards-last-refreshed"),
  rewardsStatus: document.getElementById("rewards-status"),
  rewardsProviderGrid: document.getElementById("rewards-provider-grid"),
  appearanceSoundControls: {
    buy: buildAppearanceSoundElements("buy"),
    sell: buildAppearanceSoundElements("sell")
  },
  appearanceVolume: {
    slider: document.getElementById("appearance-sound-volume-slider"),
    input: document.getElementById("appearance-sound-volume-input"),
    value: document.getElementById("appearance-sound-volume-value")
  }
};

function buildAppearanceSoundElements(side) {
  const prefix = `appearance-${side}-sound`;
  return {
    side,
    enabled: document.getElementById(`${prefix}-enabled`),
    template: document.getElementById(`${prefix}-template`),
    preview: document.getElementById(`${prefix}-preview`),
    customRow: document.getElementById(`${prefix}-custom-row`),
    customName: document.getElementById(`${prefix}-custom-name`),
    customClear: document.getElementById(`${prefix}-custom-clear`),
    customInput: document.getElementById(`${prefix}-custom-input`),
    status: document.getElementById(`${prefix}-status`)
  };
}

const PROVIDER_LABELS = {
  "helius-sender": "Helius Sender",
  hellomoon: "Hello Moon",
  "standard-rpc": "Standard RPC",
  "jito-bundle": "Jito Bundle"
};

const EXTENSION_APP_ROUTE_PROVIDERS = new Set(["helius-sender", "hellomoon"]);

function selectableExtensionRouteProvider(provider, fallback = "helius-sender") {
  const normalized = String(provider || "").trim().toLowerCase();
  return EXTENSION_APP_ROUTE_PROVIDERS.has(normalized) ? normalized : fallback;
}

// Mirrors the per-provider fee floors from the LaunchDeck settings domain:
// Helius Sender requires a minimum 0.0002 SOL tip, Jito requires 1000 lamports
// (0.000001 SOL), Hello Moon requires 0.001 SOL, and all three require a non-zero priority fee. Standard
// RPC has no tip at all and no minimum.
const PROVIDER_FEE_REQUIREMENTS = {
  "helius-sender": { minTipSol: 0.0002, priorityRequired: true, tipApplicable: true },
  hellomoon: { minTipSol: 0.001, priorityRequired: true, tipApplicable: true },
  "jito-bundle": { minTipSol: 0.000001, priorityRequired: true, tipApplicable: true },
  "standard-rpc": { minTipSol: 0, priorityRequired: true, tipApplicable: false }
};
const DEFAULT_LAUNCHDECK_MANUAL_FEE_SOL = "0.001";
const LAMPORTS_PER_SOL = 1_000_000_000;
const MAX_TRANSACTION_DELAY_MS = 2000;
const REWARD_PROVIDER_META = {
  pumpCreator: {
    title: "Pump.fun",
    label: "Pump",
    icon: "P",
    iconUrl: "../../images/pump-mark.png"
  },
  pumpCashback: {
    title: "Pump.fun cashback",
    label: "Cashback",
    icon: "C",
    iconUrl: "../../images/pump-mark.png"
  },
  bagsCreator: {
    title: "Bags",
    label: "Bags",
    icon: "B",
    iconUrl: "../../images/bagsapp-mark.png"
  }
};

function providerKey(provider) {
  return String(provider || "").trim().toLowerCase();
}

function providerLabel(provider) {
  const key = providerKey(provider);
  return PROVIDER_LABELS[key] || key || "selected provider";
}

function providerAvailability() {
  const providers = state.bootstrap?.providers;
  return providers && typeof providers === "object" ? providers : {};
}

function applyRouteProviderAvailability(select) {
  if (!select) return false;
  const providers = providerAvailability();
  const previous = select.value;
  Array.from(select.options).forEach((option) => {
    const key = providerKey(option.value);
    if (!EXTENSION_APP_ROUTE_PROVIDERS.has(key)) return;
    const entry = providers[key];
    const unavailable = Boolean(entry && entry.available === false);
    option.disabled = unavailable;
    option.textContent = PROVIDER_LABELS[key] || option.textContent;
    option.title = unavailable && entry.reason ? entry.reason : "";
  });
  if (select.selectedOptions[0]?.disabled) {
    const fallback = Array.from(select.options).find((option) =>
      EXTENSION_APP_ROUTE_PROVIDERS.has(providerKey(option.value)) && !option.disabled
    );
    if (fallback) {
      select.value = fallback.value;
    }
  }
  return select.value !== previous;
}

function applyExtensionRouteProviderAvailability() {
  [
    elements.presetModalBuyProvider,
    elements.presetModalSellProvider,
    elements.launchdeckPresetCreationProvider,
    elements.launchdeckPresetBuyProvider,
    elements.launchdeckPresetSellProvider
  ].forEach((select) => applyRouteProviderAvailability(select));
}

function providerFeeRequirements(provider) {
  return PROVIDER_FEE_REQUIREMENTS[providerKey(provider)] || null;
}

function providerMinimumTipSol(provider) {
  const req = providerFeeRequirements(provider);
  return req ? Number(req.minTipSol || 0) : 0;
}

function providerRequiresPriorityFee(provider) {
  const req = providerFeeRequirements(provider);
  return Boolean(req && req.priorityRequired);
}

function providerSupportsTip(provider) {
  const req = providerFeeRequirements(provider);
  return req ? Boolean(req.tipApplicable) : true;
}

function defaultPriorityFeeForProvider(provider) {
  return providerRequiresPriorityFee(provider) ? DEFAULT_LAUNCHDECK_MANUAL_FEE_SOL : "";
}

function defaultTipForProvider(provider) {
  return providerSupportsTip(provider) ? DEFAULT_LAUNCHDECK_MANUAL_FEE_SOL : "";
}

function normalizePriorityFeeForProvider(provider, value) {
  const trimmed = String(value || "").trim();
  if (!providerRequiresPriorityFee(provider)) return trimmed;
  return trimmed || defaultPriorityFeeForProvider(provider);
}

function normalizeTipForProvider(provider, value) {
  if (!providerSupportsTip(provider)) return "";
  const trimmed = String(value || "").trim();
  return trimmed || defaultTipForProvider(provider);
}

function formatMinSol(value) {
  if (!Number.isFinite(value) || value <= 0) return "0";
  const fixed = value.toFixed(4);
  return fixed.replace(/\.?0+$/, "") || fixed;
}

function clearFieldInvalid(input) {
  if (!input) return;
  input.classList.remove("is-invalid");
}

function markFieldsInvalid(errors) {
  document.querySelectorAll(".is-invalid").forEach((el) => el.classList.remove("is-invalid"));
  errors.forEach((err) => {
    if (!err || !err.fieldId) return;
    const el = document.getElementById(err.fieldId);
    if (!el) return;
    el.classList.add("is-invalid");
    if (!el.dataset.invalidListenerBound) {
      const clear = () => clearFieldInvalid(el);
      el.addEventListener("input", clear);
      el.addEventListener("change", clear);
      el.dataset.invalidListenerBound = "true";
    }
  });
  const firstValid = errors.find((err) => err && err.fieldId);
  if (firstValid) {
    const first = document.getElementById(firstValid.fieldId);
    if (first && typeof first.focus === "function") {
      try {
        first.focus({ preventScroll: false });
      } catch {
        first.focus();
      }
    }
  }
}

function summarizeValidationErrors(errors) {
  if (!errors.length) return "";
  if (errors.length === 1) return errors[0].message;
  const plural = errors.length === 1 ? "field" : "fields";
  return `${errors.length} ${plural} need attention: ${errors[0].message}`;
}

function showOptionsToast(title, message = "", { type = "info", timeoutMs = null } = {}) {
  const host = document.getElementById("options-toast-host");
  if (!host) return;
  const resolvedTimeoutMs = Number.isFinite(timeoutMs) ? timeoutMs : (type === "error" ? 4200 : 3200);
  const toast = document.createElement("div");
  toast.className = `options-toast is-${type}`;
  toast.setAttribute("role", type === "error" ? "alert" : "status");
  const indicator = document.createElement("span");
  indicator.className = "options-toast-icon";
  indicator.setAttribute("aria-hidden", "true");
  indicator.innerHTML = optionsToastIconMarkup(type);
  const body = document.createElement("div");
  body.className = "options-toast-body";
  if (title) {
    const titleEl = document.createElement("span");
    titleEl.className = "options-toast-title";
    titleEl.textContent = title;
    body.appendChild(titleEl);
  }
  if (message) {
    const messageEl = document.createElement("span");
    messageEl.className = "options-toast-message";
    messageEl.textContent = message;
    body.appendChild(messageEl);
  }
  toast.appendChild(indicator);
  toast.appendChild(body);
  host.prepend(toast);
  const dismiss = () => {
    if (!toast.isConnected) return;
    toast.classList.add("is-leaving");
    toast.addEventListener("animationend", () => toast.remove(), { once: true });
  };
  toast.addEventListener("click", dismiss);
  setTimeout(dismiss, resolvedTimeoutMs);
}

function optionsToastIconMarkup(type) {
  if (type === "success") {
    return `<img src="${OPTIONS_TOAST_SUCCESS_ICON_URL}" alt="" aria-hidden="true" />`;
  }
  if (type === "error") {
    return `<img src="${OPTIONS_TOAST_FAIL_ICON_URL}" alt="" aria-hidden="true" />`;
  }
  return '<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M12 22C6.477 22 2 17.523 2 12S6.477 2 12 2s10 4.477 10 10-4.477 10-10 10zm0-2a8 8 0 1 0 0-16 8 8 0 0 0 0 16zm-1-11h2v2h-2V7zm0 4h2v6h-2v-6z"/></svg>';
}

function ensureRuntimeDiagnosticsPanel() {
  let panel = document.getElementById("runtime-diagnostics-notice");
  if (panel) return panel;
  const stack = document.querySelector(".global-settings-stack");
  if (!stack) return null;
  panel = document.createElement("section");
  panel.id = "runtime-diagnostics-notice";
  panel.className = "settings-panel connection-panel connection-panel-warning";
  panel.hidden = true;
  panel.innerHTML = `
    <div class="panel-heading">
      <div>
        <span class="eyebrow">Runtime diagnostics</span>
        <h3>Endpoint warnings</h3>
        <p class="muted" data-runtime-diagnostics-summary></p>
      </div>
    </div>
    <div class="runtime-diagnostics-list" data-runtime-diagnostics-list></div>
  `;
  stack.prepend(panel);
  return panel;
}

function runtimeDiagnosticKind(diagnostic) {
  return String(diagnostic?.severity || "").toLowerCase() === "critical" ? "error" : "info";
}

function runtimeDiagnosticDetail(diagnostic) {
  const parts = [];
  if (diagnostic?.envVar) parts.push(diagnostic.envVar);
  if (diagnostic?.host) parts.push(diagnostic.host);
  if (diagnostic?.restartRequired) parts.push("Restart required after changing env-backed values.");
  if (diagnostic?.detail) parts.push(diagnostic.detail);
  return parts.join(" ");
}

function runtimeDiagnosticToastKey(diagnostic) {
  const fingerprint = String(diagnostic?.fingerprint || diagnostic?.key || "").trim();
  if (!fingerprint) return "";
  const signature = [
    diagnostic?.severity || "",
    diagnostic?.source || "",
    diagnostic?.code || "",
    diagnostic?.message || "",
    diagnostic?.detail || "",
    diagnostic?.envVar || "",
    diagnostic?.endpointKind || "",
    diagnostic?.host || "",
    diagnostic?.restartRequired ? "restart" : ""
  ].map((part) => String(part || "").trim()).join("|");
  return `${fingerprint}:${signature}`;
}

async function dismissRuntimeDiagnostic(fingerprint) {
  try {
    const result = await callBackground("trench:dismiss-runtime-diagnostic", { fingerprint });
    if (result?.ok === false) {
      throw new Error(result.error || "Diagnostic could not be dismissed.");
    }
    showOptionsToast("Diagnostic dismissed", "This exact warning stays hidden until it recovers or changes.", {
      type: "info"
    });
  } catch (error) {
    showOptionsToast("Dismiss failed", error?.message || "Diagnostic could not be dismissed.", {
      type: "error"
    });
  }
}

async function disableWarmEndpoint(envVar) {
  const normalized = String(envVar || "").trim().toUpperCase();
  if (normalized !== "WARM_RPC_URL" && normalized !== "WARM_WS_URL") return;
  if (state.engineSettingsDirty) {
    showOptionsToast(
      "Save engine settings first",
      `Save or discard your current edits before disabling ${normalized}.`,
      { type: "error" }
    );
    return;
  }
  const confirmed = window.confirm(
    `Disable ${normalized}? This clears only ${normalized}. Restart local services if the running engine already loaded it.`
  );
  if (!confirmed) return;
  try {
    const latest = normalizeSettings(await callBackground("trench:get-settings"));
    if (normalized === "WARM_RPC_URL") latest.warmRpcUrl = "";
    if (normalized === "WARM_WS_URL") latest.warmWsUrl = "";
    const saved = await callBackground("trench:save-settings", latest);
    await bumpBootstrapRevision();
    state.settings = normalizeSettings(saved);
    renderEngineSettings();
    setEngineSettingsDirty(false);
    showOptionsToast(`${normalized} disabled`, "Restart local services if the warning remains active.", {
      type: "success"
    });
  } catch (error) {
    showOptionsToast(`${normalized} not disabled`, error?.message || "Engine settings could not be saved.", {
      type: "error"
    });
  }
}

function renderRuntimeDiagnostics(snapshot = null) {
  const panel = ensureRuntimeDiagnosticsPanel();
  if (!panel) return;
  const diagnostics = Array.isArray(snapshot?.diagnostics) ? snapshot.diagnostics : [];
  const dismissed = snapshot?.dismissed && typeof snapshot.dismissed === "object"
    ? snapshot.dismissed
    : {};
  const active = diagnostics
    .filter((entry) => entry && entry.active !== false)
    .filter((entry) => !dismissed[entry.fingerprint || entry.key]);
  const activeKeys = new Set(active.map(runtimeDiagnosticToastKey).filter(Boolean));
  for (const key of Array.from(state.runtimeDiagnosticToastKeys)) {
    if (!activeKeys.has(key)) state.runtimeDiagnosticToastKeys.delete(key);
  }
  panel.hidden = active.length === 0;
  const summary = panel.querySelector("[data-runtime-diagnostics-summary]");
  const list = panel.querySelector("[data-runtime-diagnostics-list]");
  if (summary) {
    summary.textContent = active.length
      ? "Warnings are sanitized and only show endpoint kind, env var name, host, and action needed."
      : "";
  }
  if (!list) return;
  list.innerHTML = "";
  for (const diagnostic of active) {
    const row = document.createElement("div");
    row.className = "runtime-diagnostic-row";
    const title = document.createElement("strong");
    title.textContent = diagnostic.message || "Runtime diagnostic";
    const detail = document.createElement("p");
    detail.className = "muted";
    detail.textContent = runtimeDiagnosticDetail(diagnostic);
    const actions = document.createElement("div");
    actions.className = "action-row compact";
    const dismiss = document.createElement("button");
    dismiss.type = "button";
    dismiss.className = "secondary-button";
    dismiss.textContent = "Dismiss";
    dismiss.addEventListener("click", () => {
      void dismissRuntimeDiagnostic(diagnostic.fingerprint || diagnostic.key);
    });
    actions.appendChild(dismiss);
    if (diagnostic.envVar === "WARM_RPC_URL" || diagnostic.envVar === "WARM_WS_URL") {
      const disable = document.createElement("button");
      disable.type = "button";
      disable.className = "secondary-button";
      disable.textContent = diagnostic.envVar === "WARM_RPC_URL" ? "Disable Warm RPC" : "Disable Warm WS";
      disable.addEventListener("click", () => void disableWarmEndpoint(diagnostic.envVar));
      actions.appendChild(disable);
    }
    row.appendChild(title);
    row.appendChild(detail);
    row.appendChild(actions);
    list.appendChild(row);
    const fingerprint = String(diagnostic.fingerprint || diagnostic.key || "").trim();
    const toastKey = runtimeDiagnosticToastKey(diagnostic);
    if (fingerprint && toastKey && !dismissed[fingerprint] && !state.runtimeDiagnosticToastKeys.has(toastKey)) {
      state.runtimeDiagnosticToastKeys.add(toastKey);
      showOptionsToast(diagnostic.message || "Runtime diagnostic", runtimeDiagnosticDetail(diagnostic), {
        type: runtimeDiagnosticKind(diagnostic),
        timeoutMs: 5000
      });
    }
  }
}

function lockField(element) {
  if (!element) return;
  if (
    element.tagName === "SELECT" ||
    element.type === "checkbox" ||
    element.type === "radio"
  ) {
    element.disabled = true;
  } else {
    element.readOnly = true;
  }
  const wrapper = element.closest(".locked-field");
  const button = wrapper?.querySelector(".lock-toggle");
  if (button) {
    button.textContent = "🔒";
    button.classList.remove("is-unlocked");
    button.setAttribute("aria-label", "Unlock field");
    button.setAttribute("title", "Unlock to edit");
  }
}

function unlockField(element) {
  if (!element) return;
  if (
    element.tagName === "SELECT" ||
    element.type === "checkbox" ||
    element.type === "radio"
  ) {
    element.disabled = false;
  } else {
    element.readOnly = false;
  }
  const wrapper = element.closest(".locked-field");
  const button = wrapper?.querySelector(".lock-toggle");
  if (button) {
    button.textContent = "🔓";
    button.classList.add("is-unlocked");
    button.setAttribute("aria-label", "Lock field");
    button.setAttribute("title", "Lock field");
  }
  try {
    element.focus();
    if (typeof element.select === "function" && element.tagName !== "SELECT") {
      element.select();
    }
  } catch (_error) {
    /* focus may fail for hidden inputs */
  }
}

function wireLockedField(element) {
  if (!element || element.dataset.lockedFieldBound === "true") return;
  const parent = element.parentElement;
  if (!parent) return;
  const wrapper = document.createElement("div");
  wrapper.className = "locked-field";
  parent.insertBefore(wrapper, element);
  wrapper.appendChild(element);
  const button = document.createElement("button");
  button.type = "button";
  button.className = "lock-toggle";
  button.setAttribute("aria-label", "Unlock field");
  button.setAttribute("title", "Unlock to edit");
  button.textContent = "🔒";
  wrapper.appendChild(button);
  element.dataset.lockedFieldBound = "true";
  lockField(element);
  button.addEventListener("click", (event) => {
    event.preventDefault();
    if (button.classList.contains("is-unlocked")) {
      lockField(element);
    } else {
      unlockField(element);
    }
  });
  element.addEventListener("blur", () => {
    setTimeout(() => {
      if (document.activeElement === element) return;
      lockField(element);
    }, 150);
  });
}

function relockGlobalSettingsFields() {
  document
    .querySelectorAll(
      ".locked-field > input, .locked-field > textarea, .locked-field > select"
    )
    .forEach((field) => lockField(field));
}

const ENGINE_SETTINGS_DIRTY_FIELD_IDS = [
  "engine-rpc-url",
  "engine-ws-url",
  "engine-warm-rpc-url",
  "engine-warm-ws-url",
  "engine-endpoint-profile",
  "engine-endpoint-profile-custom",
  "engine-pnl-tracking-mode",
  "engine-pnl-include-fees",
  "engine-wrapper-default-fee-bps",
  "engine-max-active-batches",
  "engine-helius-rpc-url",
  "engine-helius-ws-url",
  "engine-standard-rpc-send-urls",
  "engine-allow-non-canonical-pool-trades"
];

function setEngineSettingsDirty(isDirty) {
  state.engineSettingsDirty = Boolean(isDirty);
  elements.engineSettingsDirtyBadge?.classList.toggle("hidden", !state.engineSettingsDirty);
  elements.saveEngineSettingsButton?.classList.toggle(
    "has-unsaved-changes",
    state.engineSettingsDirty
  );
}

function wireEngineSettingsDirtyFields() {
  for (const id of ENGINE_SETTINGS_DIRTY_FIELD_IDS) {
    const element = document.getElementById(id);
    if (!element || element.dataset.engineDirtyBound === "true") continue;
    const markDirty = () => setEngineSettingsDirty(true);
    element.addEventListener("input", markDirty);
    element.addEventListener("change", markDirty);
    element.dataset.engineDirtyBound = "true";
  }
}

function wireGlobalSettingsLockedFields() {
  const ids = [
    "host-auth-token-input",
    "engine-rpc-url",
    "engine-ws-url",
    "engine-warm-rpc-url",
    "engine-warm-ws-url",
    "engine-endpoint-profile",
    "engine-endpoint-profile-custom",
    "engine-pnl-tracking-mode",
    "engine-pnl-include-fees",
    "engine-wrapper-default-fee-bps",
    "engine-max-active-batches",
    "engine-helius-rpc-url",
    "engine-helius-ws-url",
    "engine-standard-rpc-send-urls",
    "engine-allow-non-canonical-pool-trades"
  ];
  for (const id of ids) {
    const element = document.getElementById(id);
    if (element) wireLockedField(element);
  }
  wireEngineSettingsDirtyFields();
}

for (const button of elements.navButtons) {
  button.addEventListener("click", () => {
    state.activeSection = button.dataset.sectionTarget;
    renderSections();
    if (state.activeSection === "wallets") {
      void refreshWalletBalances();
    } else if (state.activeSection === "rewards") {
      void refreshRewardsSummary();
    }
  });
}

elements.saveHostButton.addEventListener("click", async () => {
  const nextHost = await getHostBase();
  const permissionGranted = await ensureHostPermission(nextHost);
  if (!permissionGranted) {
    showOptionsToast("Permission denied", `Remote host permission was not granted for ${nextHost}.`, {
      type: "error"
    });
    return;
  }
  const saved = await setHostBase(nextHost);
  await setHostAuthToken(elements.hostAuthTokenInput.value);
  elements.hostInput.value = saved;
  elements.hostAuthTokenInput.value = await getHostAuthToken();
  relockGlobalSettingsFields();
  showOptionsToast(
    "Token saved",
    `Using local execution host ${saved}.`,
    { type: "success" }
  );
  await refreshEngineData();
});

elements.testHostButton.addEventListener("click", async () => {
  if (await warnIfExecutionConnectionDraft()) {
    return;
  }
  await refreshExecutionConnection();
});

elements.testLaunchdeckHostButton.addEventListener("click", async () => {
  if (await warnIfLaunchdeckConnectionDraft()) {
    return;
  }
  await refreshLaunchdeckConnection();
});

elements.reloadHostButton.addEventListener("click", async () => {
  await refreshEngineData();
});

elements.addExecutionPresetButton.addEventListener("click", () => openPresetModal());
elements.addLaunchdeckPresetButton?.addEventListener("click", () => openLaunchdeckPresetModal());
elements.addWalletButton.addEventListener("click", () => openWalletEditModal(null));
elements.openCreateGroupModalButton.addEventListener("click", () => openCreateGroupModal(null));
elements.saveEngineSettingsButton.addEventListener("click", () => void persistEngineSettings());
elements.rewardsRefreshButton?.addEventListener("click", () => void refreshRewardsSummary({ force: true }));
elements.rewardsProviderGrid?.addEventListener("click", (event) => {
  const target = event.target instanceof Element ? event.target : null;
  if (!target) return;
  if (target.closest("[data-rewards-refresh]")) {
    void refreshRewardsSummary({ force: true });
    return;
  }
  const rowButton = target.closest("[data-rewards-claim-row]");
  if (rowButton) {
    const providerId = rowButton.getAttribute("data-rewards-claim-provider") || "";
    const rowId = rowButton.getAttribute("data-rewards-claim-row") || "";
    const provider = state.rewards.providers.find((entry) => entry.providerId === providerId);
    const row = provider?.rows.find((entry) => entry.id === rowId);
    void claimRewardsForRows(row ? [row] : [], providerId);
    return;
  }
  const providerButton = target.closest("[data-rewards-claim-provider]");
  if (providerButton) {
    const providerId = providerButton.getAttribute("data-rewards-claim-provider") || "";
    const provider = state.rewards.providers.find((entry) => entry.providerId === providerId);
    void claimRewardsForRows(provider?.rows || [], providerId);
  }
});
elements.exportPnlHistoryButton?.addEventListener("click", async () => {
  try {
    showOptionsToast("Exporting", "Preparing PnL history export...", { type: "info" });
    const response = await callBackground("trench:export-pnl-history");
    const bytes = base64ToUint8Array(String(response?.zipBase64 || ""));
    triggerBrowserDownload(bytes, String(response?.filename || "trench-tools-pnl-history.zip"));
    showOptionsToast("Export ready", "PnL history export downloaded.", { type: "success" });
  } catch (error) {
    showOptionsToast("Export failed", error.message, { type: "error" });
  }
});
elements.wipePnlHistoryButton?.addEventListener("click", async () => {
  const confirmed = window.confirm(
    "Wipe all local PnL/history data stored by the execution engine on this machine?"
  );
  if (!confirmed) {
    return;
  }
  try {
    showOptionsToast("Wiping", "Wiping local PnL history...", { type: "info" });
    await callBackground("trench:wipe-pnl-history");
    await Promise.all([
      bumpBootstrapRevision(),
      bumpWalletStatusRevision()
    ]);
    showOptionsToast("History wiped", "Local PnL history wiped.", { type: "success" });
  } catch (error) {
    showOptionsToast("Wipe failed", error.message, { type: "error" });
  }
});

if (elements.engineEndpointProfile) {
  elements.engineEndpointProfile.addEventListener("change", () => {
    if (elements.engineEndpointProfile.value === "custom") {
      elements.engineEndpointProfileCustomRow?.classList.remove("hidden");
      elements.engineEndpointProfileCustom?.focus();
    } else {
      elements.engineEndpointProfileCustomRow?.classList.add("hidden");
      if (elements.engineEndpointProfileCustom) {
        elements.engineEndpointProfileCustom.value = "";
      }
    }
  });
}

elements.buyDistributionSplit.addEventListener("click", () => void setDefaultDistributionMode("split"));
elements.buyDistributionEach.addEventListener("click", () => void setDefaultDistributionMode("each"));

elements.walletEditModalClose.addEventListener("click", closeWalletEditModal);
elements.walletEditModalCancel.addEventListener("click", closeWalletEditModal);
elements.walletEditModalSave.addEventListener("click", () => void saveWalletEditModal());
elements.walletEditModalDelete.addEventListener("click", () => void deleteWalletFromEditModal());
attachModalBackdropDismiss(elements.walletEditModal, closeWalletEditModal);
elements.walletEditEmojiButton.addEventListener("click", (event) => {
  event.stopPropagation();
  toggleWalletEditEmojiPopover();
});
elements.walletEditEmojiPopover.addEventListener("click", (event) => {
  event.stopPropagation();
  const target = event.target instanceof Element ? event.target : null;
  if (!target) {
    return;
  }
  const tab = target.closest("[data-emoji-category]");
  if (tab) {
    state.walletEditModal.emojiCategoryId = tab.dataset.emojiCategory || EMOJI_CATALOG[0].id;
    renderWalletEditEmojiPopover();
    return;
  }
  const choice = target.closest("[data-emoji]");
  if (!choice) {
    return;
  }
  setWalletEditEmoji(choice.dataset.emoji || "");
  closeWalletEditEmojiPopover();
});
document.addEventListener("click", (event) => {
  if (!state.walletEditModal.emojiPopoverOpen) {
    return;
  }
  const target = event.target instanceof Element ? event.target : null;
  if (!target) {
    return;
  }
  if (target.closest("#wallet-edit-emoji-popover") || target.closest("#wallet-edit-emoji-button")) {
    return;
  }
  closeWalletEditEmojiPopover();
});

elements.createGroupModalClose.addEventListener("click", closeCreateGroupModal);
elements.createGroupModalCancel.addEventListener("click", closeCreateGroupModal);
elements.createGroupModalSave.addEventListener("click", () => void saveCreateGroupModal());
elements.createGroupModalDelete.addEventListener("click", () => void deleteGroupFromCreateModal());
attachModalBackdropDismiss(elements.createGroupModal, closeCreateGroupModal);
elements.createGroupNameInput.addEventListener("input", () => {
  state.createGroupModal.name = elements.createGroupNameInput.value;
  if (!state.createGroupModal.emoji) {
    renderCreateGroupEmojiButton();
  }
});
elements.createGroupEmojiButton.addEventListener("click", (event) => {
  event.stopPropagation();
  toggleCreateGroupEmojiPopover();
});
elements.createGroupEmojiPopover.addEventListener("click", (event) => {
  event.stopPropagation();
  const target = event.target instanceof Element ? event.target : null;
  if (!target) {
    return;
  }
  const tab = target.closest("[data-emoji-category]");
  if (tab) {
    state.createGroupModal.emojiCategoryId = tab.dataset.emojiCategory || EMOJI_CATALOG[0].id;
    renderCreateGroupEmojiPopover();
    return;
  }
  const choice = target.closest("[data-emoji]");
  if (!choice) {
    return;
  }
  setCreateGroupEmoji(choice.dataset.emoji || "");
  closeCreateGroupEmojiPopover();
});
document.addEventListener("click", (event) => {
  if (!state.createGroupModal.emojiPopoverOpen) {
    return;
  }
  const target = event.target instanceof Element ? event.target : null;
  if (!target) {
    return;
  }
  if (target.closest("#create-group-emoji-popover") || target.closest("#create-group-emoji-button")) {
    return;
  }
  closeCreateGroupEmojiPopover();
});
elements.createGroupVarianceToggle.addEventListener("change", () => {
  state.createGroupModal.varianceEnabled = elements.createGroupVarianceToggle.checked;
  syncCreateGroupModalVarianceVisibility();
});
elements.createGroupVarianceSlider.addEventListener("input", () => {
  state.createGroupModal.variancePercent = clampInt(elements.createGroupVarianceSlider.value, 0, 100);
  elements.createGroupVarianceInput.value = String(state.createGroupModal.variancePercent);
});
elements.createGroupVarianceInput.addEventListener("input", () => {
  state.createGroupModal.variancePercent = clampInt(elements.createGroupVarianceInput.value, 0, 100);
  elements.createGroupVarianceSlider.value = String(state.createGroupModal.variancePercent);
});
elements.createGroupStaggerToggle.addEventListener("change", () => {
  state.createGroupModal.staggerEnabled = elements.createGroupStaggerToggle.checked;
  syncCreateGroupModalStaggerVisibility();
});
for (const button of elements.createGroupStaggerStrategyButtons) {
  button.addEventListener("click", () => {
    state.createGroupModal.staggerStrategy =
      button.dataset.staggerStrategy === "random" ? "random" : "fixed";
    syncCreateGroupModalStaggerVisibility();
  });
}
elements.createGroupStaggerMs.addEventListener("input", () => {
  state.createGroupModal.staggerMs = clampInt(elements.createGroupStaggerMs.value, 0, 2000);
});
elements.createGroupStaggerMinMs.addEventListener("input", () => {
  state.createGroupModal.staggerMinMs = clampInt(elements.createGroupStaggerMinMs.value, 0, 2000);
});
elements.createGroupStaggerMaxMs.addEventListener("input", () => {
  state.createGroupModal.staggerMaxMs = clampInt(elements.createGroupStaggerMaxMs.value, 0, 2000);
});

elements.presetModalClose.addEventListener("click", closePresetModal);
elements.presetModalCancel.addEventListener("click", closePresetModal);
elements.presetModalSave.addEventListener("click", () => void savePresetFromModal());
elements.presetModalLabel.addEventListener("input", () => {
  // Preset keys are now derived from the name automatically and never shown
  // to the user. On edit the id stays locked to the existing preset.
  if (elements.presetModalId.dataset.locked === "true") {
    return;
  }
  elements.presetModalId.value = slugifyKey(elements.presetModalLabel.value, "preset");
});
attachModalBackdropDismiss(elements.presetModal, closePresetModal);
document.querySelectorAll("[data-buy-amount-add]").forEach((button) => {
  button.addEventListener("click", () => {
    setBuyAmountRowsVisible(2);
    focusFirstSecondRowInput();
  });
});
document.querySelectorAll("[data-buy-amount-remove]").forEach((button) => {
  button.addEventListener("click", () => {
    clearSecondBuyAmountRowInputs();
    setBuyAmountRowsVisible(1);
  });
});
document.querySelectorAll("[data-sell-percent-add]").forEach((button) => {
  button.addEventListener("click", () => {
    setSellPercentRowsVisible(2);
    focusFirstSecondSellRowInput();
  });
});
document.querySelectorAll("[data-sell-percent-remove]").forEach((button) => {
  button.addEventListener("click", () => {
    clearSecondSellPercentRowInputs();
    setSellPercentRowsVisible(1);
  });
});
elements.launchdeckPresetModalClose.addEventListener("click", closeLaunchdeckPresetModal);
elements.launchdeckPresetModalDelete?.addEventListener("click", () => void deleteLaunchdeckPresetFromModal());
elements.launchdeckPresetModalCancel.addEventListener("click", closeLaunchdeckPresetModal);
elements.launchdeckPresetModalSave.addEventListener("click", () => void saveLaunchdeckPresetFromModal());
attachModalBackdropDismiss(elements.launchdeckPresetModal, closeLaunchdeckPresetModal);
bindAutoFeeFallbackLabels();
document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") {
    return;
  }
  if (state.presetModalOpen) {
    closePresetModal();
  }
  if (state.walletEditModal.open) {
    closeWalletEditModal();
  }
  if (state.createGroupModal.open) {
    closeCreateGroupModal();
  }
});

for (const input of [
  elements.siteAxiomEnabled,
  elements.siteAxiomLauncher,
  elements.siteAxiomInstantTrade,
  elements.siteAxiomInstantTradeButtonModeCount,
  elements.siteAxiomLaunchdeck,
  elements.siteAxiomPulseQb,
  elements.siteAxiomPulsePanel,
  elements.siteAxiomPulseVamp,
  elements.siteAxiomVampIconMode,
  elements.siteAxiomPulseVampMode,
  elements.siteAxiomDexScreenerIconMode,
  elements.siteAxiomWalletTracker,
  elements.siteAxiomWatchlist,
  elements.siteJ7Enabled
]) {
  if (!input) continue;
  input.addEventListener("change", () => {
    void persistSiteSettings();
  });
}

async function init() {
  state.activeSection = await consumeRequestedSection();
  elements.hostInput.value = await getHostBase();
  elements.hostAuthTokenInput.value = await getHostAuthToken();
  elements.launchdeckHostInput.value = await getLaunchdeckHostBase();
  state.siteFeatures = await getSiteFeatures();
  state.appearance = await getAppearance();
  wireGlobalSettingsLockedFields();
  renderSections();
  renderSiteSettings();
  renderAppearanceSettings();
  registerAppearanceHandlers();
  renderEngineSettings();
  await refreshAuthTokenMergeNotice();
  await refreshEngineData();
  void callBackground("trench:get-runtime-diagnostics").then(renderRuntimeDiagnostics).catch(() => {});
  setInterval(() => {
    void callBackground("trench:get-runtime-status").catch(() => {});
  }, 15000);
  chrome.storage.onChanged.addListener((changes, areaName) => {
    if (areaName !== "local") return;
    if (changes[WALLET_STATUS_REVISION_KEY]) {
      void refreshWalletBalances();
    }
    if (changes[APPEARANCE_STORAGE_KEY]) {
      state.appearance = normalizeAppearance(changes[APPEARANCE_STORAGE_KEY].newValue || {});
      renderAppearanceSettings();
    }
    if (changes[HOST_AUTH_TOKEN_MERGE_WARNING_KEY]) {
      // The service worker can set or clear this key mid-session (its one-shot
      // migration may run after the Options page is already open), so refresh
      // the banner without requiring a manual reload.
      void refreshAuthTokenMergeNotice();
    }
    if (changes[RUNTIME_DIAGNOSTICS_SNAPSHOT_KEY]) {
      renderRuntimeDiagnostics(changes[RUNTIME_DIAGNOSTICS_SNAPSHOT_KEY]?.newValue || null);
    } else if (changes[RUNTIME_DIAGNOSTICS_REVISION_KEY]) {
      void callBackground("trench:get-runtime-diagnostics").then(renderRuntimeDiagnostics).catch(() => {});
    }
  });
}

async function refreshAuthTokenMergeNotice() {
  const notice = elements.hostAuthTokenMergeNotice;
  const dismiss = elements.hostAuthTokenMergeNoticeDismiss;
  if (!notice || !dismiss) {
    return;
  }
  const stored = await chrome.storage.local.get(HOST_AUTH_TOKEN_MERGE_WARNING_KEY);
  const warning = stored[HOST_AUTH_TOKEN_MERGE_WARNING_KEY];
  if (warning && typeof warning === "object") {
    notice.hidden = false;
  } else {
    notice.hidden = true;
  }
  if (!dismiss.dataset.bound) {
    dismiss.dataset.bound = "true";
    dismiss.addEventListener("click", async () => {
      await chrome.storage.local.remove(HOST_AUTH_TOKEN_MERGE_WARNING_KEY);
      notice.hidden = true;
    });
  }
}

async function consumeRequestedSection() {
  const stored = await chrome.storage.local.get(OPTIONS_TARGET_SECTION_KEY);
  const requestedSection = String(stored[OPTIONS_TARGET_SECTION_KEY] || "").trim();
  await chrome.storage.local.remove(OPTIONS_TARGET_SECTION_KEY);
  const availableSections = new Set(elements.navButtons.map((button) => button.dataset.sectionTarget));
  return availableSections.has(requestedSection) ? requestedSection : "presets";
}

async function assertStoredHostPermission(baseUrl) {
  if (isLoopbackHost(baseUrl)) {
    return;
  }
  const originPattern = originPatternFromHostBase(baseUrl);
  const granted = await chrome.permissions.contains({ origins: [originPattern] });
  if (!granted) {
    throw new Error(
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
    throw new Error(`Configured ${label} URL is invalid.`);
  }
  if (parsed.protocol !== "https:") {
    throw new Error(`${label} must use HTTPS when it is not loopback.`);
  }
}

async function loadLaunchdeckConnection() {
  const baseUrl = await getLaunchdeckHostBase();
  const authToken = await getHostAuthToken();
  assertSecureTransport(baseUrl, "LaunchDeck host");
  await assertStoredHostPermission(baseUrl);
  if (!authToken) {
    throw new Error("LaunchDeck host requires the shared access token.");
  }
  return { baseUrl, authToken };
}

async function launchdeckRequest(path, options = {}) {
  const { baseUrl, authToken } = await loadLaunchdeckConnection();
  const headers = new Headers(options.headers || {});
  if (authToken) {
    if (!headers.has("authorization")) {
      headers.set("authorization", `Bearer ${authToken}`);
    }
  }
  if (options.body !== undefined && !headers.has("content-type")) {
    headers.set("content-type", "application/json");
  }
  const response = await fetch(new URL(path, baseUrl).toString(), {
    method: options.method || "GET",
    headers,
    body: options.body !== undefined ? JSON.stringify(options.body) : undefined,
    credentials: "omit",
    cache: "no-cache"
  });
  const contentType = response.headers.get("content-type") || "";
  const payload = contentType.includes("application/json")
    ? await response.json().catch(() => ({}))
    : await response.text().catch(() => "");
  if (!response.ok) {
    const message =
      typeof payload === "string"
        ? payload
        : payload?.error || `${response.status} ${response.statusText}`.trim();
    throw new Error(message || "LaunchDeck request failed.");
  }
  return payload;
}

async function fetchLaunchdeckHealth() {
  return launchdeckRequest("/api/runtime-status");
}

async function fetchLaunchdeckSettingsPayload() {
  return launchdeckRequest("/api/settings");
}

async function saveLaunchdeckConfig(config) {
  return launchdeckRequest("/api/settings", {
    method: "POST",
    body: { config }
  });
}

function launchdeckConfigNeedsProviderSanitization(config) {
  const items = Array.isArray(config?.presets?.items) ? config.presets.items : [];
  return items.some((preset) => {
    const providers = [
      preset?.creationSettings?.provider,
      preset?.buySettings?.provider,
      preset?.sellSettings?.provider
    ];
    return providers.some((provider) => {
      const normalized = String(provider || "").trim().toLowerCase();
      return normalized && !EXTENSION_APP_ROUTE_PROVIDERS.has(normalized);
    });
  });
}

async function persistSanitizedLaunchdeckConfig(config) {
  try {
    const payload = await saveLaunchdeckConfig(config);
    state.launchdeckSettingsPayload = payload;
    state.launchdeckConfig = normalizeLaunchdeckConfig(payload?.config || config);
    renderPresets();
  } catch (error) {
    showOptionsToast("LaunchDeck provider reset failed", error.message, { type: "error" });
  }
}

function applyLaunchdeckSettings(payload, error = null) {
  if (payload && typeof payload === "object") {
    const rawConfig = payload.config || state.launchdeckConfig;
    const normalizedConfig = normalizeLaunchdeckConfig(rawConfig);
    state.launchdeckSettingsPayload = payload;
    state.launchdeckConfig = normalizedConfig;
    elements.launchdeckConnectionStatus.textContent = formatLaunchdeckConnectionStatusMessage();
    if (launchdeckConfigNeedsProviderSanitization(rawConfig)) {
      void persistSanitizedLaunchdeckConfig(normalizedConfig);
    }
    return;
  }
  state.launchdeckSettingsPayload = null;
  state.launchdeckConfig = createDefaultLaunchdeckConfig();
  elements.launchdeckConnectionStatus.textContent = formatLaunchdeckConnectionStatusMessage(error);
}

async function refreshLaunchdeckConnection() {
  try {
    await fetchLaunchdeckHealth();
    const payload = await fetchLaunchdeckSettingsPayload();
    applyLaunchdeckSettings(payload, null);
  } catch (error) {
    applyLaunchdeckSettings(null, error);
  }
}

async function refreshExecutionConnection() {
  setStatus("Checking execution host...");
  state.authBootstrap = null;
  try {
    state.authBootstrap = await callBackground("trench:get-auth-bootstrap");
  } catch (_error) {
    state.authBootstrap = null;
  }
  try {
    state.health = await callBackground("trench:get-runtime-status");
    elements.connectionStatus.textContent = formatConnectionStatusMessage();
    setStatus(`Execution host reachable (${state.health?.runtimeMode || "ready"})`);
  } catch (error) {
    elements.connectionStatus.textContent = formatConnectionStatusMessage(error);
    setStatus(error.message, true);
  }
}

async function refreshEngineData() {
  // The options page is a combined control surface: execution-engine owns the
  // trading/settings state here, while LaunchDeck contributes its own preset
  // payload and connection status through the dedicated LaunchDeck host.
  setStatus("Checking execution and LaunchDeck hosts...");
  state.authBootstrap = null;
  const launchdeckSettingsPromise = fetchLaunchdeckSettingsPayload()
    .then((payload) => ({ payload, error: null }))
    .catch((error) => ({ payload: null, error }));
  try {
    state.authBootstrap = await callBackground("trench:get-auth-bootstrap");
  } catch (error) {
    state.authBootstrap = null;
  }
  try {
    const [health, bootstrap, settings, presets, wallets, walletGroups, authTokens] = await Promise.all([
      callBackground("trench:get-health"),
      callBackground("trench:get-bootstrap"),
      callBackground("trench:get-settings"),
      callBackground("trench:list-presets"),
      callBackground("trench:list-wallets"),
      callBackground("trench:list-wallet-groups"),
      callBackground("trench:list-auth-tokens")
    ]);
    const launchdeckSettings = await launchdeckSettingsPromise;
    state.health = health;
    state.bootstrap = normalizeBootstrap(bootstrap);
    state.settings = normalizeSettings(settings || bootstrap.settings || {});
    state.presets = presets.map((preset) => normalizePreset(preset));
    state.wallets = wallets.map((wallet) => normalizeWallet(wallet));
    state.walletGroups = walletGroups.map((group) => normalizeWalletGroup(group));
    state.authTokens = Array.isArray(authTokens) ? authTokens : [];
    applyLaunchdeckSettings(launchdeckSettings.payload, launchdeckSettings.error);
    setStatus(`Connected to ${health.engineVersion}`);
    elements.connectionStatus.textContent = formatConnectionStatusMessage();
    renderAll();
    void refreshWalletBalances();
  } catch (error) {
    const launchdeckSettings = await launchdeckSettingsPromise;
    // Keep the last successfully loaded state on refresh failures so a
    // completed save doesn't appear to revert back to defaults.
    applyLaunchdeckSettings(launchdeckSettings.payload, launchdeckSettings.error);
    setStatus(error.message, true);
    elements.connectionStatus.textContent = formatConnectionStatusMessage(error);
    renderAll();
  }
}

function renderAll() {
  renderSections();
  renderSiteSettings();
  renderEngineSettings();
  renderPresets();
  applyExtensionRouteProviderAvailability();
  renderWallets();
  renderGroups();
  renderRewards();
  renderBuyDistribution();
  syncBootstrapPreview();
}

async function refreshWalletBalances({ force = false } = {}) {
  try {
    const snapshot = await callBackground("trench:get-balances-snapshot", { force });
    applyBalancesSnapshot(snapshot);
  } catch (error) {
    // Swallow; balances are a nice-to-have.
  }
}

function applyBalancesSnapshot(snapshot) {
  const next = new Map();
  const entries = Array.isArray(snapshot?.balances) ? snapshot.balances : [];
  for (const entry of entries) {
    const key = entry?.envKey;
    if (!key) continue;
    next.set(key, {
      balanceSol:
        typeof entry.balanceSol === "number" && Number.isFinite(entry.balanceSol)
          ? entry.balanceSol
          : null,
      balanceLamports:
        typeof entry.balanceLamports === "number" && Number.isFinite(entry.balanceLamports)
          ? entry.balanceLamports
          : null,
      balanceError: typeof entry.balanceError === "string" ? entry.balanceError : null
    });
  }
  state.walletBalances = next;
  // Balance-only update: patch the existing chips in place instead of
  // tearing down every wallet card / picker row. A wholesale re-render would
  // destroy in-flight user interactions (e.g. clicking a checkbox in the
  // create-group modal) every time the SSE stream pushes a new balance.
  patchWalletBalanceChips();
}

async function refreshRewardsSummary({ force = false } = {}) {
  if (state.rewards.loading) return;
  state.rewards.loading = true;
  renderRewards();
  try {
    const walletKeys = state.wallets
      .filter((wallet) => wallet.enabled !== false)
      .map((wallet) => wallet.key)
      .filter(Boolean);
    const summary = await callBackground("trench:get-rewards-summary", {
      walletKeys,
      force: Boolean(force)
    });
    state.rewards.providers = normalizeRewardsProviders(summary?.providers);
    state.rewards.errors = Array.isArray(summary?.errors) ? summary.errors : [];
    state.rewards.lastRefreshedAt = Date.now();
  } catch (error) {
    state.rewards.errors = [{ message: error.message || "Failed to refresh rewards." }];
    showOptionsToast("Rewards refresh failed", error.message, { type: "error" });
  } finally {
    state.rewards.loading = false;
    renderRewards();
  }
}

function normalizeRewardsProviders(providers) {
  return (Array.isArray(providers) ? providers : []).map((provider) => ({
    providerId: String(provider?.providerId || provider?.id || "").trim(),
    provider: String(provider?.provider || "").trim(),
    rewardType: String(provider?.rewardType || "").trim(),
    title: String(provider?.title || "").trim(),
    claimableLamports: normalizeLamports(provider?.claimableLamports),
    claimedLamports: normalizeLamports(provider?.claimedLamports),
    positions: typeof provider?.positions === "number" && Number.isFinite(provider.positions)
      ? provider.positions
      : null,
    configured: provider?.configured !== false,
    reason: String(provider?.reason || "").trim(),
    rows: (Array.isArray(provider?.rows) ? provider.rows : []).map(normalizeRewardRow)
  }));
}

function normalizeRewardRow(row) {
  const amountLamports = normalizeLamports(row?.amountLamports);
  return {
    id: String(row?.id || "").trim(),
    providerId: String(row?.providerId || "").trim(),
    provider: String(row?.provider || "").trim(),
    rewardType: String(row?.rewardType || "").trim(),
    walletKey: String(row?.walletKey || "").trim(),
    walletPublicKey: String(row?.walletPublicKey || "").trim(),
    walletLabel: String(row?.walletLabel || "").trim(),
    mint: String(row?.mint || "").trim(),
    amountLamports,
    amountUi: Number(row?.amountUi || 0),
    claimable: row?.claimable === true && amountLamports > 0,
    configured: row?.configured !== false,
    reason: String(row?.reason || "").trim(),
    status: String(row?.status || "").trim()
  };
}

function normalizeLamports(value) {
  if (typeof value === "number" && Number.isFinite(value)) return Math.max(0, Math.floor(value));
  if (typeof value === "string" && value.trim()) {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return Math.max(0, Math.floor(parsed));
  }
  return 0;
}

function renderRewards() {
  if (!elements.rewardsProviderGrid) return;
  const providers = state.rewards.providers;
  const totalLamports = providers.reduce((sum, provider) => sum + providerClaimableLamports(provider), 0);
  elements.rewardsTotalClaimable.textContent = `${formatRewardSol(totalLamports)} SOL`;
  elements.rewardsLastRefreshed.textContent = state.rewards.lastRefreshedAt
    ? `Last refreshed ${new Date(state.rewards.lastRefreshedAt).toLocaleTimeString()}`
    : "Not refreshed yet.";
  elements.rewardsTotalLegend.innerHTML = providers.length
    ? providers.map((provider) => {
        const meta = rewardProviderMeta(provider.providerId);
        const markHtml = meta.iconUrl
          ? `<img class="rewards-legend-mark" src="${escapeAttribute(meta.iconUrl)}" alt="" aria-hidden="true" />`
          : `<span class="rewards-legend-dot" aria-hidden="true"></span>`;
        return `<span class="rewards-legend-item">${markHtml}<span class="rewards-legend-text">${escapeText(meta.label)} ${escapeText(formatRewardSol(providerClaimableLamports(provider)))}</span></span>`;
      }).join("")
    : "";
  elements.rewardsStatus.textContent = state.rewards.loading
    ? "Checking enabled wallets in parallel..."
    : rewardsStatusText();
  elements.rewardsRefreshButton.disabled = state.rewards.loading || state.rewards.claiming;
  elements.rewardsProviderGrid.innerHTML = providers.length
    ? providers.map(renderRewardProviderCard).join("")
    : renderRewardsEmptyState();
}

function rewardsStatusText() {
  if (state.rewards.errors.length) {
    return state.rewards.errors.map((error) => String(error?.message || error || "")).filter(Boolean).join(" · ");
  }
  if (!state.rewards.lastRefreshedAt) {
    return "Open this page or press Refresh to query enabled wallets.";
  }
  return "";
}

function renderRewardsEmptyState() {
  if (state.rewards.loading) {
    return `<section class="settings-panel"><p class="muted">Loading rewards...</p></section>`;
  }
  return `<section class="settings-panel"><p class="muted">No reward data yet. Press Refresh to check enabled wallets.</p></section>`;
}

function renderRewardProviderCard(provider) {
  const meta = rewardProviderMeta(provider.providerId);
  const rows = provider.rows;
  const claimableRows = rows.filter((row) => row.claimable);
  const subtitle = `${state.wallets.filter((wallet) => wallet.enabled !== false).length} wallets connected`;
  const iconHtml = meta.iconUrl
    ? `<span class="rewards-card-icon" aria-hidden="true"><img src="${escapeAttribute(meta.iconUrl)}" alt="" /></span>`
    : `<span class="rewards-card-icon" aria-hidden="true">${escapeText(meta.icon)}</span>`;
  const secondaryStatHtml = renderRewardSecondaryStat(provider);
  return `
    <section class="rewards-provider-card" data-rewards-provider="${escapeAttribute(provider.providerId)}">
      <div class="rewards-card-header">
        <div class="rewards-card-title-row">
          ${iconHtml}
          <div>
            <div class="rewards-card-title">${escapeText(provider.title || meta.title)}</div>
            <div class="rewards-card-subtitle">${escapeText(subtitle)}</div>
          </div>
        </div>
        <button class="secondary-button rewards-card-refresh icon-button" type="button" data-rewards-refresh title="Refresh rewards" aria-label="Refresh rewards">
          <img src="../../assets/refresh.png" alt="" aria-hidden="true" />
        </button>
      </div>
      <div class="rewards-card-stats">
        <div>
          <div class="rewards-card-kicker">Claimable</div>
          <div class="rewards-stat-value">${escapeText(formatRewardSol(providerClaimableLamports(provider)))}</div>
        </div>
        ${secondaryStatHtml}
      </div>
      <div class="rewards-wallet-section-title">By wallet</div>
      <div class="rewards-wallet-list">
        ${rows.length ? rows.map((row) => renderRewardWalletRow(provider, row)).join("") : `<p class="muted">No claimable wallets found.</p>`}
      </div>
      <button class="primary-button rewards-claim-all-button" type="button" data-rewards-claim-provider="${escapeAttribute(provider.providerId)}" ${claimableRows.length && !state.rewards.claiming ? "" : "disabled"}>
        Claim All (${claimableRows.length} ${claimableRows.length === 1 ? "wallet" : "wallets"})
      </button>
      ${provider.reason ? `<p class="muted">${escapeText(provider.reason)}</p>` : ""}
    </section>
  `;
}

function renderRewardSecondaryStat(provider) {
  if (provider.positions !== null) {
    return `
      <div>
        <div class="rewards-card-kicker">Positions</div>
        <div class="rewards-stat-value">${escapeText(String(provider.positions))}</div>
      </div>
    `;
  }
  if (provider.claimedLamports > 0) {
    return `
      <div>
        <div class="rewards-card-kicker">Claimed</div>
        <div class="rewards-stat-value">${escapeText(formatRewardSol(provider.claimedLamports))}</div>
      </div>
    `;
  }
  return "";
}

function rewardsRowDisplayLabel(row) {
  const idx = state.wallets.findIndex((entry) => entry.key === row.walletKey);
  if (idx >= 0) {
    const tag = String(state.wallets[idx]?.label || "").trim();
    if (tag) return tag;
    return `#${idx + 1}`;
  }
  const fallback = String(row.walletLabel || "").trim();
  return fallback || row.walletKey || "Wallet";
}

function renderRewardWalletRow(provider, row) {
  const wallet = state.wallets.find((entry) => entry.key === row.walletKey);
  const label = rewardsRowDisplayLabel(row);
  const pubkey = row.walletPublicKey || wallet?.publicKey || "";
  const status = row.status === "claimed" ? "Claimed" : row.reason;
  return `
    <div class="rewards-wallet-row" data-reward-row-id="${escapeAttribute(row.id)}">
      <div class="rewards-wallet-main">
        <div class="rewards-wallet-name">${escapeText(label)}</div>
        <div class="rewards-wallet-sub">${escapeText(truncateMiddle(pubkey, 5, 4) || row.walletKey)}</div>
        ${status ? `<div class="rewards-wallet-status">${escapeText(status)}</div>` : ""}
      </div>
      <div class="rewards-wallet-actions">
        <div class="rewards-wallet-amount">${escapeText(formatRewardSol(row.amountLamports))} SOL</div>
        <button class="secondary-button rewards-claim-button" type="button" data-rewards-claim-row="${escapeAttribute(row.id)}" data-rewards-claim-provider="${escapeAttribute(provider.providerId)}" ${row.claimable && !state.rewards.claiming ? "" : "disabled"}>Claim</button>
      </div>
    </div>
  `;
}

function rewardProviderMeta(providerId) {
  return REWARD_PROVIDER_META[providerId] || {
    title: providerId || "Rewards",
    label: providerId || "Rewards",
    icon: "◎",
    iconUrl: ""
  };
}

function providerClaimableLamports(provider) {
  return provider.rows.reduce((sum, row) => sum + (row.claimable ? row.amountLamports : 0), 0);
}

function formatRewardSol(lamports) {
  const sol = normalizeLamports(lamports) / LAMPORTS_PER_SOL;
  if (sol === 0) return "0";
  return sol.toFixed(sol >= 1 ? 4 : 6).replace(/\.?0+$/, "");
}

async function claimRewardsForRows(rows, providerId) {
  const claimableRows = rows.filter((row) => row && row.claimable && row.amountLamports > 0);
  if (!claimableRows.length || state.rewards.claiming) return;
  state.rewards.claiming = true;
  renderRewards();
  try {
    const result = await callBackground("trench:claim-rewards", {
      providerId,
      items: claimableRows.map((row) => ({
        id: row.id,
        providerId: row.providerId,
        provider: row.provider,
        rewardType: row.rewardType,
        walletKey: row.walletKey,
        walletPublicKey: row.walletPublicKey,
        mint: row.mint,
        amountLamports: row.amountLamports
      }))
    });
    applyRewardClaimResult(result);
    const failedCount = Number(result?.failedCount || 0) + Number(result?.staleCount || 0);
    if (failedCount > 0) {
      showOptionsToast("Rewards claim incomplete", `${failedCount} claim${failedCount === 1 ? "" : "s"} failed or went stale.`, { type: "error" });
    } else {
      showOptionsToast("Rewards claimed", "Claimable rewards were claimed.", { type: "success" });
    }
  } catch (error) {
    applyRewardClaimLocalStatus(
      claimableRows.map((row) => row.id),
      error.timeout ? "stale" : "failed",
      error.message || "Claim failed."
    );
    showOptionsToast("Rewards claim failed", error.message, { type: "error" });
  } finally {
    state.rewards.claiming = false;
    renderRewards();
  }
}

function applyRewardClaimResult(result) {
  const results = Array.isArray(result?.results) ? result.results : [];
  const confirmedIds = new Set(
    results
      .filter((entry) => entry?.status === "confirmed")
      .map((entry) => String(entry.id || "").trim())
      .filter(Boolean)
  );
  const statusById = new Map(
    results
      .map((entry) => [
        String(entry?.id || "").trim(),
        {
          status: String(entry?.status || "").trim(),
          reason: String(entry?.error || "").trim()
        }
      ])
      .filter(([id]) => id)
  );
  if (!confirmedIds.size && !statusById.size) return;
  state.rewards.providers = state.rewards.providers.map((provider) => ({
    ...provider,
    rows: provider.rows.map((row) => {
      if (confirmedIds.has(row.id)) {
        return {
          ...row,
          amountLamports: 0,
          amountUi: 0,
          claimable: false,
          status: "claimed",
          reason: ""
        };
      }
      const result = statusById.get(row.id);
      if (!result || result.status === "confirmed") return row;
      return {
        ...row,
        status: result.status || "failed",
        reason: result.reason || row.reason
      };
    })
  }));
}

function applyRewardClaimLocalStatus(rowIds, status, reason) {
  const ids = new Set((Array.isArray(rowIds) ? rowIds : []).map((id) => String(id || "").trim()).filter(Boolean));
  if (!ids.size) return;
  state.rewards.providers = state.rewards.providers.map((provider) => ({
    ...provider,
    rows: provider.rows.map((row) => {
      if (!ids.has(row.id)) return row;
      return {
        ...row,
        status,
        reason
      };
    })
  }));
}

function patchWalletBalanceChips() {
  // Active wallets grid.
  const walletCards = elements.activeWalletsGrid
    ? elements.activeWalletsGrid.querySelectorAll(".wallet-card[data-wallet-key]")
    : [];
  walletCards.forEach((card) => {
    const key = card.getAttribute("data-wallet-key");
    if (!key) return;
    const chip = card.querySelector(".wallet-card-balance");
    if (!chip) return;
    updateBalanceChip(chip, state.walletBalances.get(key), "wallet-card-balance");
  });
  if (state.createGroupModal?.open && elements.createGroupWalletList) {
    const rows = elements.createGroupWalletList.querySelectorAll(
      ".create-group-wallet-option[data-wallet-key]"
    );
    rows.forEach((row) => {
      const key = row.getAttribute("data-wallet-key");
      if (!key) return;
      const chip = row.querySelector(".create-group-wallet-balance");
      if (!chip) return;
      updateBalanceChip(chip, state.walletBalances.get(key), "create-group-wallet-balance");
    });
  }
}

const SOL_ICON_SVG =
  '<svg class="sol-mark" viewBox="0 0 14 11" xmlns="http://www.w3.org/2000/svg" aria-hidden="true" focusable="false">' +
  '<path d="M2.6 0 L13.6 0 L11.4 2.8 L0.4 2.8 Z"/>' +
  '<path d="M0.4 4.1 L11.4 4.1 L13.6 6.9 L2.6 6.9 Z"/>' +
  '<path d="M2.6 8.2 L13.6 8.2 L11.4 11 L0.4 11 Z"/>' +
  "</svg>";

function buildBalanceChipInnerHtml(balance) {
  if (!balance) {
    return `${SOL_ICON_SVG}<span class="wallet-balance-value">—</span>`;
  }
  if (balance.kind === "error") {
    return `<span class="wallet-balance-value">${escapeText(balance.text)}</span>`;
  }
  return `${SOL_ICON_SVG}<span class="wallet-balance-value">${escapeText(balance.value)}</span>`;
}

function updateBalanceChip(chip, entry, baseClass) {
  const balance = formatWalletBalance(entry);
  if (balance) {
    chip.className = `${baseClass}${balance.kind === "error" ? " is-error" : ""}`;
    chip.innerHTML = buildBalanceChipInnerHtml(balance);
    if (balance.kind === "error") {
      chip.setAttribute("title", balance.text);
    } else {
      chip.setAttribute("title", `${balance.value} SOL`);
    }
  } else {
    chip.className = `${baseClass} is-pending`;
    chip.innerHTML = buildBalanceChipInnerHtml(null);
    chip.removeAttribute("title");
  }
}

function formatWalletBalance(entry) {
  if (!entry) return null;
  if (entry.balanceError) {
    return { text: "Balance unavailable", value: "—", kind: "error" };
  }
  if (typeof entry.balanceSol === "number" && Number.isFinite(entry.balanceSol)) {
    const value = entry.balanceSol;
    let valueText;
    if (value === 0) {
      valueText = "0";
    } else if (value >= 100) {
      valueText = value.toFixed(2);
    } else if (value >= 1) {
      valueText = value.toFixed(3);
    } else {
      valueText = value.toFixed(4);
    }
    return { text: `${valueText} SOL`, value: valueText, kind: "ok" };
  }
  return null;
}

function renderSections() {
  for (const button of elements.navButtons) {
    button.classList.toggle("active", button.dataset.sectionTarget === state.activeSection);
  }
  for (const section of document.querySelectorAll(".section")) {
    const sectionId = section.id;
    const isWalletsSection =
      state.activeSection === "wallets" &&
      sectionId === "section-wallets";
    const isGlobalSection =
      state.activeSection === "global" && sectionId === "section-connection";
    section.classList.toggle(
      "active",
      sectionId === `section-${state.activeSection}` || isWalletsSection || isGlobalSection
    );
  }
}

function renderSiteSettings() {
  const siteFeatures = state.siteFeatures;
  if (!siteFeatures) {
    return;
  }
  elements.siteAxiomEnabled.checked = Boolean(siteFeatures.axiom?.enabled);
  elements.siteAxiomLauncher.checked = Boolean(siteFeatures.axiom?.floatingLauncher);
  elements.siteAxiomInstantTrade.checked = Boolean(
    siteFeatures.axiom?.instantTrade ?? siteFeatures.axiom?.tokenDetailButton
  );
  if (elements.siteAxiomInstantTradeButtonModeCount) {
    const count = Number(siteFeatures.axiom?.instantTradeButtonModeCount);
    elements.siteAxiomInstantTradeButtonModeCount.value = count === 1 || count === 2 ? String(count) : "3";
  }
  if (elements.siteAxiomLaunchdeck) {
    elements.siteAxiomLaunchdeck.checked = Boolean(siteFeatures.axiom?.launchdeckInjection);
  }
  elements.siteAxiomPulseQb.checked = Boolean(siteFeatures.axiom?.pulseButton);
  elements.siteAxiomPulsePanel.checked = Boolean(siteFeatures.axiom?.pulsePanel);
  if (elements.siteAxiomPulseVamp) {
    elements.siteAxiomPulseVamp.checked = Boolean(siteFeatures.axiom?.pulseVamp);
  }
  if (elements.siteAxiomVampIconMode) {
    const mode = String(siteFeatures.axiom?.vampIconMode || "").trim().toLowerCase();
    const fallback = siteFeatures.axiom?.pulseVamp === false ? "off" : "both";
    elements.siteAxiomVampIconMode.value =
      mode === "pulse" || mode === "token" || mode === "off" ? mode : fallback;
  }
  if (elements.siteAxiomPulseVampMode) {
    const mode = String(siteFeatures.axiom?.pulseVampMode || "prefill");
    elements.siteAxiomPulseVampMode.value = mode === "insta" ? "insta" : "prefill";
  }
  if (elements.siteAxiomDexScreenerIconMode) {
    const mode = String(siteFeatures.axiom?.dexScreenerIconMode || "both").trim().toLowerCase();
    elements.siteAxiomDexScreenerIconMode.value =
      mode === "pulse" || mode === "token" || mode === "off" ? mode : "both";
  }
  if (elements.siteAxiomPostDeployAction) {
    const action = String(siteFeatures.axiom?.postDeployAction || "close_modal_toast").trim().toLowerCase();
    elements.siteAxiomPostDeployAction.value =
      action === "toast_only" || action === "open_tab_toast" || action === "open_window_toast"
        ? action
        : "close_modal_toast";
  }
  elements.siteAxiomWalletTracker.checked = Boolean(siteFeatures.axiom?.walletTracker);
  elements.siteAxiomWatchlist.checked = Boolean(siteFeatures.axiom?.watchlist);
  elements.siteJ7Enabled.checked = false;
  elements.siteJ7Enabled.disabled = true;
}

function appearanceSoundKey(side) {
  return side === "sell" ? "sellSound" : "buySound";
}

function renderAppearanceSoundSide(side) {
  const refs = elements.appearanceSoundControls[side];
  if (!refs || !refs.enabled) {
    return;
  }
  const key = appearanceSoundKey(side);
  const settings = normalizeAppearance(state.appearance)[key];
  refs.enabled.checked = Boolean(settings.enabled);
  refs.template.value = settings.templateId;

  const isCustom = settings.templateId === SOUND_CUSTOM_ID;
  refs.customRow.classList.toggle("hidden", !isCustom);
  if (settings.custom) {
    refs.customName.textContent = settings.custom.name || "Custom sound";
    refs.customName.classList.remove("is-empty");
    refs.customClear.classList.remove("hidden");
  } else {
    refs.customName.textContent = "No custom sound uploaded yet.";
    refs.customName.classList.add("is-empty");
    refs.customClear.classList.add("hidden");
  }

  const disabled = !settings.enabled;
  refs.template.disabled = disabled;
  refs.preview.disabled = disabled || (isCustom && !settings.custom);
}

function renderAppearanceVolume() {
  const refs = elements.appearanceVolume;
  if (!refs?.slider) {
    return;
  }
  const appearance = normalizeAppearance(state.appearance);
  const volume = Number.isFinite(appearance.volume) ? appearance.volume : 70;
  refs.slider.value = String(volume);
  refs.input.value = String(volume);
  refs.value.textContent = String(volume);
}

function renderAppearanceSettings() {
  renderAppearanceVolume();
  renderAppearanceSoundSide("buy");
  renderAppearanceSoundSide("sell");
}

function setAppearanceStatus(side, message = "", kind = "") {
  const refs = elements.appearanceSoundControls[side];
  if (!refs?.status) {
    return;
  }
  refs.status.textContent = message || "";
  refs.status.classList.remove("is-error", "is-success");
  if (kind === "error") {
    refs.status.classList.add("is-error");
  } else if (kind === "success") {
    refs.status.classList.add("is-success");
  }
  const timers = state.appearanceStatusTimers;
  if (timers[side]) {
    clearTimeout(timers[side]);
    timers[side] = null;
  }
  if (message) {
    timers[side] = setTimeout(() => {
      if (refs.status) {
        refs.status.textContent = "";
        refs.status.classList.remove("is-error", "is-success");
      }
      timers[side] = null;
    }, 3500);
  }
}

async function persistAppearance(nextAppearance) {
  const normalized = normalizeAppearance(nextAppearance);
  state.appearance = normalized;
  renderAppearanceSettings();
  try {
    await saveAppearance(normalized);
  } catch (error) {
    // Surface the error on both panels since we don't know which one triggered the save.
    setAppearanceStatus("buy", error?.message || "Failed to save appearance settings.", "error");
    setAppearanceStatus("sell", error?.message || "Failed to save appearance settings.", "error");
  }
}

function stopAppearancePreview() {
  const preview = state.appearancePreview;
  if (preview?.audio) {
    try {
      preview.audio.pause();
      preview.audio.currentTime = 0;
    } catch {}
  }
  state.appearancePreview = { side: null, audio: null };
  for (const key of ["buy", "sell"]) {
    const refs = elements.appearanceSoundControls[key];
    if (refs?.preview) {
      refs.preview.classList.remove("is-playing");
      refs.preview.textContent = "Preview";
    }
  }
}

function playAppearancePreview(side) {
  const refs = elements.appearanceSoundControls[side];
  if (!refs?.preview) {
    return;
  }
  stopAppearancePreview();
  const key = appearanceSoundKey(side);
  const settings = { ...state.appearance[key], enabled: true };
  const url = resolveSoundUrl(settings, (path) => chrome.runtime.getURL(path));
  if (!url) {
    setAppearanceStatus(side, "Pick or upload a sound first.", "error");
    return;
  }
  const audio = new Audio(url);
  const sharedVolume = Number(state.appearance?.volume);
  audio.volume = Math.min(
    1,
    Math.max(0, (Number.isFinite(sharedVolume) ? sharedVolume : 70) / 100)
  );
  audio.addEventListener("ended", stopAppearancePreview);
  audio.addEventListener("error", () => {
    stopAppearancePreview();
    setAppearanceStatus(side, "Failed to play the selected sound.", "error");
  });
  state.appearancePreview = { side, audio };
  refs.preview.classList.add("is-playing");
  refs.preview.textContent = "Stop";
  audio.play().catch((error) => {
    stopAppearancePreview();
    setAppearanceStatus(side, error?.message || "Audio playback blocked by the browser.", "error");
  });
}

function readFileAsDataUrl(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () => resolve(String(reader.result || "")));
    reader.addEventListener("error", () => reject(reader.error || new Error("Unable to read file")));
    reader.readAsDataURL(file);
  });
}

function registerAppearanceHandlersForSide(side) {
  const refs = elements.appearanceSoundControls[side];
  if (!refs?.enabled) {
    return;
  }
  const key = appearanceSoundKey(side);

  refs.enabled.addEventListener("change", () => {
    stopAppearancePreview();
    const next = normalizeAppearance(state.appearance);
    next[key].enabled = Boolean(refs.enabled.checked);
    void persistAppearance(next);
  });

  refs.template.addEventListener("change", () => {
    stopAppearancePreview();
    const next = normalizeAppearance(state.appearance);
    next[key].templateId = String(refs.template.value || "");
    // Keep any existing custom blob around so toggling back to "Custom" restores it.
    void persistAppearance(next);
  });

  refs.preview.addEventListener("click", () => {
    if (refs.preview.classList.contains("is-playing")) {
      stopAppearancePreview();
      return;
    }
    playAppearancePreview(side);
  });

  refs.customInput.addEventListener("change", async (event) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) {
      return;
    }
    if (!file.type.startsWith("audio/")) {
      setAppearanceStatus(side, "Please choose an audio file.", "error");
      return;
    }
    if (file.size > SOUND_CUSTOM_MAX_BYTES) {
      setAppearanceStatus(
        side,
        `File is too large. Keep it under ${Math.round(SOUND_CUSTOM_MAX_BYTES / (1024 * 1024))} MB.`,
        "error"
      );
      return;
    }
    try {
      const dataUrl = await readFileAsDataUrl(file);
      const next = normalizeAppearance(state.appearance);
      next[key].templateId = SOUND_CUSTOM_ID;
      next[key].custom = { name: file.name, dataUrl };
      await persistAppearance(next);
      setAppearanceStatus(side, `Uploaded "${file.name}".`, "success");
    } catch (error) {
      setAppearanceStatus(side, error?.message || "Failed to read file.", "error");
    }
  });

  refs.customClear.addEventListener("click", () => {
    const next = normalizeAppearance(state.appearance);
    next[key].custom = null;
    if (next[key].templateId === SOUND_CUSTOM_ID) {
      next[key].templateId = SOUND_TEMPLATES[0].id;
    }
    void persistAppearance(next);
  });
}

function registerAppearanceVolumeHandlers() {
  const refs = elements.appearanceVolume;
  if (!refs?.slider) {
    return;
  }
  let volumePersistTimer = null;
  const applyVolume = (rawValue, { immediate = false } = {}) => {
    const num = Math.min(100, Math.max(0, Math.round(Number(rawValue) || 0)));
    refs.slider.value = String(num);
    refs.input.value = String(num);
    refs.value.textContent = String(num);
    const preview = state.appearancePreview;
    if (preview?.audio) {
      preview.audio.volume = Math.min(1, Math.max(0, num / 100));
    }
    const next = normalizeAppearance(state.appearance);
    next.volume = num;
    state.appearance = next;
    if (volumePersistTimer) {
      clearTimeout(volumePersistTimer);
      volumePersistTimer = null;
    }
    if (immediate) {
      void saveAppearance(next);
      return;
    }
    volumePersistTimer = setTimeout(() => {
      volumePersistTimer = null;
      void saveAppearance(state.appearance);
    }, 180);
  };
  refs.slider.addEventListener("input", (event) => applyVolume(event.target.value));
  refs.slider.addEventListener("change", (event) => applyVolume(event.target.value, { immediate: true }));
  refs.input.addEventListener("change", (event) => applyVolume(event.target.value, { immediate: true }));
}

function registerAppearanceHandlers() {
  registerAppearanceVolumeHandlers();
  registerAppearanceHandlersForSide("buy");
  registerAppearanceHandlersForSide("sell");
}

const PRESET_REGION_OPTIONS = new Set([
  "",
  "us",
  "eu",
  "asia",
  "slc",
  "ewr",
  "lon",
  "fra",
  "ams",
  "sg",
  "tyo"
]);

function applyRegionValue(region) {
  const trimmed = String(region || "").trim();
  if (PRESET_REGION_OPTIONS.has(trimmed)) {
    elements.engineEndpointProfile.value = trimmed;
    if (elements.engineEndpointProfileCustom) {
      elements.engineEndpointProfileCustom.value = "";
    }
    if (elements.engineEndpointProfileCustomRow) {
      elements.engineEndpointProfileCustomRow.classList.add("hidden");
    }
  } else {
    elements.engineEndpointProfile.value = "custom";
    if (elements.engineEndpointProfileCustom) {
      elements.engineEndpointProfileCustom.value = trimmed;
    }
    if (elements.engineEndpointProfileCustomRow) {
      elements.engineEndpointProfileCustomRow.classList.remove("hidden");
    }
  }
}

function resolveSelectedRegion() {
  const selected = elements.engineEndpointProfile.value;
  if (selected === "custom") {
    return elements.engineEndpointProfileCustom?.value.trim() || "";
  }
  return selected.trim();
}

function validateEngineSettingsBeforeSave() {
  if (elements.engineEndpointProfile?.value !== "custom") {
    return true;
  }
  const customRegion = elements.engineEndpointProfileCustom?.value.trim() || "";
  if (customRegion) {
    return true;
  }
  const message = "Enter a custom region tag or select global before saving.";
  clearFieldInvalid(elements.engineEndpointProfileCustom);
  elements.engineEndpointProfileCustomRow?.classList.remove("hidden");
  unlockField(elements.engineEndpointProfileCustom);
  markFieldsInvalid([
    {
      fieldId: "engine-endpoint-profile-custom",
      message
    }
  ]);
  setStatus(message, true);
  showOptionsToast("Custom region required", message, { type: "error" });
  return false;
}

function resolveConfiguredRegion(settings) {
  const executionRegion = String(settings?.executionEndpointProfile ?? "").trim();
  if (executionRegion) {
    return executionRegion;
  }
  const sharedRegion = String(settings?.sharedRegion ?? "").trim();
  if (sharedRegion) {
    return sharedRegion;
  }
  return String(settings?.heliusSenderRegion ?? "").trim();
}

function renderEngineSettings() {
  const settings = state.settings || emptySettings();
  elements.engineBuySlippage.value = settings.defaultBuySlippagePercent || "";
  elements.engineSellSlippage.value = settings.defaultSellSlippagePercent || "";
  elements.engineBuyMevMode.value = settings.defaultBuyMevMode || "off";
  elements.engineSellMevMode.value = settings.defaultSellMevMode || "off";
  elements.engineProvider.value = selectableExtensionRouteProvider(settings.executionProvider);
  applyRegionValue(resolveConfiguredRegion(settings));
  elements.engineCommitment.value = "confirmed";
  elements.engineSkipPreflight.value = "on";
  elements.engineTrackSendBlockHeight.value = "off";
  if (elements.engineAllowNonCanonicalPoolTrades) {
    elements.engineAllowNonCanonicalPoolTrades.checked = Boolean(
      settings.allowNonCanonicalPoolTrades
    );
  }
  elements.engineMaxActiveBatches.value = String(settings.maxActiveBatches || 32);
  elements.engineRpcUrl.value = settings.rpcUrl || "";
  elements.engineWsUrl.value = settings.wsUrl || "";
  elements.engineWarmRpcUrl.value = settings.warmRpcUrl || "";
  if (elements.engineWarmWsUrl) elements.engineWarmWsUrl.value = settings.warmWsUrl || "";
  elements.engineSharedRegion.value = settings.sharedRegion || "";
  elements.engineHeliusRpcUrl.value = settings.heliusRpcUrl || "";
  elements.engineHeliusWsUrl.value = settings.heliusWsUrl || "";
  elements.engineStandardRpcSendUrls.value = Array.isArray(settings.standardRpcSendUrls)
    ? settings.standardRpcSendUrls.join(", ")
    : "";
  elements.engineHeliusSenderRegion.value = settings.heliusSenderRegion || "";
  if (elements.enginePnlTrackingMode) {
    elements.enginePnlTrackingMode.value = settings.pnlTrackingMode || "local";
  }
  if (elements.enginePnlIncludeFees) {
    elements.enginePnlIncludeFees.checked = settings.pnlIncludeFees !== false;
  }
  if (elements.engineWrapperDefaultFeeBps) {
    elements.engineWrapperDefaultFeeBps.value = String(
      clampWrapperFeeBps(settings.wrapperDefaultFeeBps)
    );
  }
  elements.authBootstrapHint.textContent = state.authBootstrap
    ? `Default engine token file: ${displayTokenFileName(state.authBootstrap.tokenFilePath)}`
    : "No auth bootstrap info loaded yet.";
  setEngineSettingsDirty(false);
}

function displayTokenFileName(tokenFilePath) {
  const value = String(tokenFilePath || "").trim();
  return value.split(/[\\/]/).filter(Boolean).pop() || "default-engine-token.txt";
}

async function warnIfExecutionConnectionDraft() {
  const draftHost = normalizeHostBase(elements.hostInput.value);
  const savedHost = await getHostBase();
  const draftToken = String(elements.hostAuthTokenInput.value || "").trim();
  const savedToken = await getHostAuthToken();
  if (draftHost === savedHost && draftToken === savedToken) {
    return false;
  }
  const message = "Save connection before testing. The test uses the saved execution host and token.";
  elements.connectionStatus.textContent = message;
  showOptionsToast("Save connection first", message, { type: "info" });
  return true;
}

async function warnIfLaunchdeckConnectionDraft() {
  const draftHost = normalizeLaunchdeckHostBase(elements.launchdeckHostInput.value);
  const savedHost = await getLaunchdeckHostBase();
  const draftToken = String(elements.hostAuthTokenInput.value || "").trim();
  const savedToken = await getHostAuthToken();
  const hostChanged = draftHost !== savedHost;
  const tokenChanged = draftToken !== savedToken;
  if (!hostChanged && !tokenChanged) {
    return false;
  }
  let message = "Save LaunchDeck connection before testing. The test uses the saved LaunchDeck host.";
  let title = "Save LaunchDeck connection first";
  if (hostChanged && tokenChanged) {
    message = "Save connection and LaunchDeck connection before testing. The test uses the saved LaunchDeck host and shared token.";
    title = "Save both connections first";
  } else if (tokenChanged) {
    message = "Save connection before testing. The shared token is saved in Primary connection.";
    title = "Save connection first";
  }
  elements.launchdeckConnectionStatus.textContent = message;
  showOptionsToast(title, message, { type: "info" });
  return true;
}

function base64ToUint8Array(value) {
  const normalized = String(value || "").trim();
  if (!normalized) {
    return new Uint8Array();
  }
  const binary = atob(normalized);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

function triggerBrowserDownload(bytes, filename) {
  const blob = new Blob([bytes], { type: "application/zip" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

function renderPresets() {
  renderExecutionPresets();
  renderLaunchdeckPresets();
}

const WALLET_DRAG_THRESHOLD_PX = 5;
let walletDragState = null;
let walletDragSuppressClickUntil = 0;

function renderWallets() {
  if (walletDragState) {
    if (walletDragState.dragging) {
      try {
        endWalletDrag(walletDragState);
      } catch (error) {}
    }
    walletDragState = null;
    document.body.classList.remove("wallet-drag-active");
  }
  elements.activeWalletsGrid.innerHTML = "";
  if (!state.wallets.length) {
    const empty = document.createElement("div");
    empty.className = "active-wallets-empty";
    empty.innerHTML = `
      <div class="active-wallets-empty-copy">
        <strong>No wallets yet</strong>
        <p class="muted">Add your first wallet to start executing trades from this extension.</p>
      </div>
    `;
    elements.activeWalletsGrid.appendChild(empty);
    return;
  }
  state.wallets.forEach((wallet, index) => {
    const card = document.createElement("div");
    card.className = "wallet-card";
    card.draggable = false;
    card.dataset.walletKey = wallet.key;
    card.tabIndex = 0;
    const labelText = formatWalletDisplayLabel(wallet, `Wallet ${index + 1}`);
    const shortPubkey = truncateMiddle(wallet.publicKey, 4, 4);
    const emoji = walletDisplayEmoji(wallet);
    const avatarBg = walletAvatarGradient(wallet.key || wallet.publicKey || labelText);
    const balance = formatWalletBalance(state.walletBalances.get(wallet.key));
    const balanceTitle = balance ? escapeAttribute(balance.text) : "Balance pending";
    const balanceHtml = `<span class="wallet-card-balance${balance && balance.kind === "error" ? " is-error" : ""}${balance ? "" : " is-pending"}" title="${balanceTitle}">${buildBalanceChipInnerHtml(balance)}</span>`;
    const envSlot = walletEnvSlotTag(wallet.key);
    const envSlotHtml = envSlot
      ? `<span class="wallet-card-slot" title="${escapeAttribute(wallet.key)}">${escapeText(envSlot)}</span>`
      : "";
    card.innerHTML = `
      <div class="wallet-card-drag" aria-hidden="true">⋮⋮</div>
      <div class="wallet-card-avatar" style="background:${avatarBg}">
        <span class="wallet-card-avatar-emoji">${escapeText(emoji)}</span>
      </div>
      <div class="wallet-card-copy">
        <div class="wallet-card-name-row">
          <span class="wallet-card-name">${escapeText(labelText)}</span>
          ${envSlotHtml}
          ${wallet.enabled ? "" : `<span class="wallet-card-off-chip">Off</span>`}
        </div>
        <div class="wallet-card-sub">
          <span class="wallet-card-pubkey" title="${escapeAttribute(wallet.publicKey)}">${escapeText(shortPubkey || "—")}</span>
        </div>
      </div>
      ${balanceHtml}
      <div class="wallet-card-actions">
        <span class="wallet-card-toggle" data-role="toggle" aria-label="Toggle wallet">
          <input type="checkbox" ${wallet.enabled ? "checked" : ""} />
          <span class="wallet-card-toggle-track"><span class="wallet-card-toggle-thumb"></span></span>
        </span>
      </div>
    `;
    card.addEventListener("click", (event) => {
      if (Date.now() < walletDragSuppressClickUntil) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      const target = event.target instanceof Element ? event.target : null;
      if (target?.closest('[data-role="toggle"]')) {
        event.preventDefault();
        event.stopPropagation();
        void toggleWalletEnabled(wallet);
        return;
      }
      openWalletEditModal(wallet.key);
    });
    attachWalletCardPointerHandlers(card);
    elements.activeWalletsGrid.appendChild(card);
  });
}

function attachWalletCardPointerHandlers(card) {
  card.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) {
      return;
    }
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest('[data-role="toggle"]')) {
      return;
    }
    if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA")) {
      return;
    }
    walletDragState = {
      card,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      dragging: false,
      placeholder: null,
      offsetX: 0,
      offsetY: 0,
      width: 0,
      height: 0
    };
    try {
      card.setPointerCapture(event.pointerId);
    } catch (error) {}
  });
  card.addEventListener("pointermove", (event) => {
    if (!walletDragState || walletDragState.pointerId !== event.pointerId) {
      return;
    }
    const dx = event.clientX - walletDragState.startX;
    const dy = event.clientY - walletDragState.startY;
    if (!walletDragState.dragging) {
      if (Math.hypot(dx, dy) < WALLET_DRAG_THRESHOLD_PX) {
        return;
      }
      beginWalletDrag(walletDragState);
    }
    event.preventDefault();
    updateWalletDragPosition(walletDragState, event.clientX, event.clientY);
  });
  const finishDrag = (event) => {
    if (!walletDragState || walletDragState.pointerId !== event.pointerId) {
      return;
    }
    const draggedCard = walletDragState.card;
    const dragging = walletDragState.dragging;
    try {
      if (draggedCard.hasPointerCapture(event.pointerId)) {
        draggedCard.releasePointerCapture(event.pointerId);
      }
    } catch (error) {}
    if (dragging) {
      endWalletDrag(walletDragState);
      walletDragSuppressClickUntil = Date.now() + 250;
      walletDragState = null;
      void commitWalletOrderFromDom();
    } else {
      walletDragState = null;
    }
  };
  card.addEventListener("pointerup", finishDrag);
  card.addEventListener("pointercancel", finishDrag);
  card.addEventListener("lostpointercapture", finishDrag);
}

function beginWalletDrag(dragState) {
  const card = dragState.card;
  const rect = card.getBoundingClientRect();
  dragState.dragging = true;
  dragState.offsetX = dragState.startX - rect.left;
  dragState.offsetY = dragState.startY - rect.top;
  dragState.width = rect.width;
  dragState.height = rect.height;
  dragState.swapTarget = null;

  const placeholder = document.createElement("div");
  placeholder.className = "wallet-card-placeholder";
  placeholder.style.width = `${rect.width}px`;
  placeholder.style.height = `${rect.height}px`;
  card.parentNode.insertBefore(placeholder, card);
  dragState.placeholder = placeholder;

  card.classList.add("wallet-card-dragging");
  card.style.position = "fixed";
  card.style.left = `${rect.left}px`;
  card.style.top = `${rect.top}px`;
  card.style.width = `${rect.width}px`;
  card.style.height = `${rect.height}px`;
  card.style.zIndex = "1000";
  card.style.pointerEvents = "none";
  document.body.classList.add("wallet-drag-active");
}

function updateWalletDragPosition(dragState, pointerX, pointerY) {
  const card = dragState.card;
  if (!dragState.placeholder) {
    return;
  }
  card.style.left = `${pointerX - dragState.offsetX}px`;
  card.style.top = `${pointerY - dragState.offsetY}px`;

  const grid = elements.activeWalletsGrid;
  const hit = findWalletSwapTarget(grid, card, pointerX, pointerY);
  setWalletSwapTarget(dragState, hit);
}

function setWalletSwapTarget(dragState, target) {
  if (dragState.swapTarget === target) {
    return;
  }
  if (dragState.swapTarget) {
    dragState.swapTarget.classList.remove("wallet-card-swap-target");
  }
  dragState.swapTarget = target || null;
  if (dragState.swapTarget) {
    dragState.swapTarget.classList.add("wallet-card-swap-target");
  }
}

function endWalletDrag(dragState) {
  const card = dragState.card;
  const placeholder = dragState.placeholder;
  const swapTarget = dragState.swapTarget || null;
  if (swapTarget) {
    swapTarget.classList.remove("wallet-card-swap-target");
  }
  dragState.swapTarget = null;

  const grid = elements.activeWalletsGrid;
  if (placeholder && placeholder.parentNode) {
    placeholder.parentNode.insertBefore(card, placeholder);
    placeholder.remove();
  }
  dragState.placeholder = null;

  if (
    swapTarget &&
    swapTarget !== card &&
    card.parentNode === grid &&
    swapTarget.parentNode === grid
  ) {
    swapGridChildren(grid, card, swapTarget);
  }

  card.classList.remove("wallet-card-dragging");
  card.style.position = "";
  card.style.left = "";
  card.style.top = "";
  card.style.width = "";
  card.style.height = "";
  card.style.zIndex = "";
  card.style.pointerEvents = "";
  document.body.classList.remove("wallet-drag-active");
}

function findWalletSwapTarget(grid, draggedCard, pointerX, pointerY) {
  const cards = Array.from(grid.querySelectorAll(".wallet-card"));
  for (const card of cards) {
    if (card === draggedCard) {
      continue;
    }
    const rect = card.getBoundingClientRect();
    if (
      pointerX >= rect.left &&
      pointerX <= rect.right &&
      pointerY >= rect.top &&
      pointerY <= rect.bottom
    ) {
      return card;
    }
  }
  return null;
}

function swapGridChildren(parent, a, b) {
  if (a === b) {
    return;
  }
  const children = Array.from(parent.children);
  const aIdx = children.indexOf(a);
  const bIdx = children.indexOf(b);
  if (aIdx === -1 || bIdx === -1) {
    return;
  }
  if (aIdx < bIdx) {
    const afterB = b.nextSibling;
    parent.insertBefore(b, a);
    if (afterB) {
      parent.insertBefore(a, afterB);
    } else {
      parent.appendChild(a);
    }
  } else {
    const afterA = a.nextSibling;
    parent.insertBefore(a, b);
    if (afterA) {
      parent.insertBefore(b, afterA);
    } else {
      parent.appendChild(b);
    }
  }
}

async function commitWalletOrderFromDom() {
  const orderedKeys = Array.from(
    elements.activeWalletsGrid.querySelectorAll(".wallet-card[data-wallet-key]")
  )
    .map((node) => node.dataset.walletKey || "")
    .filter(Boolean);
  if (!orderedKeys.length) {
    return;
  }
  const byKey = new Map(state.wallets.map((wallet) => [wallet.key, wallet]));
  const sameOrder = orderedKeys.length === state.wallets.length
    && orderedKeys.every((key, index) => state.wallets[index]?.key === key);
  if (sameOrder) {
    return;
  }
  const next = orderedKeys.map((key) => byKey.get(key)).filter(Boolean);
  state.wallets = next;
  try {
    await callBackground("trench:reorder-wallets", {
      walletKeys: orderedKeys
    });
    await bumpBootstrapRevision();
    showOptionsToast("Wallets reordered", "Wallet order saved.", { type: "success" });
  } catch (error) {
    showOptionsToast("Reorder failed", error.message, { type: "error" });
    await refreshEngineData();
  }
}

async function toggleWalletEnabled(wallet) {
  try {
    await callBackground("trench:update-wallet", {
      walletKey: wallet.key,
      wallet: {
        label: wallet.label,
        enabled: !wallet.enabled
      }
    });
    await bumpBootstrapRevision();
    showOptionsToast(
      !wallet.enabled ? "Wallet enabled" : "Wallet disabled",
      wallet.label || wallet.key,
      { type: "success" }
    );
    await refreshEngineData();
  } catch (error) {
    showOptionsToast("Update failed", error.message, { type: "error" });
  }
}

function renderGroups() {
  elements.walletGroupsList.innerHTML = "";
  if (!state.walletGroups.length) {
    const empty = document.createElement("div");
    empty.className = "wallet-groups-empty";
    empty.innerHTML = `<p class="muted">No wallet groups yet. Tap New Group to batch wallets for quick switching.</p>`;
    elements.walletGroupsList.appendChild(empty);
    return;
  }
  const walletsByKey = new Map(state.wallets.map((wallet) => [wallet.key, wallet]));
  state.walletGroups.forEach((group) => {
    const walletKeys = Array.isArray(group.walletKeys) ? group.walletKeys : [];
    const walletEmojis = walletKeys
      .map((key) => {
        const wallet = walletsByKey.get(key);
        if (!wallet) {
          return "";
        }
        return walletDisplayEmoji(wallet);
      })
      .filter(Boolean);
    const policy = normalizeWalletGroupPolicy(group.batchPolicy);
    const tooltipBits = [];
    tooltipBits.push(policy.distributionMode === "split" ? "Split amount" : "Full amount");
    const varianceEnabled = Boolean(policy.buyVariancePercent);
    if (varianceEnabled) {
      tooltipBits.push(`${policy.buyVariancePercent}% variance`);
    }
    const staggerEnabled = policy.transactionDelayMode !== "off";
    let staggerValueLabel = "";
    if (staggerEnabled) {
      staggerValueLabel = policy.transactionDelayStrategy === "random"
        ? `${policy.transactionDelayMinMs}-${policy.transactionDelayMaxMs}ms`
        : `${policy.transactionDelayMs}ms`;
      tooltipBits.push(`Staggered ${staggerValueLabel}`);
    }
    const groupEmoji = (group.emoji || "").trim() || defaultEmojiForKey(group.id || group.label || "group");
    const maxVisible = 6;
    const visibleEmojis = walletEmojis.slice(0, maxVisible);
    const overflow = Math.max(0, walletEmojis.length - visibleEmojis.length);
    const stackHtml = visibleEmojis
      .map((emoji) => `<span class="wallet-group-wallet-emoji">${escapeText(emoji)}</span>`)
      .join("");
    const overflowHtml = overflow > 0
      ? `<span class="wallet-group-wallet-emoji wallet-group-wallet-emoji-more">+${overflow}</span>`
      : "";
    const badges = [];
    if (varianceEnabled) {
      badges.push(`<span class="wallet-group-tag-badge wallet-group-tag-badge-variance" title="${escapeAttribute(`${policy.buyVariancePercent}% variance`)}">${policy.buyVariancePercent}% variance</span>`);
    }
    if (staggerEnabled) {
      badges.push(`<span class="wallet-group-tag-badge wallet-group-tag-badge-stagger" title="${escapeAttribute(`Staggered ${staggerValueLabel}`)}">Staggered ${escapeText(staggerValueLabel)}</span>`);
    }
    const badgesHtml = badges.length
      ? `<span class="wallet-group-tag-badges" aria-hidden="true">${badges.join("")}</span>`
      : "";
    const row = document.createElement("button");
    row.type = "button";
    row.className = "wallet-group-tag";
    row.dataset.groupId = group.id;
    row.title = tooltipBits.join(" · ");
    row.innerHTML = `
      <span class="wallet-group-tag-emoji" aria-hidden="true">${escapeText(groupEmoji)}</span>
      <span class="wallet-group-tag-name">${escapeText(group.label || group.id)}</span>
      ${badgesHtml}
      <span class="wallet-group-tag-stack" aria-hidden="true">${stackHtml}${overflowHtml}</span>
    `;
    row.addEventListener("click", () => openCreateGroupModal(group.id));
    elements.walletGroupsList.appendChild(row);
  });
}

function renderBuyDistribution() {
  const mode = state.settings.defaultDistributionMode === "split" ? "split" : "each";
  elements.buyDistributionSplit.classList.toggle("active", mode === "split");
  elements.buyDistributionEach.classList.toggle("active", mode === "each");
}

async function setDefaultDistributionMode(mode) {
  if (state.settings.defaultDistributionMode === mode) {
    return;
  }
  const previous = state.settings.defaultDistributionMode;
  state.settings = normalizeSettings({ ...state.settings, defaultDistributionMode: mode });
  renderBuyDistribution();
  try {
    await callBackground("trench:save-settings", state.settings);
    await bumpBootstrapRevision();
    showOptionsToast(
      "Buy distribution updated",
      `Set to ${mode === "split" ? "split amount" : "full amount each"}.`,
      { type: "success" }
    );
  } catch (error) {
    state.settings = normalizeSettings({ ...state.settings, defaultDistributionMode: previous });
    renderBuyDistribution();
    showOptionsToast("Update failed", error.message, { type: "error" });
  }
}

function collectSiteSettings() {
  const vampModeRaw = String(elements.siteAxiomPulseVampMode?.value || "").trim().toLowerCase();
  const pulseVampMode = vampModeRaw === "insta" ? "insta" : "prefill";
  const vampIconModeRaw = String(elements.siteAxiomVampIconMode?.value || "").trim().toLowerCase();
  const vampIconMode =
    vampIconModeRaw === "pulse" ||
    vampIconModeRaw === "token" ||
    vampIconModeRaw === "off"
      ? vampIconModeRaw
      : "both";
  const dexScreenerIconModeRaw = String(elements.siteAxiomDexScreenerIconMode?.value || "").trim().toLowerCase();
  const dexScreenerIconMode =
    dexScreenerIconModeRaw === "pulse" ||
    dexScreenerIconModeRaw === "token" ||
    dexScreenerIconModeRaw === "off"
      ? dexScreenerIconModeRaw
      : "both";
  const instantTradeButtonModeCountRaw = Number(elements.siteAxiomInstantTradeButtonModeCount?.value);
  const instantTradeButtonModeCount =
    instantTradeButtonModeCountRaw === 1 || instantTradeButtonModeCountRaw === 2
      ? instantTradeButtonModeCountRaw
      : 3;
  const postDeployActionRaw = String(elements.siteAxiomPostDeployAction?.value || "").trim().toLowerCase();
  const postDeployAction =
    postDeployActionRaw === "toast_only" ||
    postDeployActionRaw === "open_tab_toast" ||
    postDeployActionRaw === "open_window_toast"
      ? postDeployActionRaw
      : "close_modal_toast";
  return {
    axiom: {
      enabled: elements.siteAxiomEnabled.checked,
      floatingLauncher: elements.siteAxiomLauncher.checked,
      instantTrade: elements.siteAxiomInstantTrade.checked,
      instantTradeButtonModeCount,
      launchdeckInjection: elements.siteAxiomLaunchdeck ? elements.siteAxiomLaunchdeck.checked : true,
      pulseButton: elements.siteAxiomPulseQb.checked,
      pulsePanel: elements.siteAxiomPulsePanel.checked,
      pulseVamp: vampIconMode === "both" || vampIconMode === "pulse",
      pulseVampMode,
      vampIconMode,
      dexScreenerIconMode,
      postDeployAction,
      postDeployDestination: "axiom",
      walletTracker: elements.siteAxiomWalletTracker.checked,
      watchlist: elements.siteAxiomWatchlist.checked
    },
    j7: {
      enabled: false
    }
  };
}

async function persistSiteSettings() {
  try {
    state.siteFeatures = collectSiteSettings();
    await saveSiteFeatures(state.siteFeatures);
    showOptionsToast("Site settings saved", "Site activation updated.", { type: "success" });
  } catch (error) {
    showOptionsToast("Save failed", error.message, { type: "error" });
  }
}

async function persistEngineSettings() {
  if (!validateEngineSettingsBeforeSave()) {
    return;
  }
  try {
    setStatus("Saving engine settings...");
    state.settings = normalizeSettings(collectEngineSettings());
    await callBackground("trench:save-settings", state.settings);
    await bumpBootstrapRevision();
    relockGlobalSettingsFields();
    showOptionsToast("Engine settings saved", "Changes applied.", { type: "success" });
    await refreshEngineData();
  } catch (error) {
    setStatus(error.message, true);
    showOptionsToast("Save failed", error.message, { type: "error" });
  }
}

// Auth-token creation lives on the backend now; no UI entrypoint here.

function openWalletEditModal(walletKey) {
  const existing = walletKey ? state.wallets.find((wallet) => wallet.key === walletKey) : null;
  const seedKey = existing?.key || walletKey || `new-${Date.now()}`;
  const initialEmoji = (existing?.emoji || "").trim() || defaultEmojiForKey(seedKey);
  state.walletEditModal = {
    open: true,
    editingKey: existing ? existing.key : null,
    emoji: initialEmoji,
    emojiPopoverOpen: false
  };
  renderWalletEditEmojiButton();
  renderWalletEditEmojiPopover();
  closeWalletEditEmojiPopover();
  elements.walletEditPrivateKey.value = "";
  elements.walletEditPrivateKeyRow.open = false;
  if (existing) {
    elements.walletEditModalTitle.textContent = "Edit wallet";
    elements.walletEditLabel.value = existing.label || "";
    elements.walletEditEnabled.checked = Boolean(existing.enabled);
    elements.walletEditPrivateKey.placeholder = "Paste a new base58 secret to replace this wallet";
    elements.walletEditPrivateKeyLabel.textContent = "New private key";
    elements.walletEditPrivateKeySummaryLabel.textContent = "Rotate private key (optional)";
    elements.walletEditPrivateKeyRow.classList.remove("hidden");
    elements.walletEditPublicKey.value = existing.publicKey || "";
    elements.walletEditPublicKeyRow.classList.toggle("hidden", !existing.publicKey);
    elements.walletEditModalDelete.classList.remove("hidden");
    elements.walletEditModalSave.textContent = "Save";
  } else {
    elements.walletEditModalTitle.textContent = "Add wallet";
    elements.walletEditLabel.value = "";
    elements.walletEditEnabled.checked = true;
    elements.walletEditPrivateKey.placeholder = "Paste wallet private key (base58)";
    elements.walletEditPrivateKeyLabel.textContent = "Private key";
    elements.walletEditPrivateKeySummaryLabel.textContent = "Private key";
    elements.walletEditPrivateKeyRow.classList.remove("hidden");
    elements.walletEditPrivateKeyRow.open = true;
    elements.walletEditPublicKey.value = "";
    elements.walletEditPublicKeyRow.classList.add("hidden");
    elements.walletEditModalDelete.classList.add("hidden");
    elements.walletEditModalSave.textContent = "Add wallet";
  }
  elements.walletEditModal.classList.remove("hidden");
  setTimeout(() => {
    elements.walletEditLabel.focus();
  }, 20);
}

function closeWalletEditModal() {
  state.walletEditModal = createEmptyWalletEditModalState();
  elements.walletEditModal.classList.add("hidden");
  closeWalletEditEmojiPopover();
}

async function saveWalletEditModal() {
  const { editingKey, emoji } = state.walletEditModal;
  const label = elements.walletEditLabel.value.trim();
  const privateKey = elements.walletEditPrivateKey.value.trim();
  const enabled = elements.walletEditEnabled.checked;
  const emojiValue = (emoji || "").trim();
  try {
    if (!editingKey) {
      if (!privateKey) {
        showOptionsToast("Private key required", "Paste a private key to add this wallet.", {
          type: "error"
        });
        return;
      }
      await callBackground("trench:create-wallet", {
        label: label || "",
        privateKey,
        enabled,
        emoji: emojiValue
      });
    } else {
      await callBackground("trench:update-wallet", {
        walletKey: editingKey,
        wallet: {
          label,
          privateKey: privateKey || undefined,
          enabled,
          emoji: emojiValue
        }
      });
    }
    await bumpBootstrapRevision();
    showOptionsToast(
      editingKey ? "Wallet updated" : "Wallet added",
      label || "Changes saved.",
      { type: "success" }
    );
    closeWalletEditModal();
    await refreshEngineData();
  } catch (error) {
    showOptionsToast("Save failed", error.message, { type: "error" });
  }
}

function toggleWalletEditEmojiPopover() {
  if (state.walletEditModal.emojiPopoverOpen) {
    closeWalletEditEmojiPopover();
  } else {
    openWalletEditEmojiPopover();
  }
}

function openWalletEditEmojiPopover() {
  state.walletEditModal.emojiPopoverOpen = true;
  elements.walletEditEmojiPopover.classList.remove("hidden");
  elements.walletEditEmojiButton.setAttribute("aria-expanded", "true");
  renderWalletEditEmojiPopover();
}

function closeWalletEditEmojiPopover() {
  state.walletEditModal.emojiPopoverOpen = false;
  elements.walletEditEmojiPopover.classList.add("hidden");
  elements.walletEditEmojiButton.setAttribute("aria-expanded", "false");
}

function setWalletEditEmoji(emoji) {
  state.walletEditModal.emoji = emoji || "";
  renderWalletEditEmojiButton();
  renderWalletEditEmojiPopover();
}

function renderWalletEditEmojiButton() {
  const current = state.walletEditModal.emoji || defaultEmojiForKey(state.walletEditModal.editingKey || "new");
  elements.walletEditEmojiGlyph.textContent = current;
}

function renderWalletEditEmojiPopover() {
  renderEmojiPickerInto(elements.walletEditEmojiPopover, state.walletEditModal);
}

function renderEmojiPickerInto(container, modalState) {
  if (!container) {
    return;
  }
  const currentEmoji = modalState.emoji || "";
  const categoryId = modalState.emojiCategoryId || EMOJI_CATALOG[0].id;
  const category = EMOJI_CATALOG.find((cat) => cat.id === categoryId) || EMOJI_CATALOG[0];
  const tabs = EMOJI_CATALOG.map((cat) => {
    const active = cat.id === category.id ? " active" : "";
    return `<button type="button" class="emoji-picker-tab${active}" data-emoji-category="${escapeAttribute(cat.id)}" title="${escapeAttribute(cat.name)}" aria-label="${escapeAttribute(cat.name)}">${cat.label}</button>`;
  }).join("");
  const items = category.emojis
    .map((emoji) => {
      const active = emoji === currentEmoji ? " active" : "";
      return `<button type="button" class="emoji-picker-option${active}" data-emoji="${escapeAttribute(emoji)}" aria-label="${escapeAttribute(emoji)}">${emoji}</button>`;
    })
    .join("");
  container.innerHTML = `
    <div class="emoji-picker-tabs" role="tablist">${tabs}</div>
    <div class="emoji-picker-grid">${items}</div>
  `;
}

async function deleteWalletFromEditModal() {
  const { editingKey } = state.walletEditModal;
  if (!editingKey) {
    return;
  }
  if (!window.confirm("Remove this wallet from the execution engine? This cannot be undone.")) {
    return;
  }
  try {
    await callBackground("trench:delete-wallet", { walletKey: editingKey });
    await bumpBootstrapRevision();
    showOptionsToast("Wallet removed", "Wallet deleted from the execution engine.", {
      type: "success"
    });
    closeWalletEditModal();
    await refreshEngineData();
  } catch (error) {
    showOptionsToast("Remove failed", error.message, { type: "error" });
  }
}

function createEmptyWalletEditModalState() {
  return {
    open: false,
    editingKey: null,
    emoji: "",
    emojiPopoverOpen: false,
    emojiCategoryId: "smileys"
  };
}

function createEmptyCreateGroupModalState() {
  return {
    open: false,
    editingGroupId: null,
    name: "",
    emoji: "",
    emojiPopoverOpen: false,
    emojiCategoryId: "smileys",
    selectedWalletKeys: new Set(),
    varianceEnabled: false,
    variancePercent: 10,
    staggerEnabled: false,
    staggerStrategy: "fixed",
    staggerMs: 150,
    staggerMinMs: 50,
    staggerMaxMs: 250
  };
}

function openCreateGroupModal(groupId) {
  const existing = groupId ? state.walletGroups.find((group) => group.id === groupId) : null;
  const policy = existing ? normalizeWalletGroupPolicy(existing.batchPolicy) : defaultWalletGroupPolicy();
  const fresh = createEmptyCreateGroupModalState();
  fresh.open = true;
  fresh.editingGroupId = existing ? existing.id : null;
  fresh.name = existing ? existing.label || existing.id : "";
  fresh.emoji = existing ? existing.emoji || "" : "";
  fresh.selectedWalletKeys = new Set(existing ? existing.walletKeys || [] : []);
  fresh.varianceEnabled = Boolean(policy.buyVariancePercent);
  fresh.variancePercent = policy.buyVariancePercent || 10;
  fresh.staggerEnabled = policy.transactionDelayMode !== "off";
  fresh.staggerStrategy = policy.transactionDelayStrategy === "random" ? "random" : "fixed";
  fresh.staggerMs = policy.transactionDelayMs || 150;
  fresh.staggerMinMs = policy.transactionDelayMinMs || 50;
  fresh.staggerMaxMs = policy.transactionDelayMaxMs || 250;
  state.createGroupModal = fresh;

  elements.createGroupModalTitle.textContent = existing ? "Edit wallet group" : "Create wallet group";
  elements.createGroupModalSave.textContent = existing ? "Save" : "Create";
  elements.createGroupModalDelete.classList.toggle("hidden", !existing);
  elements.createGroupNameInput.value = fresh.name;
  elements.createGroupVarianceToggle.checked = fresh.varianceEnabled;
  elements.createGroupVarianceSlider.value = String(fresh.variancePercent);
  elements.createGroupVarianceInput.value = String(fresh.variancePercent);
  elements.createGroupStaggerToggle.checked = fresh.staggerEnabled;
  elements.createGroupStaggerMs.value = String(fresh.staggerMs);
  elements.createGroupStaggerMinMs.value = String(fresh.staggerMinMs);
  elements.createGroupStaggerMaxMs.value = String(fresh.staggerMaxMs);
  renderCreateGroupWalletPicker();
  renderCreateGroupEmojiButton();
  closeCreateGroupEmojiPopover();
  syncCreateGroupModalVarianceVisibility();
  syncCreateGroupModalStaggerVisibility();
  elements.createGroupModal.classList.remove("hidden");
  void refreshWalletBalances();
  setTimeout(() => {
    elements.createGroupNameInput.focus();
  }, 20);
}

function closeCreateGroupModal() {
  state.createGroupModal = createEmptyCreateGroupModalState();
  elements.createGroupModal.classList.add("hidden");
  closeCreateGroupEmojiPopover();
}

function toggleCreateGroupEmojiPopover() {
  if (state.createGroupModal.emojiPopoverOpen) {
    closeCreateGroupEmojiPopover();
  } else {
    openCreateGroupEmojiPopover();
  }
}

function openCreateGroupEmojiPopover() {
  state.createGroupModal.emojiPopoverOpen = true;
  elements.createGroupEmojiPopover.classList.remove("hidden");
  elements.createGroupEmojiButton.setAttribute("aria-expanded", "true");
  renderCreateGroupEmojiPopover();
}

function closeCreateGroupEmojiPopover() {
  state.createGroupModal.emojiPopoverOpen = false;
  if (elements.createGroupEmojiPopover) {
    elements.createGroupEmojiPopover.classList.add("hidden");
  }
  if (elements.createGroupEmojiButton) {
    elements.createGroupEmojiButton.setAttribute("aria-expanded", "false");
  }
}

function setCreateGroupEmoji(emoji) {
  state.createGroupModal.emoji = emoji || "";
  renderCreateGroupEmojiButton();
  renderCreateGroupEmojiPopover();
}

function renderCreateGroupEmojiButton() {
  const current = state.createGroupModal.emoji
    || defaultEmojiForKey(state.createGroupModal.editingGroupId || state.createGroupModal.name || "group");
  elements.createGroupEmojiGlyph.textContent = current;
}

function renderCreateGroupEmojiPopover() {
  renderEmojiPickerInto(elements.createGroupEmojiPopover, state.createGroupModal);
}

function renderCreateGroupWalletPicker() {
  elements.createGroupWalletList.innerHTML = "";
  if (!state.wallets.length) {
    const empty = document.createElement("div");
    empty.className = "create-group-empty";
    empty.innerHTML = `<p class="muted">Add wallets first to assemble them into a group.</p>`;
    elements.createGroupWalletList.appendChild(empty);
    return;
  }
  state.wallets.forEach((wallet) => {
    const isSelected = state.createGroupModal.selectedWalletKeys.has(wallet.key);
    const label = formatWalletDisplayLabel(wallet, "Wallet");
    const avatarBg = walletAvatarGradient(wallet.key || wallet.publicKey || label);
    const emoji = walletDisplayEmoji(wallet);
    const row = document.createElement("label");
    row.className = "create-group-wallet-option";
    row.dataset.walletKey = wallet.key;
    if (isSelected) {
      row.classList.add("selected");
    }
    const balance = formatWalletBalance(state.walletBalances.get(wallet.key));
    const balanceTitle = balance ? escapeAttribute(balance.text) : "Balance pending";
    const balanceHtml = `<span class="create-group-wallet-balance${balance && balance.kind === "error" ? " is-error" : ""}${balance ? "" : " is-pending"}" title="${balanceTitle}">${buildBalanceChipInnerHtml(balance)}</span>`;
    const envSlot = walletEnvSlotTag(wallet.key);
    const envSlotHtml = envSlot
      ? `<span class="create-group-wallet-slot" title="${escapeAttribute(wallet.key)}">${escapeText(envSlot)}</span>`
      : "";
    row.innerHTML = `
      <input type="checkbox" ${isSelected ? "checked" : ""} />
      <span class="wallet-card-avatar wallet-card-avatar-sm" style="background:${avatarBg}">
        <span class="wallet-card-avatar-emoji">${escapeText(emoji)}</span>
      </span>
      <span class="create-group-wallet-copy">
        <span class="create-group-wallet-name-row">
          <span class="create-group-wallet-name">${escapeText(label)}</span>
          ${envSlotHtml}
        </span>
        <span class="create-group-wallet-sub muted">${escapeText(truncateMiddle(wallet.publicKey, 6, 4) || wallet.key || "")}</span>
      </span>
      ${balanceHtml}
    `;
    const checkbox = row.querySelector("input");
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) {
        state.createGroupModal.selectedWalletKeys.add(wallet.key);
        row.classList.add("selected");
      } else {
        state.createGroupModal.selectedWalletKeys.delete(wallet.key);
        row.classList.remove("selected");
      }
    });
    elements.createGroupWalletList.appendChild(row);
  });
}

function syncCreateGroupModalVarianceVisibility() {
  elements.createGroupVarianceSliderRow.classList.toggle(
    "hidden",
    !state.createGroupModal.varianceEnabled
  );
}

function syncCreateGroupModalStaggerVisibility() {
  const enabled = state.createGroupModal.staggerEnabled;
  const strategy = state.createGroupModal.staggerStrategy;
  elements.createGroupStaggerFields.classList.toggle("hidden", !enabled);
  elements.createGroupStaggerFixed.classList.toggle("hidden", !enabled || strategy !== "fixed");
  elements.createGroupStaggerRandom.classList.toggle("hidden", !enabled || strategy !== "random");
  for (const button of elements.createGroupStaggerStrategyButtons) {
    button.classList.toggle("active-chip", button.dataset.staggerStrategy === strategy);
  }
}

async function saveCreateGroupModal() {
  const modal = state.createGroupModal;
  const label = (elements.createGroupNameInput.value || "").trim();
  if (!label) {
    showOptionsToast("Name required", "Give the group a name before saving.", { type: "error" });
    return;
  }
  const walletKeys = Array.from(modal.selectedWalletKeys).filter(Boolean);
  if (!walletKeys.length) {
    showOptionsToast("No wallets selected", "Pick at least one wallet for this group.", {
      type: "error"
    });
    return;
  }
  const existingGroup = modal.editingGroupId
    ? state.walletGroups.find((group) => group.id === modal.editingGroupId)
    : null;
  const existingPolicy = existingGroup ? normalizeWalletGroupPolicy(existingGroup.batchPolicy) : null;
  const transactionDelayMode = modal.staggerEnabled
    ? (existingPolicy?.transactionDelayMode === "first_buy_only" ? "first_buy_only" : "on")
    : "off";
  const policy = normalizeWalletGroupPolicy({
    distributionMode: state.settings.defaultDistributionMode || "each",
    buyVariancePercent: modal.varianceEnabled ? Number(modal.variancePercent || 0) : 0,
    transactionDelayMode,
    transactionDelayStrategy: modal.staggerStrategy,
    transactionDelayMs: modal.staggerMs,
    transactionDelayMinMs: modal.staggerMinMs,
    transactionDelayMaxMs: modal.staggerMaxMs
  });
  try {
    if (modal.editingGroupId) {
      const payload = {
        id: modal.editingGroupId,
        label,
        walletKeys,
        batchPolicy: policy,
        emoji: (modal.emoji || "").trim(),
        color: existingGroup?.color || ""
      };
      await callBackground("trench:update-wallet-group", {
        groupId: modal.editingGroupId,
        group: payload
      });
    } else {
      const usedIds = new Set(state.walletGroups.map((group) => group.id));
      const nextId = nextUniqueSlug(label, "group", usedIds);
      await callBackground("trench:create-wallet-group", {
        id: nextId,
        label,
        walletKeys,
        batchPolicy: policy,
        emoji: (modal.emoji || "").trim()
      });
    }
    await bumpBootstrapRevision();
    showOptionsToast(
      modal.editingGroupId ? "Group updated" : "Group created",
      label,
      { type: "success" }
    );
    closeCreateGroupModal();
    await refreshEngineData();
  } catch (error) {
    showOptionsToast("Save failed", error.message, { type: "error" });
  }
}

async function deleteGroupFromCreateModal() {
  const { editingGroupId } = state.createGroupModal;
  if (!editingGroupId) {
    return;
  }
  if (!window.confirm("Delete this wallet group?")) {
    return;
  }
  try {
    await callBackground("trench:delete-wallet-group", { groupId: editingGroupId });
    await bumpBootstrapRevision();
    showOptionsToast("Group removed", "Wallet group deleted.", { type: "success" });
    closeCreateGroupModal();
    await refreshEngineData();
  } catch (error) {
    showOptionsToast("Remove failed", error.message, { type: "error" });
  }
}

function clearInvalidStateWithinModal(modalElement) {
  if (!modalElement) return;
  modalElement.querySelectorAll(".is-invalid").forEach((el) => el.classList.remove("is-invalid"));
}

function openPresetModal(presetId = null) {
  state.editingPresetId = presetId || null;
  const preset = state.editingPresetId
    ? state.presets.find((item) => item.id === state.editingPresetId) || emptyPreset()
    : emptyPreset();
  elements.presetModalTitle.textContent = state.editingPresetId ? "Edit preset" : "Create new preset";
  fillPresetModal(preset);
  clearInvalidStateWithinModal(elements.presetModal);
  state.presetModalOpen = true;
  elements.presetModal.classList.remove("hidden");
}

function closePresetModal() {
  state.presetModalOpen = false;
  state.editingPresetId = null;
  elements.presetModal.classList.add("hidden");
}

// Only Hello Moon actually consumes MEV-protection flags; every other route
// ignores them. The select is hidden when the chosen provider doesn't support
// it and the stored value is coerced back to "off" on save.
const MEV_CAPABLE_PROVIDERS = new Set(["hellomoon"]);

function providerSupportsMev(provider) {
  return MEV_CAPABLE_PROVIDERS.has(String(provider || "").trim().toLowerCase());
}

function updateMevVisibility(routeRowKey, providerSelect, mevSelect) {
  const row = document.querySelector(`[data-route-row="${routeRowKey}"]`);
  if (!row) return;
  const supported = providerSupportsMev(providerSelect?.value);
  row.classList.toggle("route-row--no-mev", !supported);
  if (!supported && mevSelect) {
    mevSelect.value = "off";
  }
}

function bindRouteMevToggle(routeRowKey, providerSelect, mevSelect) {
  if (!providerSelect || providerSelect.dataset.mevBound === "true") return;
  providerSelect.addEventListener("change", () => {
    updateMevVisibility(routeRowKey, providerSelect, mevSelect);
  });
  providerSelect.dataset.mevBound = "true";
}

function syncLaunchdeckPresetRouteFeeInputs(providerSelect, priorityInput, tipInput, autoFeeInput = null) {
  const provider = providerSelect?.value || "helius-sender";
  const autoFee = Boolean(autoFeeInput?.checked);
  if (priorityInput) {
    priorityInput.placeholder = DEFAULT_LAUNCHDECK_MANUAL_FEE_SOL;
    if (!autoFee) {
      priorityInput.value = normalizePriorityFeeForProvider(provider, priorityInput.value);
    }
  }
  if (tipInput) {
    const supportsTip = providerSupportsTip(provider);
    tipInput.placeholder = DEFAULT_LAUNCHDECK_MANUAL_FEE_SOL;
    if (!autoFee) {
      tipInput.value = normalizeTipForProvider(provider, tipInput.value);
    } else if (!supportsTip) {
      tipInput.value = "";
    }
    const label = tipInput.closest("label");
    if (label) label.hidden = !supportsTip;
  }
}

function syncLaunchdeckPresetModalFeeInputs() {
  syncLaunchdeckPresetRouteFeeInputs(
    elements.launchdeckPresetCreationProvider,
    elements.launchdeckPresetCreationFee,
    elements.launchdeckPresetCreationTip,
    elements.launchdeckPresetCreationAutoFee
  );
  syncLaunchdeckPresetRouteFeeInputs(
    elements.launchdeckPresetBuyProvider,
    elements.launchdeckPresetBuyFee,
    elements.launchdeckPresetBuyTip,
    elements.launchdeckPresetBuyAutoFee
  );
  syncLaunchdeckPresetRouteFeeInputs(
    elements.launchdeckPresetSellProvider,
    elements.launchdeckPresetSellFee,
    elements.launchdeckPresetSellTip,
    elements.launchdeckPresetSellAutoFee
  );
}

function bindLaunchdeckPresetRouteFeeInputs(providerSelect, priorityInput, tipInput, autoFeeInput = null) {
  if (!providerSelect || providerSelect.dataset.launchdeckFeeBound === "true") return;
  providerSelect.addEventListener("change", () => {
    syncLaunchdeckPresetRouteFeeInputs(providerSelect, priorityInput, tipInput, autoFeeInput);
  });
  providerSelect.dataset.launchdeckFeeBound = "true";
}

function autoFeeFallbackLabel(kind, enabled) {
  if (kind === "priority") return enabled ? "Fallback priority fee" : "Priority fee";
  if (kind === "tip") return enabled ? "Fallback tip" : "Tip";
  return "";
}

function syncAutoFeeFallbackLabels() {
  document.querySelectorAll("[data-auto-fee-label]").forEach((label) => {
    const checkbox = document.getElementById(label.dataset.autoFeeCheckbox || "");
    label.textContent = autoFeeFallbackLabel(label.dataset.autoFeeLabel, Boolean(checkbox?.checked));
  });
}

function bindAutoFeeFallbackLabels() {
  const checkboxIds = new Set(
    Array.from(document.querySelectorAll("[data-auto-fee-label]"))
      .map((label) => label.dataset.autoFeeCheckbox)
      .filter(Boolean)
  );
  checkboxIds.forEach((id) => {
    const checkbox = document.getElementById(id);
    if (!checkbox || checkbox.dataset.autoFeeLabelBound === "true") return;
    checkbox.addEventListener("change", () => {
      syncAutoFeeFallbackLabels();
      syncLaunchdeckPresetModalFeeInputs();
    });
    checkbox.dataset.autoFeeLabelBound = "true";
  });
  syncAutoFeeFallbackLabels();
}

function getBuyAmountStack() {
  return document.querySelector("[data-buy-amount-stack]");
}

function getBuyAmountRow(rowIndex) {
  return document.querySelector(`[data-buy-amount-row="${rowIndex}"]`);
}

function setBuyAmountRowsVisible(rows) {
  const stack = getBuyAmountStack();
  if (stack) {
    stack.dataset.buyAmountRows = String(rows === 2 ? 2 : 1);
  }
  const row2 = getBuyAmountRow(2);
  if (row2) {
    if (rows === 2) {
      row2.removeAttribute("hidden");
    } else {
      row2.setAttribute("hidden", "");
    }
  }
}

function currentBuyAmountRows() {
  const stack = getBuyAmountStack();
  if (!stack) return 1;
  return stack.dataset.buyAmountRows === "2" ? 2 : 1;
}

function clearSecondBuyAmountRowInputs() {
  const row2 = getBuyAmountRow(2);
  if (!row2) return;
  row2.querySelectorAll("[data-buy-amount-input]").forEach((input) => {
    input.value = "";
  });
}

function focusFirstSecondRowInput() {
  const row2 = getBuyAmountRow(2);
  if (!row2) return;
  const first = row2.querySelector("[data-buy-amount-input]");
  if (first && typeof first.focus === "function") {
    try {
      first.focus();
    } catch (_) {
      // ignore
    }
  }
}

function getSellPercentStack() {
  return document.querySelector("[data-sell-percent-stack]");
}

function getSellPercentRow(rowIndex) {
  return document.querySelector(`[data-sell-percent-row="${rowIndex}"]`);
}

function setSellPercentRowsVisible(rows) {
  const stack = getSellPercentStack();
  if (stack) {
    stack.dataset.sellPercentRows = String(rows === 2 ? 2 : 1);
  }
  const row2 = getSellPercentRow(2);
  if (row2) {
    if (rows === 2) {
      row2.removeAttribute("hidden");
    } else {
      row2.setAttribute("hidden", "");
    }
  }
}

function currentSellPercentRows() {
  const stack = getSellPercentStack();
  if (!stack) return 1;
  return stack.dataset.sellPercentRows === "2" ? 2 : 1;
}

function clearSecondSellPercentRowInputs() {
  const row2 = getSellPercentRow(2);
  if (!row2) return;
  row2.querySelectorAll("[data-sell-percent-input]").forEach((input) => {
    input.value = "";
  });
}

function focusFirstSecondSellRowInput() {
  const row2 = getSellPercentRow(2);
  if (!row2) return;
  const first = row2.querySelector("[data-sell-percent-input]");
  if (first && typeof first.focus === "function") {
    try {
      first.focus();
    } catch (_) {
      // ignore
    }
  }
}

function fillPresetModal(preset) {
  const rows = getPresetBuyAmountRows(preset);
  const sellRows = getPresetSellPercentRows(preset);
  const buyAmounts = getPresetBuyAmounts(preset);
  const sellPercents = getPresetSellAmounts(preset);
  elements.presetModalId.value = preset.id || "";
  elements.presetModalId.dataset.locked = preset.id ? "true" : "false";
  elements.presetModalLabel.value = preset.label || "";
  setBuyAmountRowsVisible(rows);
  setSellPercentRowsVisible(sellRows);
  elements.presetModalBuyAmounts.forEach((input, index) => {
    input.value = buyAmounts[index] || "";
  });
  elements.presetModalBuyAutoFee.checked = Boolean(preset.buyAutoTipEnabled);
  if (elements.presetModalBuyAutoTip) {
    elements.presetModalBuyAutoTip.value = preset.buyAutoTipEnabled ? "on" : "off";
  }
  elements.presetModalBuyMaxFee.value = preset.buyMaxFeeSol || "";
  elements.presetModalBuyFee.value = preset.buyFeeSol || "";
  elements.presetModalBuyTip.value = preset.buyTipSol || "";
  elements.presetModalBuySlippage.value = getBuySlippagePercent(preset);
  elements.presetModalBuyMevMode.value = getBuyMevMode(preset);
  elements.presetModalBuyProvider.value = selectableExtensionRouteProvider(preset.buyProvider);
  elements.presetModalSellPercents.forEach((input, index) => {
    input.value = sellPercents[index] || "";
  });
  elements.presetModalSellAutoFee.checked = Boolean(preset.sellAutoTipEnabled);
  if (elements.presetModalSellAutoTip) {
    elements.presetModalSellAutoTip.value = preset.sellAutoTipEnabled ? "on" : "off";
  }
  elements.presetModalSellMaxFee.value = preset.sellMaxFeeSol || "";
  elements.presetModalSellFee.value = preset.sellFeeSol || "";
  elements.presetModalSellTip.value = preset.sellTipSol || "";
  elements.presetModalSellSlippage.value = getSellSlippagePercent(preset);
  elements.presetModalSellMevMode.value = getSellMevMode(preset);
  elements.presetModalSellProvider.value = selectableExtensionRouteProvider(preset.sellProvider);
  // Endpoint profile is inherited from the global region now; preserve any
  // stored value so saves remain lossless.
  elements.presetModalBuyEndpointProfile.value = preset.buyEndpointProfile || "";
  elements.presetModalSellEndpointProfile.value = preset.sellEndpointProfile || "";
  applyRouteProviderAvailability(elements.presetModalBuyProvider);
  applyRouteProviderAvailability(elements.presetModalSellProvider);

  bindRouteMevToggle("preset-buy", elements.presetModalBuyProvider, elements.presetModalBuyMevMode);
  bindRouteMevToggle("preset-sell", elements.presetModalSellProvider, elements.presetModalSellMevMode);
  updateMevVisibility("preset-buy", elements.presetModalBuyProvider, elements.presetModalBuyMevMode);
  updateMevVisibility("preset-sell", elements.presetModalSellProvider, elements.presetModalSellMevMode);
  syncAutoFeeFallbackLabels();
}

function collectPresetModal() {
  const allBuyAmounts = elements.presetModalBuyAmounts.map((input) => input.value.trim());
  let rows = currentBuyAmountRows();
  let buyAmountsSol = allBuyAmounts.slice(0, rows * 4);
  while (buyAmountsSol.length < rows * 4) {
    buyAmountsSol.push("");
  }
  // Auto-collapse: if row 2 is visible but every row-2 input is empty,
  // shrink back to a single 4-entry row before saving.
  if (rows === 2 && buyAmountsSol.slice(4, 8).every((value) => value === "")) {
    rows = 1;
    buyAmountsSol = buyAmountsSol.slice(0, 4);
  }
  const allSellPercents = elements.presetModalSellPercents.map((input) => input.value.trim());
  let sellRows = currentSellPercentRows();
  let sellAmountsPercent = allSellPercents.slice(0, sellRows * 4);
  while (sellAmountsPercent.length < sellRows * 4) {
    sellAmountsPercent.push("");
  }
  // Same auto-collapse rule for sells: if row 2 was opened but is entirely
  // empty, save it as a single-row preset.
  if (sellRows === 2 && sellAmountsPercent.slice(4, 8).every((value) => value === "")) {
    sellRows = 1;
    sellAmountsPercent = sellAmountsPercent.slice(0, 4);
  }
  const label = elements.presetModalLabel.value.trim();
  const existingId = elements.presetModalId.value.trim();
  return {
    // Keep the existing id on edit; otherwise derive from the label.
    id: existingId || slugifyKey(label, "preset"),
    label,
    buyAmountSol: buyAmountsSol.find(Boolean) || "",
    sellPercent: sellAmountsPercent.find(Boolean) || "",
    buyAmountsSol,
    buyAmountRows: rows,
    sellAmountsPercent,
    sellPercentRows: sellRows,
    buyAutoTipEnabled: Boolean(elements.presetModalBuyAutoFee.checked),
    buyMaxFeeSol: elements.presetModalBuyMaxFee.value.trim(),
    buyFeeSol: elements.presetModalBuyFee.value.trim(),
    buyTipSol: elements.presetModalBuyTip.value.trim(),
    buySlippagePercent: elements.presetModalBuySlippage.value.trim(),
    buyMevMode: elements.presetModalBuyMevMode.value.trim(),
    buyProvider: selectableExtensionRouteProvider(elements.presetModalBuyProvider.value),
    buyEndpointProfile: elements.presetModalBuyEndpointProfile.value.trim(),
    sellAutoTipEnabled: Boolean(elements.presetModalSellAutoFee.checked),
    sellMaxFeeSol: elements.presetModalSellMaxFee.value.trim(),
    sellFeeSol: elements.presetModalSellFee.value.trim(),
    sellTipSol: elements.presetModalSellTip.value.trim(),
    sellSlippagePercent: elements.presetModalSellSlippage.value.trim(),
    sellMevMode: elements.presetModalSellMevMode.value.trim(),
    sellProvider: selectableExtensionRouteProvider(elements.presetModalSellProvider.value),
    sellEndpointProfile: elements.presetModalSellEndpointProfile.value.trim()
  };
}

function validateExecutionPreset(preset) {
  const errors = [];
  const push = (fieldId, message) => errors.push({ fieldId, message });

  if (!preset.label) push("preset-modal-label", "Preset name is required.");

  const validateSide = (side) => {
    const isBuy = side === "buy";
    const providerFieldId = isBuy ? "preset-modal-buy-provider" : "preset-modal-sell-provider";
    const feeFieldId = isBuy ? "preset-modal-buy-fee" : "preset-modal-sell-fee";
    const tipFieldId = isBuy ? "preset-modal-buy-tip" : "preset-modal-sell-tip";
    const slipFieldId = isBuy ? "preset-modal-buy-slippage" : "preset-modal-sell-slippage";
    const maxFeeFieldId = isBuy ? "preset-modal-buy-max-fee" : "preset-modal-sell-max-fee";
    const provider = isBuy ? preset.buyProvider : preset.sellProvider;
    const feeSol = isBuy ? preset.buyFeeSol : preset.sellFeeSol;
    const tipSol = isBuy ? preset.buyTipSol : preset.sellTipSol;
    const slippagePercent = isBuy ? preset.buySlippagePercent : preset.sellSlippagePercent;
    const maxFeeSol = isBuy ? preset.buyMaxFeeSol : preset.sellMaxFeeSol;
    const autoFee = isBuy ? preset.buyAutoTipEnabled : preset.sellAutoTipEnabled;
    const label = providerLabel(provider);
    const sideLabel = isBuy ? "Buy" : "Sell";
    const minTip = providerMinimumTipSol(provider);
    const supportsTip = providerSupportsTip(provider);
    const requiresPriority = providerRequiresPriorityFee(provider);

    if (!provider) push(providerFieldId, `${sideLabel} route is required.`);
    if (!slippagePercent) {
      push(slipFieldId, `${sideLabel} slippage % is required.`);
    }
    if (requiresPriority) {
      if (!feeSol && !autoFee) {
        push(feeFieldId, `Priority fee is required for ${label}.`);
      } else if (feeSol && !(Number(feeSol) > 0)) {
        push(feeFieldId, `Priority fee must be greater than 0 for ${label}.`);
      }
    }
    if (supportsTip) {
      if (!tipSol && !autoFee) {
        push(tipFieldId, `Tip is required for ${label}.`);
      } else if (tipSol && (Number.isNaN(Number(tipSol)) || Number(tipSol) < 0)) {
        push(tipFieldId, "Tip must be a valid number.");
      } else if (tipSol && minTip > 0 && Number(tipSol) < minTip) {
        push(tipFieldId, `Tip must be at least ${formatMinSol(minTip)} SOL for ${label}.`);
      }
    }
    if (autoFee) {
      if (!maxFeeSol) {
        push(maxFeeFieldId, `Max Auto Fee is required when Auto fee is on.`);
      } else if (!(Number(maxFeeSol) > 0)) {
        push(maxFeeFieldId, `Max Auto Fee must be greater than 0.`);
      } else if (minTip > 0 && Number(maxFeeSol) < minTip) {
        push(maxFeeFieldId, `Max Auto Fee must be at least ${formatMinSol(minTip)} SOL for ${label} because it includes both priority fee and tip.`);
      }
    }
  };

  validateSide("buy");
  validateSide("sell");
  return errors;
}

async function savePresetFromModal() {
  try {
    const preset = normalizePreset(collectPresetModal());
    const errors = validateExecutionPreset(preset);
    markFieldsInvalid(errors);
    if (errors.length > 0) {
      showOptionsToast("Preset not saved", summarizeValidationErrors(errors), { type: "error" });
      return;
    }
    if (state.editingPresetId) {
      await callBackground("trench:update-preset", {
        presetId: state.editingPresetId,
        preset
      });
    } else {
      await callBackground("trench:create-preset", preset);
    }
    await bumpBootstrapRevision();
    showOptionsToast("Preset saved", preset.label || preset.id, { type: "success" });
    closePresetModal();
    await refreshEngineData();
  } catch (error) {
    showOptionsToast("Save failed", error.message, { type: "error" });
  }
}

function fieldValue(card, fieldName) {
  const element = card.querySelector(`[data-field="${fieldName}"]`);
  return element ? element.value.trim() : "";
}

function fieldChecked(card, fieldName) {
  const element = card.querySelector(`[data-field="${fieldName}"]`);
  return Boolean(element?.checked);
}

function normalizeBootstrap(bootstrap) {
  const normalized = {
    ...emptyBootstrap(),
    ...(bootstrap || {})
  };
  normalized.capabilities ||= emptyBootstrap().capabilities;
  normalized.settings = normalizeSettings(normalized.settings || {});
  return normalized;
}

function emptyBootstrap() {
  return {
    version: "0.2.0",
    dataRoot: ".local/execution-engine",
    capabilities: {
      platforms: ["axiom", "j7"],
      supportsBatchPreview: true,
      supportsBatchStatus: true,
      supportsResourceEditing: true
    },
    settings: emptySettings(),
    presets: [],
    config: createDefaultLaunchdeckConfig(),
    providers: {},
    providerRegistry: [],
    wallets: [],
    walletGroups: []
  };
}

function emptySettings() {
  return {
    defaultBuySlippagePercent: "20",
    defaultSellSlippagePercent: "20",
    defaultBuyMevMode: "off",
    defaultSellMevMode: "off",
    executionProvider: "helius-sender",
    executionEndpointProfile: "",
    executionCommitment: "confirmed",
    executionSkipPreflight: true,
    trackSendBlockHeight: false,
    allowNonCanonicalPoolTrades: false,
    maxActiveBatches: 32,
    rpcUrl: "",
    wsUrl: "",
    warmRpcUrl: "",
    warmWsUrl: "",
    sharedRegion: "",
    heliusRpcUrl: "",
    heliusWsUrl: "",
    standardRpcSendUrls: [],
    heliusSenderRegion: "",
    defaultDistributionMode: "each",
    pnlTrackingMode: "local",
    pnlIncludeFees: true,
    wrapperDefaultFeeBps: 10
  };
}

function collectEngineSettings() {
  const region = resolveSelectedRegion();
  return {
    defaultBuySlippagePercent: elements.engineBuySlippage.value.trim(),
    defaultSellSlippagePercent: elements.engineSellSlippage.value.trim(),
    defaultBuyMevMode: elements.engineBuyMevMode.value.trim(),
    defaultSellMevMode: elements.engineSellMevMode.value.trim(),
    executionProvider: selectableExtensionRouteProvider(elements.engineProvider.value),
    executionEndpointProfile: region,
    executionCommitment: "confirmed",
    executionSkipPreflight: true,
    trackSendBlockHeight: false,
    allowNonCanonicalPoolTrades: Boolean(
      elements.engineAllowNonCanonicalPoolTrades?.checked
    ),
    maxActiveBatches: Number(elements.engineMaxActiveBatches.value || 0),
    rpcUrl: elements.engineRpcUrl.value.trim(),
    wsUrl: elements.engineWsUrl.value.trim(),
    warmRpcUrl: elements.engineWarmRpcUrl.value.trim(),
    warmWsUrl: elements.engineWarmWsUrl?.value.trim() || "",
    // The region control is the single source of truth for where the user
    // wants to route. We mirror the same value into sharedRegion /
    // heliusSenderRegion so the engine does not need independent overrides.
    sharedRegion: region,
    heliusRpcUrl: elements.engineHeliusRpcUrl.value.trim(),
    heliusWsUrl: elements.engineHeliusWsUrl.value.trim(),
    standardRpcSendUrls: elements.engineStandardRpcSendUrls.value
      .split(",")
      .map((value) => value.trim())
      .filter(Boolean),
    heliusSenderRegion: region,
    defaultDistributionMode: state.settings?.defaultDistributionMode || "each",
    pnlTrackingMode:
      String(elements.enginePnlTrackingMode?.value || "local").trim() === "rpc" ? "rpc" : "local",
    pnlIncludeFees: Boolean(elements.enginePnlIncludeFees?.checked),
    wrapperDefaultFeeBps: clampWrapperFeeBps(
      elements.engineWrapperDefaultFeeBps?.value
    )
  };
}

function clampWrapperFeeBps(value) {
  // Mirror the on-chain allow-list (0, 10, 20) so the options form never
  // tries to persist a tier the engine/program will reject.
  if (value === null || value === undefined) return 10;
  const text = String(value).trim();
  if (!text) return 10;
  const raw = Number(text);
  if (!Number.isFinite(raw)) return 10;
  if (raw <= 0) return 0;
  if (raw <= 10) return 10;
  return 20;
}

function normalizeSettings(settings) {
  const configuredRegion = resolveConfiguredRegion(settings);
  return {
    ...emptySettings(),
    ...(settings || {}),
    defaultBuySlippagePercent: String(settings?.defaultBuySlippagePercent ?? "20").trim() || "20",
    defaultSellSlippagePercent: String(settings?.defaultSellSlippagePercent ?? "20").trim() || "20",
    defaultBuyMevMode: String(settings?.defaultBuyMevMode ?? "off").trim() || "off",
    defaultSellMevMode: String(settings?.defaultSellMevMode ?? "off").trim() || "off",
    executionProvider: selectableExtensionRouteProvider(settings?.executionProvider),
    executionEndpointProfile: configuredRegion,
    executionCommitment: "confirmed",
    executionSkipPreflight: true,
    trackSendBlockHeight: false,
    allowNonCanonicalPoolTrades: Boolean(settings?.allowNonCanonicalPoolTrades),
    maxActiveBatches: Math.max(1, Number(settings?.maxActiveBatches || 32)),
    rpcUrl: String(settings?.rpcUrl ?? "").trim(),
    wsUrl: String(settings?.wsUrl ?? "").trim(),
    warmRpcUrl: String(settings?.warmRpcUrl ?? "").trim(),
    warmWsUrl: String(settings?.warmWsUrl ?? "").trim(),
    sharedRegion: configuredRegion,
    heliusRpcUrl: String(settings?.heliusRpcUrl ?? "").trim(),
    heliusWsUrl: String(settings?.heliusWsUrl ?? "").trim(),
    standardRpcSendUrls: Array.isArray(settings?.standardRpcSendUrls)
      ? settings.standardRpcSendUrls.map((value) => String(value || "").trim()).filter(Boolean)
      : [],
    heliusSenderRegion: configuredRegion,
    defaultDistributionMode:
      String(settings?.defaultDistributionMode ?? "each").trim() === "split" ? "split" : "each",
    pnlTrackingMode:
      String(settings?.pnlTrackingMode ?? "local").trim() === "rpc" ? "rpc" : "local",
    pnlIncludeFees: settings?.pnlIncludeFees !== false,
    wrapperDefaultFeeBps: clampWrapperFeeBps(settings?.wrapperDefaultFeeBps)
  };
}

function syncBootstrapPreview() {
  const summary = {
    ...state.bootstrap,
    settings: state.settings,
    presets: state.presets,
    launchdeckConfig: state.launchdeckConfig,
    wallets: state.wallets,
    walletGroups: state.walletGroups
  };
  if (elements.bootstrapJson) {
    elements.bootstrapJson.textContent = JSON.stringify(summary, null, 2);
  }
}

async function bumpBootstrapRevision() {
  await chrome.storage.local.set({ [BOOTSTRAP_REVISION_KEY]: Date.now() });
}

async function bumpWalletStatusRevision() {
  await chrome.storage.local.set({ [WALLET_STATUS_REVISION_KEY]: Date.now() });
}

function setStatus(message, isError = false) {
  elements.hostStatusBadge.textContent = message;
  elements.hostStatusBadge.style.color = isError ? "#fca5a5" : "#d4d4d8";
}

function formatConnectionStatusMessage(error = null) {
  const configuredHost = String(elements.hostInput?.value || "").trim();
  const configuredToken = String(elements.hostAuthTokenInput?.value || "").trim();
  if (error) {
    const message = String(error?.message || error || "Execution host unavailable.");
    if (!configuredHost) {
      return "No execution host configured yet. Save a host URL and token to connect the extension.";
    }
    if (!configuredToken) {
      return `Host ${configuredHost} is saved, but no engine access token is configured yet.`;
    }
    if (/missing for .*grant access first/i.test(message)) {
      return `${message} Open Global Settings and grant the remote host permission first.`;
    }
    if (/unauthorized|missing bearer token/i.test(message.toLowerCase())) {
      return `Connected host rejected the current token. Re-enter the engine access token in Global Settings.`;
    }
    if (/unreachable|timed out|refused/i.test(message.toLowerCase())) {
      return `Could not reach ${configuredHost}. Verify the host is running and reachable from this browser profile.`;
    }
    return message;
  }
  if (!configuredHost) {
    return "No execution host configured yet. Save the local engine URL or a remote HTTPS host to begin.";
  }
  if (!configuredToken) {
    return `Host ${configuredHost} is saved. Enter the engine access token to finish pairing the extension.`;
  }
  return `Execution host reachable (${state.health?.runtimeMode || "ready"}).`;
}

function formatLaunchdeckConnectionStatusMessage(error = null) {
  const configuredHost = String(elements.launchdeckHostInput?.value || "").trim();
  const configuredToken = String(elements.hostAuthTokenInput?.value || "").trim();
  if (error) {
    const message = String(error?.message || error || "LaunchDeck host unavailable.");
    if (!configuredHost) {
      return "No LaunchDeck host configured yet. Save the local LaunchDeck URL or a remote HTTPS host to begin.";
    }
    if (!configuredToken) {
      return `Host ${configuredHost} is saved, but no shared access token is configured yet.`;
    }
    if (/missing for .*grant access first/i.test(message)) {
      return `${message} Open Global Settings and grant the remote host permission first.`;
    }
    if (/unauthorized|missing bearer token|engine request/i.test(message.toLowerCase())) {
      return "Connected LaunchDeck host rejected the current token. Re-enter the shared access token in Global Settings.";
    }
    if (/https when it is not loopback/i.test(message.toLowerCase())) {
      return message;
    }
    if (/unreachable|timed out|refused|failed/i.test(message.toLowerCase())) {
      return `Could not reach ${configuredHost}. Verify the LaunchDeck host is running and reachable from this browser profile.`;
    }
    return message;
  }
  if (!configuredHost) {
    return "No LaunchDeck host configured yet. Save the local LaunchDeck URL or a remote HTTPS host to begin.";
  }
  if (!configuredToken) {
    return `LaunchDeck host ${configuredHost} is saved. Enter the shared access token to finish pairing.`;
  }
  return `LaunchDeck host reachable (${configuredHost}).`;
}

function renderOptions(values, selected) {
  return values
    .map(
      (value) =>
        `<option value="${value}" ${String(selected) === value ? "selected" : ""}>${value}</option>`
    )
    .join("");
}

function emptyPreset() {
  return {
    id: "",
    label: "",
    buyAmountSol: "",
    sellPercent: "",
    buyAmountsSol: ["", "", "", ""],
    sellAmountsPercent: ["", "", "", ""],
    buyAutoTipEnabled: false,
    buyMaxFeeSol: "",
    buyFeeSol: "",
    buyTipSol: "",
    buySlippagePercent: "",
    buyMevMode: "off",
    buyProvider: "helius-sender",
    buyEndpointProfile: "",
    sellAutoTipEnabled: false,
    sellMaxFeeSol: "",
    sellFeeSol: "",
    sellTipSol: "",
    sellSlippagePercent: "",
    sellMevMode: "off",
    sellProvider: "helius-sender",
    sellEndpointProfile: "",
    slippagePercent: "",
    mevMode: "off"
  };
}

function defaultWallet(index) {
  return {
    key: `draft-${Date.now()}-${index}`,
    label: "",
    publicKey: "",
    enabled: true,
    privateKey: "",
    isNew: true
  };
}

function defaultWalletGroup(index) {
  return {
    id: `draft-group-${Date.now()}-${index}`,
    label: `Group ${index}`,
    walletKeys: [],
    batchPolicy: defaultWalletGroupPolicy(),
    isNew: true
  };
}

function escapeAttribute(value) {
  return String(value || "").replaceAll("&", "&amp;").replaceAll("\"", "&quot;");
}

function escapeText(value) {
  return String(value || "").replaceAll("&", "&amp;").replaceAll("<", "&lt;");
}

function normalizePreset(preset) {
  let buyAmountRows = getPresetBuyAmountRows(preset);
  let buyAmountsSol = normalizePresetValues(
    preset?.buyAmountsSol,
    preset?.buyAmountSol,
    buyAmountRows * 4
  );
  if (buyAmountRows === 2 && buyAmountsSol.slice(4, 8).every((value) => !value)) {
    buyAmountRows = 1;
    buyAmountsSol = buyAmountsSol.slice(0, 4);
  }
  let sellPercentRows = getPresetSellPercentRows(preset);
  let sellAmountsPercent = normalizePresetValues(
    preset?.sellAmountsPercent,
    preset?.sellPercent,
    sellPercentRows * 4
  );
  if (sellPercentRows === 2 && sellAmountsPercent.slice(4, 8).every((value) => !value)) {
    sellPercentRows = 1;
    sellAmountsPercent = sellAmountsPercent.slice(0, 4);
  }
  return {
    buyAutoTipEnabled: false,
    buyMaxFeeSol: "",
    buyFeeSol: "",
    buyTipSol: "",
    buySlippagePercent: "",
    buyMevMode: "off",
    buyProvider: "",
    buyEndpointProfile: "",
    sellAutoTipEnabled: false,
    sellMaxFeeSol: "",
    sellFeeSol: "",
    sellTipSol: "",
    sellSlippagePercent: "",
    sellMevMode: "off",
    sellProvider: "",
    sellEndpointProfile: "",
    slippagePercent: "",
    mevMode: "off",
    ...preset,
    id: String(preset?.id || "").trim(),
    label: String(preset?.label || "").trim(),
    buyMaxFeeSol: String(preset?.buyMaxFeeSol ?? "").trim(),
    buySlippagePercent: String(preset?.buySlippagePercent ?? preset?.slippagePercent ?? "").trim(),
    buyMevMode: String(preset?.buyMevMode ?? preset?.mevMode ?? "off").trim() || "off",
    buyProvider: selectableExtensionRouteProvider(preset?.buyProvider),
    buyEndpointProfile: String(preset?.buyEndpointProfile ?? "").trim(),
    sellMaxFeeSol: String(preset?.sellMaxFeeSol ?? "").trim(),
    sellSlippagePercent: String(preset?.sellSlippagePercent ?? preset?.slippagePercent ?? "").trim(),
    sellMevMode: String(preset?.sellMevMode ?? preset?.mevMode ?? "off").trim() || "off",
    sellProvider: selectableExtensionRouteProvider(preset?.sellProvider),
    sellEndpointProfile: String(preset?.sellEndpointProfile ?? "").trim(),
    slippagePercent: String(preset?.buySlippagePercent ?? preset?.slippagePercent ?? "").trim(),
    mevMode: String(preset?.buyMevMode ?? preset?.mevMode ?? "off").trim() || "off",
    buyAmountSol: buyAmountsSol.find(Boolean) || "",
    sellPercent: sellAmountsPercent.find(Boolean) || "",
    buyAmountsSol,
    buyAmountRows,
    sellAmountsPercent,
    sellPercentRows
  };
}

function normalizeWallet(wallet) {
  return {
    key: String(wallet?.key || "").trim(),
    label: String(wallet?.label || "").trim(),
    publicKey: String(wallet?.publicKey || "").trim(),
    enabled: Boolean(wallet?.enabled),
    privateKey: String(wallet?.privateKey || "").trim(),
    isNew: Boolean(wallet?.isNew)
  };
}

function normalizeWalletGroup(group) {
  return {
    id: String(group?.id || "").trim(),
    label: String(group?.label || "").trim(),
    walletKeys: Array.isArray(group?.walletKeys) ? group.walletKeys.map((value) => String(value || "").trim()).filter(Boolean) : [],
    batchPolicy: normalizeWalletGroupPolicy(group?.batchPolicy || {}),
    emoji: String(group?.emoji || "").trim(),
    isNew: Boolean(group?.isNew)
  };
}

function defaultWalletGroupPolicy() {
  return {
    distributionMode: "each",
    buyVariancePercent: 0,
    transactionDelayMode: "off",
    transactionDelayStrategy: "fixed",
    transactionDelayMs: 0,
    transactionDelayMinMs: 0,
    transactionDelayMaxMs: 250
  };
}

function normalizeWalletGroupPolicy(policy) {
  const normalized = {
    ...defaultWalletGroupPolicy(),
    ...(policy || {})
  };
  normalized.distributionMode = normalized.distributionMode === "split" ? "split" : "each";
  normalized.buyVariancePercent = Math.max(0, Math.min(100, Number(normalized.buyVariancePercent || 0) || 0));
  normalized.transactionDelayMode = ["off", "on", "first_buy_only"].includes(normalized.transactionDelayMode)
    ? normalized.transactionDelayMode
    : "off";
  normalized.transactionDelayStrategy = normalized.transactionDelayStrategy === "random" ? "random" : "fixed";
  normalized.transactionDelayMs = Math.min(
    MAX_TRANSACTION_DELAY_MS,
    Math.max(0, Number(normalized.transactionDelayMs || 0) || 0)
  );
  normalized.transactionDelayMinMs = Math.min(
    MAX_TRANSACTION_DELAY_MS,
    Math.max(0, Number(normalized.transactionDelayMinMs || 0) || 0)
  );
  normalized.transactionDelayMaxMs = Math.max(
    normalized.transactionDelayMinMs,
    Number(normalized.transactionDelayMaxMs || normalized.transactionDelayMinMs || 0) || 0
  );
  normalized.transactionDelayMaxMs = Math.min(MAX_TRANSACTION_DELAY_MS, normalized.transactionDelayMaxMs);
  if (normalized.transactionDelayMode === "off") {
    normalized.transactionDelayMs = 0;
    normalized.transactionDelayMinMs = 0;
    normalized.transactionDelayMaxMs = 0;
  }
  return normalized;
}

function createDefaultLaunchdeckConfig() {
  return {
    defaults: {
      launchpad: "pump",
      mode: "regular",
      activePresetId: "",
      presetEditing: false,
      quickDevBuyAmounts: ["0.5", "1", "2"],
      misc: {
        trackSendBlockHeight: false,
        allowNonCanonicalPoolTrades: false
      }
    },
    presets: {
      items: []
    }
  };
}

function createEmptyLaunchdeckPreset(index = 0) {
  return {
    id: `preset${index + 1}`,
    label: "",
    creationSettings: {
      provider: "helius-sender",
      endpointProfile: "",
      priorityFeeSol: "0.001",
      tipSol: "0.001",
      autoFee: false,
      maxFeeSol: "",
      devBuySol: "",
      mevMode: "off"
    },
    buySettings: {
      provider: "helius-sender",
      endpointProfile: "",
      priorityFeeSol: "0.001",
      tipSol: "0.001",
      slippagePercent: "20",
      autoFee: false,
      maxFeeSol: "",
      mevMode: "off",
      snipeBuyAmountSol: ""
    },
    sellSettings: {
      provider: "helius-sender",
      endpointProfile: "",
      priorityFeeSol: "0.001",
      tipSol: "0.001",
      slippagePercent: "20",
      autoFee: false,
      maxFeeSol: "",
      mevMode: "off"
    },
    postLaunchStrategy: "none"
  };
}

function normalizeLaunchdeckPreset(preset, index = 0) {
  const fallback = createEmptyLaunchdeckPreset(index);
  const creationProvider = selectableExtensionRouteProvider(
    preset?.creationSettings?.provider || fallback.creationSettings.provider
  );
  const buyProvider = selectableExtensionRouteProvider(
    preset?.buySettings?.provider || fallback.buySettings.provider
  );
  const sellProvider = selectableExtensionRouteProvider(
    preset?.sellSettings?.provider || fallback.sellSettings.provider
  );
  const creationAutoFee = Boolean(preset?.creationSettings?.autoFee);
  const buyAutoFee = Boolean(preset?.buySettings?.autoFee);
  const sellAutoFee = Boolean(preset?.sellSettings?.autoFee);
  const normalizePriorityForAutoFee = (provider, value, autoFee) =>
    autoFee ? String(value ?? "").trim() : normalizePriorityFeeForProvider(provider, value);
  const normalizeTipForAutoFee = (provider, value, autoFee) =>
    autoFee
      ? (providerSupportsTip(provider) ? String(value ?? "").trim() : "")
      : normalizeTipForProvider(provider, value);
  const normalized = {
    ...fallback,
    ...(preset || {}),
    id: String(preset?.id || fallback.id).trim() || fallback.id,
    label: String(preset?.label || fallback.label).trim(),
    creationSettings: {
      ...fallback.creationSettings,
      ...(preset?.creationSettings || {}),
      provider: creationProvider,
      devBuySol: String(preset?.creationSettings?.devBuySol || "").trim(),
      priorityFeeSol: normalizePriorityForAutoFee(creationProvider, preset?.creationSettings?.priorityFeeSol, creationAutoFee),
      tipSol: normalizeTipForAutoFee(creationProvider, preset?.creationSettings?.tipSol, creationAutoFee),
      mevMode: String(preset?.creationSettings?.mevMode || fallback.creationSettings.mevMode).trim() || "off"
    },
    buySettings: {
      ...fallback.buySettings,
      ...(preset?.buySettings || {}),
      provider: buyProvider,
      priorityFeeSol: normalizePriorityForAutoFee(buyProvider, preset?.buySettings?.priorityFeeSol, buyAutoFee),
      tipSol: normalizeTipForAutoFee(buyProvider, preset?.buySettings?.tipSol, buyAutoFee),
      slippagePercent: String(preset?.buySettings?.slippagePercent || fallback.buySettings.slippagePercent).trim(),
      mevMode: String(preset?.buySettings?.mevMode || fallback.buySettings.mevMode).trim() || "off",
      snipeBuyAmountSol: String(preset?.buySettings?.snipeBuyAmountSol || "").trim()
    },
    sellSettings: {
      ...fallback.sellSettings,
      ...(preset?.sellSettings || {}),
      provider: sellProvider,
      priorityFeeSol: normalizePriorityForAutoFee(sellProvider, preset?.sellSettings?.priorityFeeSol, sellAutoFee),
      tipSol: normalizeTipForAutoFee(sellProvider, preset?.sellSettings?.tipSol, sellAutoFee),
      slippagePercent: String(preset?.sellSettings?.slippagePercent || fallback.sellSettings.slippagePercent).trim(),
      mevMode: String(preset?.sellSettings?.mevMode || fallback.sellSettings.mevMode).trim() || "off"
    }
  };
  delete normalized.buyAmountsSol;
  delete normalized.sellAmountsPercent;
  delete normalized.buyAmountRows;
  delete normalized.sellPercentRows;
  return normalized;
}

function normalizeLaunchdeckConfig(config) {
  const fallback = createDefaultLaunchdeckConfig();
  const items = Array.isArray(config?.presets?.items) ? config.presets.items : [];
  const configuredQuickDevBuyAmounts = Array.isArray(config?.defaults?.quickDevBuyAmounts)
    ? config.defaults.quickDevBuyAmounts
    : null;
  const legacyQuickDevBuyAmounts = items.map((entry) =>
    String(entry?.creationSettings?.devBuySol || "").trim()
  );
  const quickDevBuyAmounts = fallback.defaults.quickDevBuyAmounts.map((fallbackAmount, index) => {
    const configuredAmount = configuredQuickDevBuyAmounts
      ? String(configuredQuickDevBuyAmounts[index] || "").trim()
      : "";
    if (configuredAmount) return configuredAmount;
    const legacyAmount = configuredQuickDevBuyAmounts
      ? ""
      : String(legacyQuickDevBuyAmounts[index] || "").trim();
    return legacyAmount || fallbackAmount;
  });
  const normalized = {
    ...fallback,
    ...(config || {}),
    defaults: {
      ...fallback.defaults,
      ...(config?.defaults || {}),
      quickDevBuyAmounts
    }
  };
  normalized.presets = {
    items: items.map((entry, index) => normalizeLaunchdeckPreset(entry, index))
  };
  const requestedActivePresetId = String(config?.defaults?.activePresetId || "").trim();
  normalized.defaults.activePresetId = normalized.presets.items.some((entry) => entry.id === requestedActivePresetId)
    ? requestedActivePresetId
    : (normalized.presets.items[0]?.id || "");
  return normalized;
}

function getLaunchdeckPresets() {
  return normalizeLaunchdeckConfig(state.launchdeckConfig).presets.items;
}

function renderExecutionPresets() {
  elements.executionPresetList.innerHTML = "";
  if (!state.presets.length) {
    const empty = document.createElement("div");
    empty.className = "card";
    empty.innerHTML = `
      <h3>No execution presets yet</h3>
      <p class="muted" style="margin-top: 8px;">Create the first engine preset to power the quick-trade panel.</p>
      <div class="action-row" style="margin-top: 14px;">
        <button class="secondary-button" data-add-execution-preset type="button">Create engine preset</button>
      </div>
    `;
    elements.executionPresetList.appendChild(empty);
    empty.querySelector("[data-add-execution-preset]")?.addEventListener("click", () => openPresetModal());
    return;
  }
  state.presets.forEach((preset, index) => {
    const card = document.createElement("div");
    card.className = "preset-preview-card";
    const renderEngineLane = (side, amounts, suffix, emptyLabel, specStrip) => `
      <div class="preset-lane preset-lane--${side}">
        <div class="preset-lane-amounts">
          ${renderPresetAmountChipsOrEmpty(amounts, side, suffix, emptyLabel)}
        </div>
        <div class="preset-spec-strip">${specStrip}</div>
      </div>
    `;
    card.innerHTML = `
      <div class="preset-preview-head">
        <div class="preset-preview-topline">
          <div class="preset-preview-brand">
            <img src="../../images/trench-tools-boot-logo.png" alt="" aria-hidden="true" />
          </div>
          <div>
            <h3>${escapeText(preset.label || `Preset ${index + 1}`)}</h3>
            <div class="preset-preview-subtitle">${escapeText(preset.id)}</div>
          </div>
        </div>
        <div class="preset-preview-actions">
          <button class="preset-icon-button" data-edit-execution-preset="${escapeAttribute(preset.id)}" type="button" title="Edit preset" aria-label="Edit preset">
            <img src="../../assets/edit-icon.png" alt="" aria-hidden="true" />
          </button>
          <button class="preset-icon-button is-destructive" data-delete-execution-preset="${escapeAttribute(preset.id)}" type="button" title="Remove preset" aria-label="Remove preset">
            <img src="../../assets/delete-icon.png" alt="" aria-hidden="true" />
          </button>
        </div>
      </div>
      ${renderEngineLane(
        "buy",
        getPresetBuyAmounts(preset),
        "SOL",
        "No quick-buy amounts",
        renderEnginePresetSpecStrip(preset.buyProvider, {
          autoEnabled: preset.buyAutoTipEnabled,
          maxFeeSol: preset.buyMaxFeeSol,
          priorityFeeSol: preset.buyFeeSol,
          tipSol: preset.buyTipSol,
          slippagePercent: getBuySlippagePercent(preset) || state.settings.defaultBuySlippagePercent || "0",
          mevMode: getBuyMevMode(preset) || state.settings.defaultBuyMevMode || "off"
        })
      )}
      ${renderEngineLane(
        "sell",
        getPresetSellAmounts(preset),
        "%",
        "No quick-sell amounts",
        renderEnginePresetSpecStrip(preset.sellProvider, {
          autoEnabled: preset.sellAutoTipEnabled,
          maxFeeSol: preset.sellMaxFeeSol,
          priorityFeeSol: preset.sellFeeSol,
          tipSol: preset.sellTipSol,
          slippagePercent: getSellSlippagePercent(preset) || state.settings.defaultSellSlippagePercent || "0",
          mevMode: getSellMevMode(preset) || state.settings.defaultSellMevMode || "off"
        })
      )}
    `;
    elements.executionPresetList.appendChild(card);
  });
  for (const button of elements.executionPresetList.querySelectorAll("[data-edit-execution-preset]")) {
    button.onclick = () => openPresetModal(button.dataset.editExecutionPreset);
  }
  for (const button of elements.executionPresetList.querySelectorAll("[data-delete-execution-preset]")) {
    button.onclick = async () => {
      await callBackground("trench:delete-preset", { presetId: button.dataset.deleteExecutionPreset });
      await bumpBootstrapRevision();
      showOptionsToast("Preset removed", "Execution preset deleted.", { type: "success" });
      await refreshEngineData();
    };
  }
}

function renderLaunchdeckPresets() {
  elements.launchdeckPresetList.innerHTML = "";
  const launchdeckPresets = getLaunchdeckPresets();
  if (!launchdeckPresets.length) {
    const empty = document.createElement("div");
    empty.className = "card";
    empty.innerHTML = `
      <h3>No LaunchDeck presets yet</h3>
      <p class="muted" style="margin-top: 8px;">Create the first LaunchDeck preset before using deploy, Vamp, or the LaunchDeck webapp flows.</p>
      <div class="action-row" style="margin-top: 14px;">
        <button class="secondary-button" data-add-launchdeck-preset type="button">Create LaunchDeck preset</button>
      </div>
    `;
    elements.launchdeckPresetList.appendChild(empty);
    empty.querySelector("[data-add-launchdeck-preset]")?.addEventListener("click", () => openLaunchdeckPresetModal());
    return;
  }
  launchdeckPresets.forEach((preset, index) => {
    const card = document.createElement("div");
    const isActive = state.launchdeckConfig?.defaults?.activePresetId === preset.id;
    card.className = `preset-preview-card preset-preview-card--launchdeck${isActive ? " preset-preview-card--active" : ""}`;
    const creation = preset.creationSettings || {};
    const buy = preset.buySettings || {};
    const sell = preset.sellSettings || {};
    const renderLaunchdeckLane = (tone, label, section, opts) => `
      <div class="preset-lane preset-lane--launchdeck preset-lane--${tone}">
        <span class="preset-section-pill ${tone}">${label}</span>
        <div class="preset-spec-strip">${renderLaunchdeckPresetSpecStrip(section, opts)}</div>
      </div>
    `;
    card.innerHTML = `
      <div class="preset-preview-head">
        <div class="preset-preview-topline">
          <div class="preset-preview-brand">
            <img src="../../images/trench-tools-boot-logo.png" alt="" aria-hidden="true" />
          </div>
          <div>
            <h3>${escapeText(preset.label || `LaunchDeck Preset ${index + 1}`)}</h3>
            <div class="preset-preview-subtitle">${escapeText(preset.id)}</div>
          </div>
        </div>
        <div class="preset-preview-actions">
          ${isActive
            ? `<span class="preset-active-tag" title="Active LaunchDeck preset">Active</span>`
            : `<button class="secondary-button" data-activate-launchdeck-preset="${escapeAttribute(preset.id)}" type="button">Set active</button>`}
          <button class="preset-icon-button" data-edit-launchdeck-preset="${escapeAttribute(preset.id)}" type="button" title="Edit preset" aria-label="Edit preset">
            <img src="../../assets/edit-icon.png" alt="" aria-hidden="true" />
          </button>
          <button class="preset-icon-button is-destructive" data-delete-launchdeck-preset="${escapeAttribute(preset.id)}" type="button" title="Remove preset" aria-label="Remove preset">
            <img src="../../assets/delete-icon.png" alt="" aria-hidden="true" />
          </button>
        </div>
      </div>
      ${renderLaunchdeckLane("creation", "CREATION", creation, { showSlip: false, showMev: false })}
      ${renderLaunchdeckLane("buy", "BUY", buy, { showSlip: true, showMev: true })}
      ${renderLaunchdeckLane("sell", "SELL", sell, { showSlip: true, showMev: true })}
    `;
    elements.launchdeckPresetList.appendChild(card);
  });
  for (const button of elements.launchdeckPresetList.querySelectorAll("[data-edit-launchdeck-preset]")) {
    button.onclick = () => openLaunchdeckPresetModal(button.dataset.editLaunchdeckPreset);
  }
  for (const button of elements.launchdeckPresetList.querySelectorAll("[data-activate-launchdeck-preset]")) {
    button.onclick = async () => {
      const nextConfig = normalizeLaunchdeckConfig(state.launchdeckConfig);
      nextConfig.defaults.activePresetId = button.dataset.activateLaunchdeckPreset;
      const payload = await saveLaunchdeckConfig(nextConfig);
      state.launchdeckSettingsPayload = payload || null;
      state.launchdeckConfig = normalizeLaunchdeckConfig(payload?.config || nextConfig);
      showOptionsToast("Active preset updated", "LaunchDeck active preset switched.", {
        type: "success"
      });
      await refreshEngineData();
    };
  }
  for (const button of elements.launchdeckPresetList.querySelectorAll("[data-delete-launchdeck-preset]")) {
    button.onclick = async () => {
      await deleteLaunchdeckPresetById(button.dataset.deleteLaunchdeckPreset);
    };
  }
}

function createLaunchdeckPresetDraft() {
  const presets = getLaunchdeckPresets();
  const usedIds = new Set(presets.map((entry) => entry.id));
  const draft = createEmptyLaunchdeckPreset(presets.length);
  draft.id = nextUniqueSlug(`launchdeck-preset-${presets.length + 1}`, "launchdeck-preset", usedIds);
  return draft;
}

function openLaunchdeckPresetModal(presetId = "") {
  state.editingLaunchdeckPresetId = presetId || "";
  const isEditing = Boolean(state.editingLaunchdeckPresetId);
  const preset = isEditing
    ? (getLaunchdeckPresets().find((entry) => entry.id === state.editingLaunchdeckPresetId) || createLaunchdeckPresetDraft())
    : createLaunchdeckPresetDraft();
  elements.launchdeckPresetModalTitle.textContent = isEditing
    ? "Edit LaunchDeck preset"
    : "Create LaunchDeck preset";
  if (elements.launchdeckPresetModalDelete) {
    elements.launchdeckPresetModalDelete.hidden = !isEditing;
  }
  elements.launchdeckPresetId.value = preset.id;
  elements.launchdeckPresetLabel.value = preset.label || "";

  const creation = preset.creationSettings || {};
  elements.launchdeckPresetCreationProvider.value = selectableExtensionRouteProvider(creation.provider);
  elements.launchdeckPresetCreationMev.value = creation.mevMode || "off";
  elements.launchdeckPresetCreationFee.value = creation.priorityFeeSol || "";
  elements.launchdeckPresetCreationTip.value = creation.tipSol || "";
  elements.launchdeckPresetCreationAutoFee.checked = Boolean(creation.autoFee);
  elements.launchdeckPresetCreationMaxFee.value = creation.maxFeeSol || "";

  const buy = preset.buySettings || {};
  elements.launchdeckPresetBuyProvider.value = selectableExtensionRouteProvider(buy.provider);
  elements.launchdeckPresetBuyMev.value = buy.mevMode || "off";
  elements.launchdeckPresetBuySlippage.value = buy.slippagePercent || "";
  elements.launchdeckPresetBuyFee.value = buy.priorityFeeSol || "";
  elements.launchdeckPresetBuyTip.value = buy.tipSol || "";
  elements.launchdeckPresetBuyAutoFee.checked = Boolean(buy.autoFee);
  elements.launchdeckPresetBuyMaxFee.value = buy.maxFeeSol || "";

  const sell = preset.sellSettings || {};
  elements.launchdeckPresetSellProvider.value = selectableExtensionRouteProvider(sell.provider);
  elements.launchdeckPresetSellMev.value = sell.mevMode || "off";
  elements.launchdeckPresetSellSlippage.value = sell.slippagePercent || "";
  elements.launchdeckPresetSellFee.value = sell.priorityFeeSol || "";
  elements.launchdeckPresetSellTip.value = sell.tipSol || "";
  elements.launchdeckPresetSellAutoFee.checked = Boolean(sell.autoFee);
  elements.launchdeckPresetSellMaxFee.value = sell.maxFeeSol || "";

  // Dev buy and snipe buy amounts are edited in the LaunchDeck UI itself, not
  // in this modal. Preserve whatever the backend carries so saves round-trip.
  elements.launchdeckPresetDevBuy.value = creation.devBuySol || "";
  elements.launchdeckPresetSnipeBuy.value = buy.snipeBuyAmountSol || "";
  applyRouteProviderAvailability(elements.launchdeckPresetCreationProvider);
  applyRouteProviderAvailability(elements.launchdeckPresetBuyProvider);
  applyRouteProviderAvailability(elements.launchdeckPresetSellProvider);

  bindRouteMevToggle("launchdeck-creation", elements.launchdeckPresetCreationProvider, elements.launchdeckPresetCreationMev);
  bindRouteMevToggle("launchdeck-buy", elements.launchdeckPresetBuyProvider, elements.launchdeckPresetBuyMev);
  bindRouteMevToggle("launchdeck-sell", elements.launchdeckPresetSellProvider, elements.launchdeckPresetSellMev);
  bindLaunchdeckPresetRouteFeeInputs(
    elements.launchdeckPresetCreationProvider,
    elements.launchdeckPresetCreationFee,
    elements.launchdeckPresetCreationTip,
    elements.launchdeckPresetCreationAutoFee
  );
  bindLaunchdeckPresetRouteFeeInputs(
    elements.launchdeckPresetBuyProvider,
    elements.launchdeckPresetBuyFee,
    elements.launchdeckPresetBuyTip,
    elements.launchdeckPresetBuyAutoFee
  );
  bindLaunchdeckPresetRouteFeeInputs(
    elements.launchdeckPresetSellProvider,
    elements.launchdeckPresetSellFee,
    elements.launchdeckPresetSellTip,
    elements.launchdeckPresetSellAutoFee
  );
  updateMevVisibility("launchdeck-creation", elements.launchdeckPresetCreationProvider, elements.launchdeckPresetCreationMev);
  updateMevVisibility("launchdeck-buy", elements.launchdeckPresetBuyProvider, elements.launchdeckPresetBuyMev);
  updateMevVisibility("launchdeck-sell", elements.launchdeckPresetSellProvider, elements.launchdeckPresetSellMev);
  syncLaunchdeckPresetModalFeeInputs();
  syncAutoFeeFallbackLabels();

  clearInvalidStateWithinModal(elements.launchdeckPresetModal);
  state.launchdeckPresetModalOpen = true;
  elements.launchdeckPresetModal.classList.remove("hidden");
}

function closeLaunchdeckPresetModal() {
  state.launchdeckPresetModalOpen = false;
  state.editingLaunchdeckPresetId = null;
  elements.launchdeckPresetModal.classList.add("hidden");
}

function collectLaunchdeckPresetModal() {
  const existingPresets = getLaunchdeckPresets();
  const usedIds = new Set(
    existingPresets
      .filter((entry) => entry.id !== state.editingLaunchdeckPresetId)
      .map((entry) => entry.id)
  );
  const label = elements.launchdeckPresetLabel.value.trim();
  const presetId = state.editingLaunchdeckPresetId
    || nextUniqueSlug(label || elements.launchdeckPresetId.value.trim(), "launchdeck-preset", usedIds);
  // Dev-buy and snipe values are edited inline in LaunchDeck-specific flows, so
  // preserve whatever the backend already has stored.
  const existing = existingPresets.find((entry) => entry.id === state.editingLaunchdeckPresetId)
    || createLaunchdeckPresetDraft();
  const existingCreation = existing.creationSettings || {};
  const existingBuy = existing.buySettings || {};
  const existingSell = existing.sellSettings || {};
  const creationProvider = selectableExtensionRouteProvider(
    elements.launchdeckPresetCreationProvider.value || existingCreation.provider
  );
  const buyProvider = selectableExtensionRouteProvider(
    elements.launchdeckPresetBuyProvider.value || existingBuy.provider
  );
  const sellProvider = selectableExtensionRouteProvider(
    elements.launchdeckPresetSellProvider.value || existingSell.provider
  );
  const creationAutoFee = Boolean(elements.launchdeckPresetCreationAutoFee.checked);
  const buyAutoFee = Boolean(elements.launchdeckPresetBuyAutoFee.checked);
  const sellAutoFee = Boolean(elements.launchdeckPresetSellAutoFee.checked);
  const normalizePriorityForAutoFee = (provider, input, autoFee) =>
    autoFee ? input.value.trim() : normalizePriorityFeeForProvider(provider, input.value);
  const normalizeTipForAutoFee = (provider, input, autoFee) =>
    autoFee
      ? (providerSupportsTip(provider) ? input.value.trim() : "")
      : normalizeTipForProvider(provider, input.value);
  return normalizeLaunchdeckPreset({
    id: presetId,
    label,
    creationSettings: {
      ...existingCreation,
      provider: creationProvider,
      mevMode: elements.launchdeckPresetCreationMev.value.trim() || "off",
      priorityFeeSol: normalizePriorityForAutoFee(creationProvider, elements.launchdeckPresetCreationFee, creationAutoFee),
      tipSol: normalizeTipForAutoFee(creationProvider, elements.launchdeckPresetCreationTip, creationAutoFee),
      autoFee: creationAutoFee,
      maxFeeSol: elements.launchdeckPresetCreationMaxFee.value.trim(),
      // Dev buy is entered in the LaunchDeck panel itself; preserve stored value.
      devBuySol: elements.launchdeckPresetDevBuy.value.trim() || existingCreation.devBuySol || ""
    },
    buySettings: {
      ...existingBuy,
      provider: buyProvider,
      mevMode: elements.launchdeckPresetBuyMev.value.trim() || "off",
      slippagePercent: elements.launchdeckPresetBuySlippage.value.trim(),
      priorityFeeSol: normalizePriorityForAutoFee(buyProvider, elements.launchdeckPresetBuyFee, buyAutoFee),
      tipSol: normalizeTipForAutoFee(buyProvider, elements.launchdeckPresetBuyTip, buyAutoFee),
      autoFee: buyAutoFee,
      maxFeeSol: elements.launchdeckPresetBuyMaxFee.value.trim(),
      // Snipe amount is entered in the sniper modal in the LaunchDeck UI.
      snipeBuyAmountSol: elements.launchdeckPresetSnipeBuy.value.trim() || existingBuy.snipeBuyAmountSol || ""
    },
    sellSettings: {
      ...existingSell,
      provider: sellProvider,
      mevMode: elements.launchdeckPresetSellMev.value.trim() || "off",
      slippagePercent: elements.launchdeckPresetSellSlippage.value.trim(),
      priorityFeeSol: normalizePriorityForAutoFee(sellProvider, elements.launchdeckPresetSellFee, sellAutoFee),
      tipSol: normalizeTipForAutoFee(sellProvider, elements.launchdeckPresetSellTip, sellAutoFee),
      autoFee: sellAutoFee,
      maxFeeSol: elements.launchdeckPresetSellMaxFee.value.trim()
    }
  }, Math.max(0, existingPresets.findIndex((entry) => entry.id === state.editingLaunchdeckPresetId)));
}

function validateLaunchdeckPreset(preset) {
  const errors = [];
  const push = (fieldId, message) => errors.push({ fieldId, message });

  if (!String(preset.label || "").trim()) {
    push("launchdeck-preset-label", "Preset name is required.");
  }

  const validateSection = (section, settings) => {
    const isCreation = section === "creation";
    const isBuy = section === "buy";
    const providerFieldId = `launchdeck-preset-${section}-provider`;
    const priorityFieldId = `launchdeck-preset-${section}-fee`;
    const tipFieldId = `launchdeck-preset-${section}-tip`;
    const slipFieldId = `launchdeck-preset-${section}-slippage`;
    const maxFeeFieldId = `launchdeck-preset-${section}-max-fee`;
    const sectionLabel = isCreation ? "Creation" : isBuy ? "Buy" : "Sell";
    const provider = settings.provider;
    const label = providerLabel(provider);
    const minTip = providerMinimumTipSol(provider);
    const supportsTip = providerSupportsTip(provider);
    const requiresPriority = providerRequiresPriorityFee(provider);

    if (!provider) push(providerFieldId, `${sectionLabel} route is required.`);
    if (!isCreation && !settings.slippagePercent) {
      push(slipFieldId, `${sectionLabel} slippage % is required.`);
    }
    if (requiresPriority) {
      if (!settings.priorityFeeSol && !settings.autoFee) {
        push(priorityFieldId, `Priority fee is required for ${label}.`);
      } else if (settings.priorityFeeSol && !(Number(settings.priorityFeeSol) > 0)) {
        push(priorityFieldId, `Priority fee must be greater than 0 for ${label}.`);
      }
    }
    if (supportsTip) {
      if (!settings.tipSol && !settings.autoFee) {
        push(tipFieldId, `Tip is required for ${label}.`);
      } else if (settings.tipSol && (Number.isNaN(Number(settings.tipSol)) || Number(settings.tipSol) < 0)) {
        push(tipFieldId, "Tip must be a valid number.");
      } else if (settings.tipSol && minTip > 0 && Number(settings.tipSol) < minTip) {
        push(tipFieldId, `Tip must be at least ${formatMinSol(minTip)} SOL for ${label}.`);
      }
    }
    if (settings.autoFee) {
      if (!settings.maxFeeSol) {
        push(maxFeeFieldId, `Max Auto Fee is required when Auto fee is on.`);
      } else if (!(Number(settings.maxFeeSol) > 0)) {
        push(maxFeeFieldId, `Max Auto Fee must be greater than 0.`);
      } else if (minTip > 0 && Number(settings.maxFeeSol) < minTip) {
        push(maxFeeFieldId, `Max Auto Fee must be at least ${formatMinSol(minTip)} SOL for ${label} because it includes both priority fee and tip.`);
      }
    }
  };

  validateSection("creation", preset.creationSettings || {});
  validateSection("buy", preset.buySettings || {});
  validateSection("sell", preset.sellSettings || {});
  return errors;
}

async function saveLaunchdeckPresetFromModal() {
  const nextPreset = collectLaunchdeckPresetModal();
  const errors = validateLaunchdeckPreset(nextPreset);
  markFieldsInvalid(errors);
  if (errors.length > 0) {
    showOptionsToast("Preset not saved", summarizeValidationErrors(errors), { type: "error" });
    return;
  }
  const nextConfig = normalizeLaunchdeckConfig(state.launchdeckConfig);
  const editingPresetId = state.editingLaunchdeckPresetId;
  const existingIndex = nextConfig.presets.items.findIndex((entry) => entry.id === editingPresetId);
  if (existingIndex >= 0) {
    nextConfig.presets.items[existingIndex] = nextPreset;
  } else {
    nextConfig.presets.items.push(nextPreset);
  }
  if (
    !nextConfig.defaults.activePresetId
    || !nextConfig.presets.items.some((entry) => entry.id === nextConfig.defaults.activePresetId)
  ) {
    nextConfig.defaults.activePresetId = nextPreset.id;
  }
  const payload = await saveLaunchdeckConfig(nextConfig);
  state.launchdeckSettingsPayload = payload || null;
  state.launchdeckConfig = normalizeLaunchdeckConfig(payload?.config || nextConfig);
  showOptionsToast("Preset saved", nextPreset.label || nextPreset.id, { type: "success" });
  closeLaunchdeckPresetModal();
  await refreshEngineData();
}

async function deleteLaunchdeckPresetById(presetId) {
  const normalizedPresetId = String(presetId || "").trim();
  if (!normalizedPresetId) {
    return false;
  }
  if (!window.confirm("Delete this LaunchDeck preset?")) {
    return false;
  }
  const nextConfig = normalizeLaunchdeckConfig(state.launchdeckConfig);
  nextConfig.presets.items = nextConfig.presets.items.filter((entry) => entry.id !== normalizedPresetId);
  if (!nextConfig.presets.items.some((entry) => entry.id === nextConfig.defaults.activePresetId)) {
    nextConfig.defaults.activePresetId = nextConfig.presets.items[0]?.id || "";
  }
  const payload = await saveLaunchdeckConfig(nextConfig);
  state.launchdeckSettingsPayload = payload || null;
  state.launchdeckConfig = normalizeLaunchdeckConfig(payload?.config || nextConfig);
  showOptionsToast("Preset removed", "LaunchDeck preset deleted.", { type: "success" });
  await refreshEngineData();
  return true;
}

async function deleteLaunchdeckPresetFromModal() {
  if (!state.editingLaunchdeckPresetId) {
    return;
  }
  const presetId = state.editingLaunchdeckPresetId;
  const didDelete = await deleteLaunchdeckPresetById(presetId);
  if (didDelete) {
    closeLaunchdeckPresetModal();
  }
}

function normalizePresetValues(values, legacyValue, length = 4) {
  const targetLength = Number.isFinite(length) && length > 0 ? Math.floor(length) : 4;
  const normalized = Array.isArray(values)
    ? values.slice(0, targetLength).map((value) => String(value || "").trim())
    : [];
  while (normalized.length < targetLength) {
    normalized.push("");
  }
  if (!normalized.some(Boolean) && legacyValue) {
    normalized[0] = String(legacyValue).trim();
  }
  return normalized.slice(0, targetLength);
}

function clampBuyAmountRows(rows) {
  const numeric = Number(rows);
  if (numeric === 2) {
    return 2;
  }
  return 1;
}

function clampSellPercentRows(rows) {
  const numeric = Number(rows);
  if (numeric === 2) {
    return 2;
  }
  return 1;
}

function getPresetBuyAmountRows(preset) {
  if (!preset) return 1;
  const explicit = clampBuyAmountRows(preset.buyAmountRows);
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
  const explicit = clampSellPercentRows(preset.sellPercentRows);
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
  return normalizePresetValues(preset?.buyAmountsSol, preset?.buyAmountSol, rows * 4);
}

function getPresetSellAmounts(preset) {
  const rows = getPresetSellPercentRows(preset);
  return normalizePresetValues(preset?.sellAmountsPercent, preset?.sellPercent, rows * 4);
}

function renderPresetAmountChips(values, tone, suffix) {
  return values
    .filter(Boolean)
    .map((value) => {
      let suffixMarkup = "";
      if (suffix === "SOL") {
        suffixMarkup = SOL_ICON_SVG;
      } else if (suffix) {
        suffixMarkup = suffix.startsWith("%") ? suffix : ` ${suffix}`;
      }
      return `<span class="preset-chip ${tone}">${escapeText(value)}${suffixMarkup}</span>`;
    })
    .join("");
}

function renderPresetAmountChipsOrEmpty(values, tone, suffix, emptyLabel) {
  const chips = renderPresetAmountChips(values, tone, suffix);
  return chips || `<span class="resource-badge muted">${escapeText(emptyLabel)}</span>`;
}

function renderPresetSpecToken(label, valueHtml, variant) {
  const variantClass = variant ? ` preset-spec-token--${variant}` : "";
  return `<span class="preset-spec-token${variantClass}"><span class="label">${escapeText(label)}</span><span class="value">${valueHtml}</span></span>`;
}

function joinPresetSpecTokens(tokens) {
  const filtered = tokens.filter(Boolean);
  if (!filtered.length) return "";
  return filtered.join('<span class="sep" aria-hidden="true">·</span>');
}

function renderSolAmountValue(amount) {
  return `${escapeText(amount)}${SOL_ICON_SVG}`;
}

function renderEnginePresetSpecStrip(provider, {
  autoEnabled,
  maxFeeSol,
  priorityFeeSol,
  tipSol,
  slippagePercent,
  mevMode
}) {
  const tokens = [];
  tokens.push(renderPresetSpecToken("Provider", escapeText(providerLabel(provider)), "provider"));
  if (autoEnabled) {
    const autoValue = maxFeeSol ? `Auto · cap ${renderSolAmountValue(maxFeeSol)}` : "Auto";
    tokens.push(renderPresetSpecToken("Fee", autoValue, "auto"));
  } else {
    if (priorityFeeSol) {
      tokens.push(renderPresetSpecToken("Prio", renderSolAmountValue(priorityFeeSol)));
    }
    if (providerSupportsTip(provider) && tipSol) {
      tokens.push(renderPresetSpecToken("Tip", renderSolAmountValue(tipSol)));
    }
  }
  tokens.push(renderPresetSpecToken("Slip", `${escapeText(slippagePercent || "0")}%`));
  tokens.push(renderPresetSpecToken("MEV", escapeText(mevMode || "off")));
  return joinPresetSpecTokens(tokens);
}

function renderLaunchdeckPresetSpecStrip(section, { showSlip, showMev }) {
  const provider = section.provider || "helius-sender";
  const tokens = [];
  tokens.push(renderPresetSpecToken("Provider", escapeText(providerLabel(provider)), "provider"));
  if (section.priorityFeeSol) {
    tokens.push(renderPresetSpecToken("Prio", renderSolAmountValue(section.priorityFeeSol)));
  }
  if (providerSupportsTip(provider) && section.tipSol) {
    tokens.push(renderPresetSpecToken("Tip", renderSolAmountValue(section.tipSol)));
  }
  if (showSlip) {
    const slipValue = section.slippagePercent ? `${escapeText(section.slippagePercent)}%` : "—";
    tokens.push(renderPresetSpecToken("Slip", slipValue));
  }
  if (showMev) {
    const mev = section.mevMode && section.mevMode !== "off" ? section.mevMode : "off";
    tokens.push(renderPresetSpecToken("MEV", escapeText(mev)));
  }
  if (section.autoFee) {
    const autoValue = section.maxFeeSol ? `Auto · cap ${renderSolAmountValue(section.maxFeeSol)}` : "Auto";
    tokens.push(renderPresetSpecToken("Fee", autoValue, "auto"));
  }
  return joinPresetSpecTokens(tokens);
}

function slugifyKey(value, fallback = "item") {
  const normalized = String(value || "")
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return normalized || fallback;
}

function nextUniqueSlug(value, fallback, usedIds) {
  const base = slugifyKey(value, fallback);
  if (!usedIds.has(base)) {
    return base;
  }
  let index = 2;
  while (usedIds.has(`${base}-${index}`)) {
    index += 1;
  }
  return `${base}-${index}`;
}

function formatTimestamp(unixMs) {
  if (!unixMs) {
    return "never";
  }
  const date = new Date(Number(unixMs));
  if (Number.isNaN(date.getTime())) {
    return "-";
  }
  return date.toLocaleString([], {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit"
  });
}

function cssEscape(value) {
  return String(value || "").replaceAll("\\", "\\\\").replaceAll("\"", "\\\"");
}

function clampInt(value, min, max) {
  const parsed = Math.round(Number(value));
  if (!Number.isFinite(parsed)) {
    return min;
  }
  return Math.max(min, Math.min(max, parsed));
}

function truncateMiddle(value, prefix = 4, suffix = 4) {
  const text = String(value || "").trim();
  if (!text) {
    return "";
  }
  if (text.length <= prefix + suffix + 1) {
    return text;
  }
  return `${text.slice(0, prefix)}...${text.slice(-suffix)}`;
}

function formatWalletDisplayLabel(wallet, fallback = "Wallet") {
  const label = String(wallet?.label || wallet?.customName || wallet?.key || "").trim();
  const resolved = label || fallback;
  const genericMatch = resolved.match(/^SOLANA_PRIVATE_KEY(\d+)?$/i);
  if (!genericMatch) {
    return resolved;
  }
  return `#${genericMatch[1] || "1"}`;
}

function walletEnvSlotTag(walletKey) {
  const match = String(walletKey || "").match(/^SOLANA_PRIVATE_KEY(\d+)?$/i);
  if (!match) return "";
  return `#${match[1] || "1"}`;
}

function walletInitials(label) {
  const words = String(label || "")
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  if (!words.length) {
    return "W";
  }
  if (words.length === 1) {
    return words[0].slice(0, 2).toUpperCase();
  }
  return `${words[0][0] || ""}${words[1][0] || ""}`.toUpperCase();
}

function walletAvatarGradient(seed) {
  const hash = hashString(String(seed || "wallet"));
  const hue = hash % 360;
  const hue2 = (hue + 40) % 360;
  return `linear-gradient(135deg, hsl(${hue} 70% 42%), hsl(${hue2} 70% 30%))`;
}

const WALLET_EMOJI_POOL = [
  "😀", "😎", "🤓", "🥸", "🤠", "🤖", "👾", "👻", "🐸", "🦄",
  "🐯", "🦊", "🐼", "🐨", "🦁", "🐵", "🦖", "🐙", "🦑", "🐬",
  "🦉", "🦋", "🌈", "⚡", "🔥", "✨", "🌟", "💫", "☄️", "🌙",
  "🚀", "🛸", "🪐", "🎯", "🎲", "🎩", "🕶️", "🧠", "🪄", "🔮",
  "💎", "🪙", "💰", "💵", "🏆", "🥇", "🗡️", "⚔️", "🛡️", "🏴‍☠️"
];

const EMOJI_CATALOG = [
  {
    id: "smileys",
    label: "😀",
    name: "Smileys & People",
    emojis: [
      "😀","😃","😄","😁","😆","😅","🤣","😂","🙂","🙃","😉","😊","😇","🥰","😍","🤩","😘","😗","😚","😙",
      "😋","😛","😜","🤪","😝","🤑","🤗","🤭","🤫","🤔","🤐","🤨","😐","😑","😶","😏","😒","🙄","😬","🤥",
      "😌","😔","😪","🤤","😴","😷","🤒","🤕","🤢","🤮","🤧","🥵","🥶","🥴","😵","🤯","🤠","🥳","😎","🤓",
      "🧐","😕","😟","🙁","☹️","😮","😯","😲","😳","🥺","😦","😧","😨","😰","😥","😢","😭","😱","😖","😣",
      "😞","😓","😩","😫","🥱","😤","😡","😠","🤬","😈","👿","💀","☠️","💩","🤡","👹","👺","👻","👽","👾",
      "🤖","🎃","😺","😸","😹","😻","😼","😽","🙀","😿","😾","🙈","🙉","🙊","💋","💌","💘","💝","💖","💗",
      "💓","💞","💕","💟","❣️","💔","❤️","🧡","💛","💚","💙","💜","🤎","🖤","🤍","👋","🤚","🖐️","✋","🖖",
      "👌","🤌","🤏","✌️","🤞","🤟","🤘","🤙","👈","👉","👆","🖕","👇","☝️","👍","👎","✊","👊","🤛","🤜",
      "👏","🙌","👐","🤲","🤝","🙏","💅","🤳","💪","🦾","🦿","🦵","🦶","👂","🦻","👃","🧠","🫀","🫁","🦷",
      "🦴","👀","👁️","👅","👄","👶","🧒","👦","👧","🧑","👱","👨","🧔","👩","🧓","👴","👵","🙍","🙎","🙅",
      "🙆","💁","🙋","🧏","🙇","🤦","🤷","🧑‍⚕️","👨‍⚕️","👩‍⚕️","🧑‍🎓","🧑‍🏫","🧑‍⚖️","🧑‍🌾","🧑‍🍳","🧑‍🔧","🧑‍🏭","🧑‍💼","🧑‍🔬","🧑‍💻",
      "🧑‍🎤","🧑‍🎨","🧑‍✈️","🧑‍🚀","🧑‍🚒","👮","🕵️","💂","🥷","👷","🤴","👸","👳","👲","🧕","🤵","👰","🤰","🤱","👼",
      "🎅","🤶","🦸","🦹","🧙","🧚","🧛","🧜","🧝","🧞","🧟","💆","💇","🚶","🧍","🧎","🏃","💃","🕺","🕴️",
      "👯","🧖","🧗","🤺","🏇","⛷️","🏂","🏌️","🏄","🚣","🏊","⛹️","🏋️","🚴","🚵","🤸","🤼","🤽","🤾","🤹",
      "🧘","🛀","🛌","🧑‍🤝‍🧑","👭","👫","👬","💏","💑","👪","👨‍👩‍👦","👨‍👩‍👧","👨‍👩‍👧‍👦","👨‍👩‍👦‍👦","👨‍👩‍👧‍👧","🗣️","👤","👥","🫂","👣"
    ]
  },
  {
    id: "animals",
    label: "🐼",
    name: "Animals & Nature",
    emojis: [
      "🐶","🐱","🐭","🐹","🐰","🦊","🐻","🐼","🐻‍❄️","🐨","🐯","🦁","🐮","🐷","🐽","🐸","🐵","🙈","🙉","🙊",
      "🐒","🐔","🐧","🐦","🐤","🐣","🐥","🦆","🦅","🦉","🦇","🐺","🐗","🐴","🦄","🐝","🪱","🐛","🦋","🐌",
      "🐞","🐜","🪰","🪲","🪳","🦟","🦗","🕷️","🕸️","🦂","🐢","🐍","🦎","🦖","🦕","🐙","🦑","🦐","🦞","🦀",
      "🐡","🐠","🐟","🐬","🐳","🐋","🦈","🐊","🐅","🐆","🦓","🦍","🦧","🦣","🐘","🦛","🦏","🐪","🐫","🦒",
      "🦘","🦬","🐃","🐂","🐄","🐎","🐖","🐏","🐑","🦙","🐐","🦌","🐕","🐩","🦮","🐕‍🦺","🐈","🐈‍⬛","🪶","🐓",
      "🦃","🦤","🦚","🦜","🦢","🦩","🕊️","🐇","🦝","🦨","🦡","🦫","🦦","🦥","🐁","🐀","🐿️","🦔","🐾","🐉",
      "🐲","🌵","🎄","🌲","🌳","🌴","🪵","🌱","🌿","☘️","🍀","🎍","🪴","🎋","🍃","🍂","🍁","🍄","🐚","🪨",
      "🌾","💐","🌷","🌹","🥀","🌺","🌸","🌼","🌻","🌞","🌝","🌛","🌜","🌚","🌕","🌖","🌗","🌘","🌑","🌒",
      "🌓","🌔","🌙","🌎","🌍","🌏","🪐","💫","⭐","🌟","✨","⚡","☄️","💥","🔥","🌪️","🌈","☀️","🌤️","⛅",
      "🌥️","☁️","🌦️","🌧️","⛈️","🌩️","🌨️","❄️","☃️","⛄","🌬️","💨","💧","💦","☔","☂️","🌊","🌫️"
    ]
  },
  {
    id: "food",
    label: "🍔",
    name: "Food & Drink",
    emojis: [
      "🍏","🍎","🍐","🍊","🍋","🍌","🍉","🍇","🍓","🫐","🍈","🍒","🍑","🥭","🍍","🥥","🥝","🍅","🍆","🥑",
      "🥦","🥬","🥒","🌶️","🫑","🌽","🥕","🫒","🧄","🧅","🥔","🍠","🥐","🥯","🍞","🥖","🥨","🧀","🥚","🍳",
      "🧈","🥞","🧇","🥓","🥩","🍗","🍖","🦴","🌭","🍔","🍟","🍕","🫓","🥪","🥙","🧆","🌮","🌯","🫔","🥗",
      "🥘","🫕","🥫","🍝","🍜","🍲","🍛","🍣","🍱","🥟","🦪","🍤","🍙","🍚","🍘","🍥","🥠","🥮","🍢","🍡",
      "🍧","🍨","🍦","🥧","🧁","🍰","🎂","🍮","🍭","🍬","🍫","🍿","🍩","🍪","🌰","🥜","🍯","🥛","🍼","☕",
      "🫖","🍵","🧃","🥤","🧋","🍶","🍺","🍻","🥂","🍷","🥃","🍸","🍹","🧉","🍾","🧊","🥄","🍴","🍽️","🥣",
      "🥡","🥢","🧂"
    ]
  },
  {
    id: "activities",
    label: "⚽",
    name: "Activities & Sports",
    emojis: [
      "⚽","🏀","🏈","⚾","🥎","🎾","🏐","🏉","🥏","🎱","🪀","🏓","🏸","🥅","🏒","🏑","🏏","🥍","🏌️","⛳",
      "🪁","🏹","🎣","🤿","🥊","🥋","🎽","🛹","🛼","🛷","⛸️","🥌","🎿","⛷️","🏂","🪂","🏋️","🤼","🤸","🤺",
      "🤾","🏌️","🏇","🧘","🏄","🏊","🤽","🚣","🧗","🚵","🚴","🏆","🥇","🥈","🥉","🏅","🎖️","🏵️","🎗️","🎫",
      "🎟️","🎪","🤹","🎭","🩰","🎨","🎬","🎤","🎧","🎼","🎹","🥁","🪘","🎷","🎺","🎸","🪕","🎻","🎲","♟️",
      "🎯","🎳","🎮","🎰","🧩","🎴","🀄","🎊","🎉","🎈","🎆","🎇","🧨","🎁","🎀","🪅","🪆"
    ]
  },
  {
    id: "travel",
    label: "🚗",
    name: "Travel & Places",
    emojis: [
      "🚗","🚕","🚙","🚌","🚎","🏎️","🚓","🚑","🚒","🚐","🛻","🚚","🚛","🚜","🦯","🦽","🦼","🛴","🚲","🛵",
      "🏍️","🛺","🚨","🚔","🚍","🚘","🚖","🚡","🚠","🚟","🚃","🚋","🚞","🚝","🚄","🚅","🚈","🚂","🚆","🚇",
      "🚊","🚉","✈️","🛫","🛬","🛩️","💺","🛰️","🚀","🛸","🚁","🛶","⛵","🚤","🛥️","🛳️","⛴️","🚢","⚓","🪝",
      "⛽","🚧","🚦","🚥","🚏","🗺️","🗿","🗽","🗼","🏰","🏯","🏟️","🎡","🎢","🎠","⛲","⛱️","🏖️","🏝️","🏜️",
      "🌋","⛰️","🏔️","🗻","🏕️","⛺","🛖","🏠","🏡","🏘️","🏚️","🏗️","🏭","🏢","🏬","🏣","🏤","🏥","🏦","🏨",
      "🏪","🏫","🏩","💒","🏛️","⛪","🕌","🕍","🛕","🕋","⛩️","🏙️","🌆","🌇","🌃","🌌","🌉","🌁"
    ]
  },
  {
    id: "objects",
    label: "💡",
    name: "Objects",
    emojis: [
      "⌚","📱","📲","💻","⌨️","🖥️","🖨️","🖱️","🖲️","🕹️","🗜️","💽","💾","💿","📀","📼","📷","📸","📹","🎥",
      "📽️","🎞️","📞","☎️","📟","📠","📺","📻","🎙️","🎚️","🎛️","🧭","⏱️","⏲️","⏰","🕰️","⌛","⏳","📡","🔋",
      "🔌","💡","🔦","🕯️","🪔","🧯","🛢️","💸","💵","💴","💶","💷","💰","💳","💎","⚖️","🪜","🧰","🪛","🔧",
      "🔨","⚒️","🛠️","⛏️","🪚","🔩","⚙️","🪤","🧱","⛓️","🧲","🔫","💣","🧨","🪓","🔪","🗡️","⚔️","🛡️","🚬",
      "⚰️","🪦","⚱️","🏺","🔮","📿","🧿","💈","⚗️","🔭","🔬","🕳️","🩹","🩺","💊","💉","🩸","🧬","🦠","🧫",
      "🧪","🌡️","🧹","🪠","🧺","🧻","🚽","🚰","🚿","🛁","🛀","🧼","🪥","🪒","🧽","🪣","🧴","🛎️","🔑","🗝️",
      "🚪","🪑","🛋️","🛏️","🛌","🧸","🪆","🖼️","🪞","🪟","🛍️","🛒","🎁","🎈","🎏","🎀","🪄","🪅","🎊","🎉",
      "🎎","🏮","🎐","🧧","✉️","📩","📨","📧","💌","📥","📤","📦","🏷️","🪧","📪","📫","📬","📭","📮","📯",
      "📜","📃","📄","📑","🧾","📊","📈","📉","🗒️","🗓️","📆","📅","🗑️","📇","🗃️","🗳️","🗄️","📋","📁","📂",
      "🗂️","🗞️","📰","📓","📔","📒","📕","📗","📘","📙","📚","📖","🔖","🧷","🔗","📎","🖇️","📐","📏","🧮",
      "📌","📍","✂️","🖊️","🖋️","✒️","🖌️","🖍️","📝","✏️","🔍","🔎","🔏","🔐","🔒","🔓"
    ]
  },
  {
    id: "symbols",
    label: "💠",
    name: "Symbols",
    emojis: [
      "❤️","🧡","💛","💚","💙","💜","🖤","🤍","🤎","💔","❣️","💕","💞","💓","💗","💖","💘","💝","💟","☮️",
      "✝️","☪️","🕉️","☸️","✡️","🔯","🕎","☯️","☦️","🛐","⛎","♈","♉","♊","♋","♌","♍","♎","♏","♐",
      "♑","♒","♓","🆔","⚛️","☢️","☣️","📴","📳","🈶","🈚","🈸","🈺","🈷️","✴️","🆚","💮","🉐","㊙️","㊗️",
      "🈴","🈵","🈹","🈲","🅰️","🅱️","🆎","🆑","🅾️","🆘","❌","⭕","🛑","⛔","📛","🚫","💯","💢","♨️","🚷",
      "🚯","🚳","🚱","🔞","📵","🚭","❗","❕","❓","❔","‼️","⁉️","🔅","🔆","〽️","⚠️","🚸","🔱","⚜️","🔰",
      "♻️","✅","🈯","💹","❇️","✳️","❎","🌐","💠","Ⓜ️","🌀","💤","🏧","🚾","♿","🅿️","🈳","🈂️","🛂","🛃",
      "🛄","🛅","🚹","🚺","🚼","🚻","🚮","🎦","📶","🈁","🔣","ℹ️","🔤","🔡","🔠","🆖","🆗","🆙","🆒","🆕",
      "🆓","0️⃣","1️⃣","2️⃣","3️⃣","4️⃣","5️⃣","6️⃣","7️⃣","8️⃣","9️⃣","🔟","🔢","#️⃣","*️⃣","⏏️","▶️","⏸️","⏯️","⏹️",
      "⏺️","⏭️","⏮️","⏩","⏪","⏫","⏬","◀️","🔼","🔽","➡️","⬅️","⬆️","⬇️","↗️","↘️","↙️","↖️","↕️","↔️",
      "↪️","↩️","⤴️","⤵️","🔀","🔁","🔂","🔄","🔃","🎵","🎶","➕","➖","➗","✖️","🟰","♾️","💲","💱","™️",
      "©️","®️","〰️","➰","➿","🔚","🔙","🔛","🔝","🔜","✔️","☑️","🔘","🔴","🟠","🟡","🟢","🔵","🟣","⚫",
      "⚪","🟤","🔺","🔻","🔸","🔹","🔶","🔷","🔳","🔲","▪️","▫️","◾","◽","◼️","◻️","🟥","🟧","🟨","🟩",
      "🟦","🟪","⬛","⬜","🟫","🔈","🔇","🔉","🔊","🔔","🔕","📣","📢","👁️‍🗨️","💬","💭","🗯️","♠️","♣️","♥️","♦️",
      "🃏","🕐","🕑","🕒","🕓","🕔","🕕","🕖","🕗","🕘","🕙","🕚","🕛"
    ]
  },
  {
    id: "flags",
    label: "🏳️",
    name: "Flags",
    emojis: [
      "🏁","🚩","🎌","🏴","🏳️","🏳️‍🌈","🏳️‍⚧️","🏴‍☠️","🇦🇺","🇦🇹","🇧🇷","🇨🇦","🇨🇳","🇩🇪","🇪🇸","🇫🇮","🇫🇷","🇬🇧","🇭🇰","🇮🇩",
      "🇮🇪","🇮🇱","🇮🇳","🇮🇷","🇮🇹","🇯🇵","🇰🇷","🇲🇽","🇳🇱","🇳🇴","🇳🇿","🇵🇱","🇵🇹","🇷🇴","🇷🇺","🇸🇦","🇸🇪","🇸🇬","🇹🇷","🇺🇦",
      "🇺🇸","🇻🇳","🇿🇦"
    ]
  }
];

function defaultEmojiForKey(seed) {
  const hash = hashString(String(seed || "wallet"));
  return WALLET_EMOJI_POOL[hash % WALLET_EMOJI_POOL.length];
}

function walletDisplayEmoji(wallet) {
  const explicit = (wallet?.emoji || "").trim();
  if (explicit) {
    return explicit;
  }
  return defaultEmojiForKey(wallet?.key || wallet?.publicKey || wallet?.label || "wallet");
}

function hashString(value) {
  let hash = 2166136261;
  for (let i = 0; i < value.length; i += 1) {
    hash ^= value.charCodeAt(i);
    hash = (hash + ((hash << 1) + (hash << 4) + (hash << 7) + (hash << 8) + (hash << 24))) >>> 0;
  }
  return hash >>> 0;
}

// Dismiss a modal only when both pointerdown and pointerup happen directly on the backdrop.
// This prevents accidental closes when the user drags to select text inside the modal and
// releases the mouse outside of it.
function attachModalBackdropDismiss(overlay, close) {
  if (!overlay) {
    return;
  }
  let pointerDownOnOverlay = false;
  overlay.addEventListener("pointerdown", (event) => {
    pointerDownOnOverlay = event.target === overlay;
  });
  overlay.addEventListener("pointerup", (event) => {
    const started = pointerDownOnOverlay;
    pointerDownOnOverlay = false;
    if (started && event.target === overlay) {
      close();
    }
  });
  overlay.addEventListener("pointercancel", () => {
    pointerDownOnOverlay = false;
  });
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

init();
