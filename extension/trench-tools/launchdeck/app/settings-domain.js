(function initLaunchDeckSettingsDomain(global) {
  function createSettingsDomain(config) {
    const {
      elements = {},
      constants = {},
      renderCache = {},
      renderUtils = {},
      state = {},
      helpers = {},
      actions = {},
    } = config || {};

    const {
      topPresetChipBar,
      settingsPresetChipBar,
      presetEditToggle,
      devBuyQuickButtons,
      changeDevBuyPresetsButton,
      cancelDevBuyPresetsButton,
      saveDevBuyPresetsButton,
      providerSelect,
      creationTipInput,
      creationPriorityInput,
      creationMevModeSelect,
      creationAutoFeeInput,
      creationAutoFeeButton,
      creationMaxFeeInput,
      buyProviderSelect,
      buyPriorityFeeInput,
      buyTipInput,
      buySlippageInput,
      buyMevModeSelect,
      buyAutoFeeInput,
      buyAutoFeeButton,
      buyMaxFeeInput,
      buyHelloMoonMevWarning,
      buyStandardRpcWarning,
      sellProviderSelect,
      sellPriorityFeeInput,
      sellTipInput,
      sellSlippageInput,
      sellMevModeSelect,
      sellAutoFeeInput,
      sellAutoFeeButton,
      sellMaxFeeInput,
      sellHelloMoonMevWarning,
      sellStandardRpcWarning,
      settingsBackendRegionSummary,
      namePresetModal,
      namePresetClose,
      namePresetEditorList,
      namePresetFormActions,
      namePresetAddButton,
      namePresetCancelEditButton,
      namePresetUpdateButton,
      namePresetFormTitle,
      namePresetNewName,
      namePresetNewNamePrefix,
      namePresetNewNameSuffix,
      namePresetNewTickerPrefix,
      namePresetNewTickerSuffix,
      namePresetNewFirstWord,
      namePresetNewAbbreviate,
      namePresetError,
      settingsModal,
      settingsClose,
      settingsCancel,
      output,
    } = elements;

    const {
      defaultQuickDevBuyAmounts = ["0.5", "1", "2"],
      defaultPresetId = "preset1",
      standardRpcSlippageDefault = "20",
      providerLabels = {},
      routeCapabilities = {},
      providerFeeRequirements = {},
    } = constants;

    const {
      getLatestWalletStatus = () => null,
      setLatestWalletStatus = () => {},
      getLatestRuntimeStatus = () => null,
    } = state;

    const {
      escapeHTML = (value) => String(value || ""),
      getNamedValue = () => "",
      isNamedChecked = () => false,
      validateFieldByName = () => "",
      validateAllInlineFields = () => [],
      focusFirstInvalidInlineField = () => {},
      hasBootstrapConfig = () => false,
      setStatusLabel = () => {},
    } = helpers;

    const {
      scheduleLiveSyncBroadcast = () => {},
      queueWarmActivity = () => {},
      syncDevAutoSellUI = () => {},
      clearDevBuyState = () => {},
      renderNamePresetStrip = () => {},
    } = actions;

    let settingsModalInitialConfig = null;
    let devBuyPresetEditorOpen = false;
    let syncingPresetInputs = false;
    let lastTopPresetMarkup = "";
    let lastSettingsPresetMarkup = "";
    let lastQuickDevBuyMarkup = "";
    let lastNamePresetEditorMarkup = "";
    let namePresetEditingIndex = -1;
    let namePresetPersistInFlight = false;
    const DEFAULT_MANUAL_FEE_SOL = "0.001";
    const DEFAULT_NAME_PRESET_BUTTONS = [
      {
        id: "ification",
        name: "ify, ification",
        namePrefix: "",
        nameSuffix: "ification",
        tickerPrefix: "",
        tickerSuffix: "ify",
        tickerUseFirstWord: true,
        tickerAbbreviate: false,
      },
      {
        id: "otus",
        name: "_OTUS",
        namePrefix: "",
        nameSuffix: " Of The United States",
        tickerPrefix: "",
        tickerSuffix: "OTUS",
        tickerUseFirstWord: false,
        tickerAbbreviate: true,
      },
      {
        id: "justice-for",
        name: "Justice For",
        namePrefix: "Justice For ",
        nameSuffix: "",
        tickerPrefix: "",
        tickerSuffix: "",
        tickerUseFirstWord: true,
        tickerAbbreviate: false,
      },
    ];

    function cloneConfig(value) {
      return value ? JSON.parse(JSON.stringify(value)) : null;
    }

    function createFallbackConfig() {
      return {
        defaults: {
          launchpad: "pump",
          mode: "regular",
          activePresetId: defaultPresetId,
          presetEditing: false,
          quickDevBuyAmounts: [...defaultQuickDevBuyAmounts],
          namePresetButtons: getDefaultNamePresetButtons(),
          misc: {
            trackSendBlockHeight: false,
          },
        },
        presets: {
          items: defaultQuickDevBuyAmounts.map((_, index) => ({
            id: `preset${index + 1}`,
            label: `P${index + 1}`,
            creationSettings: {
              provider: "helius-sender",
              tipSol: DEFAULT_MANUAL_FEE_SOL,
              priorityFeeSol: DEFAULT_MANUAL_FEE_SOL,
              mevMode: "off",
              autoFee: false,
              maxFeeSol: "",
              devBuySol: "",
            },
            buySettings: {
              provider: "helius-sender",
              priorityFeeSol: DEFAULT_MANUAL_FEE_SOL,
              tipSol: DEFAULT_MANUAL_FEE_SOL,
              slippagePercent: "",
              mevMode: "off",
              autoFee: false,
              maxFeeSol: "",
              snipeBuyAmountSol: "",
            },
            sellSettings: {
              provider: "helius-sender",
              priorityFeeSol: DEFAULT_MANUAL_FEE_SOL,
              tipSol: DEFAULT_MANUAL_FEE_SOL,
              slippagePercent: "",
              mevMode: "off",
              autoFee: false,
              maxFeeSol: "",
            },
            automaticDevSell: {
              enabled: false,
              percent: 100,
              triggerFamily: "time",
              triggerMode: "block-offset",
              delayMs: 0,
              targetBlockOffset: 0,
              marketCapEnabled: false,
              marketCapThreshold: "",
              marketCapScanTimeoutSeconds: 30,
              marketCapTimeoutAction: "stop",
            },
            postLaunchStrategy: "none",
          })),
        },
      };
    }

    function getConfig() {
      const latestWalletStatus = getLatestWalletStatus();
      return latestWalletStatus && latestWalletStatus.config
        ? latestWalletStatus.config
        : createFallbackConfig();
    }

    function isTrackSendBlockHeightEnabled(configValue = getConfig()) {
      return Boolean(
        configValue
        && configValue.defaults
        && configValue.defaults.misc
        && configValue.defaults.misc.trackSendBlockHeight,
      );
    }

    function getPresetItems(configValue = getConfig()) {
      const items = configValue && configValue.presets && Array.isArray(configValue.presets.items)
        ? configValue.presets.items
        : null;
      return items && items.length ? items : createFallbackConfig().presets.items;
    }

    function getDefaultNamePresetButtons() {
      return DEFAULT_NAME_PRESET_BUTTONS.map((entry) => ({ ...entry }));
    }

    function normalizeNamePresetButton(entry, index = 0) {
      const source = entry && typeof entry === "object" ? entry : {};
      const fallback = DEFAULT_NAME_PRESET_BUTTONS[index] || {};
      const tickerAbbreviate = Boolean(source.tickerAbbreviate ?? fallback.tickerAbbreviate);
      return {
        id: String(source.id || fallback.id || `name-preset-${index + 1}`).trim() || `name-preset-${index + 1}`,
        name: String(source.name || fallback.name || `Preset ${index + 1}`).trim() || `Preset ${index + 1}`,
        namePrefix: String(source.namePrefix ?? fallback.namePrefix ?? ""),
        nameSuffix: String(source.nameSuffix ?? fallback.nameSuffix ?? ""),
        tickerPrefix: String(source.tickerPrefix ?? fallback.tickerPrefix ?? ""),
        tickerSuffix: String(source.tickerSuffix ?? fallback.tickerSuffix ?? ""),
        tickerUseFirstWord: tickerAbbreviate ? false : Boolean(source.tickerUseFirstWord ?? fallback.tickerUseFirstWord),
        tickerAbbreviate,
      };
    }

    function normalizeNamePresetButtons(value) {
      if (Array.isArray(value) && value.length === 0) return [];
      const normalized = Array.isArray(value)
        ? value.map((entry, index) => normalizeNamePresetButton(entry, index))
        : [];
      return normalized.length ? normalized : getDefaultNamePresetButtons();
    }

    function getNamePresetButtons(configValue = getConfig()) {
      return normalizeNamePresetButtons(
        configValue
        && configValue.defaults
        && configValue.defaults.namePresetButtons,
      );
    }

    function getActivePresetId(configValue = getConfig()) {
      return configValue && configValue.defaults && configValue.defaults.activePresetId
        ? configValue.defaults.activePresetId
        : defaultPresetId;
    }

    function getActivePreset(configValue = getConfig()) {
      const items = getPresetItems(configValue);
      return items.find((entry) => entry.id === getActivePresetId(configValue)) || items[0];
    }

    function getPresetDisplayLabel(preset, index = 0) {
      const rawLabel = String((preset && preset.label) || "").trim();
      const labelMatch = rawLabel.match(/^preset\s*([0-9]+)$/i);
      if (labelMatch) return `P${labelMatch[1]}`;
      const idMatch = String((preset && preset.id) || "").trim().match(/^preset([0-9]+)$/i);
      if (!rawLabel && idMatch) return `P${idMatch[1]}`;
      return rawLabel || `P${index + 1}`;
    }

    function isPresetEditing(configValue = getConfig()) {
      return Boolean(configValue && configValue.defaults && configValue.defaults.presetEditing);
    }

    function setConfig(nextConfig) {
      const latestWalletStatus = getLatestWalletStatus();
      if (!latestWalletStatus) {
        setLatestWalletStatus({
          connected: false,
          config: cloneConfig(nextConfig),
        });
      } else {
        setLatestWalletStatus({
          ...latestWalletStatus,
          config: cloneConfig(nextConfig),
        });
      }
      renderPresetChips();
      renderQuickDevBuyButtons(nextConfig);
      renderNamePresetEditor(nextConfig);
      renderNamePresetStrip();
      scheduleLiveSyncBroadcast({ immediate: true });
    }

    function normalizeAutoFeeCapValue(value) {
      const trimmed = String(value || "").trim();
      if (!trimmed) return "";
      const numeric = Number(trimmed);
      if (!Number.isFinite(numeric)) return trimmed;
      return numeric <= 0 ? "" : trimmed;
    }

    function setRegionRouting(nextRegionRouting) {
      const latestWalletStatus = getLatestWalletStatus();
      if (!latestWalletStatus) {
        setLatestWalletStatus({
          connected: false,
          config: cloneConfig(getConfig()),
          regionRouting: nextRegionRouting || null,
        });
        return;
      }
      setLatestWalletStatus({
        ...latestWalletStatus,
        regionRouting: nextRegionRouting || latestWalletStatus.regionRouting || null,
      });
    }

    function formatBackendRegionValue(value, fallback = "global") {
      const normalized = String(value || "").trim();
      return normalized || fallback;
    }

    function formatWarmProviders(values = []) {
      const normalized = Array.isArray(values)
        ? values.map((value) => String(value || "").trim()).filter(Boolean)
        : [];
      return normalized.length ? normalized.join(" | ") : "--";
    }

    function formatWarmTimestamp(value) {
      const timestampMs = Number(value || 0);
      if (!timestampMs) return "--";
      const date = new Date(timestampMs);
      if (Number.isNaN(date.getTime())) return "--";
      return date.toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      });
    }

    function getProvider() {
      return providerSelect ? providerSelect.value || "helius-sender" : "helius-sender";
    }

    function getBuyProvider() {
      return buyProviderSelect ? buyProviderSelect.value || "helius-sender" : "helius-sender";
    }

    function getSellProvider() {
      return sellProviderSelect ? sellProviderSelect.value || "helius-sender" : "helius-sender";
    }

    function getRouteCapabilities(route, rowType) {
      const normalizedRoute = String(route || "helius-sender").trim().toLowerCase();
      return routeCapabilities[normalizedRoute] && routeCapabilities[normalizedRoute][rowType]
        ? routeCapabilities[normalizedRoute][rowType]
        : routeCapabilities["helius-sender"][rowType];
    }

    function providerFeeRequirementsFor(provider) {
      return providerFeeRequirements[String(provider || "").trim().toLowerCase()] || null;
    }

    function providerMinimumTipSol(provider) {
      const requirements = providerFeeRequirementsFor(provider);
      return requirements ? Number(requirements.minTipSol || 0) : 0;
    }

    function formatSolAmount(value) {
      const numeric = Number(value);
      if (!Number.isFinite(numeric) || numeric <= 0) return "";
      return numeric.toFixed(9).replace(/0+$/, "").replace(/\.$/, "");
    }

    function providerTipPlaceholder(provider) {
      return DEFAULT_MANUAL_FEE_SOL;
    }

    function providerPriorityFeePlaceholder(provider) {
      return providerRequiresPriorityFee(provider) ? DEFAULT_MANUAL_FEE_SOL : "";
    }

    function providerRequiresPriorityFee(provider) {
      const requirements = providerFeeRequirementsFor(provider);
      return Boolean(requirements && requirements.priorityRequired);
    }

    function providerRequirementLabel(provider) {
      const normalized = String(provider || "").trim().toLowerCase();
      return providerLabels[normalized] || normalized || "selected provider";
    }

    function validateNonNegativeSolField(value) {
      if (!value) return "";
      const numeric = Number(value);
      if (Number.isNaN(numeric) || numeric < 0) return "Must be a valid number";
      return "";
    }

    function validateRequiredPriorityFeeField(value, provider) {
      const label = providerRequirementLabel(provider);
      if (!value) return `Priority fee is required for ${label}.`;
      const numeric = Number(value);
      if (Number.isNaN(numeric) || numeric <= 0) {
        return `Priority fee must be greater than 0 for ${label}.`;
      }
      return "";
    }

    function validateRequiredTipField(value, provider) {
      const label = providerRequirementLabel(provider);
      const minimumTipSol = providerMinimumTipSol(provider);
      if (!value) return `Tip is required for ${label}.`;
      const numeric = Number(value);
      if (Number.isNaN(numeric) || numeric < 0) return "Must be a valid number";
      if (minimumTipSol > 0 && numeric < minimumTipSol) {
        return `Tip must be at least ${formatSolAmount(minimumTipSol)} SOL for ${label}.`;
      }
      return "";
    }

    function validateOptionalAutoFeeCapField(value, provider) {
      if (!value) return "";
      const numeric = Number(value);
      if (Number.isNaN(numeric) || numeric <= 0) return "Must be greater than 0";
      const minimumTipSol = providerMinimumTipSol(provider);
      if (minimumTipSol > 0 && numeric < minimumTipSol) {
        return `Max auto fee must be at least ${formatSolAmount(minimumTipSol)} SOL for ${providerRequirementLabel(provider)}.`;
      }
      return "";
    }

    function validateRequiredAutoFeeCapField(value, provider) {
      if (!value) return "Max auto fee is required when Auto Fee is on.";
      return validateOptionalAutoFeeCapField(value, provider);
    }

    function setFieldEnabled(input, enabled) {
      if (!input) return;
      input.disabled = !enabled;
      const label = input.closest("label");
      if (label) label.classList.toggle("is-disabled", !enabled);
    }

    function syncAutoFeeButtonState(button, input) {
      if (!button || !input) return;
      button.classList.toggle("active", Boolean(input.checked));
      button.setAttribute("aria-pressed", input.checked ? "true" : "false");
      button.disabled = Boolean(input.disabled);
    }

    function syncAutoFeeButtons() {
      syncAutoFeeButtonState(creationAutoFeeButton, creationAutoFeeInput);
      syncAutoFeeButtonState(buyAutoFeeButton, buyAutoFeeInput);
      syncAutoFeeButtonState(sellAutoFeeButton, sellAutoFeeInput);
    }

    function syncFeeInputValue(input, supported) {
      if (!input) return;
      if (!supported) {
        input.value = "";
        return;
      }
      if (!String(input.value || "").trim()) {
        input.value = DEFAULT_MANUAL_FEE_SOL;
      }
    }

    function syncProviderFeeValues() {
      const creationCapabilities = getRouteCapabilities(getProvider(), "creation");
      const buyCapabilities = getRouteCapabilities(getBuyProvider(), "buy");
      const sellCapabilities = getRouteCapabilities(getSellProvider(), "sell");
      syncFeeInputValue(creationTipInput, creationCapabilities.tip);
      syncFeeInputValue(creationPriorityInput, creationCapabilities.priority);
      syncFeeInputValue(buyTipInput, buyCapabilities.tip);
      syncFeeInputValue(buyPriorityFeeInput, buyCapabilities.priority);
      syncFeeInputValue(sellTipInput, sellCapabilities.tip);
      syncFeeInputValue(sellPriorityFeeInput, sellCapabilities.priority);
    }

    function syncProviderPlaceholders() {
      if (creationTipInput) creationTipInput.placeholder = providerTipPlaceholder(getProvider());
      if (buyTipInput) buyTipInput.placeholder = providerTipPlaceholder(getBuyProvider());
      if (sellTipInput) sellTipInput.placeholder = providerTipPlaceholder(getSellProvider());
      if (creationPriorityInput) {
        creationPriorityInput.placeholder = providerPriorityFeePlaceholder(getProvider());
      }
      if (buyPriorityFeeInput) {
        buyPriorityFeeInput.placeholder = providerPriorityFeePlaceholder(getBuyProvider());
      }
      if (sellPriorityFeeInput) {
        sellPriorityFeeInput.placeholder = providerPriorityFeePlaceholder(getSellProvider());
      }
      if (buySlippageInput) buySlippageInput.placeholder = "20";
      if (sellSlippageInput) sellSlippageInput.placeholder = "20";
    }

    function isHelloMoonProvider(provider) {
      return String(provider || "").trim().toLowerCase() === "hellomoon";
    }

    function defaultMevModeForProvider(provider) {
      return isHelloMoonProvider(provider) ? "reduced" : "off";
    }

    function normalizeMevMode(value, fallback = "off") {
      if (typeof value === "boolean") return value ? "reduced" : "off";
      const normalized = String(value || "").trim().toLowerCase();
      return normalized === "reduced" || normalized === "secure" || normalized === "off"
        ? normalized
        : fallback;
    }

    function normalizeSelectableMevMode(_provider, value, fallback = "off") {
      return normalizeMevMode(value, fallback);
    }

    function setMevModeOptionAvailability(select, provider) {
      if (!select) return;
      const secureOption = Array.from(select.options).find((option) => option.value === "secure");
      if (!secureOption) return;
      void provider;
      secureOption.disabled = false;
      secureOption.textContent = "Secure";
      secureOption.title = "";
    }

    function setMevModeSelectValue(select, value, fallback = "off", provider = "") {
      if (!select) return;
      setMevModeOptionAvailability(select, provider);
      const normalizedValue = normalizeSelectableMevMode(provider, value, fallback);
      select.value = normalizedValue;
      select.dataset.lastProvider = String(provider || "").trim().toLowerCase();
      if (isHelloMoonProvider(provider)) {
        select.dataset.lastHellomoonMode = normalizedValue;
      }
    }

    function setFieldVisibility(input, visible) {
      if (!input) return;
      const label = input.closest("label");
      if (label) label.hidden = !visible;
    }

    function isStandardRpcProvider(provider) {
      return String(provider || "").trim().toLowerCase() === "standard-rpc";
    }

    function parseNumericSettingValue(value) {
      const normalized = String(value || "").trim().replace(",", ".");
      if (!normalized) return null;
      const parsed = Number(normalized);
      return Number.isFinite(parsed) ? parsed : null;
    }

    function ensureStandardRpcSlippageDefault(input, provider) {
      if (!input || !isStandardRpcProvider(provider)) return false;
      const parsed = parseNumericSettingValue(input.value);
      if (parsed == null) {
        if (input.value.trim() === standardRpcSlippageDefault) return false;
        input.value = standardRpcSlippageDefault;
        return true;
      }
      return false;
    }

    function standardRpcSlippageWarningText(sideLabel, input) {
      const parsed = parseNumericSettingValue(input && input.value);
      const overrideText = parsed != null && parsed > Number(standardRpcSlippageDefault)
        ? " Values above 20% should only be used intentionally for edge cases."
        : " Default slippage is 20%.";
      return `Standard RPC ${sideLabel}: higher MEV and slippage risk.${overrideText}`;
    }

    function hellomoonMevWarningText(sideLabel) {
      return `Hello Moon ${sideLabel}: Off mode uses QUIC without MEV protection and carries higher MEV risk than Reduced.`;
    }

    function syncStandardRpcWarnings() {
      const buyIsStandardRpc = isStandardRpcProvider(getBuyProvider());
      if (buyStandardRpcWarning) {
        buyStandardRpcWarning.hidden = !buyIsStandardRpc;
        buyStandardRpcWarning.textContent = buyIsStandardRpc
          ? standardRpcSlippageWarningText("buys", buySlippageInput)
          : "";
      }
      const sellIsStandardRpc = isStandardRpcProvider(getSellProvider());
      if (sellStandardRpcWarning) {
        sellStandardRpcWarning.hidden = !sellIsStandardRpc;
        sellStandardRpcWarning.textContent = sellIsStandardRpc
          ? standardRpcSlippageWarningText("sells", sellSlippageInput)
          : "";
      }
    }

    function syncHelloMoonMevWarnings() {
      const buyHasHelloMoonOff = isHelloMoonProvider(getBuyProvider())
        && normalizeMevMode(buyMevModeSelect ? buyMevModeSelect.value : "off") === "off";
      if (buyHelloMoonMevWarning) {
        buyHelloMoonMevWarning.hidden = !buyHasHelloMoonOff;
        buyHelloMoonMevWarning.textContent = buyHasHelloMoonOff
          ? hellomoonMevWarningText("buys")
          : "";
      }
      const sellHasHelloMoonOff = isHelloMoonProvider(getSellProvider())
        && normalizeMevMode(sellMevModeSelect ? sellMevModeSelect.value : "off") === "off";
      if (sellHelloMoonMevWarning) {
        sellHelloMoonMevWarning.hidden = !sellHasHelloMoonOff;
        sellHelloMoonMevWarning.textContent = sellHasHelloMoonOff
          ? hellomoonMevWarningText("sells")
          : "";
      }
    }

    function syncAutoFeeControls() {
      const editing = isPresetEditing(getConfig());
      const creationAuto = Boolean(creationAutoFeeInput && creationAutoFeeInput.checked);
      const buyAuto = Boolean(buyAutoFeeInput && buyAutoFeeInput.checked);
      const sellAuto = Boolean(sellAutoFeeInput && sellAutoFeeInput.checked);
      const creationCapabilities = getRouteCapabilities(getProvider(), "creation");
      const buyCapabilities = getRouteCapabilities(getBuyProvider(), "buy");
      const sellCapabilities = getRouteCapabilities(getSellProvider(), "sell");

      setFieldEnabled(creationPriorityInput, editing && creationCapabilities.priority && !creationAuto);
      setFieldEnabled(creationTipInput, editing && creationCapabilities.tip && !creationAuto);
      setFieldEnabled(creationMaxFeeInput, editing && (creationCapabilities.priority || creationCapabilities.tip) && creationAuto);

      setFieldEnabled(buyPriorityFeeInput, editing && buyCapabilities.priority && !buyAuto);
      setFieldEnabled(buyTipInput, editing && buyCapabilities.tip && !buyAuto);
      setFieldEnabled(buyMaxFeeInput, editing && (buyCapabilities.priority || buyCapabilities.tip) && buyAuto);

      setFieldEnabled(sellPriorityFeeInput, editing && sellCapabilities.priority && !sellAuto);
      setFieldEnabled(sellTipInput, editing && sellCapabilities.tip && !sellAuto);
      setFieldEnabled(sellMaxFeeInput, editing && (sellCapabilities.priority || sellCapabilities.tip) && sellAuto);
      syncAutoFeeButtons();
    }

    function syncSettingsCapabilities() {
      const editing = isPresetEditing(getConfig());
      const creationProvider = getProvider();
      const buyProvider = getBuyProvider();
      const sellProvider = getSellProvider();
      const creationCapabilities = getRouteCapabilities(creationProvider, "creation");
      const buyCapabilities = getRouteCapabilities(buyProvider, "buy");
      const sellCapabilities = getRouteCapabilities(sellProvider, "sell");

      if (providerSelect) providerSelect.disabled = !editing;
      if (buyProviderSelect) buyProviderSelect.disabled = !editing;
      if (sellProviderSelect) sellProviderSelect.disabled = !editing;
      setFieldVisibility(creationTipInput, creationCapabilities.tip);
      setFieldVisibility(creationPriorityInput, creationCapabilities.priority);
      setFieldVisibility(creationMevModeSelect, isHelloMoonProvider(creationProvider));
      setFieldVisibility(buyPriorityFeeInput, buyCapabilities.priority);
      setFieldVisibility(buyTipInput, buyCapabilities.tip);
      setFieldVisibility(buySlippageInput, buyCapabilities.slippage);
      setFieldVisibility(buyMevModeSelect, isHelloMoonProvider(buyProvider));
      setFieldVisibility(sellPriorityFeeInput, sellCapabilities.priority);
      setFieldVisibility(sellTipInput, sellCapabilities.tip);
      setFieldVisibility(sellSlippageInput, sellCapabilities.slippage);
      setFieldVisibility(sellMevModeSelect, isHelloMoonProvider(sellProvider));
      setMevModeOptionAvailability(creationMevModeSelect, creationProvider);
      setMevModeOptionAvailability(buyMevModeSelect, buyProvider);
      setMevModeOptionAvailability(sellMevModeSelect, sellProvider);
      setFieldEnabled(creationAutoFeeInput, editing && (creationCapabilities.priority || creationCapabilities.tip));
      setFieldEnabled(buyAutoFeeInput, editing && (buyCapabilities.priority || buyCapabilities.tip));
      setFieldEnabled(sellAutoFeeInput, editing && (sellCapabilities.priority || sellCapabilities.tip));
      setFieldEnabled(creationMevModeSelect, editing && isHelloMoonProvider(creationProvider));
      setFieldEnabled(buyMevModeSelect, editing && isHelloMoonProvider(buyProvider));
      setFieldEnabled(sellMevModeSelect, editing && isHelloMoonProvider(sellProvider));
      setFieldEnabled(buySlippageInput, editing && buyCapabilities.slippage);
      setFieldEnabled(sellSlippageInput, editing && sellCapabilities.slippage);
      syncProviderPlaceholders();
      syncProviderFeeValues();
      syncAutoFeeControls();
      syncStandardRpcWarnings();
      syncHelloMoonMevWarnings();
    }

    function renderBackendRegionSummary(regionRouting = getLatestWalletStatus() && getLatestWalletStatus().regionRouting) {
      if (!settingsBackendRegionSummary) return;
      if (!regionRouting || typeof regionRouting !== "object") {
        if (renderUtils.setCachedHTML) {
          renderUtils.setCachedHTML(
            renderCache,
            "backendRegion",
            settingsBackendRegionSummary,
            '<div class="settings-section-copy">Loading backend routing defaults...</div>',
          );
        } else {
          settingsBackendRegionSummary.innerHTML = '<div class="settings-section-copy">Loading backend routing defaults...</div>';
        }
        return;
      }
      const shared = regionRouting && regionRouting.shared ? regionRouting.shared : {};
      const providers = regionRouting && regionRouting.providers ? regionRouting.providers : {};
      const latestRuntimeStatus = getLatestRuntimeStatus();
      const warm = latestRuntimeStatus && latestRuntimeStatus.warm && typeof latestRuntimeStatus.warm === "object"
        ? latestRuntimeStatus.warm
        : null;
      const sharedConfigured = formatBackendRegionValue(shared.configured, "None");
      const sharedEffective = formatBackendRegionValue(shared.effective);
      const providerRows = ["helius-sender", "jito-bundle"].map((provider) => {
        const entry = providers[provider] || {};
        const configured = formatBackendRegionValue(entry.configured, "None");
        const effective = formatBackendRegionValue(entry.effective);
        const metaText = entry.endpointOverrideActive
          ? `Override: ${configured} | endpoint pinned`
          : `Override: ${configured}`;
        return `
      <div class="settings-region-card">
        <div class="settings-region-card-head">
          <strong>${escapeHTML(providerLabels[provider] || provider)}</strong>
          <span class="settings-region-effective">${escapeHTML(effective)}</span>
        </div>
        <div class="settings-region-meta">${escapeHTML(metaText)}</div>
      </div>
    `;
      }).join("");
      const warmCard = warm ? `
      <div class="settings-region-card settings-region-card-shared">
        <div class="settings-region-card-head">
          <strong>Warm</strong>
          <span class="settings-region-effective">${escapeHTML(warm.active ? "active" : "suspended")}</span>
        </div>
        <div class="settings-region-meta">${escapeHTML(String(warm.reason || "--"))}</div>
        <div class="settings-region-meta">Providers: ${escapeHTML(formatWarmProviders(warm.selectedProviders))}</div>
        <div class="settings-region-meta">Last active: ${escapeHTML(formatWarmTimestamp(warm.lastActivityAtMs))}</div>
      </div>
    ` : "";
      const markup = `
    <div class="settings-region-row">
      <div class="settings-region-card settings-region-card-shared">
        <div class="settings-region-card-head">
          <strong>Shared</strong>
          <span class="settings-region-effective">${escapeHTML(sharedEffective)}</span>
        </div>
        <div class="settings-region-meta">Configured: ${escapeHTML(sharedConfigured)}</div>
      </div>
      ${providerRows}
      ${warmCard}
    </div>
    <div class="settings-sidebar-note">
      Region defaults are recommended because provider fanout usually reaches more nearby supported endpoints and lands faster and more reliably than pinning one endpoint. Change backend env values, then run <code>npm restart</code>.
    </div>
  `;
      if (renderUtils.setCachedHTML) {
        renderUtils.setCachedHTML(renderCache, "backendRegion", settingsBackendRegionSummary, markup);
      } else {
        settingsBackendRegionSummary.innerHTML = markup;
      }
    }

    function renderPresetChipMarkup(configValue = getConfig(), { topBar = false } = {}) {
      const activePresetId = getActivePresetId(configValue);
      return getPresetItems(configValue).map((preset, index) => `
    <button
      type="button"
      class="preset-chip${preset.id === activePresetId ? " active" : ""}${topBar ? " compact" : ""}"
      data-preset-id="${escapeHTML(preset.id)}"
    >
      ${escapeHTML(topBar ? getPresetDisplayLabel(preset, index) : `Preset ${index + 1}`)}
    </button>
  `).join("");
    }

    function renderPresetChips() {
      const configValue = getConfig();
      const topMarkup = renderPresetChipMarkup(configValue, { topBar: true });
      const settingsMarkup = renderPresetChipMarkup(configValue, { topBar: false });
      if (topPresetChipBar && topMarkup !== lastTopPresetMarkup) {
        topPresetChipBar.innerHTML = topMarkup;
        lastTopPresetMarkup = topMarkup;
      }
      if (settingsPresetChipBar && settingsMarkup !== lastSettingsPresetMarkup) {
        settingsPresetChipBar.innerHTML = settingsMarkup;
        lastSettingsPresetMarkup = settingsMarkup;
      }
      if (presetEditToggle) {
        const editing = isPresetEditing(configValue);
        presetEditToggle.classList.toggle("active", editing);
        presetEditToggle.setAttribute("aria-pressed", editing ? "true" : "false");
        presetEditToggle.innerHTML = editing ? "Lock" : "&#9998;";
        presetEditToggle.title = editing ? "Lock active preset" : "Unlock active preset for editing";
      }
    }

    function getQuickDevBuyPresetAmounts(configValue = getLatestWalletStatus() && getLatestWalletStatus().config) {
      const globalAmounts = configValue && configValue.defaults && Array.isArray(configValue.defaults.quickDevBuyAmounts)
        ? configValue.defaults.quickDevBuyAmounts
        : [];
      const presetItems = configValue && configValue.presets && Array.isArray(configValue.presets.items)
        ? configValue.presets.items
        : [];
      return defaultQuickDevBuyAmounts.map((fallback, index) => {
        const globalValue = typeof globalAmounts[index] === "string"
          ? globalAmounts[index].trim()
          : "";
        if (globalValue) return globalValue;
        const preset = presetItems[index];
        const value = preset && preset.creationSettings && typeof preset.creationSettings.devBuySol === "string"
          ? preset.creationSettings.devBuySol.trim()
          : "";
        return value || fallback;
      });
    }

    function renderQuickDevBuyButtons(configValue = getLatestWalletStatus() && getLatestWalletStatus().config) {
      if (!devBuyQuickButtons) return;
      const amounts = getQuickDevBuyPresetAmounts(configValue);
      const markup = amounts.map((amount, index) => {
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
              placeholder="${escapeHTML(defaultQuickDevBuyAmounts[index])}"
            >
          </span>
        </label>
      `;
        }
        return `
      <button type="button" class="dev-buy-quick-button" data-quick-buy-index="${index}" data-quick-buy-amount="${escapeHTML(amount)}">
        <span class="dev-buy-quick-content">
          <img src="/images/solana-mark.png" alt="SOL" class="sol-logo inline-sol-logo quick-buy-sol-logo">
          <strong class="dev-buy-quick-value">${escapeHTML(amount)}</strong>
        </span>
      </button>
    `;
      }).join("");
      if (markup === lastQuickDevBuyMarkup) return;
      devBuyQuickButtons.innerHTML = markup;
      lastQuickDevBuyMarkup = markup;
    }

    function namePresetTickerMode(preset) {
      if (preset && preset.tickerAbbreviate) return "abbreviate";
      if (preset && preset.tickerUseFirstWord) return "first-word";
      return "current";
    }

    function renderNamePresetEditor(configValue = getConfig()) {
      if (!namePresetEditorList) return;
      const presets = getNamePresetButtons(configValue);
      const markup = presets.map((preset, index) => `
        <div class="name-preset-editor-row" data-name-preset-index="${index}" data-name-preset-id="${escapeHTML(preset.id)}">
          <div class="name-preset-editor-card-main">
            <strong>${escapeHTML(preset.name)}</strong>
            <span>${escapeHTML(formatNamePresetSummary(preset))}</span>
          </div>
          <div class="name-preset-editor-card-actions">
            <button type="button" class="name-preset-icon-button" data-name-preset-edit="${index}" aria-label="Edit ${escapeHTML(preset.name)}">&#9998;</button>
            <button type="button" class="name-preset-icon-button danger" data-name-preset-remove="${index}" aria-label="Remove ${escapeHTML(preset.name)}">&#128465;</button>
          </div>
        </div>
      `).join("");
      if (markup === lastNamePresetEditorMarkup) return;
      namePresetEditorList.innerHTML = markup;
      lastNamePresetEditorMarkup = markup;
    }

    function formatNamePresetSummary(preset) {
      const nameParts = [
        preset.namePrefix ? `"${preset.namePrefix}" + ` : "",
        "name",
        preset.nameSuffix ? ` + "${preset.nameSuffix}"` : "",
      ].join("");
      const tickerSource = preset.tickerAbbreviate
        ? "abbrev"
        : preset.tickerUseFirstWord ? "auto ticker" : "ticker";
      const tickerParts = [
        preset.tickerPrefix ? `"${preset.tickerPrefix}" + ` : "",
        tickerSource,
        preset.tickerSuffix ? ` + "${preset.tickerSuffix}"` : "",
      ].join("");
      return `${nameParts} / ${tickerParts}`;
    }

    function setNamePresetButtonsLocal(presets) {
      const configValue = cloneConfig(getConfig()) || createFallbackConfig();
      configValue.defaults = configValue.defaults || {};
      configValue.defaults.namePresetButtons = normalizeNamePresetButtons(presets);
      setConfig(configValue);
      renderNamePresetEditor(configValue);
      return configValue.defaults.namePresetButtons;
    }

    function setNamePresetFormActionsDisabled(disabled) {
      [namePresetAddButton, namePresetCancelEditButton, namePresetUpdateButton].forEach((button) => {
        if (button) button.disabled = Boolean(disabled);
      });
    }

    async function persistNamePresetButtons(previousPresets) {
      if (namePresetPersistInFlight) return false;
      namePresetPersistInFlight = true;
      setNamePresetFormActionsDisabled(true);
      try {
        const nextConfig = cloneConfig(getConfig()) || createFallbackConfig();
        const nextNamePresetButtons = getNamePresetButtons(nextConfig);
        const response = await global.fetch("/api/settings", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ config: nextConfig }),
        });
        const payload = await response.json().catch(() => ({}));
        if (!response.ok || !payload.ok) {
          throw new Error((payload && payload.error) || "Failed to save name presets.");
        }
        const savedConfig = cloneConfig(payload.config || nextConfig) || nextConfig;
        savedConfig.defaults = savedConfig.defaults || {};
        savedConfig.defaults.namePresetButtons = nextNamePresetButtons;
        setRegionRouting(payload.regionRouting || (getLatestWalletStatus() && getLatestWalletStatus().regionRouting));
        setConfig(savedConfig);
        renderNamePresetEditor(savedConfig);
        if (payload.regionRouting) renderBackendRegionSummary(payload.regionRouting);
        return true;
      } catch (error) {
        void previousPresets;
        setNamePresetError(error && error.message ? error.message : "Preset kept locally, but save failed.");
        return false;
      } finally {
        namePresetPersistInFlight = false;
        setNamePresetFormActionsDisabled(false);
      }
    }

    function setNamePresetFormMode(mode) {
      const editing = mode === "edit";
      if (namePresetFormActions) namePresetFormActions.dataset.namePresetMode = editing ? "edit" : "add";
      if (namePresetFormTitle) namePresetFormTitle.textContent = editing ? "Edit Preset" : "Add New Preset";
    }

    function clearNamePresetForm() {
      namePresetEditingIndex = -1;
      [
        namePresetNewName,
        namePresetNewNamePrefix,
        namePresetNewNameSuffix,
        namePresetNewTickerPrefix,
        namePresetNewTickerSuffix,
      ].forEach((input) => {
        if (input) input.value = "";
      });
      if (namePresetNewFirstWord) namePresetNewFirstWord.checked = true;
      if (namePresetNewAbbreviate) namePresetNewAbbreviate.checked = false;
      setNamePresetError("");
      setNamePresetFormMode("add");
    }

    function setNamePresetError(message = "") {
      if (!namePresetError) return;
      namePresetError.textContent = message;
      namePresetError.hidden = !message;
    }

    function readNamePresetForm() {
      const tickerAbbreviate = Boolean(namePresetNewAbbreviate && namePresetNewAbbreviate.checked);
      const presets = getNamePresetButtons();
      const existingId = namePresetEditingIndex >= 0 && presets[namePresetEditingIndex]
        ? presets[namePresetEditingIndex].id
        : "";
      return {
        id: existingId || `custom-${Date.now().toString(36)}`,
        name: namePresetNewName ? namePresetNewName.value : "",
        namePrefix: namePresetNewNamePrefix ? namePresetNewNamePrefix.value : "",
        nameSuffix: namePresetNewNameSuffix ? namePresetNewNameSuffix.value : "",
        tickerPrefix: namePresetNewTickerPrefix ? namePresetNewTickerPrefix.value : "",
        tickerSuffix: namePresetNewTickerSuffix ? namePresetNewTickerSuffix.value : "",
        tickerUseFirstWord: tickerAbbreviate ? false : Boolean(namePresetNewFirstWord && namePresetNewFirstWord.checked),
        tickerAbbreviate,
      };
    }

    function populateNamePresetForm(index) {
      const presets = getNamePresetButtons();
      const preset = presets[index];
      if (!preset) return;
      namePresetEditingIndex = index;
      if (namePresetNewName) namePresetNewName.value = preset.name || "";
      if (namePresetNewNamePrefix) namePresetNewNamePrefix.value = preset.namePrefix || "";
      if (namePresetNewNameSuffix) namePresetNewNameSuffix.value = preset.nameSuffix || "";
      if (namePresetNewTickerPrefix) namePresetNewTickerPrefix.value = preset.tickerPrefix || "";
      if (namePresetNewTickerSuffix) namePresetNewTickerSuffix.value = preset.tickerSuffix || "";
      if (namePresetNewFirstWord) namePresetNewFirstWord.checked = Boolean(preset.tickerUseFirstWord);
      if (namePresetNewAbbreviate) namePresetNewAbbreviate.checked = Boolean(preset.tickerAbbreviate);
      setNamePresetError("");
      setNamePresetFormMode("edit");
      if (namePresetNewName) {
        try { namePresetNewName.focus(); } catch (_) { /* noop */ }
      }
    }

    function cancelNamePresetEdit() {
      clearNamePresetForm();
    }

    async function addNamePresetEditorRow() {
      if (namePresetPersistInFlight) return;
      const previousPresets = getNamePresetButtons();
      const formPreset = readNamePresetForm();
      if (!String(formPreset.name || "").trim()) {
        setNamePresetError("Button name is required.");
        return;
      }
      const targetIndex = namePresetEditingIndex >= 0 ? namePresetEditingIndex : previousPresets.length;
      const normalized = normalizeNamePresetButton(formPreset, targetIndex);
      const nextPresets = previousPresets.slice();
      if (namePresetEditingIndex >= 0) {
        nextPresets[namePresetEditingIndex] = normalized;
      } else {
        nextPresets.push(normalized);
      }
      setNamePresetButtonsLocal(nextPresets);
      const persisted = await persistNamePresetButtons(previousPresets);
      if (persisted) clearNamePresetForm();
    }

    async function removeNamePresetEditorRow(index) {
      if (namePresetPersistInFlight) return;
      const previousPresets = getNamePresetButtons();
      const nextPresets = previousPresets.slice();
      nextPresets.splice(index, 1);
      setNamePresetButtonsLocal(nextPresets);
      const persisted = await persistNamePresetButtons(previousPresets);
      if (!persisted) return;
      if (namePresetEditingIndex === index) {
        clearNamePresetForm();
      } else if (namePresetEditingIndex > index) {
        namePresetEditingIndex -= 1;
      }
    }

    function showNamePresetModal() {
      clearNamePresetForm();
      renderNamePresetEditor();
      if (namePresetModal) namePresetModal.hidden = false;
    }

    function hideNamePresetModal() {
      if (!namePresetModal) return false;
      namePresetModal.hidden = true;
      clearNamePresetForm();
      return true;
    }

    function getDevBuyPresetEditorInputs() {
      return devBuyQuickButtons
        ? Array.from(devBuyQuickButtons.querySelectorAll("[data-dev-buy-preset-input]"))
        : [];
    }

    function populateDevBuyPresetEditor(configValue = getLatestWalletStatus() && getLatestWalletStatus().config) {
      const amounts = getQuickDevBuyPresetAmounts(configValue);
      getDevBuyPresetEditorInputs().forEach((input, index) => {
        if (input) input.value = amounts[index] || "";
      });
    }

    function isDevBuyPresetEditorOpen() {
      return devBuyPresetEditorOpen;
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
      const configValue = cloneConfig(getConfig()) || createFallbackConfig();
      const editorInputs = getDevBuyPresetEditorInputs();
      configValue.defaults = configValue.defaults || {};
      configValue.defaults.quickDevBuyAmounts = defaultQuickDevBuyAmounts.map((fallback, index) => {
        const input = editorInputs[index];
        return input ? String(input.value || "").trim() : fallback;
      });
      return configValue;
    }

    async function saveDevBuyPresetEditor() {
      const nextConfig = buildConfigWithUpdatedDevBuyPresets();
      if (saveDevBuyPresetsButton) saveDevBuyPresetsButton.disabled = true;
      if (cancelDevBuyPresetsButton) cancelDevBuyPresetsButton.disabled = true;
      if (changeDevBuyPresetsButton) changeDevBuyPresetsButton.disabled = true;
      try {
        const response = await global.fetch("/api/settings", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ config: nextConfig }),
        });
        const payload = await response.json();
        if (!response.ok || !payload.ok) {
          throw new Error(payload.error || "Failed to save quick deploy presets.");
        }
        const savedConfig = cloneConfig(payload.config || nextConfig) || nextConfig;
        setRegionRouting(payload.regionRouting || (getLatestWalletStatus() && getLatestWalletStatus().regionRouting));
        setConfig(savedConfig);
        renderQuickDevBuyButtons(savedConfig);
        populateDevBuyPresetEditor(savedConfig);
        renderBackendRegionSummary(payload.regionRouting);
        setDevBuyPresetEditorOpen(false);
      } catch (error) {
        setStatusLabel("Error");
        if (output) output.textContent = error.message;
      } finally {
        if (saveDevBuyPresetsButton) saveDevBuyPresetsButton.disabled = false;
        if (cancelDevBuyPresetsButton) cancelDevBuyPresetsButton.disabled = false;
        if (changeDevBuyPresetsButton) changeDevBuyPresetsButton.disabled = false;
      }
    }

    function applyPresetToSettingsInputs(preset, options = {}) {
      if (!preset) return;
      const { syncToMainForm = true } = options;
      const creationSettings = preset.creationSettings || {};
      const buySettings = preset.buySettings || {};
      const sellSettings = preset.sellSettings || {};
      syncingPresetInputs = true;
      if (providerSelect) providerSelect.value = creationSettings.provider || "helius-sender";
      if (creationTipInput) creationTipInput.value = creationSettings.tipSol || "";
      if (creationPriorityInput) creationPriorityInput.value = creationSettings.priorityFeeSol || "";
      setMevModeSelectValue(
        creationMevModeSelect,
        creationSettings.mevMode,
        defaultMevModeForProvider(creationSettings.provider),
        creationSettings.provider,
      );
      if (creationAutoFeeInput) creationAutoFeeInput.checked = Boolean(creationSettings.autoFee);
      if (creationMaxFeeInput) creationMaxFeeInput.value = creationSettings.maxFeeSol || "";
      if (buyProviderSelect) buyProviderSelect.value = buySettings.provider || "helius-sender";
      if (buyPriorityFeeInput) buyPriorityFeeInput.value = buySettings.priorityFeeSol || "";
      if (buyTipInput) buyTipInput.value = buySettings.tipSol || "";
      if (buySlippageInput) buySlippageInput.value = buySettings.slippagePercent || "";
      setMevModeSelectValue(
        buyMevModeSelect,
        buySettings.mevMode ?? buySettings.mevProtect,
        defaultMevModeForProvider(buySettings.provider),
        buySettings.provider,
      );
      if (buyAutoFeeInput) buyAutoFeeInput.checked = Boolean(buySettings.autoFee);
      if (buyMaxFeeInput) buyMaxFeeInput.value = buySettings.maxFeeSol || "";
      if (sellProviderSelect) sellProviderSelect.value = sellSettings.provider || "helius-sender";
      if (sellPriorityFeeInput) sellPriorityFeeInput.value = sellSettings.priorityFeeSol || "";
      if (sellTipInput) sellTipInput.value = sellSettings.tipSol || "";
      if (sellSlippageInput) sellSlippageInput.value = sellSettings.slippagePercent || "";
      setMevModeSelectValue(
        sellMevModeSelect,
        sellSettings.mevMode ?? sellSettings.mevProtect,
        defaultMevModeForProvider(sellSettings.provider),
        sellSettings.provider,
      );
      if (sellAutoFeeInput) sellAutoFeeInput.checked = Boolean(sellSettings.autoFee);
      if (sellMaxFeeInput) sellMaxFeeInput.value = sellSettings.maxFeeSol || "";
      syncingPresetInputs = false;
      const buyStandardizedDefaultsApplied =
        ensureStandardRpcSlippageDefault(buySlippageInput, getBuyProvider());
      const sellStandardizedDefaultsApplied =
        ensureStandardRpcSlippageDefault(sellSlippageInput, getSellProvider());
      const standardizedDefaultsApplied =
        buyStandardizedDefaultsApplied || sellStandardizedDefaultsApplied;

      if (syncToMainForm) {
        clearDevBuyState();
      }

      syncDevAutoSellUI();
      syncSettingsCapabilities();
      if (syncToMainForm || standardizedDefaultsApplied) {
        syncActivePresetFromInputs();
      }
      renderPresetChips();
      renderQuickDevBuyButtons(getConfig());
    }

    function syncActivePresetFromInputs() {
      if (syncingPresetInputs) return;
      const configValue = cloneConfig(getConfig());
      const activePreset = getActivePreset(configValue);
      if (!activePreset) return;
      syncProviderFeeValues();
      activePreset.creationSettings = {
        ...activePreset.creationSettings,
        provider: getProvider(),
        tipSol: creationTipInput ? creationTipInput.value.trim() : "",
        priorityFeeSol: creationPriorityInput ? creationPriorityInput.value.trim() : "",
        mevMode: normalizeMevMode(creationMevModeSelect ? creationMevModeSelect.value : "off"),
        autoFee: Boolean(creationAutoFeeInput && creationAutoFeeInput.checked),
        maxFeeSol: normalizeAutoFeeCapValue(creationMaxFeeInput ? creationMaxFeeInput.value : ""),
        devBuySol: activePreset.creationSettings && activePreset.creationSettings.devBuySol
          ? activePreset.creationSettings.devBuySol.trim()
          : "",
      };
      activePreset.buySettings = {
        ...activePreset.buySettings,
        provider: getBuyProvider(),
        priorityFeeSol: buyPriorityFeeInput ? buyPriorityFeeInput.value.trim() : "",
        tipSol: buyTipInput ? buyTipInput.value.trim() : "",
        slippagePercent: buySlippageInput ? buySlippageInput.value.trim() : "",
        mevMode: normalizeMevMode(buyMevModeSelect ? buyMevModeSelect.value : "off"),
        autoFee: Boolean(buyAutoFeeInput && buyAutoFeeInput.checked),
        maxFeeSol: normalizeAutoFeeCapValue(buyMaxFeeInput ? buyMaxFeeInput.value : ""),
      };
      activePreset.sellSettings = {
        ...activePreset.sellSettings,
        provider: getSellProvider(),
        priorityFeeSol: sellPriorityFeeInput ? sellPriorityFeeInput.value.trim() : "",
        tipSol: sellTipInput ? sellTipInput.value.trim() : "",
        slippagePercent: sellSlippageInput ? sellSlippageInput.value.trim() : "",
        mevMode: normalizeMevMode(sellMevModeSelect ? sellMevModeSelect.value : "off"),
        autoFee: Boolean(sellAutoFeeInput && sellAutoFeeInput.checked),
        maxFeeSol: normalizeAutoFeeCapValue(sellMaxFeeInput ? sellMaxFeeInput.value : ""),
      };
      setConfig(configValue);
      syncSettingsCapabilities();
    }

    function setActivePreset(presetId, options = {}) {
      const configValue = cloneConfig(getConfig());
      const exists = getPresetItems(configValue).some((entry) => entry.id === presetId);
      configValue.defaults = {
        ...(configValue.defaults || {}),
        activePresetId: exists ? presetId : defaultPresetId,
      };
      setConfig(configValue);
      applyPresetToSettingsInputs(getActivePreset(configValue), options);
      queueWarmActivity({ immediate: true });
    }

    function setPresetEditing(editing) {
      const configValue = cloneConfig(getConfig());
      configValue.defaults = {
        ...(configValue.defaults || {}),
        presetEditing: Boolean(editing),
      };
      setConfig(configValue);
      const inputs = [
        providerSelect,
        creationTipInput,
        creationPriorityInput,
        creationAutoFeeInput,
        creationMaxFeeInput,
        buyProviderSelect,
        buyPriorityFeeInput,
        buyTipInput,
        buySlippageInput,
        buyMevModeSelect,
        buyAutoFeeInput,
        buyMaxFeeInput,
        sellProviderSelect,
        sellPriorityFeeInput,
        sellTipInput,
        sellSlippageInput,
        sellMevModeSelect,
        sellAutoFeeInput,
        sellMaxFeeInput,
      ];
      inputs.forEach((input) => {
        if (!input) return;
        input.disabled = !editing;
      });
      syncSettingsCapabilities();
    }

    function setSettingsLoadingState(isLoading) {
      if (!settingsModal) return;
      settingsModal.classList.toggle("settings-loading", Boolean(isLoading));
      const controls = settingsModal.querySelectorAll("input, select, button");
      controls.forEach((control) => {
        if (control === settingsClose || control === settingsCancel) return;
        control.disabled = Boolean(isLoading);
      });
    }

    function validateSettingsModalBeforeSave() {
      const errors = validateAllInlineFields();
      if (!errors.length) return [];
      focusFirstInvalidInlineField();
      return errors;
    }

    function validateProviderFeeFields(scope) {
      const names = scope === "creation"
        ? ["creationPriorityFeeSol", "creationTipSol", "creationMaxFeeSol"]
        : scope === "buy"
          ? ["buyPriorityFeeSol", "buyTipSol", "buyMaxFeeSol"]
          : scope === "sell"
            ? ["sellPriorityFeeSol", "sellTipSol", "sellMaxFeeSol"]
            : [];
      names.forEach((name) => validateFieldByName(name));
    }

    function applyProviderAvailability(providers = {}) {
      [providerSelect, buyProviderSelect, sellProviderSelect].forEach((select) => {
        if (!select) return;
        Array.from(select.options).forEach((option) => {
          const entry = providers[option.value];
          option.disabled = Boolean(entry && !entry.available);
          option.textContent = providerLabels[option.value] || option.textContent.replace(/ \(unverified\)$/, "");
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

    function showSettingsModal() {
      if (!hasBootstrapConfig()) {
        setSettingsLoadingState(true);
        renderBackendRegionSummary(null);
        if (settingsModal) settingsModal.hidden = false;
        return;
      }
      setSettingsLoadingState(false);
      renderPresetChips();
      renderNamePresetEditor();
      renderNamePresetStrip();
      applyPresetToSettingsInputs(getActivePreset(getConfig()), { syncToMainForm: false });
      setPresetEditing(isPresetEditing(getConfig()));
      renderBackendRegionSummary();
      settingsModalInitialConfig = cloneConfig(getConfig());
      if (settingsModal) settingsModal.hidden = false;
    }

    function hideSettingsModal(reason = "dismiss") {
      if (!settingsModal) return false;
      if (reason === "cancel") {
        if (settingsModalInitialConfig) {
          const restoredConfig = cloneConfig(settingsModalInitialConfig);
          if (!restoredConfig.defaults) restoredConfig.defaults = {};
          restoredConfig.defaults.presetEditing = false;
          setConfig(restoredConfig);
          renderNamePresetEditor(restoredConfig);
          renderNamePresetStrip();
          applyPresetToSettingsInputs(getActivePreset(restoredConfig), { syncToMainForm: false });
          renderBackendRegionSummary();
        }
      } else if (reason === "save") {
        setPresetEditing(false);
      } else {
        return false;
      }
      settingsModal.hidden = true;
      settingsModalInitialConfig = null;
      return true;
    }

    return {
      applyPresetToSettingsInputs,
      applyProviderAvailability,
      buildConfigWithUpdatedDevBuyPresets,
      cloneConfig,
      createFallbackConfig,
      defaultMevModeForProvider,
      ensureStandardRpcSlippageDefault,
      getActivePreset,
      getActivePresetId,
      getBuyProvider,
      getConfig,
      getDefaultNamePresetButtons,
      getNamePresetButtons,
      getPresetDisplayLabel,
      getPresetItems,
      getProvider,
      getQuickDevBuyPresetAmounts,
      getRouteCapabilities,
      getSellProvider,
      hideSettingsModal,
      isDevBuyPresetEditorOpen,
      isHelloMoonProvider,
      isPresetEditing,
      isTrackSendBlockHeightEnabled,
      normalizeAutoFeeCapValue,
      normalizeMevMode,
      populateDevBuyPresetEditor,
      providerMinimumTipSol,
      providerRequiresPriorityFee,
      renderBackendRegionSummary,
      renderPresetChips,
      renderQuickDevBuyButtons,
      saveDevBuyPresetEditor,
      setConfig,
      setDevBuyPresetEditorOpen,
      setMevModeSelectValue,
      addNamePresetEditorRow,
      removeNamePresetEditorRow,
      renderNamePresetEditor,
      showNamePresetModal,
      hideNamePresetModal,
      populateNamePresetForm,
      cancelNamePresetEdit,
      setPresetEditing,
      setRegionRouting,
      setSettingsLoadingState,
      showSettingsModal,
      syncActivePresetFromInputs,
      syncSettingsCapabilities,
      setActivePreset,
      validateNonNegativeSolField,
      validateOptionalAutoFeeCapField,
      validateRequiredAutoFeeCapField,
      validateProviderFeeFields,
      validateRequiredPriorityFeeField,
      validateRequiredTipField,
      validateSettingsModalBeforeSave,
    };
  }

  global.LaunchDeckSettingsDomain = {
    create: createSettingsDomain,
  };
})(window);
