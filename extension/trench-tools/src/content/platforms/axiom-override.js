(function installTrenchToolsAxiomOverride() {
  const OVERRIDE_VERSION = "pulse-row-cache-v9";
  const previousOverrideVersion = window.__trenchToolsAxiomOverrideVersion;
  if (previousOverrideVersion === OVERRIDE_VERSION) {
    return;
  }
  const previousCleanup = window.__trenchToolsAxiomOverrideCleanup;
  const previousCleanupRestoresGlobals =
    previousCleanup?.__trenchToolsRestoresGlobals === true;
  if (previousOverrideVersion && typeof previousCleanup !== "function") {
    try {
      const reloadKey = `trenchToolsAxiomOverrideReloaded:${OVERRIDE_VERSION}`;
      if (window.sessionStorage?.getItem(reloadKey) !== "1") {
        window.sessionStorage?.setItem(reloadKey, "1");
        window.location.reload();
        return;
      }
    } catch (_error) {}
  }
  if (typeof previousCleanup === "function") {
    try {
      previousCleanup();
    } catch (_error) {}
    if (!previousCleanupRestoresGlobals) {
      try {
        const reloadKey = `trenchToolsAxiomOverrideReloaded:${OVERRIDE_VERSION}`;
        if (window.sessionStorage?.getItem(reloadKey) !== "1") {
          window.sessionStorage?.setItem(reloadKey, "1");
          window.location.reload();
          return;
        }
      } catch (_error) {}
    }
  }
  window.__trenchToolsAxiomOverrideInstalled = true;
  window.__trenchToolsAxiomOverrideVersion = OVERRIDE_VERSION;

  const PreviousMutationObserver =
    window.MutationObserver?.__trenchToolsWrappedMutationObserver || window.MutationObserver;
  window.MutationObserver = class TrenchToolsMutationObserver extends PreviousMutationObserver {
    constructor(callback, options) {
      const source = String(callback || "").toLowerCase();
      if (
        source.includes("visibility") ||
        source.includes("decoy") ||
        source.includes("width") ||
        source.includes("eea7ed")
      ) {
        callback = () => {};
      }
      super(callback, options);
    }
  };
  window.MutationObserver.__trenchToolsPulseMutationObserverVersion = OVERRIDE_VERSION;
  window.MutationObserver.__trenchToolsWrappedMutationObserver = PreviousMutationObserver;

  const PreviousWebSocket = window.WebSocket?.__trenchToolsWrappedWebSocket || window.WebSocket;
  const originalAddEventListener = PreviousWebSocket.prototype.addEventListener;
  let pulseUpdateTimeout = 0;
  let pulseBackfillTimer = 0;
  let pulseBridgeTimer = 0;
  let pulseRowSeedTimer = 0;
  let pendingPulseData = [];
  let lastPulseMetadataCaptureAt = 0;
  let lastPulseRowSignature = "";
  let stablePulseRowScans = 0;
  let visibilityChangeHandler = null;
  const backfilledPairMetadataUrls = new Set();
  const seededPulseRows = new WeakMap();
  let cachedPulseData = null;
  let cacheTimestamp = 0;
  let lastObservedPath = window.location.pathname || "";
  const DEBOUNCE_DELAY_MS = 50;
  const EXPIRY_TIME_MS = 5 * 60 * 1000;
  const CACHE_DURATION_MS = 1000;
  const PULSE_CARD_SELECTOR = [
    "div[style*='position: absolute'][style*='width: 100%']",
    "div.cursor-pointer[data-search]",
    "div[class*='group/pulseRow']"
  ].join(", ");
  const PULSE_ROW_SEED_INTERVAL_MS = 750;
  const PULSE_ROW_SEED_BACKOFF_INTERVAL_MS = 5000;
  const PULSE_IDLE_RECHECK_INTERVAL_MS = 10000;
  const PULSE_HIDDEN_RECHECK_INTERVAL_MS = 15000;
  const PULSE_BACKFILL_INTERVAL_MS = 1000;
  const PULSE_BACKFILL_BACKOFF_INTERVAL_MS = 5000;
  const PULSE_BRIDGE_RECOVERY_INTERVAL_MS = 5000;
  const PULSE_RESCAN_EVENT = "trench-tools:axiom-pulse-rescan";
  const TOKEN_DETAIL_NATIVE_HOVER_EVENT = "trench-tools:axiom-token-detail-native-hover";
  const PULSE_ROW_SEED_MAX_CARDS = 40;
  const PULSE_ROW_SEED_MAX_NODES = 180;
  const PULSE_ROW_SEED_MAX_KEYS = 80;
  const PULSE_ROW_SEED_MAX_DEPTH = 8;
  const BASE58_ADDRESS_RE = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/;
  const debugState = {
    version: OVERRIDE_VERSION,
    rowSeedRuns: 0,
    rowSeedLastDelayMs: 0,
    backfillRuns: 0,
    backfillLastDelayMs: 0,
    bridgeChecks: 0,
    bridgeLastDelayMs: 0,
    socketAttachCount: 0,
    socketUpdateCount: 0,
    socketLastUpdateAt: 0,
    socketLastMergeCount: 0,
    socketJoinCount: 0
  };
  window.__trenchToolsAxiomPulseMetadataDebug = debugState;

  function isPulseSurface() {
    return /\/pulse\b/i.test(window.location.pathname || "");
  }

  function shouldPausePulseWork() {
    return document.hidden || !isPulseSurface();
  }

  function clearScheduledWork() {
    window.clearTimeout(pulseUpdateTimeout);
    window.clearTimeout(pulseBackfillTimer);
    window.clearTimeout(pulseBridgeTimer);
    window.clearTimeout(pulseRowSeedTimer);
    pulseUpdateTimeout = 0;
    pulseBackfillTimer = 0;
    pulseBridgeTimer = 0;
    pulseRowSeedTimer = 0;
    if (visibilityChangeHandler) {
      document.removeEventListener("visibilitychange", visibilityChangeHandler);
      visibilityChangeHandler = null;
    }
    document.removeEventListener(PULSE_RESCAN_EVENT, handlePulseRescanRequest);
    document.removeEventListener(TOKEN_DETAIL_NATIVE_HOVER_EVENT, handleTokenDetailNativeHoverRequest, true);
    window.removeEventListener("popstate", handlePathChange);
    if (window.history?.pushState?.__trenchToolsWrappedHistoryMethod) {
      window.history.pushState = window.history.pushState.__trenchToolsWrappedHistoryMethod;
    }
    if (window.history?.replaceState?.__trenchToolsWrappedHistoryMethod) {
      window.history.replaceState = window.history.replaceState.__trenchToolsWrappedHistoryMethod;
    }
    if (window.MutationObserver?.__trenchToolsPulseMutationObserverVersion === OVERRIDE_VERSION) {
      window.MutationObserver = window.MutationObserver.__trenchToolsWrappedMutationObserver;
    }
    if (window.fetch?.__trenchToolsPulseMetadataFetchVersion === OVERRIDE_VERSION) {
      window.fetch = window.fetch.__trenchToolsWrappedFetch;
    }
    if (window.WebSocket?.__trenchToolsPulseWebSocketVersion === OVERRIDE_VERSION) {
      window.WebSocket = window.WebSocket.__trenchToolsWrappedWebSocket;
    }
  }

  clearScheduledWork.__trenchToolsRestoresGlobals = true;
  window.__trenchToolsAxiomOverrideCleanup = clearScheduledWork;
  document.addEventListener(TOKEN_DETAIL_NATIVE_HOVER_EVENT, handleTokenDetailNativeHoverRequest, true);

  function getCachedPulseData() {
    const now = Date.now();
    if (!cachedPulseData || now - cacheTimestamp > CACHE_DURATION_MS) {
      const raw = localStorage.getItem("axiom.pulse");
      cachedPulseData = raw ? JSON.parse(raw) : { content: [] };
      cacheTimestamp = now;
    }
    return cachedPulseData;
  }

  function handleTokenDetailNativeHoverRequest(event) {
    const action = event?.detail?.action;
    if (action !== "enter" && action !== "leave") {
      return;
    }
    const nativeControl = event.target;
    if (!(nativeControl instanceof HTMLElement)) {
      return;
    }
    const handlerName = action === "leave" ? "onMouseLeave" : "onMouseEnter";
    if (invokeTokenDetailNativeReactHoverHandler(nativeControl, handlerName, event)) {
      event.preventDefault();
      return;
    }
    if (dispatchTokenDetailNativeHoverFallback(nativeControl, action)) {
      event.preventDefault();
    }
  }

  function invokeTokenDetailNativeReactHoverHandler(nativeControl, handlerName, sourceEvent) {
    const reactProps = tokenDetailNativeReactProps(nativeControl);
    const handler = reactProps?.[handlerName];
    if (typeof handler !== "function") {
      return false;
    }
    try {
      handler({
        currentTarget: nativeControl,
        target: nativeControl,
        relatedTarget: null,
        type: handlerName === "onMouseLeave" ? "mouseleave" : "mouseenter",
        nativeEvent: sourceEvent || null,
        preventDefault() {},
        stopPropagation() {},
        isDefaultPrevented: () => false,
        isPropagationStopped: () => false,
        persist() {}
      });
      return true;
    } catch (_error) {
      return false;
    }
  }

  function tokenDetailNativeReactProps(nativeControl) {
    const propsKey = Object.keys(nativeControl).find((key) => key.startsWith("__reactProps$"));
    return propsKey ? nativeControl[propsKey] : null;
  }

  function dispatchTokenDetailNativeHoverFallback(nativeControl, action) {
    const eventTypes = action === "leave"
      ? ["pointerout", "pointerleave", "mouseout", "mouseleave"]
      : ["pointerover", "pointerenter", "mouseover", "mouseenter"];
    const rect = nativeControl.getBoundingClientRect();
    const clientX = rect.width > 0 ? rect.left + rect.width / 2 : 0;
    const clientY = rect.height > 0 ? rect.top + rect.height / 2 : 0;
    try {
      eventTypes.forEach((eventType) => {
        const pointerLike = eventType.startsWith("pointer");
        const init = {
          bubbles: true,
          cancelable: true,
          composed: true,
          view: window,
          clientX,
          clientY,
          screenX: window.screenX + clientX,
          screenY: window.screenY + clientY,
          button: 0,
          buttons: 0
        };
        const hoverEvent = pointerLike && typeof PointerEvent === "function"
          ? new PointerEvent(eventType, {
            ...init,
            pointerId: 1,
            pointerType: "mouse",
            isPrimary: true,
            width: 1,
            height: 1,
            pressure: 0
          })
          : new MouseEvent(eventType, init);
        nativeControl.dispatchEvent(hoverEvent);
      });
      return true;
    } catch (_error) {
      return false;
    }
  }

  function normalizePulseEntry(entry, currentTimestamp) {
    if (Array.isArray(entry)) {
      const pairAddress = String(entry[0] || "").trim();
      const tokenAddress = String(entry[1] || "").trim();
      if (!BASE58_ADDRESS_RE.test(pairAddress) || !BASE58_ADDRESS_RE.test(tokenAddress)) {
        return null;
      }
      return {
        pairAddress,
        tokenAddress,
        lastSeen: currentTimestamp
      };
    }
    if (!entry || typeof entry !== "object") {
      return null;
    }
    const pairAddress = String(entry.pairAddress || "").trim();
    const tokenAddress = String(entry.tokenAddress || entry.mint || "").trim();
    if (!BASE58_ADDRESS_RE.test(pairAddress) || !BASE58_ADDRESS_RE.test(tokenAddress)) {
      return null;
    }
    return {
      pairAddress,
      tokenAddress,
      lastSeen: currentTimestamp
    };
  }

  function mergePulseEntries(nextEntries) {
    if (!Array.isArray(nextEntries) || nextEntries.length === 0) {
      return 0;
    }
    const previousPulse = getCachedPulseData();
    const cutoffTime = Date.now() - EXPIRY_TIME_MS;
    const currentTimestamp = Date.now();
    const newTokenMap = new Map();

    nextEntries.forEach((entry) => {
      const normalizedEntry = normalizePulseEntry(entry, currentTimestamp);
      if (!normalizedEntry) {
        return;
      }
      newTokenMap.set(normalizedEntry.tokenAddress, normalizedEntry);
    });

    if (newTokenMap.size === 0) {
      return 0;
    }

    const filteredPrevious = (previousPulse.content || []).filter((token) => {
      const tokenTime =
        typeof token.lastSeen === "number" ? token.lastSeen : new Date(token.lastSeen).getTime();
      return tokenTime > cutoffTime && !newTokenMap.has(token.tokenAddress);
    });

    const nextPulseState = {
      content: [...filteredPrevious, ...newTokenMap.values()]
    };
    localStorage.setItem("axiom.pulse", JSON.stringify(nextPulseState));
    cachedPulseData = nextPulseState;
    cacheTimestamp = Date.now();
    lastPulseMetadataCaptureAt = cacheTimestamp;
    return newTokenMap.size;
  }

  function debouncedPulseUpdate(newPulseData) {
    window.clearTimeout(pulseUpdateTimeout);
    pendingPulseData.push(newPulseData);
    pulseUpdateTimeout = window.setTimeout(() => {
      try {
        const allNewData = pendingPulseData.splice(0);
        const nextEntries = [];
        allNewData.forEach((pulseData) => {
          if (!pulseData?.content || !Array.isArray(pulseData.content)) {
            return;
          }
          pulseData.content.forEach((token) => {
            nextEntries.push(token);
          });
        });
        debugState.socketLastMergeCount = mergePulseEntries(nextEntries);
      } catch (error) {
        console.error("Error in batched pulse update:", error);
      }
    }, DEBOUNCE_DELAY_MS);
  }

  function findPulseEntryInReactProps(card) {
    if (!(card instanceof HTMLElement)) {
      return null;
    }

    function findEntryInPayload(payload) {
      const seen = new WeakSet();
      const entry = {
        pairAddress: "",
        tokenAddress: ""
      };

      function captureField(name, value) {
        if (entry[name]) {
          return;
        }
        const normalized = String(value || "").trim();
        if (BASE58_ADDRESS_RE.test(normalized)) {
          entry[name] = normalized;
        }
      }

      function walk(value, depth) {
        if (
          entry.pairAddress && entry.tokenAddress ||
          value == null ||
          depth > PULSE_ROW_SEED_MAX_DEPTH
        ) {
          return;
        }
        if (typeof value === "string") {
          return;
        }
        if (typeof value !== "object" || seen.has(value)) {
          return;
        }
        seen.add(value);

        captureField("pairAddress", value.pairAddress);
        captureField("tokenAddress", value.tokenAddress || value.mint);
        if (entry.pairAddress && entry.tokenAddress) {
          return;
        }

        const keys = Object.keys(value).slice(0, PULSE_ROW_SEED_MAX_KEYS);
        for (const key of keys) {
          if (
            key === "return" ||
            key === "sibling" ||
            key === "alternate" ||
            key === "_debugOwner" ||
            key === "_owner"
          ) {
            continue;
          }
          if (key.startsWith("_") && !key.startsWith("__react")) {
            continue;
          }
          try {
            walk(value[key], depth + 1);
          } catch (_error) {}
          if (entry.pairAddress && entry.tokenAddress) {
            return;
          }
        }
      }

      walk(payload, 0);
      return entry.pairAddress && entry.tokenAddress ? entry : null;
    }

    function findEntryInReactPayload(value) {
      try {
        return findEntryInPayload(value);
      } catch (_error) {
        return null;
      }
    }

    function findEntryInReactStateNodeProps(fiber) {
      const stateNode = fiber?.stateNode;
      if (!(stateNode instanceof HTMLElement)) {
        return null;
      }
      for (const key of Object.keys(stateNode)) {
        if (key.startsWith("__reactProps$")) {
          const entry = findEntryInReactPayload(stateNode[key]);
          if (entry) {
            return entry;
          }
        }
      }
      return null;
    }

    const nodes = [card, ...card.querySelectorAll("*")].slice(0, PULSE_ROW_SEED_MAX_NODES);
    for (const node of nodes) {
      for (const key of Object.keys(node)) {
        if (key.startsWith("__reactProps$")) {
          const entry = findEntryInReactPayload(node[key]);
          if (entry) {
            return entry;
          }
          continue;
        }
        if (
          !key.startsWith("__reactFiber$")
        ) {
          continue;
        }
        const fiber = node[key];
        const entry =
          findEntryInReactStateNodeProps(fiber) ||
          findEntryInReactPayload(fiber?.memoizedProps) ||
          findEntryInReactPayload(fiber?.pendingProps);
        if (entry) {
          return entry;
        }
      }
    }
    return null;
  }

  function findPulseCardFromCopyButton(copyButton) {
    if (!(copyButton instanceof HTMLElement)) {
      return null;
    }
    let current = copyButton;
    while (current && current !== document.body) {
      if (!(current instanceof HTMLElement)) {
        current = current.parentElement;
        continue;
      }
      const copyButtons = current.querySelectorAll("button.group\\/copy");
      const hasSingleCopyButton = copyButtons.length === 1 && copyButtons[0] === copyButton;
      const hasMemeLink = current.querySelector("a[href*='/meme/']") instanceof HTMLAnchorElement;
      const hasQuickBuy = current.querySelector("button.group\\/quickBuyButton") instanceof HTMLElement;
      const hasCardLikeText = /new pairs|final stretch|migrated/i.test(String(current.textContent || ""));
      if (hasSingleCopyButton && (hasMemeLink || hasQuickBuy || hasCardLikeText)) {
        return current;
      }
      current = current.parentElement;
    }
    return copyButton.closest(PULSE_CARD_SELECTOR);
  }

  function annotatePulseCardRoute(card, entry) {
    if (!(card instanceof HTMLElement) || !entry) {
      return;
    }
    card.dataset.trenchToolsPulsePairAddress = entry.pairAddress;
    card.dataset.trenchToolsPulseTokenAddress = entry.tokenAddress;
  }

  function seedPulseCacheFromRenderedRows() {
    if (shouldPausePulseWork()) {
      return { cards: 0, entries: 0, changed: false, paused: true };
    }
    let cards = [];
    try {
      const cardSet = new Set();
      document.querySelectorAll(PULSE_CARD_SELECTOR).forEach((card) => {
        if (card instanceof HTMLElement) {
          cardSet.add(card);
        }
      });
      document.querySelectorAll("button.group\\/copy").forEach((copyButton) => {
        const card = findPulseCardFromCopyButton(copyButton);
        if (card instanceof HTMLElement) {
          cardSet.add(card);
        }
      });
      cards = Array.from(cardSet).slice(0, PULSE_ROW_SEED_MAX_CARDS);
    } catch (_error) {
      return { cards: 0, entries: 0, changed: false, paused: false };
    }

    const entries = [];
    for (const card of cards) {
      const copyText = String(card.querySelector("button.group\\/copy")?.textContent || "").trim();
      const seeded = seededPulseRows.get(card);
      if (seeded?.copyText === copyText && seeded.entry) {
        annotatePulseCardRoute(card, seeded.entry);
        entries.push(seeded.entry);
        continue;
      }
      const entry = findPulseEntryInReactProps(card);
      if (entry) {
        seededPulseRows.set(card, { copyText, entry });
        annotatePulseCardRoute(card, entry);
        entries.push(entry);
      }
    }
    const signature = entries
      .map((entry) => `${entry.pairAddress}:${entry.tokenAddress}`)
      .sort()
      .join("|");
    const changed = signature !== lastPulseRowSignature;
    stablePulseRowScans = changed ? 0 : stablePulseRowScans + 1;
    lastPulseRowSignature = signature;
    mergePulseEntries(entries);
    return {
      cards: cards.length,
      entries: entries.length,
      changed,
      paused: false
    };
  }

  function shouldCaptureBatchPairMetadata(url) {
    return String(url || "").includes("/batch-pair-metadata?");
  }

  function capturePairMetadataResponse(response) {
    if (!response || !shouldCaptureBatchPairMetadata(response.url)) {
      return;
    }
    try {
      response.clone().json().then((payload) => {
        if (Array.isArray(payload)) {
          mergePulseEntries(payload);
        }
      }).catch(() => {});
    } catch (_error) {}
  }

  function installFetchBridge() {
    const currentFetch = window.fetch?.__trenchToolsWrappedFetch || window.fetch;
    if (
      typeof currentFetch !== "function" ||
      window.fetch?.__trenchToolsPulseMetadataFetchVersion === OVERRIDE_VERSION
    ) {
      return;
    }
    function TrenchToolsPulseMetadataFetch() {
      return currentFetch.apply(this, arguments).then((response) => {
        capturePairMetadataResponse(response);
        return response;
      });
    }
    TrenchToolsPulseMetadataFetch.__trenchToolsPulseMetadataFetch = true;
    TrenchToolsPulseMetadataFetch.__trenchToolsPulseMetadataFetchVersion = OVERRIDE_VERSION;
    TrenchToolsPulseMetadataFetch.__trenchToolsWrappedFetch = currentFetch;
    window.fetch = TrenchToolsPulseMetadataFetch;
  }

  function backfillPairMetadataFromResources() {
    if (shouldPausePulseWork()) {
      return 0;
    }
    const currentFetch = window.fetch?.__trenchToolsWrappedFetch || window.fetch;
    if (typeof currentFetch !== "function") {
      return 0;
    }
    let urls = [];
    try {
      urls = performance.getEntriesByType("resource")
        .map((entry) => entry.name)
        .filter((url) => shouldCaptureBatchPairMetadata(url));
    } catch (_error) {
      return 0;
    }
    let requested = 0;
    urls.slice(-3).forEach((url) => {
      if (backfilledPairMetadataUrls.has(url)) {
        return;
      }
      backfilledPairMetadataUrls.add(url);
      requested += 1;
      try {
        currentFetch(url, { credentials: "include" })
          .then((response) => response.json())
          .then((payload) => {
            if (Array.isArray(payload)) {
              mergePulseEntries(payload);
            }
          })
          .catch(() => {});
      } catch (_error) {}
    });
    return requested;
  }

  function shouldInterceptWebSocket(url) {
    return String(url || "").toLowerCase().includes("axiom.trade");
  }

  function attachPulseSocketListeners(ws) {
    try {
      debugState.socketAttachCount += 1;
      originalAddEventListener.call(ws, "message", (event) => {
        if (!event?.data || typeof event.data !== "string") {
          return;
        }
        if (!event.data.toLowerCase().includes("update_pulse")) {
          return;
        }
        debugState.socketUpdateCount += 1;
        debugState.socketLastUpdateAt = Date.now();
        try {
          const parsed = JSON.parse(event.data);
          debouncedPulseUpdate(parsed);
        } catch (error) {
          console.error("Error parsing pulse data:", error);
        }
      });
      originalAddEventListener.call(ws, "open", () => {
        debugState.socketJoinCount += 1;
        ws.send('{"action":"join","room":"update_pulse_v2"}');
      });
    } catch (error) {
      console.error("Error adding pulse message listener:", error);
    }
  }

  function installWebSocketBridge() {
    if (typeof PreviousWebSocket !== "function") {
      return;
    }
    if (window.WebSocket?.__trenchToolsPulseWebSocketVersion === OVERRIDE_VERSION) {
      window.WebSocket.__trenchToolsPulseBridgeActive = true;
      return;
    }

    function TrenchToolsPulseWebSocket(url, protocols) {
      const ws =
        arguments.length > 1
          ? new PreviousWebSocket(url, protocols)
          : new PreviousWebSocket(url);
      if (shouldInterceptWebSocket(url)) {
        attachPulseSocketListeners(ws);
      }
      return ws;
    }

    TrenchToolsPulseWebSocket.__trenchToolsPulseWebSocket = true;
    TrenchToolsPulseWebSocket.__trenchToolsPulseWebSocketVersion = OVERRIDE_VERSION;
    TrenchToolsPulseWebSocket.__trenchToolsWrappedWebSocket = PreviousWebSocket;
    TrenchToolsPulseWebSocket.__trenchToolsPulseBridgeActive = true;
    TrenchToolsPulseWebSocket.prototype = PreviousWebSocket.prototype;
    TrenchToolsPulseWebSocket.prototype.constructor = TrenchToolsPulseWebSocket;
    Object.setPrototypeOf(TrenchToolsPulseWebSocket, PreviousWebSocket);
    window.WebSocket = TrenchToolsPulseWebSocket;
  }

  function recentPulseEntryCount() {
    try {
      const cutoffTime = Date.now() - EXPIRY_TIME_MS;
      const entries = getCachedPulseData().content || [];
      return entries.filter((entry) => {
        const tokenTime =
          typeof entry.lastSeen === "number" ? entry.lastSeen : new Date(entry.lastSeen).getTime();
        return tokenTime > cutoffTime;
      }).length;
    } catch (_error) {
      return 0;
    }
  }

  function schedulePulseRowSeed(delayMs = PULSE_ROW_SEED_INTERVAL_MS) {
    window.clearTimeout(pulseRowSeedTimer);
    debugState.rowSeedLastDelayMs = delayMs;
    pulseRowSeedTimer = window.setTimeout(runPulseRowSeed, delayMs);
  }

  function requestPulseRescan(delayMs = 0) {
    if (delayMs === 0 && !document.hidden && isPulseSurface()) {
      installFetchBridge();
      installWebSocketBridge();
      seedPulseCacheFromRenderedRows();
      backfillPairMetadataFromResources();
    }
    schedulePulseRowSeed(delayMs);
    schedulePulseBackfill(delayMs);
    schedulePulseBridgeCheck(delayMs);
  }

  function runPulseRowSeed() {
    pulseRowSeedTimer = 0;
    debugState.rowSeedRuns += 1;
    const result = seedPulseCacheFromRenderedRows();
    if (document.hidden) {
      schedulePulseRowSeed(PULSE_HIDDEN_RECHECK_INTERVAL_MS);
      return;
    }
    if (!isPulseSurface()) {
      schedulePulseRowSeed(PULSE_IDLE_RECHECK_INTERVAL_MS);
      return;
    }
    const enoughRecentEntries = recentPulseEntryCount() >= Math.max(8, Math.min(result.cards || 0, PULSE_ROW_SEED_MAX_CARDS));
    const canBackOff = enoughRecentEntries && result.entries > 0 && stablePulseRowScans >= 2;
    schedulePulseRowSeed(canBackOff ? PULSE_ROW_SEED_BACKOFF_INTERVAL_MS : PULSE_ROW_SEED_INTERVAL_MS);
  }

  function schedulePulseBackfill(delayMs = PULSE_BACKFILL_INTERVAL_MS) {
    window.clearTimeout(pulseBackfillTimer);
    debugState.backfillLastDelayMs = delayMs;
    pulseBackfillTimer = window.setTimeout(runPulseBackfill, delayMs);
  }

  function runPulseBackfill() {
    pulseBackfillTimer = 0;
    debugState.backfillRuns += 1;
    installFetchBridge();
    const requested = backfillPairMetadataFromResources();
    if (document.hidden) {
      schedulePulseBackfill(PULSE_HIDDEN_RECHECK_INTERVAL_MS);
      return;
    }
    if (!isPulseSurface()) {
      schedulePulseBackfill(PULSE_IDLE_RECHECK_INTERVAL_MS);
      return;
    }
    const recentlyCaptured = Date.now() - lastPulseMetadataCaptureAt < 10000;
    schedulePulseBackfill(
      recentlyCaptured && requested === 0
        ? PULSE_BACKFILL_BACKOFF_INTERVAL_MS
        : PULSE_BACKFILL_INTERVAL_MS
    );
  }

  function schedulePulseBridgeCheck(delayMs = PULSE_BRIDGE_RECOVERY_INTERVAL_MS) {
    window.clearTimeout(pulseBridgeTimer);
    debugState.bridgeLastDelayMs = delayMs;
    pulseBridgeTimer = window.setTimeout(runPulseBridgeCheck, delayMs);
  }

  function runPulseBridgeCheck() {
    pulseBridgeTimer = 0;
    debugState.bridgeChecks += 1;
    if (!document.hidden && isPulseSurface()) {
      installWebSocketBridge();
    }
    schedulePulseBridgeCheck(
      document.hidden || !isPulseSurface()
        ? PULSE_IDLE_RECHECK_INTERVAL_MS
        : PULSE_BRIDGE_RECOVERY_INTERVAL_MS
    );
  }

  function handlePulseRescanRequest() {
    if (!document.hidden && isPulseSurface()) {
      requestPulseRescan(0);
    }
  }

  function handlePathChange() {
    const nextPath = window.location.pathname || "";
    if (nextPath === lastObservedPath) {
      return;
    }
    const wasPulse = /\/pulse\b/i.test(lastObservedPath);
    const isPulse = /\/pulse\b/i.test(nextPath);
    lastObservedPath = nextPath;
    if (isPulse && !wasPulse && !document.hidden) {
      requestPulseRescan(0);
    }
  }

  function installPulseRouteWatcher() {
    const currentPushState = window.history?.pushState;
    const currentReplaceState = window.history?.replaceState;
    if (typeof currentPushState === "function" && !currentPushState.__trenchToolsPulseRouteWrapped) {
      const wrappedPushState = function TrenchToolsPulsePushState() {
        const result = currentPushState.apply(this, arguments);
        queueMicrotask(handlePathChange);
        return result;
      };
      wrappedPushState.__trenchToolsPulseRouteWrapped = true;
      wrappedPushState.__trenchToolsWrappedHistoryMethod = currentPushState;
      window.history.pushState = wrappedPushState;
    }
    if (typeof currentReplaceState === "function" && !currentReplaceState.__trenchToolsPulseRouteWrapped) {
      const wrappedReplaceState = function TrenchToolsPulseReplaceState() {
        const result = currentReplaceState.apply(this, arguments);
        queueMicrotask(handlePathChange);
        return result;
      };
      wrappedReplaceState.__trenchToolsPulseRouteWrapped = true;
      wrappedReplaceState.__trenchToolsWrappedHistoryMethod = currentReplaceState;
      window.history.replaceState = wrappedReplaceState;
    }
    window.addEventListener("popstate", handlePathChange);
  }

  visibilityChangeHandler = () => {
    if (!document.hidden) {
      requestPulseRescan(0);
    }
  };
  document.addEventListener("visibilitychange", visibilityChangeHandler);
  document.addEventListener(PULSE_RESCAN_EVENT, handlePulseRescanRequest);

  installPulseRouteWatcher();
  installFetchBridge();
  installWebSocketBridge();
  runPulseBackfill();
  runPulseRowSeed();
  schedulePulseBridgeCheck(PULSE_BRIDGE_RECOVERY_INTERVAL_MS);

})();
