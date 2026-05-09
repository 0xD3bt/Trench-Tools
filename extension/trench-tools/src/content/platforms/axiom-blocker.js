(function installTrenchToolsAxiomBlocker() {
  // `axiom-override.js` is loaded directly into the page's main world at
  // document_start from manifest.json so it can wrap WebSocket before Axiom
  // creates/caches a constructor. This stub is retained for update continuity.
  const SITE_FEATURES_KEY = "trenchTools.siteFeatures";
  const BUTTON_MODE_KEY = "trenchToolsAxiomTokenDetailButtonMode";
  const COMPACT_KEY = "trenchToolsAxiomTokenDetailCompactButtons";
  const SIZE_KEY = "trenchToolsAxiomTokenDetailPanelSizes";
  const NATIVE_SIZE_KEY = "instantTradeModalSize";
  const STYLE_ID = "trench-tools-axiom-token-detail-initial-size";
  const DEFAULT_WIDTH = 312;
  const DEFAULT_HEIGHT = 372;

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
      if (buttonMode !== "axiom") {
        installPreloadCloneObserver();
      }
    } catch (_error) {}
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

  function installPreloadCloneObserver() {
    const mount = () => {
      const panel = document.querySelector("div#instant-trade");
      if (!panel) {
        return;
      }
      panel.querySelectorAll("div.flex-row.w-full").forEach((row) => {
        if (!(row instanceof HTMLElement)) {
          return;
        }
        if (
          row.querySelector(
            ":scope > [data-trench-tools-token-detail-inline], :scope > [data-trench-tools-token-detail-preload-inline]"
          )
        ) {
          return;
        }
        const controls = Array.from(row.children).filter((element) => {
          if (!(element instanceof HTMLElement) || !element.matches("div.rounded-full")) {
            return false;
          }
          if (String(element.className || "").includes("group/wallets")) {
            return false;
          }
          const amount = String(element.textContent || "").replace(/\s+/g, "").replace("%", "").trim();
          return amount && Number.isFinite(Number(amount));
        });
        if (controls.length < 2) {
          return;
        }
        row.append(...controls.map((control) => buildPreloadClone(control)));
      });
    };
    const observer = new MutationObserver(mount);
    observer.observe(document.documentElement, { childList: true, subtree: true });
    mount();
    window.setTimeout(() => observer.disconnect(), 10000);
  }

  function buildPreloadClone(control) {
    const clone = control.cloneNode(true);
    clone.classList.add("trench-tools-axiom-token-detail-bloom-clone");
    clone.setAttribute("data-trench-tools-token-detail-preload-inline", "true");
    Object.assign(clone.style, {
      minWidth: "40px",
      pointerEvents: "none",
      zIndex: "1000"
    });
    return clone;
  }
})();
