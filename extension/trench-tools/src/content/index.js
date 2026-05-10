const trenchToolsContentModules = window.__trenchToolsContentModules || {};
const callBackground = trenchToolsContentModules.callBackground;
const createLaunchdeckShellController = trenchToolsContentModules.createLaunchdeckShellController;

(function trenchToolsContentScript() {
  if (window.__trenchToolsContentScriptInstance?.active) {
    return;
  }
  if (typeof callBackground !== "function" || typeof createLaunchdeckShellController !== "function") {
    console.error("Trench Tools content modules missing.");
    return;
  }

  const BASE58_REGEX = /\b[1-9A-HJ-NP-Za-km-z]{32,44}\b/g;
  const PREFERENCES_KEY = "trenchTools.panelPreferences";
  const PANEL_CHANNEL_OUT = "trench-tools-content";
  const PANEL_CHANNEL_IN = "trench-tools-panel";
  function buildPanelIframeUrl(mode = "persistent") {
    const url = new URL(chrome.runtime.getURL("src/panel/index.html"));
    url.searchParams.set("parentOrigin", window.location.origin);
    url.searchParams.set("mode", mode);
    return url.toString();
  }
  const PANEL_ORIGIN = new URL(buildPanelIframeUrl()).origin;
  const LOGO_URL = chrome.runtime.getURL("images/trench-tools-boot-logo.png");
  const INLINE_LOGO_URL = chrome.runtime.getURL("assets/TT-compact.png");
  const TOAST_LINK_ICON_URL = chrome.runtime.getURL("assets/link-icon.png");
  const TOAST_SUCCESS_ICON_URL = chrome.runtime.getURL("assets/confirmed-icon.png");
  const TOAST_FAIL_ICON_URL = chrome.runtime.getURL("assets/fail-icon.png");
  const SITE_FEATURES_KEY = "trenchTools.siteFeatures";
  const APPEARANCE_KEY = "trenchTools.appearance";
  const BOOTSTRAP_REVISION_KEY = "trenchTools.bootstrapRevision";
  const WALLET_STATUS_REVISION_KEY = "trenchTools.walletStatusRevision";
  const WALLET_STATUS_DIFF_KEY = "trenchTools.walletStatusDiff";
  const WALLET_STATUS_MARK_REVISION_KEY = "trenchTools.walletStatusMarkRevision";
  const WALLET_STATUS_MARK_DIFF_KEY = "trenchTools.walletStatusMarkDiff";
  const RUNTIME_DIAGNOSTICS_REVISION_KEY = "trenchTools.runtimeDiagnosticsRevision";
  const RUNTIME_DIAGNOSTICS_SNAPSHOT_KEY = "trenchTools.runtimeDiagnosticsSnapshot";
  const LAST_TRADE_EVENT_KEY = "trenchTools.lastTradeEvent";
  const BATCH_STATUS_EVENT_KEY = "trenchTools.lastBatchStatusEvent";
  const HOST_AUTH_TOKEN_KEY = "trenchTools.hostAuthToken";
  const LAUNCHDECK_HOST_KEY = "trenchTools.launchdeckHostBaseUrl";
  const BATCH_STATUS_STREAM_STALE_FALLBACK_MS = 2500;
  const BUY_SOUND_TEMPLATE_PATHS = {
    "notification-1": "assets/Notification-1.mp3",
    "notification-2": "assets/notification-2.mp3",
    "notification-3": "assets/notification-3.mp3",
    "notification-4": "assets/notification-4.mp3",
    "notification-5": "assets/notification-5.mp3",
    "notification-6": "assets/notification-6.mp3"
  };
  const BUY_SOUND_CUSTOM_ID = "custom";
  const BUY_SOUND_PLAY_CACHE_LIMIT = 400;
  const QUICK_PANEL_DEFAULT_WIDTH = 375;
  const QUICK_PANEL_DEFAULT_HEIGHT = 453;
  const EXTENSION_RELOAD_TOAST_FALLBACK_DELAY_MS = 1800;
  const WALLET_STATUS_QUOTE_REFRESH_MS = 1250;
  const WALLET_STATUS_FAST_QUOTE_REFRESH_MS = 75;
  const TRADE_PRIME_HEARTBEAT_MS = 15000;
  const TRADE_PRIME_MIN_INTERVAL_MS = 4000;
  const EXTENSION_RELOAD_FRIENDLY_MESSAGE =
    "Extension connection lost. Refresh to reconnect.";
  const PANEL_Z_INDEX = {
    DEFAULT: "999999",
    FLYOUT: "1000000",
    LOWERED_FLYOUT: "51",
    LOWERED: "50"
  };
  const AXIOM_PANEL_LAYER_OVERLAY_SELECTOR = [
    '[class*="fixed"][class*="z-[9999]"]',
    ".fixed.z-\\[9999\\]",
    '[class*="z-[9999]"][style*="position: fixed"]',
    '[data-popper-placement]:not([style*="visibility: hidden"])',
    '[data-popper-reference-hidden]:not([style*="visibility: hidden"])',
    '[data-popper-escaped]:not([style*="visibility: hidden"])',
    '[class*="popper"]:not([style*="visibility: hidden"])',
    '[role="dialog"]:not([style*="visibility: hidden"])',
    '[role="alertdialog"]:not([style*="visibility: hidden"])',
    '[role="menu"]:not([style*="visibility: hidden"])',
    ".p-overlaypanel:not(.p-overlaypanel-hidden)"
  ].join(", ");
  const contentRuntime = window.TrenchToolsContentRuntime;
  if (!contentRuntime) {
    console.error("Trench Tools content runtime missing.");
    return;
  }

  const platform = detectPlatform();
  if (!platform) {
    return;
  }
  const readinessSurfaceId = `${platform}:${Date.now()}:${Math.random().toString(36).slice(2)}`;

  const state = {
    platform,
    bootstrap: null,
    walletStatus: null,
    hostError: "",
    siteFeatures: defaultSiteFeatures(),
    preferences: defaultPreferences(),
    tokenContext: null,
    panelTokenContext: null,
    quickPanelTokenContext: null,
    preview: null,
    batchStatus: null,
    batchStatuses: new Map(),
    batchStatusStreamRevisions: new Map(),
    activePanelBatchId: null,
    launchdeckShell: null,
    panelFrame: null,
    panelWrapper: null,
    launcherButton: null,
    panelPosition: null,
    panelReady: false,
    panelOpen: false,
    panelDimensions: null,
    panelScale: 1,
    panelNaturalHeight: 0,
    quickPanelWrapper: null,
    quickPanelFrame: null,
    quickPanelReady: false,
    quickPanelOpen: false,
    quickPanelAnchorRect: null,
    quickPanelAnchorElement: null,
    quickPanelCloseHandlers: null,
    quickPanelLifecycleCleanup: null,
    statusPollTimers: new Map(),
    activeToasts: new Map(),
    localExecutionPendings: new Map(),
    localExecutionPendingBatchIds: new Map(),
    batchToastIds: new Map(),
    runtimeDiagnosticToastKeys: new Set(),
    runtimeDiagnosticNotice: null,
    toastCleanupInterval: null,
    mutationObserver: null,
    panelLayerObservers: new Map(),
    pendingMintRequests: new Map(),
    prewarmSnapshots: new Map(),
    activeDrag: null,
    hostRevisionTimer: null,
    lastTradePrimeAt: 0,
    currentRouteUrl: window.location.href,
    routeReconcileTimer: null,
    routeReconcileDelays: new Map(),
    mountSweepTimers: new Map(),
    navigationHooksInstalled: false,
    tokenRequestSeq: 0,
    walletStatusRequestSeq: 0,
    previewRequestSeq: 0,
    walletStatusRefreshTimer: null,
    walletStatusQuoteRefreshTimer: null,
    tokenDistributionPending: "",
    extensionReloadToastShown: false,
    extensionReloadFallbackTimer: null,
    appearance: defaultAppearance(),
    buySoundPlayed: new Set()
  };

  let platformHelpers = null;
  let platformAdapter = null;
  const lifecycle = {
    destroyed: false,
    panelMessageListener: null,
    storageChangeListener: null,
    pagehideListener: null,
    popstateListener: null,
    hashchangeListener: null,
    visibilityChangeListener: null,
    focusListener: null,
    errorListener: null,
    unhandledRejectionListener: null,
    originalPushState: null,
    originalReplaceState: null
  };

  window.__trenchToolsContentScriptInstance = {
    active: true,
    teardown
  };
  window.__trenchToolsContentScriptActive = true;

  attachGlobalErrorRecoveryHandlers();
  initialize().catch((error) => {
    console.error("Trench Tools init failed", error);
    surfaceUserFacingError(error);
  });

  function tradeReadinessSurfaceId() {
    return readinessSurfaceId;
  }

  function setTradeReadiness(active, surface = platform) {
    callBackground("trench:set-trade-readiness", {
      surfaceId: tradeReadinessSurfaceId(),
      active: Boolean(active),
      surface
    }).catch(() => {});
  }

  function primeTradeRuntime(reason = "manual", options = {}) {
    if (lifecycle.destroyed || document.visibilityState === "hidden") {
      return;
    }
    const now = Date.now();
    if (!options.force && now - state.lastTradePrimeAt < TRADE_PRIME_MIN_INTERVAL_MS) {
      return;
    }
    state.lastTradePrimeAt = now;
    const startedAt = now;
    callBackground("trench:prime-trade-runtime", {
      surfaceId: tradeReadinessSurfaceId(),
      surface: platform,
      reason
    })
      .then((result) => {
        console.debug(
          "[trench][latency] phase=content-prime reason=%s roundtrip_ms=%s background_ready_ms=%s prime_ms=%s",
          reason,
          Date.now() - startedAt,
          result?.backgroundReadyMs ?? "",
          result?.primeMs ?? result?.backgroundPrimeMs ?? ""
        );
      })
      .catch(() => {});
  }

  function getPlatformHelpers() {
    if (platformHelpers) {
      return platformHelpers;
    }

    platformHelpers = {
      state,
      platform,
      extractMintFromUrl,
      extractMintFromText,
      extractMintFromSelectors,
      findElementShowingMint,
      injectInlineControls,
      buildInlineButton,
      buildInlineIconButton,
      setInlineButtonStyleSet,
      setInlineButtonLabel,
      removeInjectedControls,
      attachInlineSizeSync,
      teardownInlineSizeSync,
      resolveInlineToken,
      setInlineTokenContext,
      handleInlineTradeRequest,
      openInlinePanelForMint,
      prewarmForMint,
      handleTradeRequest,
      handleTokenDistributionRequest,
      resolveQuickBuyAmount,
      quickBuyLabel,
      openPanel,
      async openLaunchdeckOverlay(options = {}) {
        if (!(await ensureValidLaunchdeckPresetForExtension())) {
          return;
        }
        ensureLaunchdeckShell();
        state.launchdeckShell.openOverlay(options);
      },
      async openLaunchdeckPopout(options = {}) {
        if (!(await ensureValidLaunchdeckPresetForExtension())) {
          return;
        }
        ensureLaunchdeckShell();
        state.launchdeckShell.openPopout(options);
      },
      showToast,
      getQuickBuyBaseStyles: () => getQuickBuyBaseStylesForPlatform(platform)
    };

    return platformHelpers;
  }

  function getPlatformAdapter() {
    if (platformAdapter) {
      return platformAdapter;
    }

    platformAdapter = contentRuntime.createPlatformAdapter(platform, getPlatformHelpers());
    if (!platformAdapter) {
      throw new Error(`No platform adapter registered for ${platform}.`);
    }
    return platformAdapter;
  }

  async function initialize() {
    state.siteFeatures = await loadSiteFeatures();
    if (!isPlatformEnabled()) {
      return;
    }
    state.preferences = await loadPreferences();
    state.appearance = await loadAppearance();
    state.panelPosition = await loadPanelPosition();
    state.panelDimensions = await loadPanelDimensions();
    state.panelScale = await loadPanelScale();
    ensureLaunchdeckShell();
    await refreshBootstrap();
    if (!state.hostRevisionTimer) {
      state.hostRevisionTimer = window.setInterval(() => {
        primeTradeRuntime("heartbeat");
      }, TRADE_PRIME_HEARTBEAT_MS);
    }
    attachStorageChangeListener();
    attachPanelMessageListener();
    primeTradeRuntime("init", { force: true });
    void callBackground("trench:get-runtime-diagnostics")
      .then(surfaceRuntimeDiagnostics)
      .catch(() => {});
    installNavigationHooks();
    mountPlatformObserver();
    scheduleMountSweeps("init");
    setTradeReadiness(true, platform);
    // Clear this surface's active mint when the page is being unloaded
    // so the host stops subscribing to the ATA for a tab the user has
    // closed. `pagehide` is more reliable than `beforeunload` in MV3
    // content scripts (fires on bfcache navigations too).
    lifecycle.pagehideListener = () => {
      setTradeReadiness(false, platform);
      clearActiveMarkForSurface();
      clearActiveMintForSurface();
    };
    window.addEventListener("pagehide", lifecycle.pagehideListener, { once: true });
    await reconcileSurfaceState("init");
    scheduleRouteReconcile("init-delayed-fast", 250);
    scheduleRouteReconcile("init-delayed-slow", 1000);
  }

  function ensureLaunchdeckShell() {
    if (!state.launchdeckShell) {
      state.launchdeckShell = createLaunchdeckShellController({
        onPostDeploySuccess: handleLaunchdeckPostDeploySuccess
      });
    }
    return state.launchdeckShell;
  }

  function normalizePostDeployDestination(value, fallback = "axiom") {
    const destination = String(value || "").trim().toLowerCase();
    return destination === "axiom" ? destination : fallback;
  }

  function normalizePostDeployAddress(value) {
    const text = String(value || "").trim();
    return /^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(text) ? text : "";
  }

  function readPostDeployNestedString(value, path) {
    let current = value;
    for (const key of path) {
      if (!current || typeof current !== "object") return "";
      current = current[key];
    }
    return typeof current === "string" ? current : "";
  }

  function resolveLaunchdeckPostDeployRoute(destination, report) {
    if (normalizePostDeployDestination(destination) !== "axiom" || !report || typeof report !== "object") {
      return "";
    }
    const candidates = [
      report.pair,
      report.pairAddress,
      report.routeAddress,
      report.poolAddress,
      report.poolId,
      report.launchpadPoolAddress,
      report.bondingCurve,
      report.bondingCurveAddress,
      report.marketKey,
      report.marketAddress,
      report.preMigrationDbcPoolAddress,
      report.postMigrationDammPoolAddress,
      readPostDeployNestedString(report, ["launch", "pairAddress"]),
      readPostDeployNestedString(report, ["launch", "routeAddress"]),
      readPostDeployNestedString(report, ["launch", "poolAddress"]),
      readPostDeployNestedString(report, ["bagsLaunch", "preMigrationDbcPoolAddress"]),
      readPostDeployNestedString(report, ["bagsLaunch", "postMigrationDammPoolAddress"])
    ];
    for (const candidate of candidates) {
      const route = normalizePostDeployAddress(candidate);
      if (route) return route;
    }
    return "";
  }

  function buildLaunchdeckPostDeployUrl(destination, report) {
    const route = resolveLaunchdeckPostDeployRoute(destination, report);
    return route && normalizePostDeployDestination(destination) === "axiom"
      ? `https://axiom.trade/meme/${encodeURIComponent(route)}`
      : "";
  }

  function normalizePostDeployUrl(value) {
    const url = String(value || "").trim();
    if (!url) return "";
    try {
      const parsed = new URL(url);
      return parsed.protocol === "https:" || parsed.protocol === "http:" ? parsed.toString() : "";
    } catch (_error) {
      return "";
    }
  }

  function launchdeckPostDeployPreferences() {
    const axiom = state.siteFeatures?.axiom || {};
    return {
      action: normalizeAxiomPostDeployAction(axiom.postDeployAction, "close_modal_toast"),
      destination: normalizePostDeployDestination(axiom.postDeployDestination, "axiom")
    };
  }

  function launchdeckPostDeployTitle(payload, report) {
    const supplied = String(payload?.title || "").trim();
    if (supplied) return supplied;
    const ticker = String(report?.symbol || report?.ticker || report?.tokenSymbol || report?.token?.symbol || "")
      .trim()
      .replace(/^\$+/, "")
      .toUpperCase()
      .slice(0, 24);
    return ticker ? `$${ticker} successfully deployed` : "Token successfully deployed";
  }

  function openLaunchdeckPostDeployUrl(url, mode = "tab") {
    const normalizedUrl = normalizePostDeployUrl(url);
    if (!normalizedUrl) return;
    const normalizedMode = mode === "window" ? "window" : "tab";
    void callBackground("trench:open-external-url", {
      url: normalizedUrl,
      mode: normalizedMode
    }).catch(() => {
      if (normalizedMode === "window") {
        window.open(normalizedUrl, "_blank", "popup=yes,width=1100,height=760,resizable=yes,scrollbars=yes");
      } else {
        window.open(normalizedUrl, "_blank", "noopener,noreferrer");
      }
    });
  }

  function handleLaunchdeckPostDeploySuccess(payload, shellActions = {}) {
    const report = payload?.report && typeof payload.report === "object" ? payload.report : null;
    const preferences = launchdeckPostDeployPreferences();
    const url = normalizePostDeployUrl(payload?.url) || buildLaunchdeckPostDeployUrl(preferences.destination, report);
    const title = launchdeckPostDeployTitle(payload, report);
    renderToast({
      id: `launchdeck-post-deploy-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      title,
      kind: "success",
      ttlMs: 5000,
      linkHref: url,
      linkLabel: "Open",
      clickHandler: url ? () => openLaunchdeckPostDeployUrl(url, "tab") : null
    });
    if (preferences.action === "close_modal_toast") {
      shellActions.closeOverlay?.();
    } else if (preferences.action === "open_tab_toast" && url) {
      openLaunchdeckPostDeployUrl(url, "tab");
    } else if (preferences.action === "open_window_toast" && url) {
      openLaunchdeckPostDeployUrl(url, "window");
    }
  }

  function installNavigationHooks() {
    if (state.navigationHooksInstalled) {
      return;
    }
    state.navigationHooksInstalled = true;
    const wrapHistoryMethod = (methodName) => {
      const original = window.history[methodName];
      if (typeof original !== "function") {
        return;
      }
      if (methodName === "pushState") {
        lifecycle.originalPushState = original;
      } else if (methodName === "replaceState") {
        lifecycle.originalReplaceState = original;
      }
      window.history[methodName] = function patchedHistoryMethod(...args) {
        const result = original.apply(this, args);
        scheduleRouteReconcile(`history-${methodName}`, 0);
        scheduleMountSweeps(`history-${methodName}`);
        return result;
      };
    };
    wrapHistoryMethod("pushState");
    wrapHistoryMethod("replaceState");
    lifecycle.popstateListener = () => {
      scheduleRouteReconcile("popstate", 0);
      scheduleMountSweeps("popstate");
    };
    lifecycle.hashchangeListener = () => {
      scheduleRouteReconcile("hashchange", 0);
      scheduleMountSweeps("hashchange");
    };
    lifecycle.visibilityChangeListener = () => {
      if (document.visibilityState === "visible") {
        setTradeReadiness(true, platform);
        primeTradeRuntime("visibility", { force: true });
        scheduleRouteReconcile("visibility", 0);
        scheduleMountSweeps("visibility");
        syncWalletStatusQuoteRefresh();
        syncActiveMarkSubscription();
      } else {
        setTradeReadiness(false, platform);
        syncWalletStatusQuoteRefresh();
        syncActiveMarkSubscription();
      }
    };
    lifecycle.focusListener = () => {
      setTradeReadiness(true, platform);
      primeTradeRuntime("focus", { force: true });
    };
    window.addEventListener("popstate", lifecycle.popstateListener);
    window.addEventListener("hashchange", lifecycle.hashchangeListener);
    document.addEventListener("visibilitychange", lifecycle.visibilityChangeListener);
    window.addEventListener("focus", lifecycle.focusListener);
  }

  function scheduleRouteReconcile(reason, delayMs = 0) {
    if (lifecycle.destroyed) {
      return;
    }
    clearPendingRouteReconcile(reason);
    const timer = window.setTimeout(() => {
      state.routeReconcileDelays.delete(reason);
      void reconcileSurfaceState(reason);
    }, delayMs);
    state.routeReconcileDelays.set(reason, timer);
  }

  function scheduleMountSweeps(reason = "manual") {
    if (lifecycle.destroyed || platform !== "axiom") {
      return;
    }
    const existingTimers = state.mountSweepTimers.get(reason) || [];
    existingTimers.forEach((timer) => window.clearTimeout(timer));

    // Bloom's Axiom path does an immediate sweep, then retries while the SPA
    // finishes replacing virtualized rows/panels. Keep this idempotent: the
    // platform mount functions decide whether anything is missing.
    const timers = [];
    for (const delayMs of [0, 100, 300, 750, 1500, 3000, 5000]) {
      const timer = window.setTimeout(() => {
        const activeTimers = state.mountSweepTimers.get(reason);
        if (Array.isArray(activeTimers)) {
          const index = activeTimers.indexOf(timer);
          if (index !== -1) {
            activeTimers.splice(index, 1);
          }
          if (!activeTimers.length) {
            state.mountSweepTimers.delete(reason);
          }
        }
        scheduleMountScan();
      }, delayMs);
      timers.push(timer);
    }
    state.mountSweepTimers.set(reason, timers);
  }

  async function reconcileSurfaceState(reason = "manual") {
    if (lifecycle.destroyed) {
      return state.tokenContext;
    }
    const nextUrl = window.location.href;
    const routeChanged = nextUrl !== state.currentRouteUrl;
    state.currentRouteUrl = nextUrl;
    state.tokenRequestSeq += 1;
    const routeTokenRequestSeq = state.tokenRequestSeq;

    if (routeChanged && state.quickPanelOpen) {
      closeQuickPanel();
    }

    // Preserve an already-open panel across coin-to-coin navigation.
    const panelWasOpenBeforeRoute = Boolean(state.panelOpen);

    // Clear token-scoped state before awaited route work can surface stale stats.
    if (routeChanged && reason !== "init") {
      if (state.walletStatusRefreshTimer) {
        window.clearTimeout(state.walletStatusRefreshTimer);
        state.walletStatusRefreshTimer = null;
      }
      state.panelTokenContext = null;
      state.tokenContext = null;
      clearPanelTokenScopedState();
      state.batchStatuses.clear();
      state.batchStatusStreamRevisions.clear();
      state.activePanelBatchId = null;
      pushPanelState();
      pushPanelPreview();
      pushPanelBatchStatus();
    }

    let tokenContext = null;
    try {
      tokenContext = await refreshCurrentToken({ requestSeq: routeTokenRequestSeq });
    } catch (error) {
      if (routeTokenRequestSeq !== state.tokenRequestSeq) {
        return;
      }
      setPageTokenContext(null);
      if (state.panelOpen) {
        surfaceUserFacingError(error, { pushToPanel: true, toast: false });
      }
    }
    if (routeTokenRequestSeq !== state.tokenRequestSeq) {
      return;
    }

    const nextSurface = String(tokenContext?.surface || "").trim();
    const isCoinDetailSurface = Boolean(tokenContext?.mint) && nextSurface === "token_detail";

    // Avoid tearing down during transient SPA route gaps.
    let rawCandidateSurface = "";
    try {
      rawCandidateSurface = String(
        getPlatformAdapter()?.getCurrentTokenCandidate?.()?.surface || ""
      ).trim();
    } catch {
      rawCandidateSurface = "";
    }
    const isDefinitelyNonCoinSurface =
      rawCandidateSurface === "pulse" ||
      (!isCoinDetailSurface && rawCandidateSurface && rawCandidateSurface !== "token_detail");

    if (isCoinDetailSurface) {
      if (panelWasOpenBeforeRoute) {
        ensurePanelFrame();
        setPersistentPanelTokenContext(tokenContext);
        await setPanelHidden(false, false);
      } else if (shouldAutoOpenPanel(tokenContext)) {
        const hidden = await syncVisibilityFromStorage(tokenContext);
        if (!hidden && !state.panelOpen) {
          maybeAutoOpenPanel(tokenContext);
        } else {
          ensurePanelFrame();
          setPersistentPanelTokenContext(tokenContext);
        }
      } else {
        ensureFloatingLauncher(tokenContext);
      }
      scheduleWalletStatusRefresh({
        tokenContext: currentActivePanelTokenContext() || tokenContext,
        force: true,
        delayMs: reason === "init" ? 0 : 80
      });
    } else if (isDefinitelyNonCoinSurface) {
      await setPanelHidden(true, false);
      destroyPersistentPanelFrame();
      setPersistentPanelTokenContext(null);
      clearPanelTokenScopedState();
      pushPanelState();
    } else if (!panelWasOpenBeforeRoute) {
      await setPanelHidden(true, false);
      destroyPersistentPanelFrame();
      setPersistentPanelTokenContext(null);
      clearPanelTokenScopedState();
      pushPanelState();
    }

    syncActiveMintSubscription();
    primeTradeRuntime(`route-${reason}`, { force: routeChanged });
    scheduleMountSweeps(`reconcile-${reason}`);
  }

  function defaultSiteFeatures() {
    return {
      axiom: {
        enabled: true,
        autoOpenPanel: false,
        floatingLauncher: true,
        instantTrade: true,
        launchdeckInjection: true,
        pulseButton: true,
        pulsePanel: true,
        pulseVamp: true,
        pulseVampMode: "prefill",
        instantTradeButtonModeCount: 3,
        vampIconMode: "both",
        dexScreenerIconMode: "both",
        postDeployAction: "close_modal_toast",
        postDeployDestination: "axiom",
        walletTracker: true,
        watchlist: true
      },
      j7: {
        enabled: false
      }
    };
  }

  const EXTENSION_APP_ROUTE_PROVIDERS = new Set(["helius-sender", "hellomoon"]);

  function explicitPresetProviderIsSupported(provider) {
    const normalized = String(provider || "").trim().toLowerCase();
    return !normalized || EXTENSION_APP_ROUTE_PROVIDERS.has(normalized);
  }

  function presetUsesSupportedRoutes(preset) {
    return explicitPresetProviderIsSupported(preset?.buyProvider) &&
      explicitPresetProviderIsSupported(preset?.sellProvider);
  }

  function normalizeExtensionBootstrap(bootstrap) {
    if (!bootstrap || typeof bootstrap !== "object") {
      return bootstrap;
    }
    const presets = Array.isArray(bootstrap.presets)
      ? bootstrap.presets.filter(presetUsesSupportedRoutes)
      : [];
    return {
      ...bootstrap,
      presets
    };
  }

  function normalizePulseVampMode(value, fallback = "prefill") {
    const mode = String(value || "").trim().toLowerCase();
    return mode === "insta" || mode === "prefill" ? mode : fallback;
  }

  function normalizeDexScreenerIconMode(value, fallback = "both") {
    const mode = String(value || "").trim().toLowerCase();
    return mode === "both" || mode === "pulse" || mode === "token" || mode === "off"
      ? mode
      : fallback;
  }

  function normalizeVampIconMode(value, fallback = "both") {
    const mode = String(value || "").trim().toLowerCase();
    return mode === "both" || mode === "pulse" || mode === "token" || mode === "off"
      ? mode
      : fallback;
  }

  function normalizeAxiomInstantTradeButtonModeCount(value, fallback = 3) {
    const count = Number(value);
    return count === 1 || count === 2 || count === 3 ? count : fallback;
  }

  function normalizeAxiomPostDeployAction(value, fallback = "close_modal_toast") {
    const action = String(value || "").trim().toLowerCase();
    return action === "close_modal_toast" ||
      action === "toast_only" ||
      action === "open_tab_toast" ||
      action === "open_window_toast"
      ? action
      : fallback;
  }

  function normalizeAxiomPostDeployDestination(value, fallback = "axiom") {
    const destination = String(value || "").trim().toLowerCase();
    return destination === "axiom" ? destination : fallback;
  }

  function panelStorageScope() {
    return platform || window.location.hostname;
  }

  function panelPositionStorageKey() {
    return `trenchTools.panelPosition.${panelStorageScope()}`;
  }

  function hiddenStateStorageKey() {
    return `trenchTools.hiddenState.${panelStorageScope()}`;
  }

  function panelDimensionsStorageKey() {
    return `trenchTools.panelDimensions.v2.${panelStorageScope()}`;
  }

  function panelScaleStorageKey() {
    return `trenchTools.panelScale.${panelStorageScope()}`;
  }

  function cloneTokenContext(tokenContext) {
    return tokenContext ? { ...tokenContext } : null;
  }

  function tokenContextRouteIdentity(tokenContext) {
    return String(
      tokenContext?.routeAddress
      || tokenContext?.rawAddress
      || tokenContext?.address
      || tokenContext?.mint
      || ""
    ).trim();
  }

  function tokenContextKey(tokenContext) {
    const routeIdentity = tokenContextRouteIdentity(tokenContext);
    if (!routeIdentity) {
      return "";
    }
    return [
      String(tokenContext.surface || "").trim(),
      routeIdentity,
      String(tokenContext.url || tokenContext.sourceUrl || "").trim()
    ].join("|");
  }

  function currentPanelRouteIdentity() {
    return tokenContextRouteIdentity(currentActivePanelTokenContext());
  }

  function currentActivePanelTokenContext() {
    if (state.quickPanelOpen && (state.quickPanelTokenContext?.routeAddress || state.quickPanelTokenContext?.mint)) {
      return state.quickPanelTokenContext;
    }
    if (state.panelOpen && (state.panelTokenContext?.routeAddress || state.panelTokenContext?.mint)) {
      return state.panelTokenContext;
    }
    return null;
  }

  function currentVisiblePanelMode() {
    if (state.quickPanelOpen) {
      return "quick";
    }
    if (state.panelOpen) {
      return "persistent";
    }
    return "";
  }

  function clearPanelTokenScopedState() {
    state.walletStatusRequestSeq += 1;
    state.previewRequestSeq += 1;
    state.walletStatus = null;
    state.preview = null;
    state.batchStatus = null;
    state.activePanelBatchId = null;
    state.batchStatusStreamRevisions.clear();
    syncActiveMarkSubscription();
  }

  function clearPendingRouteReconcile(delayKey) {
    const timer = state.routeReconcileDelays.get(delayKey);
    if (timer) {
      window.clearTimeout(timer);
      state.routeReconcileDelays.delete(delayKey);
    }
  }

  function scheduleWalletStatusRefresh({ force = true, tokenContext = null, delayMs = 80 } = {}) {
    if (state.walletStatusRefreshTimer) {
      window.clearTimeout(state.walletStatusRefreshTimer);
      state.walletStatusRefreshTimer = null;
    }
    state.walletStatusRefreshTimer = window.setTimeout(() => {
      state.walletStatusRefreshTimer = null;
      void refreshPanelWalletStatus({ tokenContext, force });
    }, delayMs);
  }

  function clearWalletStatusQuoteRefreshTimer() {
    if (state.walletStatusQuoteRefreshTimer) {
      window.clearTimeout(state.walletStatusQuoteRefreshTimer);
      state.walletStatusQuoteRefreshTimer = null;
    }
  }

  function activeQuoteRefreshTokenContext() {
    if (!(state.panelOpen || state.quickPanelOpen) || document.visibilityState === "hidden") {
      return null;
    }
    const tokenContext = currentActivePanelTokenContext();
    const mint = String(tokenContext?.mint || state.walletStatus?.mint || "").trim();
    if (!tokenContext || !mint) {
      return null;
    }
    const walletStatusMint = String(state.walletStatus?.mint || "").trim();
    if (walletStatusMint && walletStatusMint !== mint) {
      return null;
    }
    const amount = Number(
      state.walletStatus?.holdingAmount ??
      state.walletStatus?.tokenBalance ??
      state.walletStatus?.mintBalanceUi
    );
    return Number.isFinite(amount) && amount > 0 ? tokenContext : null;
  }

  function scheduleWalletStatusQuoteRefresh(
    delayMs = WALLET_STATUS_QUOTE_REFRESH_MS,
    { reset = false } = {}
  ) {
    if (lifecycle.destroyed) {
      return;
    }
    if (state.walletStatusQuoteRefreshTimer) {
      if (!reset) {
        return;
      }
      clearWalletStatusQuoteRefreshTimer();
    }
    if (!activeQuoteRefreshTokenContext()) {
      return;
    }
    state.walletStatusQuoteRefreshTimer = window.setTimeout(() => {
      state.walletStatusQuoteRefreshTimer = null;
      const tokenContext = activeQuoteRefreshTokenContext();
      if (!tokenContext) {
        return;
      }
      void refreshPanelWalletStatus({ tokenContext, force: true })
        .finally(() => {
          scheduleWalletStatusQuoteRefresh();
        });
    }, Math.max(50, Number(delayMs) || WALLET_STATUS_QUOTE_REFRESH_MS));
  }

  function syncWalletStatusQuoteRefresh() {
    if (activeQuoteRefreshTokenContext()) {
      scheduleWalletStatusQuoteRefresh();
    } else {
      clearWalletStatusQuoteRefreshTimer();
    }
  }

  function syncActiveMintSubscription() {
    const activeTokenContext =
      currentActivePanelTokenContext() ||
      ((shouldAutoOpenPanel(state.tokenContext) || shouldMountLauncher(state.tokenContext))
        ? state.tokenContext
        : null);
    if (activeTokenContext?.mint) {
      setActiveMintForSurface(activeTokenContext.mint);
    } else {
      clearActiveMintForSurface();
    }
    syncActiveMarkSubscription();
  }

  function setPageTokenContext(tokenContext) {
    const previousKey = tokenContextKey(state.tokenContext);
    const nextTokenContext = cloneTokenContext(tokenContext);
    const nextKey = tokenContextKey(nextTokenContext);
    state.tokenContext = nextTokenContext;
    if (previousKey && nextKey && previousKey !== nextKey && !currentActivePanelTokenContext()) {
      clearPanelTokenScopedState();
    }
    ensureFloatingLauncher(state.tokenContext);
    syncActiveMintSubscription();
    pushPanelState();
    return state.tokenContext;
  }

  function setPersistentPanelTokenContext(tokenContext) {
    const previousKey = tokenContextKey(state.panelTokenContext);
    const nextTokenContext = cloneTokenContext(tokenContext);
    const nextKey = tokenContextKey(nextTokenContext);
    if (previousKey && nextKey && previousKey !== nextKey) {
      clearPanelTokenScopedState();
    } else if (previousKey && !nextKey) {
      clearPanelTokenScopedState();
    }
    state.panelTokenContext = nextTokenContext;
    syncActiveMintSubscription();
    pushPanelState();
    return state.panelTokenContext;
  }

  function setQuickPanelTokenContext(tokenContext) {
    const previousKey = tokenContextKey(state.quickPanelTokenContext);
    const nextTokenContext = cloneTokenContext(tokenContext);
    const nextKey = tokenContextKey(nextTokenContext);
    if (previousKey && nextKey && previousKey !== nextKey) {
      clearPanelTokenScopedState();
    } else if (previousKey && !nextKey) {
      clearPanelTokenScopedState();
    }
    state.quickPanelTokenContext = nextTokenContext;
    syncActiveMintSubscription();
    pushPanelState();
    return state.quickPanelTokenContext;
  }

  function enforcePersistentSurfaceBoundary() {
    let candidate = null;
    try {
      candidate = getPlatformAdapter()?.getCurrentTokenCandidate?.() || null;
    } catch {
      candidate = null;
    }
    if (!candidate?.surface) {
      return;
    }
    // Tear the panel down ONLY when the platform says we're on a surface
    // that isn't a coin-detail page (e.g. axiom's pulse surface). We don't
    // gate this on `shouldMountLauncher` anymore because that also folds in
    // the user's `floatingLauncher` toggle — which should control the
    // launcher button, not destroy an already-open panel on the right
    // surface. Without this distinction, a mutation during coin→coin
    // navigation can destroy the panel even though the new route is still a
    // coin-detail page.
    const surface = String(candidate.surface || "").trim();
    const surfaceAllowsPanel = surface === "token_detail";
    if (!surfaceAllowsPanel) {
      destroyPersistentPanelFrame();
      setPersistentPanelTokenContext(null);
      if (state.launcherButton) {
        state.launcherButton.remove();
        state.launcherButton = null;
      }
      return;
    }
    // We're on a coin-detail surface — if the user has the floating launcher
    // disabled, just clean up the launcher button but keep the panel.
    if (!shouldMountLauncher(candidate) && state.launcherButton) {
      state.launcherButton.remove();
      state.launcherButton = null;
    }
  }

  function isExtensionContextInvalid(error) {
    return /Extension context invalidated/i.test(String(error?.message || error || ""));
  }

  function isHostAvailabilityError(error) {
    const code = String(error?.code || "").trim().toUpperCase();
    const status = Number.isInteger(error?.status) ? Number(error.status) : null;
    if ([
      "HOST_UNREACHABLE",
      "HOST_TIMEOUT",
      "HOST_UNAUTHORIZED",
      "HOST_INSECURE_TRANSPORT",
      "HOST_INVALID_URL",
      "HOST_OVERLOADED"
    ].includes(code)) {
      return true;
    }
    return Number.isInteger(status) && status >= 500;
  }

  function isExtensionReloadedError(error) {
    const code = String(error?.code || "").trim().toUpperCase();
    if (code === "EXTENSION_RELOADED") {
      return true;
    }
    if (isExtensionContextInvalid(error)) {
      return true;
    }
    return /Extension connection lost|Extension reloaded/i.test(String(error?.message || error || ""));
  }

  function buildHostAvailabilityCopy(error) {
    if (isExtensionReloadedError(error)) {
      return {
        title: "Extension connection lost",
        detail: "Refresh this tab to reconnect Trench Tools.",
        message: EXTENSION_RELOAD_FRIENDLY_MESSAGE
      };
    }
    if (!isHostAvailabilityError(error)) {
      return null;
    }
    const code = String(error?.code || "").trim().toUpperCase();
    const status = Number.isInteger(error?.status) ? Number(error.status) : null;
    const rawMessage = normalizeErrorMessageForDisplay(error);
    switch (code) {
      case "HOST_UNAUTHORIZED":
        return {
          title: "Engine token rejected",
          detail: "Open Trench Tools settings and update the execution engine access token.",
          message: "Execution engine token rejected."
        };
      case "HOST_TIMEOUT":
        return {
          title: "Engine request timed out",
          detail: "The execution engine did not respond in time. Check that it is running and not overloaded.",
          message: "Execution engine request timed out."
        };
      case "HOST_INVALID_URL":
        return {
          title: "Engine URL invalid",
          detail: rawMessage || "Open Trench Tools settings and fix the execution engine URL.",
          message: rawMessage || "Execution engine URL is invalid."
        };
      case "HOST_INSECURE_TRANSPORT":
        return {
          title: "Engine URL blocked",
          detail: rawMessage || "Remote execution hosts must use HTTPS. Localhost can use HTTP.",
          message: rawMessage || "Execution engine URL is blocked."
        };
      case "HOST_OVERLOADED":
        return {
          title: "Engine overloaded",
          detail: "The execution engine is rate limiting requests. Wait a moment and try again.",
          message: rawMessage || "Execution engine is overloaded."
        };
      case "HOST_UNREACHABLE":
        return {
          title: "Execution engine offline",
          detail: "Start the execution engine, then try opening the panel again.",
          message: "Execution engine offline."
        };
      default:
        if (Number.isInteger(status) && status >= 500) {
          return {
            title: "Engine request failed",
            detail: "The execution engine returned a server error. Check the engine terminal and try again.",
            message: rawMessage || "Execution engine request failed."
          };
        }
        return {
          title: "Execution engine unavailable",
          detail: rawMessage || "Check that the execution engine is running, then try again.",
          message: rawMessage || "Execution engine unavailable."
        };
    }
  }

  function userFacingErrorMessage(error) {
    const hostCopy = buildHostAvailabilityCopy(error);
    if (hostCopy) {
      return hostCopy.message || hostCopy.title;
    }
    const message = normalizeErrorMessageForDisplay(error);
    return message || "Unknown error";
  }

  function normalizeErrorMessageForDisplay(errorOrMessage) {
    let message = String(errorOrMessage?.message || errorOrMessage || "").trim();
    if (!message) {
      return "";
    }
    message = message.replace(/\s+/g, " ").trim();
    message = message.replace(/^\[[^\]]+\]\s*/, "").trim();

    if (isZeroTokenBalanceMessage(message)) {
      return "You have 0 tokens.";
    }

    if (/^No supported execution venue for address [^.]+\./i.test(message)) {
      return "No supported execution venue for this token/pair.";
    }
    if (/^No supported execution venue for /i.test(message)) {
      return "No supported execution venue for this token/pair.";
    }

    const wrapperMatch = message.match(/^Wrapper wrap failed for ([^(]+?) \((?:[^)]*)\):\s*(.+)$/i);
    if (wrapperMatch) {
      const reason = String(wrapperMatch[2] || "").trim();
      if (/allowlisted venue instructions/i.test(reason) && /exactly one per tx/i.test(reason)) {
        return "Route contains multiple venue instructions and cannot be wrapped.";
      }
      return reason || "Could not prepare this transaction.";
    }

    const onChainMatch = message.match(
      /^Transaction [1-9A-HJ-NP-Za-km-z]{40,120} failed on-chain:\s*(.+)$/i,
    );
    if (onChainMatch) {
      const payload = String(onChainMatch[1] || "").trim();
      try {
        const parsed = JSON.parse(payload);
        const instructionError = Array.isArray(parsed?.InstructionError)
          ? parsed.InstructionError
          : null;
        if (instructionError?.length >= 2) {
          const errorCode = instructionError[1];
          if (typeof errorCode === "string" && errorCode.trim()) {
            return `Transaction failed on-chain (${errorCode.trim()}).`;
          }
          if (Number.isInteger(errorCode?.Custom)) {
            return `Transaction failed on-chain (custom error ${errorCode.Custom}).`;
          }
        }
      } catch {
        // Fall through to the raw payload below when the on-chain error body
        // is not JSON.
      }
      return payload
        ? `Transaction failed on-chain: ${payload}`
        : "Transaction failed on-chain.";
    }

    return message;
  }

  function isZeroTokenBalanceMessage(message) {
    const normalized = String(message || "").replace(/\s+/g, " ").trim();
    if (!normalized) {
      return false;
    }
    return /^You have 0 tokens\.?$/i.test(normalized)
      || /^Wallet has no token balance for this sell request\.?$/i.test(normalized)
      || /^Token account [1-9A-HJ-NP-Za-km-z]{32,44} for mint [1-9A-HJ-NP-Za-km-z]{32,44} was not visible after retries\b/i.test(normalized);
  }

  function isZeroTokenBalanceError(errorOrMessage) {
    return isZeroTokenBalanceMessage(normalizeErrorMessageForDisplay(errorOrMessage));
  }

  function balanceGateFailureReason(errorOrMessage) {
    const message = normalizeErrorMessageForDisplay(errorOrMessage);
    if (/^Insufficient SOL balance for buy amount\.?$/i.test(message)) {
      return "Insufficient SOL balance for buy amount";
    }
    if (/^Insufficient token balance for sell amount\.?$/i.test(message)) {
      return "Insufficient token balance for sell amount";
    }
    return "";
  }

  function balanceGateFailureToastTitle(reason, count) {
    const walletLabel = count === 1 ? "wallet" : "wallets";
    return `Transactions failed to send: ${reason} (${count} ${walletLabel})`;
  }

  function balanceGateFailureSummary(wallets) {
    const failedWallets = (Array.isArray(wallets) ? wallets : []).filter((walletState) => {
      return String(walletState?.status || "").trim().toLowerCase() === "failed";
    });
    const balanceFailures = failedWallets
      .map((walletState) => balanceGateFailureReason(walletState?.error))
      .filter(Boolean);
    if (balanceFailures.length === 0) {
      return null;
    }
    const reason = balanceFailures[0];
    const allSameReason = balanceFailures.every((value) => value === reason);
    if (!allSameReason || balanceFailures.length !== failedWallets.length) {
      return null;
    }
    return {
      reason,
      count: balanceFailures.length,
      title: balanceGateFailureToastTitle(reason, balanceFailures.length),
    };
  }

  function buildErrorToastCopy(errorOrMessage, options = {}) {
    const normalizedSide = String(options.side || "").trim().toLowerCase();
    const fallbackTitle = normalizedSide ? `${capitalize(normalizedSide)} failed` : "Request failed";
    const hostCopy = buildHostAvailabilityCopy(errorOrMessage);
    if (hostCopy) {
      return hostCopy;
    }
    const message = normalizeErrorMessageForDisplay(errorOrMessage);
    if (!message) {
      return {
        title: fallbackTitle,
        detail: "",
        message: fallbackTitle
      };
    }
    if (isZeroTokenBalanceMessage(message)) {
      const title = "Transaction failed to send: You have 0 tokens.";
      return {
        title,
        detail: "",
        message
      };
    }
    if (/^No supported execution venue/i.test(message)) {
      return {
        title: "No supported route",
        detail: "This token or pair is not supported for execution yet.",
        message
      };
    }
    if (/multiple venue instructions/i.test(message) && /cannot be wrapped/i.test(message)) {
      return {
        title: "Unsupported route",
        detail: message,
        message
      };
    }
    if (/slippage/i.test(message)) {
      return {
        title: "Slippage too high",
        detail: /^slippage too high\.?$/i.test(message) ? "" : message,
        message
      };
    }
    if (/^No route\b/i.test(message) || /\bno route\b/i.test(message)) {
      return {
        title: "No route",
        detail: message,
        message
      };
    }
    if (message.length <= 48) {
      return {
        title: message,
        detail: "",
        message
      };
    }
    return {
      title: fallbackTitle,
      detail: message,
      message
    };
  }

  function attachGlobalErrorRecoveryHandlers() {
    if (!lifecycle.errorListener) {
      lifecycle.errorListener = (event) => {
        const candidate = event?.error || event?.message;
        if (!isExtensionReloadedError(candidate)) {
          return;
        }
        scheduleExtensionReloadFallbackToast();
        event.preventDefault();
      };
      window.addEventListener("error", lifecycle.errorListener);
    }
    if (!lifecycle.unhandledRejectionListener) {
      lifecycle.unhandledRejectionListener = (event) => {
        if (!isExtensionReloadedError(event?.reason)) {
          return;
        }
        scheduleExtensionReloadFallbackToast();
        event.preventDefault();
      };
      window.addEventListener("unhandledrejection", lifecycle.unhandledRejectionListener);
    }
  }

  function scheduleExtensionReloadFallbackToast() {
    if (
      lifecycle.destroyed
      || state.extensionReloadToastShown
      || state.extensionReloadFallbackTimer
    ) {
      return;
    }
    state.extensionReloadFallbackTimer = window.setTimeout(() => {
      state.extensionReloadFallbackTimer = null;
      if (lifecycle.destroyed || state.extensionReloadToastShown) {
        return;
      }
      state.extensionReloadToastShown = true;
      renderToast({
        id: "extension-reloaded",
        title: "Connection lost",
        detail: "Refresh to reconnect.",
        kind: "info",
        persistent: true,
        actionLabel: "Refresh",
        actionHandler: () => window.location.reload()
      });
    }, EXTENSION_RELOAD_TOAST_FALLBACK_DELAY_MS);
  }

  function surfaceUserFacingError(error, options = {}) {
    if (!error || lifecycle.destroyed) {
      return;
    }
    if (isExtensionReloadedError(error)) {
      scheduleExtensionReloadFallbackToast();
      return;
    }
    const toastCopy = buildErrorToastCopy(error, { side: options.side });
    const message = toastCopy.message || userFacingErrorMessage(error);
    if (options.pushToPanel) {
      pushPanelError(message, {
        title: toastCopy.title,
        kind: "error",
        source: isHostAvailabilityError(error) ? "host" : "notice"
      });
    }
    if (options.toast === false) {
      return;
    }
    renderToast({
      id: `notice-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      title: toastCopy.title,
      detail: toastCopy.detail,
      kind: "error",
      ttlMs: 4200
    });
  }

  function runBestEffortTeardownStep(label, callback) {
    try {
      callback();
    } catch (error) {
      if (isExtensionReloadedError(error)) {
        return;
      }
      console.warn(`Trench Tools teardown step failed (${label})`, error);
    }
  }

  function teardown() {
    if (lifecycle.destroyed) {
      return;
    }
    lifecycle.destroyed = true;
    window.__trenchToolsContentScriptActive = false;
    runBestEffortTeardownStep("trade-readiness", () => {
      setTradeReadiness(false, platform);
    });
    if (window.__trenchToolsContentScriptInstance?.teardown === teardown) {
      window.__trenchToolsContentScriptInstance = null;
    }

    runBestEffortTeardownStep("panel-message-listener", () => {
      if (lifecycle.panelMessageListener) {
        window.removeEventListener("message", lifecycle.panelMessageListener);
        lifecycle.panelMessageListener = null;
      }
    });
    runBestEffortTeardownStep("storage-change-listener", () => {
      if (lifecycle.storageChangeListener) {
        chrome.storage.onChanged.removeListener(lifecycle.storageChangeListener);
        lifecycle.storageChangeListener = null;
      }
    });
    runBestEffortTeardownStep("pagehide-listener", () => {
      if (lifecycle.pagehideListener) {
        window.removeEventListener("pagehide", lifecycle.pagehideListener);
        lifecycle.pagehideListener = null;
      }
    });
    runBestEffortTeardownStep("popstate-listener", () => {
      if (lifecycle.popstateListener) {
        window.removeEventListener("popstate", lifecycle.popstateListener);
        lifecycle.popstateListener = null;
      }
    });
    runBestEffortTeardownStep("hashchange-listener", () => {
      if (lifecycle.hashchangeListener) {
        window.removeEventListener("hashchange", lifecycle.hashchangeListener);
        lifecycle.hashchangeListener = null;
      }
    });
    runBestEffortTeardownStep("visibility-listener", () => {
      if (lifecycle.visibilityChangeListener) {
        document.removeEventListener("visibilitychange", lifecycle.visibilityChangeListener);
        lifecycle.visibilityChangeListener = null;
      }
    });
    runBestEffortTeardownStep("focus-listener", () => {
      if (lifecycle.focusListener) {
        window.removeEventListener("focus", lifecycle.focusListener);
        lifecycle.focusListener = null;
      }
    });
    runBestEffortTeardownStep("error-listener", () => {
      if (lifecycle.errorListener) {
        window.removeEventListener("error", lifecycle.errorListener);
        lifecycle.errorListener = null;
      }
    });
    runBestEffortTeardownStep("unhandled-rejection-listener", () => {
      if (lifecycle.unhandledRejectionListener) {
        window.removeEventListener("unhandledrejection", lifecycle.unhandledRejectionListener);
        lifecycle.unhandledRejectionListener = null;
      }
    });
    runBestEffortTeardownStep("history-restoration", () => {
      if (lifecycle.originalPushState) {
        window.history.pushState = lifecycle.originalPushState;
        lifecycle.originalPushState = null;
      }
      if (lifecycle.originalReplaceState) {
        window.history.replaceState = lifecycle.originalReplaceState;
        lifecycle.originalReplaceState = null;
      }
    });
    state.navigationHooksInstalled = false;

    runBestEffortTeardownStep("timers-and-observers", () => {
      if (state.hostRevisionTimer) {
        window.clearInterval(state.hostRevisionTimer);
        state.hostRevisionTimer = null;
      }
      if (state.extensionReloadFallbackTimer) {
        window.clearTimeout(state.extensionReloadFallbackTimer);
        state.extensionReloadFallbackTimer = null;
      }
      if (state.walletStatusRefreshTimer) {
        window.clearTimeout(state.walletStatusRefreshTimer);
        state.walletStatusRefreshTimer = null;
      }
      clearWalletStatusQuoteRefreshTimer();
      for (const timer of state.routeReconcileDelays.values()) {
        window.clearTimeout(timer);
      }
      state.routeReconcileDelays.clear();
      for (const timers of state.mountSweepTimers.values()) {
        timers.forEach((timer) => window.clearTimeout(timer));
      }
      state.mountSweepTimers.clear();
      for (const timer of state.statusPollTimers.values()) {
        window.clearInterval(timer);
      }
      state.statusPollTimers.clear();
      if (state.toastCleanupInterval) {
        window.clearInterval(state.toastCleanupInterval);
        state.toastCleanupInterval = null;
      }
      for (const entry of state.activeToasts.values()) {
        window.clearTimeout(entry?.timeoutId);
        entry?.element?.remove();
      }
      state.activeToasts.clear();
      if (state.mutationObserver) {
        state.mutationObserver.disconnect();
        state.mutationObserver = null;
      }
      disconnectPanelLayerObservers();
      if (debounceTimer) {
        window.clearTimeout(debounceTimer);
        debounceTimer = 0;
      }
      if (scanFrameId) {
        window.cancelAnimationFrame(scanFrameId);
        scanFrameId = 0;
      }
    });

    runBestEffortTeardownStep("panels-and-drag", () => {
      clearQuickPanelCloseHandlers();
      closeQuickPanel();
      destroyPersistentPanelFrame();
      if (state.activeDrag?.dragging) {
        void endPanelDrag();
      }
    });
    runBestEffortTeardownStep("launchdeck-shell", () => {
      state.launchdeckShell?.destroy?.();
      state.launchdeckShell = null;
    });
    runBestEffortTeardownStep("dom-cleanup", () => {
      state.launcherButton?.remove();
      state.launcherButton = null;
      document.getElementById("trench-tools-toast-host")?.remove();
      document.getElementById("trench-tools-toast-styles")?.remove();
      document.getElementById("trench-tools-floating-launcher")?.remove();
      document.querySelectorAll(".trench-tools-pulse-panel-owner").forEach((element) => {
        element.classList.remove("trench-tools-pulse-panel-owner");
      });
    });
    runBestEffortTeardownStep("active-mark", () => {
      clearActiveMarkForSurface();
    });
    runBestEffortTeardownStep("active-mint", () => {
      clearActiveMintForSurface();
    });
  }

  async function safeStorageGet(key) {
    try {
      return await chrome.storage.local.get(key);
    } catch (error) {
      if (isExtensionContextInvalid(error)) {
        return {};
      }
      throw error;
    }
  }

  async function safeStorageSet(value) {
    try {
      await chrome.storage.local.set(value);
      return true;
    } catch (error) {
      if (isExtensionContextInvalid(error)) {
        return false;
      }
      throw error;
    }
  }

  function defaultPreferences() {
    return {
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
      quickBuyAmount: "",
      buyAmountSol: "",
      customSellPercent: "",
      customSellSol: ""
    };
  }

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

  function getValidQuickBuyAmount(value) {
    const normalized = normalizeQuickBuyAmountInput(value);
    if (!normalized || normalized.endsWith(".")) {
      return "";
    }
    const parsed = Number(normalized);
    if (!Number.isFinite(parsed) || parsed <= 0) {
      return "";
    }
    return normalized;
  }

  function normalizePreferencesValue(value) {
    const normalized = {
      presetId: String(value?.presetId || "").trim(),
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
      includeFees: typeof value?.includeFees === "boolean" ? value.includeFees : null,
      quickBuyAmount: normalizeQuickBuyAmountInput(value?.quickBuyAmount || ""),
      // Custom panel buy input is intentionally session-local. Quick-buy
      // buttons should not backfill it, and a page refresh should reset it.
      buyAmountSol: "",
      customSellPercent: normalizeQuickBuyAmountInput(value?.customSellPercent || ""),
      customSellSol: normalizeQuickBuyAmountInput(value?.customSellSol || ""),
      hideWalletGroupRow: Boolean(value?.hideWalletGroupRow),
      hidePresetChipRow: Boolean(value?.hidePresetChipRow)
    };
    mirrorWalletSelectionPreferenceOntoPreferences(normalized, value);
    normalized.selectionRevision = Math.max(0, Number(value?.selectionRevision || 0) || 0);
    return normalized;
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

    const normalizedWalletKeys = Array.isArray(value?.walletKeys)
      ? value.walletKeys
      : Array.isArray(value?.selectionTarget?.walletKeys)
        ? value.selectionTarget.walletKeys
        : [];
    const target = normalizeSelectionTarget(value?.selectionTarget || {
      type: value?.selectionMode,
      walletKey: value?.walletKey,
      walletGroupId: value?.walletGroupId,
      walletKeys: normalizedWalletKeys
    });
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

  function normalizeSelectionTarget(value) {
    const type = String(value?.type || value?.selectionMode || "wallet_group").trim() || "wallet_group";
    return {
      type: ["wallet_group", "wallet_list", "single_wallet"].includes(type) ? type : "wallet_group",
      walletKey: String(value?.walletKey || "").trim(),
      walletGroupId: String(value?.walletGroupId || "").trim(),
      walletKeys: Array.isArray(value?.walletKeys)
        ? value.walletKeys.map((entry) => String(entry || "").trim()).filter(Boolean)
        : []
    };
  }

  async function loadPreferences() {
    const stored = await safeStorageGet(PREFERENCES_KEY);
    return normalizePreferencesValue(stored[PREFERENCES_KEY] || {});
  }

  function serializePreferencesForStorage(preferences) {
    const selection = normalizeWalletSelectionPreference(preferences);
    return {
      presetId: String(preferences?.presetId || "").trim(),
      selectionSource: selection.selectionSource,
      activeWalletGroupId: selection.activeWalletGroupId,
      manualWalletKeys: [...selection.manualWalletKeys],
      selectionRevision: Math.max(0, Number(preferences?.selectionRevision || 0) || 0),
      includeFees: typeof preferences?.includeFees === "boolean" ? preferences.includeFees : null,
      quickBuyAmount: normalizeQuickBuyAmountInput(preferences?.quickBuyAmount || ""),
      customSellPercent: normalizeQuickBuyAmountInput(preferences?.customSellPercent || ""),
      customSellSol: normalizeQuickBuyAmountInput(preferences?.customSellSol || ""),
      hideWalletGroupRow: Boolean(preferences?.hideWalletGroupRow),
      hidePresetChipRow: Boolean(preferences?.hidePresetChipRow)
    };
  }

  async function loadSiteFeatures() {
    const stored = await safeStorageGet(SITE_FEATURES_KEY);
    return normalizeSiteFeaturesValue(stored[SITE_FEATURES_KEY] || {});
  }

  function normalizeSiteFeaturesValue(value) {
    const defaults = defaultSiteFeatures();
    return {
      axiom: {
        ...defaults.axiom,
        ...(value.axiom || {}),
        enabled: value.axiom?.enabled ?? defaults.axiom.enabled,
        instantTrade: value.axiom?.instantTrade ?? value.axiom?.tokenDetailButton ?? defaults.axiom.instantTrade,
        launchdeckInjection: value.axiom?.launchdeckInjection ?? value.axiom?.launchdeck ?? defaults.axiom.launchdeckInjection,
        pulseButton: value.axiom?.pulseButton ?? defaults.axiom.pulseButton,
        pulsePanel: value.axiom?.pulsePanel ?? defaults.axiom.pulsePanel,
        pulseVamp: value.axiom?.pulseVamp ?? defaults.axiom.pulseVamp,
        pulseVampMode: normalizePulseVampMode(value.axiom?.pulseVampMode, defaults.axiom.pulseVampMode),
        instantTradeButtonModeCount: normalizeAxiomInstantTradeButtonModeCount(
          value.axiom?.instantTradeButtonModeCount,
          defaults.axiom.instantTradeButtonModeCount
        ),
        vampIconMode: normalizeVampIconMode(
          value.axiom?.vampIconMode,
          value.axiom?.pulseVamp === false ? "off" : defaults.axiom.vampIconMode
        ),
        dexScreenerIconMode: normalizeDexScreenerIconMode(
          value.axiom?.dexScreenerIconMode,
          defaults.axiom.dexScreenerIconMode
        ),
        postDeployAction: normalizeAxiomPostDeployAction(
          value.axiom?.postDeployAction,
          defaults.axiom.postDeployAction
        ),
        postDeployDestination: normalizeAxiomPostDeployDestination(
          value.axiom?.postDeployDestination,
          defaults.axiom.postDeployDestination
        ),
        walletTracker: value.axiom?.walletTracker ?? defaults.axiom.walletTracker,
        watchlist: value.axiom?.watchlist ?? defaults.axiom.watchlist
      },
      j7: {
        ...defaults.j7,
        ...(value.j7 || {}),
        enabled: false
      }
    };
  }

  function defaultSoundSettings(side) {
    return {
      enabled: true,
      templateId: side === "sell" ? "notification-2" : "notification-1",
      custom: null
    };
  }

  function defaultAppearance() {
    return {
      volume: 70,
      buySound: defaultSoundSettings("buy"),
      sellSound: defaultSoundSettings("sell")
    };
  }

  function normalizeSoundValue(value, defaults) {
    const source = value || {};
    let custom = null;
    if (source.custom && typeof source.custom === "object") {
      const dataUrl = typeof source.custom.dataUrl === "string"
        ? source.custom.dataUrl.trim()
        : "";
      if (dataUrl.startsWith("data:")) {
        custom = {
          name: String(source.custom.name || "Custom sound").slice(0, 128),
          dataUrl
        };
      }
    }
    const rawTemplate = String(source.templateId || "").trim();
    const templateValid =
      rawTemplate === BUY_SOUND_CUSTOM_ID ||
      Object.prototype.hasOwnProperty.call(BUY_SOUND_TEMPLATE_PATHS, rawTemplate);
    const templateId = templateValid ? rawTemplate : defaults.templateId;
    return {
      enabled: Boolean(source.enabled ?? defaults.enabled),
      templateId,
      custom
    };
  }

  // Prefer the shared top-level volume; fall back to any legacy per-side volume
  // so a stored older shape doesn't reset to 70 on the first read.
  function pickSharedVolume(value, defaultVolume) {
    const candidates = [value?.volume, value?.buySound?.volume, value?.sellSound?.volume];
    for (const candidate of candidates) {
      const num = Number(candidate);
      if (Number.isFinite(num)) {
        return Math.min(100, Math.max(0, Math.round(num)));
      }
    }
    return defaultVolume;
  }

  function normalizeAppearanceValue(value) {
    const defaults = defaultAppearance();
    return {
      volume: pickSharedVolume(value, defaults.volume),
      buySound: normalizeSoundValue(value?.buySound, defaults.buySound),
      sellSound: normalizeSoundValue(value?.sellSound, defaults.sellSound)
    };
  }

  async function loadAppearance() {
    const stored = await safeStorageGet(APPEARANCE_KEY);
    return normalizeAppearanceValue(stored[APPEARANCE_KEY] || {});
  }

  function resolveSoundUrlFromSettings(settings) {
    if (!settings?.enabled) {
      return "";
    }
    if (settings.templateId === BUY_SOUND_CUSTOM_ID) {
      return settings.custom?.dataUrl || "";
    }
    const relativePath = BUY_SOUND_TEMPLATE_PATHS[settings.templateId];
    if (!relativePath) {
      return "";
    }
    try {
      return chrome.runtime.getURL(relativePath);
    } catch {
      return relativePath;
    }
  }

  function rememberBuySoundPlayed(key) {
    if (!key || state.buySoundPlayed.has(key)) {
      return false;
    }
    state.buySoundPlayed.add(key);
    if (state.buySoundPlayed.size > BUY_SOUND_PLAY_CACHE_LIMIT) {
      const iterator = state.buySoundPlayed.values();
      for (let i = 0; i < 80; i += 1) {
        const next = iterator.next();
        if (next.done) break;
        state.buySoundPlayed.delete(next.value);
      }
    }
    return true;
  }

  function playSideConfirmationSound(side) {
    const key = side === "sell" ? "sellSound" : "buySound";
    const settings = state.appearance?.[key];
    if (!settings || !settings.enabled) {
      return;
    }
    const url = resolveSoundUrlFromSettings(settings);
    if (!url) {
      return;
    }
    const volume = Math.min(
      1,
      Math.max(0, Number(state.appearance?.volume ?? 70) / 100)
    );
    // Route through the background offscreen document so playback works
    // regardless of the host page's CSP / autoplay policy.
    try {
      const maybePromise = callBackground("trench:play-sound", { url, volume });
      if (maybePromise && typeof maybePromise.catch === "function") {
        maybePromise.catch(() => {});
      }
    } catch {
      // Background unavailable (extension reloading) - best effort fall back
      // to an in-page Audio(), which may still work on permissive sites.
      try {
        const audio = new Audio(url);
        audio.volume = volume;
        const promise = audio.play();
        if (promise && typeof promise.catch === "function") {
          promise.catch(() => {});
        }
      } catch {}
    }
  }

  async function loadPanelPosition() {
    const key = panelPositionStorageKey();
    const stored = await safeStorageGet(key);
    return stored[key] || null;
  }

  async function loadPanelDimensions() {
    const key = panelDimensionsStorageKey();
    const stored = await safeStorageGet(key);
    const value = stored[key];
    if (!value || typeof value !== "object") {
      return null;
    }
    const width = Number(value.width);
    const height = Number(value.height);
    if (!Number.isFinite(width) || !Number.isFinite(height)) {
      return null;
    }
    return { width, height };
  }

  async function loadPanelScale() {
    const key = panelScaleStorageKey();
    const stored = await safeStorageGet(key);
    const value = Number(stored[key]);
    return Number.isFinite(value) && value > 0 ? value : 1;
  }

  async function savePreferences(nextPreferences) {
    state.preferences = normalizePreferencesValue({
      ...state.preferences,
      ...nextPreferences
    });
    await safeStorageSet({ [PREFERENCES_KEY]: serializePreferencesForStorage(state.preferences) });
    pushPanelState();
  }

  function buildWalletStatusRequestPayload({ tokenContext = null, force = false } = {}) {
    const payload = {};
    const activePreset = getActivePreset();
    if (activePreset?.id || state.preferences?.presetId) {
      payload.presetId = String(activePreset?.id || state.preferences?.presetId || "").trim();
    }
    const buyFundingPolicy = activePolicyValue("buy");
    const sellSettlementPolicy = activePolicyValue("sell");
    if (buyFundingPolicy) {
      payload.buyFundingPolicy = backendPolicyValue(buyFundingPolicy);
    }
    if (sellSettlementPolicy) {
      payload.sellSettlementPolicy = backendPolicyValue(sellSettlementPolicy);
    }
    const selection = normalizeWalletSelectionPreference(state.preferences);
    if (selection.selectionSource === "group") {
      payload.walletGroupId = requireSelectedWalletGroupId(selection);
    } else {
      const manualWalletKeys = Array.from(new Set((selection.manualWalletKeys || []).filter(Boolean)));
      if (manualWalletKeys.length === 1) {
        payload.walletKey = manualWalletKeys[0];
      } else if (manualWalletKeys.length > 1) {
        payload.walletKeys = manualWalletKeys;
      }
    }
    if (tokenContext?.mint) {
      payload.mint = tokenContext.mint;
      payload.includeSolBalance = true;
      payload.includeUsd1Balance = shouldIncludeUsd1BalanceForWalletStatus({ tokenContext });
    }
    if (tokenContext?.surface) {
      payload.surface = tokenContext.surface;
    }
    if (tokenContext?.url) {
      payload.pageUrl = tokenContext.url;
    }
    if (tokenContext?.source) {
      payload.source = tokenContext.source;
    }
    applyWalletStatusRouteHints(payload, tokenContext);
    if (force) {
      payload.force = true;
    }
    return payload;
  }

  function applyWalletStatusRouteHints(payload, tokenContext = null) {
    if (!payload || !tokenContext) {
      return;
    }
    const routeAddress = getTokenContextRouteAddress(tokenContext);
    const pair = getTokenContextPairAddress(tokenContext);
    if (routeAddress) {
      payload.routeAddress = routeAddress;
    }
    if (pair) {
      payload.pair = pair;
    }
    const prewarmResponse = readCachedPrewarmResponse(tokenContext);
    const warmKey = String(prewarmResponse?.warmKey || tokenContext?.warmKey || "").trim();
    if (warmKey) {
      payload.warmKey = warmKey;
    }
    const routeFields = [
      ["family", prewarmResponse?.family ?? tokenContext?.family],
      ["lifecycle", prewarmResponse?.lifecycle ?? tokenContext?.lifecycle],
      ["quoteAsset", prewarmResponse?.quoteAsset ?? prewarmResponse?.quote_asset ?? tokenContext?.quoteAsset ?? tokenContext?.quote_asset],
      [
        "canonicalMarketKey",
        prewarmResponse?.canonicalMarketKey ??
          prewarmResponse?.canonical_market_key ??
          tokenContext?.canonicalMarketKey ??
          tokenContext?.canonical_market_key
      ]
    ];
    routeFields.forEach(([key, value]) => {
      const normalized = String(value || "").trim();
      if (normalized) {
        payload[key] = normalized;
      }
    });
  }

  function buildPnlHistoryScopePayload(tokenContext = null) {
    const resolvedTokenContext = tokenContext || currentActivePanelTokenContext() || state.tokenContext || null;
    const mint = String(resolvedTokenContext?.mint || state.walletStatus?.mint || "").trim();
    const payload = { mint };
    const selection = normalizeWalletSelectionPreference(state.preferences);
    if (selection.selectionSource === "group") {
      payload.walletGroupId = requireSelectedWalletGroupId(selection);
    } else {
      const manualWalletKeys = Array.from(new Set((selection.manualWalletKeys || []).filter(Boolean)));
      if (manualWalletKeys.length === 1) {
        payload.walletKey = manualWalletKeys[0];
      } else if (manualWalletKeys.length > 1) {
        payload.walletKeys = manualWalletKeys;
      }
    }
    return payload;
  }

  function normalizePolicyValue(value) {
    return String(value || "")
      .trim()
      .toLowerCase()
      .replace(/[-\s]+/g, "_");
  }

  function backendPolicyValue(value) {
    const normalized = normalizePolicyValue(value);
    return normalized === "prefer_usd1_else_topup"
      ? "prefer_usd1_else_top_up"
      : normalized;
  }

  function activePolicyDefaultValue(kind) {
    const settings = state.bootstrap?.settings || {};
    const defaults = state.bootstrap?.config?.defaults?.misc || {};
    return normalizePolicyValue(
      kind === "buy"
        ? (defaults.defaultBuyFundingPolicy ?? settings.defaultBuyFundingPolicy)
        : (defaults.defaultSellSettlementPolicy ?? settings.defaultSellSettlementPolicy)
    );
  }

  function activeCanonicalPresetConfig() {
    const presetId = String(getActivePreset()?.id || state.preferences?.presetId || "").trim();
    const items = Array.isArray(state.bootstrap?.config?.presets?.items)
      ? state.bootstrap.config.presets.items
      : [];
    return items.find((item) => String(item?.id || "").trim() === presetId) || null;
  }

  function activePolicyValue(kind) {
    const preset = getActivePreset();
    const fallbackDefault = activePolicyDefaultValue(kind);
    const canonicalPreset = activeCanonicalPresetConfig();
    if (kind === "buy") {
      const routePolicy = normalizePolicyValue(canonicalPreset?.buySettings?.buyFundingPolicy);
      if (routePolicy) {
        return routePolicy;
      }
      const presetPolicy = normalizePolicyValue(preset?.buyFundingPolicy);
      if (!preset?.buyFundingPolicyExplicit && presetPolicy === "sol_only") {
        return fallbackDefault || presetPolicy;
      }
      return presetPolicy || fallbackDefault;
    }
    const routePolicy = normalizePolicyValue(canonicalPreset?.sellSettings?.sellSettlementPolicy);
    if (routePolicy) {
      return routePolicy;
    }
    const presetPolicy = normalizePolicyValue(preset?.sellSettlementPolicy);
    if (!preset?.sellSettlementPolicyExplicit && presetPolicy === "always_to_sol") {
      return fallbackDefault || presetPolicy;
    }
    return presetPolicy || fallbackDefault;
  }

  function shouldIncludeUsd1BalanceForWalletStatus({ tokenContext = null } = {}) {
    if (!tokenContext?.mint) {
      return true;
    }
    const buyPolicy = activePolicyValue("buy");
    const sellPolicy = activePolicyValue("sell");
    if (!buyPolicy && !sellPolicy) {
      // Unknown bootstrap/config state: keep the previous full-balance behavior.
      return true;
    }
    return (
      buyPolicy === "usd1_only" ||
      buyPolicy === "prefer_usd1_else_topup" ||
      buyPolicy === "prefer_usd1_else_top_up" ||
      sellPolicy === "always_to_usd1" ||
      sellPolicy === "match_stored_entry_preference"
    );
  }

  function walletStatusDiffTouchesCurrentMint(diff) {
    if (!diff || typeof diff !== "object") {
      return true;
    }
    const currentMint = String(currentActivePanelTokenContext()?.mint || state.tokenContext?.mint || "").trim();
    const changedMints = Array.isArray(diff?.changedMints)
      ? diff.changedMints
        .map((value) => String(value || "").trim())
        .filter(Boolean)
      : [];
    if (changedMints.length > 0) {
      return !currentMint || changedMints.includes(currentMint);
    }
    const tokenMints = Array.isArray(diff?.tokenBalanceEntries)
      ? diff.tokenBalanceEntries
        .map((entry) => String(entry?.mint || "").trim())
        .filter(Boolean)
      : [];
    if (tokenMints.length > 0) {
      return !currentMint || tokenMints.includes(currentMint);
    }
    return false;
  }

  function normalizeWalletBalanceEntries(diff) {
    return Array.isArray(diff?.walletBalanceEntries)
      ? diff.walletBalanceEntries
        .map((entry) => ({
          envKey: String(entry?.envKey || "").trim(),
          balanceSol: Number(entry?.balanceSol),
          balanceLamports: Number(entry?.balanceLamports),
          usd1Balance: Number(entry?.usd1Balance),
          balanceError: typeof entry?.balanceError === "string" && entry.balanceError
            ? entry.balanceError
            : null,
        }))
        .filter((entry) => entry.envKey)
      : [];
  }

  function applyLiveWalletStatusBalanceDiff(diff) {
    const walletStatus = state.walletStatus;
    if (!walletStatus || !Array.isArray(walletStatus.wallets) || walletStatus.wallets.length === 0) {
      return false;
    }
    const entries = normalizeWalletBalanceEntries(diff);
    if (entries.length === 0) {
      return false;
    }
    const entryByWalletKey = new Map(entries.map((entry) => [entry.envKey, entry]));
    let applicable = false;
    let changed = false;
    const wallets = walletStatus.wallets.map((wallet) => {
      const walletKeys = Array.from(new Set([
        String(wallet?.key || "").trim(),
        String(wallet?.envKey || "").trim()
      ].filter(Boolean)));
      const entry = walletKeys.map((key) => entryByWalletKey.get(key)).find(Boolean);
      if (!entry) {
        return wallet;
      }
      applicable = true;
      const nextWallet = { ...wallet };
      let walletChanged = false;
      if (Number.isFinite(entry.balanceSol) && nextWallet.balanceSol !== entry.balanceSol) {
        nextWallet.balanceSol = entry.balanceSol;
        walletChanged = true;
      }
      if (Number.isFinite(entry.balanceLamports) && nextWallet.balanceLamports !== entry.balanceLamports) {
        nextWallet.balanceLamports = entry.balanceLamports;
        walletChanged = true;
      }
      if (Number.isFinite(entry.usd1Balance) && nextWallet.usd1Balance !== entry.usd1Balance) {
        nextWallet.usd1Balance = entry.usd1Balance;
        walletChanged = true;
      }
      if ((nextWallet.balanceError || null) !== entry.balanceError) {
        nextWallet.balanceError = entry.balanceError;
        walletChanged = true;
      }
      if (walletChanged) {
        changed = true;
      }
      return walletChanged ? nextWallet : wallet;
    });
    if (!applicable) {
      return false;
    }
    if (!changed) {
      return true;
    }
    const selectedWalletKeys = new Set(
      Array.isArray(walletStatus.walletKeys)
        ? walletStatus.walletKeys.map((value) => String(value || "").trim()).filter(Boolean)
        : []
    );
    const selectedWallets = wallets.filter((wallet) => {
      const walletKey = String(wallet?.key || wallet?.envKey || "").trim();
      return selectedWalletKeys.size === 0 || selectedWalletKeys.has(walletKey);
    });
    const balanceSol = selectedWallets.reduce((sum, wallet) => {
      const value = Number(wallet?.balanceSol);
      return sum + (Number.isFinite(value) ? value : 0);
    }, 0);
    const balanceLamports = selectedWallets.reduce((sum, wallet) => {
      const value = Number(wallet?.balanceLamports);
      return sum + (Number.isFinite(value) ? value : 0);
    }, 0);
    const usd1Balance = selectedWallets.reduce((sum, wallet) => {
      const value = Number(wallet?.usd1Balance);
      return sum + (Number.isFinite(value) ? value : 0);
    }, 0);
    state.walletStatus = {
      ...walletStatus,
      wallets,
      balanceSol,
      balanceLamports,
      usd1Balance,
    };
    pushPanelState();
    notifyPlatformWalletStatusChange();
    syncActiveMarkSubscription();
    return true;
  }

  function applyLiveWalletStatusMintDiff(diff) {
    const currentMint = String(
      currentActivePanelTokenContext()?.mint ||
      state.tokenContext?.mint ||
      state.walletStatus?.mint ||
      ""
    ).trim();
    const walletStatus = state.walletStatus;
    if (!currentMint || !walletStatus || !Array.isArray(walletStatus.wallets) || walletStatus.wallets.length === 0) {
      return false;
    }
    const walletStatusMint = String(walletStatus.mint || "").trim();
    if (walletStatusMint && walletStatusMint !== currentMint) {
      return false;
    }
    const tokenBalanceEntries = Array.isArray(diff?.tokenBalanceEntries)
      ? diff.tokenBalanceEntries
      : [];
    const numberFieldOrNull = (value) => (
      typeof value === "number" && Number.isFinite(value) ? value : null
    );
    const entriesForMint = tokenBalanceEntries
      .map((entry) => ({
        envKey: String(entry?.envKey || "").trim(),
        mint: String(entry?.mint || "").trim(),
        tokenBalance: numberFieldOrNull(entry?.tokenBalance),
        tokenBalanceRaw: numberFieldOrNull(entry?.tokenBalanceRaw),
        tokenDecimals: Number.isInteger(entry?.tokenDecimals) && entry.tokenDecimals >= 0
          ? entry.tokenDecimals
          : null,
      }))
      .filter((entry) => (
        entry.envKey &&
        entry.mint === currentMint &&
        (
          entry.tokenBalance != null ||
          entry.tokenBalanceRaw != null ||
          entry.tokenDecimals != null
        )
      ));
    if (entriesForMint.length === 0) {
      return false;
    }
    const balanceByWalletKey = new Map(entriesForMint.map((entry) => [entry.envKey, entry]));
    let changed = false;
    const wallets = walletStatus.wallets.map((wallet) => {
      const walletKeys = Array.from(new Set([
        String(wallet?.key || "").trim(),
        String(wallet?.envKey || "").trim()
      ].filter(Boolean)));
      const entry = walletKeys.map((key) => balanceByWalletKey.get(key)).find(Boolean);
      if (!entry) {
        return wallet;
      }
      const nextTokenBalance = entry.tokenBalance;
      const nextTokenBalanceRaw = entry.tokenBalanceRaw;
      const nextTokenDecimals = entry.tokenDecimals;
      const prevComparable = Number(
        wallet?.mintBalanceUi ??
        wallet?.mintBalance ??
        wallet?.tokenBalance ??
        wallet?.holdingAmount
      );
      const prevRaw = Number(wallet?.mintBalanceRaw ?? wallet?.tokenBalanceRaw);
      const prevDecimals = Number(wallet?.tokenDecimals ?? wallet?.mintDecimals);
      const uiUnchanged = nextTokenBalance == null ||
        (Number.isFinite(prevComparable) && prevComparable === nextTokenBalance);
      const rawUnchanged = nextTokenBalanceRaw == null ||
        (Number.isFinite(prevRaw) && prevRaw === nextTokenBalanceRaw);
      const decimalsUnchanged = nextTokenDecimals == null ||
        (Number.isInteger(prevDecimals) && prevDecimals === nextTokenDecimals);
      if (uiUnchanged && rawUnchanged && decimalsUnchanged) {
        return wallet;
      }
      changed = true;
      const nextWallet = {
        ...wallet,
        mint: currentMint,
      };
      if (nextTokenBalance != null) {
        nextWallet.mintBalance = nextTokenBalance;
        nextWallet.mintBalanceUi = nextTokenBalance;
        nextWallet.tokenBalance = nextTokenBalance;
        nextWallet.holdingAmount = nextTokenBalance;
        if (nextTokenBalance === 0 && nextTokenBalanceRaw == null) {
          nextWallet.mintBalanceRaw = 0;
          nextWallet.tokenBalanceRaw = 0;
        }
      }
      if (nextTokenBalanceRaw != null) {
        nextWallet.mintBalanceRaw = nextTokenBalanceRaw;
        nextWallet.tokenBalanceRaw = nextTokenBalanceRaw;
      }
      if (nextTokenDecimals != null) {
        nextWallet.mintDecimals = nextTokenDecimals;
        nextWallet.tokenDecimals = nextTokenDecimals;
      }
      return nextWallet;
    });
    if (!changed) {
      return true;
    }
    const selectedWalletKeys = new Set(
      Array.isArray(walletStatus.walletKeys)
        ? walletStatus.walletKeys.map((value) => String(value || "").trim()).filter(Boolean)
        : []
    );
    const selectedWallets = wallets.filter((wallet) => {
      const walletKey = String(wallet?.key || wallet?.envKey || "").trim();
      return selectedWalletKeys.size === 0 || selectedWalletKeys.has(walletKey);
    });
    const aggregateMintUi = selectedWallets.reduce((sum, wallet) => {
      const tokenBalance = Number(
        wallet?.mintBalanceUi ??
        wallet?.mintBalance ??
        wallet?.tokenBalance ??
        wallet?.holdingAmount
      );
      return sum + (Number.isFinite(tokenBalance) ? tokenBalance : 0);
    }, 0);
    const selectedMintUiKnown = selectedWallets.some((wallet) => {
      const tokenBalance = Number(
        wallet?.mintBalanceUi ??
        wallet?.mintBalance ??
        wallet?.tokenBalance ??
        wallet?.holdingAmount
      );
      return Number.isFinite(tokenBalance);
    });
    const aggregateMintRaw = selectedWallets.reduce((sum, wallet) => {
      const raw = Number(wallet?.mintBalanceRaw ?? wallet?.tokenBalanceRaw);
      return sum + (Number.isFinite(raw) ? raw : 0);
    }, 0);
    const selectedMintRawKnown = selectedWallets.some((wallet) => {
      const raw = Number(wallet?.mintBalanceRaw ?? wallet?.tokenBalanceRaw);
      return Number.isFinite(raw);
    });
    const aggregateMintDecimals = selectedWallets
      .map((wallet) => Number(wallet?.tokenDecimals ?? wallet?.mintDecimals))
      .find((decimals) => Number.isInteger(decimals) && decimals >= 0);
    const previousAmount = Number(walletStatus.holdingAmount ?? walletStatus.tokenBalance ?? walletStatus.mintBalanceUi);
    const previousHoldingValueSol = Number(walletStatus.holdingValueSol);
    const holdingValueSol = selectedMintUiKnown
      ? (
        aggregateMintUi === 0
          ? 0
          : (
            Number.isFinite(previousAmount) &&
            previousAmount === aggregateMintUi &&
            Number.isFinite(previousHoldingValueSol)
              ? previousHoldingValueSol
              : null
          )
      )
      : walletStatus.holdingValueSol;
    const shouldRefreshQuote = selectedMintUiKnown && aggregateMintUi > 0 && holdingValueSol == null;
    const trackedBoughtSol = Number(walletStatus.trackedBoughtSol);
    const remainingCostBasisSol = Number(walletStatus.remainingCostBasisSol);
    const explicitFeeTotalSol = Number(walletStatus.explicitFeeTotalSol);
    const realizedPnlGrossSol = Number(walletStatus.realizedPnlGrossSol);
    const nextUnrealizedPnlGrossSol =
      holdingValueSol != null && Number.isFinite(remainingCostBasisSol)
        ? holdingValueSol - remainingCostBasisSol
        : null;
    const nextPnlGross =
      Number.isFinite(realizedPnlGrossSol) && Number.isFinite(nextUnrealizedPnlGrossSol)
        ? realizedPnlGrossSol + nextUnrealizedPnlGrossSol
        : null;
    const nextPnlNet =
      Number.isFinite(nextPnlGross) && Number.isFinite(explicitFeeTotalSol)
        ? nextPnlGross - explicitFeeTotalSol
        : null;
    const nextPnlPercentGross =
      Number.isFinite(trackedBoughtSol) &&
      trackedBoughtSol > 0 &&
      Number.isFinite(nextPnlGross)
        ? (nextPnlGross / trackedBoughtSol) * 100
        : null;
    const nextPnlPercentNet =
      Number.isFinite(trackedBoughtSol) &&
      trackedBoughtSol > 0 &&
      Number.isFinite(nextPnlNet)
        ? (nextPnlNet / trackedBoughtSol) * 100
        : null;
    state.walletStatus = {
      ...walletStatus,
      mint: currentMint,
      wallets,
      mintBalance: selectedMintUiKnown ? aggregateMintUi : walletStatus.mintBalance,
      mintBalanceUi: selectedMintUiKnown ? aggregateMintUi : walletStatus.mintBalanceUi,
      mintBalanceRaw: selectedMintRawKnown ? aggregateMintRaw : walletStatus.mintBalanceRaw,
      tokenBalance: selectedMintUiKnown ? aggregateMintUi : walletStatus.tokenBalance,
      tokenBalanceRaw: selectedMintRawKnown ? aggregateMintRaw : walletStatus.tokenBalanceRaw,
      tokenDecimals: Number.isInteger(aggregateMintDecimals) ? aggregateMintDecimals : walletStatus.tokenDecimals,
      mintDecimals: Number.isInteger(aggregateMintDecimals) ? aggregateMintDecimals : walletStatus.mintDecimals,
      holdingAmount: selectedMintUiKnown ? aggregateMintUi : walletStatus.holdingAmount,
      holdingValueSol,
      holding: holdingValueSol,
      unrealizedPnlGrossSol: nextUnrealizedPnlGrossSol,
      unrealizedPnlNetSol: nextUnrealizedPnlGrossSol,
      pnlGross: nextPnlGross,
      pnlNet: nextPnlNet,
      pnlPercentGross: nextPnlPercentGross,
      pnlPercentNet: nextPnlPercentNet,
      pnlRequiresQuote:
        selectedMintUiKnown
          ? aggregateMintUi > 0 && (shouldRefreshQuote || walletStatus.pnlRequiresQuote === true)
          : walletStatus.pnlRequiresQuote,
    };
    pushPanelState();
    notifyPlatformWalletStatusChange();
    syncActiveMarkSubscription();
    syncWalletStatusQuoteRefresh();
    if (shouldRefreshQuote) {
      scheduleWalletStatusQuoteRefresh(WALLET_STATUS_FAST_QUOTE_REFRESH_MS, { reset: true });
    }
    return true;
  }

  function sortedStringList(values) {
    return (Array.isArray(values) ? values : [])
      .map((value) => String(value || "").trim())
      .filter(Boolean)
      .sort();
  }

  function sameStringList(left, right) {
    const a = sortedStringList(left);
    const b = sortedStringList(right);
    return a.length === b.length && a.every((value, index) => value === b[index]);
  }

  function applyLiveWalletStatusMarkDiff(diff) {
    if (!diff || typeof diff !== "object" || !state.walletStatus) {
      return false;
    }
    const diffSurfaceId = String(diff.surfaceId || "").trim();
    if (diffSurfaceId && diffSurfaceId !== ACTIVE_MINTS_SURFACE_ID) {
      return false;
    }
    const currentMint = String(
      currentActivePanelTokenContext()?.mint ||
      state.tokenContext?.mint ||
      state.walletStatus?.mint ||
      ""
    ).trim();
    const diffMint = String(diff.mint || "").trim();
    if (!currentMint || !diffMint || currentMint !== diffMint) {
      return false;
    }
    const walletStatusMint = String(state.walletStatus?.mint || "").trim();
    if (walletStatusMint && walletStatusMint !== diffMint) {
      return false;
    }
    if (!sameStringList(state.walletStatus?.walletKeys, diff.walletKeys)) {
      return false;
    }
    const currentGroup = String(state.walletStatus?.walletGroupId || "").trim();
    const diffGroup = String(diff.walletGroupId || "").trim();
    if (currentGroup !== diffGroup) {
      return false;
    }
    const holdingValueSol = Number(diff.holdingValueSol ?? diff.holding);
    const pnlGross = Number(diff.pnlGross);
    const pnlNet = Number(diff.pnlNet);
    const tokenBalance = Number(diff.tokenBalance);
    const tokenBalanceRaw = Number(diff.tokenBalanceRaw);
    state.walletStatus = {
      ...state.walletStatus,
      holdingValueSol: Number.isFinite(holdingValueSol) ? holdingValueSol : state.walletStatus.holdingValueSol,
      holding: Number.isFinite(holdingValueSol) ? holdingValueSol : state.walletStatus.holding,
      pnlGross: Number.isFinite(pnlGross) ? pnlGross : state.walletStatus.pnlGross,
      pnlNet: Number.isFinite(pnlNet) ? pnlNet : state.walletStatus.pnlNet,
      pnlPercentGross: Number.isFinite(Number(diff.pnlPercentGross))
        ? Number(diff.pnlPercentGross)
        : state.walletStatus.pnlPercentGross,
      pnlPercentNet: Number.isFinite(Number(diff.pnlPercentNet))
        ? Number(diff.pnlPercentNet)
        : state.walletStatus.pnlPercentNet,
      tokenBalance: Number.isFinite(tokenBalance) ? tokenBalance : state.walletStatus.tokenBalance,
      mintBalance: Number.isFinite(tokenBalance) ? tokenBalance : state.walletStatus.mintBalance,
      mintBalanceUi: Number.isFinite(tokenBalance) ? tokenBalance : state.walletStatus.mintBalanceUi,
      holdingAmount: Number.isFinite(tokenBalance) ? tokenBalance : state.walletStatus.holdingAmount,
      tokenBalanceRaw: Number.isFinite(tokenBalanceRaw) ? tokenBalanceRaw : state.walletStatus.tokenBalanceRaw,
      mintBalanceRaw: Number.isFinite(tokenBalanceRaw) ? tokenBalanceRaw : state.walletStatus.mintBalanceRaw,
      holdingQuoteSource: diff.quoteSource || state.walletStatus.holdingQuoteSource,
      holdingQuoteAsset: "SOL",
      holdingQuoteAgeMs: 0,
      holdingQuoteError: null,
      pnlRequiresQuote: false,
    };
    pushPanelState();
    notifyPlatformWalletStatusChange();
    return true;
  }

  async function resolveWalletStatusTokenContext(tokenContext = null) {
    const baseTokenContext = tokenContext || currentActivePanelTokenContext() || state.tokenContext || null;
    if (!baseTokenContext) {
      return null;
    }
    if (String(baseTokenContext?.mint || "").trim()) {
      return baseTokenContext;
    }
    const routeAddress = getTokenContextRouteAddress(baseTokenContext);
    if (!routeAddress || !baseTokenContext?.surface) {
      return baseTokenContext;
    }
    const resolved = await resolveInlineTokenInternal(
      {
        address: routeAddress,
        surface: baseTokenContext.surface,
        url: baseTokenContext.url || window.location.href,
        source: baseTokenContext.source || "page"
      },
      baseTokenContext.surface,
      baseTokenContext.url || window.location.href,
      { silent: true }
    );
    if (!resolved) {
      return baseTokenContext;
    }
    const nextTokenContext = {
      ...baseTokenContext,
      ...resolved,
      url: baseTokenContext.url || resolved.url || window.location.href
    };
    if (state.quickPanelOpen && state.quickPanelTokenContext === baseTokenContext) {
      setQuickPanelTokenContext(nextTokenContext);
    } else if (state.panelOpen && state.panelTokenContext === baseTokenContext) {
      setPersistentPanelTokenContext(nextTokenContext);
    } else if (state.tokenContext === baseTokenContext) {
      setPageTokenContext(nextTokenContext);
    }
    return nextTokenContext;
  }

  async function refreshPanelWalletStatus({ tokenContext = null, force = false } = {}) {
    const requestSeq = ++state.walletStatusRequestSeq;
    try {
      const resolvedTokenContext = await resolveWalletStatusTokenContext(tokenContext);
      const expectedMint = String(resolvedTokenContext?.mint || "").trim();
      const nextWalletStatus = await callBackground(
        "trench:get-wallet-status",
        buildWalletStatusRequestPayload({
          tokenContext: resolvedTokenContext,
          force
        })
      );
      if (requestSeq !== state.walletStatusRequestSeq) {
        return state.walletStatus;
      }
      // Mint is the only dimension of the token context that actually scopes
      // the wallet-status payload (aggregate_trade_ledger keys by
      // (wallet_key, mint)). Comparing the whole `surface|mint|url` key here
      // would discard valid responses whenever the URL query changes but the
      // coin doesn't, so we compare mints directly against whatever is
      // active in the panel right now.
      const currentMint =
        String((currentActivePanelTokenContext() || state.tokenContext)?.mint || "").trim();
      if (expectedMint && currentMint && expectedMint !== currentMint) {
        return state.walletStatus;
      }
      // Backend echoes the requested mint back on the payload; if it ever
      // disagrees with the mint we asked about OR the mint we're currently
      // showing, drop the response on the floor.
      const responseMint = String(nextWalletStatus?.mint || "").trim();
      if (expectedMint && responseMint && responseMint !== expectedMint) {
        return state.walletStatus;
      }
      if (responseMint && currentMint && responseMint !== currentMint) {
        return state.walletStatus;
      }
      state.walletStatus = nextWalletStatus;
      state.hostError = "";
      pushPanelState();
      notifyPlatformWalletStatusChange();
      syncActiveMarkSubscription();
      syncWalletStatusQuoteRefresh();
      return state.walletStatus;
    } catch (error) {
      if (requestSeq !== state.walletStatusRequestSeq) {
        return state.walletStatus;
      }
      state.hostError = isHostAvailabilityError(error) ? userFacingErrorMessage(error) : "";
      surfaceUserFacingError(error, { pushToPanel: true, toast: false });
      pushPanelState();
      syncWalletStatusQuoteRefresh();
      return null;
    }
  }

  async function handlePnlHistoryAction(action) {
    const tokenContext = currentActivePanelTokenContext() || state.tokenContext || null;
    const payload = buildPnlHistoryScopePayload(tokenContext);
    if (!String(payload.mint || "").trim()) {
      throw new Error("Open a token page before using PnL history actions.");
    }
    if (action === "resync") {
      await callBackground("trench:resync-pnl-history", payload);
    } else if (action === "reset") {
      await callBackground("trench:reset-pnl-history", payload);
    } else {
      return;
    }
    await refreshPanelWalletStatus({ tokenContext, force: true });
  }

  async function refreshBootstrap(showFailureToast = false) {
    try {
      state.bootstrap = normalizeExtensionBootstrap(await callBackground("trench:get-bootstrap"));
      state.hostError = "";
      hydrateDefaultSelections();
      pushPanelState();
      dismissOpenSurfacesIfPresetsEmpty();
      return state.bootstrap;
    } catch (error) {
      state.hostError = isHostAvailabilityError(error) ? userFacingErrorMessage(error) : "";
      if (showFailureToast) {
        surfaceUserFacingError(error);
      }
      pushPanelState();
      return null;
    }
  }

  function dismissOpenSurfacesIfPresetsEmpty() {
    const presets = Array.isArray(state.bootstrap?.presets)
      ? state.bootstrap.presets.filter((preset) => String(preset?.id || "").trim())
      : [];
    if (presets.length) {
      return;
    }
    const hadOpenSurface = state.panelOpen || state.quickPanelOpen;
    if (state.quickPanelOpen) {
      closeQuickPanel();
    }
    if (state.panelOpen) {
      closePanel();
    }
    if (hadOpenSurface) {
      showMissingExecutionPresetToast(
        "Last execution preset was removed. Click here to create a new one."
      );
    }
  }

  function dismissOpenSurfacesIfPlatformDisabled() {
    let enabled = true;
    try {
      enabled = isPlatformEnabled();
    } catch {
      return;
    }
    if (enabled) {
      return;
    }
    const hadOpenSurface = state.panelOpen || state.quickPanelOpen;
    if (state.quickPanelOpen) {
      closeQuickPanel();
    }
    if (state.panelOpen) {
      closePanel();
    }
    if (hadOpenSurface) {
      showToast(
        `${platform} integration disabled. Re-enable it from Trench Tools options.`,
        "info"
      );
    }
  }

  function hydrateDefaultSelections() {
    const presets = state.bootstrap?.presets || [];
    const wallets = state.bootstrap?.wallets || [];
    const walletGroups = state.bootstrap?.walletGroups || [];

    if (!state.preferences.presetId && presets[0]) {
      state.preferences.presetId = presets[0].id;
    }
    const selection = normalizeWalletSelectionPreference(state.preferences);
    if (walletGroups[0] && !selection.activeWalletGroupId) {
      selection.activeWalletGroupId = walletGroups[0].id;
    }
    if (selection.selectionSource === "group") {
      const knownGroupIds = new Set(walletGroups.map((group) => group.id));
      if (!knownGroupIds.has(selection.activeWalletGroupId)) {
        if (walletGroups[0]) {
          selection.activeWalletGroupId = walletGroups[0].id;
        } else if (wallets[0]) {
          selection.selectionSource = "manual";
          selection.manualWalletKeys = [wallets[0].key];
        }
      }
    } else {
      const knownWalletKeys = new Set(wallets.map((wallet) => wallet.key));
      selection.manualWalletKeys = selection.manualWalletKeys.filter((key) => knownWalletKeys.has(key));
    }
    mirrorWalletSelectionPreferenceOntoPreferences(state.preferences, selection);
    state.preferences = normalizePreferencesValue(state.preferences);
  }

  function openOptionsSection(section) {
    return callBackground("trench:open-options", { section });
  }

  function openPresetSettingsSection() {
    return openOptionsSection("presets");
  }

  function openGlobalSettingsSection() {
    return openOptionsSection("global");
  }

  function showMissingExecutionPresetToast(message, linkMatch = "Click here") {
    const hasMatch = typeof message === "string" && message.includes(linkMatch);
    renderToast({
      id: "execution-preset-required",
      title: message,
      kind: "error",
      ttlMs: 5200,
      titleLink: hasMatch
        ? {
            match: linkMatch,
            onClick: () => {
              void openPresetSettingsSection().catch((error) => {
                surfaceUserFacingError(error, { toast: false });
              });
            }
          }
        : null
    });
  }

  function isBootstrapLoaded() {
    return state.bootstrap !== null && Array.isArray(state.bootstrap?.presets);
  }

  function resolveExecutionPresetSelection() {
    if (!isBootstrapLoaded()) {
      return { preset: null, repaired: false, bootstrapLoaded: false };
    }
    const presets = state.bootstrap.presets.filter((preset) => String(preset?.id || "").trim());
    if (!presets.length) {
      return { preset: null, repaired: false, bootstrapLoaded: true };
    }
    const requestedPresetId = String(state.preferences?.presetId || "").trim();
    const selectedPreset = presets.find((preset) => preset.id === requestedPresetId) || null;
    if (selectedPreset) {
      return { preset: selectedPreset, repaired: false, bootstrapLoaded: true };
    }
    return {
      preset: presets[0],
      repaired: requestedPresetId !== presets[0].id,
      bootstrapLoaded: true
    };
  }

  async function ensureValidExecutionPreset({
    missingMessage = "No valid preset saved. Click here to create a preset",
    pushToPanel = false
  } = {}) {
    if (!isBootstrapLoaded()) {
      await refreshBootstrap(true);
    }
    const resolution = resolveExecutionPresetSelection();
    if (!resolution.bootstrapLoaded) {
      const message = state.hostError || "Execution engine offline.";
      if (pushToPanel) {
        pushPanelError(message, {
          title: "Execution engine unavailable",
          kind: "error",
          source: "host"
        });
      }
      if (!state.hostError) {
        renderToast({
          id: "execution-engine-unavailable",
          title: "Execution engine unavailable",
          detail: message,
          kind: "error",
          ttlMs: 6200,
          actionLabel: "Open settings",
          actionHandler: () => {
            void openGlobalSettingsSection().catch((error) => {
              surfaceUserFacingError(error, { toast: false });
            });
          }
        });
      }
      return null;
    }
    if (!resolution.preset) {
      if (pushToPanel) {
        pushPanelError(missingMessage, {
          title: "Preset required",
          kind: "error",
          source: "notice"
        });
      }
      showMissingExecutionPresetToast(missingMessage);
      return null;
    }
    if (resolution.repaired) {
      await savePreferences({ presetId: resolution.preset.id });
    }
    return resolution.preset;
  }

  function ensureValidExecutionPresetSync({
    missingMessage = "No valid preset saved. Click here to create a preset",
    showFailureToast = true
  } = {}) {
    const resolution = resolveExecutionPresetSelection();
    if (!resolution.bootstrapLoaded) {
      void refreshBootstrap(showFailureToast);
      return null;
    }
    if (!resolution.preset) {
      showMissingExecutionPresetToast(missingMessage);
      return null;
    }
    if (resolution.repaired) {
      state.preferences = normalizePreferencesValue({
        ...state.preferences,
        presetId: resolution.preset.id
      });
      pushPanelState();
      void safeStorageSet({
        [PREFERENCES_KEY]: serializePreferencesForStorage(state.preferences)
      });
    }
    return resolution.preset;
  }

  function showMissingLaunchdeckPresetToast(
    message = "No valid LaunchDeck preset saved. Click here to create a preset",
    linkMatch = "Click here"
  ) {
    const hasMatch = typeof message === "string" && message.includes(linkMatch);
    renderToast({
      id: "launchdeck-preset-required",
      title: message,
      kind: "error",
      ttlMs: 5200,
      titleLink: hasMatch
        ? {
            match: linkMatch,
            onClick: () => {
              void openPresetSettingsSection().catch((error) => {
                surfaceUserFacingError(error, { toast: false });
              });
            }
          }
        : null
    });
  }

  function showLaunchdeckConnectionToast(message) {
    renderToast({
      id: "launchdeck-connection-required",
      title: message,
      kind: "error",
      ttlMs: 6200,
      actionLabel: "Open settings",
      actionHandler: () => {
        void openGlobalSettingsSection().catch((error) => {
          surfaceUserFacingError(error, { toast: false });
        });
      }
    });
  }

  function isUnknownBackgroundMessageError(error, type) {
    return String(error?.message || "").includes(`Unknown message type: ${type}`);
  }

  async function fetchLaunchdeckHostSettingsForExtension() {
    try {
      return await callBackground("trench:get-launchdeck-host-settings");
    } catch (error) {
      if (!isUnknownBackgroundMessageError(error, "trench:get-launchdeck-host-settings")) {
        throw error;
      }
      return callBackground("trench:get-launchdeck-settings");
    }
  }

  let launchdeckPresetCheckPromise = null;
  let launchdeckPresetCheckCachedAt = 0;
  let launchdeckPresetCheckCachedPayload = null;
  const LAUNCHDECK_PRESET_CHECK_TTL_MS = 15_000;

  function invalidateLaunchdeckPresetCache() {
    launchdeckPresetCheckPromise = null;
    launchdeckPresetCheckCachedAt = 0;
    launchdeckPresetCheckCachedPayload = null;
  }

  async function ensureValidLaunchdeckPresetForExtension() {
    const now = Date.now();
    if (
      launchdeckPresetCheckCachedPayload
      && now - launchdeckPresetCheckCachedAt < LAUNCHDECK_PRESET_CHECK_TTL_MS
    ) {
      return launchdeckPresetCheckCachedPayload;
    }
    if (!launchdeckPresetCheckPromise) {
      launchdeckPresetCheckPromise = (async () => {
        try {
          const payload = await fetchLaunchdeckHostSettingsForExtension();
          const presets = Array.isArray(payload?.config?.presets?.items)
            ? payload.config.presets.items.filter((preset) => String(preset?.id || "").trim())
            : [];
          if (!presets.length) {
            showMissingLaunchdeckPresetToast(
              "No LaunchDeck deploy preset saved. Click here to create one.",
            );
            return null;
          }
          launchdeckPresetCheckCachedPayload = payload;
          launchdeckPresetCheckCachedAt = Date.now();
          return payload;
        } catch (error) {
          if (error?.code === "LAUNCHDECK_NOT_CONFIGURED") {
            showLaunchdeckConnectionToast(
              error.message || "LaunchDeck host is not configured. Open settings to connect."
            );
            return null;
          }
          throw error;
        } finally {
          launchdeckPresetCheckPromise = null;
        }
      })();
    }
    return launchdeckPresetCheckPromise;
  }

  function detectPlatform() {
    return contentRuntime.detectPlatform(window.location.hostname);
  }

  function isPlatformEnabled() {
    return Boolean(getPlatformAdapter()?.isEnabled(state.siteFeatures));
  }

  function currentLauncherTokenContext() {
    if (state.tokenContext?.mint && state.tokenContext?.surface) {
      return state.tokenContext;
    }
    try {
      return getPlatformAdapter()?.getCurrentTokenCandidate?.() || null;
    } catch {
      return null;
    }
  }

  function shouldMountLauncher(tokenContext = currentLauncherTokenContext()) {
    return Boolean(getPlatformAdapter()?.shouldMountLauncher(state.siteFeatures, tokenContext));
  }

  function ensureFloatingLauncher(tokenContext = currentLauncherTokenContext()) {
    if (!shouldMountLauncher(tokenContext)) {
      if (state.launcherButton) {
        state.launcherButton.remove();
        state.launcherButton = null;
      }
      return;
    }

    if (document.getElementById("trench-tools-floating-launcher")) {
      if (state.launcherButton) {
        state.launcherButton.style.display = state.panelOpen ? "none" : "flex";
      }
      return;
    }

    const button = document.createElement("button");
    button.id = "trench-tools-floating-launcher";
    button.type = "button";
    const image = document.createElement("img");
    image.src = LOGO_URL;
    image.alt = "Trench Tools";
    Object.assign(image.style, {
      width: "22px",
      height: "22px",
      objectFit: "contain",
      pointerEvents: "none"
    });
    Object.assign(button.style, {
      position: "fixed",
      right: "18px",
      bottom: "18px",
      zIndex: PANEL_Z_INDEX.DEFAULT,
      width: "48px",
      height: "48px",
      border: "1px solid rgba(255, 255, 255, 0.16)",
      borderRadius: "999px",
      background: "#000000",
      color: "#ffffff",
      fontSize: "14px",
      fontWeight: "700",
      cursor: "pointer",
      boxShadow: "0 10px 24px rgba(0, 0, 0, 0.35)",
      display: "none",
      alignItems: "center",
      justifyContent: "center",
      transition: "all 0.2s ease"
    });
    button.addEventListener("mouseenter", () => {
      button.style.background = "#18181b";
      button.style.transform = "scale(1.06)";
    });
    button.addEventListener("mouseleave", () => {
      button.style.background = "#000000";
      button.style.transform = "scale(1)";
    });
    button.addEventListener("click", () => {
      void (async () => {
        const tokenContext = await refreshCurrentToken();
        if (!tokenContext) {
          showToast("No token detected on this surface yet.", "error");
          return;
        }
        openPanel(tokenContext, { requireValidPreset: true });
      })().catch((error) => {
        surfaceUserFacingError(error);
      });
    });
    button.appendChild(image);
    button.style.display = state.panelOpen ? "none" : "flex";
    document.documentElement.appendChild(button);
    state.launcherButton = button;
  }

  function ensurePanelFrame() {
    if (state.panelWrapper && state.panelFrame) {
      return;
    }

    const wrapper = document.createElement("div");
    wrapper.id = "trench-tools-panel-wrapper";
    Object.assign(wrapper.style, {
      position: "fixed",
      top: "0",
      left: "0",
      display: "none",
      width: "375px",
      height: "430px",
      backgroundColor: "#000000",
      overflow: "hidden",
      zIndex: PANEL_Z_INDEX.DEFAULT,
      pointerEvents: "auto",
      border: "0",
      borderRadius: "12px",
      boxShadow: "0 20px 60px rgba(0, 0, 0, 0.42)"
    });

    const iframe = document.createElement("iframe");
    iframe.src = buildPanelIframeUrl("persistent");
    iframe.title = "Trench Tools Panel";
    iframe.allow = "clipboard-read; clipboard-write";
    Object.assign(iframe.style, {
      width: "100%",
      height: "100%",
      border: "0",
      margin: "0",
      padding: "0",
      display: "block",
      pointerEvents: "auto",
      background: "#050505"
    });

    wrapper.appendChild(iframe);
    document.documentElement.appendChild(wrapper);
    state.panelWrapper = wrapper;
    state.panelFrame = iframe;
    attachPanelResizeHandle(wrapper);
    applyPanelShellMetrics();
    ensurePanelLayerObserver(wrapper);
  }

  function ensureQuickPanelFrame() {
    if (state.quickPanelWrapper && state.quickPanelFrame) {
      return;
    }

    const wrapper = document.createElement("div");
    wrapper.id = "trench-tools-quick-panel-wrapper";
    Object.assign(wrapper.style, {
      position: "fixed",
      width: `${QUICK_PANEL_DEFAULT_WIDTH}px`,
      maxWidth: "calc(100vw - 24px)",
      height: `${QUICK_PANEL_DEFAULT_HEIGHT}px`,
      maxHeight: "calc(100vh - 24px)",
      borderRadius: "12px",
      overflow: "hidden",
      boxShadow: "0 20px 60px rgba(0, 0, 0, 0.42)",
      zIndex: PANEL_Z_INDEX.DEFAULT,
      display: "none",
      background: "#000000",
      border: "0"
    });

    const iframe = document.createElement("iframe");
    iframe.src = buildPanelIframeUrl("quick");
    iframe.title = "Trench Tools Quick Panel";
    iframe.allow = "clipboard-read; clipboard-write";
    Object.assign(iframe.style, {
      width: "100%",
      height: "100%",
      border: "0",
      background: "#0a0f16"
    });

    wrapper.appendChild(iframe);
    document.documentElement.appendChild(wrapper);
    state.quickPanelWrapper = wrapper;
    state.quickPanelFrame = iframe;
    ensurePanelLayerObserver(wrapper);
  }

  function shouldIgnorePanelLayerCandidate(element) {
    const role = element.getAttribute("role");
    return Boolean(
      role === "tooltip" ||
      role === "menu" ||
      role === "listbox" ||
      element.hasAttribute("data-popper-placement")
    );
  }

  function panelLayerRectsIntersect(target, candidate) {
    const targetRect = target.getBoundingClientRect();
    const candidateRect = candidate.getBoundingClientRect();
    if (candidateRect.width === 0 || candidateRect.height === 0) {
      return false;
    }
    return (
      targetRect.left < candidateRect.right &&
      targetRect.right > candidateRect.left &&
      targetRect.top < candidateRect.bottom &&
      targetRect.bottom > candidateRect.top
    );
  }

  function hasOverlappingAxiomLayerOverlay(target) {
    if (!(target instanceof HTMLElement)) {
      return false;
    }
    return Array.from(document.querySelectorAll(AXIOM_PANEL_LAYER_OVERLAY_SELECTOR))
      .filter((candidate) => (
        candidate instanceof HTMLElement &&
        candidate !== target &&
        !target.contains(candidate) &&
        !shouldIgnorePanelLayerCandidate(candidate) &&
        panelLayerRectsIntersect(target, candidate)
      ))
      .length > 0;
  }

  function applyPanelLayeringForTarget(target) {
    if (!(target instanceof HTMLElement)) {
      return;
    }
    const lowered = hasOverlappingAxiomLayerOverlay(target);
    const defaultZIndex =
      target.id === "trench-tools-panel-flyout-overlay"
        ? PANEL_Z_INDEX.FLYOUT
        : PANEL_Z_INDEX.DEFAULT;
    const loweredZIndex =
      target.id === "trench-tools-panel-flyout-overlay"
        ? PANEL_Z_INDEX.LOWERED_FLYOUT
        : PANEL_Z_INDEX.LOWERED;
    target.style.zIndex = lowered ? loweredZIndex : defaultZIndex;
  }

  function applyPanelLayering() {
    if (platform !== "axiom") {
      return;
    }
    [state.panelWrapper, state.quickPanelWrapper, state.panelFlyoutOverlay]
      .filter((target) => target instanceof HTMLElement)
      .forEach((target) => applyPanelLayeringForTarget(target));
  }

  function panelLayerMutationTouchesCandidate(mutations) {
    for (const mutation of mutations) {
      if (mutation.addedNodes.length > 0) {
        for (const node of mutation.addedNodes) {
          if (
            node instanceof HTMLElement &&
            (node.matches?.(AXIOM_PANEL_LAYER_OVERLAY_SELECTOR) ||
              node.querySelector?.(AXIOM_PANEL_LAYER_OVERLAY_SELECTOR))
          ) {
            return true;
          }
        }
      }
      if (mutation.removedNodes.length > 0) {
        for (const node of mutation.removedNodes) {
          if (
            node instanceof HTMLElement &&
            (node.matches?.(AXIOM_PANEL_LAYER_OVERLAY_SELECTOR) ||
              node.querySelector?.(AXIOM_PANEL_LAYER_OVERLAY_SELECTOR))
          ) {
            return true;
          }
        }
      }
      if (
        mutation.type === "attributes" &&
        (
          mutation.attributeName === "class" ||
          mutation.attributeName === "role" ||
          mutation.attributeName?.startsWith("data-popper-")
        )
      ) {
        return true;
      }
    }
    return false;
  }

  function ensurePanelLayerObserver(target) {
    if (platform !== "axiom") {
      return;
    }
    if (!(target instanceof HTMLElement)) {
      return;
    }
    applyPanelLayeringForTarget(target);
    if (state.panelLayerObservers.has(target)) {
      return;
    }
    const root = document.body || document.documentElement;
    if (!root) {
      return;
    }
    const observer = new MutationObserver((mutations) => {
      if (panelLayerMutationTouchesCandidate(mutations)) {
        applyPanelLayeringForTarget(target);
      }
    });
    observer.observe(root, {
      childList: true,
      subtree: true,
      attributes: true
    });
    state.panelLayerObservers.set(target, observer);
  }

  function disconnectPanelLayerObserver(target) {
    const observer = state.panelLayerObservers.get(target);
    if (!observer) {
      return;
    }
    observer.disconnect();
    state.panelLayerObservers.delete(target);
  }

  function disconnectPanelLayerObservers() {
    state.panelLayerObservers.forEach((observer) => observer.disconnect());
    state.panelLayerObservers.clear();
  }

  function destroyPersistentPanelFrame() {
    disconnectPanelLayerObserver(state.panelWrapper);
    if (state.panelWrapper) {
      state.panelWrapper.remove();
    }
    state.panelWrapper = null;
    state.panelFrame = null;
    state.panelReady = false;
    state.panelOpen = false;
  }

  // The "Hide wallet groups" / "Hide preset row" hover flyouts are rendered as
  // a host-page overlay (outside the panel iframe) so chip lists can extend
  // beyond the panel's bounds without being clipped by the iframe. The
  // iframe sends `panel-flyout-show` / `panel-flyout-host-leave` /
  // `panel-flyout-cancel`; we render the chips here, manage hover/dismiss
  // transitions, and call back into `savePreferences` on selection.

  state.panelFlyoutOverlay = null;
  state.panelFlyoutKind = null;
  state.panelFlyoutSource = null;
  state.panelFlyoutCloseTimer = null;

  const PANEL_FLYOUT_HOVER_GAP_MS = 160;

  function panelFlyoutPointerDown(event) {
    if (!state.panelFlyoutOverlay || state.panelFlyoutOverlay.style.display === "none") {
      return;
    }
    const target = event.target;
    if (target instanceof Node && state.panelFlyoutOverlay.contains(target)) {
      return;
    }
    const sourceFrame = state.panelFlyoutSource === "quick"
      ? state.quickPanelWrapper
      : state.panelWrapper;
    if (sourceFrame instanceof HTMLElement && target instanceof Node && sourceFrame.contains(target)) {
      // Clicks inside the panel iframe are handled by the iframe's own
      // dropdown logic; we don't want to dismiss the overlay just because
      // the user clicked the toggle button to open it.
      return;
    }
    hidePanelFlyoutOverlay();
  }

  function ensurePanelFlyoutOverlay() {
    if (state.panelFlyoutOverlay && document.documentElement.contains(state.panelFlyoutOverlay)) {
      return state.panelFlyoutOverlay;
    }
    document.addEventListener("pointerdown", panelFlyoutPointerDown, true);
    const overlay = document.createElement("div");
    overlay.id = "trench-tools-panel-flyout-overlay";
    Object.assign(overlay.style, {
      position: "fixed",
      display: "none",
      flexDirection: "column",
      alignItems: "stretch",
      gap: "1px",
      padding: "3px",
      minWidth: "150px",
      maxWidth: "240px",
      borderRadius: "4px",
      border: "1px solid rgba(255, 255, 255, 0.12)",
      background: "rgba(10, 10, 10, 0.98)",
      boxShadow: "0 12px 32px rgba(0, 0, 0, 0.42)",
      zIndex: PANEL_Z_INDEX.FLYOUT,
      pointerEvents: "auto",
      fontFamily: '"Inter", "Segoe UI", system-ui, -apple-system, sans-serif',
      fontSize: "12px",
      color: "#fafafa",
      lineHeight: "1.2"
    });
    overlay.addEventListener("mouseenter", () => {
      clearPanelFlyoutCloseTimer();
    });
    overlay.addEventListener("mouseleave", () => {
      schedulePanelFlyoutClose();
    });
    document.documentElement.appendChild(overlay);
    state.panelFlyoutOverlay = overlay;
    ensurePanelLayerObserver(overlay);
    return overlay;
  }

  function clearPanelFlyoutCloseTimer() {
    if (state.panelFlyoutCloseTimer) {
      clearTimeout(state.panelFlyoutCloseTimer);
      state.panelFlyoutCloseTimer = null;
    }
  }

  function schedulePanelFlyoutClose() {
    clearPanelFlyoutCloseTimer();
    state.panelFlyoutCloseTimer = setTimeout(() => {
      hidePanelFlyoutOverlay();
    }, PANEL_FLYOUT_HOVER_GAP_MS);
  }

  function hidePanelFlyoutOverlay() {
    clearPanelFlyoutCloseTimer();
    if (state.panelFlyoutOverlay) {
      state.panelFlyoutOverlay.style.display = "none";
      state.panelFlyoutOverlay.innerHTML = "";
    }
    state.panelFlyoutKind = null;
    state.panelFlyoutSource = null;
  }

  function handlePanelFlyoutHostLeave() {
    // The cursor left the in-iframe trigger. Close after a short delay so
    // the user can move into the overlay; the overlay's own mouseenter
    // will cancel the timer.
    schedulePanelFlyoutClose();
  }

  function handlePanelFlyoutShow({ source, kind, anchor }) {
    if (kind !== "wallet-groups" && kind !== "preset-row") {
      return;
    }
    const iframe = source === "quick" ? state.quickPanelFrame : state.panelFrame;
    if (!iframe || !anchor) {
      return;
    }
    clearPanelFlyoutCloseTimer();
    state.panelFlyoutKind = kind;
    state.panelFlyoutSource = source;
    const overlay = ensurePanelFlyoutOverlay();
    overlay.innerHTML = "";
    populatePanelFlyoutChips(overlay, kind);
    overlay.style.display = "flex";
    positionPanelFlyoutOverlay(overlay, iframe, anchor, source);
  }

  function positionPanelFlyoutOverlay(overlay, iframe, anchor, source) {
    const iframeRect = iframe.getBoundingClientRect();
    const scale = source === "persistent"
      ? clamp(Number(state.panelScale) || 1, 0.5, 4)
      : 1;
    const anchorViewportLeft = iframeRect.left + (Number(anchor.left) || 0) * scale;
    const anchorViewportTop = iframeRect.top + (Number(anchor.top) || 0) * scale;
    const anchorHeight = (Number(anchor.height) || 0) * scale;
    // Force a layout pass to read the overlay's natural size.
    overlay.style.left = "-9999px";
    overlay.style.top = "-9999px";
    const overlayRect = overlay.getBoundingClientRect();
    const overlayWidth = Math.ceil(overlayRect.width);
    const overlayHeight = Math.ceil(overlayRect.height);
    const margin = 4;
    // Prefer opening to the LEFT of the iframe. If there isn't enough room,
    // fall back to the RIGHT edge of the iframe.
    let left = anchorViewportLeft - overlayWidth - 6;
    if (left < margin) {
      left = iframeRect.right + 6;
    }
    if (left + overlayWidth > window.innerWidth - margin) {
      left = Math.max(margin, window.innerWidth - overlayWidth - margin);
    }
    let top = anchorViewportTop;
    if (top + overlayHeight > window.innerHeight - margin) {
      top = Math.max(margin, anchorViewportTop + anchorHeight - overlayHeight);
    }
    if (top < margin) top = margin;
    overlay.style.left = `${Math.round(left)}px`;
    overlay.style.top = `${Math.round(top)}px`;
  }

  function populatePanelFlyoutChips(overlay, kind) {
    if (kind === "wallet-groups") {
      const groups = Array.isArray(state.bootstrap?.walletGroups)
        ? state.bootstrap.walletGroups
        : [];
      const selection = normalizeWalletSelectionPreference(state.preferences);
      if (groups.length === 0) {
        overlay.appendChild(
          createPanelFlyoutChip({
            label: "Set up groups",
            active: false,
            onClick: () => {
              hidePanelFlyoutOverlay();
              dismissIframeFlyoutMenu();
              void openOptionsSection("wallets");
            }
          })
        );
        return;
      }
      groups.forEach((group) => {
        const isActive =
          selection.selectionSource === "group" &&
          selection.activeWalletGroupId === group.id;
        overlay.appendChild(
          createPanelFlyoutChip({
            label: group.label || group.id,
            active: isActive,
            onClick: () => {
              hidePanelFlyoutOverlay();
              dismissIframeFlyoutMenu();
              const nextRevision =
                Math.max(0, Number(state.preferences?.selectionRevision || 0) || 0) + 1;
              void savePreferences({
                selectionSource: "group",
                activeWalletGroupId: group.id,
                manualWalletKeys: Array.isArray(state.preferences?.manualWalletKeys)
                  ? [...state.preferences.manualWalletKeys]
                  : [],
                selectionRevision: nextRevision
              });
            }
          })
        );
      });
      return;
    }
    if (kind === "preset-row") {
      const presets = Array.isArray(state.bootstrap?.presets) ? state.bootstrap.presets : [];
      const activePresetId = String(
        state.preferences?.presetId || presets[0]?.id || ""
      ).trim();
      if (presets.length === 0) {
        overlay.appendChild(
          createPanelFlyoutChip({
            label: "Create preset",
            active: false,
            onClick: () => {
              hidePanelFlyoutOverlay();
              dismissIframeFlyoutMenu();
              void openOptionsSection("presets");
            }
          })
        );
        return;
      }
      presets.forEach((preset) => {
        overlay.appendChild(
          createPanelFlyoutChip({
            label: preset.label || preset.id,
            active: preset.id === activePresetId,
            onClick: () => {
              hidePanelFlyoutOverlay();
              dismissIframeFlyoutMenu();
              void savePreferences({ presetId: preset.id });
            }
          })
        );
      });
    }
  }

  function createPanelFlyoutChip({ label, active, onClick }) {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = label;
    Object.assign(button.style, {
      padding: "5px 10px",
      borderRadius: "6px",
      border: active ? "1px solid rgba(255, 255, 255, 0.42)" : "1px solid transparent",
      background: active ? "rgba(255, 255, 255, 0.10)" : "rgba(255, 255, 255, 0.04)",
      color: "#fafafa",
      cursor: "pointer",
      fontSize: "11px",
      fontFamily: "inherit",
      lineHeight: "1.2",
      whiteSpace: "nowrap",
      maxWidth: "200px",
      overflow: "hidden",
      textOverflow: "ellipsis"
    });
    button.addEventListener("mouseenter", () => {
      if (!active) button.style.background = "rgba(255, 255, 255, 0.10)";
    });
    button.addEventListener("mouseleave", () => {
      if (!active) button.style.background = "rgba(255, 255, 255, 0.04)";
    });
    button.addEventListener("click", () => {
      try {
        onClick();
      } catch (error) {
        // Surface unexpected click errors but never let them bubble out of
        // the message listener.
        // eslint-disable-next-line no-console
        console.error("trench-tools: flyout chip click failed", error);
      }
    });
    return button;
  }

  function dismissIframeFlyoutMenu() {
    const target = state.panelFlyoutSource === "quick"
      ? state.quickPanelFrame?.contentWindow
      : state.panelFrame?.contentWindow;
    if (!target) return;
    target.postMessage(
      {
        channel: PANEL_CHANNEL_OUT,
        type: "panel-flyout-dismiss-menu"
      },
      PANEL_ORIGIN
    );
  }

  function attachPanelMessageListener() {
    if (lifecycle.panelMessageListener) {
      return;
    }
    lifecycle.panelMessageListener = async (event) => {
      if (!event.data || event.data.channel !== PANEL_CHANNEL_IN) {
        return;
      }
      const isPersistentSource = event.source === state.panelFrame?.contentWindow;
      const isQuickSource = event.source === state.quickPanelFrame?.contentWindow;
      if (!isPersistentSource && !isQuickSource) {
        return;
      }
      if (event.origin !== PANEL_ORIGIN) {
        return;
      }

      try {
        switch (event.data.type) {
          case "panel-ready":
            if (isQuickSource) {
              state.quickPanelReady = true;
            } else {
              state.panelReady = true;
            }
            pushPanelState();
            if (isPersistentSource) {
              applyPanelShellMetrics();
            }
            break;
          case "panel-resize":
          case "quick-panel-resize":
            if (isQuickSource && state.quickPanelWrapper) {
              const requestedHeight = Number(event.data.payload?.height || 0);
              // Width is fixed: the panel is designed for a single column at
              // QUICK_PANEL_DEFAULT_WIDTH and must never grow horizontally.
              // We deliberately ignore the iframe's reported width because
              // a single overflowing child (e.g. a long token symbol or an
              // unwrapped chip) can push `scrollWidth` to viewport width on
              // wide pages like Axiom Pulse and balloon the popup.
              // Height tracks the panel's natural content height (so hiding
              // rows can shrink and showing extra rows can grow), capped to
              // the viewport.
              const heightCap = Math.max(0, window.innerHeight - 24);
              const desiredHeight = Math.ceil(requestedHeight) || QUICK_PANEL_DEFAULT_HEIGHT;
              const height = Math.min(heightCap, desiredHeight);
              Object.assign(state.quickPanelWrapper.style, {
                width: `${QUICK_PANEL_DEFAULT_WIDTH}px`,
                height: `${height}px`
              });
              if (state.quickPanelOpen) {
                positionQuickPanel(currentQuickPanelAnchor() || state.quickPanelWrapper);
              }
            } else if (isPersistentSource) {
              const requestedHeight = Number(event.data.payload?.height || 0);
              if (Number.isFinite(requestedHeight) && requestedHeight > 0) {
                const previous = Number(state.panelNaturalHeight) || 0;
                if (Math.abs(previous - requestedHeight) >= 1) {
                  state.panelNaturalHeight = requestedHeight;
                  applyPanelShellMetrics();
                }
              }
            }
            break;
          case "panel-flyout-show":
            handlePanelFlyoutShow({
              source: isQuickSource ? "quick" : "persistent",
              kind: String(event.data.payload?.kind || "").trim(),
              anchor: event.data.payload?.anchor || null
            });
            break;
          case "panel-flyout-host-leave":
            handlePanelFlyoutHostLeave();
            break;
          case "panel-flyout-cancel":
            hidePanelFlyoutOverlay();
            break;
          case "minimize-panel":
            if (isQuickSource) {
              closeQuickPanel();
            } else {
              await setPanelHidden(true);
            }
            break;
          case "close-panel":
            if (isQuickSource) {
              closeQuickPanel();
            } else {
              await setPanelHidden(true);
            }
            break;
          case "persist-preferences":
            await savePreferences(event.data.payload || {});
            await refreshPanelWalletStatus({ force: true });
            break;
          case "start-drag":
            if (!isQuickSource) {
              beginPanelDrag(event.data.payload || {});
            }
            break;
          case "request-preview":
            await handlePreviewRequest(event.data.payload || {});
            break;
          case "request-buy":
            await handleTradeRequest("buy", event.data.payload || {});
            break;
          case "request-buy-preset":
            await handleTradeRequest("buy", event.data.payload || {});
            break;
          case "request-sell":
            await handleTradeRequest("sell", event.data.payload || {});
            break;
          case "request-sell-shortcut":
            await handleTradeRequest("sell", event.data.payload || {});
            break;
          case "request-token-split":
            await handleTokenDistributionRequest("split", event.data.payload || {});
            break;
          case "request-token-consolidate":
            await handleTokenDistributionRequest("consolidate", event.data.payload || {});
            break;
          case "resync-pnl-history":
            await handlePnlHistoryAction("resync");
            break;
          case "reset-pnl-history":
            await handlePnlHistoryAction("reset");
            break;
          case "refresh-panel":
            await refreshBootstrap(true);
            await refreshPanelTokenContext();
            await refreshPanelWalletStatus({ force: true });
            break;
          case "open-options":
            await callBackground("trench:open-options", event.data.payload || {});
            break;
          default:
            break;
        }
      } catch (error) {
        surfaceUserFacingError(error, { pushToPanel: true, toast: false });
      }
    };
    window.addEventListener("message", lifecycle.panelMessageListener);
  }

  function attachStorageChangeListener() {
    if (lifecycle.storageChangeListener) {
      return;
    }
    lifecycle.storageChangeListener = (changes, areaName) => {
      if (areaName !== "local") {
        return;
      }
      if (changes[SITE_FEATURES_KEY]) {
        state.siteFeatures = normalizeSiteFeaturesValue(changes[SITE_FEATURES_KEY].newValue || {});
        dismissOpenSurfacesIfPlatformDisabled();
        ensureFloatingLauncher(state.tokenContext);
        scanAndMount();
        scheduleRouteReconcile("site-features", 0);
      }
      if (changes[APPEARANCE_KEY]) {
        state.appearance = normalizeAppearanceValue(changes[APPEARANCE_KEY].newValue || {});
      }
      if (changes[BOOTSTRAP_REVISION_KEY]) {
        void refreshBootstrap().then(() => {
          scheduleWalletStatusRefresh({ force: true, delayMs: 40 });
        });
      }
      if (changes[HOST_AUTH_TOKEN_KEY]) {
        invalidateLaunchdeckPresetCache();
        void refreshBootstrap().then(() => {
          scheduleWalletStatusRefresh({ force: true, delayMs: 40 });
          scheduleRouteReconcile("host-auth", 0);
        });
      }
      if (changes[LAUNCHDECK_HOST_KEY]) {
        invalidateLaunchdeckPresetCache();
      }
      if (changes[WALLET_STATUS_REVISION_KEY]) {
        const walletStatusDiff = changes[WALLET_STATUS_DIFF_KEY]?.newValue;
        const appliedLiveBalanceDiff = applyLiveWalletStatusBalanceDiff(walletStatusDiff);
        if (
          (state.panelOpen || state.quickPanelOpen || state.tokenContext?.mint) &&
          walletStatusDiffTouchesCurrentMint(walletStatusDiff)
        ) {
          const appliedLiveMintDiff = applyLiveWalletStatusMintDiff(walletStatusDiff);
          if (!appliedLiveMintDiff && !appliedLiveBalanceDiff) {
            scheduleWalletStatusRefresh({ force: true, delayMs: 120 });
          }
        }
      }
      if (changes[WALLET_STATUS_MARK_REVISION_KEY] || changes[WALLET_STATUS_MARK_DIFF_KEY]) {
        applyLiveWalletStatusMarkDiff(changes[WALLET_STATUS_MARK_DIFF_KEY]?.newValue);
      }
      if (changes[RUNTIME_DIAGNOSTICS_SNAPSHOT_KEY]) {
        surfaceRuntimeDiagnostics(changes[RUNTIME_DIAGNOSTICS_SNAPSHOT_KEY]?.newValue);
      } else if (changes[RUNTIME_DIAGNOSTICS_REVISION_KEY]) {
        void callBackground("trench:get-runtime-diagnostics")
          .then(surfaceRuntimeDiagnostics)
          .catch(() => {});
      }
      if (changes[LAST_TRADE_EVENT_KEY]?.newValue) {
        const tradeEvent = changes[LAST_TRADE_EVENT_KEY].newValue;
        applyTradeEventToBatchStatus(tradeEvent);
        if (
          String(tradeEvent?.status || "").trim().toLowerCase() === "confirmed" &&
          tradeEvent?.ledgerApplied === true &&
          (state.panelOpen || state.quickPanelOpen || state.tokenContext?.mint)
        ) {
          scheduleWalletStatusRefresh({ force: true, delayMs: 180 });
        }
      }
      if (changes[BATCH_STATUS_EVENT_KEY]?.newValue) {
        applyStreamedBatchStatus(changes[BATCH_STATUS_EVENT_KEY].newValue);
      }
      if (changes[PREFERENCES_KEY]) {
        state.preferences = normalizePreferencesValue(changes[PREFERENCES_KEY].newValue || {});
        pushPanelState();
        scheduleWalletStatusRefresh({ force: true, delayMs: 40 });
        scanAndMount();
      }
    };
    chrome.storage.onChanged.addListener(lifecycle.storageChangeListener);
  }

  async function handlePreviewRequest(payload) {
    const requestSeq = ++state.previewRequestSeq;
    try {
      await savePreferences(payload);
      if (!(await ensureValidExecutionPreset({
        missingMessage: "No valid preset saved. Click here to create a preset",
        pushToPanel: true
      }))) {
        return;
      }
      const tokenContext =
        (await refreshPanelTokenContext({ silent: true })) ||
        currentActivePanelTokenContext() ||
        state.tokenContext ||
        (await refreshCurrentToken());
      if (!tokenContext) {
        throw new Error("No token selected.");
      }
      const previewSide = String(payload?.side || "buy").trim().toLowerCase() === "sell"
        ? "sell"
        : "buy";
      const prewarmResponse = await ensurePrewarmResponse(tokenContext, previewSide);
      const warmReuseFields = buildWarmReuseFields(prewarmResponse, previewSide);
      const routeRequest = getTokenContextRouteRequest(tokenContext);
      const requestAddress = routeRequest.address;
      const previewPayload = {
        address: requestAddress,
        platform: normalizeRouteValue(tokenContext?.platform || state.platform) || undefined,
        mint: normalizeRouteValue(tokenContext?.mint) || undefined,
        pair: routeRequest.pair || undefined,
        presetId: state.preferences.presetId,
        side: previewSide,
        buyAmountSol:
          previewSide === "buy"
            ? String(payload?.buyAmountSol || "").trim() || undefined
            : undefined,
        sellPercent:
          previewSide === "sell"
            ? String(payload?.sellPercent || "").trim() || undefined
            : undefined,
        sellOutputSol:
          previewSide === "sell"
            ? String(payload?.sellOutputSol || "").trim() || undefined
            : undefined,
        ...warmReuseFields,
        ...selectionPayloadFromPreferences()
      };
      const preview = await callBackground("trench:preview-batch", previewPayload);
      if (requestSeq !== state.previewRequestSeq) {
        return;
      }
      // Compare the route identity we asked about against whatever is
      // currently active in the panel so same-mint / different-route
      // surfaces cannot land a stale preview.
      const expectedRouteIdentity =
        tokenContextRouteIdentity(tokenContext) || String(requestAddress || "").trim();
      const currentRouteIdentity =
        tokenContextRouteIdentity(currentActivePanelTokenContext() || state.tokenContext);
      if (expectedRouteIdentity && currentRouteIdentity && expectedRouteIdentity !== currentRouteIdentity) {
        return;
      }
      state.preview = preview;
      pushPanelPreview();
    } catch (error) {
      if (requestSeq !== state.previewRequestSeq) {
        return;
      }
      state.hostError = isHostAvailabilityError(error) ? userFacingErrorMessage(error) : "";
      surfaceUserFacingError(error, { pushToPanel: true, toast: false });
    }
  }

  function normalizeWalletKeyList(value) {
    return Array.from(new Set(
      (Array.isArray(value) ? value : [])
        .map((entry) => String(entry || "").trim())
        .filter(Boolean)
    ));
  }

  function selectedWalletGroupIdFromValue(value = {}) {
    const explicitGroupId = String(
      value.walletGroupId ||
      value.activeWalletGroupId ||
      value.selectionTarget?.walletGroupId ||
      ""
    ).trim();
    if (explicitGroupId) {
      return explicitGroupId;
    }
    const selectionSource = String(value.selectionSource || "").trim().toLowerCase();
    if (selectionSource && selectionSource !== "group") {
      return "";
    }
    const groups = Array.isArray(state.bootstrap?.walletGroups) ? state.bootstrap.walletGroups : [];
    return String(groups[0]?.id || "").trim();
  }

  function requireSelectedWalletGroupId(value = {}) {
    const walletGroupId = selectedWalletGroupIdFromValue(value);
    if (!walletGroupId) {
      throw new Error("Select a wallet group.");
    }
    return walletGroupId;
  }

  function selectedWalletKeysFromPanelPayload(payload = {}) {
    const walletKeys = normalizeWalletKeyList(payload.walletKeys);
    if (walletKeys.length) {
      return walletKeys;
    }
    const walletKey = String(payload.walletKey || "").trim();
    if (walletKey) {
      return [walletKey];
    }
    const walletGroupId = selectedWalletGroupIdFromValue(payload);
    if (!walletGroupId) {
      return [];
    }
    const groups = Array.isArray(state.bootstrap?.walletGroups) ? state.bootstrap.walletGroups : [];
    const group = groups.find((entry) => String(entry?.id || "").trim() === walletGroupId);
    return normalizeWalletKeyList(group?.walletKeys);
  }

  function getPanelWalletTokenBalanceNumber(wallet) {
    const value = Number(
      wallet?.tokenBalance ??
      wallet?.mintBalanceUi ??
      wallet?.mintBalance ??
      wallet?.holdingAmount ??
      0
    );
    return Number.isFinite(value) ? value : 0;
  }

  function summarizeTokenDistributionFailure(result, fallbackMessage) {
    const transfers = Array.isArray(result?.transfers) ? result.transfers : [];
    const failed = transfers.find((entry) => {
      const status = String(entry?.status || "").trim().toLowerCase();
      return status === "failed" || entry?.error;
    });
    if (!failed) {
      return fallbackMessage;
    }
    const source = String(failed.sourceWalletKey || failed.source || "").trim();
    const destination = String(failed.destinationWalletKey || failed.destination || "").trim();
    const error = String(failed.error || fallbackMessage || "Token distribution failed.").trim();
    const route = source && destination ? `${source} -> ${destination}` : source || destination;
    return route ? `${route}: ${error}` : error;
  }

  async function handleTokenDistributionRequest(action, payload, options = {}) {
    const normalizedAction = action === "consolidate" ? "consolidate" : "split";
    if (state.tokenDistributionPending) {
      showToast("Token distribution already in progress.", "error");
      return;
    }
    state.tokenDistributionPending = normalizedAction;
    pushPanelState();

    const toastId = `token-distribution-${normalizedAction}`;
    let toastStarted = false;
    const startingToast = normalizedAction === "split" ? "Splitting tokens" : "Consolidating tokens";
    const completeToast = normalizedAction === "split" ? "Tokens split" : "Consolidation complete";
    const failureMessage = normalizedAction === "split" ? "Token split failed." : "Token consolidation failed.";

    try {
      if (options.persistPreferences !== false) {
        await savePreferences(payload);
      }
      const tokenContext =
        (await refreshPanelTokenContext({ silent: true })) ||
        currentActivePanelTokenContext() ||
        state.tokenContext ||
        (await refreshCurrentToken());
      const mint = String(tokenContext?.mint || state.walletStatus?.mint || "").trim();
      if (!mint) {
        throw new Error("No token selected.");
      }

      const selectedWalletKeys = selectedWalletKeysFromPanelPayload(payload);
      if (!selectedWalletKeys.length) {
        throw new Error("Select at least one wallet.");
      }

      const statusWallets = Array.isArray(state.walletStatus?.wallets) ? state.walletStatus.wallets : [];
      const holderKeys = new Set(
        statusWallets
          .filter((wallet) => getPanelWalletTokenBalanceNumber(wallet) > 0)
          .map((wallet) => String(wallet?.key || wallet?.envKey || "").trim())
          .filter(Boolean)
      );

      let request;
      if (normalizedAction === "split") {
        const payloadSourceWalletKeys = normalizeWalletKeyList(payload.sourceWalletKeys)
          .filter((walletKey) => selectedWalletKeys.includes(walletKey));
        const sourceWalletKeys = payloadSourceWalletKeys.length
          ? payloadSourceWalletKeys
          : selectedWalletKeys.filter((walletKey) => holderKeys.has(walletKey));
        if (selectedWalletKeys.length < 2) {
          throw new Error("Select at least two wallets to split tokens.");
        }
        if (!sourceWalletKeys.length) {
          throw new Error("Select at least one wallet that holds this token.");
        }
        request = {
          mint,
          presetId: normalizeRouteValue(payload.presetId || state.preferences.presetId) || undefined,
          walletKeys: selectedWalletKeys,
          sourceWalletKeys
        };
      } else {
        if (selectedWalletKeys.length !== 1) {
          showToast("Select only one wallet to consolidate tokens.", "error");
          return;
        }
        request = {
          mint,
          presetId: normalizeRouteValue(payload.presetId || state.preferences.presetId) || undefined,
          destinationWalletKey: selectedWalletKeys[0]
        };
      }

      toastStarted = true;
      renderToast({
        id: toastId,
        title: `${startingToast}...`,
        kind: "info",
        pending: true,
        persistent: true,
        ttlMs: 0
      });

      const result = await callBackground(
        normalizedAction === "split" ? "trench:token-split" : "trench:token-consolidate",
        request
      );
      await refreshPanelWalletStatus({ tokenContext, force: true }).catch(() => null);
      const failedCount = Number(result?.failedCount || 0);
      if (Number.isFinite(failedCount) && failedCount > 0) {
        throw new Error(summarizeTokenDistributionFailure(result, failureMessage));
      }
      renderToast({
        id: toastId,
        title: completeToast,
        kind: "success",
        ttlMs: 3200
      });
    } catch (error) {
      state.hostError = isHostAvailabilityError(error) ? userFacingErrorMessage(error) : "";
      if (toastStarted) {
        renderToast({
          id: toastId,
          title: failureMessage,
          detail: userFacingErrorMessage(error),
          kind: "error",
          ttlMs: 4200
        });
        surfaceUserFacingError(error, { pushToPanel: true, toast: false });
      } else {
        surfaceUserFacingError(error, { pushToPanel: true });
      }
    } finally {
      if (state.tokenDistributionPending === normalizedAction) {
        state.tokenDistributionPending = "";
        pushPanelState();
      }
    }
  }

  async function handleTradeRequest(side, payload, options = {}) {
    const requestEntryStartedAt = Date.now();
    let clientRequestId = "";
    try {
      const requestedBuyAmount = String(payload?.buyAmountSol || "").trim();
      const requestedSellPercent = String(payload?.sellPercent || "").trim();
      const requestedSellOutputSol = String(payload?.sellOutputSol || "").trim();
      if (options.persistPreferences === false) {
        state.preferences = normalizePreferencesValue({
          ...state.preferences,
          ...payload
        });
        pushPanelState();
      } else {
        await savePreferences(payload);
      }
      if (!(await ensureValidExecutionPreset({
        missingMessage: "No valid preset saved. Click here to create a preset",
        pushToPanel: true
      }))) {
        return;
      }
      const selection = selectionPayloadFromPreferences();
      if (!selection.walletKey && !selection.walletGroupId && !selection.walletKeys?.length) {
        throw new Error("Select at least one wallet.");
      }
      const tokenContext =
        options.tokenContextOverride ||
        (await refreshPanelTokenContext({ silent: true })) ||
        currentActivePanelTokenContext() ||
        state.tokenContext ||
        (await refreshCurrentToken());
      if (!tokenContext) {
        throw new Error("No token selected.");
      }
      clientRequestId = crypto.randomUUID();
      const warmReuseFields = await resolveTradeWarmReuseFields(tokenContext, {
        ...options,
        side,
        skipBlockingPrewarm: true
      });
      const routeRequest = getTokenContextRouteRequest(tokenContext);
      const requestAddress = routeRequest.address;
      const request = {
        clientRequestId,
        clientStartedAtUnixMs: requestEntryStartedAt,
        clientRequestStartedAtUnixMs: requestEntryStartedAt,
        address: requestAddress,
        mint: normalizeRouteValue(tokenContext?.mint) || undefined,
        platform: String(tokenContext?.platform || state.platform || "").trim() || undefined,
        pair: routeRequest.pair || undefined,
        presetId: state.preferences.presetId,
        ...selection,
        ...warmReuseFields
      };
      const hostPayload =
        side === "buy"
          ? {
              ...request,
              buyAmountSol: requestedBuyAmount || undefined
            }
          : {
              ...request,
              sellPercent: requestedSellPercent || undefined,
              sellOutputSol: requestedSellOutputSol || undefined
            };
      rememberLocalExecutionPending({
        clientRequestId,
        side,
        walletCount: selectedWalletCountForRequest(selection)
      });
      const dispatchStartedAt = Date.now();
      console.debug(
        "[trench][latency] phase=content-prep clientRequestId=%s side=%s prep_ms=%s",
        clientRequestId,
        side,
        dispatchStartedAt - requestEntryStartedAt
      );
      const result =
        side === "buy"
          ? await callBackground("trench:buy", hostPayload)
          : await callBackground("trench:sell", hostPayload);
      console.debug(
        "[trench][latency] phase=content-accepted clientRequestId=%s batch=%s click_to_accepted_ms=%s background_roundtrip_ms=%s",
        clientRequestId,
        result?.batchId || "",
        Date.now() - requestEntryStartedAt,
        Date.now() - dispatchStartedAt
      );
      surfaceAutoFeeWarnings(result);
      bindLocalExecutionPendingToBatch(clientRequestId, result.batchId);

      setActivePanelBatchStatus(
        state.batchStatuses.get(result.batchId) || createAcceptedBatchStatus(result, side, tokenContext)
      );
      pushPanelBatchStatus();
      await pollBatchStatus(result.batchId, side, result.walletCount, result.clientRequestId);
    } catch (error) {
      dismissLocalExecutionPending({ clientRequestId });
      state.hostError = isHostAvailabilityError(error) ? userFacingErrorMessage(error) : "";
      surfaceUserFacingError(error, { pushToPanel: true, side });
    }
  }

  function clearBatchStatusPoller(batchId) {
    const timer = state.statusPollTimers.get(batchId);
    if (timer) {
      window.clearInterval(timer);
      state.statusPollTimers.delete(batchId);
    }
  }

  function surfaceAutoFeeWarnings(result) {
    const warnings = Array.isArray(result?.warnings) ? result.warnings : [];
    for (const warning of warnings) {
      const message = String(warning || "").trim();
      if (!message || !message.toLowerCase().startsWith("auto fee unavailable:")) {
        continue;
      }
      showToast(message, "error");
    }
  }

  function createAcceptedBatchStatus(result, fallbackSide, tokenContext = null) {
    const normalizedStatus = String(result?.status || "submitted").toLowerCase();
    const walletCount = Number(result?.walletCount || 0);
    const queuedWallets = normalizedStatus === "queued" ? walletCount : 0;
    const submittedWallets = normalizedStatus === "submitted" ? walletCount : 0;

    return {
      batchId: result.batchId,
      side: result.side || fallbackSide,
      status: result.status || "submitted",
      summary: {
        totalWallets: walletCount,
        queuedWallets,
        submittedWallets,
        confirmedWallets: 0,
        failedWallets: 0
      },
      wallets: [],
      routeIdentity: tokenContextRouteIdentity(tokenContext) || undefined
    };
  }

  function rememberBatchStatus(batchStatus) {
    if (!batchStatus?.batchId) {
      return null;
    }
    const existing = state.batchStatuses.get(batchStatus.batchId);
    const routeIdentity = String(batchStatus?.routeIdentity || existing?.routeIdentity || "").trim();
    const normalized = routeIdentity
      ? { ...batchStatus, routeIdentity }
      : { ...batchStatus };
    state.batchStatuses.set(batchStatus.batchId, normalized);
    return normalized;
  }

  function currentPanelMint() {
    return String(currentActivePanelTokenContext()?.mint || "").trim();
  }

  function batchStatusMint(batchStatus) {
    const selector = batchStatus?.plannedExecution;
    const runtimeBundle = selector?.runtimeBundle;
    if (runtimeBundle?.kind === "pump_bonding_curve") {
      return String(runtimeBundle.value?.mint || "").trim();
    }
    if (runtimeBundle?.kind === "pump_amm") {
      return String(runtimeBundle.value?.baseMint || "").trim();
    }
    return "";
  }

  function batchStatusRouteIdentity(batchStatus) {
    const explicit = String(batchStatus?.routeIdentity || "").trim();
    if (explicit) {
      return explicit;
    }
    return "";
  }

  function setActivePanelBatchStatus(batchStatus) {
    if (!batchStatus?.batchId) {
      return;
    }
    const remembered = rememberBatchStatus(batchStatus) || batchStatus;
    state.activePanelBatchId = remembered.batchId;
    state.batchStatus = remembered;
  }

  function updatePanelBatchStatus(batchStatus) {
    if (!batchStatus?.batchId) {
      return;
    }
    const remembered = rememberBatchStatus(batchStatus) || batchStatus;
    const activeRouteIdentity = currentPanelRouteIdentity();
    const nextRouteIdentity = batchStatusRouteIdentity(remembered);
    if (activeRouteIdentity) {
      if (nextRouteIdentity && activeRouteIdentity !== nextRouteIdentity) {
        return;
      }
      if (!nextRouteIdentity && state.activePanelBatchId !== remembered.batchId) {
        return;
      }
    }
    const activeMint = currentPanelMint();
    const nextMint = batchStatusMint(remembered);
    if (activeMint && nextMint && activeMint !== nextMint) {
      return;
    }
    if (!state.activePanelBatchId || state.activePanelBatchId === remembered.batchId) {
      state.activePanelBatchId = remembered.batchId;
      state.batchStatus = remembered;
      pushPanelBatchStatus();
    }
  }

  function applyStreamedBatchStatus(event) {
    if (!event || typeof event !== "object") {
      return;
    }
    const batchId = String(event.batchId || "").trim();
    if (!batchId) {
      return;
    }
    const clientRequestId = String(event.clientRequestId || "").trim();
    const revision = Number.isInteger(event.revision) ? event.revision : 0;
    const previousRevision = state.batchStatusStreamRevisions.get(batchId) || 0;
    if (revision > 0 && previousRevision > 0 && revision <= previousRevision) {
      return;
    }
    if (revision > 0) {
      state.batchStatusStreamRevisions.set(batchId, revision);
    }
    const localPending = localExecutionPendingForBatch(batchId, clientRequestId);
    if (localPending && !localPending.batchId) {
      bindLocalExecutionPendingToBatch(localPending.clientRequestId, batchId);
    }
    if (localPending && !state.activePanelBatchId) {
      state.activePanelBatchId = batchId;
    }
    const isKnownBatch =
      state.activePanelBatchId === batchId ||
      state.batchStatuses.has(batchId) ||
      Boolean(localPending);
    if (!isKnownBatch) {
      rememberBatchStatus(event);
      return;
    }
    clearBatchStatusPoller(batchId);
    updatePanelBatchStatus(event);
    syncExecutionToastsFromBatchStatus(event, {
      side: event.side,
      walletCount: event.summary?.totalWallets,
      clientRequestId
    });
    const emittedAt = Number(event.streamEmittedAtUnixMs || 0);
    if (emittedAt > 0) {
      console.debug(
        "[trench][latency] phase=content-batch-applied batch=%s revision=%s stream_to_ui_ms=%s",
        batchId,
        revision,
        Math.max(0, Date.now() - emittedAt)
      );
    }
    const status = String(event.status || "").trim().toLowerCase();
    if (status === "confirmed") {
      void callBackground("trench:invalidate-balances", { afterTrade: true }).catch(() => {});
      window.setTimeout(() => {
        void refreshPanelWalletStatus({ force: true });
      }, 900);
    }
    if (status && !["confirmed", "failed"].includes(status)) {
      scheduleBatchStatusFallback(batchId, event.side, event.summary?.totalWallets, clientRequestId, revision);
    }
  }

  function summarizeBatchWallets(wallets, fallbackTotalWallets = 0) {
    const rows = Array.isArray(wallets) ? wallets : [];
    let queuedWallets = 0;
    let submittedWallets = 0;
    let confirmedWallets = 0;
    let failedWallets = 0;

    rows.forEach((wallet) => {
      const normalizedStatus = String(wallet?.status || "").trim().toLowerCase();
      if (normalizedStatus === "confirmed") {
        confirmedWallets += 1;
      } else if (normalizedStatus === "failed") {
        failedWallets += 1;
      } else if (normalizedStatus === "queued") {
        queuedWallets += 1;
      } else {
        submittedWallets += 1;
      }
    });

    const totalWallets = Number(fallbackTotalWallets || rows.length || 0);
    let status = "queued";
    if (failedWallets === totalWallets && totalWallets > 0) {
      status = "failed";
    } else if (confirmedWallets === totalWallets && totalWallets > 0) {
      status = "confirmed";
    } else if (confirmedWallets > 0 || failedWallets > 0) {
      status = "partially_confirmed";
    } else if (submittedWallets > 0) {
      status = "submitted";
    }

    return {
      status,
      summary: {
        totalWallets,
        queuedWallets,
        submittedWallets,
        confirmedWallets,
        failedWallets
      }
    };
  }

  function formatTradeEventError(err) {
    if (err == null) {
      return "";
    }
    if (typeof err === "string") {
      return err;
    }
    try {
      return JSON.stringify(err);
    } catch {
      return "Transaction failed.";
    }
  }

  function resolveTradeEventWalletIndex(batchStatus, signature) {
    const wallets = Array.isArray(batchStatus?.wallets) ? batchStatus.wallets : [];
    const normalizedSignature = String(signature || "").trim();
    if (!wallets.length) {
      return -1;
    }
    if (normalizedSignature) {
      const exactIndex = wallets.findIndex(
        (wallet) => String(wallet?.txSignature || "").trim() === normalizedSignature
      );
      if (exactIndex >= 0) {
        return exactIndex;
      }
    }
    const unresolvedIndex = wallets.findIndex((wallet) => {
      const normalizedStatus = String(wallet?.status || "").trim().toLowerCase();
      return !["confirmed", "failed"].includes(normalizedStatus);
    });
    if (unresolvedIndex >= 0) {
      return unresolvedIndex;
    }
    return wallets.length === 1 ? 0 : -1;
  }

  function applyTradeEventToBatchStatus(event) {
    const batchId = String(event?.batchId || "").trim();
    const status = String(event?.status || "").trim().toLowerCase();
    if (!batchId || !["confirmed", "failed"].includes(status)) {
      return;
    }

    const existing = state.batchStatuses.get(batchId);
    if (!existing || !Array.isArray(existing.wallets) || !existing.wallets.length) {
      return;
    }

    const walletIndex = resolveTradeEventWalletIndex(existing, event?.signature);
    if (walletIndex < 0) {
      return;
    }

    const wallets = existing.wallets.map((wallet, index) => {
      if (index !== walletIndex) {
        return wallet;
      }
      const nextSignature = String(event?.signature || "").trim() || wallet.txSignature || "";
      return {
        ...wallet,
        status,
        txSignature: nextSignature || null,
        error: status === "failed"
          ? (formatTradeEventError(event?.err) || wallet.error || "Transaction failed.")
          : null
      };
    });

    const next = {
      ...existing,
      wallets,
      updatedAtUnixMs: Date.now()
    };
    const aggregated = summarizeBatchWallets(wallets, existing?.summary?.totalWallets);
    next.status = aggregated.status;
    next.summary = aggregated.summary;

    updatePanelBatchStatus(next);
    syncExecutionToastsFromBatchStatus(next, { side: next.side });
  }

  function scheduleBatchStatusFallback(
    batchId,
    side,
    walletCount,
    clientRequestId = "",
    expectedRevision = 0,
    attempt = 0
  ) {
    clearBatchStatusPoller(batchId);
    const hasStreamRevision =
      expectedRevision > 0 || (state.batchStatusStreamRevisions.get(batchId) || 0) > 0;
    const delayMs = hasStreamRevision
      ? BATCH_STATUS_STREAM_STALE_FALLBACK_MS
      : attempt < 6
        ? 350
        : attempt < 12
          ? 750
          : 1500;
    const timer = window.setTimeout(async () => {
      state.statusPollTimers.delete(batchId);
      const latestRevision = state.batchStatusStreamRevisions.get(batchId) || 0;
      if (expectedRevision > 0 && latestRevision > expectedRevision) {
        return;
      }
      try {
        const batchStatus = await callBackground("trench:get-batch-status", { batchId });
        const latestStatus = String(batchStatus?.status || "").toLowerCase();
        updatePanelBatchStatus(batchStatus);
        syncExecutionToastsFromBatchStatus(batchStatus, { side, walletCount, clientRequestId });
        if (["confirmed", "failed"].includes(latestStatus)) {
          if (latestStatus === "confirmed") {
            // Background owns the cross-surface balance invalidation (see balances-store).
            void callBackground("trench:invalidate-balances", { afterTrade: true }).catch(() => {});
            window.setTimeout(() => {
              void refreshPanelWalletStatus({ force: true });
            }, 900);
          }
          clearBatchStatusPoller(batchId);
          return;
        }
        scheduleBatchStatusFallback(batchId, side, walletCount, clientRequestId, latestRevision, attempt + 1);
      } catch (error) {
        clearBatchStatusPoller(batchId);
        dismissLocalExecutionPending({ batchId, clientRequestId });
        state.hostError = isHostAvailabilityError(error) ? userFacingErrorMessage(error) : "";
        if (isExtensionReloadedError(error)) {
          scheduleExtensionReloadFallbackToast();
        } else {
          const toastCopy = buildErrorToastCopy(error, { side });
          pushPanelError(toastCopy.message, {
            title: toastCopy.title,
            kind: "error",
            source: "notice"
          });
          renderToast({
            id: `execution-${batchId}`,
            title: toastCopy.title,
            detail: toastCopy.detail,
            kind: "error",
            ttlMs: 3500
          });
        }
      }
    }, delayMs);
    state.statusPollTimers.set(batchId, timer);
  }

  async function pollBatchStatus(batchId, side, walletCount, clientRequestId = "") {
    clearBatchStatusPoller(batchId);
    try {
      const batchStatus = await callBackground("trench:get-batch-status", { batchId });
      const latestStatus = String(batchStatus?.status || "").toLowerCase();
      updatePanelBatchStatus(batchStatus);
      syncExecutionToastsFromBatchStatus(batchStatus, { side, walletCount, clientRequestId });
      if (latestStatus === "confirmed") {
        void callBackground("trench:invalidate-balances", { afterTrade: true }).catch(() => {});
        window.setTimeout(() => {
          void refreshPanelWalletStatus({ force: true });
        }, 900);
      }
      if (!["confirmed", "failed"].includes(latestStatus)) {
        scheduleBatchStatusFallback(
          batchId,
          side,
          walletCount,
          clientRequestId,
          state.batchStatusStreamRevisions.get(batchId) || 0
        );
      }
    } catch (error) {
      scheduleBatchStatusFallback(batchId, side, walletCount, clientRequestId, 0, 1);
    }
  }

  function walletLabelForToast(walletKey) {
    const compactWalletLabel = (label) => {
      const normalized = String(label || "").trim();
      if (!normalized) {
        return "";
      }
      const genericMatch = normalized.match(/^SOLANA_PRIVATE_KEY(\d+)?$/i);
      if (!genericMatch) {
        return normalized;
      }
      return `#${genericMatch[1] || "1"}`;
    };

    const bootstrapWallet = state.bootstrap?.wallets?.find((wallet) => wallet.key === walletKey);
    if (bootstrapWallet?.label) {
      return compactWalletLabel(bootstrapWallet.label);
    }
    const statusWallet = state.walletStatus?.wallets?.find((wallet) => wallet.key === walletKey || wallet.envKey === walletKey);
    if (statusWallet?.label) {
      return compactWalletLabel(statusWallet.label);
    }
    if (statusWallet?.customName) {
      return compactWalletLabel(statusWallet.customName);
    }
    return compactWalletLabel(walletKey || "");
  }

  function executionToastPayloadForWallet(batchStatus, walletState, fallback = {}) {
    const normalizedStatus = String(walletState?.status || batchStatus?.status || "").toLowerCase();
    const normalizedSide = String(walletState?.side || batchStatus?.side || fallback.side || "trade").toLowerCase();
    const walletLabel = walletLabelForToast(walletState?.walletKey || "");
    const walletPrefix = walletLabel ? `${walletLabel} - ` : "";
    const txSignature = walletState?.txSignature || "";
    const error = walletState?.error || "";

    if (normalizedStatus === "confirmed") {
      return {
        id: `execution-${batchStatus?.batchId || fallback.id}-${walletState?.walletKey || "wallet"}`,
        title: `${walletPrefix}${capitalize(normalizedSide)} transaction included`,
        kind: "success",
        linkHref: txSignature ? `https://solscan.io/tx/${txSignature}` : "",
        ttlMs: 3500
      };
    }

    if (normalizedStatus === "failed") {
      const balanceGateReason = balanceGateFailureReason(error);
      if (balanceGateReason) {
        return {
          id: `execution-balance-gate-${batchStatus?.batchId || fallback.id}-${walletState?.walletKey || "wallet"}`,
          title: balanceGateFailureToastTitle(balanceGateReason, 1),
          detail: "",
          kind: "error",
          ttlMs: 3500
        };
      }
      const toastCopy = buildErrorToastCopy(error, { side: normalizedSide });
      return {
        id: `execution-${batchStatus?.batchId || fallback.id}-${walletState?.walletKey || "wallet"}`,
        title: `${walletPrefix}${toastCopy.title}`,
        detail: toastCopy.detail,
        kind: "error",
        linkHref: txSignature ? `https://solscan.io/tx/${txSignature}` : "",
        ttlMs: 3500
      };
    }

    return {
      id: `execution-${batchStatus?.batchId || fallback.id}-${walletState?.walletKey || "wallet"}`,
      title: `${walletPrefix}${capitalize(normalizedSide)} transaction pending...`,
      kind: "info",
      pending: true,
      persistent: true,
      ttlMs: 0
    };
  }

  function zeroTokenBalanceFailureCount(wallets) {
    return (Array.isArray(wallets) ? wallets : []).filter((walletState) => {
      const normalizedStatus = String(walletState?.status || "").trim().toLowerCase();
      return normalizedStatus === "failed" && isZeroTokenBalanceError(walletState?.error);
    }).length;
  }

  function batchExecutionToastPayload(batchStatus, fallback = {}, toastIdOverride = "") {
    const summary = batchStatus?.summary || {};
    const wallets = Array.isArray(batchStatus?.wallets) ? batchStatus.wallets : [];
    const totalWallets = Number(
      summary.totalWallets || fallback.walletCount || wallets.length || 0
    );
    const confirmedWallets = Number(summary.confirmedWallets || 0);
    const failedWallets = Number(summary.failedWallets || 0);
    const pendingWallets = Math.max(0, totalWallets - confirmedWallets - failedWallets);
    const normalizedSide = String(batchStatus?.side || fallback?.side || "trade").trim().toLowerCase();
    const toastId = String(
      toastIdOverride || `execution-batch-${batchStatus?.batchId || fallback.id || "batch"}`
    ).trim();
    const zeroBalanceFailures = zeroTokenBalanceFailureCount(wallets);
    const balanceGateFailures = balanceGateFailureSummary(wallets);

    if (balanceGateFailures && failedWallets === totalWallets && pendingWallets === 0) {
      return {
        id: toastId,
        title: balanceGateFailures.title,
        kind: "error",
        ttlMs: 3500
      };
    }

    if (normalizedSide === "sell" && zeroBalanceFailures > 1 && pendingWallets === 0) {
      return {
        id: toastId,
        title: `${zeroBalanceFailures} wallets missing token balances`,
        kind: "error",
        ttlMs: 3500
      };
    }

    if (failedWallets === totalWallets && totalWallets > 0) {
      return {
        id: toastId,
        title: `${failedWallets} ${normalizedSide} transactions failed`,
        kind: "error",
        ttlMs: 3500
      };
    }

    if (pendingWallets === 0 && confirmedWallets > 0) {
      return {
        id: toastId,
        title: `${confirmedWallets} ${normalizedSide} transactions confirmed`,
        kind: "success",
        ttlMs: 3500
      };
    }

    return {
      id: toastId,
      title: `${totalWallets} ${normalizedSide} transactions pending...`,
      kind: "info",
      pending: true,
      persistent: true,
      ttlMs: 0
    };
  }

  function syncExecutionToastsFromBatchStatus(batchStatus, fallback = {}) {
    const wallets = Array.isArray(batchStatus?.wallets) ? batchStatus.wallets : [];
    const totalWallets = Number(
      batchStatus?.summary?.totalWallets || fallback?.walletCount || wallets.length || 0
    );
    const pendingWallets = Math.max(
      0,
      totalWallets
        - Number(batchStatus?.summary?.confirmedWallets || 0)
        - Number(batchStatus?.summary?.failedWallets || 0)
    );
    const batchId = String(batchStatus?.batchId || fallback?.id || "").trim();
    const localPending = localExecutionPendingForBatch(
      batchId,
      String(batchStatus?.clientRequestId || fallback?.clientRequestId || "").trim()
    );
    if (totalWallets > 1) {
      const zeroBalanceFailures = zeroTokenBalanceFailureCount(wallets);
      const balanceGateFailures = balanceGateFailureSummary(wallets);
      renderToast(
        batchExecutionToastPayload(
          batchStatus,
          fallback,
          batchToastIdForBatch(batchId, localPending)
        )
      );
      if (
        balanceGateFailures &&
        !(Number(batchStatus?.summary?.failedWallets || 0) === totalWallets && pendingWallets === 0)
      ) {
        renderToast({
          id: `execution-balance-gate-${batchId || fallback?.id || "batch"}`,
          title: balanceGateFailures.title,
          kind: "error",
          ttlMs: 3500
        });
      }
      if (pendingWallets === 0 && localPending) {
        releaseLocalExecutionPendingTracking({ clientRequestId: localPending.clientRequestId });
      }
      wallets
        .filter((walletState) => {
          const isFailed = String(walletState?.status || "").trim().toLowerCase() === "failed";
          const isGroupedBalanceGateFailure =
            balanceGateFailures && balanceGateFailureReason(walletState?.error) === balanceGateFailures.reason;
          return isFailed &&
            !isGroupedBalanceGateFailure &&
            (zeroBalanceFailures <= 1 || !isZeroTokenBalanceError(walletState?.error));
        })
        .forEach((walletState) => {
          renderToast(executionToastPayloadForWallet(batchStatus, walletState, fallback));
        });
      const side = String(batchStatus?.side || fallback?.side || "").trim().toLowerCase();
      if (side === "buy" || side === "sell") {
        const anyConfirmed = wallets.some(
          (walletState) => String(walletState?.status || "").toLowerCase() === "confirmed"
        );
        if (batchId && anyConfirmed && rememberBuySoundPlayed(`${side}:${batchId}`)) {
          playSideConfirmationSound(side);
        }
      }
      return;
    }
    const hasTerminalWallet = wallets.some((walletState) => {
      const normalizedStatus = String(walletState?.status || "").trim().toLowerCase();
      return normalizedStatus === "confirmed" || normalizedStatus === "failed";
    });
    if (localPending && !hasTerminalWallet) {
      return;
    }
    if (localPending && hasTerminalWallet) {
      dismissLocalExecutionPending({ clientRequestId: localPending.clientRequestId });
    }
    if (!wallets.length) {
      return;
    }

    wallets.forEach((walletState) => {
      renderToast(executionToastPayloadForWallet(batchStatus, walletState, fallback));
    });

    const side = String(batchStatus?.side || fallback?.side || "").trim().toLowerCase();
    if (side === "buy" || side === "sell") {
      const batchId = String(batchStatus?.batchId || fallback?.id || "").trim();
      const anyConfirmed = wallets.some(
        (walletState) => String(walletState?.status || "").toLowerCase() === "confirmed"
      );
      if (batchId && anyConfirmed && rememberBuySoundPlayed(`${side}:${batchId}`)) {
        playSideConfirmationSound(side);
      }
    }
  }

  function normalizeRouteValue(value) {
    const normalized = String(value || "").trim();
    return normalized || "";
  }

  function getCandidateRouteAddress(candidate) {
    return normalizeRouteValue(candidate?.address || candidate?.mint);
  }

  function getCandidatePairAddress(candidate) {
    return normalizeRouteValue(candidate?.pair);
  }

  function normalizeCompanionPair(address, pair) {
    const normalizedAddress = normalizeRouteValue(address);
    const normalizedPair = normalizeRouteValue(pair);
    return normalizedPair && normalizedPair !== normalizedAddress ? normalizedPair : "";
  }

  function getCandidateMintAddress(candidate) {
    return normalizeRouteValue(candidate?.mint || candidate?.tokenMint);
  }

  function getTokenContextRouteAddress(tokenContext) {
    return normalizeRouteValue(
      tokenContext?.routeAddress
      || tokenContext?.rawAddress
      || tokenContext?.address
      || tokenContext?.mint
    );
  }

  function getTokenContextPairAddress(tokenContext, fallback = "") {
    const pair = normalizeRouteValue(
      fallback
      || tokenContext?.pairAddress
      || tokenContext?.resolvedPair
      || tokenContext?.pair
    );
    return normalizeCompanionPair(getTokenContextRouteAddress(tokenContext), pair);
  }

  function shouldPreferPairRouteForTokenContext(tokenContext) {
    return String(tokenContext?.platform || state.platform || platform || "").trim().toLowerCase() === "axiom";
  }

  function getTokenContextRouteRequest(tokenContext) {
    const routeAddress = getTokenContextRouteAddress(tokenContext) || normalizeRouteValue(tokenContext?.mint);
    const pairAddress = getTokenContextPairAddress(tokenContext);
    // Axiom supplies pair + mint identities; execute against the pair so the
    // engine takes the direct pair-classifier route instead of mint+pair.
    const address = shouldPreferPairRouteForTokenContext(tokenContext) && pairAddress
      ? pairAddress
      : routeAddress;
    return {
      address,
      pair: normalizeCompanionPair(address, pairAddress)
    };
  }

  function buildRouteRequestKey(surface, address, pair = "") {
    const normalizedAddress = normalizeRouteValue(address);
    return [
      String(surface || "").trim(),
      normalizedAddress,
      normalizeCompanionPair(normalizedAddress, pair)
    ].join(":");
  }

  function normalizeInlineRouteReference(routeOrAddress, surface, url = window.location.href, options = {}) {
    if (routeOrAddress && typeof routeOrAddress === "object") {
      return {
        address: getCandidateRouteAddress(routeOrAddress),
        mint: getCandidateMintAddress(routeOrAddress),
        pair: getTokenContextPairAddress(routeOrAddress, options.pair),
        source: String(routeOrAddress.source || options.source || "page").trim() || "page",
        surface: String(routeOrAddress.surface || surface || "").trim(),
        url: String(routeOrAddress.url || url || window.location.href).trim() || window.location.href
      };
    }
    const normalizedAddress = normalizeRouteValue(routeOrAddress);
    return {
      address: normalizedAddress,
      mint: normalizeRouteValue(options.mint),
      pair: normalizeCompanionPair(normalizedAddress, options.pair),
      source: String(options.source || "page").trim() || "page",
      surface: String(surface || "").trim(),
      url: String(url || window.location.href).trim() || window.location.href
    };
  }

  function enrichResolvedTokenContext(tokenContext, fallback = {}) {
    if (!tokenContext || typeof tokenContext !== "object") {
      return tokenContext;
    }
    const fallbackPair = normalizeRouteValue(fallback.pair);
    const fallbackMint = normalizeRouteValue(fallback.mint);
    return enrichTokenContextWithPriceHint({
      ...tokenContext,
      mint: normalizeRouteValue(tokenContext.mint || fallbackMint) || undefined,
      routeAddress:
        getTokenContextRouteAddress(tokenContext) || normalizeRouteValue(fallback.address) || undefined,
      rawAddress:
        normalizeRouteValue(tokenContext.rawAddress || fallback.address) || undefined,
      pairAddress:
        getTokenContextPairAddress(tokenContext, fallbackPair) || undefined
    });
  }

  async function refreshCurrentToken(options = {}) {
    const requestSeq = Number.isInteger(options.requestSeq) ? options.requestSeq : ++state.tokenRequestSeq;
    const requestUrl = window.location.href;
    const candidate = await getCurrentTokenCandidate();
    const candidateAddress = getCandidateRouteAddress(candidate);
    if (!candidateAddress) {
      if (requestSeq === state.tokenRequestSeq && requestUrl === state.currentRouteUrl) {
        setPageTokenContext(null);
      }
      return null;
    }

    const candidatePair = getCandidatePairAddress(candidate);
    const candidateMint = getCandidateMintAddress(candidate);
    const cacheKey = buildRouteRequestKey(candidate.surface, candidateAddress, candidatePair);
    if (state.pendingMintRequests.has(cacheKey)) {
      return state.pendingMintRequests.get(cacheKey);
    }

    const resolvePayload = {
      source: candidate.source,
      platform,
      surface: candidate.surface,
      url: candidate.url || window.location.href,
      address: candidateAddress,
      mint: candidateMint || undefined,
      pair: candidatePair || undefined
    };
    const promise = callBackground("trench:resolve-token", resolvePayload)
      .then((tokenContext) => {
        const enriched = enrichResolvedTokenContext(tokenContext, {
          address: candidateAddress,
          mint: candidateMint,
          pair: candidatePair
        });
        if (requestSeq !== state.tokenRequestSeq || requestUrl !== state.currentRouteUrl) {
          return state.tokenContext;
        }
        setPageTokenContext(enriched);
        // Fire a best-effort prewarm now that we know the active raw route
        // address. The backend de-dupes concurrent warms for the same
        // fingerprint, so calling here is cheap.
        schedulePrewarmForTokenContext(enriched, "resolve-token");
        return enriched;
      })
      .finally(() => state.pendingMintRequests.delete(cacheKey));

    state.pendingMintRequests.set(cacheKey, promise);
    return promise;
  }

  // Trade warming is only useful when the user actually intends to
  // trade. We fire `trench:prewarm-mint` from three high-signal UI
  // events: token-detail mount (via `refreshCurrentToken`), panel
  // open, and debounced hover on trade controls. All calls share a
  // short-lived "already warmed" memo keyed by the raw route address so
  // hover churn doesn't flood the host.
  const PREWARM_MEMO_TTL_MS = 20_000;
  const PREWARM_RESPONSE_FALLBACK_TTL_MS = 30_000;
  const prewarmMemo = new Map();

  function normalizePrewarmKeyPart(value, options = {}) {
    const normalized = String(value || "").trim();
    if (!normalized) {
      return "-";
    }
    if (options.uppercase) {
      return normalized.toUpperCase();
    }
    if (options.lowercase) {
      return normalized.toLowerCase();
    }
    return normalized;
  }

  function prewarmPolicyShapeFromSource(source = {}) {
    return {
      buyFundingPolicy: backendPolicyValue(source.buyFundingPolicy) || backendPolicyValue(activePolicyValue("buy")),
      sellSettlementPolicy: backendPolicyValue(source.sellSettlementPolicy) || backendPolicyValue(activePolicyValue("sell"))
    };
  }

  function readPrewarmMemoKey(routeShape = {}) {
    const address = normalizePrewarmKeyPart(routeShape.address);
    const pair = normalizePrewarmKeyPart(normalizeCompanionPair(routeShape.address, routeShape.pair));
    const policyShape = prewarmPolicyShapeFromSource(routeShape);
    return [
      `address=${address}`,
      `pair=${pair}`,
      `buyPolicy=${normalizePrewarmKeyPart(policyShape.buyFundingPolicy, { lowercase: true })}`,
      `sellPolicy=${normalizePrewarmKeyPart(policyShape.sellSettlementPolicy, { lowercase: true })}`
    ].join("|");
  }

  function prewarmRouteShapeFromTokenContext(tokenContext) {
    const routeRequest = getTokenContextRouteRequest(tokenContext);
    return {
      address: routeRequest.address,
      pair: routeRequest.pair
    };
  }

  function buildPrewarmPayload(tokenContext, side = "") {
    const routeRequest = getTokenContextRouteRequest(tokenContext);
    const address = routeRequest.address;
    if (!address) {
      return null;
    }
    const normalizedSide = String(side || "").trim().toLowerCase();
    const payload = {
      address,
      mint: normalizeRouteValue(tokenContext?.mint) || undefined,
      pair: routeRequest.pair || undefined,
      platform: normalizeRouteValue(tokenContext?.platform || state.platform) || undefined,
      sourceUrl: tokenContext.sourceUrl || tokenContext.url || window.location.href
    };
    if (normalizedSide === "buy" || normalizedSide === "sell") {
      payload.side = normalizedSide;
    }
    const buyFundingPolicy = activePolicyValue("buy");
    const sellSettlementPolicy = activePolicyValue("sell");
    if (buyFundingPolicy) {
      payload.buyFundingPolicy = backendPolicyValue(buyFundingPolicy);
    }
    if (sellSettlementPolicy) {
      payload.sellSettlementPolicy = backendPolicyValue(sellSettlementPolicy);
    }
    return payload;
  }

  function unwrapPrewarmHostResponse(result) {
    if (!result || typeof result !== "object") {
      return null;
    }
    if (result.ok === true && result.data && typeof result.data === "object") {
      return result.data;
    }
    if (typeof result.warmKey === "string" && result.warmKey.trim()) {
      return result;
    }
    return null;
  }

  function pruneExpiredPrewarmSnapshots() {
    const now = Date.now();
    for (const [key, snapshot] of state.prewarmSnapshots.entries()) {
      if (!snapshot || snapshot.expiresAt <= now) {
        state.prewarmSnapshots.delete(key);
      }
    }
  }

  function prewarmSnapshotKeys(requestPayload, response) {
    const policyShape = prewarmPolicyShapeFromSource(requestPayload);
    const requestShape = {
      address: String(requestPayload?.address || "").trim(),
      pair: String(requestPayload?.pair || "").trim(),
      ...policyShape
    };
    const rawAddress = String(response?.rawAddress || "").trim();
    const resolvedPair = String(response?.resolvedPair || "").trim();
    const variants = [
      requestShape,
      { address: rawAddress, pair: requestShape.pair, ...policyShape },
      {
        address: String(response?.resolvedMint || "").trim(),
        pair: resolvedPair || requestShape.pair,
        ...policyShape
      },
      { address: rawAddress, pair: resolvedPair, ...policyShape }
    ];
    return [...new Set(
      variants
        .filter((variant) => variant.address)
        .map((variant) => readPrewarmMemoKey(variant))
    )];
  }

  function rememberPrewarmResponse(requestPayload, response) {
    if (!response || typeof response !== "object") {
      return null;
    }
    const warmKey = String(response.warmKey || "").trim();
    if (!warmKey) {
      return null;
    }
    const staleAfterMs = Number(response.staleAfterMs);
    const ttlMs = Number.isFinite(staleAfterMs) && staleAfterMs > 0
      ? staleAfterMs
      : PREWARM_RESPONSE_FALLBACK_TTL_MS;
    const snapshot = {
      response,
      cachedAt: Date.now(),
      expiresAt: Date.now() + ttlMs
    };
    pruneExpiredPrewarmSnapshots();
    prewarmSnapshotKeys(requestPayload, response).forEach((key) => {
      state.prewarmSnapshots.set(key, snapshot);
    });
    return response;
  }

  function readCachedPrewarmResponse(tokenContext) {
    const routeShape = {
      ...prewarmRouteShapeFromTokenContext(tokenContext),
      ...prewarmPolicyShapeFromSource()
    };
    if (!String(routeShape.address || "").trim()) {
      return null;
    }
    pruneExpiredPrewarmSnapshots();
    const candidateKeys = [readPrewarmMemoKey(routeShape)];
    for (const key of candidateKeys) {
      const snapshot = state.prewarmSnapshots.get(key);
      if (snapshot?.response) {
        return snapshot.response;
      }
    }
    return null;
  }

  function cachedPrewarmSatisfiesSide(response, side = "") {
    if (!response) {
      return false;
    }
    const normalizedSide = String(side || "").trim().toLowerCase();
    if (normalizedSide === "sell") {
      return response.sellWarmed === true;
    }
    if (normalizedSide === "buy") {
      return response.buyWarmed !== false;
    }
    return true;
  }

  async function requestAndRememberPrewarm(payload) {
    if (!payload?.address) {
      return null;
    }
    const result = await callBackground("trench:prewarm-mint", payload);
    const response = unwrapPrewarmHostResponse(result);
    if (response) {
      return rememberPrewarmResponse(payload, response);
    }
    return null;
  }

  async function ensurePrewarmResponse(tokenContext, side = "") {
    const cached = readCachedPrewarmResponse(tokenContext);
    if (cachedPrewarmSatisfiesSide(cached, side)) {
      return cached;
    }
    const payload = buildPrewarmPayload(tokenContext, side);
    if (!payload) {
      return null;
    }
    try {
      return await requestAndRememberPrewarm(payload);
    } catch {
      return null;
    }
  }

  async function resolveTradeWarmReuseFields(tokenContext, options = {}) {
    const prewarmResponse = options.skipBlockingPrewarm
      ? readCachedPrewarmResponse(tokenContext)
      : await ensurePrewarmResponse(tokenContext, options.side);
    return buildWarmReuseFields(prewarmResponse, options.side);
  }

  function buildWarmReuseFields(prewarmResponse = null, side = "") {
    const normalizedSide = String(side || "").trim().toLowerCase();
    const hasSideSpecificWarmKeys =
      Object.prototype.hasOwnProperty.call(prewarmResponse || {}, "buyWarmKey") ||
      Object.prototype.hasOwnProperty.call(prewarmResponse || {}, "sellWarmKey");
    const sideWarmKey = normalizedSide === "sell"
      ? prewarmResponse?.sellWarmKey
      : prewarmResponse?.buyWarmKey;
    return {
      warmKey: sideWarmKey || (!hasSideSpecificWarmKeys ? prewarmResponse?.warmKey : undefined)
    };
  }

  function schedulePrewarmForTokenContext(tokenContext, reason) {
    if (!getTokenContextRouteAddress(tokenContext) && !tokenContext?.mint) {
      return;
    }
    const payload = buildPrewarmPayload(tokenContext);
    if (payload) {
      schedulePrewarm(payload, reason);
    }
  }

  function schedulePrewarm(payload, reason) {
    if (!payload?.address) {
      return;
    }
    const key = readPrewarmMemoKey(payload);
    const now = Date.now();
    const memoed = prewarmMemo.get(key);
    if (memoed && now - memoed < PREWARM_MEMO_TTL_MS) {
      return;
    }
    prewarmMemo.set(key, now);
    // Fire and forget — prewarm failures must never break the
    // user-facing trade path.
    requestAndRememberPrewarm(payload).catch(() => {});
  }

  // Exposed to inline-panel callers so hover/focus handlers inside
  // platform adapters can drive prewarm from card controls too.
  const prewarmForMint = (routeOrAddress, options = {}) => {
    const routeRef = normalizeInlineRouteReference(routeOrAddress, options.surface || "", options.sourceUrl, options);
    if (!routeRef.address) {
      return;
    }
    const policyShape = prewarmPolicyShapeFromSource();
    const side = String(options.side || "").trim().toLowerCase();
    schedulePrewarm(
      {
        address: routeRef.address,
        mint: routeRef.mint || undefined,
        pair: routeRef.pair || undefined,
        platform: normalizeRouteValue(options.platform || platform) || undefined,
        sourceUrl: options.sourceUrl || window.location.href,
        ...(side === "buy" || side === "sell" ? { side } : {}),
        ...(policyShape.buyFundingPolicy ? { buyFundingPolicy: policyShape.buyFundingPolicy } : {}),
        ...(policyShape.sellSettlementPolicy ? { sellSettlementPolicy: policyShape.sellSettlementPolicy } : {})
      },
      options.reason || "adapter-hover"
    );
  };

  // The host's balance stream only subscribes to per-wallet ATAs for
  // mints someone is actively viewing. Tell it which mint this content
  // script currently cares about so the wallet-token cache actually
  // receives events (and so SOL-panel balance polling collapses into
  // one live subscription instead of polling).
  const ACTIVE_MINTS_SURFACE_ID = `content:${platform || "unknown"}:${Date.now().toString(36)}:${Math.random().toString(36).slice(2)}`;
  let lastActiveMint = "";
  let lastActiveMarkSignature = "";
  let activeMarkInflight = null;
  let pendingActiveMarkRequest = null;
  function setActiveMintForSurface(mint) {
    const normalized = typeof mint === "string" ? mint.trim() : "";
    if (normalized === lastActiveMint) {
      return;
    }
    lastActiveMint = normalized;
    const payload = {
      surfaceId: ACTIVE_MINTS_SURFACE_ID,
      mints: normalized ? [normalized] : []
    };
    // Fire-and-forget. A failure here has zero user-visible impact —
    // sells fall back to the RPC path, and the next active-mint call
    // will retry.
    callBackground("trench:set-active-mints", payload).catch(() => {});
  }
  function clearActiveMintForSurface() {
    setActiveMintForSurface("");
  }

  function clearActiveMarkForSurface() {
    enqueueActiveMarkPayload({ active: false, surfaceId: ACTIVE_MINTS_SURFACE_ID }, { force: true });
  }

  function walletStatusTokenAmount(status = state.walletStatus) {
    const amount = Number(
      status?.holdingAmount ??
      status?.tokenBalance ??
      status?.mintBalanceUi ??
      status?.mintBalance
    );
    return Number.isFinite(amount) ? amount : 0;
  }

  function walletStatusTokenDecimals(status = state.walletStatus) {
    const decimals = Number(status?.tokenDecimals ?? status?.mintDecimals);
    return Number.isInteger(decimals) && decimals >= 0 ? decimals : null;
  }

  function walletStatusTokenRaw(status = state.walletStatus) {
    const raw = Number(status?.tokenBalanceRaw ?? status?.mintBalanceRaw);
    return Number.isFinite(raw) && raw >= 0 ? Math.round(raw) : null;
  }

  function activeMarkWalletBalances(status = state.walletStatus) {
    return Array.isArray(status?.wallets)
      ? status.wallets
        .map((wallet) => ({
          envKey: String(wallet?.envKey || wallet?.key || "").trim(),
          tokenBalance: Number(
            wallet?.tokenBalance ??
            wallet?.mintBalanceUi ??
            wallet?.mintBalance ??
            wallet?.holdingAmount
          )
        }))
        .filter((entry) => entry.envKey && Number.isFinite(entry.tokenBalance) && entry.tokenBalance >= 0)
      : [];
  }

  function buildActiveMarkPayload() {
    if (!(state.panelOpen || state.quickPanelOpen) || document.visibilityState === "hidden") {
      return { active: false, surfaceId: ACTIVE_MINTS_SURFACE_ID };
    }
    const tokenContext = currentActivePanelTokenContext();
    const mint = String(tokenContext?.mint || state.walletStatus?.mint || "").trim();
    if (!tokenContext || !mint) {
      return { active: false, surfaceId: ACTIVE_MINTS_SURFACE_ID };
    }
    const walletStatusMint = String(state.walletStatus?.mint || "").trim();
    if (walletStatusMint && walletStatusMint !== mint) {
      return { active: false, surfaceId: ACTIVE_MINTS_SURFACE_ID };
    }
    const tokenBalance = walletStatusTokenAmount();
    if (!(tokenBalance > 0)) {
      return { active: false, surfaceId: ACTIVE_MINTS_SURFACE_ID };
    }
    const payload = buildWalletStatusRequestPayload({ tokenContext, force: false });
    payload.active = true;
    payload.surfaceId = ACTIVE_MINTS_SURFACE_ID;
    payload.mint = mint;
    payload.tokenBalance = tokenBalance;
    const raw = walletStatusTokenRaw();
    if (raw != null) {
      payload.tokenBalanceRaw = raw;
    }
    const decimals = walletStatusTokenDecimals();
    if (decimals != null) {
      payload.tokenDecimals = decimals;
    }
    payload.walletTokenBalances = activeMarkWalletBalances();
    return payload;
  }

  function syncActiveMarkSubscription() {
    const payload = buildActiveMarkPayload();
    enqueueActiveMarkPayload(payload);
  }

  function enqueueActiveMarkPayload(payload, { force = false } = {}) {
    const signature = JSON.stringify(payload);
    if (!force && signature === lastActiveMarkSignature && !activeMarkInflight) {
      return;
    }
    pendingActiveMarkRequest = { payload, signature, force };
    flushActiveMarkQueue();
  }

  function flushActiveMarkQueue() {
    if (activeMarkInflight || !pendingActiveMarkRequest) {
      return;
    }
    const request = pendingActiveMarkRequest;
    pendingActiveMarkRequest = null;
    if (!request.force && request.signature === lastActiveMarkSignature) {
      flushActiveMarkQueue();
      return;
    }
    activeMarkInflight = callBackground("trench:set-active-mark", request.payload)
      .then(() => {
        lastActiveMarkSignature = request.signature;
      })
      .catch(() => {
        lastActiveMarkSignature = "";
      })
      .finally(() => {
        activeMarkInflight = null;
        flushActiveMarkQueue();
      });
  }

  async function getCurrentTokenCandidate() {
    return getPlatformAdapter().getCurrentTokenCandidate();
  }

  function enrichTokenContextWithPriceHint(tokenContext) {
    if (!tokenContext) {
      return tokenContext;
    }
    const adapter = getPlatformAdapter();
    if (typeof adapter.getQuotedPriceHint !== "function") {
      return tokenContext;
    }
    const quotedPrice = Number(adapter.getQuotedPriceHint(tokenContext));
    if (!Number.isFinite(quotedPrice) || quotedPrice <= 0) {
      return tokenContext;
    }
    return {
      ...tokenContext,
      quotedPrice
    };
  }

  function mountPlatformObserver() {
    if (lifecycle.destroyed) {
      return;
    }
    scanAndMount();
    const observer = new MutationObserver((mutations) => {
      if (lifecycle.destroyed) {
        return;
      }
      try {
        if (window.location.href !== state.currentRouteUrl) {
          scheduleRouteReconcile("mutation-route-change", 0);
        }
        const adapter = getPlatformAdapter();
        if (typeof adapter.handleMutations === "function") {
          const handled = adapter.handleMutations(mutations);
          if (handled !== false) {
            return;
          }
        }
        scheduleMountScan();
      } catch (error) {
        if (isExtensionReloadedError(error)) {
          scheduleExtensionReloadFallbackToast();
          return;
        }
        console.error("Trench Tools mutation observer failed", error);
      }
    });
    let observerOptions;
    try {
      observerOptions = getPlatformAdapter().getObserverOptions?.() || {
        subtree: true,
        childList: true,
        attributes: false
      };
    } catch (error) {
      if (isExtensionReloadedError(error)) {
        scheduleExtensionReloadFallbackToast();
        return;
      }
      throw error;
    }
    observer.observe(document.documentElement, observerOptions);
    state.mutationObserver = observer;
  }

  let debounceTimer = 0;
  let scanFrameId = 0;
  function debounce(callback, delay) {
    window.clearTimeout(debounceTimer);
    debounceTimer = window.setTimeout(callback, delay);
  }

  function scheduleMountScan() {
    if (!scanFrameId) {
      scanFrameId = window.requestAnimationFrame(() => {
        scanFrameId = 0;
        scanAndMount();
      });
    }
    debounce(scanAndMount, 90);
  }

  function scanAndMount() {
    if (lifecycle.destroyed) {
      return;
    }
    try {
      if (window.location.href !== state.currentRouteUrl) {
        scheduleRouteReconcile("scan-route-change", 0);
      }
      enforcePersistentSurfaceBoundary();
      getPlatformAdapter().mount();
      let liveTokenCandidate = state.tokenContext;
      try {
        liveTokenCandidate = getPlatformAdapter()?.getCurrentTokenCandidate?.() || state.tokenContext;
      } catch {
        liveTokenCandidate = state.tokenContext;
      }
      ensureFloatingLauncher(liveTokenCandidate);
    } catch (error) {
      if (isExtensionReloadedError(error)) {
        scheduleExtensionReloadFallbackToast();
        return;
      }
      console.error("Trench Tools scan-and-mount failed", error);
    }
  }

  function injectInlineControls(target, mint, surface) {
    if (!target || target.dataset.trenchToolsMounted === mint) {
      return;
    }

    target.dataset.trenchToolsMounted = mint;

    const quickBuyButton = buildInlineButton(async () => {
      const tokenContext = await resolveInlineToken(mint, surface);
      if (!tokenContext) {
        return;
      }
      await handleTradeRequest("buy", {
        ...state.preferences,
        buyAmountSol: resolveQuickBuyAmount()
      }, {
        persistPreferences: false,
        tokenContextOverride: tokenContext
      });
    });

    quickBuyButton.setAttribute("data-trench-tools-inline", mint);
    target.insertAdjacentElement("afterend", quickBuyButton);
  }

  function buildInlineButton(onClick, styleSet = quickBuyStyles()) {
    const button = document.createElement("button");
    button.type = "button";
    const logo = document.createElement("img");
    logo.src = INLINE_LOGO_URL;
    logo.alt = "";
    Object.assign(logo.style, {
      width: "14px",
      height: "14px",
      objectFit: "contain",
      flexShrink: "0",
      marginRight: "4px",
      pointerEvents: "none"
    });

    const label = document.createElement("span");
    Object.assign(label.style, {
      whiteSpace: "nowrap",
      lineHeight: "1",
      pointerEvents: "none"
    });

    button.append(logo, label);
    button._trenchInlineLabel = label;
    button._trenchInlineLogo = logo;
    setInlineButtonLabel(button, quickBuyLabel());
    setInlineButtonStyleSet(button, styleSet);
    button.addEventListener("mouseenter", () => {
      if (button._trenchInlineStyles) {
        Object.assign(button.style, button._trenchInlineStyles.hover);
      }
    });
    button.addEventListener("mouseleave", () => {
      if (button._trenchInlineStyles) {
        Object.assign(button.style, button._trenchInlineStyles.base);
      }
    });
    button.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      onClick().catch((error) => {
        surfaceUserFacingError(error);
      });
    });
    return button;
  }

  function buildInlineIconButton(onClick, styleSet = quickBuyStyles()) {
    const button = document.createElement("button");
    button.type = "button";

    const logo = document.createElement("img");
    logo.src = INLINE_LOGO_URL;
    logo.alt = "";
    Object.assign(logo.style, {
      width: "14px",
      height: "14px",
      objectFit: "contain",
      flexShrink: "0",
      pointerEvents: "none"
    });

    button.appendChild(logo);
    button._trenchInlineLogo = logo;
    setInlineButtonStyleSet(button, styleSet);
    button.addEventListener("mouseenter", () => {
      if (button._trenchInlineStyles) {
        Object.assign(button.style, button._trenchInlineStyles.hover);
      }
    });
    button.addEventListener("mouseleave", () => {
      if (button._trenchInlineStyles) {
        Object.assign(button.style, button._trenchInlineStyles.base);
      }
    });
    button.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      onClick().catch((error) => {
        surfaceUserFacingError(error);
      });
    });
    return button;
  }

  function setInlineButtonStyleSet(button, styleSet) {
    button._trenchInlineStyles = styleSet;
    Object.assign(button.style, styleSet.base);
    if (button._trenchInlineLogo instanceof HTMLImageElement) {
      const logoSize = styleSet.logoSize || "14px";
      const hasVisibleLabel = Boolean(button?._trenchInlineLabel?.textContent);
      const logoGap = hasVisibleLabel ? styleSet.logoGap || "4px" : "0px";
      Object.assign(button._trenchInlineLogo.style, {
        width: logoSize,
        height: logoSize,
        marginRight: logoGap
      });
    }
  }

  function setInlineButtonLabel(button, value) {
    if (button?._trenchInlineLabel instanceof HTMLElement) {
      const labelValue = String(value || "");
      button._trenchInlineLabel.textContent = labelValue;
      button._trenchInlineLabel.style.display = labelValue ? "" : "none";
      if (button._trenchInlineStyles) {
        setInlineButtonStyleSet(button, button._trenchInlineStyles);
      }
    } else {
      button.textContent = value;
    }
  }

  function removeInjectedControls(selector) {
    document.querySelectorAll(selector).forEach((element) => {
      teardownInlineSizeSync(element);
      element.remove();
    });
  }

  function attachInlineSizeSync(target, button, styleFactory) {
    if (!(target instanceof HTMLElement) || !(button instanceof HTMLButtonElement)) {
      return;
    }

    if (
      button._trenchInlineSyncTarget === target &&
      button._trenchInlineResizeObserver &&
      button._trenchInlineStyleFactory === styleFactory
    ) {
      return;
    }

    teardownInlineSizeSync(button);

    if (typeof ResizeObserver !== "function") {
      return;
    }

    const observer = new ResizeObserver(() => {
      if (!document.contains(target) || !document.contains(button)) {
        teardownInlineSizeSync(button);
        return;
      }
      setInlineButtonStyleSet(button, styleFactory(target));
    });

    observer.observe(target);
    button._trenchInlineSyncTarget = target;
    button._trenchInlineResizeObserver = observer;
    button._trenchInlineStyleFactory = styleFactory;
  }

  function teardownInlineSizeSync(button) {
    if (!(button instanceof HTMLElement)) {
      return;
    }

    if (typeof button._trenchInlineCleanup === "function") {
      button._trenchInlineCleanup();
      button._trenchInlineCleanup = null;
    }
    if (button._trenchInlineResizeObserver) {
      button._trenchInlineResizeObserver.disconnect();
      button._trenchInlineResizeObserver = null;
    }
    button._trenchInlineSyncTarget = null;
    button._trenchInlineStyleFactory = null;
  }

  async function resolveInlineToken(routeOrAddress, surface, url = window.location.href, options = {}) {
    const routeRef = normalizeInlineRouteReference(routeOrAddress, surface, url, options);
    const cacheKey = buildRouteRequestKey(routeRef.surface, routeRef.address, routeRef.pair);
    if (state.pendingMintRequests.has(cacheKey)) {
      return state.pendingMintRequests.get(cacheKey);
    }
    const promise = resolveInlineTokenInternal(routeRef, surface, url, options).finally(() => {
      state.pendingMintRequests.delete(cacheKey);
    });
    state.pendingMintRequests.set(cacheKey, promise);
    return promise;
  }

  async function resolveInlineTokenInternal(routeOrAddress, surface, url = window.location.href, options = {}) {
    const routeRef = normalizeInlineRouteReference(routeOrAddress, surface, url, options);
    if (!routeRef.address) {
      return null;
    }
    try {
      const tokenContext = await callBackground("trench:resolve-token", {
        source: routeRef.source || "page",
        platform,
        surface: routeRef.surface,
        url: routeRef.url,
        address: routeRef.address,
        mint: routeRef.mint || undefined,
        pair: routeRef.pair || undefined
      });
      const enriched = enrichResolvedTokenContext({
        ...tokenContext,
        url: routeRef.url
      }, {
        address: routeRef.address,
        mint: routeRef.mint,
        pair: routeRef.pair
      });
      return enriched;
    } catch (error) {
      if (!options.silent) {
        surfaceUserFacingError(error);
      }
      return null;
    }
  }

  function setInlineTokenContext(routeOrAddress, surface, url = window.location.href, options = {}) {
    const routeRef = normalizeInlineRouteReference(routeOrAddress, surface, url, options);
    return enrichResolvedTokenContext({
      platform,
      surface: routeRef.surface,
      mint: routeRef.mint || routeRef.address,
      routeAddress: routeRef.address,
      pairAddress: routeRef.pair || undefined,
      url: routeRef.url
    }, {
      address: routeRef.address,
      mint: routeRef.mint,
      pair: routeRef.pair
    });
  }

  function setPanelTokenContext(tokenContext) {
    return setPersistentPanelTokenContext(tokenContext);
  }

  async function refreshPanelTokenContext(options = {}) {
    const quickMode = options.mode === "quick" || (options.mode == null && state.quickPanelOpen);
    const baseTokenContext = quickMode ? state.quickPanelTokenContext : state.panelTokenContext;
    const baseAddress = baseTokenContext?.routeAddress || baseTokenContext?.mint;
    if (!baseAddress || !baseTokenContext?.surface) {
      return null;
    }
    const resolved = await resolveInlineTokenInternal(
      {
        address: baseAddress,
        mint: baseTokenContext.mint,
        pair: getTokenContextPairAddress(baseTokenContext) || undefined,
        surface: baseTokenContext.surface,
        url: baseTokenContext.url || window.location.href
      },
      baseTokenContext.surface,
      baseTokenContext.url || window.location.href,
      options
    );
    if (resolved) {
      const nextTokenContext = {
        ...baseTokenContext,
        ...resolved,
        url: baseTokenContext.url || resolved.url || window.location.href
      };
      if (quickMode) {
        setQuickPanelTokenContext(nextTokenContext);
      } else {
        setPersistentPanelTokenContext(nextTokenContext);
      }
      await refreshPanelWalletStatus({
        tokenContext: nextTokenContext,
        force: true
      });
    }
    return quickMode ? state.quickPanelTokenContext : state.panelTokenContext;
  }

  async function handleInlineTradeRequest(side, routeOrAddress, surface, payload, url = window.location.href, options = {}) {
    const tokenContext = setInlineTokenContext(routeOrAddress, surface, url, options);
    await handleTradeRequest(side, payload, {
      persistPreferences: false,
      tokenContextOverride: tokenContext,
      skipBlockingPrewarm: options.skipBlockingPrewarm === true
    });
  }

  function localExecutionPendingForBatch(batchId = "", clientRequestId = "") {
    const normalizedBatchId = String(batchId || "").trim();
    if (normalizedBatchId) {
      const pendingClientRequestId = state.localExecutionPendingBatchIds.get(normalizedBatchId);
      if (pendingClientRequestId) {
        return state.localExecutionPendings.get(pendingClientRequestId) || null;
      }
    }
    const normalizedClientRequestId = String(clientRequestId || "").trim();
    if (normalizedClientRequestId) {
      return state.localExecutionPendings.get(normalizedClientRequestId) || null;
    }
    return null;
  }

  function selectedWalletCountForRequest(selection = {}) {
    if (Array.isArray(selection.walletKeys) && selection.walletKeys.length) {
      return selection.walletKeys.length;
    }
    if (selection.walletKey) {
      return 1;
    }
    const walletGroupId = String(selection.walletGroupId || "").trim();
    if (walletGroupId) {
      const walletGroups = Array.isArray(state.bootstrap?.walletGroups) ? state.bootstrap.walletGroups : [];
      const group = walletGroups.find((entry) => String(entry?.id || "").trim() === walletGroupId);
      const walletKeys = Array.isArray(group?.walletKeys) ? group.walletKeys.filter(Boolean) : [];
      return walletKeys.length;
    }
    return 0;
  }

  function rememberLocalExecutionPending({ clientRequestId, side, walletCount = 1 }) {
    const normalizedClientRequestId = String(clientRequestId || "").trim();
    if (!normalizedClientRequestId) {
      return;
    }
    const existing = state.localExecutionPendings.get(normalizedClientRequestId);
    if (existing) {
      return existing;
    }
    const toastId = `execution-local-${normalizedClientRequestId}`;
    const normalizedSide = String(side || "trade").trim().toLowerCase();
    const normalizedWalletCount = Math.max(1, Number(walletCount || 1));
    renderToast({
      id: toastId,
      title: normalizedWalletCount > 1
        ? `${normalizedWalletCount} ${normalizedSide} transactions pending...`
        : `${capitalize(normalizedSide)} transaction pending...`,
      kind: "info",
      pending: true,
      persistent: true,
      ttlMs: 0
    });
    const record = {
      clientRequestId: normalizedClientRequestId,
      toastId,
      side: String(side || "trade").trim().toLowerCase(),
      batchId: ""
    };
    state.localExecutionPendings.set(normalizedClientRequestId, record);
    return record;
  }

  function bindLocalExecutionPendingToBatch(clientRequestId, batchId) {
    const normalizedClientRequestId = String(clientRequestId || "").trim();
    const normalizedBatchId = String(batchId || "").trim();
    if (!normalizedClientRequestId || !normalizedBatchId) {
      return;
    }
    const record = state.localExecutionPendings.get(normalizedClientRequestId);
    if (!record) {
      return;
    }
    record.batchId = normalizedBatchId;
    state.localExecutionPendingBatchIds.set(normalizedBatchId, normalizedClientRequestId);
    state.batchToastIds.set(normalizedBatchId, record.toastId);
  }

  function dismissLocalExecutionPending({ clientRequestId = "", batchId = "" } = {}) {
    const record = localExecutionPendingForBatch(batchId, clientRequestId);
    if (!record) {
      return;
    }
    dismissToast(record.toastId);
    state.localExecutionPendings.delete(record.clientRequestId);
    if (record.batchId) {
      state.localExecutionPendingBatchIds.delete(record.batchId);
      state.batchToastIds.delete(record.batchId);
    }
  }

  function releaseLocalExecutionPendingTracking({ clientRequestId = "", batchId = "" } = {}) {
    const record = localExecutionPendingForBatch(batchId, clientRequestId);
    if (!record) {
      return;
    }
    state.localExecutionPendings.delete(record.clientRequestId);
    if (record.batchId) {
      state.localExecutionPendingBatchIds.delete(record.batchId);
    }
  }

  function batchToastIdForBatch(batchId = "", localPending = null) {
    if (localPending?.toastId) {
      return localPending.toastId;
    }
    const normalizedBatchId = String(batchId || "").trim();
    if (normalizedBatchId) {
      return state.batchToastIds.get(normalizedBatchId) || `execution-batch-${normalizedBatchId}`;
    }
    return "execution-batch-unknown";
  }

  function currentQuickPanelAnchor() {
    if (state.quickPanelAnchorElement instanceof HTMLElement && document.body.contains(state.quickPanelAnchorElement)) {
      return state.quickPanelAnchorElement;
    }
    if (!state.quickPanelAnchorRect) {
      return null;
    }
    return {
      getBoundingClientRect() {
        return state.quickPanelAnchorRect;
      }
    };
  }

  function positionQuickPanel(anchor) {
    if (!state.quickPanelWrapper || !anchor) {
      return;
    }
    const anchorRect = anchor.getBoundingClientRect();
    state.quickPanelAnchorElement = anchor instanceof HTMLElement ? anchor : null;
    state.quickPanelAnchorRect = {
      left: anchorRect.left,
      top: anchorRect.top,
      right: anchorRect.right,
      bottom: anchorRect.bottom,
      width: anchorRect.width,
      height: anchorRect.height
    };
    const panelRect = state.quickPanelWrapper.getBoundingClientRect();
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;
    let left = anchorRect.right + 10;
    let top = anchorRect.top;
    if (left + panelRect.width > viewportWidth) {
      left = anchorRect.left - panelRect.width - 10;
    }
    const anchorMidY = anchorRect.top + (anchorRect.height / 2);
    const topThird = viewportHeight / 3;
    if (anchorMidY < topThird) {
      top = anchorRect.bottom + 10;
    } else if (anchorMidY > topThird * 2) {
      top = anchorRect.top - panelRect.height - 10;
    } else {
      top = anchorRect.top - ((panelRect.height - anchorRect.height) / 2);
    }
    left = Math.max(10, Math.min(viewportWidth - panelRect.width - 10, left));
    top = Math.max(10, Math.min(viewportHeight - panelRect.height - 10, top));
    Object.assign(state.quickPanelWrapper.style, {
      left: `${left}px`,
      top: `${top}px`,
      transform: "none"
    });
  }

  function clearQuickPanelCloseHandlers() {
    if (!state.quickPanelCloseHandlers) {
      return;
    }
    const { click, keydown, resize, scroll, mutation } = state.quickPanelCloseHandlers;
    if (click) {
      document.removeEventListener("click", click, true);
    }
    if (keydown) {
      document.removeEventListener("keydown", keydown, true);
    }
    if (resize) {
      window.removeEventListener("resize", resize);
    }
    if (scroll) {
      window.removeEventListener("scroll", scroll, true);
    }
    if (mutation) {
      mutation.disconnect();
    }
    state.quickPanelCloseHandlers = null;
  }

  function clearQuickPanelLifecycleCleanup() {
    const cleanup = state.quickPanelLifecycleCleanup;
    state.quickPanelLifecycleCleanup = null;
    if (typeof cleanup === "function") {
      try {
        cleanup();
      } catch (error) {
        console.warn("Trench Tools quick panel cleanup failed", error);
      }
    }
  }

  function closeQuickPanel() {
    clearQuickPanelCloseHandlers();
    clearQuickPanelLifecycleCleanup();
    hidePanelFlyoutOverlay();
    if (state.quickPanelWrapper) {
      disconnectPanelLayerObserver(state.quickPanelWrapper);
      state.quickPanelWrapper.remove();
    }
    document.querySelectorAll(".trench-tools-pulse-panel-owner").forEach((element) => {
      element.classList.remove("trench-tools-pulse-panel-owner");
    });
    state.quickPanelWrapper = null;
    state.quickPanelFrame = null;
    state.quickPanelReady = false;
    state.quickPanelOpen = false;
    state.quickPanelAnchorRect = null;
    state.quickPanelAnchorElement = null;
    setQuickPanelTokenContext(null);
    syncWalletStatusQuoteRefresh();
  }

  function registerQuickPanelCloseHandlers(anchor) {
    clearQuickPanelCloseHandlers();
    const click = (event) => {
      const target = event.target;
      const insidePanel = state.quickPanelWrapper?.contains(target);
      const insideAnchor = anchor instanceof HTMLElement && anchor.contains(target);
      if (!insidePanel && !insideAnchor) {
        closeQuickPanel();
      }
    };
    const keydown = (event) => {
      if (event.key === "Escape") {
        closeQuickPanel();
      }
    };
    const resize = () => {
      positionQuickPanel(anchor || currentQuickPanelAnchor());
    };
    const scroll = () => {
      const liveAnchor = anchor || currentQuickPanelAnchor();
      if (!liveAnchor) {
        closeQuickPanel();
        return;
      }
      positionQuickPanel(liveAnchor);
    };
    const mutation = new MutationObserver(() => {
      if (anchor instanceof HTMLElement && !document.body.contains(anchor)) {
        closeQuickPanel();
      }
    });
    document.addEventListener("click", click, true);
    document.addEventListener("keydown", keydown, true);
    window.addEventListener("resize", resize);
    window.addEventListener("scroll", scroll, true);
    mutation.observe(document.body, { childList: true, subtree: true });
    state.quickPanelCloseHandlers = { click, keydown, resize, scroll, mutation };
  }

  function openInlinePanelForMint(routeOrAddress, surface, url = window.location.href, anchor = null, options = {}) {
    const tokenContext = setInlineTokenContext(routeOrAddress, surface, url, options);
    if (!isBootstrapLoaded()) {
      openQuickPanelSurface(tokenContext, anchor, options);
      void refreshBootstrap(true).then((bootstrap) => {
        if (!bootstrap) {
          return;
        }
        if (!ensureValidExecutionPresetSync({ showFailureToast: false })) {
          closeQuickPanel();
          return;
        }
        startQuickPanelDataRefresh(tokenContext);
      });
      return;
    }
    if (!ensureValidExecutionPresetSync()) {
      return;
    }
    openQuickPanelSurface(tokenContext, anchor, options);
    startQuickPanelDataRefresh(tokenContext);
  }

  function openQuickPanelSurface(tokenContext, anchor = null, options = {}) {
    ensureQuickPanelFrame();
    setQuickPanelTokenContext(tokenContext);
    clearQuickPanelLifecycleCleanup();
    if (typeof options.onOpen === "function") {
      try {
        const cleanup = options.onOpen();
        if (typeof cleanup === "function") {
          state.quickPanelLifecycleCleanup = cleanup;
        }
      } catch (error) {
        console.warn("Trench Tools quick panel open hook failed", error);
      }
    }
    state.quickPanelWrapper.style.display = "block";
    state.quickPanelOpen = true;
    positionQuickPanel(anchor || currentQuickPanelAnchor() || state.quickPanelWrapper);
    applyPanelLayering();
    registerQuickPanelCloseHandlers(anchor);
  }

  function startQuickPanelDataRefresh(tokenContext) {
    void refreshPanelTokenContext({ silent: true, mode: "quick" })
      .then((resolvedTokenContext) => {
        return refreshPanelWalletStatus({ tokenContext: resolvedTokenContext || tokenContext, force: true });
      });
    schedulePrewarmForTokenContext(tokenContext, "inline-panel-open");
    syncActiveMintSubscription();
  }

  function openPanel(tokenContext, options = {}) {
    if (options.requireValidPreset) {
      if (!isBootstrapLoaded()) {
        openPersistentPanelSurface(tokenContext);
        void refreshBootstrap(true).then((bootstrap) => {
          if (!bootstrap) {
            return;
          }
          if (!ensureValidExecutionPresetSync({ showFailureToast: false })) {
            closePanel();
            return;
          }
          startPersistentPanelDataRefresh(tokenContext);
        });
        return;
      }
      if (!ensureValidExecutionPresetSync()) {
        return;
      }
    }
    openPersistentPanelSurface(tokenContext);
    startPersistentPanelDataRefresh(tokenContext);
  }

  function openPersistentPanelSurface(tokenContext) {
    ensurePanelFrame();
    closeQuickPanel();
    setPanelTokenContext(tokenContext);
    setPanelHidden(false);
    applyPanelLayering();
  }

  function startPersistentPanelDataRefresh(tokenContext) {
    void refreshPanelTokenContext({ silent: true, mode: "persistent" })
      .then((resolvedTokenContext) => {
        return refreshPanelWalletStatus({ tokenContext: resolvedTokenContext || tokenContext, force: true });
      });
    schedulePrewarmForTokenContext(tokenContext, "panel-open");
    syncActiveMintSubscription();
  }

  function closePanel() {
    setPanelHidden(true);
  }

  function panelWindows() {
    const windows = [];
    if (state.panelReady && state.panelFrame?.contentWindow) {
      windows.push({
        windowRef: state.panelFrame.contentWindow,
        tokenContext: state.panelTokenContext || null
      });
    }
    if (state.quickPanelReady && state.quickPanelFrame?.contentWindow) {
      windows.push({
        windowRef: state.quickPanelFrame.contentWindow,
        tokenContext: state.quickPanelTokenContext || null
      });
    }
    return windows;
  }

  function pushPanelState() {
    const windows = panelWindows();
    if (!windows.length) {
      return;
    }
    windows.forEach(({ windowRef, tokenContext }) => {
      windowRef.postMessage(
        {
          channel: PANEL_CHANNEL_OUT,
          type: "panel-state",
          payload: {
            bootstrap: state.bootstrap,
            walletStatus: state.walletStatus,
            tokenContext: tokenContext || null,
            preferences: state.preferences,
            preview: state.preview,
            batchStatus: state.batchStatus,
            tokenDistributionPending: state.tokenDistributionPending,
            hostError: state.hostError,
            runtimeDiagnosticNotice: state.runtimeDiagnosticNotice
          }
        },
        PANEL_ORIGIN
      );
    });
  }

  function notifyPlatformWalletStatusChange() {
    try {
      getPlatformAdapter()?.handleWalletStatusChange?.();
    } catch (error) {
      if (isExtensionReloadedError(error)) {
        scheduleExtensionReloadFallbackToast();
        return;
      }
      console.error("Trench Tools wallet-status platform refresh failed", error);
    }
  }

  function pushPanelPreview() {
    const windows = panelWindows();
    if (!windows.length) {
      return;
    }
    const message = {
      channel: PANEL_CHANNEL_OUT,
      type: "panel-preview",
      payload: state.preview
    };
    windows.forEach(({ windowRef }) => windowRef.postMessage(message, PANEL_ORIGIN));
  }

  function pushPanelBatchStatus() {
    const windows = panelWindows();
    if (!windows.length) {
      return;
    }
    const message = {
      channel: PANEL_CHANNEL_OUT,
      type: "panel-batch-status",
      payload: state.batchStatus
    };
    windows.forEach(({ windowRef }) => windowRef.postMessage(message, PANEL_ORIGIN));
  }

  function pushPanelError(message, options = {}) {
    const windows = panelWindows();
    if (!windows.length) {
      return;
    }
    const title = String(options.title || "").trim();
    const kind = String(options.kind || "error").trim() || "error";
    const source = String(options.source || "notice").trim() || "notice";
    const payload = {
      channel: PANEL_CHANNEL_OUT,
      type: "panel-error",
      payload: { message, title, kind, source }
    };
    windows.forEach(({ windowRef }) => windowRef.postMessage(payload, PANEL_ORIGIN));
  }

  function selectionPayloadFromPreferences() {
    const selection = normalizeWalletSelectionPreference(state.preferences);
    if (selection.selectionSource === "group") {
      return { walletGroupId: selectedWalletGroupIdFromValue(selection) };
    }
    const manualWalletKeys = Array.from(new Set((selection.manualWalletKeys || []).filter(Boolean)));
    if (manualWalletKeys.length <= 1) {
      return { walletKey: manualWalletKeys[0] || "" };
    }
    return { walletKeys: manualWalletKeys };
  }

  function getActivePreset() {
    const presets = state.bootstrap?.presets || [];
    return presets.find((preset) => preset.id === state.preferences.presetId) || presets[0] || null;
  }


  function extractMintFromUrl(url) {
    if (!url) {
      return "";
    }
    const matches = String(url).match(BASE58_REGEX);
    return matches ? matches[0] : "";
  }

  function extractMintFromText(text) {
    if (!text) {
      return "";
    }
    const matches = String(text).match(BASE58_REGEX);
    return matches ? matches[0] : "";
  }

  function extractMintFromSelectors(selectors) {
    for (const selector of selectors) {
      for (const element of document.querySelectorAll(selector)) {
        const mint =
          extractMintFromText(element.textContent) ||
          extractMintFromText(element.getAttribute("title")) ||
          extractMintFromText(element.getAttribute("data-address")) ||
          extractMintFromText(element.getAttribute("data-copy"));
        if (mint) {
          return mint;
        }
      }
    }
    return "";
  }

  function findElementShowingMint(mint) {
    const selectors = ["code", "a", "button", "span", "div", "p"];
    for (const selector of selectors) {
      for (const element of document.querySelectorAll(selector)) {
        const text = (element.textContent || "").trim();
        if (text === mint || text.includes(mint)) {
          return element;
        }
      }
    }
    return null;
  }

  function ensureToastHost() {
    let host = document.getElementById("trench-tools-toast-host");
    if (host) {
      return host;
    }

    host = document.createElement("div");
    host.id = "trench-tools-toast-host";
    Object.assign(host.style, {
      position: "fixed",
      top: "16px",
      left: "50%",
      transform: "translateX(-50%)",
      display: "flex",
      flexDirection: "column",
      alignItems: "center",
      gap: "8px",
      width: "calc(100vw - 24px)",
      maxWidth: "none",
      zIndex: "2147483647",
      pointerEvents: "none"
    });
    document.documentElement.appendChild(host);

    let styleTag = document.getElementById("trench-tools-toast-styles");
    if (!styleTag) {
      styleTag = document.createElement("style");
      styleTag.id = "trench-tools-toast-styles";
      styleTag.textContent = `
        @keyframes trench-tools-spin {
          from { transform: rotate(0deg); }
          to { transform: rotate(360deg); }
        }
      `;
      document.documentElement.appendChild(styleTag);
    }

    return host;
  }

  function ensureToastCleanupInterval() {
    if (state.toastCleanupInterval) {
      return;
    }
    state.toastCleanupInterval = window.setInterval(cleanupOrphanToasts, 5000);
  }

  function cleanupOrphanToasts() {
    for (const [toastId, entry] of state.activeToasts.entries()) {
      if (!entry?.element || !document.documentElement.contains(entry.element)) {
        window.clearTimeout(entry?.timeoutId);
        state.activeToasts.delete(toastId);
      }
    }
  }

  function dismissToast(toastId) {
    const entry = state.activeToasts.get(toastId);
    if (!entry) {
      return;
    }
    window.clearTimeout(entry.timeoutId);
    entry.element.style.opacity = "0";
    entry.element.style.transform = "translateY(-8px)";
    window.setTimeout(() => {
      entry.element.remove();
    }, 220);
    state.activeToasts.delete(toastId);
  }

  function resetToastProgress(entry) {
    entry.progressTrack.style.display = "none";
    entry.progress.style.transition = "none";
    entry.progress.style.transform = "translateX(-100%)";
    entry.frozenTransform = "";
  }

  function startToastProgress(entry, durationMs, reset = false) {
    if (!durationMs) {
      resetToastProgress(entry);
      return;
    }

    entry.progressTrack.style.display = "block";
    entry.progress.style.transition = "none";
    if (reset) {
      entry.progress.style.transform = "translateX(-100%)";
    } else if (entry.frozenTransform) {
      entry.progress.style.transform = entry.frozenTransform;
    }
    entry.progress.offsetWidth;
    entry.progress.style.transition = `transform ${durationMs}ms linear`;
    entry.progress.style.transform = "translateX(0)";
    entry.frozenTransform = "";
  }

  function runToastDismissTimer(toastId, durationMs, reset = false) {
    const entry = state.activeToasts.get(toastId);
    if (!entry) {
      return;
    }
    window.clearTimeout(entry.timeoutId);
    if (!durationMs) {
      entry.timeoutId = null;
      entry.remainingMs = 0;
      resetToastProgress(entry);
      return;
    }
    entry.remainingMs = durationMs;
    entry.dismissStartedAt = Date.now();
    startToastProgress(entry, durationMs, reset);
    entry.timeoutId = window.setTimeout(() => dismissToast(toastId), durationMs);
  }

  function pauseToastDismiss(toastId) {
    const entry = state.activeToasts.get(toastId);
    if (!entry || !entry.remainingMs || !entry.timeoutId) {
      return;
    }
    window.clearTimeout(entry.timeoutId);
    entry.timeoutId = null;
    entry.remainingMs = Math.max(0, entry.remainingMs - (Date.now() - entry.dismissStartedAt));
    const transform = window.getComputedStyle(entry.progress).transform;
    entry.progress.style.transition = "none";
    entry.progress.style.transform = transform;
    entry.frozenTransform = transform;
  }

  function resumeToastDismiss(toastId) {
    const entry = state.activeToasts.get(toastId);
    if (!entry || !entry.remainingMs || entry.timeoutId) {
      return;
    }
    runToastDismissTimer(toastId, entry.remainingMs, false);
  }

  function scheduleToastDismiss(toastId, ttlMs) {
    const entry = state.activeToasts.get(toastId);
    if (!entry) {
      return;
    }
    if (!ttlMs) {
      window.clearTimeout(entry.timeoutId);
      entry.timeoutId = null;
      entry.remainingMs = 0;
      resetToastProgress(entry);
      return;
    }
    runToastDismissTimer(toastId, ttlMs, true);
  }

  function getOrCreateToastEntry(toastId) {
    const existing = state.activeToasts.get(toastId);
    if (existing) {
      return existing;
    }

    const host = ensureToastHost();
    const toast = document.createElement("div");
    const header = document.createElement("div");
    const icon = document.createElement("div");
    const copy = document.createElement("div");
    const title = document.createElement("div");
    const detail = document.createElement("div");
    const action = document.createElement("a");
    const actionIcon = document.createElement("img");
    const progressTrack = document.createElement("div");
    const progress = document.createElement("div");

    Object.assign(toast.style, {
      width: "max-content",
      maxWidth: "min(92vw, 520px)",
      boxSizing: "border-box",
      borderRadius: "6px",
      padding: "12px",
      border: "1px solid rgba(39, 39, 42, 0.9)",
      background: "#18181B",
      boxShadow: "0 1px 3px rgba(0, 0, 0, 0.12)",
      fontFamily: "\"Inter\", \"Suisse Intl Medium\", ui-sans-serif, system-ui, sans-serif",
      pointerEvents: "auto",
      position: "relative",
      overflow: "hidden",
      opacity: "0",
      transform: "translateY(-8px)",
      transition: "transform 0.22s ease, opacity 0.22s ease"
    });
    Object.assign(header.style, {
      display: "flex",
      alignItems: "flex-start",
      gap: "8px"
    });
    Object.assign(icon.style, {
      flex: "0 0 auto",
      width: "16px",
      height: "16px",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      color: "#fafafa",
      marginTop: "1px"
    });
    Object.assign(copy.style, {
      minWidth: "0",
      flex: "1",
      display: "flex",
      flexDirection: "column",
      justifyContent: "center"
    });
    Object.assign(title.style, {
      color: "#fafafa",
      fontSize: "13px",
      fontWeight: "500",
      lineHeight: "1.3",
      letterSpacing: "0.01em",
      whiteSpace: "normal",
      overflow: "visible",
      textOverflow: "clip",
      overflowWrap: "anywhere"
    });
    Object.assign(detail.style, {
      marginTop: "2px",
      color: "#a1a1aa",
      fontSize: "12px",
      lineHeight: "1.35",
      wordBreak: "break-word",
      overflowWrap: "anywhere"
    });
    Object.assign(action.style, {
      display: "none",
      marginLeft: "8px",
      alignItems: "center",
      justifyContent: "center",
      color: "#a1a1aa",
      width: "18px",
      height: "18px",
      flex: "0 0 auto",
      textDecoration: "none",
      cursor: "pointer",
      opacity: "0.82",
      transition: "opacity 0.15s ease, transform 0.15s ease"
    });
    action.target = "_blank";
    action.rel = "noreferrer";
    action.setAttribute("aria-label", "View transaction");
    actionIcon.src = TOAST_LINK_ICON_URL;
    actionIcon.alt = "";
    actionIcon.setAttribute("aria-hidden", "true");
    Object.assign(actionIcon.style, {
      width: "14px",
      height: "14px",
      display: "block",
      objectFit: "contain",
      filter: "brightness(0) invert(1)",
      opacity: "0.85"
    });
    action.appendChild(actionIcon);
    Object.assign(progressTrack.style, {
      position: "absolute",
      left: "0",
      right: "0",
      bottom: "0",
      height: "2px",
      overflow: "hidden",
      display: "none"
    });
    Object.assign(progress.style, {
      width: "100%",
      height: "100%",
      opacity: "0.75",
      transform: "translateX(-100%)"
    });
    progressTrack.appendChild(progress);

    copy.append(title, detail);
    header.append(icon, copy, action);
    toast.append(header, progressTrack);
    host.prepend(toast);

    const entry = {
      element: toast,
      icon,
      title,
      detail,
      action,
      progressTrack,
      progress,
      actionHandler: null,
      clickHandler: null,
      timeoutId: null,
      remainingMs: 0,
      dismissStartedAt: 0,
      frozenTransform: ""
    };
    state.activeToasts.set(toastId, entry);
    toast.addEventListener("click", (event) => {
      if (event.target.tagName === "A" || event.target.closest("a")) {
        return;
      }
      if (typeof entry.clickHandler === "function") {
        entry.clickHandler();
      }
      dismissToast(toastId);
    });
    toast.addEventListener("mouseenter", () => pauseToastDismiss(toastId));
    toast.addEventListener("mouseleave", () => resumeToastDismiss(toastId));
    action.addEventListener("click", (event) => {
      if (typeof entry.actionHandler !== "function") {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      entry.actionHandler();
    });
    action.addEventListener("mouseenter", () => {
      action.style.opacity = "1";
      action.style.transform = "translateY(-1px)";
    });
    action.addEventListener("mouseleave", () => {
      action.style.opacity = "0.82";
      action.style.transform = "translateY(0)";
    });
    ensureToastCleanupInterval();
    requestAnimationFrame(() => {
      toast.style.opacity = "1";
      toast.style.transform = "translateY(0)";
    });
    return entry;
  }

  function applyToastTitle(entry, title, titleLink) {
    const titleText = typeof title === "string" ? title : "";
    entry.title.textContent = "";
    const link = titleLink && typeof titleLink === "object" ? titleLink : null;
    const match = link && typeof link.match === "string" ? link.match : "";
    const matchIndex = match ? titleText.indexOf(match) : -1;
    const hasInlineLink = matchIndex >= 0 && (typeof link.onClick === "function" || (typeof link.href === "string" && link.href));

    if (hasInlineLink) {
      Object.assign(entry.title.style, {
        whiteSpace: "normal",
        overflow: "visible",
        textOverflow: "clip",
        overflowWrap: "anywhere"
      });
      const before = titleText.slice(0, matchIndex);
      const after = titleText.slice(matchIndex + match.length);
      if (before) {
        entry.title.appendChild(document.createTextNode(before));
      }
      const anchor = document.createElement("a");
      anchor.textContent = match;
      if (typeof link.href === "string" && link.href) {
        anchor.href = link.href;
        anchor.target = "_blank";
        anchor.rel = "noreferrer";
      } else {
        anchor.href = "#";
      }
      Object.assign(anchor.style, {
        color: "#fafafa",
        textDecoration: "underline",
        textUnderlineOffset: "2px",
        cursor: "pointer",
        fontWeight: "600"
      });
      if (typeof link.onClick === "function") {
        anchor.addEventListener("click", (event) => {
          event.preventDefault();
          event.stopPropagation();
          link.onClick();
        });
      } else {
        anchor.addEventListener("click", (event) => {
          event.stopPropagation();
        });
      }
      entry.title.appendChild(anchor);
      if (after) {
        entry.title.appendChild(document.createTextNode(after));
      }
    } else {
      Object.assign(entry.title.style, {
        whiteSpace: "normal",
        overflow: "visible",
        textOverflow: "clip",
        overflowWrap: "anywhere"
      });
      entry.title.textContent = titleText;
    }
  }

  function renderToast({
    id,
    title,
    detail,
    kind = "info",
    linkHref = "",
    linkLabel = "View transaction",
    actionHandler = null,
    actionLabel = "",
    titleLink = null,
    clickHandler = null,
    ttlMs = 3200,
    persistent = false,
    pending = false
  }) {
    if (lifecycle.destroyed) {
      return;
    }
    const entry = getOrCreateToastEntry(id);
    const iconMarkup =
      pending
        ? `<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M18.364 5.636L16.95 7.05A7 7 0 1 0 19 12h2a9 9 0 1 1-2.636-6.364z"/></svg>`
        : kind === "success"
          ? `<img src="${TOAST_SUCCESS_ICON_URL}" alt="" aria-hidden="true" style="width:16px;height:16px;display:block;object-fit:contain;" />`
          : kind === "error"
            ? `<img src="${TOAST_FAIL_ICON_URL}" alt="" aria-hidden="true" style="width:16px;height:16px;display:block;object-fit:contain;" />`
            : `<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M12 22C6.477 22 2 17.523 2 12S6.477 2 12 2s10 4.477 10 10-4.477 10-10 10zm0-2a8 8 0 1 0 0-16 8 8 0 0 0 0 16zm-1-11h2v2h-2V7zm0 4h2v6h-2v-6z"/></svg>`;
    const kindStyles =
      kind === "error"
        ? {
            border: "rgba(255, 255, 255, 0.14)",
            background: "#080808",
            glow: "0 10px 24px rgba(0, 0, 0, 0.28)",
            icon: "#fafafa"
          }
        : kind === "success"
          ? {
              border: "rgba(255, 255, 255, 0.14)",
              background: "#080808",
              glow: "0 10px 24px rgba(0, 0, 0, 0.28)",
              icon: "#fafafa"
            }
          : {
              border: "rgba(255, 255, 255, 0.14)",
              background: "#080808",
              glow: "0 10px 24px rgba(0, 0, 0, 0.28)",
              icon: "#fafafa"
            };

    Object.assign(entry.element.style, {
      borderColor: kindStyles.border,
      background: kindStyles.background,
      boxShadow: kindStyles.glow
    });
    entry.icon.innerHTML = iconMarkup;
    Object.assign(entry.icon.style, {
      color: kindStyles.icon,
      animation: pending ? "trench-tools-spin 1s linear infinite" : "none"
    });
    applyToastTitle(entry, title, titleLink);
    entry.detail.textContent = detail || "";
    entry.detail.style.display = detail ? "block" : "none";
    entry.progress.style.background = "rgba(255, 255, 255, 0.92)";
    entry.actionHandler = !linkHref && typeof actionHandler === "function" ? actionHandler : null;
    entry.clickHandler = typeof clickHandler === "function" ? clickHandler : null;
    entry.element.style.cursor = entry.clickHandler ? "pointer" : "default";
    const resolvedActionLabel = actionLabel || linkLabel;
    if (linkHref || entry.actionHandler) {
      if (linkHref) {
        entry.action.href = linkHref;
        entry.action.target = "_blank";
        entry.action.rel = "noreferrer";
      } else {
        entry.action.href = "#";
        entry.action.removeAttribute("target");
        entry.action.removeAttribute("rel");
      }
      entry.action.setAttribute("aria-label", resolvedActionLabel);
      entry.action.title = resolvedActionLabel;
      entry.action.style.display = "inline-flex";
      entry.action.style.color = "#a1a1aa";
    } else {
      entry.actionHandler = null;
      entry.action.removeAttribute("href");
      entry.action.removeAttribute("target");
      entry.action.removeAttribute("rel");
      entry.action.removeAttribute("aria-label");
      entry.action.removeAttribute("title");
      entry.action.style.display = "none";
    }

    scheduleToastDismiss(id, persistent ? 0 : ttlMs);
  }

  function showToast(message, kind = "info") {
    if (lifecycle.destroyed) {
      return;
    }
    renderToast({
      id: `notice-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      title: message,
      kind,
      ttlMs: kind === "error" ? 4200 : 3200
    });
  }

  function runtimeDiagnosticKind(diagnostic) {
    return String(diagnostic?.severity || "").toLowerCase() === "critical" ? "error" : "info";
  }

  function runtimeDiagnosticDetail(diagnostic) {
    const parts = [];
    if (diagnostic?.envVar) parts.push(diagnostic.envVar);
    if (diagnostic?.host) parts.push(diagnostic.host);
    if (diagnostic?.restartRequired) parts.push("restart may be required");
    const detail = String(diagnostic?.detail || "").trim();
    if (detail) parts.push(detail);
    return parts.join(" | ");
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

  function surfaceRuntimeDiagnostics(snapshot = null) {
    const diagnostics = Array.isArray(snapshot?.diagnostics) ? snapshot.diagnostics : [];
    const dismissed = snapshot?.dismissed && typeof snapshot.dismissed === "object"
      ? snapshot.dismissed
      : {};
    const activeKeys = new Set(
      diagnostics
        .filter((diagnostic) => diagnostic && diagnostic.active !== false)
        .map(runtimeDiagnosticToastKey)
        .filter(Boolean)
    );
    for (const key of Array.from(state.runtimeDiagnosticToastKeys)) {
      if (!activeKeys.has(key)) state.runtimeDiagnosticToastKeys.delete(key);
    }
    const panelDiagnostic = diagnostics.find(
      (diagnostic) =>
        diagnostic &&
        diagnostic.active !== false &&
        !dismissed[diagnostic.fingerprint || diagnostic.key]
    );
    if (panelDiagnostic) {
      const detail = runtimeDiagnosticDetail(panelDiagnostic);
      state.runtimeDiagnosticNotice = {
        title: panelDiagnostic.message || "Runtime diagnostic",
        message: detail || panelDiagnostic.message || "Runtime diagnostic",
        kind: runtimeDiagnosticKind(panelDiagnostic),
        source: "runtime-diagnostic"
      };
      pushPanelError(state.runtimeDiagnosticNotice.message, {
        title: panelDiagnostic.message || "Runtime diagnostic",
        kind: runtimeDiagnosticKind(panelDiagnostic),
        source: "runtime-diagnostic"
      });
    } else {
      state.runtimeDiagnosticNotice = null;
      pushPanelError("", { source: "runtime-diagnostic" });
    }
    diagnostics
      .filter((diagnostic) => diagnostic && diagnostic.active !== false)
      .filter((diagnostic) => !dismissed[diagnostic.fingerprint || diagnostic.key])
      .slice(0, 2)
      .forEach((diagnostic) => {
        const fingerprint = String(diagnostic.fingerprint || diagnostic.key || "").trim();
        const toastKey = runtimeDiagnosticToastKey(diagnostic);
        if (!fingerprint) return;
        if (!toastKey) return;
        if (state.runtimeDiagnosticToastKeys.has(toastKey)) return;
        state.runtimeDiagnosticToastKeys.add(toastKey);
        renderToast({
          id: `runtime-diagnostic-${fingerprint}`,
          title: diagnostic.message || "Runtime diagnostic",
          detail: runtimeDiagnosticDetail(diagnostic),
          kind: runtimeDiagnosticKind(diagnostic),
          ttlMs: 6200,
          actionLabel: "Dismiss",
          actionHandler: () => {
            void callBackground("trench:dismiss-runtime-diagnostic", { fingerprint }).catch(() => {});
            dismissToast(`runtime-diagnostic-${fingerprint}`);
          }
        });
      });
  }

  function capitalize(value) {
    return String(value).charAt(0).toUpperCase() + String(value).slice(1);
  }

  function quickBuyLabel() {
    const quickBuyAmount = getValidQuickBuyAmount(state.preferences.quickBuyAmount);
    return quickBuyAmount || "";
  }

  function resolveQuickBuyAmount() {
    const quickBuyAmount = getValidQuickBuyAmount(state.preferences.quickBuyAmount);
    return quickBuyAmount || "";
  }

  function getQuickBuyBaseStylesForPlatform(platformId = platform) {
    if (platformId === "j7") {
      return {
        base: {
          border: "1px solid rgba(255, 255, 255, 0.2)",
          borderRadius: "4px",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          backgroundColor: "#000000",
          color: "#ffffff",
          zIndex: "1000",
          cursor: "pointer",
          transition: "background-color 0.2s ease",
          height: "21px",
          padding: "0px 4px",
          fontSize: "12px",
          fontWeight: "500",
          lineHeight: "1"
        },
        hover: {
          backgroundColor: "#18181b"
        },
        logoSize: "12px",
        logoGap: "4px"
      };
    }

    return {
      base: {
        border: "1px solid rgba(255, 255, 255, 0.2)",
        borderRadius: "12px",
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        backgroundColor: "#000000",
        color: "#ffffff",
        zIndex: "1000",
        cursor: "pointer",
        transition: "background-color 0.2s ease",
        marginLeft: "6px",
        padding: "0px 8px",
        height: "24px",
        fontSize: "12px",
        fontWeight: "600"
      },
      hover: {
        backgroundColor: "#18181b"
      },
      logoSize: "14px",
      logoGap: "4px"
    };
  }

  function quickBuyStyles() {
    return getPlatformAdapter()?.getQuickBuyStyles?.() || getQuickBuyBaseStylesForPlatform(platform);
  }

  function shouldAutoOpenPanel(tokenContext) {
    if (!tokenContext) {
      return false;
    }

    return Boolean(getPlatformAdapter()?.shouldAutoOpenPanel(tokenContext));
  }

  function maybeAutoOpenPanel(tokenContext) {
    if (!tokenContext || !shouldAutoOpenPanel(tokenContext) || state.panelOpen) {
      return;
    }
    openPanel(tokenContext, { requireValidPreset: true });
  }

  async function syncVisibilityFromStorage(tokenContext = state.tokenContext) {
    const key = hiddenStateStorageKey();
    const stored = await safeStorageGet(key);
    const storedHidden = stored[key];
    const legacyAutoOpen = Boolean(state.siteFeatures?.axiom?.autoOpenPanel);
    const hidden = shouldAutoOpenPanel(tokenContext)
      ? storedHidden ?? !legacyAutoOpen
      : true;
    await setPanelHidden(hidden, false);
    return hidden;
  }

  async function setPanelHidden(hidden, persist = true) {
    if (!hidden) {
      ensurePanelFrame();
      applyPanelPosition();
    } else {
      hidePanelFlyoutOverlay();
    }
    if (state.panelWrapper) {
      state.panelWrapper.style.display = hidden ? "none" : "block";
    }
    applyPanelLayering();
    if (state.launcherButton) {
      state.launcherButton.style.display = hidden && shouldMountLauncher(state.tokenContext) ? "flex" : "none";
    }
    state.panelOpen = !hidden;
    syncActiveMintSubscription();
    syncWalletStatusQuoteRefresh();
    if (persist) {
      await safeStorageSet({ [hiddenStateStorageKey()]: hidden });
    }
  }

  const PANEL_BASE_WIDTH = 375;
  // Fallback used until the panel posts its first measured natural height.
  // Once the iframe reports a real `panel-resize`, state.panelNaturalHeight
  // takes over so the persistent shell shrinks/grows with the content.
  const PANEL_BASE_HEIGHT_FALLBACK = 430;
  const PANEL_SCALE_MIN = 0.7;
  const PANEL_SCALE_MAX = 1.6;

  function getPanelNaturalHeight() {
    const reported = Number(state.panelNaturalHeight);
    if (Number.isFinite(reported) && reported > 0) {
      return reported;
    }
    return PANEL_BASE_HEIGHT_FALLBACK;
  }

  function writePanelShellStyles(scale) {
    if (!state.panelWrapper) {
      return;
    }
    const clamped = clamp(Number(scale) || 1, PANEL_SCALE_MIN, PANEL_SCALE_MAX);
    const naturalHeight = getPanelNaturalHeight();
    const scaledWidth = Math.round(PANEL_BASE_WIDTH * clamped);
    const scaledHeight = Math.round(naturalHeight * clamped);
    state.panelWrapper.style.width = `${scaledWidth}px`;
    state.panelWrapper.style.height = `${scaledHeight}px`;
    state.panelWrapper.style.minHeight = "";
    state.panelWrapper.style.transform = "";
    state.panelWrapper.style.transformOrigin = "";
    state.panelWrapper.style.willChange = "";
    state.panelWrapper.style.backfaceVisibility = "";
    state.panelWrapper.style.WebkitBackfaceVisibility = "";
    state.panelWrapper.dataset.panelScale = String(clamped);
    if (state.panelFrame) {
      state.panelFrame.style.zoom = clamped === 1 ? "" : String(clamped);
    }
  }

  function applyPanelShellMetrics() {
    if (!state.panelWrapper) {
      return;
    }
    const scale = clamp(Number(state.panelScale) || 1, PANEL_SCALE_MIN, PANEL_SCALE_MAX);
    state.panelScale = scale;
    state.panelDimensions = { width: PANEL_BASE_WIDTH, height: getPanelNaturalHeight() };
    writePanelShellStyles(scale);
  }

  function applyPanelPosition() {
    if (!state.panelWrapper) {
      return;
    }

    applyPanelShellMetrics();
    const scale = state.panelScale;
    const naturalHeight = getPanelNaturalHeight();
    const scaledWidth = Math.round(PANEL_BASE_WIDTH * scale);
    const scaledHeight = Math.round(naturalHeight * scale);
    const maxLeft = Math.max(0, window.innerWidth - scaledWidth);
    const maxTop = Math.max(0, window.innerHeight - scaledHeight);

    if (!state.panelPosition) {
      const centerLeft = Math.round((window.innerWidth - scaledWidth) / 2);
      const centerTop = Math.round((window.innerHeight - scaledHeight) / 2);
      state.panelWrapper.style.left = `${centerLeft}px`;
      state.panelWrapper.style.top = `${centerTop}px`;
      return;
    }

    const clampedLeft = Math.round(clamp(Number(state.panelPosition.left) || 0, 0, maxLeft));
    const clampedTop = Math.round(clamp(Number(state.panelPosition.top) || 0, 0, maxTop));
    state.panelPosition = { left: clampedLeft, top: clampedTop };
    state.panelWrapper.style.left = `${clampedLeft}px`;
    state.panelWrapper.style.top = `${clampedTop}px`;
  }

  function attachPanelResizeHandle(wrapper) {
    const handle = document.createElement("div");
    handle.id = "trench-tools-panel-resize-handle";
    Object.assign(handle.style, {
      position: "absolute",
      right: "0",
      bottom: "0",
      width: "18px",
      height: "18px",
      cursor: "nwse-resize",
      zIndex: "100",
      background: "transparent",
      pointerEvents: "auto",
      touchAction: "none"
    });
    const glyph = document.createElement("div");
    Object.assign(glyph.style, {
      position: "absolute",
      right: "4px",
      bottom: "4px",
      width: "10px",
      height: "10px",
      opacity: "0.7",
      pointerEvents: "none",
      // Diagonal grip lines drawn in black so they stay visible when the
      // handle sits over the bright green buy-shortcut in the bottom-right
      // corner. The handle is otherwise transparent, so the panel's dark
      // background still shows the lines fine when no button is underneath.
      backgroundImage: "linear-gradient(135deg, transparent 55%, rgba(0,0,0,0.75) 55%, rgba(0,0,0,0.75) 62%, transparent 62%, transparent 72%, rgba(0,0,0,0.75) 72%, rgba(0,0,0,0.75) 79%, transparent 79%, transparent 89%, rgba(0,0,0,0.75) 89%, rgba(0,0,0,0.75) 96%, transparent 96%)"
    });
    handle.appendChild(glyph);
    wrapper.appendChild(handle);

    let activePointerId = null;
    let startX = 0;
    let startY = 0;
    let startScale = 1;
    let baseVisualWidth = PANEL_BASE_WIDTH;
    let baseVisualHeight = PANEL_BASE_HEIGHT_FALLBACK;

    const writeScale = (nextScale) => {
      state.panelScale = nextScale;
      if (!state.panelWrapper) {
        return;
      }
      const naturalHeight = getPanelNaturalHeight();
      const scaledWidth = Math.round(PANEL_BASE_WIDTH * nextScale);
      const scaledHeight = Math.round(naturalHeight * nextScale);
      state.panelWrapper.style.width = `${scaledWidth}px`;
      state.panelWrapper.style.height = `${scaledHeight}px`;
      if (state.panelFrame) {
        state.panelFrame.style.zoom = nextScale === 1 ? "" : String(nextScale);
      }
      if (state.panelPosition) {
        return;
      }
      const centerLeft = Math.round((window.innerWidth - scaledWidth) / 2);
      const centerTop = Math.round((window.innerHeight - scaledHeight) / 2);
      state.panelWrapper.style.left = `${centerLeft}px`;
      state.panelWrapper.style.top = `${centerTop}px`;
    };

    const onMove = (event) => {
      if (activePointerId == null || event.pointerId !== activePointerId) {
        return;
      }
      event.preventDefault();
      const dx = event.clientX - startX;
      const dy = event.clientY - startY;
      const naturalHeight = getPanelNaturalHeight();
      const scaleFromX = (baseVisualWidth + dx) / PANEL_BASE_WIDTH;
      const scaleFromY = (baseVisualHeight + dy) / naturalHeight;
      const nextScale = clamp(Math.max(scaleFromX, scaleFromY), PANEL_SCALE_MIN, PANEL_SCALE_MAX);
      writeScale(nextScale);
    };

    const finishResize = async (event) => {
      if (activePointerId == null) {
        return;
      }
      if (event && event.pointerId !== activePointerId) {
        return;
      }
      const pointerId = activePointerId;
      activePointerId = null;
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", finishResize);
      handle.removeEventListener("pointercancel", finishResize);
      try {
        handle.releasePointerCapture(pointerId);
      } catch (err) {
        // pointer capture may already be released
      }
      applyPanelPosition();
      await safeStorageSet({ [panelScaleStorageKey()]: state.panelScale });
    };

    handle.addEventListener("pointerdown", (event) => {
      if (event.button !== 0) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      activePointerId = event.pointerId;
      startX = event.clientX;
      startY = event.clientY;
      startScale = clamp(Number(state.panelScale) || 1, PANEL_SCALE_MIN, PANEL_SCALE_MAX);
      baseVisualWidth = PANEL_BASE_WIDTH * startScale;
      baseVisualHeight = getPanelNaturalHeight() * startScale;
      try {
        handle.setPointerCapture(event.pointerId);
      } catch (err) {
        // setPointerCapture can throw on exotic input devices; fall back to window listeners
      }
      handle.addEventListener("pointermove", onMove);
      handle.addEventListener("pointerup", finishResize);
      handle.addEventListener("pointercancel", finishResize);
    });
  }

  function pagePointFromIframeClient(clientX, clientY) {
    const iframe = state.panelFrame;
    if (!iframe || typeof clientX !== "number" || typeof clientY !== "number") {
      return { x: clientX, y: clientY };
    }
    const bounds = iframe.getBoundingClientRect();
    return { x: bounds.left + clientX, y: bounds.top + clientY };
  }

  function beginPanelDrag(payload) {
    if (!state.panelWrapper || !state.panelFrame || state.activeDrag?.dragging) {
      return;
    }
    if (!payload || typeof payload.clientX !== "number" || typeof payload.clientY !== "number") {
      return;
    }

    const { x: pageX, y: pageY } = pagePointFromIframeClient(payload.clientX, payload.clientY);

    const rect = state.panelWrapper.getBoundingClientRect();
    state.panelPosition = { left: rect.left, top: rect.top };
    applyPanelPosition();

    const placed = state.panelWrapper.getBoundingClientRect();
    const offsetX = pageX - placed.left;
    const offsetY = pageY - placed.top;

    const onOverlayMove = (event) => {
      if (!state.activeDrag?.dragging) {
        return;
      }
      state.activeDrag.pointerX = event.clientX;
      state.activeDrag.pointerY = event.clientY;
    };

    const onOverlayMouseUp = () => {
      void endPanelDrag();
    };

    const onDocumentMouseUpCapture = () => {
      void endPanelDrag();
    };

    const onDocumentKeydownCapture = (event) => {
      if (event.key === "Escape") {
        void endPanelDrag();
      }
    };

    const overlay = document.createElement("div");
    overlay.id = "trench-tools-drag-overlay";
    Object.assign(overlay.style, {
      position: "fixed",
      top: "0",
      left: "0",
      width: "100vw",
      height: "100vh",
      zIndex: "2147483647",
      cursor: "move",
      background: "transparent",
      pointerEvents: "auto"
    });
    overlay.addEventListener("mousemove", onOverlayMove);
    overlay.addEventListener("mouseup", onOverlayMouseUp, true);
    document.documentElement.appendChild(overlay);

    state.panelWrapper.style.userSelect = "none";
    state.panelWrapper.style.cursor = "grabbing";
    document.body.style.userSelect = "none";

    document.addEventListener("mouseup", onDocumentMouseUpCapture, true);
    document.addEventListener("keydown", onDocumentKeydownCapture, true);

    const drag = {
      dragging: true,
      offsetX,
      offsetY,
      width: placed.width,
      height: placed.height,
      pointerX: pageX,
      pointerY: pageY,
      rafId: 0,
      overlay,
      onOverlayMove,
      onOverlayMouseUp,
      onDocumentMouseUpCapture,
      onDocumentKeydownCapture
    };
    state.activeDrag = drag;

    const tick = () => {
      const d = state.activeDrag;
      if (!d?.dragging) {
        return;
      }
      let left = d.pointerX - d.offsetX;
      let top = d.pointerY - d.offsetY;
      const maxLeft = Math.max(0, window.innerWidth - d.width);
      const maxTop = Math.max(0, window.innerHeight - d.height);
      left = clamp(left, 0, maxLeft);
      top = clamp(top, 0, maxTop);
      state.panelPosition = { left, top };
      applyPanelPosition();
      d.rafId = window.requestAnimationFrame(tick);
    };

    drag.rafId = window.requestAnimationFrame(tick);
    state.panelFrame.contentWindow?.focus();
  }

  async function endPanelDrag() {
    const drag = state.activeDrag;
    if (!drag?.dragging) {
      return;
    }
    drag.dragging = false;

    if (drag.rafId) {
      window.cancelAnimationFrame(drag.rafId);
      drag.rafId = 0;
    }

    if (drag.overlay) {
      drag.overlay.removeEventListener("mousemove", drag.onOverlayMove);
      drag.overlay.removeEventListener("mouseup", drag.onOverlayMouseUp, true);
      drag.overlay.remove();
    }

    document.removeEventListener("mouseup", drag.onDocumentMouseUpCapture, true);
    document.removeEventListener("keydown", drag.onDocumentKeydownCapture, true);

    if (state.panelWrapper) {
      state.panelWrapper.style.userSelect = "";
      state.panelWrapper.style.cursor = "";
    }
    document.body.style.userSelect = "";

    state.activeDrag = null;
    await safeStorageSet({ [panelPositionStorageKey()]: state.panelPosition });
  }

  function clamp(value, min, max) {
    return Math.min(max, Math.max(min, value));
  }
})();
