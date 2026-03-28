const form = document.getElementById("launch-form");
const output = document.getElementById("output");
const statusNode = document.getElementById("status");
const metaNode = document.getElementById("meta");
const outputSection = document.getElementById("output-section");
const buttons = Array.from(document.querySelectorAll("[data-action]"));
const walletBox = document.querySelector(".wallet-box");
const walletSelect = document.getElementById("wallet-select");
const walletBalance = document.getElementById("wallet-balance");
const walletTriggerButton = document.getElementById("wallet-trigger-button");
const walletDropdown = document.getElementById("wallet-dropdown");
const walletDropdownList = document.getElementById("wallet-dropdown-list");
const walletRefreshButton = document.getElementById("wallet-refresh-button");
const walletSummarySol = document.getElementById("wallet-summary-sol");
const walletSummaryUsd = document.getElementById("wallet-summary-usd");
const topPresetChipBar = document.getElementById("top-preset-chip-bar");
const openVampButton = document.getElementById("open-vamp-button");
const themeToggleButton = document.getElementById("toggle-theme-button");
const feeSplitPill = document.getElementById("fee-split-pill");
const imageInput = document.getElementById("image-input");
const openImageLibraryButton = document.getElementById("open-image-library-button");
const imagePreview = document.getElementById("image-preview");
const imageEmpty = document.getElementById("image-empty");
const imageStatus = document.getElementById("image-status");
const imagePath = document.getElementById("image-path");
const imageLibraryModal = document.getElementById("image-library-modal");
const imageLibraryClose = document.getElementById("image-library-close");
const imageLibrarySearchInput = document.getElementById("image-library-search-input");
const imageLibraryUploadButton = document.getElementById("image-library-upload-button");
const imageLibraryGrid = document.getElementById("image-library-grid");
const imageLibraryEmpty = document.getElementById("image-library-empty");
const imageCategoryChips = document.getElementById("image-category-chips");
const newImageCategoryButton = document.getElementById("new-image-category-button");
const imageItemMenu = document.getElementById("image-item-menu");
const imageMenuFavorite = document.getElementById("image-menu-favorite");
const imageMenuEdit = document.getElementById("image-menu-edit");
const imageMenuDelete = document.getElementById("image-menu-delete");
const imageDetailsModal = document.getElementById("image-details-modal");
const imageDetailsTitle = document.getElementById("image-details-title");
const imageDetailsClose = document.getElementById("image-details-close");
const imageDetailsCancel = document.getElementById("image-details-cancel");
const imageDetailsSave = document.getElementById("image-details-save");
const imageDetailsName = document.getElementById("image-details-name");
const imageDetailsTags = document.getElementById("image-details-tags");
const imageDetailsAddTag = document.getElementById("image-details-add-tag");
const imageDetailsTagList = document.getElementById("image-details-tag-list");
const imageDetailsError = document.getElementById("image-details-error");
const imageDetailsCategoryRow = document.getElementById("image-details-category-row");
const imageDetailsCategory = document.getElementById("image-details-category");
const imageDetailsNewCategory = document.getElementById("image-details-new-category");
const imageCategoryModal = document.getElementById("image-category-modal");
const imageCategoryClose = document.getElementById("image-category-close");
const imageCategoryCancel = document.getElementById("image-category-cancel");
const imageCategorySave = document.getElementById("image-category-save");
const imageCategoryName = document.getElementById("image-category-name");
const imageCategoryError = document.getElementById("image-category-error");
const metadataUri = document.getElementById("metadata-uri");
const nameInput = form.querySelector('[name="name"]');
const symbolInput = form.querySelector('[name="symbol"]');
const descriptionInput = form.querySelector('[name="description"]');
const descriptionDisclosure = document.getElementById("description-disclosure");
const descriptionToggle = document.getElementById("description-toggle");
const descriptionPanelBody = document.getElementById("description-panel-body");
const descriptionCharCount = document.getElementById("description-char-count");
const nameCharCount = document.getElementById("name-char-count");
const symbolCharCount = document.getElementById("symbol-char-count");
const tickerCapsToggle = document.getElementById("ticker-caps-toggle");
const devBuyModeInput = getNamedInput("devBuyMode");
const devBuyAmountInput = getNamedInput("devBuyAmount");
const postLaunchStrategyInput = getNamedInput("postLaunchStrategy");
const snipeBuyAmountInput = getNamedInput("snipeBuyAmountSol");
const sniperEnabledInput = getNamedInput("sniperEnabled");
const sniperConfigJsonInput = getNamedInput("sniperConfigJson");
const vanityPrivateKeyInput = getNamedInput("vanityPrivateKey");
const devBuyQuickButtons = document.getElementById("dev-buy-quick-buttons");
const changeDevBuyPresetsButton = document.getElementById("change-dev-buy-presets-button");
const cancelDevBuyPresetsButton = document.getElementById("cancel-dev-buy-presets-button");
const saveDevBuyPresetsButton = document.getElementById("save-dev-buy-presets-button");
const devBuySolInput = document.getElementById("dev-buy-sol-input");
const devBuyPercentInput = document.getElementById("dev-buy-percent-input");
const devBuyCustomDeployButton = document.getElementById("dev-buy-custom-deploy");
const quoteOutput = document.getElementById("quote-output");
const creationTipInput = document.getElementById("creation-tip-input");
const creationPriorityInput = document.getElementById("creation-priority-input");
const launchpadInputs = Array.from(document.querySelectorAll('input[name="launchpad"]'));
const providerSelect = document.getElementById("provider-select");
const buyProviderSelect = document.getElementById("buy-provider-select");
const sellProviderSelect = document.getElementById("sell-provider-select");
const buyPriorityFeeInput = document.getElementById("buy-priority-fee-input");
const buyTipInput = document.getElementById("buy-tip-input");
const buySlippageInput = document.getElementById("buy-slippage-input");
const sellPriorityFeeInput = document.getElementById("sell-priority-fee-input");
const sellTipInput = document.getElementById("sell-tip-input");
const sellSlippageInput = document.getElementById("sell-slippage-input");
const settingsPresetChipBar = document.getElementById("settings-preset-chip-bar");
const presetEditToggle = document.getElementById("preset-edit-toggle");
const agentUnlockedAuthority = document.getElementById("agent-unlocked-authority");
const agentSplitList = document.getElementById("agent-split-list");
const agentSplitAdd = document.getElementById("agent-split-add");
const agentSplitReset = document.getElementById("agent-split-reset");
const agentSplitEven = document.getElementById("agent-split-even");
const agentSplitTotal = document.getElementById("agent-split-total");
const agentSplitBar = document.getElementById("agent-split-bar");
const agentSplitLegendList = document.getElementById("agent-split-legend-list");
const agentSplitModal = document.getElementById("agent-split-modal");
const agentSplitClose = document.getElementById("agent-split-close");
const agentSplitCancel = document.getElementById("agent-split-cancel");
const agentSplitSave = document.getElementById("agent-split-save");
const agentSplitModalError = document.getElementById("agent-split-modal-error");
const feeSplitEnabled = form.querySelector('[name="feeSplitEnabled"]');
const feeSplitList = document.getElementById("fee-split-list");
const feeSplitAdd = document.getElementById("fee-split-add");
const feeSplitReset = document.getElementById("fee-split-reset");
const feeSplitEven = document.getElementById("fee-split-even");
const feeSplitTotal = document.getElementById("fee-split-total");
const feeSplitBar = document.getElementById("fee-split-bar");
const feeSplitLegendList = document.getElementById("fee-split-legend-list");
const feeSplitModal = document.getElementById("fee-split-modal");
const feeSplitClose = document.getElementById("fee-split-close");
const feeSplitDisable = document.getElementById("fee-split-disable");
const feeSplitSave = document.getElementById("fee-split-save");
const deployModal = document.getElementById("deploy-modal");
const modalBody = document.getElementById("modal-body");
const modalClose = document.getElementById("modal-close");
const modalCancel = document.getElementById("modal-cancel");
const modalConfirm = document.getElementById("modal-confirm");
const testFillButton = document.getElementById("test-fill-button");
const openPopoutButton = document.getElementById("open-popout-button");
const toggleOutputButton = document.getElementById("toggle-output-button");
const openSettingsButton = document.getElementById("open-settings-button");
const saveSettingsButton = document.getElementById("save-settings-button");
const settingsModal = document.getElementById("settings-modal");
const settingsClose = document.getElementById("settings-close");
const settingsCancel = document.getElementById("settings-cancel");
const modeSniperButton = document.getElementById("mode-sniper-button");
const modeVanityButton = document.getElementById("mode-vanity-button");
const devAutoSellButton = document.getElementById("dev-auto-sell-button");
const devAutoSellPanel = document.getElementById("dev-auto-sell-panel");
const autoSellEnabledInput = document.getElementById("auto-sell-enabled-input");
const autoSellToggleState = document.getElementById("auto-sell-toggle-state");
const autoSellDelaySlider = document.getElementById("auto-sell-delay-slider");
const autoSellPercentSlider = document.getElementById("auto-sell-percent-slider");
const autoSellDelayValue = document.getElementById("auto-sell-delay-value");
const autoSellPercentValue = document.getElementById("auto-sell-percent-value");
const autoSellSettings = document.getElementById("auto-sell-settings");
const sniperModal = document.getElementById("sniper-modal");
const sniperClose = document.getElementById("sniper-close");
const sniperCancel = document.getElementById("sniper-cancel");
const sniperSave = document.getElementById("sniper-save");
const sniperRefreshButton = document.getElementById("sniper-refresh-button");
const sniperResetButton = document.getElementById("sniper-reset-button");
const sniperEnabledToggle = document.getElementById("sniper-enabled-toggle");
const sniperEnabledState = document.getElementById("sniper-enabled-state");
const sniperWalletsSection = document.getElementById("sniper-wallets-section");
const sniperWalletList = document.getElementById("sniper-wallet-list");
const sniperSelectionSummary = document.getElementById("sniper-selection-summary");
const sniperModalError = document.getElementById("sniper-modal-error");
const vanityModal = document.getElementById("vanity-modal");
const vanityClose = document.getElementById("vanity-close");
const vanitySave = document.getElementById("vanity-save");
const vanityClear = document.getElementById("vanity-clear");
const vanityPrivateKeyText = document.getElementById("vanity-private-key-input");
const vanityModalError = document.getElementById("vanity-modal-error");
const vampModal = document.getElementById("vamp-modal");
const vampClose = document.getElementById("vamp-close");
const vampCancel = document.getElementById("vamp-cancel");
const vampImport = document.getElementById("vamp-import");
const vampContractInput = document.getElementById("vamp-contract-input");
const vampStatus = document.getElementById("vamp-status");
const vampError = document.getElementById("vamp-error");
const OUTPUT_SECTION_VISIBILITY_KEY = "launchdeck.outputSectionVisible";
const THEME_MODE_STORAGE_KEY = "launchdeck.themeMode";
const pageSearchParams = new URLSearchParams(window.location.search);
const isPopoutMode = pageSearchParams.get("popout") === "1";
const bootstrapState = window.__LAUNCHDECK_BOOTSTRAP__ && typeof window.__LAUNCHDECK_BOOTSTRAP__ === "object"
  ? window.__LAUNCHDECK_BOOTSTRAP__
  : null;
const DEFAULT_LAUNCHPAD_TOKEN_METADATA = Object.freeze({
  nameMaxLength: 32,
  symbolMaxLength: 10,
});

if (isPopoutMode) {
  document.body.classList.add("popout-mode");
  document.title = "LaunchDeck Popout";
}

setThemeMode(getStoredThemeMode(), { persist: false });
setOutputSectionVisible(getStoredOutputSectionVisible());

let uploadedImage = null;
let latestWalletStatus = bootstrapState && bootstrapState.config
  ? { config: bootstrapState.config }
  : null;
let latestLaunchpadRegistry = bootstrapState && bootstrapState.launchpads && typeof bootstrapState.launchpads === "object"
  ? bootstrapState.launchpads
  : {};
let quoteTimer = null;
let defaultsApplied = false;
let imageLibraryState = {
  images: [],
  categories: [],
  search: "",
  category: "all",
  activeImageId: "",
};
let activeImageMenuId = "";
let activeImageDetailsId = "";
let imageDetailsTagsState = [];
let isEditingNewImageUpload = false;
let imageCategoryModalContext = "library";
let tickerManuallyEdited = false;
let syncingTickerFromName = false;
let tickerClearedForManualEntry = false;
let syncingDevBuyInputs = false;
let devBuyPresetEditorOpen = false;
let lastDevBuyEditSource = "sol";
let syncingPresetInputs = false;
let lastTopPresetMarkup = "";
let lastSettingsPresetMarkup = "";
let lastQuickDevBuyMarkup = "";
let sniperState = {
  enabled: false,
  wallets: {},
};
const SPLIT_COLORS = ["#5b7cff", "#ff5d5d", "#14c38e", "#ffb020", "#7c5cff", "#00b8d9", "#ef5da8", "#8b5cf6"];
const DEFAULT_QUICK_DEV_BUY_AMOUNTS = ["0.5", "1", "2"];
const DEFAULT_PRESET_ID = "preset1";
const SNIPER_BALANCE_PRESETS = [
  { label: "MAX", ratio: 1 },
  { label: "75%", ratio: 0.75 },
  { label: "50%", ratio: 0.5 },
  { label: "25%", ratio: 0.25 },
];
const PROVIDER_LABELS = {
  helius: "Helius",
  jito: "Jito",
  astralane: "Astralane",
  bloxroute: "bloXroute",
  hellomoon: "Hello Moon",
};
const ROUTE_CAPABILITIES = {
  helius: {
    creation: { tip: true, priority: true, slippage: false },
    buy: { tip: true, priority: true, slippage: true },
    sell: { tip: true, priority: true, slippage: true },
  },
  jito: {
    creation: { tip: true, priority: true, slippage: false },
    buy: { tip: true, priority: true, slippage: true },
    sell: { tip: true, priority: true, slippage: true },
  },
  astralane: {
    creation: { tip: true, priority: true, slippage: false },
    buy: { tip: true, priority: true, slippage: true },
    sell: { tip: true, priority: true, slippage: true },
  },
  bloxroute: {
    creation: { tip: true, priority: true, slippage: false },
    buy: { tip: true, priority: true, slippage: true },
    sell: { tip: true, priority: true, slippage: true },
  },
  hellomoon: {
    creation: { tip: true, priority: true, slippage: false },
    buy: { tip: true, priority: true, slippage: true },
    sell: { tip: true, priority: true, slippage: true },
  },
};
const TOTAL_SUPPLY_TOKENS = 1_000_000_000n;
const TOKEN_DECIMALS = 6;
const TEST_PRESET = {
  name: "test",
  symbol: "test",
  description: "test",
  website: "https://test.com/",
  twitter: "https://x.com/test",
  telegram: "https://t.me/test",
  devBuyMode: "sol",
  devBuyAmount: "0.1",
  creationTipSol: "0.01",
  creationPriorityFeeSol: "0.001",
  buyPriorityFeeSol: "0.009",
  buyTipSol: "0.01",
  buySlippagePercent: "90",
  sellPriorityFeeSol: "0.009",
  sellTipSol: "0.01",
  sellSlippagePercent: "90",
  skipPreflight: "false",
};

function setBusy(busy, label) {
  statusNode.textContent = label;
  buttons.forEach((button) => {
    button.disabled = busy;
  });
  if (openSettingsButton) openSettingsButton.disabled = busy;
  if (modeSniperButton) modeSniperButton.disabled = busy;
  if (modeVanityButton) modeVanityButton.disabled = busy;
  if (devAutoSellButton) devAutoSellButton.disabled = busy;
  if (saveSettingsButton) saveSettingsButton.disabled = busy;
  if (changeDevBuyPresetsButton) changeDevBuyPresetsButton.disabled = busy;
  if (saveDevBuyPresetsButton) saveDevBuyPresetsButton.disabled = busy;
  if (cancelDevBuyPresetsButton) cancelDevBuyPresetsButton.disabled = busy;
  if (devBuyCustomDeployButton) devBuyCustomDeployButton.disabled = busy;
}

function getNamedInput(name) {
  return document.querySelector(`[name="${name}"]`);
}

function getNamedValue(name) {
  const input = getNamedInput(name);
  return input ? input.value : "";
}

function setNamedValue(name, value) {
  const input = getNamedInput(name);
  if (input) input.value = value;
}

function isNamedChecked(name) {
  const input = getNamedInput(name);
  return Boolean(input && input.checked);
}

function formatSliderValue(value, suffix, digits = 0) {
  const numeric = Number(value || 0);
  if (!Number.isFinite(numeric)) return `0${suffix}`;
  return `${numeric.toFixed(digits)}${suffix}`;
}

function normalizeDecimalInput(value, maxDecimals = 6) {
  const raw = String(value || "").replace(/,/g, ".").trim();
  if (!raw) return "";
  const sanitized = raw.replace(/[^\d.]/g, "");
  const [whole = "", fractional = ""] = sanitized.split(".");
  const safeWhole = whole.replace(/^0+(?=\d)/, "") || (whole ? "0" : "");
  return fractional !== undefined && sanitized.includes(".")
    ? `${safeWhole || "0"}.${fractional.slice(0, maxDecimals)}`
    : safeWhole;
}

function updateDescriptionDisclosure() {
  const currentLength = descriptionInput ? descriptionInput.value.length : 0;
  const maxLength = descriptionInput ? Number(descriptionInput.getAttribute("maxlength") || 500) : 500;
  const expanded = Boolean(descriptionPanelBody && !descriptionPanelBody.hidden);
  if (descriptionCharCount) descriptionCharCount.textContent = `${currentLength}/${maxLength}`;
  if (descriptionToggle) descriptionToggle.setAttribute("aria-expanded", expanded ? "true" : "false");
  if (descriptionDisclosure) {
    descriptionDisclosure.classList.toggle("is-open", expanded);
    descriptionDisclosure.classList.toggle("has-content", currentLength > 0);
  }
}

function toggleDescriptionDisclosure(forceOpen) {
  if (!descriptionPanelBody) return;
  const nextOpen = typeof forceOpen === "boolean" ? forceOpen : descriptionPanelBody.hidden;
  descriptionPanelBody.hidden = !nextOpen;
  updateDescriptionDisclosure();
}

function parseDecimalToBigInt(rawValue, decimals) {
  const raw = String(rawValue || "").trim();
  if (!raw) return 0n;
  if (!/^\d+(\.\d+)?$/.test(raw)) {
    throw new Error("Invalid decimal input.");
  }
  const [whole, fractional = ""] = raw.split(".");
  if (fractional.length > decimals) {
    throw new Error(`Too many decimal places (max ${decimals}).`);
  }
  const combined = `${whole}${fractional.padEnd(decimals, "0")}`.replace(/^0+(?=\d)/, "");
  return BigInt(combined || "0");
}

function formatBigIntDecimal(value, decimals, maxFractionDigits = decimals) {
  const negative = value < 0n;
  const absolute = negative ? -value : value;
  const base = 10n ** BigInt(decimals);
  const whole = absolute / base;
  const fraction = absolute % base;
  if (fraction === 0n) return `${negative ? "-" : ""}${whole.toString()}`;
  let fractionText = fraction.toString().padStart(decimals, "0").slice(0, maxFractionDigits);
  fractionText = fractionText.replace(/0+$/, "");
  return `${negative ? "-" : ""}${whole.toString()}${fractionText ? `.${fractionText}` : ""}`;
}

function getQuickDevBuyPresetAmounts(config = latestWalletStatus && latestWalletStatus.config) {
  const presetItems = config && config.presets && Array.isArray(config.presets.items)
    ? config.presets.items
    : [];
  return DEFAULT_QUICK_DEV_BUY_AMOUNTS.map((fallback, index) => {
    const preset = presetItems[index];
    const value = preset && preset.creationSettings && typeof preset.creationSettings.devBuySol === "string"
      ? preset.creationSettings.devBuySol.trim()
      : "";
    return value || fallback;
  });
}

function renderQuickDevBuyButtons(config = latestWalletStatus && latestWalletStatus.config) {
  if (!devBuyQuickButtons) return;
  const presetItems = getPresetItems(config);
  const markup = presetItems.map((preset, index) => {
    const amount = preset.creationSettings.devBuySol || DEFAULT_QUICK_DEV_BUY_AMOUNTS[index];
    if (devBuyPresetEditorOpen) {
      return `
        <label class="dev-buy-quick-button dev-buy-quick-button-editing" data-quick-buy-index="${index}">
          <span class="dev-buy-quick-content dev-buy-quick-content-editing">
            <input
              type="text"
              inputmode="decimal"
              class="dev-buy-quick-value dev-buy-quick-editor-input"
              data-dev-buy-preset-input="${index}"
              value="${escapeHTML(amount)}"
              placeholder="${escapeHTML(DEFAULT_QUICK_DEV_BUY_AMOUNTS[index])}"
            >
          </span>
        </label>
      `;
    }
    return `
      <button type="button" class="dev-buy-quick-button" data-quick-buy-index="${index}" data-quick-buy-preset-id="${escapeHTML(preset.id)}" data-quick-buy-amount="${escapeHTML(amount)}">
        <span class="dev-buy-quick-content">
          <img src="/solana-mark.png" alt="SOL" class="sol-logo inline-sol-logo quick-buy-sol-logo">
          <strong class="dev-buy-quick-value">${escapeHTML(amount)}</strong>
        </span>
      </button>
    `;
  }).join("");
  if (markup === lastQuickDevBuyMarkup) return;
  devBuyQuickButtons.innerHTML = markup;
  lastQuickDevBuyMarkup = markup;
}

function populateDevBuyPresetEditor(config = latestWalletStatus && latestWalletStatus.config) {
  const amounts = getQuickDevBuyPresetAmounts(config);
  getDevBuyPresetEditorInputs().forEach((input, index) => {
    if (input) input.value = amounts[index] || "";
  });
}

function getDevBuyPresetEditorInputs() {
  return devBuyQuickButtons
    ? Array.from(devBuyQuickButtons.querySelectorAll("[data-dev-buy-preset-input]"))
    : [];
}

function setDevBuyPresetEditorOpen(isOpen) {
  devBuyPresetEditorOpen = Boolean(isOpen);
  if (changeDevBuyPresetsButton) {
    changeDevBuyPresetsButton.hidden = devBuyPresetEditorOpen;
    changeDevBuyPresetsButton.setAttribute("aria-expanded", devBuyPresetEditorOpen ? "true" : "false");
  }
  if (cancelDevBuyPresetsButton) cancelDevBuyPresetsButton.hidden = !devBuyPresetEditorOpen;
  if (saveDevBuyPresetsButton) saveDevBuyPresetsButton.hidden = !devBuyPresetEditorOpen;
  renderQuickDevBuyButtons(getConfig());
}

function buildConfigWithUpdatedDevBuyPresets() {
  const config = cloneConfig(getConfig()) || createFallbackConfig();
  const presetItems = getPresetItems(config);
  const editorInputs = getDevBuyPresetEditorInputs();
  config.presets = config.presets || {};
  config.presets.items = presetItems.map((preset, index) => {
    const input = editorInputs[index];
    const nextValue = input ? String(input.value || "").trim() : "";
    return {
      ...preset,
      creationSettings: {
        ...(preset.creationSettings || {}),
        devBuySol: nextValue,
      },
    };
  });
  return config;
}

async function saveDevBuyPresetEditor() {
  const nextConfig = buildConfigWithUpdatedDevBuyPresets();
  if (saveDevBuyPresetsButton) saveDevBuyPresetsButton.disabled = true;
  if (cancelDevBuyPresetsButton) cancelDevBuyPresetsButton.disabled = true;
  if (changeDevBuyPresetsButton) changeDevBuyPresetsButton.disabled = true;
  try {
    const response = await fetch("/api/settings/save", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ config: nextConfig }),
    });
    const payload = await response.json();
    if (!response.ok || !payload.ok) {
      throw new Error(payload.error || "Failed to save quick deploy presets.");
    }
    setConfig(payload.config);
    if (latestWalletStatus) latestWalletStatus.config = payload.config;
    renderQuickDevBuyButtons(payload.config);
    populateDevBuyPresetEditor(payload.config);
    setDevBuyPresetEditorOpen(false);
  } catch (error) {
    statusNode.textContent = "Error";
    output.textContent = error.message;
  } finally {
    if (saveDevBuyPresetsButton) saveDevBuyPresetsButton.disabled = false;
    if (cancelDevBuyPresetsButton) cancelDevBuyPresetsButton.disabled = false;
    if (changeDevBuyPresetsButton) changeDevBuyPresetsButton.disabled = false;
  }
}

function cloneConfig(value) {
  return value ? JSON.parse(JSON.stringify(value)) : null;
}

function createFallbackConfig() {
  return {
    defaults: {
      launchpad: "pump",
      mode: "regular",
      activePresetId: DEFAULT_PRESET_ID,
      presetEditing: false,
    },
    presets: {
      items: DEFAULT_QUICK_DEV_BUY_AMOUNTS.map((amount, index) => ({
        id: `preset${index + 1}`,
        label: `P${index + 1}`,
        creationSettings: {
          provider: "helius",
          policy: "safe",
          tipSol: "0.01",
          priorityFeeSol: "0.001",
          devBuySol: amount,
        },
        buySettings: {
          provider: "helius",
          policy: "safe",
          priorityFeeSol: "0.009",
          tipSol: "0.01",
          slippagePercent: "90",
          snipeBuyAmountSol: "",
        },
        sellSettings: {
          provider: "helius",
          policy: "safe",
          priorityFeeSol: "0.009",
          tipSol: "0.01",
          slippagePercent: "90",
        },
        automaticDevSell: {
          enabled: false,
          percent: 0,
          delaySeconds: 0,
        },
        postLaunchStrategy: "none",
      })),
    },
  };
}

function getConfig() {
  return latestWalletStatus && latestWalletStatus.config
    ? latestWalletStatus.config
    : createFallbackConfig();
}

function getPresetItems(config = getConfig()) {
  return config && config.presets && Array.isArray(config.presets.items)
    ? config.presets.items
    : createFallbackConfig().presets.items;
}

function getActivePresetId(config = getConfig()) {
  return config && config.defaults && config.defaults.activePresetId
    ? config.defaults.activePresetId
    : DEFAULT_PRESET_ID;
}

function getActivePreset(config = getConfig()) {
  const items = getPresetItems(config);
  return items.find((entry) => entry.id === getActivePresetId(config)) || items[0];
}

function getPresetDisplayLabel(preset, index = 0) {
  const rawLabel = String((preset && preset.label) || "").trim();
  const labelMatch = rawLabel.match(/^preset\s*([0-9]+)$/i);
  if (labelMatch) return `P${labelMatch[1]}`;
  const idMatch = String((preset && preset.id) || "").trim().match(/^preset([0-9]+)$/i);
  if (!rawLabel && idMatch) return `P${idMatch[1]}`;
  return rawLabel || `P${index + 1}`;
}

function isPresetEditing(config = getConfig()) {
  return Boolean(config && config.defaults && config.defaults.presetEditing);
}

function setConfig(nextConfig) {
  if (!latestWalletStatus) {
    latestWalletStatus = {
      connected: false,
      config: cloneConfig(nextConfig),
    };
  } else {
    latestWalletStatus = {
      ...latestWalletStatus,
      config: cloneConfig(nextConfig),
    };
  }
  renderPresetChips();
  renderQuickDevBuyButtons(nextConfig);
}

function renderPresetChipMarkup(config = getConfig(), { topBar = false } = {}) {
  const activePresetId = getActivePresetId(config);
  return getPresetItems(config).map((preset, index) => `
    <button
      type="button"
      class="preset-chip${preset.id === activePresetId ? " active" : ""}${topBar ? " compact" : ""}"
      data-preset-id="${escapeHTML(preset.id)}"
    >
      ${escapeHTML(getPresetDisplayLabel(preset, index))}
    </button>
  `).join("");
}

function renderPresetChips() {
  const config = getConfig();
  const topMarkup = renderPresetChipMarkup(config, { topBar: true });
  const settingsMarkup = renderPresetChipMarkup(config, { topBar: false });
  if (topPresetChipBar && topMarkup !== lastTopPresetMarkup) {
    topPresetChipBar.innerHTML = topMarkup;
    lastTopPresetMarkup = topMarkup;
  }
  if (settingsPresetChipBar && settingsMarkup !== lastSettingsPresetMarkup) {
    settingsPresetChipBar.innerHTML = settingsMarkup;
    lastSettingsPresetMarkup = settingsMarkup;
  }
  if (presetEditToggle) {
    const editing = isPresetEditing(config);
    presetEditToggle.classList.toggle("active", editing);
    presetEditToggle.setAttribute("aria-pressed", editing ? "true" : "false");
    presetEditToggle.innerHTML = editing ? "Lock" : "&#9998;";
    presetEditToggle.title = editing ? "Lock active preset" : "Unlock active preset for editing";
  }
}

function setDevBuyHiddenState(mode, amount) {
  if (devBuyModeInput) devBuyModeInput.value = mode || "sol";
  if (devBuyAmountInput) devBuyAmountInput.value = amount || "";
}

function clearDevBuyState() {
  setDevBuyHiddenState("sol", "");
  syncingDevBuyInputs = true;
  if (devBuySolInput) devBuySolInput.value = "";
  if (devBuyPercentInput) devBuyPercentInput.value = "";
  syncingDevBuyInputs = false;
  quoteOutput.textContent = "No dev buy selected.";
  syncActivePresetDevBuy("");
}

function syncActivePresetDevBuy(amount) {
  if (!isPresetEditing(getConfig())) return;
  const config = cloneConfig(getConfig());
  const activePreset = getActivePreset(config);
  if (!activePreset) return;
  activePreset.creationSettings = {
    ...activePreset.creationSettings,
    devBuySol: String(amount || "").trim(),
  };
  setConfig(config);
}

function percentToTokenAmount(percentValue) {
  const percentRaw = parseDecimalToBigInt(percentValue, 4);
  const rawTokens = (TOTAL_SUPPLY_TOKENS * (10n ** BigInt(TOKEN_DECIMALS)) * percentRaw) / 1_000_000n;
  return formatBigIntDecimal(rawTokens, TOKEN_DECIMALS, TOKEN_DECIMALS);
}

async function updateDevBuyFromSolInput(value) {
  const amount = normalizeDecimalInput(value, 9);
  syncingDevBuyInputs = true;
  if (devBuySolInput) devBuySolInput.value = amount;
  syncingDevBuyInputs = false;
  lastDevBuyEditSource = "sol";
  if (!amount) {
    clearDevBuyState();
    return;
  }
  setDevBuyHiddenState("sol", amount);
  syncActivePresetDevBuy(amount);
  await updateQuote();
}

async function updateDevBuyFromPercentInput(value) {
  const percent = normalizeDecimalInput(value, 4);
  syncingDevBuyInputs = true;
  if (devBuyPercentInput) devBuyPercentInput.value = percent;
  syncingDevBuyInputs = false;
  lastDevBuyEditSource = "percent";
  if (!percent) {
    clearDevBuyState();
    return;
  }
  try {
    const tokenAmount = percentToTokenAmount(percent);
    if (!tokenAmount || Number(percent) <= 0) {
      clearDevBuyState();
      return;
    }
    setDevBuyHiddenState("tokens", tokenAmount);
    await updateQuote();
  } catch (_error) {
    setDevBuyHiddenState("tokens", "");
    quoteOutput.textContent = "Enter a valid percentage.";
  }
}

async function triggerDeployWithDevBuy(mode, amount, source = "sol") {
  setDevBuyHiddenState(mode, amount);
  lastDevBuyEditSource = source;
  if (source === "sol") {
    syncingDevBuyInputs = true;
    if (devBuySolInput) devBuySolInput.value = amount;
    syncingDevBuyInputs = false;
    await updateQuote();
  } else {
    await updateQuote();
  }
  const errors = validateForm();
  if (showValidationErrors(errors)) return;
  clearValidationErrors();
  await run("deploy");
}

function isTickerCapsEnabled() {
  return Boolean(tickerCapsToggle && tickerCapsToggle.classList.contains("active"));
}

function getLaunchpadTokenMetadata(launchpad = getLaunchpad()) {
  const entry = latestLaunchpadRegistry && latestLaunchpadRegistry[launchpad];
  const metadata = entry && entry.tokenMetadata ? entry.tokenMetadata : {};
  const nameMaxLength = Number(metadata.nameMaxLength || DEFAULT_LAUNCHPAD_TOKEN_METADATA.nameMaxLength);
  const symbolMaxLength = Number(metadata.symbolMaxLength || DEFAULT_LAUNCHPAD_TOKEN_METADATA.symbolMaxLength);
  return {
    nameMaxLength: Number.isFinite(nameMaxLength) && nameMaxLength > 0
      ? nameMaxLength
      : DEFAULT_LAUNCHPAD_TOKEN_METADATA.nameMaxLength,
    symbolMaxLength: Number.isFinite(symbolMaxLength) && symbolMaxLength > 0
      ? symbolMaxLength
      : DEFAULT_LAUNCHPAD_TOKEN_METADATA.symbolMaxLength,
  };
}

function formatTickerValue(value) {
  const { symbolMaxLength } = getLaunchpadTokenMetadata();
  const normalized = String(value || "").replace(/\s+/g, " ").trimStart();
  const clipped = normalized.slice(0, symbolMaxLength);
  return isTickerCapsEnabled() ? clipped.toUpperCase() : clipped;
}

function getAutoTickerValue() {
  return formatTickerValue(nameInput ? nameInput.value : "");
}

function updateTokenFieldCounts() {
  const { nameMaxLength, symbolMaxLength } = getLaunchpadTokenMetadata();
  if (nameInput) {
    nameInput.maxLength = nameMaxLength;
    if (nameInput.value.length > nameMaxLength) {
      nameInput.value = nameInput.value.slice(0, nameMaxLength);
    }
  }
  if (symbolInput) {
    symbolInput.maxLength = symbolMaxLength;
    const formatted = formatTickerValue(symbolInput.value);
    if (symbolInput.value !== formatted) {
      syncingTickerFromName = true;
      symbolInput.value = formatted;
      syncingTickerFromName = false;
    }
  }
  if (nameCharCount && nameInput) {
    nameCharCount.textContent = `${nameInput.value.length}/${nameMaxLength}`;
  }
  if (symbolCharCount && symbolInput) {
    symbolCharCount.textContent = `${symbolInput.value.length}/${symbolMaxLength}`;
  }
}

function applyLaunchpadTokenMetadata() {
  updateTokenFieldCounts();
  if (!tickerManuallyEdited) {
    syncTickerFromName();
    return;
  }
  applyTickerCapsMode();
}

function syncTickerFromName() {
  if (!nameInput || !symbolInput || tickerManuallyEdited) {
    updateTokenFieldCounts();
    return;
  }
  syncingTickerFromName = true;
  symbolInput.value = getAutoTickerValue();
  syncingTickerFromName = false;
  tickerClearedForManualEntry = false;
  updateTokenFieldCounts();
}

function applyTickerCapsMode() {
  if (!symbolInput) return;
  syncingTickerFromName = true;
  symbolInput.value = formatTickerValue(symbolInput.value);
  syncingTickerFromName = false;
  updateTokenFieldCounts();
}

function getMode() {
  const checked = form.querySelector('input[name="mode"]:checked');
  return checked ? checked.value : "regular";
}

function getDevBuyMode() {
  const explicit = getNamedValue("devBuyMode");
  return explicit || "sol";
}

function getLaunchpad() {
  const checked = document.querySelector('input[name="launchpad"]:checked');
  return checked ? checked.value : "pump";
}

function getProvider() {
  return providerSelect ? providerSelect.value || "helius" : "helius";
}

function getPolicy() {
  return "safe";
}

function getBuyProvider() {
  return buyProviderSelect ? buyProviderSelect.value || "helius" : "helius";
}

function getBuyPolicy() {
  return "safe";
}

function getSellProvider() {
  return sellProviderSelect ? sellProviderSelect.value || "helius" : "helius";
}

function getSellPolicy() {
  return "safe";
}

function getRouteCapabilities(route, rowType) {
  const normalizedRoute = String(route || "helius").trim().toLowerCase();
  return ROUTE_CAPABILITIES[normalizedRoute] && ROUTE_CAPABILITIES[normalizedRoute][rowType]
    ? ROUTE_CAPABILITIES[normalizedRoute][rowType]
    : ROUTE_CAPABILITIES.helius[rowType];
}

function setFieldEnabled(input, enabled) {
  if (!input) return;
  input.disabled = !enabled;
  const label = input.closest("label");
  if (label) label.classList.toggle("is-disabled", !enabled);
}

function syncSettingsCapabilities() {
  const editing = isPresetEditing(getConfig());
  const creationCapabilities = getRouteCapabilities(getProvider(), "creation");
  const buyCapabilities = getRouteCapabilities(getBuyProvider(), "buy");
  const sellCapabilities = getRouteCapabilities(getSellProvider(), "sell");

  if (providerSelect) providerSelect.disabled = !editing;
  if (buyProviderSelect) buyProviderSelect.disabled = !editing;
  if (sellProviderSelect) sellProviderSelect.disabled = !editing;
  setFieldEnabled(creationTipInput, editing && creationCapabilities.tip);
  setFieldEnabled(creationPriorityInput, editing && creationCapabilities.priority);
  setFieldEnabled(buyPriorityFeeInput, editing && buyCapabilities.priority);
  setFieldEnabled(buyTipInput, editing && buyCapabilities.tip);
  setFieldEnabled(buySlippageInput, editing && buyCapabilities.slippage);
  setFieldEnabled(sellPriorityFeeInput, editing && sellCapabilities.priority);
  setFieldEnabled(sellTipInput, editing && sellCapabilities.tip);
  setFieldEnabled(sellSlippageInput, editing && sellCapabilities.slippage);
}

function applyPresetToSettingsInputs(preset, options = {}) {
  if (!preset) return;
  const { syncToMainForm = true } = options;
  syncingPresetInputs = true;
  if (providerSelect) providerSelect.value = preset.creationSettings.provider || "helius";
  if (creationTipInput) creationTipInput.value = preset.creationSettings.tipSol || "";
  if (creationPriorityInput) creationPriorityInput.value = preset.creationSettings.priorityFeeSol || "";
  if (buyProviderSelect) buyProviderSelect.value = preset.buySettings.provider || "helius";
  if (buyPriorityFeeInput) buyPriorityFeeInput.value = preset.buySettings.priorityFeeSol || "";
  if (buyTipInput) buyTipInput.value = preset.buySettings.tipSol || "";
  if (buySlippageInput) buySlippageInput.value = preset.buySettings.slippagePercent || "";
  if (sellProviderSelect) sellProviderSelect.value = preset.sellSettings.provider || "helius";
  if (sellPriorityFeeInput) sellPriorityFeeInput.value = preset.sellSettings.priorityFeeSol || "";
  if (sellTipInput) sellTipInput.value = preset.sellSettings.tipSol || "";
  if (sellSlippageInput) sellSlippageInput.value = preset.sellSettings.slippagePercent || "";
  syncingPresetInputs = false;

  if (syncToMainForm) {
    clearDevBuyState();
  }

  syncDevAutoSellUI();
  syncSettingsCapabilities();
  renderPresetChips();
  renderQuickDevBuyButtons(getConfig());
}

function syncActivePresetFromInputs() {
  if (syncingPresetInputs) return;
  const config = cloneConfig(getConfig());
  const activePreset = getActivePreset(config);
  if (!activePreset) return;
  activePreset.creationSettings = {
    ...activePreset.creationSettings,
    provider: getProvider(),
    policy: getPolicy(),
    tipSol: creationTipInput ? creationTipInput.value.trim() : "",
    priorityFeeSol: creationPriorityInput ? creationPriorityInput.value.trim() : "",
    devBuySol: activePreset.creationSettings && activePreset.creationSettings.devBuySol
      ? activePreset.creationSettings.devBuySol.trim()
      : "",
  };
  activePreset.buySettings = {
    ...activePreset.buySettings,
    provider: getBuyProvider(),
    policy: getBuyPolicy(),
    priorityFeeSol: buyPriorityFeeInput ? buyPriorityFeeInput.value.trim() : "",
    tipSol: buyTipInput ? buyTipInput.value.trim() : "",
    slippagePercent: buySlippageInput ? buySlippageInput.value.trim() : "",
  };
  activePreset.sellSettings = {
    ...activePreset.sellSettings,
    provider: getSellProvider(),
    policy: getSellPolicy(),
    priorityFeeSol: sellPriorityFeeInput ? sellPriorityFeeInput.value.trim() : "",
    tipSol: sellTipInput ? sellTipInput.value.trim() : "",
    slippagePercent: sellSlippageInput ? sellSlippageInput.value.trim() : "",
  };
  setConfig(config);
  syncSettingsCapabilities();
}

function setActivePreset(presetId, options = {}) {
  const config = cloneConfig(getConfig());
  const exists = getPresetItems(config).some((entry) => entry.id === presetId);
  config.defaults = {
    ...(config.defaults || {}),
    activePresetId: exists ? presetId : DEFAULT_PRESET_ID,
  };
  setConfig(config);
  applyPresetToSettingsInputs(getActivePreset(config), options);
}

function setPresetEditing(editing) {
  const config = cloneConfig(getConfig());
  config.defaults = {
    ...(config.defaults || {}),
    presetEditing: Boolean(editing),
  };
  setConfig(config);
  const inputs = [
    providerSelect,
    creationTipInput,
    creationPriorityInput,
    buyProviderSelect,
    buyPriorityFeeInput,
    buyTipInput,
    buySlippageInput,
    sellProviderSelect,
    sellPriorityFeeInput,
    sellTipInput,
    sellSlippageInput,
  ];
  inputs.forEach((input) => {
    if (!input) return;
    input.disabled = !editing;
  });
  syncSettingsCapabilities();
}

function syncDevAutoSellUI() {
  const enabled = isNamedChecked("automaticDevSellEnabled");
  const delay = getNamedValue("automaticDevSellDelaySeconds") || "0";
  const percent = getNamedValue("automaticDevSellPercent") || "0";

  if (devAutoSellButton) devAutoSellButton.classList.toggle("active", enabled);
  if (autoSellToggleState) autoSellToggleState.textContent = enabled ? "ON" : "OFF";
  if (autoSellEnabledInput) autoSellEnabledInput.checked = enabled;
  if (autoSellSettings) autoSellSettings.hidden = !enabled;
  if (autoSellDelaySlider) {
    autoSellDelaySlider.value = delay;
    autoSellDelaySlider.disabled = !enabled;
  }
  if (autoSellPercentSlider) {
    autoSellPercentSlider.value = percent;
    autoSellPercentSlider.disabled = !enabled;
  }
  if (autoSellDelayValue) autoSellDelayValue.textContent = formatSliderValue(delay, "s", 1);
  if (autoSellPercentValue) autoSellPercentValue.textContent = formatSliderValue(percent, "%", 0);
  syncSettingsCapabilities();
}

function setSniperModalError(message = "") {
  if (sniperModalError) sniperModalError.textContent = message;
}

function getSniperSelectedEntries() {
  return Object.entries(sniperState.wallets || {})
    .filter(([, entry]) => entry && entry.selected)
    .map(([envKey, entry]) => ({
      envKey,
      amountSol: String(entry.amountSol || "").trim(),
    }));
}

function resetSniperState() {
  sniperState = {
    enabled: false,
    wallets: {},
  };
  applySniperStateToForm();
  renderSniperUI();
}

function applySniperStateToForm() {
  const selectedEntries = getSniperSelectedEntries().filter((entry) => Number(entry.amountSol) > 0);
  if (sniperEnabledInput) sniperEnabledInput.value = sniperState.enabled ? "true" : "false";
  if (sniperConfigJsonInput) sniperConfigJsonInput.value = JSON.stringify(selectedEntries);
  if (postLaunchStrategyInput) postLaunchStrategyInput.value = sniperState.enabled && selectedEntries.length > 0 ? "snipe-own-launch" : "none";
  if (snipeBuyAmountInput) {
    const total = selectedEntries.reduce((sum, entry) => sum + Number(entry.amountSol || 0), 0);
    snipeBuyAmountInput.value = total > 0 ? total.toFixed(6).replace(/\.?0+$/, "") : "";
  }
}

function renderSniperButtonState() {
  const selectedEntries = getSniperSelectedEntries().filter((entry) => Number(entry.amountSol) > 0);
  if (modeSniperButton) {
    modeSniperButton.classList.toggle("active", sniperState.enabled && selectedEntries.length > 0);
  }
}

function getWalletBalanceForSniper(wallet) {
  if (!wallet) return 0;
  if (wallet.balanceSol != null && Number.isFinite(Number(wallet.balanceSol))) {
    return Number(wallet.balanceSol);
  }
  if (latestWalletStatus && wallet.envKey === latestWalletStatus.selectedWalletKey) {
    return Number(latestWalletStatus.balanceSol || 0);
  }
  return 0;
}

function renderSniperWalletList() {
  if (!sniperWalletList) return;
  const wallets = latestWalletStatus && Array.isArray(latestWalletStatus.wallets) ? latestWalletStatus.wallets : [];
  const selectedKey = latestWalletStatus && latestWalletStatus.selectedWalletKey ? latestWalletStatus.selectedWalletKey : "";
  if (selectedKey && sniperState.wallets[selectedKey]) {
    sniperState.wallets[selectedKey] = {
      ...sniperState.wallets[selectedKey],
      selected: false,
      amountSol: "",
    };
    applySniperStateToForm();
    renderSniperButtonState();
  }
  const selectedCount = getSniperSelectedEntries().length;
  if (sniperSelectionSummary) {
    sniperSelectionSummary.textContent = `${selectedCount} wallet${selectedCount === 1 ? "" : "s"} selected`;
  }
  if (sniperWalletsSection) sniperWalletsSection.hidden = !sniperState.enabled;
  if (sniperEnabledState) sniperEnabledState.textContent = sniperState.enabled ? "ON" : "OFF";
  if (sniperEnabledToggle) sniperEnabledToggle.checked = sniperState.enabled;

  if (wallets.length === 0) {
    sniperWalletList.innerHTML = `<div class="sniper-wallet-empty muted">No wallets found in \`.env\`.</div>`;
    return;
  }

  sniperWalletList.innerHTML = wallets.map((wallet) => {
    const disabled = wallet.envKey === selectedKey;
    const balanceSol = getWalletBalanceForSniper(wallet);
    const state = sniperState.wallets[wallet.envKey] || { selected: false, amountSol: "" };
    return `
      <div class="sniper-wallet-row${disabled ? " is-disabled" : ""}${state.selected ? " is-selected" : ""}" data-sniper-wallet-row="${escapeHTML(wallet.envKey)}">
        <label class="sniper-wallet-main">
          <input
            type="checkbox"
            class="sniper-wallet-checkbox"
            data-sniper-wallet-checkbox="${escapeHTML(wallet.envKey)}"
            ${state.selected ? "checked" : ""}
            ${disabled ? "disabled" : ""}
          >
          <div class="sniper-wallet-info">
            <div class="sniper-wallet-name">${escapeHTML(`Imported SOL Key #${walletIndexFromEnvKey(wallet.envKey)}`)}</div>
            <div class="sniper-wallet-meta">
              <span>${escapeHTML(shortenAddress(wallet.publicKey || "invalid", 5))}</span>
              ${disabled ? '<span class="sniper-wallet-pill">Deployer</span>' : ""}
            </div>
          </div>
          <div class="sniper-wallet-balance">${Number(balanceSol).toFixed(3)}</div>
        </label>
        <div class="sniper-wallet-config"${!state.selected || disabled ? " hidden" : ""}>
          <label class="sniper-wallet-amount">
            <span>Amount</span>
            <input type="text" inputmode="decimal" value="${escapeHTML(state.amountSol || "")}" data-sniper-wallet-amount="${escapeHTML(wallet.envKey)}" placeholder="0">
          </label>
          <div class="sniper-wallet-presets">
            ${SNIPER_BALANCE_PRESETS.map((preset) => `
              <button type="button" class="button subtle sniper-preset-button" data-sniper-preset="${escapeHTML(wallet.envKey)}" data-sniper-ratio="${preset.ratio}">
                ${escapeHTML(preset.label)}
              </button>
            `).join("")}
          </div>
        </div>
      </div>
    `;
  }).join("");
}

function renderVanityButtonState() {
  if (!modeVanityButton) return;
  modeVanityButton.classList.toggle("active", Boolean(getNamedValue("vanityPrivateKey").trim()));
}

function renderSniperUI() {
  applySniperStateToForm();
  renderSniperButtonState();
  renderSniperWalletList();
}

function showSniperModal() {
  setSniperModalError("");
  renderSniperUI();
  if (sniperModal) sniperModal.hidden = false;
}

function hideSniperModal() {
  if (sniperModal) sniperModal.hidden = true;
}

function validateSniperState() {
  if (!sniperState.enabled) return [];
  const wallets = getSniperSelectedEntries();
  if (wallets.length === 0) return ["Select at least one sniper wallet."];
  const errors = [];
  wallets.forEach((entry) => {
    const amount = Number(entry.amountSol);
    if (!entry.amountSol || !Number.isFinite(amount) || amount <= 0) {
      errors.push(`Sniper wallet #${walletIndexFromEnvKey(entry.envKey)} needs a positive buy amount.`);
    }
  });
  return errors;
}

function showVanityModal() {
  if (vanityPrivateKeyText) vanityPrivateKeyText.value = getNamedValue("vanityPrivateKey");
  if (vanityModalError) vanityModalError.textContent = "";
  if (vanityModal) vanityModal.hidden = false;
}

function hideVanityModal() {
  if (vanityModal) vanityModal.hidden = true;
}

function showVampModal() {
  if (vampError) vampError.textContent = "";
  if (vampStatus) {
    vampStatus.hidden = true;
    vampStatus.textContent = "";
  }
  if (vampContractInput) vampContractInput.value = "";
  if (vampModal) vampModal.hidden = false;
  if (vampContractInput) queueMicrotask(() => vampContractInput.focus());
}

function hideVampModal() {
  if (vampModal) vampModal.hidden = true;
}

function setVampStatus(message = "") {
  if (!vampStatus) return;
  vampStatus.hidden = !message;
  vampStatus.textContent = message;
}

async function importVampToken() {
  const contractAddress = vampContractInput ? vampContractInput.value.trim() : "";
  if (!contractAddress) {
    if (vampError) vampError.textContent = "Contract address is required.";
    return;
  }
  if (vampError) vampError.textContent = "";
  setVampStatus("Importing token metadata...");
  if (vampImport) vampImport.disabled = true;
  if (vampCancel) vampCancel.disabled = true;
  if (vampClose) vampClose.disabled = true;
  try {
    const response = await fetch("/api/vamp", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ contractAddress }),
    });
    const payload = await response.json();
    if (!response.ok || !payload.ok) {
      throw new Error(payload.error || "Failed to import token metadata.");
    }

    if (nameInput) nameInput.value = payload.token && payload.token.name ? payload.token.name : "";
    if (symbolInput) {
      syncingTickerFromName = true;
      symbolInput.value = formatTickerValue(payload.token && payload.token.symbol ? payload.token.symbol : "");
      syncingTickerFromName = false;
      tickerManuallyEdited = Boolean(String(payload.token && payload.token.symbol ? payload.token.symbol : "").trim());
      tickerClearedForManualEntry = false;
    }
    if (descriptionInput) {
      descriptionInput.value = payload.token && payload.token.description ? payload.token.description : "";
      toggleDescriptionDisclosure(Boolean(descriptionInput.value.trim()));
      updateDescriptionDisclosure();
    }
    const websiteInput = form.querySelector('[name="website"]');
    const twitterInput = form.querySelector('[name="twitter"]');
    const telegramInput = form.querySelector('[name="telegram"]');
    if (websiteInput) websiteInput.value = payload.token && payload.token.website ? payload.token.website : "";
    if (twitterInput) twitterInput.value = payload.token && payload.token.twitter ? payload.token.twitter : "";
    if (telegramInput) telegramInput.value = payload.token && payload.token.telegram ? payload.token.telegram : "";
    metadataUri.value = "";
    updateTokenFieldCounts();

    if (payload.image) {
      imageLibraryState.activeImageId = payload.image.id || "";
      setSelectedImage(payload.image);
      try {
        await fetchImageLibrary();
      } catch (_error) {
        // Keep the imported image selected even if the library refresh fails.
      }
    }

    imageStatus.textContent = payload.image
      ? "Token image imported to library."
      : (payload.warning || "");
    imagePath.textContent = "";
    hideVampModal();
  } catch (error) {
    if (vampError) vampError.textContent = error.message;
    setVampStatus("");
  } finally {
    if (vampImport) vampImport.disabled = false;
    if (vampCancel) vampCancel.disabled = false;
    if (vampClose) vampClose.disabled = false;
  }
}

function applyVanityValue(rawValue) {
  const nextValue = String(rawValue || "").trim();
  if (vanityPrivateKeyInput) vanityPrivateKeyInput.value = nextValue;
  renderVanityButtonState();
}

function hydrateModeActionState() {
  const enabled = getNamedValue("sniperEnabled") === "true";
  let wallets = {};
  try {
    const parsed = JSON.parse(getNamedValue("sniperConfigJson") || "[]");
    if (Array.isArray(parsed)) {
      wallets = parsed.reduce((accumulator, entry) => {
        if (!entry || !entry.envKey) return accumulator;
        accumulator[entry.envKey] = {
          selected: true,
          amountSol: normalizeDecimalInput(entry.amountSol || ""),
        };
        return accumulator;
      }, {});
    }
  } catch (_error) {
    wallets = {};
  }
  sniperState = { enabled, wallets };
  renderSniperUI();
  renderVanityButtonState();
}

function hideImageItemMenu() {
  if (!imageItemMenu) return;
  imageItemMenu.hidden = true;
  imageItemMenu.style.left = "";
  imageItemMenu.style.top = "";
  activeImageMenuId = "";
}

function renderImageDetailsTags() {
  if (!imageDetailsTagList) return;
  imageDetailsTagList.innerHTML = imageDetailsTagsState.map((tag, index) => `
    <button type="button" class="image-tag-chip" data-image-tag-index="${index}">
      <span>${escapeHTML(tag)}</span>
      <span class="image-tag-chip-remove">&times;</span>
    </button>
  `).join("");
}

function setImageDetailsError(message = "") {
  if (imageDetailsError) imageDetailsError.textContent = message;
}

function normalizeImageTag(value) {
  return String(value || "").trim().replace(/\s+/g, " ").slice(0, 24);
}

function normalizeImageCategoryName(value) {
  return String(value || "").trim().replace(/\s+/g, " ").slice(0, 32);
}

function renderImageDetailsCategoryOptions(selectedCategory = "") {
  if (!imageDetailsCategory) return;
  const selected = normalizeImageCategoryName(selectedCategory);
  const categories = [...imageLibraryState.categories];
  if (selected && !categories.some((entry) => entry.toLowerCase() === selected.toLowerCase())) {
    categories.push(selected);
    categories.sort((a, b) => a.localeCompare(b));
  }
  imageDetailsCategory.innerHTML = [
    '<option value="">Uncategorized</option>',
    ...categories.map((category) => `<option value="${escapeHTML(category)}">${escapeHTML(category)}</option>`),
  ].join("");
  imageDetailsCategory.value = selected;
}

function addImageDetailTag(rawValue) {
  const value = normalizeImageTag(rawValue);
  if (!value) return false;
  if (imageDetailsTagsState.some((tag) => tag.toLowerCase() === value.toLowerCase())) return false;
  imageDetailsTagsState.push(value);
  renderImageDetailsTags();
  if (imageDetailsTags) imageDetailsTags.value = "";
  return true;
}

function setSelectedImage(image) {
  uploadedImage = image || null;
  metadataUri.value = "";
  if (!image) {
    imageStatus.textContent = "";
    imagePath.textContent = "";
    setImagePreview("");
    return;
  }
  imageStatus.textContent = "";
  imagePath.textContent = "";
  setImagePreview(image.previewUrl);
}

function renderImageCategoryChips() {
  if (!imageCategoryChips) return;
  imageCategoryChips.innerHTML = imageLibraryState.categories.map((category) => `
    <button type="button" class="image-category-chip${imageLibraryState.category === category ? " active" : ""}" data-image-category="${escapeHTML(category)}">
      ${escapeHTML(category)}
    </button>
  `).join("");
  document.querySelectorAll("[data-image-category]").forEach((button) => {
    button.classList.toggle("active", button.getAttribute("data-image-category") === imageLibraryState.category);
  });
}

function renderImageLibraryGrid() {
  if (!imageLibraryGrid) return;
  const imageTiles = imageLibraryState.images.map((image) => `
    <div class="image-library-item${image.id === imageLibraryState.activeImageId ? " active" : ""}" data-image-id="${escapeHTML(image.id)}" tabindex="0" role="button" aria-label="${escapeHTML(image.name || image.fileName || "image")}">
      <img src="${escapeHTML(image.previewUrl)}" alt="${escapeHTML(image.name || image.fileName || "image")}">
      <button type="button" class="image-library-item-menu-trigger" data-image-menu-id="${escapeHTML(image.id)}">&hellip;</button>
    </div>
  `);
  imageTiles.push(`
    <button type="button" class="image-library-item image-library-upload-tile" data-image-upload-tile>
      <span>+</span>
    </button>
  `);
  imageLibraryGrid.innerHTML = imageTiles.join("");
  const isEmpty = imageLibraryState.images.length === 0;
  imageLibraryGrid.hidden = isEmpty;
  if (imageLibraryEmpty) imageLibraryEmpty.hidden = !isEmpty;
}

async function fetchImageLibrary() {
  const params = new URLSearchParams();
  if (imageLibraryState.search) params.set("search", imageLibraryState.search);
  if (imageLibraryState.category === "favorites") {
    params.set("favoritesOnly", "true");
  } else if (imageLibraryState.category && imageLibraryState.category !== "all") {
    params.set("category", imageLibraryState.category);
  }
  const response = await fetch(`/api/images?${params.toString()}`);
  const payload = await response.json();
  if (!response.ok || !payload.ok) {
    throw new Error(payload.error || "Failed to load images.");
  }
  imageLibraryState.images = Array.isArray(payload.images) ? payload.images : [];
  imageLibraryState.categories = Array.isArray(payload.categories) ? payload.categories : [];
  renderImageCategoryChips();
  renderImageLibraryGrid();
}

function showImageLibraryModal() {
  if (imageLibraryModal) imageLibraryModal.hidden = false;
  imageLibraryState.activeImageId = uploadedImage && uploadedImage.id ? uploadedImage.id : "";
  fetchImageLibrary().catch((error) => {
    imageStatus.textContent = error.message;
  });
}

function hideImageLibraryModal() {
  if (imageLibraryModal) imageLibraryModal.hidden = true;
  hideImageItemMenu();
}

function showImageDetailsModal(image, options = {}) {
  if (!image) return;
  hideImageItemMenu();
  activeImageDetailsId = image.id;
  isEditingNewImageUpload = Boolean(options.isNewUpload);
  setImageDetailsError("");
  if (imageDetailsName) imageDetailsName.value = image.name || "";
  imageDetailsTagsState = Array.isArray(image.tags) ? [...image.tags] : [];
  if (imageDetailsTags) imageDetailsTags.value = "";
  renderImageDetailsTags();
  renderImageDetailsCategoryOptions(image.category || "");
  if (imageDetailsCategoryRow) imageDetailsCategoryRow.hidden = false;
  if (imageDetailsTitle) {
    imageDetailsTitle.textContent = options.isNewUpload ? "Name Image" : "Edit Image Details";
  }
  if (imageDetailsModal) imageDetailsModal.hidden = false;
}

function hideImageDetailsModal() {
  if (imageDetailsModal) imageDetailsModal.hidden = true;
  setImageDetailsError("");
  activeImageDetailsId = "";
  imageDetailsTagsState = [];
  renderImageDetailsTags();
  isEditingNewImageUpload = false;
}

function setImageCategoryError(message = "") {
  if (imageCategoryError) imageCategoryError.textContent = message;
}

function showImageCategoryModal(context = "library") {
  imageCategoryModalContext = context;
  setImageCategoryError("");
  if (imageCategoryName) imageCategoryName.value = "";
  if (imageCategoryModal) imageCategoryModal.hidden = false;
  if (imageCategoryName) imageCategoryName.focus();
}

function hideImageCategoryModal() {
  if (imageCategoryModal) imageCategoryModal.hidden = true;
  if (imageCategoryName) imageCategoryName.value = "";
  setImageCategoryError("");
}

async function createImageCategory(rawName) {
  const name = normalizeImageCategoryName(rawName);
  if (!name) {
    throw new Error("Category name is required.");
  }
  const response = await fetch("/api/images/categories", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name }),
  });
  const payload = await response.json();
  if (!response.ok || !payload.ok) {
    throw new Error(payload.error || "Failed to create category.");
  }
  imageLibraryState.categories = Array.isArray(payload.categories) ? payload.categories : imageLibraryState.categories;
  renderImageCategoryChips();
  renderImageDetailsCategoryOptions(payload.category || name);
  return payload.category || name;
}

function openImageItemMenu(imageId, anchor) {
  const image = imageLibraryState.images.find((entry) => entry.id === imageId);
  if (!image || !anchor || !imageItemMenu) return;
  activeImageMenuId = imageId;
  imageMenuFavorite.textContent = image.isFavorite ? "Remove Favorite" : "Add to Favorites";
  const rect = anchor.getBoundingClientRect();
  imageItemMenu.style.left = `${Math.max(12, rect.right - 180)}px`;
  imageItemMenu.style.top = `${rect.bottom + 6}px`;
  imageItemMenu.hidden = false;
}

function setImagePreview(previewUrl) {
  if (!previewUrl) {
    imagePreview.removeAttribute("src");
    imagePreview.hidden = true;
    imageEmpty.hidden = false;
    return;
  }
  imagePreview.src = previewUrl;
  imagePreview.hidden = false;
  imageEmpty.hidden = true;
}

function selectedWalletKey() {
  return walletSelect.value || "";
}

function shortAddress(value) {
  if (!value) return "-";
  if (value.length <= 10) return value;
  return `${value.slice(0, 4)}...${value.slice(-4)}`;
}

function walletIndexFromEnvKey(envKey) {
  if (!envKey) return "?";
  const suffix = envKey.replace("SOLANA_PRIVATE_KEY", "");
  return suffix ? suffix : "1";
}

function walletLabel(wallet, balanceSol) {
  if (!wallet) return "No wallet";
  const index = walletIndexFromEnvKey(wallet.envKey);
  if (!wallet.publicKey) return `#${index}: invalid`;
  const bal = balanceSol != null ? ` | ${Number(balanceSol).toFixed(4)} SOL` : "";
  return `#${index} - ${wallet.publicKey}${bal}`;
}

function walletDisplayName(wallet) {
  if (!wallet) return "No wallet";
  const index = walletIndexFromEnvKey(wallet.envKey);
  return `Imported SOL Key ${index}`;
}

function walletBalanceSol(wallet) {
  if (!wallet || wallet.balanceSol == null || Number.isNaN(Number(wallet.balanceSol))) return 0;
  return Number(wallet.balanceSol);
}

function formatWalletSol(value) {
  return Number(value || 0).toFixed(2);
}

function formatWalletUsd(value) {
  return Number(value || 0).toFixed(2);
}

function walletUsdValue(wallet) {
  if (!wallet || wallet.usd1Balance == null || Number.isNaN(Number(wallet.usd1Balance))) return 0;
  return Number(wallet.usd1Balance);
}

function renderWalletSummary() {
  if (!walletSummarySol || !walletSummaryUsd) return;
  const wallets = latestWalletStatus && Array.isArray(latestWalletStatus.wallets) ? latestWalletStatus.wallets : [];
  const selectedWallet = wallets.find((wallet) => wallet.envKey === selectedWalletKey()) || null;
  walletSummarySol.textContent = formatWalletSol(walletBalanceSol(selectedWallet));
  walletSummaryUsd.textContent = formatWalletUsd(walletUsdValue(selectedWallet));
}

function renderWalletDropdownList(wallets = [], selectedKey = "") {
  if (!walletDropdownList) return;
  if (!wallets.length) {
    walletDropdownList.innerHTML = `<div class="wallet-empty-state">No wallets found</div>`;
    return;
  }
  walletDropdownList.innerHTML = wallets.map((wallet) => {
    const solValue = walletBalanceSol(wallet);
    const usdValue = walletUsdValue(wallet);
    return `
      <button
        type="button"
        class="wallet-option-button${wallet.envKey === selectedKey ? " is-selected" : ""}"
        data-wallet-key="${escapeHTML(wallet.envKey || "")}"
      >
        <span class="wallet-option-main">
          <span class="wallet-option-name">${escapeHTML(walletDisplayName(wallet))}</span>
          <span class="wallet-option-meta">${escapeHTML(shortenAddress(wallet.publicKey || "Unavailable"))}</span>
        </span>
        <span class="wallet-option-balances">
          <span class="wallet-option-sol">${escapeHTML(formatWalletSol(solValue))} SOL</span>
          <span class="wallet-option-usd">${escapeHTML(formatWalletUsd(usdValue))} USD1</span>
        </span>
      </button>
    `;
  }).join("");
}

function setWalletDropdownOpen(isOpen) {
  if (walletDropdown) walletDropdown.hidden = !isOpen;
  if (walletTriggerButton) walletTriggerButton.setAttribute("aria-expanded", String(isOpen));
}

function toggleWalletDropdown() {
  setWalletDropdownOpen(!walletDropdown || walletDropdown.hidden);
}

function connectedWalletText() {
  return latestWalletStatus && latestWalletStatus.wallet ? latestWalletStatus.wallet : "Connected wallet";
}

function shortenAddress(addr, chars = 6) {
  if (!addr || addr.length <= chars * 2 + 3) return addr;
  return addr.slice(0, chars) + "..." + addr.slice(-chars);
}

function connectedWalletShort() {
  return latestWalletStatus && latestWalletStatus.wallet
    ? shortenAddress(latestWalletStatus.wallet)
    : "Connected wallet";
}

function getFeeSplitRows() {
  return Array.from(feeSplitList.querySelectorAll(".fee-split-row"));
}

function createFeeSplitRow(entry = {}) {
  const row = document.createElement("div");
  row.className = "fee-split-row";
  row.dataset.type = entry.type || "wallet";
  if (entry.defaultReceiver) row.dataset.defaultReceiver = "true";
  row.innerHTML = `
    <div class="fee-split-row-top">
      <div class="recipient-type-tabs">
        <button type="button" class="recipient-type-tab" data-type="wallet">Wallet</button>
        <button type="button" class="recipient-type-tab" data-type="github">GitHub</button>
      </div>
      <button type="button" class="recipient-remove" aria-label="Remove recipient">×</button>
    </div>
    <div class="fee-split-row-main">
      <div class="recipient-target-wrap">
        <input class="recipient-target" type="text" placeholder="Wallet address">
        <button type="button" class="recipient-lock-toggle">Set</button>
      </div>
      <div class="recipient-share-box">
        <input class="recipient-share" type="number" min="0" max="100" step="0.01" placeholder="0">
        <span>%</span>
      </div>
    </div>
    <input class="recipient-slider" type="range" min="0" max="100" step="0.01" value="0">
  `;

  row.querySelector(".recipient-target").value = entry.value || "";
  row.querySelector(".recipient-share").value = entry.sharePercent || "";
  row.querySelector(".recipient-slider").value = entry.sharePercent || "0";
  updateFeeSplitRowType(row, row.dataset.type);
  setRecipientTargetLocked(row, Boolean(entry.targetLocked));
  return row;
}

function updateFeeSplitRowType(row, type) {
  row.dataset.type = type;
  row.querySelectorAll(".recipient-type-tab").forEach((button) => {
    button.classList.toggle("active", button.dataset.type === type);
  });
  const target = row.querySelector(".recipient-target");
  target.placeholder = type === "github" ? "GitHub username" : "Wallet address";
}

function setRecipientTargetLocked(row, locked) {
  if (!row || row.dataset.locked === "true") return;
  const target = row.querySelector(".recipient-target");
  const toggle = row.querySelector(".recipient-lock-toggle");
  if (!target || !toggle) return;

  if (locked) {
    if (!target.value.trim()) {
      target.focus();
      return;
    }
    row.dataset.targetLocked = "true";
    target.readOnly = true;
    target.title = target.value.trim();
    toggle.textContent = "Set";
    toggle.classList.add("is-set");
  } else {
    delete row.dataset.targetLocked;
    target.readOnly = false;
    target.title = "";
    toggle.textContent = "Set";
    toggle.classList.remove("is-set");
  }

  row.querySelectorAll(".recipient-type-tab").forEach((button) => {
    button.disabled = locked;
  });
}

function ensureFeeSplitDefaultRow() {
  if (!feeSplitList) return;
  const deployerAddress = latestWalletStatus && latestWalletStatus.wallet ? latestWalletStatus.wallet : "";
  let defaultRow = getFeeSplitRows().find((row) => row.dataset.defaultReceiver === "true");
  if (!defaultRow) {
    feeSplitList.appendChild(createFeeSplitRow({
      type: "wallet",
      value: deployerAddress,
      sharePercent: "100",
      defaultReceiver: true,
      targetLocked: true,
    }));
    defaultRow = getFeeSplitRows().find((row) => row.dataset.defaultReceiver === "true");
  }
  if (!defaultRow) return;
  const target = defaultRow.querySelector(".recipient-target");
  const share = defaultRow.querySelector(".recipient-share");
  const slider = defaultRow.querySelector(".recipient-slider");
  if (target && deployerAddress && (!target.value.trim() || target.value !== deployerAddress)) {
    target.value = deployerAddress;
  }
  if (share && !share.value.trim()) share.value = "100";
  if (slider && !Number(slider.value || 0)) slider.value = share && share.value.trim() ? share.value.trim() : "100";
  setRecipientTargetLocked(defaultRow, true);
}

function syncFeeSplitTotals() {
  const rows = getFeeSplitRows();
  const total = rows.reduce((sum, row) => {
    const value = Number(row.querySelector(".recipient-share").value || 0);
    return sum + (Number.isFinite(value) ? value : 0);
  }, 0);
  feeSplitTotal.textContent = `${total.toFixed(2).replace(/\.00$/, "")}%`;
  feeSplitTotal.classList.toggle("invalid", Math.abs(total - 100) > 0.001 && total !== 0);
  feeSplitReset.disabled = rows.length === 0;
  feeSplitEven.disabled = rows.length === 0;

  if (rows.length === 0 || total === 0) {
    feeSplitBar.style.background = "#1e2630";
    feeSplitLegendList.innerHTML = "";
    return;
  }

  let running = 0;
  const gradientStops = [];
  const legendItems = [];
  rows.forEach((row, index) => {
    const share = Number(row.querySelector(".recipient-share").value || 0);
    const color = SPLIT_COLORS[index % SPLIT_COLORS.length];
    const targetValue = row.querySelector(".recipient-target").value.trim();
    const label = targetValue
      ? row.dataset.type === "github"
        ? `@${targetValue.replace(/^@+/, "")}`
        : targetValue
      : row.dataset.type === "github"
        ? "@github"
        : "wallet";
    if (share > 0) {
      const start = running;
      running += share;
      gradientStops.push(`${color} ${start}%`, `${color} ${running}%`);
      legendItems.push(
        `<span class="legend-chip"><span class="legend-dot" style="background:${color}"></span>${label} ${share.toFixed(2).replace(/\.00$/, "")}%</span>`
      );
    }
  });

  if (running < 100) {
    gradientStops.push(`#1e2630 ${running}%`, "#1e2630 100%");
  }

  feeSplitBar.style.background = gradientStops.length
    ? `linear-gradient(90deg, ${gradientStops.join(", ")})`
    : "#1e2630";
  feeSplitLegendList.innerHTML = legendItems.join("");
}

function updateFeeSplitVisibility() {
  const active = getMode() === "regular" && feeSplitEnabled.checked;
  feeSplitPill.classList.toggle("active", active);
  feeSplitPill.disabled = getMode() !== "regular";
  if (active) ensureFeeSplitDefaultRow();
  if (!active && feeSplitModal) feeSplitModal.hidden = true;
  syncFeeSplitTotals();
}

function showFeeSplitModal() {
  if (getMode() !== "regular") return;
  feeSplitEnabled.checked = true;
  updateFeeSplitVisibility();
  if (feeSplitModal) feeSplitModal.hidden = false;
}

function hideFeeSplitModal() {
  if (feeSplitModal) feeSplitModal.hidden = true;
}

function createAgentSplitRow(entry = {}) {
  const isAgent = entry.locked === true;
  const row = document.createElement("div");
  row.className = "fee-split-row";
  row.dataset.type = isAgent ? "agent" : (entry.type || "wallet");
  if (isAgent) row.dataset.locked = "true";
  if (entry.defaultReceiver) row.dataset.defaultReceiver = "true";

  if (isAgent) {
    row.innerHTML = `
      <div class="fee-split-row-top">
        <div class="recipient-type-tabs">
          <span class="recipient-type-tab active locked-tab">Agent Buyback</span>
        </div>
      </div>
      <div class="fee-split-row-main">
        <input class="recipient-target" type="text" value="Agent fee split receiver (derived)" disabled>
        <div class="recipient-share-box">
          <input class="recipient-share" type="number" min="0" max="100" step="0.01" placeholder="0">
          <span>%</span>
        </div>
      </div>
      <input class="recipient-slider" type="range" min="0" max="100" step="0.01" value="0">
    `;
  } else {
    row.innerHTML = `
      <div class="fee-split-row-top">
        <div class="recipient-type-tabs">
          <button type="button" class="recipient-type-tab" data-type="wallet">Wallet</button>
          <button type="button" class="recipient-type-tab" data-type="github">GitHub</button>
        </div>
        <button type="button" class="recipient-remove" aria-label="Remove recipient">×</button>
      </div>
      <div class="fee-split-row-main">
        <div class="recipient-target-wrap">
          <input class="recipient-target" type="text" placeholder="Wallet address">
          <button type="button" class="recipient-lock-toggle">Set</button>
        </div>
        <div class="recipient-share-box">
          <input class="recipient-share" type="number" min="0" max="100" step="0.01" placeholder="0">
          <span>%</span>
        </div>
      </div>
      <input class="recipient-slider" type="range" min="0" max="100" step="0.01" value="0">
    `;
    row.querySelector(".recipient-target").value = entry.value || "";
    updateFeeSplitRowType(row, row.dataset.type);
    setRecipientTargetLocked(row, Boolean(entry.targetLocked));
  }
  row.querySelector(".recipient-share").value = entry.sharePercent || "";
  row.querySelector(".recipient-slider").value = entry.sharePercent || "0";
  return row;
}

function getAgentSplitRows() {
  return Array.from(agentSplitList.querySelectorAll(".fee-split-row"));
}

function syncAgentSplitTotals() {
  const rows = getAgentSplitRows();
  const total = rows.reduce((sum, row) => {
    const value = Number(row.querySelector(".recipient-share").value || 0);
    return sum + (Number.isFinite(value) ? value : 0);
  }, 0);
  agentSplitTotal.textContent = `${total.toFixed(2).replace(/\.00$/, "")}%`;
  agentSplitTotal.classList.toggle("invalid", Math.abs(total - 100) > 0.001 && total !== 0);
  agentSplitReset.disabled = rows.length === 0;
  agentSplitEven.disabled = rows.length === 0;

  if (rows.length === 0 || total === 0) {
    agentSplitBar.style.background = "#1e2630";
    agentSplitLegendList.innerHTML = "";
    return;
  }

  let running = 0;
  const gradientStops = [];
  const legendItems = [];
  rows.forEach((row, index) => {
    const share = Number(row.querySelector(".recipient-share").value || 0);
    const color = SPLIT_COLORS[index % SPLIT_COLORS.length];
    const targetValue = row.querySelector(".recipient-target").value.trim();
    const label = row.dataset.locked
      ? "Agent Buyback"
      : targetValue
        ? row.dataset.type === "github"
          ? `@${targetValue.replace(/^@+/, "")}`
          : shortenAddress(targetValue, 4) || "wallet"
        : "wallet";
    if (share > 0) {
      const start = running;
      running += share;
      gradientStops.push(`${color} ${start}%`, `${color} ${running}%`);
      legendItems.push(
        `<span class="legend-chip"><span class="legend-dot" style="background:${color}"></span>${label} ${share.toFixed(2).replace(/\.00$/, "")}%</span>`
      );
    }
  });

  if (running < 100) {
    gradientStops.push(`#1e2630 ${running}%`, "#1e2630 100%");
  }

  agentSplitBar.style.background = gradientStops.length
    ? `linear-gradient(90deg, ${gradientStops.join(", ")})`
    : "#1e2630";
  agentSplitLegendList.innerHTML = legendItems.join("");
}

function initAgentSplitIfEmpty() {
  if (agentSplitList.children.length === 0) {
    resetAgentSplitToDefault();
  }
}

function showAgentSplitModal() {
  if (getMode() !== "agent-custom") return;
  initAgentSplitIfEmpty();
  syncAgentSplitTotals();
  if (agentSplitModalError) agentSplitModalError.textContent = "";
  if (agentSplitModal) agentSplitModal.hidden = false;
}

function hideAgentSplitModal() {
  if (agentSplitModal) agentSplitModal.hidden = true;
}

function setAgentSplitModalError(message = "") {
  if (agentSplitModalError) agentSplitModalError.textContent = message;
}

function resetAgentSplitToDefault() {
  if (!agentSplitList) return;
  agentSplitList.innerHTML = "";
  agentSplitList.appendChild(createAgentSplitRow({ locked: true, sharePercent: "50" }));
  agentSplitList.appendChild(
    createAgentSplitRow({
      type: "wallet",
      value: latestWalletStatus && latestWalletStatus.wallet ? latestWalletStatus.wallet : "",
      sharePercent: "50",
      defaultReceiver: true,
      targetLocked: true,
    })
  );
  syncAgentSplitTotals();
  setAgentSplitModalError("");
}

function attemptCloseAgentSplitModal() {
  const errors = validateAgentSplit();
  if (errors.length > 0) {
    setAgentSplitModalError(errors[0]);
    return false;
  }
  setAgentSplitModalError("");
  hideAgentSplitModal();
  return true;
}

function normalizeAgentSplitStructure({ afterAdd = false } = {}) {
  const rows = getAgentSplitRows();
  const agentRow = rows.find((row) => row.dataset.locked === "true");
  const otherRows = rows.filter((row) => row.dataset.locked !== "true");
  if (!agentRow) return;

  const agentShareInput = agentRow.querySelector(".recipient-share");
  const agentSliderInput = agentRow.querySelector(".recipient-slider");

  if (otherRows.length === 0) {
    agentShareInput.value = "100";
    agentSliderInput.value = "100";
    return;
  }

  if (afterAdd && otherRows.length === 1) {
    const currentAgentShare = Number(agentShareInput.value || 0);
    const currentOtherShare = Number(otherRows[0].querySelector(".recipient-share").value || 0);
    if (Math.abs(currentAgentShare - 100) < 0.001 && Math.abs(currentOtherShare) < 0.001) {
      agentShareInput.value = "50";
      agentSliderInput.value = "50";
      otherRows[0].querySelector(".recipient-share").value = "50";
      otherRows[0].querySelector(".recipient-slider").value = "50";
    }
  }
}

function collectAgentSplitRecipients() {
  return getAgentSplitRows().map((row) => {
    if (row.dataset.locked) {
      const sharePercent = row.querySelector(".recipient-share").value.trim();
      const numericShare = Number(sharePercent);
      return {
        type: "agent",
        shareBps: Number.isFinite(numericShare) ? Math.round(numericShare * 100) : NaN,
      };
    }
    const type = row.dataset.type || "wallet";
    const value = row.querySelector(".recipient-target").value.trim();
    const sharePercent = row.querySelector(".recipient-share").value.trim();
    if (!value && !sharePercent) return null;
    const numericShare = Number(sharePercent);
    return {
      type,
      address: type === "wallet" ? value : "",
      githubUsername: type === "github" ? value.replace(/^@+/, "") : "",
      shareBps: Number.isFinite(numericShare) ? Math.round(numericShare * 100) : NaN,
    };
  }).filter(Boolean);
}

function updateLockedModeFields() {
  const full = connectedWalletText();
  const short = connectedWalletShort();
  if (agentUnlockedAuthority) {
    agentUnlockedAuthority.value = short;
    agentUnlockedAuthority.title = full;
  }

  const defaultReceiverRow = getAgentSplitRows().find((row) => row.dataset.defaultReceiver === "true");
  if (defaultReceiverRow) {
    const target = defaultReceiverRow.querySelector(".recipient-target");
    if (target && !target.value.trim() && latestWalletStatus && latestWalletStatus.wallet) {
      target.value = latestWalletStatus.wallet;
      setRecipientTargetLocked(defaultReceiverRow, true);
      syncAgentSplitTotals();
    }
  }

  ensureFeeSplitDefaultRow();
}

function updateModeVisibility() {
  const mode = getMode();
  document.querySelectorAll("[data-mode-panel]").forEach((node) => {
    node.hidden = node.getAttribute("data-mode-panel") !== mode;
  });
  updateFeeSplitVisibility();
  updateLockedModeFields();
  if (mode === "agent-custom") initAgentSplitIfEmpty();
  if (mode !== "agent-custom") hideAgentSplitModal();
  updateJitoVisibility();
}

function usesBundledJito() {
  return getProvider() === "jito" && Number(creationTipInput ? creationTipInput.value || 0 : 0) > 0;
}

function updateJitoVisibility() {
  syncSettingsCapabilities();
}

function applyProviderAvailability(providers = {}) {
  [providerSelect, buyProviderSelect, sellProviderSelect].forEach((select) => {
    if (!select) return;
    Array.from(select.options).forEach((option) => {
      const entry = providers[option.value];
      option.disabled = Boolean(entry && !entry.available);
      option.textContent = entry && entry.supportState === "unverified"
        ? `${PROVIDER_LABELS[option.value] || option.textContent.replace(/ \(unverified\)$/, "")} (unverified)`
        : (PROVIDER_LABELS[option.value] || option.textContent.replace(/ \(unverified\)$/, ""));
      if (entry && entry.reason) {
        option.title = entry.reason;
      }
    });
    if (select.selectedOptions[0] && select.selectedOptions[0].disabled) {
      const fallback = Array.from(select.options).find((option) => !option.disabled);
      if (fallback) select.value = fallback.value;
    }
  });
  syncSettingsCapabilities();
}

function applyLaunchpadAvailability(launchpads = {}) {
  if (!launchpadInputs.length) return;
  latestLaunchpadRegistry = launchpads && typeof launchpads === "object" ? launchpads : {};
  launchpadInputs.forEach((input) => {
    const label = input.closest(".launchpad-option");
    const titleNode = label ? label.querySelector(".launchpad-title") : null;
    const entry = launchpads[input.value];
    const unavailable = Boolean(entry && !entry.available);
    input.disabled = unavailable;
    if (label) {
      label.classList.toggle("is-disabled", unavailable);
      if (entry && entry.reason) {
        label.title = entry.reason;
      } else {
        label.removeAttribute("title");
      }
    }
    if (titleNode) {
      const baseLabel = input.value === "bagsapp"
        ? "Bagsapp"
        : input.value.charAt(0).toUpperCase() + input.value.slice(1);
      titleNode.textContent = entry && entry.supportState === "unverified"
        ? `${baseLabel} (unverified)`
        : baseLabel;
    }
  });

  const checked = document.querySelector('input[name="launchpad"]:checked');
  if (!checked || checked.disabled) {
    const fallback = launchpadInputs.find((input) => !input.disabled);
    if (fallback) fallback.checked = true;
  }

  applyLaunchpadTokenMetadata();
}

function applyPersistentDefaults(config) {
  if (!config || defaultsApplied) return;
  const defaults = config.defaults || {};
  if (defaults.launchpad) {
    const launchpadInput = document.querySelector(`input[name="launchpad"][value="${defaults.launchpad}"]`);
    if (launchpadInput) launchpadInput.checked = true;
  }
  setConfig(config);
  applyPresetToSettingsInputs(getActivePreset(config));
  if (defaults.automaticDevSell) {
    if (autoSellEnabledInput) autoSellEnabledInput.checked = Boolean(defaults.automaticDevSell.enabled);
    setNamedValue("automaticDevSellPercent", String(defaults.automaticDevSell.percent || 0));
    setNamedValue("automaticDevSellDelaySeconds", String(defaults.automaticDevSell.delaySeconds || 0));
  }
  setPresetEditing(Boolean(defaults.presetEditing));
  renderQuickDevBuyButtons(config);
  populateDevBuyPresetEditor(config);
  defaultsApplied = true;
}

function collectFeeSplitRecipients() {
  return Array.from(feeSplitList.querySelectorAll(".fee-split-row"))
    .map((row) => {
      const type = row.dataset.type || "wallet";
      const value = row.querySelector(".recipient-target").value.trim();
      const sharePercent = row.querySelector(".recipient-share").value.trim();
      if (!value && !sharePercent) return null;
      const numericShare = Number(sharePercent);
      return {
        type,
        address: type === "wallet" ? value : "",
        githubUsername: type === "github" ? value.replace(/^@+/, "") : "",
        shareBps: Number.isFinite(numericShare) ? Math.round(numericShare * 100) : NaN,
      };
    })
    .filter(Boolean);
}

function readForm() {
  const data = new FormData(form);
  const values = Object.fromEntries(data.entries());
  const mode = values.mode || "regular";
  const devBuyAmount = String(values.devBuyAmount || "").trim();
  const agentSplitRecipients = mode === "agent-custom" ? collectAgentSplitRecipients() : [];
  const agentBuyback = agentSplitRecipients.find((entry) => entry.type === "agent");
  let sniperWallets = [];
  try {
    const parsed = JSON.parse(getNamedValue("sniperConfigJson") || "[]");
    if (Array.isArray(parsed)) {
      sniperWallets = parsed.filter((entry) => entry && entry.envKey);
    }
  } catch (_error) {
    sniperWallets = [];
  }

  return {
    selectedWalletKey: selectedWalletKey(),
    launchpad: getLaunchpad(),
    provider: getProvider(),
    policy: getPolicy(),
    buyProvider: getBuyProvider(),
    buyPolicy: getBuyPolicy(),
    sellProvider: getSellProvider(),
    sellPolicy: getSellPolicy(),
    activePresetId: getActivePresetId(),
    mode,
    name: values.name || "",
    symbol: values.symbol || "",
    description: values.description || "",
    website: values.website || "",
    twitter: values.twitter || "",
    telegram: values.telegram || "",
    mayhemMode: data.get("mayhemMode") === "on",
    agentAuthority: values.agentAuthority || "",
    buybackPercent:
      mode === "agent-custom"
        ? agentBuyback ? String(agentBuyback.shareBps / 100) : ""
        : values.agentUnlockedBuybackPercent || "",
    agentSplitRecipients,
    devBuyMode: devBuyAmount ? getDevBuyMode() : "",
    devBuyAmount,
    autoGas: true,
    buyAutoGas: true,
    priorityFeeSol: getNamedValue("creationPriorityFeeSol") || "",
    creationTipSol: getNamedValue("creationTipSol") || "",
    maxPriorityFeeSol: getNamedValue("creationPriorityFeeSol") || "",
    maxTipSol: getNamedValue("creationTipSol") || "",
    buyPriorityFeeSol: getNamedValue("buyPriorityFeeSol") || "",
    buyTipSol: getNamedValue("buyTipSol") || "",
    buySlippagePercent: getNamedValue("buySlippagePercent") || "",
    buyMaxPriorityFeeSol: getNamedValue("buyPriorityFeeSol") || "",
    buyMaxTipSol: getNamedValue("buyTipSol") || "",
    sellPriorityFeeSol: getNamedValue("sellPriorityFeeSol") || "",
    sellTipSol: getNamedValue("sellTipSol") || "",
    sellSlippagePercent: getNamedValue("sellSlippagePercent") || "",
    enableJito: getProvider() === "jito" || Number(getNamedValue("creationTipSol") || 0) > 0,
    jitoTipSol: getNamedValue("creationTipSol") || "",
    skipPreflight: getNamedValue("skipPreflight") === "true",
    feeSplitEnabled: mode === "regular" && feeSplitEnabled.checked,
    feeSplitRecipients: mode === "regular" && feeSplitEnabled.checked ? collectFeeSplitRecipients() : [],
    postLaunchStrategy: getNamedValue("postLaunchStrategy") || "none",
    snipeBuyAmountSol: getNamedValue("snipeBuyAmountSol") || "",
    sniperEnabled: getNamedValue("sniperEnabled") === "true",
    sniperWallets,
    sniperConfigJson: getNamedValue("sniperConfigJson") || "[]",
    automaticDevSellEnabled: isNamedChecked("automaticDevSellEnabled"),
    automaticDevSellPercent: getNamedValue("automaticDevSellPercent") || "0",
    automaticDevSellDelaySeconds: getNamedValue("automaticDevSellDelaySeconds") || "0",
    vanityPrivateKey: getNamedValue("vanityPrivateKey") || "",
    imageFileName: uploadedImage ? uploadedImage.fileName : "",
    metadataUri: metadataUri.value || "",
  };
}

function renderWalletOptions(wallets, selectedKey, balanceSol) {
  walletSelect.innerHTML = "";
  if (!wallets || wallets.length === 0) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "No wallets found";
    walletSelect.appendChild(option);
    walletSelect.disabled = true;
    renderWalletDropdownList([], "");
    renderWalletSummary();
    return;
  }

  walletSelect.disabled = false;
  wallets.forEach((wallet) => {
    const option = document.createElement("option");
    option.value = wallet.envKey;
    const bal = wallet.envKey === selectedKey ? balanceSol : null;
    option.textContent = walletLabel(wallet, bal);
    if (wallet.envKey === selectedKey) {
      option.selected = true;
    }
    walletSelect.appendChild(option);
  });
  renderWalletDropdownList(wallets, selectedKey);
  renderWalletSummary();
}

async function refreshWalletStatus(preserveSelection = true) {
  try {
    const wallet = preserveSelection ? selectedWalletKey() : "";
    const url = wallet ? `/api/status?wallet=${encodeURIComponent(wallet)}` : "/api/status";
    const response = await fetch(url);
    const payload = await response.json();
    if (!response.ok || !payload.ok) {
      throw new Error(payload.error || "Failed to load wallet status.");
    }

    latestWalletStatus = payload;
    renderWalletOptions(payload.wallets || [], payload.selectedWalletKey || "", payload.balanceSol);
    applyPersistentDefaults(payload.config);
    applyProviderAvailability(payload.providers || {});
    applyLaunchpadAvailability(payload.launchpads || {});
    renderQuickDevBuyButtons(payload.config);
    populateDevBuyPresetEditor(payload.config);
    renderSniperUI();

    if (!payload.connected) {
      if (walletBalance) walletBalance.textContent = "-";
      metaNode.textContent = "No wallet configured. Add SOLANA_PRIVATE_KEY to .env.";
      updateLockedModeFields();
      return;
    }

    if (walletBalance) walletBalance.textContent = `${Number(payload.balanceSol).toFixed(4)} SOL`;
    const selectedWallet = (payload.wallets || []).find((walletEntry) => walletEntry.envKey === payload.selectedWalletKey);
    metaNode.textContent = selectedWallet ? `Using ${walletLabel(selectedWallet)}` : "Wallet ready";
    updateLockedModeFields();
  } catch (error) {
    if (walletBalance) walletBalance.textContent = "-";
    metaNode.textContent = error.message;
  }
}

async function updateQuote() {
  const buyAmount = getNamedValue("devBuyAmount").trim();
  if (!buyAmount) {
    quoteOutput.textContent = "No dev buy selected.";
    if (!syncingDevBuyInputs) {
      syncingDevBuyInputs = true;
      if (devBuyPercentInput) devBuyPercentInput.value = "";
      if (lastDevBuyEditSource !== "percent" && devBuySolInput) devBuySolInput.value = "";
      syncingDevBuyInputs = false;
    }
    return;
  }

  try {
    const mode = getDevBuyMode();
    const response = await fetch(`/api/quote?mode=${encodeURIComponent(mode)}&amount=${encodeURIComponent(buyAmount)}`);
    const payload = await response.json();
    if (!response.ok || !payload.ok) {
      throw new Error(payload.error || "Quote failed.");
    }
    if (!payload.quote) {
      quoteOutput.textContent = "Enter a valid dev buy amount.";
      return;
    }
    syncingDevBuyInputs = true;
    if (mode === "sol") {
      if (devBuyPercentInput) devBuyPercentInput.value = payload.quote.estimatedSupplyPercent;
    } else {
      if (devBuySolInput) devBuySolInput.value = payload.quote.estimatedSol;
      if (devBuyPercentInput) devBuyPercentInput.value = payload.quote.estimatedSupplyPercent;
    }
    syncingDevBuyInputs = false;
    quoteOutput.textContent =
      mode === "sol"
        ? `Estimated tokens out: ${payload.quote.estimatedTokens} (${payload.quote.estimatedSupplyPercent}% supply)`
        : `Estimated SOL required: ${payload.quote.estimatedSol} for ${payload.quote.estimatedSupplyPercent}% supply`;
  } catch (error) {
    quoteOutput.textContent = error.message;
  }
}

function queueQuoteUpdate() {
  clearTimeout(quoteTimer);
  quoteTimer = setTimeout(updateQuote, 250);
}

async function uploadSelectedImage(file) {
  const dataUrl = await new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = () => reject(new Error("Failed to read image."));
    reader.readAsDataURL(file);
  });

  const response = await fetch("/api/upload-image", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      filename: file.name,
      dataUrl,
    }),
  });

  const payload = await response.json();
  if (!response.ok || !payload.ok) {
    throw new Error(payload.error || "Image upload failed.");
  }

  imageStatus.textContent = "Image uploaded to library.";
  imagePath.textContent = "";
  imageLibraryState.activeImageId = payload.id || "";
  try {
    await fetchImageLibrary();
  } catch (error) {
    imageStatus.textContent = error.message;
  }
  showImageDetailsModal(payload, { isNewUpload: true });
}

async function applyTestPreset() {
  form.querySelector('[name="name"]').value = TEST_PRESET.name;
  tickerManuallyEdited = false;
  syncTickerFromName();
  form.querySelector('[name="description"]').value = TEST_PRESET.description;
  form.querySelector('[name="website"]').value = TEST_PRESET.website;
  form.querySelector('[name="twitter"]').value = TEST_PRESET.twitter;
  form.querySelector('[name="telegram"]').value = TEST_PRESET.telegram;
  setDevBuyHiddenState(TEST_PRESET.devBuyMode, TEST_PRESET.devBuyAmount);
  syncingDevBuyInputs = true;
  if (devBuySolInput) devBuySolInput.value = TEST_PRESET.devBuyMode === "sol" ? TEST_PRESET.devBuyAmount : "";
  if (devBuyPercentInput) devBuyPercentInput.value = "";
  syncingDevBuyInputs = false;
  setNamedValue("creationTipSol", TEST_PRESET.creationTipSol);
  setNamedValue("creationPriorityFeeSol", TEST_PRESET.creationPriorityFeeSol);
  setNamedValue("buyPriorityFeeSol", TEST_PRESET.buyPriorityFeeSol);
  setNamedValue("buyTipSol", TEST_PRESET.buyTipSol);
  setNamedValue("buySlippagePercent", TEST_PRESET.buySlippagePercent);
  setNamedValue("sellPriorityFeeSol", TEST_PRESET.sellPriorityFeeSol);
  setNamedValue("sellTipSol", TEST_PRESET.sellTipSol);
  setNamedValue("sellSlippagePercent", TEST_PRESET.sellSlippagePercent);
  setNamedValue("skipPreflight", TEST_PRESET.skipPreflight);

  clearValidationErrors();
  Object.keys(fieldValidators).forEach((name) => setFieldError(name, ""));
  updateTokenFieldCounts();
  updateJitoVisibility();
  queueQuoteUpdate();

  try {
    imageStatus.textContent = "Uploading image...";
    imagePath.textContent = "";
    metadataUri.value = "";
    const response = await fetch("/solana-logo.png");
    if (!response.ok) {
      throw new Error("Failed to load test image.");
    }
    const blob = await response.blob();
    const file = new File([blob], "image (17).png", { type: blob.type || "image/png" });
    try {
      const dataTransfer = new DataTransfer();
      dataTransfer.items.add(file);
      imageInput.files = dataTransfer.files;
    } catch (_error) {
      // Some browsers restrict programmatic file input assignment.
    }
    setImagePreview(URL.createObjectURL(file));
    await uploadSelectedImage(file);
  } catch (error) {
    uploadedImage = null;
    imageStatus.textContent = error.message;
    imagePath.textContent = "";
  }
}

const fieldValidators = {
  website(v) {
    if (!v) return "";
    if (!/^https?:\/\/.+/i.test(v)) return "Must start with https://";
    return "";
  },
  twitter(v) {
    if (!v) return "";
    if (/^https?:\/\/(x\.com|twitter\.com)\/.+/i.test(v)) return "";
    if (/^@?[\w]{1,15}$/.test(v)) return "";
    return "Enter a valid URL (https://x.com/...) or @username";
  },
  telegram(v) {
    if (!v) return "";
    if (/^https?:\/\/t\.me\/.+/i.test(v)) return "";
    return "Enter a valid Telegram link (https://t.me/username)";
  },
  devBuyAmount(v) {
    if (!v) return "";
    const n = Number(v);
    if (isNaN(n) || n <= 0) return "Must be a positive number";
    return "";
  },
  creationTipSol(v) {
    if (!v) return "";
    const n = Number(v);
    if (isNaN(n) || n < 0) return "Must be a valid number";
    return "";
  },
  creationPriorityFeeSol(v) {
    if (!v) return "";
    const n = Number(v);
    if (isNaN(n) || n < 0) return "Must be a valid number";
    return "";
  },
  buyPriorityFeeSol(v) {
    if (!v) return "";
    const n = Number(v);
    if (isNaN(n) || n < 0) return "Must be a valid number";
    return "";
  },
  buyTipSol(v) {
    if (!v) return "";
    const n = Number(v);
    if (isNaN(n) || n < 0) return "Must be a valid number";
    return "";
  },
  buySlippagePercent(v) {
    if (!v) return "";
    const n = Number(v);
    if (isNaN(n) || n < 0) return "Must be a valid number";
    return "";
  },
  sellPriorityFeeSol(v) {
    if (!v) return "";
    const n = Number(v);
    if (isNaN(n) || n < 0) return "Must be a valid number";
    return "";
  },
  sellTipSol(v) {
    if (!v) return "";
    const n = Number(v);
    if (isNaN(n) || n < 0) return "Must be a valid number";
    return "";
  },
  sellSlippagePercent(v) {
    if (!v) return "";
    const n = Number(v);
    if (isNaN(n) || n < 0) return "Must be a valid number";
    return "";
  },
  automaticDevSellPercent(v) {
    if (!isNamedChecked("automaticDevSellEnabled")) return "";
    const n = Number(v);
    if (isNaN(n) || n < 0 || n > 100) return "Must be between 0 and 100";
    return "";
  },
  automaticDevSellDelaySeconds(v) {
    if (!isNamedChecked("automaticDevSellEnabled")) return "";
    const n = Number(v);
    if (isNaN(n) || n < 0 || n > 10) return "Must be between 0 and 10";
    return "";
  },
  agentUnlockedBuybackPercent(v) {
    if (getMode() !== "agent-unlocked") return "";
    if (!v) return "Buyback % is required";
    const n = Number(v);
    if (isNaN(n) || n < 0 || n > 100) return "Must be between 0 and 100";
    return "";
  },
};

function setFieldError(name, msg) {
  const errorEl = document.querySelector(`.field-error[data-error-for="${name}"]`);
  const input = getNamedInput(name);
  if (errorEl) errorEl.textContent = msg || "";
  if (input) input.classList.toggle("input-error", !!msg);
}

function validateFieldByName(name) {
  const input = getNamedInput(name);
  if (!input || !fieldValidators[name]) return "";
  const msg = fieldValidators[name](input.value.trim());
  setFieldError(name, msg);
  return msg;
}

function validateAllInlineFields() {
  const errors = [];
  for (const name of Object.keys(fieldValidators)) {
    const msg = validateFieldByName(name);
    if (msg) errors.push(msg);
  }
  return errors;
}

function validateAgentSplit() {
  const errors = [];
  const recipients = collectAgentSplitRecipients();
  if (getMode() !== "agent-custom") return errors;

  if (recipients.length === 0) {
    errors.push("Agent fee split is required.");
    return errors;
  }

  const total = recipients.reduce((sum, entry) => sum + (Number(entry.shareBps) || 0), 0);
  const agentRows = recipients.filter((entry) => entry.type === "agent");
  if (agentRows.length !== 1) {
    errors.push("Agent custom mode requires exactly one agent buyback row.");
  }
  if (total !== 10_000) {
    errors.push("Agent custom fee split must total 100%.");
  }

  recipients.forEach((entry, index) => {
    if (!Number.isFinite(entry.shareBps) || entry.shareBps < 0) {
      errors.push(`Agent split recipient ${index + 1} has an invalid %.`);
      return;
    }
    if (entry.type === "agent") return;
    if (entry.type === "wallet" && !entry.address) {
      errors.push(`Agent split recipient ${index + 1} is missing a wallet address.`);
    }
    if (entry.type === "github" && !entry.githubUsername) {
      errors.push(`Agent split recipient ${index + 1} is missing a GitHub username.`);
    }
  });

  return errors;
}

function validateForm() {
  const errors = [];
  const f = readForm();
  if (!f.name.trim()) errors.push("Token name is required.");
  if (!f.symbol.trim()) errors.push("Ticker is required.");
  if (!uploadedImage) errors.push("Token image is required.");
  if (!latestWalletStatus || !latestWalletStatus.connected) errors.push("No wallet connected.");
  if (f.automaticDevSellEnabled && !f.devBuyAmount) errors.push("Dev auto-sell requires a dev buy amount.");
  validateSniperState().forEach((msg) => errors.push(msg));
  const inlineErrors = validateAllInlineFields();
  inlineErrors.forEach((msg) => errors.push(msg));
  validateAgentSplit().forEach((msg) => errors.push(msg));
  return errors;
}

function showValidationErrors(errors) {
  let container = document.getElementById("validation-errors");
  if (errors.length === 0) {
    if (container) container.remove();
    return false;
  }
  if (!container) {
    container = document.createElement("div");
    container.id = "validation-errors";
    container.className = "validation-errors";
    const outputAnchor = outputSection || document.querySelector(".output");
    outputAnchor.parentNode.insertBefore(container, outputAnchor);
  }
  container.innerHTML = errors.map((e) => `<span>${escapeHTML(e)}</span>`).join("");
  container.scrollIntoView({ behavior: "smooth", block: "nearest" });
  return true;
}

function clearValidationErrors() {
  const container = document.getElementById("validation-errors");
  if (container) container.remove();
}

function buildDeployPreviewHTML() {
  const f = readForm();
  const walletAddr = latestWalletStatus && latestWalletStatus.wallet ? latestWalletStatus.wallet : "Unknown";
  const bal = latestWalletStatus && latestWalletStatus.balanceSol ? `${Number(latestWalletStatus.balanceSol).toFixed(4)} SOL` : "-";

  const imgSrc = imagePreview.src && !imagePreview.hidden ? imagePreview.src : "";
  const tokenImgHTML = imgSrc
    ? `<img class="modal-token-img" src="${imgSrc}" alt="">`
    : `<div class="modal-token-img-empty">No img</div>`;

  const devBuyText = f.devBuyAmount
    ? `${f.devBuyAmount} ${f.devBuyMode === "tokens" ? "tokens" : "SOL"}`
    : "None";

  const quoteText = quoteOutput.textContent || "";

  const modeLabels = {
    regular: "Regular",
    cashback: "Cashback",
    "agent-custom": "Agent Custom",
    "agent-unlocked": "Agent Unlocked",
    "agent-locked": "Agent Locked",
  };

  let feesText = "Default (deployer)";
  if (f.mode === "cashback") feesText = "Cashback to traders";
  else if (f.mode === "agent-locked") feesText = "Agent escrow (locked)";
  else if (f.mode === "agent-custom") {
    const parts = (f.agentSplitRecipients || []).map((entry) => {
      const share = (Number(entry.shareBps || 0) / 100).toFixed(2).replace(/\.00$/, "");
      if (entry.type === "agent") return `Agent ${share}%`;
      if (entry.type === "github") return `@${entry.githubUsername} ${share}%`;
      return `${shortenAddress(entry.address || "wallet", 4)} ${share}%`;
    });
    feesText = parts.length > 0 ? parts.join(" | ") : "Agent split";
  } else if (f.mode === "agent-unlocked") {
    feesText = "Untouched on launch; configure once later";
  } else if (f.feeSplitEnabled) {
    feesText = `Fee split (${f.feeSplitRecipients.length} recipients)`;
  }

  let txSettingsText = usesBundledJito()
    ? "Priority: custom"
    : `Priority: ${f.priorityFeeSol || "off"}`;
  if (f.jitoTipSol) txSettingsText += ` | Tip: ${f.jitoTipSol || "default"} SOL`;
  if (f.skipPreflight) txSettingsText += " | Preflight off";

  const buybackText = f.mode === "agent-locked" ? "100%"
    : (f.mode === "agent-custom" || f.mode === "agent-unlocked") ? `${f.buybackPercent || "50"}%`
    : "-";
  const sniperText = f.sniperEnabled
    ? (f.sniperWallets.length
      ? f.sniperWallets.map((entry) => `#${walletIndexFromEnvKey(entry.envKey)} ${entry.amountSol} SOL`).join(" | ")
      : "Enabled")
    : "Off";
  const vanityText = f.vanityPrivateKey ? "Custom vanity key attached" : "Off";

  const rows = [
    { label: "Wallet", value: walletAddr, cls: "" },
    { label: "Balance", value: bal, cls: "green" },
    { label: "Preset", value: f.activePresetId || DEFAULT_PRESET_ID, cls: "secondary" },
    { label: "Platform", value: f.launchpad || "pump", cls: "" },
    { label: "Creation", value: `${f.provider || "helius"} / ${f.policy || "safe"}`, cls: "" },
    { label: "Buy Route", value: `${f.buyProvider || "helius"} / ${f.buyPolicy || "safe"} | slip ${f.buySlippagePercent || "90"}%`, cls: "secondary" },
    { label: "Sell Route", value: `${f.sellProvider || "helius"} / ${f.sellPolicy || "safe"} | slip ${f.sellSlippagePercent || "90"}%`, cls: "secondary" },
    { label: "Mode", value: `${modeLabels[f.mode] || f.mode}${f.mayhemMode ? " + Mayhem" : ""}`, cls: "" },
    ...(f.mode.startsWith("agent") ? [{ label: "Buyback", value: buybackText, cls: "" }] : []),
    { label: "Fees", value: feesText, cls: "secondary" },
    { label: "Dev Buy", value: devBuyText, cls: "" },
    ...(f.automaticDevSellEnabled ? [{
      label: "Dev Auto Sell",
      value: `${f.automaticDevSellPercent || "0"}% after ${Number(f.automaticDevSellDelaySeconds || 0).toFixed(1)}s`,
      cls: "secondary",
    }] : []),
    { label: "Sniper", value: sniperText, cls: "secondary" },
    { label: "Vanity", value: vanityText, cls: "secondary" },
    ...(quoteText && f.devBuyAmount ? [{ label: "Estimate", value: quoteText, cls: "secondary" }] : []),
    { label: "Tx Settings", value: txSettingsText, cls: "secondary" },
  ];

  return `
    <div class="modal-token-header">
      ${tokenImgHTML}
      <div class="modal-token-info">
        <div class="modal-token-name">${escapeHTML(f.name || "Unnamed")}</div>
        <div class="modal-token-ticker">$${escapeHTML(f.symbol || "???")}</div>
      </div>
    </div>
    ${rows.map((r) => `
      <div class="modal-preview-row">
        <div class="modal-preview-label">${r.label}</div>
        <div class="modal-preview-value ${r.cls}">${escapeHTML(r.value)}</div>
      </div>
    `).join("")}
    <div class="modal-warning">This will broadcast a real transaction on Solana mainnet.</div>
  `;
}

function escapeHTML(str) {
  const div = document.createElement("div");
  div.textContent = str;
  return div.innerHTML;
}

function showDeployModal() {
  modalBody.innerHTML = buildDeployPreviewHTML();
  deployModal.hidden = false;
}

function hideDeployModal() {
  deployModal.hidden = true;
}

async function run(action) {
  const actualAction = action === "deploy" ? "send" : action;
  const label = action === "deploy" ? "Deploying..." : action === "simulate" ? "Simulating..." : "Building...";
  setBusy(true, label);
  output.textContent = "Working...";

  try {
    const response = await fetch("/api/run", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        action: actualAction,
        form: readForm(),
      }),
    });
    const payload = await response.json();
    if (!response.ok || !payload.ok) {
      throw new Error(payload.error || "Request failed.");
    }

    statusNode.textContent = action === "deploy" ? "Deployed" : action === "simulate" ? "Simulated" : "Built";
    const wallet = latestWalletStatus && latestWalletStatus.selectedWalletKey
      ? `Using #${walletIndexFromEnvKey(latestWalletStatus.selectedWalletKey)}`
      : "Wallet ready";
    metaNode.textContent = `${wallet} | ${payload.report.launchpad || "pump"} | ${payload.report.execution.resolvedProvider || payload.report.execution.provider || "auto"} | Mint: ${shortAddress(payload.report.mint)}`;
    metadataUri.value = payload.metadataUri || "";
    output.textContent = payload.text;
    await refreshWalletStatus(true);
  } catch (error) {
    statusNode.textContent = "Error";
    output.textContent = error.message;
  } finally {
    buttons.forEach((button) => {
      button.disabled = false;
    });
  }
}

function buildSavedConfigFromForm() {
  const current = cloneConfig(getConfig());
  const base = current || createFallbackConfig();
  const f = readForm();

  base.defaults = {
    ...(base.defaults || {}),
    launchpad: f.launchpad || "pump",
    mode: f.mode || "regular",
    activePresetId: f.activePresetId || DEFAULT_PRESET_ID,
    presetEditing: isPresetEditing(base),
    automaticDevSell: {
      enabled: Boolean(f.automaticDevSellEnabled),
      percent: Number(f.automaticDevSellPercent || 0),
      delaySeconds: Number(f.automaticDevSellDelaySeconds || 0),
    },
  };

  base.presets = base.presets || {};
  base.presets.items = getPresetItems(base).map((preset) => preset.id === f.activePresetId
    ? {
        ...preset,
        creationSettings: {
          ...preset.creationSettings,
          provider: f.provider || "helius",
          policy: f.policy || "safe",
          tipSol: f.creationTipSol || "",
          priorityFeeSol: f.priorityFeeSol || "",
          devBuySol: (preset.creationSettings && preset.creationSettings.devBuySol) || "",
        },
        buySettings: {
          ...preset.buySettings,
          provider: f.buyProvider || "helius",
          policy: f.buyPolicy || "safe",
          priorityFeeSol: f.buyPriorityFeeSol || "",
          tipSol: f.buyTipSol || "",
          slippagePercent: f.buySlippagePercent || "",
        },
        sellSettings: {
          ...preset.sellSettings,
          provider: f.sellProvider || "helius",
          policy: f.sellPolicy || "safe",
          priorityFeeSol: f.sellPriorityFeeSol || "",
          tipSol: f.sellTipSol || "",
          slippagePercent: f.sellSlippagePercent || "",
        },
      }
    : preset);

  return base;
}

async function saveSettings() {
  setBusy(true, "Saving defaults...");
  try {
    const response = await fetch("/api/settings", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        config: buildSavedConfigFromForm(),
      }),
    });
    const payload = await response.json();
    if (!response.ok || !payload.ok) {
      throw new Error(payload.error || "Failed to save settings.");
    }
    statusNode.textContent = "Defaults saved";
    setConfig(payload.config);
    metaNode.textContent = "Launch defaults and selected presets saved.";
    renderQuickDevBuyButtons(payload.config);
    populateDevBuyPresetEditor(payload.config);
    hideSettingsModal();
  } catch (error) {
    statusNode.textContent = "Error";
    output.textContent = error.message;
  } finally {
    buttons.forEach((button) => {
      button.disabled = false;
    });
    if (saveSettingsButton) saveSettingsButton.disabled = false;
  }
}

function showSettingsModal() {
  renderPresetChips();
  applyPresetToSettingsInputs(getActivePreset(getConfig()), { syncToMainForm: false });
  setPresetEditing(isPresetEditing(getConfig()));
  if (settingsModal) settingsModal.hidden = false;
}

function hideSettingsModal() {
  if (settingsModal) settingsModal.hidden = true;
}

function getStoredOutputSectionVisible() {
  try {
    const stored = window.localStorage.getItem(OUTPUT_SECTION_VISIBILITY_KEY);
    if (stored === "true") return true;
    if (stored === "false") return false;
  } catch (_error) {
    // Ignore storage access failures and fall back to default visible state.
  }
  return true;
}

function setOutputSectionVisible(isVisible) {
  document.documentElement.classList.toggle("output-hidden", !isVisible);
  document.body.classList.toggle("output-hidden", !isVisible);
  if (outputSection) outputSection.hidden = !isVisible;
  if (toggleOutputButton) {
    toggleOutputButton.classList.toggle("active", isVisible);
    toggleOutputButton.setAttribute("aria-pressed", String(isVisible));
  }
  try {
    window.localStorage.setItem(OUTPUT_SECTION_VISIBILITY_KEY, String(isVisible));
  } catch (_error) {
    // Ignore storage access failures and keep the UI functional.
  }
}

function getStoredThemeMode() {
  try {
    const stored = window.localStorage.getItem(THEME_MODE_STORAGE_KEY);
    return stored === "light" ? "light" : "dark";
  } catch (_error) {
    return "dark";
  }
}

function setThemeMode(mode, { persist = true } = {}) {
  const normalized = mode === "light" ? "light" : "dark";
  document.documentElement.classList.toggle("theme-light", normalized === "light");
  document.body.classList.toggle("theme-light", normalized === "light");
  if (themeToggleButton) {
    themeToggleButton.textContent = normalized === "light" ? "☾" : "☀";
    themeToggleButton.classList.toggle("active", normalized === "light");
    themeToggleButton.setAttribute("aria-pressed", normalized === "light" ? "true" : "false");
    themeToggleButton.setAttribute("title", normalized === "light" ? "Switch to dark mode" : "Switch to white mode");
    themeToggleButton.setAttribute("aria-label", normalized === "light" ? "Switch to dark mode" : "Switch to white mode");
  }
  if (!persist) return;
  try {
    window.localStorage.setItem(THEME_MODE_STORAGE_KEY, normalized);
  } catch (_error) {
    // Ignore storage failures and keep theme switching functional.
  }
}

function completeInitialBoot() {
  if (window.__launchdeckBootFallback) {
    window.clearTimeout(window.__launchdeckBootFallback);
    window.__launchdeckBootFallback = null;
  }
  requestAnimationFrame(() => {
    document.documentElement.classList.remove("boot-pending");
  });
}

function openPopoutWindow() {
  const popoutUrl = new URL(window.location.href);
  popoutUrl.searchParams.set("popout", "1");
  window.open(
    popoutUrl.toString(),
    "launchdeck-popout",
    "popup=yes,width=560,height=920,menubar=no,toolbar=no,location=no,status=no,resizable=yes,scrollbars=yes",
  );
}

function toggleDevAutoSellPanel(forceOpen) {
  if (!devAutoSellPanel) return;
  const shouldOpen = typeof forceOpen === "boolean" ? forceOpen : devAutoSellPanel.hidden;
  devAutoSellPanel.hidden = !shouldOpen;
}

async function updateImageRecord(id, updates) {
  const response = await fetch("/api/images/update", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id, ...updates }),
  });
  const payload = await response.json();
  if (!response.ok || !payload.ok) {
    throw new Error(payload.error || "Failed to update image.");
  }
  imageLibraryState.images = Array.isArray(payload.images) ? payload.images : imageLibraryState.images;
  imageLibraryState.categories = Array.isArray(payload.categories) ? payload.categories : imageLibraryState.categories;
  const updated = payload.image || imageLibraryState.images.find((entry) => entry.id === id);
  if (uploadedImage && uploadedImage.id === id && updated) {
    setSelectedImage(updated);
  }
  renderImageCategoryChips();
  renderImageDetailsCategoryOptions(updated ? updated.category || "" : imageDetailsCategory ? imageDetailsCategory.value : "");
  renderImageLibraryGrid();
}

async function deleteImageRecord(id) {
  const response = await fetch("/api/images/delete", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id }),
  });
  const payload = await response.json();
  if (!response.ok || !payload.ok) {
    throw new Error(payload.error || "Failed to delete image.");
  }
  imageLibraryState.images = Array.isArray(payload.images) ? payload.images : [];
  imageLibraryState.categories = Array.isArray(payload.categories) ? payload.categories : [];
  if (uploadedImage && uploadedImage.id === id) {
    setSelectedImage(null);
  }
  renderImageCategoryChips();
  renderImageDetailsCategoryOptions(imageDetailsCategory ? imageDetailsCategory.value : "");
  renderImageLibraryGrid();
}

form.querySelectorAll('input[name="mode"]').forEach((node) => {
  node.addEventListener("change", () => {
    updateModeVisibility();
    if (node.value === "agent-custom" && node.checked) {
      showAgentSplitModal();
    }
  });
  if (node.value === "agent-custom") {
    const option = node.closest(".mode-option");
    if (option) {
      option.addEventListener("click", () => {
        queueMicrotask(() => {
          if (node.checked) showAgentSplitModal();
        });
      });
    }
  }
});
launchpadInputs.forEach((input) => {
  input.addEventListener("change", () => {
    if (!input.checked) return;
    applyLaunchpadTokenMetadata();
  });
});
if (nameInput) {
  nameInput.addEventListener("input", () => {
    syncTickerFromName();
  });
}
if (descriptionToggle) {
  descriptionToggle.addEventListener("click", () => {
    toggleDescriptionDisclosure();
  });
}
if (descriptionInput) {
  descriptionInput.addEventListener("input", () => {
    updateDescriptionDisclosure();
  });
}
if (symbolInput) {
  symbolInput.addEventListener("focus", () => {
    const autoTickerValue = getAutoTickerValue();
    if (!tickerManuallyEdited && autoTickerValue && symbolInput.value === autoTickerValue) {
      syncingTickerFromName = true;
      symbolInput.value = "";
      syncingTickerFromName = false;
      tickerManuallyEdited = true;
      tickerClearedForManualEntry = true;
      updateTokenFieldCounts();
    }
  });
  symbolInput.addEventListener("input", () => {
    if (syncingTickerFromName) return;
    const formatted = formatTickerValue(symbolInput.value);
    syncingTickerFromName = true;
    if (symbolInput.value !== formatted) {
      symbolInput.value = formatted;
    }
    syncingTickerFromName = false;
    tickerManuallyEdited = true;
    tickerClearedForManualEntry = symbolInput.value.trim().length === 0;
    updateTokenFieldCounts();
  });
  symbolInput.addEventListener("blur", () => {
    if (!symbolInput.value.trim()) {
      tickerManuallyEdited = false;
      tickerClearedForManualEntry = false;
      syncTickerFromName();
      return;
    }
    tickerClearedForManualEntry = false;
  });
}
if (tickerCapsToggle) {
  tickerCapsToggle.addEventListener("click", () => {
    tickerCapsToggle.classList.toggle("active");
    applyTickerCapsMode();
    if (!tickerManuallyEdited) {
      syncTickerFromName();
    }
  });
}
if (changeDevBuyPresetsButton) {
  changeDevBuyPresetsButton.addEventListener("click", () => {
    setDevBuyPresetEditorOpen(true);
    populateDevBuyPresetEditor(getConfig());
  });
}
if (cancelDevBuyPresetsButton) {
  cancelDevBuyPresetsButton.addEventListener("click", () => {
    setDevBuyPresetEditorOpen(false);
  });
}
if (saveDevBuyPresetsButton) {
  saveDevBuyPresetsButton.addEventListener("click", async () => {
    await saveDevBuyPresetEditor();
  });
}
if (devBuySolInput) {
  devBuySolInput.addEventListener("input", async () => {
    if (syncingDevBuyInputs) return;
    await updateDevBuyFromSolInput(devBuySolInput.value);
  });
}
if (devBuyPercentInput) {
  devBuyPercentInput.addEventListener("input", async () => {
    if (syncingDevBuyInputs) return;
    await updateDevBuyFromPercentInput(devBuyPercentInput.value);
  });
}
if (providerSelect) providerSelect.addEventListener("change", () => {
  syncActivePresetFromInputs();
  updateJitoVisibility();
});
if (buyProviderSelect) buyProviderSelect.addEventListener("change", syncActivePresetFromInputs);
if (sellProviderSelect) sellProviderSelect.addEventListener("change", syncActivePresetFromInputs);
feeSplitPill.addEventListener("click", () => {
  if (getMode() !== "regular") return;
  if (!feeSplitEnabled.checked) {
    feeSplitEnabled.checked = true;
    showFeeSplitModal();
    return;
  }
  showFeeSplitModal();
});
if (walletTriggerButton) {
  walletTriggerButton.addEventListener("click", () => {
    toggleWalletDropdown();
  });
}
if (walletRefreshButton) {
  walletRefreshButton.addEventListener("click", async () => {
    await refreshWalletStatus(true);
  });
}
if (walletDropdownList) {
  walletDropdownList.addEventListener("click", (event) => {
    const button = event.target.closest(".wallet-option-button");
    if (!button || !walletSelect) return;
    const nextKey = String(button.dataset.walletKey || "").trim();
    if (!nextKey) return;
    walletSelect.value = nextKey;
    setWalletDropdownOpen(false);
    refreshWalletStatus(true);
  });
}
walletSelect.addEventListener("change", () => {
  refreshWalletStatus(true);
});
document.addEventListener("click", (event) => {
  if (!walletDropdown || walletDropdown.hidden) return;
  const target = event.target;
  if (walletBox && walletBox.contains(target)) return;
  setWalletDropdownOpen(false);
});

feeSplitAdd.addEventListener("click", () => {
  feeSplitList.appendChild(createFeeSplitRow({ type: "wallet", sharePercent: "" }));
  syncFeeSplitTotals();
});

feeSplitReset.addEventListener("click", () => {
  getFeeSplitRows().forEach((row) => {
    row.querySelector(".recipient-share").value = "0";
    row.querySelector(".recipient-slider").value = "0";
  });
  syncFeeSplitTotals();
});

feeSplitEven.addEventListener("click", () => {
  const rows = getFeeSplitRows();
  const targetRows = rows;
  if (targetRows.length === 0) return;
  rows.forEach((row) => {
    row.querySelector(".recipient-share").value = "0";
    row.querySelector(".recipient-slider").value = "0";
  });
  const evenShare = Number((100 / targetRows.length).toFixed(2));
  let assigned = 0;
  targetRows.forEach((row, index) => {
    const share = index === targetRows.length - 1 ? Number((100 - assigned).toFixed(2)) : evenShare;
    assigned += share;
    row.querySelector(".recipient-share").value = String(share);
    row.querySelector(".recipient-slider").value = String(share);
  });
  syncFeeSplitTotals();
});

feeSplitList.addEventListener("click", (event) => {
  const lockToggle = event.target.closest(".recipient-lock-toggle");
  if (lockToggle) {
    const row = lockToggle.closest(".fee-split-row");
    setRecipientTargetLocked(row, row.dataset.targetLocked !== "true");
    syncFeeSplitTotals();
    return;
  }
  const tab = event.target.closest(".recipient-type-tab");
  if (tab) {
    updateFeeSplitRowType(tab.closest(".fee-split-row"), tab.dataset.type);
    return;
  }
  const removeButton = event.target.closest(".recipient-remove");
  if (removeButton) {
    removeButton.closest(".fee-split-row").remove();
    syncFeeSplitTotals();
  }
});

feeSplitList.addEventListener("input", (event) => {
  const row = event.target.closest(".fee-split-row");
  if (!row) return;
  if (event.target.classList.contains("recipient-slider")) {
    row.querySelector(".recipient-share").value = event.target.value;
  }
  if (event.target.classList.contains("recipient-share")) {
    row.querySelector(".recipient-slider").value = event.target.value || "0";
  }
  syncFeeSplitTotals();
});

agentSplitAdd.addEventListener("click", () => {
  agentSplitList.appendChild(createAgentSplitRow({ type: "wallet", sharePercent: "" }));
  normalizeAgentSplitStructure({ afterAdd: true });
  syncAgentSplitTotals();
  setAgentSplitModalError("");
});

agentSplitReset.addEventListener("click", () => {
  getAgentSplitRows().forEach((row) => {
    row.querySelector(".recipient-share").value = "0";
    row.querySelector(".recipient-slider").value = "0";
  });
  syncAgentSplitTotals();
  setAgentSplitModalError("");
});

agentSplitEven.addEventListener("click", () => {
  const rows = getAgentSplitRows();
  const targetRows = rows;
  if (targetRows.length === 0) return;
  rows.forEach((row) => {
    row.querySelector(".recipient-share").value = "0";
    row.querySelector(".recipient-slider").value = "0";
  });
  const evenShare = Number((100 / targetRows.length).toFixed(2));
  let assigned = 0;
  targetRows.forEach((row, index) => {
    const share = index === targetRows.length - 1 ? Number((100 - assigned).toFixed(2)) : evenShare;
    assigned += share;
    row.querySelector(".recipient-share").value = String(share);
    row.querySelector(".recipient-slider").value = String(share);
  });
  syncAgentSplitTotals();
  setAgentSplitModalError("");
});

agentSplitList.addEventListener("click", (event) => {
  const lockToggle = event.target.closest(".recipient-lock-toggle");
  if (lockToggle) {
    const row = lockToggle.closest(".fee-split-row");
    setRecipientTargetLocked(row, row.dataset.targetLocked !== "true");
    syncAgentSplitTotals();
    setAgentSplitModalError("");
    return;
  }
  const tab = event.target.closest(".recipient-type-tab");
  if (tab && tab.dataset.type) {
    updateFeeSplitRowType(tab.closest(".fee-split-row"), tab.dataset.type);
    syncAgentSplitTotals();
    setAgentSplitModalError("");
    return;
  }
  const removeButton = event.target.closest(".recipient-remove");
  if (removeButton) {
    removeButton.closest(".fee-split-row").remove();
    normalizeAgentSplitStructure();
    syncAgentSplitTotals();
    setAgentSplitModalError("");
  }
});

agentSplitList.addEventListener("input", (event) => {
  const row = event.target.closest(".fee-split-row");
  if (!row) return;
  if (event.target.classList.contains("recipient-slider")) {
    row.querySelector(".recipient-share").value = event.target.value;
  }
  if (event.target.classList.contains("recipient-share")) {
    row.querySelector(".recipient-slider").value = event.target.value || "0";
  }
  if (event.target.classList.contains("recipient-target") && row.dataset.defaultReceiver === "true") {
    delete row.dataset.defaultReceiver;
  }
  syncAgentSplitTotals();
  setAgentSplitModalError("");
});

Object.keys(fieldValidators).forEach((name) => {
  const input = getNamedInput(name);
  if (!input) return;
  input.addEventListener("blur", () => validateFieldByName(name));
  input.addEventListener("input", () => {
    if (input.classList.contains("input-error")) validateFieldByName(name);
  });
});

if (openImageLibraryButton) {
  openImageLibraryButton.addEventListener("click", showImageLibraryModal);
}
imageInput.addEventListener("change", async () => {
  const [file] = imageInput.files || [];
  if (!file) return;
  imageStatus.textContent = "Uploading image to library...";

  try {
    await uploadSelectedImage(file);
  } catch (error) {
    imageStatus.textContent = error.message;
  } finally {
    imageInput.value = "";
  }
});
if (imageLibraryClose) imageLibraryClose.addEventListener("click", hideImageLibraryModal);
if (imageLibraryModal) {
  imageLibraryModal.addEventListener("click", (event) => {
    if (event.target === imageLibraryModal) hideImageLibraryModal();
  });
}
if (imageLibraryUploadButton) {
  imageLibraryUploadButton.addEventListener("click", () => {
    imageInput.value = "";
    imageInput.click();
  });
}
if (imageLibrarySearchInput) {
  imageLibrarySearchInput.addEventListener("input", () => {
    imageLibraryState.search = imageLibrarySearchInput.value.trim();
    fetchImageLibrary().catch((error) => {
      imageStatus.textContent = error.message;
    });
  });
}
document.querySelectorAll("[data-image-category]").forEach((button) => {
  button.addEventListener("click", () => {
    imageLibraryState.category = button.getAttribute("data-image-category") || "all";
    fetchImageLibrary().catch((error) => {
      imageStatus.textContent = error.message;
    });
  });
});
if (imageCategoryChips) {
  imageCategoryChips.addEventListener("click", (event) => {
    const chip = event.target.closest("[data-image-category]");
    if (!chip) return;
    imageLibraryState.category = chip.getAttribute("data-image-category") || "all";
    fetchImageLibrary().catch((error) => {
      imageStatus.textContent = error.message;
    });
  });
}
if (newImageCategoryButton) {
  newImageCategoryButton.addEventListener("click", () => showImageCategoryModal("library"));
}
if (imageLibraryGrid) {
  imageLibraryGrid.addEventListener("click", (event) => {
    const uploadTile = event.target.closest("[data-image-upload-tile]");
    if (uploadTile) {
      imageInput.value = "";
      imageInput.click();
      return;
    }
    const menuButton = event.target.closest("[data-image-menu-id]");
    if (menuButton) {
      event.stopPropagation();
      openImageItemMenu(menuButton.getAttribute("data-image-menu-id"), menuButton);
      return;
    }
    const imageButton = event.target.closest("[data-image-id]");
    if (!imageButton) return;
    const image = imageLibraryState.images.find((entry) => entry.id === imageButton.getAttribute("data-image-id"));
    if (!image) return;
    imageLibraryState.activeImageId = image.id;
    setSelectedImage(image);
    hideImageLibraryModal();
  });
}
if (imageMenuFavorite) {
  imageMenuFavorite.addEventListener("click", async () => {
    const image = imageLibraryState.images.find((entry) => entry.id === activeImageMenuId);
    if (!image) return;
    try {
      await updateImageRecord(image.id, { isFavorite: !image.isFavorite });
      hideImageItemMenu();
    } catch (error) {
      imageStatus.textContent = error.message;
    }
  });
}
if (imageMenuEdit) {
  imageMenuEdit.addEventListener("click", () => {
    const image = imageLibraryState.images.find((entry) => entry.id === activeImageMenuId);
    if (!image) return;
    hideImageItemMenu();
    showImageDetailsModal(image);
  });
}
if (imageMenuDelete) {
  imageMenuDelete.addEventListener("click", async () => {
    const image = imageLibraryState.images.find((entry) => entry.id === activeImageMenuId);
    if (!image) return;
    if (!window.confirm(`Delete image "${image.name}"?`)) return;
    try {
      await deleteImageRecord(image.id);
      hideImageItemMenu();
    } catch (error) {
      imageStatus.textContent = error.message;
    }
  });
}
if (imageDetailsClose) imageDetailsClose.addEventListener("click", hideImageDetailsModal);
if (imageDetailsCancel) imageDetailsCancel.addEventListener("click", hideImageDetailsModal);
if (imageDetailsNewCategory) {
  imageDetailsNewCategory.addEventListener("click", () => showImageCategoryModal("details"));
}
if (imageCategoryClose) imageCategoryClose.addEventListener("click", hideImageCategoryModal);
if (imageCategoryCancel) imageCategoryCancel.addEventListener("click", hideImageCategoryModal);
if (imageCategoryName) {
  imageCategoryName.addEventListener("keydown", async (event) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    if (imageCategorySave) imageCategorySave.click();
  });
}
if (imageCategorySave) {
  imageCategorySave.addEventListener("click", async () => {
    setImageCategoryError("");
    imageCategorySave.disabled = true;
    imageCategorySave.textContent = "Creating...";
    try {
      const createdCategory = await createImageCategory(imageCategoryName ? imageCategoryName.value : "");
      if (imageCategoryModalContext === "details") {
        renderImageDetailsCategoryOptions(createdCategory);
        if (imageDetailsCategory) imageDetailsCategory.value = createdCategory;
      } else {
        imageLibraryState.category = createdCategory;
        await fetchImageLibrary();
      }
      hideImageCategoryModal();
    } catch (error) {
      setImageCategoryError(error.message || "Failed to create category.");
    } finally {
      imageCategorySave.disabled = false;
      imageCategorySave.textContent = "Create Category";
    }
  });
}
if (imageDetailsAddTag) {
  imageDetailsAddTag.addEventListener("click", () => {
    addImageDetailTag(imageDetailsTags ? imageDetailsTags.value : "");
  });
}
if (imageDetailsTags) {
  imageDetailsTags.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === ",") {
      event.preventDefault();
      addImageDetailTag(imageDetailsTags.value);
    }
  });
}
if (imageDetailsTagList) {
  imageDetailsTagList.addEventListener("click", (event) => {
    const chip = event.target.closest("[data-image-tag-index]");
    if (!chip) return;
    const index = Number(chip.getAttribute("data-image-tag-index"));
    if (!Number.isInteger(index) || index < 0) return;
    imageDetailsTagsState.splice(index, 1);
    renderImageDetailsTags();
  });
}
if (imageDetailsSave) {
  imageDetailsSave.addEventListener("click", async () => {
    if (!activeImageDetailsId) return;
    setImageDetailsError("");
    addImageDetailTag(imageDetailsTags ? imageDetailsTags.value : "");
    imageDetailsSave.disabled = true;
    imageDetailsSave.textContent = "Saving...";
    try {
      await updateImageRecord(activeImageDetailsId, {
        name: imageDetailsName ? imageDetailsName.value.trim() : "",
        tags: imageDetailsTagsState,
        category: imageDetailsCategory ? imageDetailsCategory.value.trim() : "",
      });
      if (isEditingNewImageUpload) {
        imageStatus.textContent = "Image saved to library.";
        imagePath.textContent = "";
      }
      hideImageDetailsModal();
    } catch (error) {
      setImageDetailsError(error.message || "Failed to save image details.");
    } finally {
      imageDetailsSave.disabled = false;
      imageDetailsSave.textContent = "Save Changes";
    }
  });
}

testFillButton.addEventListener("click", async () => {
  await applyTestPreset();
});
if (openPopoutButton) {
  openPopoutButton.addEventListener("click", openPopoutWindow);
}
if (openVampButton) {
  openVampButton.addEventListener("click", showVampModal);
}
if (themeToggleButton) {
  themeToggleButton.addEventListener("click", () => {
    const nextMode = document.documentElement.classList.contains("theme-light") ? "dark" : "light";
    setThemeMode(nextMode);
  });
}
if (toggleOutputButton) {
  toggleOutputButton.addEventListener("click", () => {
    setOutputSectionVisible(outputSection ? outputSection.hidden : true);
  });
}
if (openSettingsButton) {
  openSettingsButton.addEventListener("click", showSettingsModal);
}
saveSettingsButton.addEventListener("click", async () => {
  await saveSettings();
});

buttons.forEach((button) => {
  button.addEventListener("click", () => {
    const action = button.dataset.action;
    const errors = validateForm();
    if (showValidationErrors(errors)) return;
    clearValidationErrors();
    if (action === "deploy") {
      showDeployModal();
    } else {
      run(action);
    }
  });
});

modalClose.addEventListener("click", hideDeployModal);
modalCancel.addEventListener("click", hideDeployModal);
modalConfirm.addEventListener("click", () => {
  hideDeployModal();
  run("deploy");
});
if (settingsClose) settingsClose.addEventListener("click", hideSettingsModal);
if (settingsCancel) settingsCancel.addEventListener("click", hideSettingsModal);
if (settingsModal) {
  settingsModal.addEventListener("click", (event) => {
    if (event.target === settingsModal) hideSettingsModal();
  });
}
if (topPresetChipBar) {
  topPresetChipBar.addEventListener("click", (event) => {
    const chip = event.target.closest("[data-preset-id]");
    if (!chip) return;
    setActivePreset(chip.getAttribute("data-preset-id") || DEFAULT_PRESET_ID);
  });
}
if (settingsPresetChipBar) {
  settingsPresetChipBar.addEventListener("click", (event) => {
    const chip = event.target.closest("[data-preset-id]");
    if (!chip) return;
    setActivePreset(chip.getAttribute("data-preset-id") || DEFAULT_PRESET_ID);
  });
}
if (presetEditToggle) {
  presetEditToggle.addEventListener("click", () => {
    setPresetEditing(!isPresetEditing(getConfig()));
  });
}
[
  creationTipInput,
  creationPriorityInput,
  buyPriorityFeeInput,
  buyTipInput,
  buySlippageInput,
  sellPriorityFeeInput,
  sellTipInput,
  sellSlippageInput,
].forEach((input) => {
  if (!input) return;
  const eventName = input.tagName === "SELECT" || input.type === "checkbox" ? "change" : "input";
  input.addEventListener(eventName, () => {
    syncActivePresetFromInputs();
    if (input.name) validateFieldByName(input.name);
  });
});
if (devBuyQuickButtons) {
  devBuyQuickButtons.addEventListener("click", async (event) => {
    if (devBuyPresetEditorOpen) return;
    const button = event.target.closest("[data-quick-buy-amount]");
    if (!button) return;
    const presetId = button.getAttribute("data-quick-buy-preset-id") || DEFAULT_PRESET_ID;
    const amount = button.getAttribute("data-quick-buy-amount") || "";
    if (!amount) return;
    setActivePreset(presetId);
    await triggerDeployWithDevBuy("sol", amount, "sol");
  });
}
if (devBuyCustomDeployButton) {
  devBuyCustomDeployButton.addEventListener("click", async () => {
    const mode = getDevBuyMode();
    const amount = getNamedValue("devBuyAmount").trim();
    if (!amount) {
      showValidationErrors(["Custom dev buy amount is required."]);
      return;
    }
    await triggerDeployWithDevBuy(mode, amount, lastDevBuyEditSource);
  });
}
if (devAutoSellButton) {
  devAutoSellButton.addEventListener("click", (event) => {
    event.stopPropagation();
    toggleDevAutoSellPanel();
  });
}
if (modeSniperButton) {
  modeSniperButton.addEventListener("click", () => {
    showSniperModal();
  });
}
if (modeVanityButton) {
  modeVanityButton.addEventListener("click", () => {
    showVanityModal();
  });
}
if (autoSellEnabledInput) {
  autoSellEnabledInput.addEventListener("change", () => {
    syncDevAutoSellUI();
    syncActivePresetFromInputs();
    validateFieldByName("automaticDevSellPercent");
    validateFieldByName("automaticDevSellDelaySeconds");
  });
}
if (autoSellDelaySlider) {
  autoSellDelaySlider.addEventListener("input", () => {
    setNamedValue("automaticDevSellDelaySeconds", autoSellDelaySlider.value);
    syncDevAutoSellUI();
    syncActivePresetFromInputs();
    validateFieldByName("automaticDevSellDelaySeconds");
  });
}
if (autoSellPercentSlider) {
  autoSellPercentSlider.addEventListener("input", () => {
    setNamedValue("automaticDevSellPercent", autoSellPercentSlider.value);
    syncDevAutoSellUI();
    syncActivePresetFromInputs();
    validateFieldByName("automaticDevSellPercent");
  });
}
if (sniperEnabledToggle) {
  sniperEnabledToggle.addEventListener("change", () => {
    sniperState.enabled = sniperEnabledToggle.checked;
    setSniperModalError("");
    renderSniperUI();
  });
}
if (sniperWalletList) {
  sniperWalletList.addEventListener("change", (event) => {
    const checkbox = event.target.closest("[data-sniper-wallet-checkbox]");
    if (!checkbox) return;
    const envKey = checkbox.getAttribute("data-sniper-wallet-checkbox");
    if (!envKey) return;
    sniperState.wallets[envKey] = {
      ...(sniperState.wallets[envKey] || {}),
      selected: checkbox.checked,
      amountSol: checkbox.checked ? (sniperState.wallets[envKey] && sniperState.wallets[envKey].amountSol) || "" : "",
    };
    setSniperModalError("");
    renderSniperUI();
  });
  sniperWalletList.addEventListener("input", (event) => {
    const amountInput = event.target.closest("[data-sniper-wallet-amount]");
    if (!amountInput) return;
    const envKey = amountInput.getAttribute("data-sniper-wallet-amount");
    if (!envKey) return;
    const normalized = normalizeDecimalInput(amountInput.value);
    amountInput.value = normalized;
    sniperState.wallets[envKey] = {
      ...(sniperState.wallets[envKey] || {}),
      selected: true,
      amountSol: normalized,
    };
    applySniperStateToForm();
    renderSniperButtonState();
    setSniperModalError("");
  });
  sniperWalletList.addEventListener("click", (event) => {
    const presetButton = event.target.closest("[data-sniper-preset]");
    if (!presetButton) return;
    const envKey = presetButton.getAttribute("data-sniper-preset");
    const ratio = Number(presetButton.getAttribute("data-sniper-ratio") || 0);
    const wallet = latestWalletStatus && Array.isArray(latestWalletStatus.wallets)
      ? latestWalletStatus.wallets.find((entry) => entry.envKey === envKey)
      : null;
    if (!envKey || !wallet || !Number.isFinite(ratio)) return;
    const balance = getWalletBalanceForSniper(wallet);
    const amount = normalizeDecimalInput((balance * ratio).toFixed(6));
    sniperState.wallets[envKey] = {
      ...(sniperState.wallets[envKey] || {}),
      selected: true,
      amountSol: amount,
    };
    setSniperModalError("");
    renderSniperUI();
  });
}
if (sniperRefreshButton) {
  sniperRefreshButton.addEventListener("click", async () => {
    setSniperModalError("");
    try {
      await refreshWalletStatus(true);
    } catch (error) {
      setSniperModalError(error.message || "Failed to refresh balances.");
    }
  });
}
if (sniperResetButton) {
  sniperResetButton.addEventListener("click", () => {
    setSniperModalError("");
    resetSniperState();
  });
}
if (sniperSave) {
  sniperSave.addEventListener("click", () => {
    const errors = validateSniperState();
    if (errors.length > 0) {
      setSniperModalError(errors[0]);
      return;
    }
    setSniperModalError("");
    hideSniperModal();
  });
}
if (sniperClose) sniperClose.addEventListener("click", hideSniperModal);
if (sniperCancel) sniperCancel.addEventListener("click", hideSniperModal);
if (sniperModal) {
  sniperModal.addEventListener("click", (event) => {
    if (event.target === sniperModal) hideSniperModal();
  });
}
if (feeSplitClose) feeSplitClose.addEventListener("click", hideFeeSplitModal);
if (feeSplitSave) feeSplitSave.addEventListener("click", hideFeeSplitModal);
if (feeSplitDisable) {
  feeSplitDisable.addEventListener("click", () => {
    feeSplitEnabled.checked = false;
    updateFeeSplitVisibility();
    hideFeeSplitModal();
  });
}
if (feeSplitModal) {
  feeSplitModal.addEventListener("click", (event) => {
    if (event.target === feeSplitModal) hideFeeSplitModal();
  });
}
if (agentSplitClose) agentSplitClose.addEventListener("click", attemptCloseAgentSplitModal);
if (agentSplitCancel) {
  agentSplitCancel.addEventListener("click", () => {
    resetAgentSplitToDefault();
    hideAgentSplitModal();
  });
}
if (agentSplitSave) agentSplitSave.addEventListener("click", attemptCloseAgentSplitModal);
if (agentSplitModal) {
  agentSplitModal.addEventListener("click", (event) => {
    if (event.target === agentSplitModal) attemptCloseAgentSplitModal();
  });
}
if (vanitySave) {
  vanitySave.addEventListener("click", () => {
    const nextValue = vanityPrivateKeyText ? vanityPrivateKeyText.value.trim() : "";
    if (vanityModalError) vanityModalError.textContent = "";
    applyVanityValue(nextValue);
    hideVanityModal();
  });
}
if (vanityClear) {
  vanityClear.addEventListener("click", () => {
    if (vanityPrivateKeyText) vanityPrivateKeyText.value = "";
    if (vanityModalError) vanityModalError.textContent = "";
    applyVanityValue("");
    hideVanityModal();
  });
}
if (vanityClose) vanityClose.addEventListener("click", hideVanityModal);
if (vanityModal) {
  vanityModal.addEventListener("click", (event) => {
    if (event.target === vanityModal) hideVanityModal();
  });
}
if (vampImport) {
  vampImport.addEventListener("click", async () => {
    await importVampToken();
  });
}
if (vampClose) vampClose.addEventListener("click", hideVampModal);
if (vampCancel) vampCancel.addEventListener("click", hideVampModal);
if (vampContractInput) {
  vampContractInput.addEventListener("keydown", async (event) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    await importVampToken();
  });
}
if (vampModal) {
  vampModal.addEventListener("click", (event) => {
    if (event.target === vampModal) hideVampModal();
  });
}
document.addEventListener("click", (event) => {
  if (!imageItemMenu.hidden) {
    const clickedMenu = imageItemMenu.contains(event.target);
    const clickedTrigger = event.target.closest("[data-image-menu-id]");
    if (!clickedMenu && !clickedTrigger) hideImageItemMenu();
  }
  if (!devAutoSellPanel || devAutoSellPanel.hidden) return;
  if (devAutoSellPanel.contains(event.target) || (devAutoSellButton && devAutoSellButton.contains(event.target))) return;
  toggleDevAutoSellPanel(false);
});
deployModal.addEventListener("click", (event) => {
  if (event.target === deployModal) hideDeployModal();
});

updateModeVisibility();
updateJitoVisibility();
syncDevAutoSellUI();
hydrateModeActionState();
if (bootstrapState && bootstrapState.config) {
  applyPersistentDefaults(bootstrapState.config);
}
if (bootstrapState && bootstrapState.launchpads) {
  applyLaunchpadAvailability(bootstrapState.launchpads);
}
renderPresetChips();
renderQuickDevBuyButtons();
populateDevBuyPresetEditor();
updateTokenFieldCounts();
updateDescriptionDisclosure();
updateQuote();
Promise.resolve(refreshWalletStatus(false)).finally(() => {
  completeInitialBoot();
});
