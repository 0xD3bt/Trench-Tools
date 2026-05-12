(function installTrenchToolsAxiomBlocker() {
  // `axiom-override.js` is loaded directly into the page's main world at
  // document_start from manifest.json so it can wrap WebSocket before Axiom
  // creates/caches a constructor. This stub is retained for update continuity.
  const SITE_FEATURES_KEY = "trenchTools.siteFeatures";
  const BUTTON_MODE_KEY = "trenchToolsAxiomTokenDetailButtonMode";
  const COMPACT_KEY = "trenchToolsAxiomTokenDetailCompactButtons";
  const SIZE_KEY = "trenchToolsAxiomTokenDetailPanelSizes";
  const NATIVE_SIZE_KEY = "instantTradeModalSize";
  const POSITION_KEY = "trenchToolsAxiomInstantTradeCompactPosition";
  const STYLE_ID = "trench-tools-axiom-token-detail-initial-size";
  const DEFAULT_WIDTH = 312;
  const DEFAULT_HEIGHT = 372;
  const INITIAL_POSITION_TIMEOUT_MS = 8000;
  let initialPositionObserver = null;
  let initialPositionObserverInstalled = false;

  removePreloadClones();

  if (/pulse/i.test(window.location.href) || document.getElementById(STYLE_ID)) {
    return;
  }

  function readJson(key) {
    try {
      return JSON.parse(window.localStorage?.getItem(key) || "{}");
    } catch (_error) {
      return {};
    }
  }

  function normalizeSize(value, fallback, options = {}) {
    const width = Number(value?.width);
    const height = Number(value?.height);
    const minWidth = Number.isFinite(options.minWidth) ? options.minWidth : 220;
    return {
      width: Number.isFinite(width) && width >= minWidth ? Math.round(width) : fallback.width,
      height: Number.isFinite(height) && height >= 180 ? Math.round(height) : fallback.height
    };
  }

  function parseMetric(value) {
    if (typeof value === "number") {
      return value;
    }
    if (typeof value === "string") {
      return Number.parseFloat(value);
    }
    return Number.NaN;
  }

  function isMeaningfulSize(size, fallback) {
    return Math.abs(Number(size?.width) - Number(fallback?.width)) > 8 ||
      Math.abs(Number(size?.height) - Number(fallback?.height)) > 8;
  }

  function buttonModeCount(siteFeatures) {
    const count = Number(siteFeatures?.axiom?.instantTradeButtonModeCount);
    return count === 1 || count === 2 || count === 3 ? count : 3;
  }

  function constrainButtonMode(mode, siteFeatures) {
    const normalizedMode = mode === "axiom" || mode === "trench" || mode === "dual" ? mode : "dual";
    const count = buttonModeCount(siteFeatures);
    if (count === 1) {
      return "trench";
    }
    if (count === 2 && normalizedMode === "axiom") {
      return "dual";
    }
    return normalizedMode;
  }

  function readButtonMode(siteFeatures) {
    try {
      const mode = String(window.localStorage?.getItem(BUTTON_MODE_KEY) || "").trim().toLowerCase();
      if (mode === "axiom" || mode === "trench" || mode === "dual") {
        return constrainButtonMode(mode, siteFeatures);
      }
      return constrainButtonMode(window.localStorage?.getItem(COMPACT_KEY) === "true" ? "trench" : "dual", siteFeatures);
    } catch (_error) {
      return constrainButtonMode("dual", siteFeatures);
    }
  }

  refreshInstantTradeEnabledPreference();

  function installInitialPanelStyle(siteFeatures) {
    if (document.getElementById(STYLE_ID)) {
      return;
    }
    try {
      const buttonMode = readButtonMode(siteFeatures);
      const singleButtonMode = buttonMode === "axiom" || buttonMode === "trench";
      const storedSizes = readJson(SIZE_KEY);
      const defaults = {
        compact: { width: DEFAULT_WIDTH, height: DEFAULT_HEIGHT },
        expanded: { width: DEFAULT_WIDTH * 2, height: DEFAULT_HEIGHT }
      };
      const mode = singleButtonMode ? "compact" : "expanded";
      const size = resolvePanelSize(mode, storedSizes, defaults);
      const style = document.createElement("style");
      style.id = STYLE_ID;
      const trenchOnlyRules = buttonMode === "trench"
        ? `
        div#instant-trade .buy-click-container div.flex-row.w-full:has(> [data-trench-tools-token-detail-inline], > [data-trench-tools-token-detail-preload-inline]) > div.rounded-full:not([data-trench-tools-token-detail-inline]):not([data-trench-tools-token-detail-preload-inline]) {
          display: none !important;
        }
        div#instant-trade .buy-click-container > div:first-child > div:first-child > div:first-child > div:nth-child(2) {
          display: none !important;
        }
      `
        : "";
      const axiomOnlyRules = buttonMode === "axiom"
        ? `
        div#instant-trade .buy-click-container div.flex-row.w-full > [data-trench-tools-token-detail-inline],
        div#instant-trade .buy-click-container div.flex-row.w-full > [data-trench-tools-token-detail-preload-inline] {
          display: none !important;
        }
      `
        : "";
      style.textContent = `
        div:has(> div:has(> div#instant-trade)),
        div:has(> div#instant-trade) {
          width: ${size.width}px !important;
          min-width: ${size.width}px !important;
          max-width: ${size.width}px !important;
        }
        div#instant-trade {
          width: ${size.width}px !important;
          height: ${size.height}px !important;
        }
        div:has(> div#instant-trade) > div:not(#instant-trade) {
          width: ${size.width}px !important;
        }
        .trench-tools-axiom-token-detail-bloom-clone {
          border: 1px solid #EEA7ED !important;
          color: #EEA7ED !important;
          z-index: 1000;
        }
        ${trenchOnlyRules}
        ${axiomOnlyRules}
      `;
      (document.head || document.documentElement).appendChild(style);
      installInitialPositionObserver(size);
    } catch (_error) {}
  }

  function installInitialPositionObserver(size) {
    if (initialPositionObserverInstalled || !hasSavedCompactPosition()) {
      return;
    }
    initialPositionObserverInstalled = true;
    const tryApply = () => {
      const panel = document.querySelector("div#instant-trade");
      if (!(panel instanceof HTMLElement)) {
        return false;
      }
      return applyEarlyCompactTransform(panel, size);
    };
    if (tryApply()) {
      return;
    }
    try {
      initialPositionObserver = new MutationObserver(() => {
        if (tryApply()) {
          disconnectInitialPositionObserver();
        }
      });
      initialPositionObserver.observe(document.documentElement, { childList: true, subtree: true });
    } catch (_error) {
      disconnectInitialPositionObserver();
      return;
    }
    window.setTimeout(disconnectInitialPositionObserver, INITIAL_POSITION_TIMEOUT_MS);
  }

  function disconnectInitialPositionObserver() {
    if (initialPositionObserver) {
      try {
        initialPositionObserver.disconnect();
      } catch (_error) {}
      initialPositionObserver = null;
    }
  }

  function hasSavedCompactPosition() {
    try {
      const parsed = JSON.parse(window.localStorage?.getItem(POSITION_KEY) || "{}");
      const x = parseMetric(parsed?.x ?? parsed?.left);
      const y = parseMetric(parsed?.y ?? parsed?.top);
      return Number.isFinite(x) && Number.isFinite(y);
    } catch (_error) {
      return false;
    }
  }

  function applyEarlyCompactTransform(panel, size) {
    const container = findFixedDragContainer(panel);
    if (!(container instanceof HTMLElement)) {
      return false;
    }
    if (container.hasAttribute("data-trench-tools-token-detail-managed-transform")) {
      return true;
    }
    const position = readCompactPosition(size);
    if (!position) {
      return true;
    }
    try {
      container.setAttribute(
        "data-trench-tools-token-detail-original-transform",
        container.style.transform || ""
      );
      container.setAttribute(
        "data-trench-tools-token-detail-original-transform-priority",
        container.style.getPropertyPriority("transform") || ""
      );
      container.setAttribute(
        "data-trench-tools-token-detail-original-transition",
        container.style.transition || ""
      );
      container.setAttribute(
        "data-trench-tools-token-detail-original-transition-priority",
        container.style.getPropertyPriority("transition") || ""
      );
      container.setAttribute("data-trench-tools-token-detail-managed-transform", "true");
      container.style.setProperty("transition", "none", "important");
      container.style.setProperty(
        "transform",
        `translate(${position.x}px, ${position.y}px)`,
        "important"
      );
    } catch (_error) {}
    return true;
  }

  function findFixedDragContainer(panel) {
    const panelRect = panel.getBoundingClientRect();
    let firstFixed = null;
    let current = panel.parentElement;
    for (let depth = 0; current instanceof HTMLElement && depth < 6; depth += 1) {
      if (current === document.body || current === document.documentElement) {
        break;
      }
      const computed = window.getComputedStyle?.(current);
      if (computed?.position === "fixed") {
        firstFixed = firstFixed || current;
        const rect = current.getBoundingClientRect();
        if (
          Number.isFinite(rect.width) &&
          Number.isFinite(rect.height) &&
          rect.width > 0 &&
          rect.height > 0 &&
          rect.width < window.innerWidth - 8 &&
          rect.height < window.innerHeight - 8 &&
          rect.width >= panelRect.width - 8 &&
          rect.width <= panelRect.width + 180 &&
          rect.height >= panelRect.height - 8 &&
          rect.height <= panelRect.height + 220
        ) {
          return current;
        }
      }
      current = current.parentElement;
    }
    return firstFixed;
  }

  function readCompactPosition(size) {
    try {
      const parsed = JSON.parse(window.localStorage?.getItem(POSITION_KEY) || "{}");
      const rawX = parseMetric(parsed?.x ?? parsed?.left);
      const rawY = parseMetric(parsed?.y ?? parsed?.top);
      if (!Number.isFinite(rawX) || !Number.isFinite(rawY)) {
        return null;
      }
      const width = Number(size?.width);
      const height = Number(size?.height);
      const maxX = Number.isFinite(width) && width > 0
        ? Math.max(0, window.innerWidth - width)
        : window.innerWidth;
      const maxY = Number.isFinite(height) && height > 0
        ? Math.max(0, window.innerHeight - height)
        : window.innerHeight;
      const edgeThresholdPx = 24;
      const edgeX = String(parsed?.edgeX || "").trim().toLowerCase();
      const offsetX = parseMetric(parsed?.offsetX);
      const resolvedX =
        edgeX === "right" && Number.isFinite(offsetX) && Number.isFinite(width) && width > 0
          ? window.innerWidth - width - (offsetX <= edgeThresholdPx ? 0 : offsetX)
          : edgeX === "left" && Number.isFinite(offsetX)
            ? (offsetX <= edgeThresholdPx ? 0 : offsetX)
            : Number.isFinite(width) && width > 0 && maxX - rawX <= edgeThresholdPx
              ? maxX
              : rawX <= edgeThresholdPx
                ? 0
                : rawX;
      return {
        x: Math.round(clampPosition(resolvedX, 0, maxX)),
        y: Math.round(clampPosition(rawY, 0, maxY))
      };
    } catch (_error) {
      return null;
    }
  }

  function clampPosition(value, min, max) {
    if (!Number.isFinite(value)) {
      return min;
    }
    return Math.min(Math.max(value, min), max);
  }

  function resolvePanelSize(mode, storedSizes, defaults) {
    const fallback = defaults[mode];
    const stored = normalizeSize(storedSizes?.[mode], fallback, {
      minWidth: mode === "expanded" ? DEFAULT_WIDTH : 220
    });
    if (isMeaningfulSize(stored, fallback)) {
      return stored;
    }
    if (mode !== "expanded") {
      return fallback;
    }
    const nativeSize = readNativeExpandedSize(fallback);
    return nativeSize || fallback;
  }

  function readNativeExpandedSize(fallback) {
    const parsed = readJson(NATIVE_SIZE_KEY);
    const width = parseMetric(parsed?.width);
    const height = parseMetric(parsed?.height);
    if (!Number.isFinite(width) || !Number.isFinite(height)) {
      return null;
    }
    const size = normalizeSize(
      {
        width,
        height: height >= fallback.height - 8 ? height : fallback.height
      },
      fallback,
      { minWidth: nativeExpandedMinWidth() }
    );
    return isMeaningfulSize(size, fallback) ? size : null;
  }

  function nativeExpandedMinWidth() {
    return DEFAULT_WIDTH + 48;
  }

  function refreshInstantTradeEnabledPreference() {
    try {
      if (typeof chrome !== "undefined" && chrome?.storage?.local?.get) {
        chrome.storage.local.get(SITE_FEATURES_KEY, (stored) => {
          try {
            const storageError = chrome.runtime?.lastError;
            if (!storageError) {
              const enabled = normalizeStoredInstantTradeEnabled(stored?.[SITE_FEATURES_KEY]);
              if (enabled) {
                installInitialPanelStyle(stored?.[SITE_FEATURES_KEY]);
              } else {
                removeInitialPanelStyle();
                removePreloadClones();
              }
            }
          } catch (_error) {}
        });
      }
    } catch (_error) {}
  }

  function normalizeStoredInstantTradeEnabled(siteFeatures) {
    const axiom = siteFeatures?.axiom || {};
    if (axiom.enabled === false) {
      return false;
    }
    return Boolean(axiom.instantTrade ?? axiom.tokenDetailButton ?? true);
  }

  function removeInitialPanelStyle() {
    document.getElementById(STYLE_ID)?.remove();
  }

  function removePreloadClones() {
    document.querySelectorAll("[data-trench-tools-token-detail-preload-inline]").forEach((element) => {
      element.remove();
    });
  }

})();
