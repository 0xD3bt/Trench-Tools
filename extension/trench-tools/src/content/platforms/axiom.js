(function registerTrenchToolsAxiomAdapter() {
  const runtime = window.TrenchToolsContentRuntime;
  if (!runtime) {
    throw new Error("Trench Tools content runtime missing.");
  }

  runtime.registerPlatformAdapter("axiom", {
    matchesHost(hostname) {
      return hostname === "axiom.trade";
    },

    createAdapter(helpers) {
      const AXIOM_OVERRIDE_SCRIPT_ID = "trench-tools-axiom-override";
      const AXIOM_OVERRIDE_MARKER = "trenchToolsAxiomOverride";
      const AXIOM_OVERRIDE_VERSION = "pulse-row-cache-v9";
      const LAUNCH_SHELL_ROW_SELECTOR = "div.flex.flex-row.gap-4.items-center";
      const PULSE_CARD_SELECTOR = [
        "div[style*='position: absolute'][style*='width: 100%']",
        "div.cursor-pointer[data-search]",
        "div[class*='group/pulseRow']"
      ].join(", ");
      const MEME_LINK_SELECTOR = "a[href*='/meme/']";
      const WALLET_ROW_SELECTOR = [
        "div.flex-row.group",
        "div[class*='flex-row'][class*='group']",
        "div[role='row']"
      ].join(", ");
      const AXIOM_TOKEN_DETAIL_BUTTON_MODE_STORAGE_KEY = "trenchToolsAxiomTokenDetailButtonMode";
      const AXIOM_TOKEN_DETAIL_COMPACT_STORAGE_KEY = "trenchToolsAxiomTokenDetailCompactButtons";
      const AXIOM_TOKEN_DETAIL_PANEL_SIZE_STORAGE_KEY = "trenchToolsAxiomTokenDetailPanelSizes";
      const AXIOM_TOKEN_DETAIL_INITIAL_SIZE_STYLE_ID = "trench-tools-axiom-token-detail-initial-size";
      const AXIOM_INSTANT_TRADE_MODAL_POSITION_KEY = "instantTradeModalPosition";
      const AXIOM_INSTANT_TRADE_MODAL_SIZE_KEY = "instantTradeModalSize";
      const AXIOM_TOKEN_DETAIL_WALLET_SELECTION_KEY = "trenchToolsAxiomTokenDetailWalletSelection";
      const AXIOM_TOKEN_DETAIL_DEFAULT_WIDTH = 312;
      const AXIOM_TOKEN_DETAIL_DEFAULT_HEIGHT = 372;
      const PULSE_PANEL_OWNER_CLASS = "trench-tools-pulse-panel-owner";
      let queuedDomOperationTimer = 0;
      const queuedDomOperations = new Map();
      let overridesInjected = false;
      const targetedObservers = new Map();
      let targetedObserverSignature = "";
      let targetedObserverRetryTimer = 0;
      let targetedObserverRetryCount = 0;
      let axiomTokenDetailButtonMode = readAxiomTokenDetailButtonModePreference();
      let axiomTokenDetailPanelSizes = readAxiomTokenDetailPanelSizePreference();
      let axiomTokenDetailPanelSizeResizeObserver = null;
      let axiomTokenDetailPanelSizeMutationObserver = null;
      let axiomTokenDetailPanelSizeResizePanel = null;
      let axiomTokenDetailPanelApplyingLayout = false;
      let axiomTokenDetailGroupRowResizeObserver = null;
      let axiomTokenDetailGroupRowResizePanel = null;
      let axiomTokenDetailCompactDragHeader = null;
      let axiomTokenDetailCompactDragState = null;
      let axiomTokenDetailWalletControl = null;
      let axiomTokenDetailWalletControlCleanup = null;
      let axiomTokenDetailFloatingPresetRefreshRoot = null;
      let axiomTokenDetailFloatingPresetRefreshObserver = null;
      let axiomTokenDetailFloatingPresetRefreshRoute = null;
      let axiomTokenDetailHardpanelRefreshRoot = null;
      let axiomTokenDetailHardpanelRefreshCleanup = null;
      let axiomTokenDetailHardpanelRefreshObserver = null;

      function isExtensionContextInvalid(error) {
        return /Extension context invalidated/i.test(String(error?.message || error || ""));
      }

      function safeRuntimeGetUrl(path) {
        try {
          return chrome.runtime.getURL(path);
        } catch (error) {
          if (isExtensionContextInvalid(error)) {
            return "";
          }
          throw error;
        }
      }

      function ensureAxiomPageOverrides() {
        const root = document.documentElement;
        if (!root) {
          return;
        }
        const currentOverrideState = root.dataset[AXIOM_OVERRIDE_MARKER];
        if (
          currentOverrideState === `pending:${AXIOM_OVERRIDE_VERSION}` ||
          currentOverrideState === `installed:${AXIOM_OVERRIDE_VERSION}` ||
          overridesInjected ||
          document.getElementById(AXIOM_OVERRIDE_SCRIPT_ID)
        ) {
          return;
        }
        overridesInjected = true;
        const script = document.createElement("script");
        script.id = AXIOM_OVERRIDE_SCRIPT_ID;
        const overrideUrl = safeRuntimeGetUrl("src/content/platforms/axiom-override.js");
        if (!overrideUrl) {
          overridesInjected = false;
          return;
        }
        script.src = overrideUrl;
        script.async = false;
        root.dataset[AXIOM_OVERRIDE_MARKER] = `pending:${AXIOM_OVERRIDE_VERSION}`;
        script.addEventListener("load", () => {
          root.dataset[AXIOM_OVERRIDE_MARKER] = `installed:${AXIOM_OVERRIDE_VERSION}`;
          script.remove();
        }, { once: true });
        script.addEventListener("error", () => {
          overridesInjected = false;
          delete root.dataset[AXIOM_OVERRIDE_MARKER];
          script.remove();
        }, { once: true });
        (document.head || root).appendChild(script);
      }

      function requestAxiomPulseMetadataRescan() {
        try {
          document.dispatchEvent(new Event("trench-tools:axiom-pulse-rescan"));
        } catch (_error) {}
      }

      function getObserverOptions() {
        return {
          childList: true,
          subtree: true,
          attributes: false,
          characterData: false
        };
      }

      function queueDomOperation(key, operation, priority = "normal") {
        const existing = queuedDomOperations.get(key);
        if (!existing || priority === "urgent" || existing.priority !== "urgent") {
          queuedDomOperations.set(key, { operation, priority });
        }
        if (queuedDomOperationTimer) {
          return;
        }
        queuedDomOperationTimer = window.setTimeout(
          () => {
            const operations = Array.from(queuedDomOperations.values());
            queuedDomOperations.clear();
            queuedDomOperationTimer = 0;
            operations
              .sort((left, right) => (left.priority === "urgent" && right.priority !== "urgent" ? -1 : 1))
              .forEach(({ operation: queuedOperation }) => {
                try {
                  queuedOperation();
                } catch (error) {
                  if (isExtensionContextInvalid(error)) {
                    return;
                  }
                  console.error("Axiom DOM operation failed:", error);
                }
              });
          },
          priority === "urgent" ? 0 : 16
        );
      }

      function isPulseUrl(url) {
        return /pulse/i.test(String(url));
      }

      function getAxiomSurfaceState(
        pageAddress = resolveCurrentPageAddress(),
        axiomFeatures = helpers.state.siteFeatures?.axiom || {}
      ) {
        const pulse = isPulseUrl(window.location.href);
        const tokenDetail = Boolean(pageAddress && !pulse);
        const deferredListSurface = !pulse;
        const likelyListSurface = isAxiomListSurface();
        const likelyWalletTrackerSurface = isAxiomWalletTrackerSurface();
        const likelyWatchlistSurface = isAxiomWatchlistSurface();
        const walletTracker = findAxiomWalletTrackerRows().length > 0;
        const watchlist = findAxiomWatchlistAnchors().length > 0;
        return {
          pulse,
          tokenDetail,
          walletTracker,
          watchlist,
          walletTrackerPending: Boolean(
            axiomFeatures.walletTracker &&
            deferredListSurface &&
            (likelyWalletTrackerSurface || likelyListSurface) &&
            !walletTracker
          ),
          watchlistPending: Boolean(
            axiomFeatures.watchlist &&
            deferredListSurface &&
            (likelyWatchlistSurface || likelyListSurface) &&
            !watchlist
          )
        };
      }

      function disconnectAxiomTargetedObserver() {
        targetedObservers.forEach((observer) => observer.disconnect());
        targetedObservers.clear();
        targetedObserverSignature = "";
        window.clearTimeout(targetedObserverRetryTimer);
        targetedObserverRetryTimer = 0;
        targetedObserverRetryCount = 0;
      }

      function ensureAxiomTargetedObserver(surfaceState) {
        const targetInfos = resolveAxiomTargetedObserverTargets(surfaceState);
        if (!targetInfos.length) {
          targetedObservers.forEach((observer) => observer.disconnect());
          targetedObservers.clear();
          targetedObserverSignature = "";
          scheduleAxiomTargetedObserverRetry(surfaceState);
          return;
        }
        window.clearTimeout(targetedObserverRetryTimer);
        targetedObserverRetryTimer = 0;
        targetedObserverRetryCount = 0;
        const signature = targetInfos.map((entry) => entry.key).join("|");
        if (targetedObserverSignature === signature) {
          return;
        }
        targetedObservers.forEach((observer) => observer.disconnect());
        targetedObservers.clear();
        targetedObserverSignature = signature;
        targetInfos.forEach((targetInfo) => {
          const observer = new MutationObserver((mutations) => {
            try {
              handleMutations(mutations);
            } catch (error) {
              if (isExtensionContextInvalid(error)) {
                return;
              }
              console.error("Axiom targeted observer failed:", error);
            }
          });
          observer.observe(targetInfo.target, {
            childList: true,
            characterData: true,
            subtree: true
          });
          targetedObservers.set(targetInfo.key, observer);
        });
      }

      function scheduleAxiomTargetedObserverRetry(surfaceState) {
        const shouldRetry = Boolean(
          surfaceState?.pulse ||
          surfaceState?.tokenDetail ||
          surfaceState?.walletTracker ||
          surfaceState?.watchlist ||
          surfaceState?.walletTrackerPending ||
          surfaceState?.watchlistPending
        );
        if (!shouldRetry || targetedObserverRetryTimer) {
          return;
        }
        targetedObserverRetryCount += 1;
        const retryDelayMs = targetedObserverRetryCount <= 20
          ? Math.min(2500, 100 + targetedObserverRetryCount * 150)
          : 10000;
        targetedObserverRetryTimer = window.setTimeout(() => {
          targetedObserverRetryTimer = 0;
          mount();
        }, retryDelayMs);
      }

      function resolveAxiomTargetedObserverTargets(surfaceState) {
        const targets = [];
        if (surfaceState.pulse) {
          targets.push(
            ...findAxiomPulseObserverTargets().map((target) => ({
              key: `pulse-list:${getTrackedNodeId(target, "trenchToolsObserverTargetId", "observer")}`,
              target
            })),
            ...findAxiomLaunchShellObserverTargets().map((target) => ({
              key: `pulse-shell:${getTrackedNodeId(target, "trenchToolsObserverTargetId", "observer")}`,
              target
            }))
          );
        }
        if (surfaceState.walletTracker) {
          const row = findAxiomWalletTrackerRows()[0];
          const target = row?.parentElement;
          if (target instanceof HTMLElement) {
            targets.push({
              key: `wallet-tracker:${getTrackedNodeId(target, "trenchToolsObserverTargetId", "observer")}`,
              target
            });
          }
        }
        if (surfaceState.watchlist) {
          const anchor = findAxiomWatchlistAnchors()[0];
          const target = anchor?.parentElement;
          if (target instanceof HTMLElement) {
            targets.push({
              key: `watchlist:${getTrackedNodeId(target, "trenchToolsObserverTargetId", "observer")}`,
              target
            });
          }
        }
        if (surfaceState.tokenDetail) {
          const tokenDetailTargets = findAxiomTokenDetailObserverTargets();
          tokenDetailTargets.forEach((target) => {
            targets.push({
              key: `token-detail:${getTrackedNodeId(target, "trenchToolsObserverTargetId", "observer")}`,
              target
            });
          });
        }
        return targets;
      }

      function findAxiomPulseObserverTargets() {
        const scrollContainers = Array.from(
          document.querySelectorAll("div.absolute.inset-0.overflow-y-auto")
        ).filter((element) => element instanceof HTMLElement);
        const populatedContainers = scrollContainers.filter((element) =>
          element.querySelector(PULSE_CARD_SELECTOR) instanceof HTMLElement ||
          element.querySelector("button.group\\/copy") instanceof HTMLElement
        );
        if (populatedContainers.length) {
          return populatedContainers;
        }
        if (scrollContainers.length === 1) {
          return [scrollContainers[0]];
        }

        const firstCard = findAxiomPulseCards()[0];
        const cardContainer = firstCard?.closest("div.absolute.inset-0.overflow-y-auto");
        return cardContainer instanceof HTMLElement ? [cardContainer] : [];
      }

      function findAxiomLaunchShellObserverTargets() {
        const mountTarget = findAxiomLaunchShellMountTarget();
        const target = mountTarget?.parentElement || mountTarget;
        return target instanceof HTMLElement ? [target] : [];
      }

      function isAxiomListSurface() {
        const path = window.location.pathname || "";
        if (/portfolio|wallet|tracker|watchlist/i.test(path)) {
          return true;
        }
        const pageText = String(document.body?.innerText || "").slice(0, 4000);
        return /\b(portfolio|wallet tracker|watchlist)\b/i.test(pageText);
      }

      function isAxiomWalletTrackerSurface() {
        const path = window.location.pathname || "";
        if (/portfolio|wallet|tracker/i.test(path)) {
          return true;
        }
        const pageText = String(document.body?.innerText || "").slice(0, 4000);
        return /\b(portfolio|wallet tracker|tracked wallets?|wallets bought)\b/i.test(pageText);
      }

      function isAxiomWatchlistSurface() {
        const path = window.location.pathname || "";
        if (/watchlist/i.test(path)) {
          return true;
        }
        const pageText = String(document.body?.innerText || "").slice(0, 4000);
        return /\bwatchlist\b/i.test(pageText);
      }

      function resolveObservedAddress(...candidates) {
        for (const candidate of candidates) {
          const normalized = String(candidate || "").trim();
          if (!normalized) {
            continue;
          }
          return normalized;
        }
        return "";
      }

      function extractAxiomPairFromMemeUrl(url) {
        return resolveObservedAddress(helpers.extractMintFromUrl(url));
      }

      function buildAxiomPairRouteReference(pairAddressOrUrlId, surface, url = window.location.href) {
        const pairAddress = String(pairAddressOrUrlId || "").trim();
        if (!pairAddress) {
          return null;
        }
        const pulseCacheEntry = lookupPulseCacheEntry(pairAddress);
        const resolvedPairAddress = String(pulseCacheEntry?.pairAddress || pairAddress).trim();
        const tokenMint = String(pulseCacheEntry?.tokenAddress || "").trim();
        return {
          address: resolvedPairAddress,
          routeKey: resolvedPairAddress,
          pairAddress: resolvedPairAddress,
          mint: tokenMint || null,
          pair: null,
          source: "page",
          surface,
          url
        };
      }

      function buildAxiomPairRouteReferenceFromUrl(url, surface) {
        return buildAxiomPairRouteReference(extractAxiomPairFromMemeUrl(url), surface, url);
      }

      function extractAxiomTokenMintFromElement(root, pairAddress = "") {
        if (!(root instanceof Element) && root !== document) {
          return "";
        }
        const normalizedPairAddress = String(pairAddress || "").trim();
        const images = Array.from(root.querySelectorAll("img"));
        for (const image of images) {
          const candidates = [image.currentSrc, image.src, image.getAttribute("src"), image.alt];
          for (const candidate of candidates) {
            const mint = String(helpers.extractMintFromText(candidate) || "").trim();
            if (mint && mint !== normalizedPairAddress) {
              return mint;
            }
          }
        }
        return "";
      }

      function buildAxiomPairRouteReferenceFromElement(url, surface, root) {
        const route = buildAxiomPairRouteReferenceFromUrl(url, surface);
        if (!route || route.mint) {
          return route;
        }
        const tokenMint = extractAxiomTokenMintFromElement(root, route.address);
        return tokenMint ? { ...route, mint: tokenMint } : route;
      }

      function routePayloadFromButton(button, fallback = {}) {
        const address = String(button?.getAttribute("data-route-key") || fallback.address || "").trim();
        const mint = String(button?.getAttribute("data-mint") || fallback.mint || "").trim();
        const pair = String(button?.getAttribute("data-pair") || fallback.pair || "").trim();
        if (!address) {
          return null;
        }
        return {
          address,
          ...(mint ? { mint } : {}),
          ...(pair ? { pair } : {}),
          source: "page",
          surface: fallback.surface || "",
          url: fallback.url || window.location.href
        };
      }

      function attachAxiomIntentPrewarm(control, surface, options = {}) {
        if (!(control instanceof HTMLElement) || control.dataset.trenchToolsAxiomIntentPrewarm === "1") {
          return;
        }
        control.dataset.trenchToolsAxiomIntentPrewarm = "1";
        const prewarm = () => {
          const route = routePayloadFromButton(control, {
            address: options.address,
            mint: options.mint,
            pair: options.pair || "",
            surface,
            url: options.url || control.getAttribute("data-route-url") || window.location.href
          });
          if (!route?.address || typeof helpers.prewarmForMint !== "function") {
            return;
          }
          helpers.prewarmForMint(route, {
            surface,
            sourceUrl: route.url || options.url || window.location.href,
            side: options.side || control.getAttribute("data-side") || "",
            reason: options.reason || `axiom-${surface}-intent`
          });
        };
        control.addEventListener("pointerenter", prewarm);
        control.addEventListener("pointerdown", prewarm);
        control.addEventListener("mousedown", prewarm);
        control.addEventListener("focus", prewarm);
      }

      function buildObservedCandidate(address, surface, url = window.location.href) {
        return buildAxiomPairRouteReference(address, surface, url);
      }

      function findPulseRouteLink(root) {
        if (!(root instanceof Element) && root !== document) {
          return null;
        }
        return Array.from(root.querySelectorAll(MEME_LINK_SELECTOR)).find((anchor) =>
          anchor instanceof HTMLAnchorElement && helpers.extractMintFromUrl(anchor.href)
        ) || null;
      }

      function resolveCurrentPageAddress() {
        // On Axiom token-detail pages the route is the only authoritative
        // identity source we trust. The URL often carries the active
        // pool/pair identifier rather than a literal mint, and the backend
        // canonicalizes pair -> mint during resolve-token. We intentionally do
        // not fall back to other visible addresses or surrounding `/meme/*`
        // links because those can belong to unrelated assets shown elsewhere
        // on the page.
        return resolveObservedAddress(helpers.extractMintFromUrl(window.location.href));
      }

      function getCurrentTokenCandidate() {
        const surface = isPulseUrl(window.location.href) ? "pulse" : "token_detail";
        const directAddress = resolveCurrentPageAddress();

        // Token-detail pages bind only to the active route identity.
        if (surface === "token_detail" && directAddress) {
          return buildObservedCandidate(directAddress, surface, window.location.href);
        }

        const pulseRoots = [];
        const ownerCard = document.querySelector(`.${PULSE_PANEL_OWNER_CLASS}`);
        if (ownerCard instanceof HTMLElement) {
          pulseRoots.push(ownerCard);
        }
        findAxiomPulseCards().forEach((card) => {
          if (card !== ownerCard) {
            pulseRoots.push(card);
          }
        });

        for (const root of pulseRoots) {
          const pulseLink = findPulseRouteLink(root);
          if (!(pulseLink instanceof HTMLAnchorElement)) {
            continue;
          }
          const pulseAddress = resolveObservedAddress(helpers.extractMintFromUrl(pulseLink.href));
          const candidate = buildObservedCandidate(pulseAddress, "pulse", pulseLink.href);
          if (candidate) {
            return candidate;
          }
        }

        return {
          address: "",
          mint: null,
          pair: null,
          source: "page",
          surface: "pulse",
          url: window.location.href
        };
      }

      function mount() {
        const axiomFeatures = helpers.state.siteFeatures?.axiom || {};
        const instantTradeEnabled = Boolean(axiomFeatures.enabled && axiomFeatures.instantTrade);
        if (!axiomFeatures.enabled) {
          disconnectAxiomTargetedObserver();
          cleanupAxiomTokenDetailManualBuyButtons();
          helpers.removeInjectedControls("[data-trench-tools-token-detail-action-inline]");
          helpers.removeInjectedControls("[data-trench-tools-pulse-inline]");
          helpers.removeInjectedControls("[data-trench-tools-pulse-panel-inline]");
          helpers.removeInjectedControls("[data-trench-tools-pulse-vamp-inline]");
          helpers.removeInjectedControls("[data-trench-tools-pulse-dex-inline]");
          helpers.removeInjectedControls("[data-trench-tools-wallet-tracker-inline]");
          helpers.removeInjectedControls("[data-trench-tools-axiom-watchlist-inline]");
          helpers.removeInjectedControls("[data-trench-tools-launchdeck-shell]");
          return;
        }
        ensureAxiomPageOverrides();
        if (!isPulseUrl(window.location.href) && instantTradeEnabled) {
          ensureAxiomTokenDetailInitialSizeStyle();
        }
        if (!instantTradeEnabled) {
          cleanupAxiomTokenDetailManualBuyButtons();
        }
        if (!axiomFeatures.pulseButton) {
          helpers.removeInjectedControls("[data-trench-tools-pulse-inline]");
        }
        if (!axiomFeatures.pulsePanel) {
          helpers.removeInjectedControls("[data-trench-tools-pulse-panel-inline]");
        }
        if (!shouldShowAxiomVampIcon("pulse")) {
          helpers.removeInjectedControls("[data-trench-tools-pulse-vamp-inline]");
        }
        if (!shouldShowAxiomDexScreenerIcon("pulse")) {
          helpers.removeInjectedControls("[data-trench-tools-pulse-dex-inline]");
        }
        if (!axiomFeatures.walletTracker) {
          helpers.removeInjectedControls("[data-trench-tools-wallet-tracker-inline]");
        }
        if (!axiomFeatures.watchlist) {
          helpers.removeInjectedControls("[data-trench-tools-axiom-watchlist-inline]");
        }

        const pageAddress = resolveCurrentPageAddress();
        const surfaceState = getAxiomSurfaceState(pageAddress, axiomFeatures);
        ensureAxiomTargetedObserver(surfaceState);
        if (surfaceState.tokenDetail) {
          const tokenDetailRoute = buildObservedCandidate(pageAddress, "token_detail", window.location.href) || pageAddress;
          mountAxiomTokenDetailHeaderActions(tokenDetailRoute);
          if (instantTradeEnabled) {
            mountAxiomTokenDetailQuickButton(tokenDetailRoute);
          }
        } else if (!surfaceState.tokenDetail) {
          cleanupAxiomTokenDetailManualBuyButtons();
          helpers.removeInjectedControls("[data-trench-tools-token-detail-action-inline]");
        }

        if (surfaceState.pulse && axiomFeatures.launchdeckInjection) {
          mountAxiomLaunchShellControls();
        } else {
          helpers.removeInjectedControls("[data-trench-tools-launchdeck-shell]");
        }

        if (
          surfaceState.pulse &&
          (
            axiomFeatures.pulseButton ||
            axiomFeatures.pulsePanel ||
            shouldShowAxiomVampIcon("pulse") ||
            shouldShowAxiomDexScreenerIcon("pulse")
          )
        ) {
          requestAxiomPulseMetadataRescan();
          mountAxiomPulseQuickButtons();
        } else if (!surfaceState.pulse) {
          helpers.removeInjectedControls("[data-trench-tools-pulse-inline]");
          helpers.removeInjectedControls("[data-trench-tools-pulse-panel-inline]");
          helpers.removeInjectedControls("[data-trench-tools-pulse-vamp-inline]");
          helpers.removeInjectedControls("[data-trench-tools-pulse-dex-inline]");
        }
        if (surfaceState.walletTracker && axiomFeatures.walletTracker) {
          mountAxiomWalletTrackerQuickButtons();
        } else if (!surfaceState.walletTracker || !axiomFeatures.walletTracker) {
          helpers.removeInjectedControls("[data-trench-tools-wallet-tracker-inline]");
        }
        if (surfaceState.watchlist && axiomFeatures.watchlist) {
          mountAxiomWatchlistQuickButtons();
        } else if (!surfaceState.watchlist || !axiomFeatures.watchlist) {
          helpers.removeInjectedControls("[data-trench-tools-axiom-watchlist-inline]");
        }
      }

      function mountAxiomPulseQuickButtons() {
        for (const card of findAxiomPulseCards()) {
          mountAxiomPulseQuickButtonCard(card);
        }
      }

      function findAxiomPulseCardFromCopyButton(copyButton) {
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

      function findAxiomPulseCards() {
        const cards = new Set();
        document.querySelectorAll("button.group\\/copy").forEach((button) => {
          const card = findAxiomPulseCardFromCopyButton(button);
          if (card instanceof HTMLElement) {
            cards.add(card);
          }
        });
        if (!cards.size) {
          document.querySelectorAll(PULSE_CARD_SELECTOR).forEach((card) => {
            if (card instanceof HTMLElement) {
              cards.add(card);
            }
          });
        }
        return Array.from(cards);
      }

      function resolveAxiomPulseCardForControl(control, fallbackCard) {
        if (control instanceof HTMLElement) {
          const closestCard = control.closest(PULSE_CARD_SELECTOR);
          if (closestCard instanceof HTMLElement) {
            return closestCard;
          }
          let current = control.parentElement;
          while (current && current !== document.body) {
            const copyButton = current.querySelector("button.group\\/copy");
            if (copyButton instanceof HTMLElement) {
              return findAxiomPulseCardFromCopyButton(copyButton) || current;
            }
            current = current.parentElement;
          }
        }
        return fallbackCard instanceof HTMLElement && document.contains(fallbackCard)
          ? fallbackCard
          : null;
      }

      function mountAxiomLaunchShellControls() {
        const target = findAxiomLaunchShellMountTarget();
        if (!(target instanceof HTMLElement)) {
          return;
        }
        const ttCompactLogoUrl = safeRuntimeGetUrl("assets/TT-compact.png");
        if (!ttCompactLogoUrl) {
          return;
        }
        let wrapper = target.querySelector("[data-trench-tools-launchdeck-shell]");
        if (!(wrapper instanceof HTMLElement)) {
          wrapper = document.createElement("div");
          wrapper.setAttribute("data-trench-tools-launchdeck-shell", "true");
          Object.assign(wrapper.style, {
            display: "inline-flex",
            alignItems: "center",
            gap: "6px",
            marginLeft: "8px",
          });
          wrapper.appendChild(
            buildLaunchShellButton(
              "Deploy",
              "#000000",
              "#ffffff",
              () => {
                return helpers.openLaunchdeckOverlay({ mode: "create" });
              },
              { iconUrl: ttCompactLogoUrl, iconAlt: "TT" },
            ),
          );
          wrapper.appendChild(
            buildLaunchShellButton("Vamp", "#000000", "#ffffff", async () => {
              const contractAddress = await promptVampContractAddress();
              if (!contractAddress) return;
              return helpers.openLaunchdeckOverlay({
                mode: "create",
                contractAddress,
              });
            }),
          );
          wrapper.appendChild(
            buildLaunchShellButton("Webapp", "#000000", "#ffffff", () => {
              return helpers.openLaunchdeckPopout({ mode: "webapp" });
            }),
          );
          target.appendChild(wrapper);
        }
      }

      function buildLaunchShellButton(label, background, color, onClick, options = {}) {
        const { iconUrl = "", iconAlt = "" } = options;
        const buttonFont = 'Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
        const button = document.createElement("button");
        button.type = "button";
        Object.assign(button.style, {
          height: "28px",
          minHeight: "28px",
          borderRadius: "12px",
          border: "1px solid rgba(255, 255, 255, 0.20)",
          background,
          color,
          padding: iconUrl ? "0 10px 0 9px" : "0 11px",
          fontSize: "12px",
          fontWeight: "600",
          fontFamily: buttonFont,
          letterSpacing: "0",
          cursor: "pointer",
          transition: "transform 120ms ease, opacity 120ms ease, background-color 120ms ease, border-color 120ms ease",
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          gap: iconUrl ? "6px" : "0",
          lineHeight: "1",
          boxShadow: "none",
          whiteSpace: "nowrap",
        });
        if (iconUrl) {
          const icon = document.createElement("img");
          icon.src = iconUrl;
          icon.alt = iconAlt;
          Object.assign(icon.style, {
            width: "14px",
            height: "14px",
            display: "block",
            objectFit: "contain",
            flex: "0 0 auto",
            filter: "brightness(0) invert(1)",
          });
          button.appendChild(icon);
        }
        const labelNode = document.createElement("span");
        labelNode.textContent = label;
        button.appendChild(labelNode);
        button.addEventListener("mouseenter", () => {
          button.style.transform = "translateY(-1px)";
          button.style.opacity = "1";
          button.style.backgroundColor = "#18181b";
          button.style.borderColor = "rgba(255, 255, 255, 0.28)";
        });
        button.addEventListener("mouseleave", () => {
          button.style.transform = "translateY(0)";
          button.style.opacity = "1";
          button.style.backgroundColor = background;
          button.style.borderColor = "rgba(255, 255, 255, 0.20)";
        });
        button.addEventListener("click", (event) => {
          event.preventDefault();
          event.stopPropagation();
          void Promise.resolve()
            .then(() => onClick())
            .catch((error) => {
              if (isExtensionContextInvalid(error)) {
                return;
              }
              helpers.showToast?.(error?.message || "Action failed.", "error");
              console.error("Axiom launch shell action failed:", error);
            });
        });
        return button;
      }

      function findAxiomLaunchShellMountTarget() {
        const displayRow = Array.from(
          document.querySelectorAll(LAUNCH_SHELL_ROW_SELECTOR)
        ).find((row) => {
          const trigger = row.querySelector("button span");
          if (trigger?.textContent?.trim() === "Display") {
            return true;
          }
          return String(row.textContent || "").trim().startsWith("Display");
        });
        if (displayRow instanceof HTMLElement) {
          return displayRow;
        }
        return document.querySelector("header div.flex.flex-row.gap-4.items-center");
      }

      function isLaunchShellRowElement(element) {
        if (!(element instanceof HTMLElement)) {
          return false;
        }
        const row = element.matches(LAUNCH_SHELL_ROW_SELECTOR)
          ? element
          : element.closest(LAUNCH_SHELL_ROW_SELECTOR);
        if (!(row instanceof HTMLElement)) {
          return false;
        }
        const trigger = row.querySelector("button span");
        if (trigger?.textContent?.trim() === "Display") {
          return true;
        }
        return String(row.textContent || "").trim().startsWith("Display");
      }

      function promptVampContractAddress() {
        return new Promise((resolve) => {
          const bodyFont = 'Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
          let overlay = document.getElementById("trench-tools-vamp-overlay");
          if (overlay instanceof HTMLElement) {
            overlay.remove();
          }
          overlay = document.createElement("div");
          overlay.id = "trench-tools-vamp-overlay";
          Object.assign(overlay.style, {
            position: "fixed",
            inset: "0",
            background: "rgba(4, 6, 10, 0.22)",
            zIndex: "2147483647",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            padding: "24px",
            backdropFilter: "blur(18px) saturate(1.02)",
          });
          overlay.innerHTML = `
            <div style="width:min(360px, 100%); display:grid; gap:12px; padding:14px; border:1px solid rgba(255,255,255,0.10); border-radius:14px; background:#050505; box-shadow:0 24px 80px rgba(0,0,0,0.52), 0 0 0 1px rgba(255,255,255,0.02); color:#fff; font-family:${bodyFont};">
              <div style="display:flex; align-items:center; min-width:0;">
                <div style="font:600 14px/1.2 ${bodyFont}; color:#ffffff; letter-spacing:-0.01em;">Vamp a coin</div>
              </div>
              <div style="display:grid; gap:8px;">
                <input id="trench-tools-vamp-input" type="text" inputmode="verbatim" spellcheck="false" autocomplete="off" autocorrect="off" autocapitalize="off" aria-autocomplete="none" enterkeyhint="done" data-form-type="other" data-lpignore="true" data-1p-ignore="true" name="tt-vamp-contract" placeholder="Paste token mint / contract" style="width:100%; height:40px; padding:0 12px; border-radius:10px; border:1px solid rgba(255,255,255,0.12); background:#111111; color:#ffffff; outline:none; font:500 12px/1 ${bodyFont}; box-sizing:border-box;">
                <div id="trench-tools-vamp-error" style="min-height:16px; color:#ffffff; opacity:0.78; font:500 11px/1.35 ${bodyFont};"></div>
              </div>
              <div style="display:grid; grid-template-columns:1fr 1fr; gap:8px;">
                <button type="button" id="trench-tools-vamp-cancel" style="height:38px; padding:0 14px; border-radius:8px; border:1px solid rgba(255,255,255,0.10); background:#101010; color:#ffffff; cursor:pointer; font:600 12px/1 ${bodyFont}; transition:transform 120ms ease, opacity 120ms ease, border-color 120ms ease, background 120ms ease;">Cancel</button>
                <button type="button" id="trench-tools-vamp-confirm" style="height:38px; padding:0 14px; border-radius:8px; border:1px solid rgba(255,255,255,0.12); background:#ffffff; color:#050505; cursor:pointer; font:700 12px/1 ${bodyFont}; transition:transform 120ms ease, opacity 120ms ease, box-shadow 120ms ease, background 120ms ease;">Vamp</button>
              </div>
            </div>
          `;
          const input = overlay.querySelector("#trench-tools-vamp-input");
          if (input instanceof HTMLInputElement) {
            input.setAttribute("autocomplete", "off");
            input.setAttribute("autocorrect", "off");
            input.setAttribute("autocapitalize", "off");
            input.setAttribute("spellcheck", "false");
            input.setAttribute("aria-autocomplete", "none");
            input.setAttribute("data-form-type", "other");
            input.setAttribute("data-lpignore", "true");
            input.setAttribute("data-1p-ignore", "true");
            input.setAttribute("name", "tt-vamp-contract");
            input.addEventListener("focus", () => {
              input.style.borderColor = "rgba(255,255,255,0.34)";
              input.style.boxShadow = "0 0 0 1px rgba(255,255,255,0.12)";
            });
            input.addEventListener("blur", () => {
              input.style.borderColor = "rgba(255,255,255,0.12)";
              input.style.boxShadow = "none";
            });
          }
          const errorNode = overlay.querySelector("#trench-tools-vamp-error");
          const cancelButton = overlay.querySelector("#trench-tools-vamp-cancel");
          const confirmButton = overlay.querySelector("#trench-tools-vamp-confirm");
          [cancelButton, confirmButton].forEach((button) => {
            if (!(button instanceof HTMLButtonElement)) return;
            button.addEventListener("mouseenter", () => {
              button.style.transform = "translateY(-1px)";
              button.style.opacity = "0.96";
              if (button === confirmButton) {
                button.style.boxShadow = "0 10px 22px rgba(255,255,255,0.12)";
              } else {
                button.style.borderColor = "rgba(255,255,255,0.18)";
                button.style.background = "#141414";
              }
            });
            button.addEventListener("mouseleave", () => {
              button.style.transform = "translateY(0)";
              button.style.opacity = "1";
              if (button === confirmButton) {
                button.style.boxShadow = "none";
              } else {
                button.style.borderColor = "rgba(255,255,255,0.10)";
                button.style.background = "#101010";
              }
            });
          });
          const finish = (value) => {
            overlay.remove();
            resolve(value);
          };
          const confirm = () => {
            const value = String(input?.value || "").trim();
            if (!/^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(value)) {
              if (errorNode) errorNode.textContent = "Enter a valid Solana contract address.";
              input?.focus();
              return;
            }
            finish(value);
          };
          overlay.addEventListener("click", (event) => {
            if (event.target === overlay) finish(null);
          });
          overlay.querySelector("#trench-tools-vamp-cancel")?.addEventListener("click", () => finish(null));
          overlay.querySelector("#trench-tools-vamp-confirm")?.addEventListener("click", confirm);
          input?.addEventListener("keydown", (event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              confirm();
            }
            if (event.key === "Escape") {
              finish(null);
            }
          });
          document.documentElement.appendChild(overlay);
          queueMicrotask(() => input?.focus());
        });
      }

      function mountAxiomPulseQuickButtonCard(card) {
        if (!(card instanceof HTMLElement)) {
          return;
        }
        const cardId = getAxiomPulseCardId(card);

        const copyButton = card.querySelector("button.group\\/copy");
        if (!(copyButton instanceof HTMLElement)) {
          teardownAxiomPulseCardControls(card);
          return;
        }

        const tokenUrl = findPulseRouteLink(card)?.href || window.location.href;

        const targets = findAxiomPulseMountTargets(card);
        if (!targets.length) {
          teardownAxiomPulseCardControls(card);
          return;
        }

        targets.forEach((target) => {
          if (!target.dataset.trenchToolsPulseAnchorId) {
            target.dataset.trenchToolsPulseAnchorId = `pulse-anchor-${Math.random().toString(36).slice(2, 10)}`;
          }
        });

        cleanupAxiomPulseInlineControls(card, targets);
        targets.forEach((target) => {
          if (helpers.state.siteFeatures?.axiom?.pulseButton) {
            ensureAxiomPulseInlineControl(card, target, tokenUrl);
          }
          if (helpers.state.siteFeatures?.axiom?.pulsePanel) {
            ensureAxiomPulsePanelControl(card, target, tokenUrl, cardId);
          }
        });

        if (shouldShowAxiomVampIcon("pulse") || shouldShowAxiomDexScreenerIcon("pulse")) {
          ensureAxiomPulseVampIcon(card);
        } else {
          removeAxiomPulseVampIcon(card);
        }
      }

      function findAxiomPulseMountTargets(card) {
        const visibleMountTarget = (element) => {
          if (!(element instanceof HTMLElement)) {
            return false;
          }
          const rect = element.getBoundingClientRect();
          return rect.width > 0 && rect.height > 0;
        };

        const pumpBadge = card.querySelector('img[src="https://axiom.trade/images/pump.svg"]');
        const isPumpCard = Boolean(pumpBadge && pumpBadge.closest("div.bg-primaryBlue"));
        if (isPumpCard) {
          const targets = Array.from(
            card.querySelectorAll(
              "img[src*='pump-grad.svg'], img[src*='virtual-curve-grad.svg'], img[src*='bonk-grad.svg'], img[src*='boop-grad.svg']"
            )
          )
            .map((image) => image.closest("div.relative"))
            .filter(visibleMountTarget);
          if (targets.length) {
            return [Array.from(new Set(targets))[0]];
          }
        }

        const quickBuyTargets = Array.from(card.querySelectorAll("button.group\\/quickBuyButton")).filter(
          visibleMountTarget
        );
        if (quickBuyTargets.length) {
          return [quickBuyTargets[0]];
        }
        const copyButton = card.querySelector("button.group\\/copy");
        if (copyButton instanceof HTMLElement) {
          return [copyButton];
        }
        const tokenLink = card.querySelector("a[href*='/meme/']");
        if (tokenLink instanceof HTMLElement) {
          return [tokenLink];
        }
        return [];
      }

      function cleanupAxiomPulseInlineControls(card, targets) {
        const activeAnchorIds = new Set(
          targets
            .map((target) => target.dataset.trenchToolsPulseAnchorId)
            .filter(Boolean)
        );

        card
          .querySelectorAll(
            "[data-trench-tools-pulse-inline], [data-trench-tools-pulse-panel-inline]"
          )
          .forEach((element) => {
            const anchorId = element.getAttribute("data-anchor-id") || "";
            if (!anchorId || !activeAnchorIds.has(anchorId)) {
              helpers.teardownInlineSizeSync(element);
              element.remove();
            }
          });

        card.querySelectorAll("span[data-trench-tools-inline]").forEach((element) => {
          element.remove();
        });
      }

      function teardownAxiomPulseCardControls(card) {
        if (!(card instanceof HTMLElement)) {
          return;
        }
        card
          .querySelectorAll(
            "[data-trench-tools-pulse-inline], [data-trench-tools-pulse-panel-inline], [data-trench-tools-pulse-vamp-inline], [data-trench-tools-pulse-dex-inline]"
          )
          .forEach((element) => {
            helpers.teardownInlineSizeSync(element);
            element.remove();
          });
        card.querySelectorAll("span[data-trench-tools-inline]").forEach((element) => {
          element.remove();
        });
      }

      function getAxiomPulseCardId(card) {
        if (!(card instanceof HTMLElement)) {
          return "";
        }
        if (!card.dataset.trenchToolsPulseCardId) {
          card.dataset.trenchToolsPulseCardId =
            card.getAttribute("data-card-id") ||
            card.style.transform ||
            `pulse-card-${Math.random().toString(36).slice(2, 10)}`;
        }
        return card.dataset.trenchToolsPulseCardId;
      }

      function markAxiomPulsePanelOwner(cardId) {
        document.querySelectorAll(`.${PULSE_PANEL_OWNER_CLASS}`).forEach((element) => {
          element.classList.remove(PULSE_PANEL_OWNER_CLASS);
        });
        const activeCard = document.querySelector(`[data-trench-tools-pulse-card-id="${cardId}"]`);
        if (activeCard instanceof HTMLElement) {
          activeCard.classList.add(PULSE_PANEL_OWNER_CLASS);
        }
      }

      function pauseAxiomPulsePanelRow(card) {
        if (!(card instanceof HTMLElement)) {
          return null;
        }
        document.querySelectorAll(".trench-tools-pulse-panel-open-card").forEach((element) => {
          if (element !== card && typeof element._trenchToolsPulsePanelResume === "function") {
            element._trenchToolsPulsePanelResume();
          }
        });

        card._trenchToolsPulsePanelPreviousBackground = card.style.backgroundColor;
        card.classList.add("trench-tools-pulse-panel-open-card");
        card.style.setProperty("background-color", "rgba(255, 255, 255, 0.05)", "important");

        let overlay = card.querySelector(":scope > .trench-tools-pulse-panel-hover-overlay");
        if (!(overlay instanceof HTMLElement)) {
          overlay = document.createElement("div");
          overlay.className = "trench-tools-pulse-panel-hover-overlay";
          Object.assign(overlay.style, {
            position: "fixed",
            inset: "0",
            zIndex: "1",
            pointerEvents: "none",
            backgroundColor: "transparent",
            display: "none"
          });
          card.appendChild(overlay);
        }

        const rowContainer = card.closest(".h-full.flex-1.overflow-hidden.bg-backgroundSecondary");
        const dispatchMouseEvent = (element, type) => {
          if (element instanceof HTMLElement) {
            element.dispatchEvent(new MouseEvent(type, {
              bubbles: true,
              cancelable: true,
              view: window
            }));
          }
        };
        const hold = () => {
          dispatchMouseEvent(card, "mouseenter");
          dispatchMouseEvent(card, "mouseover");
          dispatchMouseEvent(card, "mousedown");
          dispatchMouseEvent(rowContainer, "mouseenter");
        };
        hold();
        const holdInterval = window.setInterval(() => {
          if (!document.body.contains(card) || !card.querySelector(":scope > .trench-tools-pulse-panel-hover-overlay")) {
            window.clearInterval(holdInterval);
            return;
          }
          hold();
        }, 100);

        const resume = () => {
          window.clearInterval(holdInterval);
          overlay?.remove();
          if (card._trenchToolsPulsePanelPreviousBackground) {
            card.style.backgroundColor = card._trenchToolsPulsePanelPreviousBackground;
          } else {
            card.style.removeProperty("background-color");
          }
          delete card._trenchToolsPulsePanelPreviousBackground;
          delete card._trenchToolsPulsePanelResume;
          card.classList.remove("trench-tools-pulse-panel-open-card");
          ["mouseup", "mouseleave", "mouseout", "blur"].forEach((eventName) => {
            dispatchMouseEvent(card, eventName);
            dispatchMouseEvent(rowContainer, eventName);
          });
          window.setTimeout(() => {
            dispatchMouseEvent(card, "mouseleave");
            dispatchMouseEvent(rowContainer, "mouseleave");
          }, 50);
        };
        card._trenchToolsPulsePanelResume = resume;
        return resume;
      }

      function pulseCopyParts(copyButton) {
        const copyText = String(copyButton?.textContent || "").trim();
        if (!copyText.includes("...")) {
          return null;
        }
        const [prefix, suffix] = copyText.split("...");
        const normalizedPrefix = String(prefix || "").trim();
        const normalizedSuffix = String(suffix || "").trim();
        if (!normalizedPrefix || !normalizedSuffix) {
          return null;
        }
        return {
          copyText,
          prefix: normalizedPrefix,
          suffix: normalizedSuffix
        };
      }

      function pulseRouteFromEntry(entry) {
        const pairAddress = String(entry?.pairAddress || "").trim();
        const tokenAddress = String(entry?.tokenAddress || "").trim();
        if (!pairAddress) {
          return null;
        }
        return {
          address: pairAddress,
          routeKey: pairAddress,
          pairAddress,
          mint: tokenAddress,
          pair: null,
          surface: "pulse",
          source: "page"
        };
      }

      function pulseRouteFromCardDataset(card) {
        if (!(card instanceof HTMLElement)) {
          return null;
        }
        return pulseRouteFromEntry({
          pairAddress: card.dataset.trenchToolsPulsePairAddress,
          tokenAddress: card.dataset.trenchToolsPulseTokenAddress
        });
      }

      function resolvePulseTradeRouteForCard(card) {
        if (!(card instanceof HTMLElement)) {
          return null;
        }
        const datasetRoute = pulseRouteFromCardDataset(card);
        if (datasetRoute) {
          return datasetRoute;
        }
        const copyButton = card.querySelector("button.group\\/copy");
        const parts = pulseCopyParts(copyButton);
        if (parts) {
          const pulseEntry = lookupPulseCacheEntry("", parts.prefix, parts.suffix);
          const route = pulseRouteFromEntry(pulseEntry);
          if (route) {
            return route;
          }
        }
        const routeLink = findPulseRouteLink(card);
        if (routeLink instanceof HTMLAnchorElement) {
          return buildAxiomPairRouteReferenceFromElement(routeLink.href, "pulse", card);
        }
        return null;
      }

      async function resolvePulseTradeRouteWithFallback(card, control, tokenUrl = window.location.href) {
        const primaryRoute =
          normalizePulseRoute(resolvePulseTradeRouteForCard(card)) ||
          routePayloadFromPulseControl(control, { url: tokenUrl });
        if (primaryRoute?.address) {
          return primaryRoute;
        }
        return null;
      }

      function normalizePulseRoute(route) {
        const routeAddress = String(route?.address || "").trim();
        if (!routeAddress) {
          return null;
        }
        const routeMint = String(route?.mint || "").trim();
        if (routeMint) {
          return route;
        }
        const pulseEntry = lookupPulseCacheEntry(routeAddress);
        const cachedRoute = pulseRouteFromEntry(pulseEntry);
        if (cachedRoute) {
          return cachedRoute;
        }
        return route;
      }

      function routePayloadFromPulseControl(control, fallback = {}) {
        const route = routePayloadFromButton(control, {
          ...fallback,
          surface: "pulse"
        });
        return normalizePulseRoute(route);
      }

      function bindPulseRouteToControl(control, route, tokenUrl = window.location.href) {
        if (!(control instanceof HTMLElement)) {
          return;
        }
        const normalizedRoute = normalizePulseRoute(route);
        if (!normalizedRoute?.address) {
          control.removeAttribute("data-route-key");
          control.removeAttribute("data-mint");
          control.removeAttribute("data-pair");
          control.removeAttribute("data-route-url");
          return;
        }
        control.setAttribute("data-route-key", normalizedRoute.address);
        if (normalizedRoute.mint) {
          control.setAttribute("data-mint", normalizedRoute.mint);
        } else {
          control.removeAttribute("data-mint");
        }
        if (normalizedRoute.pair) {
          control.setAttribute("data-pair", normalizedRoute.pair);
        } else {
          control.removeAttribute("data-pair");
        }
        control.setAttribute("data-route-url", tokenUrl || window.location.href);
      }

      function ensureAxiomPulseInlineControl(card, target, tokenUrl = window.location.href) {
        if (!(target instanceof HTMLElement)) {
          return;
        }

        const parent = target.parentElement;
        if (parent instanceof HTMLElement) {
          Object.assign(parent.style, {
            display: "flex",
            alignItems: "center",
            gap: "2px",
            marginBottom: "-4px"
          });
        }

        const anchorId = target.dataset.trenchToolsPulseAnchorId;
        const currentRoute = normalizePulseRoute(resolvePulseTradeRouteForCard(card));
        const currentTokenUrl = findPulseRouteLink(card)?.href || tokenUrl;
        let inlineButton =
          (parent instanceof HTMLElement &&
            parent.querySelector(`[data-trench-tools-pulse-inline][data-anchor-id="${anchorId}"]`)) ||
          null;
        if (!(inlineButton instanceof HTMLButtonElement)) {
          inlineButton = helpers.buildInlineButton(
            async () => {
              const liveCard = resolveAxiomPulseCardForControl(inlineButton, card);
              const liveTokenUrl = findPulseRouteLink(liveCard)?.href ||
                inlineButton.getAttribute("data-route-url") ||
                tokenUrl;
              const liveRoute = await resolvePulseTradeRouteWithFallback(liveCard, inlineButton, liveTokenUrl);
              if (!liveRoute?.address) {
                throw new Error("Token not found.");
              }
              await helpers.handleInlineTradeRequest("buy", liveRoute, "pulse", {
                ...helpers.state.preferences,
                buyAmountSol: helpers.resolveQuickBuyAmount()
              }, liveTokenUrl, {
                skipBlockingPrewarm: true
              });
            },
            pulseQuickBuyStyles(target)
          );
          inlineButton.setAttribute("data-trench-tools-pulse-inline", "true");
          inlineButton.setAttribute("data-anchor-id", anchorId);
        }

        bindPulseRouteToControl(inlineButton, currentRoute, currentTokenUrl);
        if (currentRoute?.address) {
          helpers.prewarmForMint?.(currentRoute, {
            surface: "pulse",
            sourceUrl: currentTokenUrl,
            side: "buy",
            reason: "axiom-pulse-mount"
          });
        }
        attachAxiomIntentPrewarm(inlineButton, "pulse", { url: currentTokenUrl, side: "buy" });
        helpers.setInlineButtonStyleSet(inlineButton, pulseQuickBuyStyles(target));
        helpers.setInlineButtonLabel(inlineButton, helpers.quickBuyLabel());
        if (target.nextElementSibling !== inlineButton) {
          target.insertAdjacentElement("afterend", inlineButton);
        }
      }

      function ensureAxiomPulsePanelControl(card, target, tokenUrl = window.location.href, cardId = "") {
        if (!(target instanceof HTMLElement)) {
          return;
        }

        const parent = target.parentElement;
        const anchorId = target.dataset.trenchToolsPulseAnchorId;
        if (!(parent instanceof HTMLElement) || !anchorId) {
          return;
        }
        const currentRoute = normalizePulseRoute(resolvePulseTradeRouteForCard(card));
        const currentTokenUrl = findPulseRouteLink(card)?.href || tokenUrl;

        let panelButton =
          parent.querySelector(`[data-trench-tools-pulse-panel-inline][data-anchor-id="${anchorId}"]`) ||
          null;
        if (!(panelButton instanceof HTMLButtonElement)) {
          panelButton = helpers.buildInlineIconButton(
            async () => {
              const liveCard = resolveAxiomPulseCardForControl(panelButton, card);
              const liveTokenUrl = findPulseRouteLink(liveCard)?.href ||
                panelButton.getAttribute("data-route-url") ||
                tokenUrl;
              const liveRoute = await resolvePulseTradeRouteWithFallback(liveCard, panelButton, liveTokenUrl);
              if (!liveRoute?.address) {
                throw new Error("Token not found.");
              }
              if (cardId) {
                markAxiomPulsePanelOwner(cardId);
              } else if (liveCard instanceof HTMLElement) {
                markAxiomPulsePanelOwner(getAxiomPulseCardId(liveCard));
              }
              helpers.openInlinePanelForMint(liveRoute, "pulse", liveTokenUrl, panelButton, {
                onOpen: () => pauseAxiomPulsePanelRow(liveCard)
              });
            },
            pulsePanelButtonStyles(target)
          );
          panelButton.setAttribute("data-trench-tools-pulse-panel-inline", "true");
          panelButton.setAttribute("data-anchor-id", anchorId);

          // Panel clicks still open instantly; route prewarm is scheduled
          // separately when a stable Pulse route is available.
        }

        bindPulseRouteToControl(panelButton, currentRoute, currentTokenUrl);
        if (currentRoute?.address) {
          helpers.prewarmForMint?.(currentRoute, {
            surface: "pulse",
            sourceUrl: currentTokenUrl,
            reason: "axiom-pulse-panel-mount"
          });
        }
        attachAxiomIntentPrewarm(panelButton, "pulse", { url: currentTokenUrl });
        helpers.setInlineButtonStyleSet(panelButton, pulsePanelButtonStyles(target));
        const quickBuyButton =
          parent.querySelector(`[data-trench-tools-pulse-inline][data-anchor-id="${anchorId}"]`) || target;
        if (quickBuyButton.nextElementSibling !== panelButton) {
          quickBuyButton.insertAdjacentElement("afterend", panelButton);
        }
      }

      function pulseVampIconUrl() {
        return safeRuntimeGetUrl("assets/vamp-icon.png");
      }

      function dexScreenerIconUrl() {
        return safeRuntimeGetUrl("assets/dexscreener-icon.png");
      }

      function axiomVampIconMode() {
        const axiomFeatures = helpers.state.siteFeatures?.axiom || {};
        const mode = String(axiomFeatures.vampIconMode || "").trim().toLowerCase();
        if (mode === "both" || mode === "pulse" || mode === "token" || mode === "off") {
          return mode;
        }
        return axiomFeatures.pulseVamp === false ? "off" : "both";
      }

      function shouldShowAxiomVampIcon(surface) {
        if (!helpers.state.siteFeatures?.axiom?.launchdeckInjection) {
          return false;
        }
        const mode = axiomVampIconMode();
        return mode === "both" || mode === surface;
      }

      function axiomDexScreenerIconMode() {
        const mode = String(helpers.state.siteFeatures?.axiom?.dexScreenerIconMode || "both").trim().toLowerCase();
        return mode === "pulse" || mode === "token" || mode === "off" ? mode : "both";
      }

      function shouldShowAxiomDexScreenerIcon(surface) {
        const mode = axiomDexScreenerIconMode();
        return mode === "both" || mode === surface;
      }

      function buildDexScreenerUrl(mint) {
        const normalizedMint = String(mint || "").trim();
        return normalizedMint
          ? `https://dexscreener.com/solana/${encodeURIComponent(normalizedMint)}`
          : "";
      }

      function updateDexScreenerLink(link, mint) {
        if (!(link instanceof HTMLAnchorElement)) {
          return;
        }
        const url = buildDexScreenerUrl(mint);
        link.href = url || "#";
        if (url) {
          link.target = "_blank";
          link.rel = "noopener noreferrer";
        } else {
          link.removeAttribute("target");
          link.removeAttribute("rel");
        }
      }

      function openDexScreenerLink(mint, targetWindow = null) {
        const url = buildDexScreenerUrl(mint);
        if (!url) {
          throw new Error("Token mint not found.");
        }
        if (targetWindow && !targetWindow.closed) {
          targetWindow.location.href = url;
          targetWindow.opener = null;
          return;
        }
        const opened = window.open(url, "_blank");
        if (!opened) {
          throw new Error("Pop-up blocked. Allow pop-ups for Axiom to open Dexscreener.");
        }
        try {
          opened.opener = null;
        } catch (_error) {}
      }

      function openPendingDexScreenerTab() {
        const opened = window.open("about:blank", "_blank");
        if (opened) {
          try {
            opened.opener = null;
          } catch (_error) {}
        }
        return opened;
      }

      async function resolveMintForExternalLink(route, surface, url = window.location.href) {
        const routeMint = String(route?.mint || route?.tokenMint || "").trim();
        if (routeMint) {
          return routeMint;
        }
        try {
          const resolved = await helpers.resolveInlineToken(route, surface, url, { silent: true });
          const resolvedMint = String(resolved?.mint || resolved?.tokenMint || "").trim();
          if (resolvedMint) {
            return resolvedMint;
          }
        } catch (_error) {}
        return "";
      }

      function pulseVampResolveBehavior() {
        const axiomFeatures = helpers.state.siteFeatures?.axiom || {};
        const mode = String(axiomFeatures.pulseVampMode || "prefill").trim().toLowerCase();
        return mode === "insta" ? "insta" : "prefill";
      }

      // Mirrors Uxento's anchor discovery: the card's social-icon toolbar row.
      function findAxiomPulseVampAnchor(card) {
        if (!(card instanceof HTMLElement)) {
          return null;
        }
        let anchor = card.querySelector(
          ".flex.flex-row.flex-shrink-0.gap-\\[8px\\].justify-start.items-center"
        );
        if (!anchor) {
          anchor = card.querySelector(
            ".flex.flex-row.gap-\\[8px\\].justify-start.items-center"
          );
        }
        if (!anchor) {
          const search = card.querySelector(".ri-search-line");
          if (search) {
            anchor = search.closest(".flex.flex-row");
          }
        }
        if (!anchor) {
          const social = card.querySelector(
            "[class*='ri-twitter'], [class*='ri-tiktok'], a[href*='x.com/search']"
          );
          if (social) {
            anchor =
              social.closest(".flex.flex-row.gap-\\[8px\\]") ||
              social.closest(".flex.gap-\\[8px\\]") ||
              social.parentElement;
          }
        }
        return anchor instanceof HTMLElement ? anchor : null;
      }

      function removeAxiomPulseVampIcon(card) {
        if (!(card instanceof HTMLElement)) {
          return;
        }
        card
          .querySelectorAll("[data-trench-tools-pulse-vamp-inline], [data-trench-tools-pulse-dex-inline]")
          .forEach((element) => {
            helpers.teardownInlineSizeSync?.(element);
            element.remove();
          });
      }

      function ensureAxiomPulseVampIcon(card) {
        const anchor = findAxiomPulseVampAnchor(card);
        if (!anchor) {
          removeAxiomPulseVampIcon(card);
          return;
        }
        const showVampIcon = shouldShowAxiomVampIcon("pulse");
        const showDexIcon = shouldShowAxiomDexScreenerIcon("pulse");
        if (!showVampIcon && !showDexIcon) {
          removeAxiomPulseVampIcon(card);
          return;
        }

        // If an existing vamp icon is elsewhere in the card, remove it so we
        // can re-attach it to the fresh anchor (Axiom re-renders cards often).
        card
          .querySelectorAll("[data-trench-tools-pulse-vamp-inline], [data-trench-tools-pulse-dex-inline]")
          .forEach((element) => {
            if (element.parentElement !== anchor) {
              helpers.teardownInlineSizeSync?.(element);
              element.remove();
            }
          });

        let vampIcon = anchor.querySelector(
          ":scope > [data-trench-tools-pulse-vamp-inline]"
        );
        if (showVampIcon && !(vampIcon instanceof HTMLAnchorElement)) {
          vampIcon = buildAxiomPulseVampIcon(card);
        } else if (!showVampIcon) {
          vampIcon?.remove();
          vampIcon = null;
        }

        let dexIcon = anchor.querySelector(
          ":scope > [data-trench-tools-pulse-dex-inline]"
        );
        if (showDexIcon && !(dexIcon instanceof HTMLAnchorElement)) {
          dexIcon = buildAxiomPulseDexScreenerIcon(card);
        } else if (!showDexIcon) {
          dexIcon?.remove();
          dexIcon = null;
        }

        const currentRoute = normalizePulseRoute(resolvePulseTradeRouteForCard(card));
        const currentTokenUrl = findPulseRouteLink(card)?.href || window.location.href;
        if (dexIcon instanceof HTMLAnchorElement) {
          dexIcon.querySelector("[data-trench-tools-axiom-watchlist-inline]")?.remove();
          bindPulseRouteToControl(dexIcon, currentRoute, currentTokenUrl);
          updateDexScreenerLink(dexIcon, currentRoute?.mint || "");
        }
        if (vampIcon instanceof HTMLAnchorElement) {
          vampIcon.querySelector("[data-trench-tools-axiom-watchlist-inline]")?.remove();
        }

        if (vampIcon instanceof HTMLAnchorElement && vampIcon.parentElement !== anchor) {
          anchor.appendChild(vampIcon);
        }
        if (dexIcon instanceof HTMLAnchorElement) {
          if (vampIcon instanceof HTMLAnchorElement) {
            if (dexIcon.parentElement !== anchor || vampIcon.nextElementSibling !== dexIcon) {
              vampIcon.insertAdjacentElement("afterend", dexIcon);
            }
          } else if (dexIcon.parentElement !== anchor) {
            anchor.appendChild(dexIcon);
          }
        }
      }

      function buildAxiomPulseVampIcon(card) {
        const link = document.createElement("a");
        link.className = "flex items-center";
        link.href = "#";
        link.setAttribute("data-trench-tools-pulse-vamp-inline", "true");
        link.setAttribute("aria-label", "Vamp token");
        link.title = "Vamp token (uses LaunchDeck behavior set in Settings)";
        link.style.cursor = "pointer";
        link.style.marginLeft = "0px";

        const icon = document.createElement("img");
        icon.src = pulseVampIconUrl();
        icon.alt = "Vamp";
        icon.draggable = false;
        icon.className = "transition-all duration-[125ms]";
        Object.assign(icon.style, {
          width: "16px",
          height: "16px",
          objectFit: "contain",
          display: "block",
          flexShrink: "0",
          pointerEvents: "none",
          filter: "brightness(0) invert(1)",
          opacity: "0.75",
          transition: "all 0.125s ease-in-out",
        });
        link.appendChild(icon);

        link.addEventListener("mouseenter", () => {
          icon.style.opacity = "1";
        });
        link.addEventListener("mouseleave", () => {
          icon.style.opacity = "0.75";
        });

        link.addEventListener("click", (event) => {
          event.preventDefault();
          event.stopPropagation();
          void handleAxiomPulseVampClick(card).catch((error) => {
            if (isExtensionContextInvalid(error)) {
              return;
            }
            helpers.showToast?.(error?.message || "Vamp failed.", "error");
          });
        });

        return link;
      }

      function buildAxiomPulseDexScreenerIcon(card) {
        const link = document.createElement("a");
        link.className = "flex items-center";
        link.href = "#";
        link.setAttribute("data-trench-tools-pulse-dex-inline", "true");
        link.setAttribute("aria-label", "Open token on Dexscreener");
        link.title = "Open token on Dexscreener";
        link.style.cursor = "pointer";
        link.style.marginLeft = "0px";

        const icon = document.createElement("img");
        icon.src = dexScreenerIconUrl();
        icon.alt = "Dexscreener";
        icon.draggable = false;
        icon.className = "transition-all duration-[125ms]";
        Object.assign(icon.style, {
          width: "16px",
          height: "16px",
          objectFit: "contain",
          display: "block",
          flexShrink: "0",
          pointerEvents: "none",
          opacity: "0.75",
          transition: "all 0.125s ease-in-out",
        });
        link.appendChild(icon);

        link.addEventListener("mouseenter", () => {
          icon.style.opacity = "1";
        });
        link.addEventListener("mouseleave", () => {
          icon.style.opacity = "0.75";
        });

        link.addEventListener("click", (event) => {
          event.stopPropagation();
          if (link.href && link.getAttribute("href") !== "#") {
            return;
          }
          event.preventDefault();
          const pendingTab = openPendingDexScreenerTab();
          void handleAxiomPulseDexScreenerClick(card, link, pendingTab).catch((error) => {
            pendingTab?.close?.();
            if (isExtensionContextInvalid(error)) {
              return;
            }
            helpers.showToast?.(error?.message || "Dexscreener link failed.", "error");
          });
        });

        return link;
      }

      async function handleAxiomPulseDexScreenerClick(card, control = null, pendingTab = null) {
        const liveCard = resolveAxiomPulseCardForControl(control, card);
        const liveTokenUrl = findPulseRouteLink(liveCard)?.href ||
          control?.getAttribute?.("data-route-url") ||
          window.location.href;
        const liveRoute = await resolvePulseTradeRouteWithFallback(liveCard, control, liveTokenUrl);
        if (!liveRoute?.address) {
          throw new Error("Token not found.");
        }
        const mint = await resolveMintForExternalLink(liveRoute, "pulse", liveTokenUrl);
        openDexScreenerLink(mint, pendingTab);
      }

      const AXIOM_VAMP_CAPTURE_STORAGE_PREFIX = "trenchTools.vampImageCapture.";
      const AXIOM_VAMP_CAPTURE_TTL_MS = 15 * 60 * 1000;
      const AXIOM_VAMP_MAX_CAPTURE_DIMENSION = 512;
      const AXIOM_VAMP_MAX_INLINE_DATA_URL_LENGTH = 1_000_000;
      const AXIOM_VAMP_MAX_STORED_DATA_URL_LENGTH = 2_000_000;

      function axiomVampCaptureStorageKey() {
        return `${AXIOM_VAMP_CAPTURE_STORAGE_PREFIX}${Date.now()}.${Math.random().toString(36).slice(2)}`;
      }

      function axiomVampImageSource(image) {
        return String(
          image.currentSrc ||
          image.src ||
          image.getAttribute("src") ||
          ""
        ).trim();
      }

      function axiomImageAddressFromSource(source) {
        const raw = String(source || "").trim();
        if (!raw || raw.startsWith("data:")) {
          return "";
        }
        try {
          const parsed = new URL(raw, window.location.href);
          if (parsed.hostname !== "axiomtrading.axiom-cdn.io") {
            return "";
          }
          return parsed.pathname
            .split("/")
            .pop()
            .replace(/\.[^.]+$/, "")
            .trim();
        } catch (_error) {
          return "";
        }
      }

      function isVisibleAxiomVampImage(image) {
        if (!(image instanceof HTMLImageElement)) {
          return false;
        }
        const rect = image.getBoundingClientRect();
        const style = window.getComputedStyle(image);
        return rect.width >= 40
          && rect.height >= 40
          && rect.bottom > 0
          && rect.right > 0
          && rect.top < window.innerHeight
          && rect.left < window.innerWidth
          && style.display !== "none"
          && style.visibility !== "hidden"
          && Number(style.opacity || "1") > 0.01;
      }

      function scoreAxiomVampImageCandidate(image, contractAddress) {
        if (!isVisibleAxiomVampImage(image)) {
          return -1;
        }
        const source = axiomVampImageSource(image);
        if (!source) {
          return -1;
        }
        const sourceAddress = axiomImageAddressFromSource(source);
        if (contractAddress && sourceAddress && sourceAddress !== contractAddress) {
          return -1;
        }
        const normalizedSource = source.toLowerCase();
        const normalizedAlt = String(image.alt || "").toLowerCase();
        const normalizedClassName = String(image.className || "").toLowerCase();
        if (
          normalizedAlt === "vamp" ||
          normalizedSource.includes("vamp-icon") ||
          normalizedSource.includes("trench-tools")
        ) {
          return -1;
        }
        const rect = image.getBoundingClientRect();
        let score = 0;
        if (source.startsWith("data:image/")) score += 120;
        if (contractAddress && source.includes(contractAddress)) score += 220;
        if (normalizedSource.includes("axiomtrading.axiom-cdn.io")) score += 170;
        if (normalizedSource.includes("axiom-assets.axiom-cdn.io/pfps/")) score -= 120;
        if (normalizedClassName.includes("object-cover")) score += 25;
        if (rect.width >= 56 && rect.width <= 96 && rect.height >= 56 && rect.height <= 96) {
          score += 35;
        }
        if (image.naturalWidth > 0 && image.naturalHeight > 0) score += 20;
        return score;
      }

      function selectAxiomVampImageCandidate(card, contractAddress) {
        if (!(card instanceof Element)) {
          return null;
        }
        const candidates = Array.from(card.querySelectorAll("img"))
          .map((image) => ({
            image,
            score: scoreAxiomVampImageCandidate(image, contractAddress),
          }))
          .filter((entry) => entry.score > 0)
          .sort((left, right) => right.score - left.score);
        const selected = candidates[0];
        if (!selected || selected.score < 50) {
          return null;
        }
        return selected.image;
      }

      function canvasDataUrlFromImage(image) {
        if (!(image instanceof HTMLImageElement) || !image.complete || !image.naturalWidth || !image.naturalHeight) {
          return "";
        }
        try {
          const canvas = document.createElement("canvas");
          canvas.width = image.naturalWidth;
          canvas.height = image.naturalHeight;
          const context = canvas.getContext("2d");
          if (!context) return "";
          context.drawImage(image, 0, 0, canvas.width, canvas.height);
          return canvas.toDataURL("image/png");
        } catch (_error) {
          return "";
        }
      }

      function blobToDataUrl(blob) {
        return new Promise((resolve, reject) => {
          const reader = new FileReader();
          reader.addEventListener("load", () => resolve(String(reader.result || "")), { once: true });
          reader.addEventListener("error", () => reject(reader.error || new Error("Failed to read image.")), { once: true });
          reader.readAsDataURL(blob);
        });
      }

      async function compressedImageDataUrlFromBlob(blob) {
        if (typeof createImageBitmap !== "function") {
          return blobToDataUrl(blob);
        }
        let bitmap = null;
        try {
          bitmap = await createImageBitmap(blob);
          const scale = Math.min(
            1,
            AXIOM_VAMP_MAX_CAPTURE_DIMENSION / Math.max(bitmap.width, bitmap.height, 1)
          );
          const width = Math.max(1, Math.round(bitmap.width * scale));
          const height = Math.max(1, Math.round(bitmap.height * scale));
          const canvas = document.createElement("canvas");
          canvas.width = width;
          canvas.height = height;
          const context = canvas.getContext("2d");
          if (!context) {
            return blobToDataUrl(blob);
          }
          context.drawImage(bitmap, 0, 0, width, height);
          return canvas.toDataURL("image/webp", 0.92);
        } catch (_error) {
          return blobToDataUrl(blob);
        } finally {
          if (bitmap && typeof bitmap.close === "function") {
            bitmap.close();
          }
        }
      }

      async function dataUrlFromAxiomVampImage(image) {
        const source = axiomVampImageSource(image);
        if (!source) {
          return "";
        }
        if (source.startsWith("data:image/")) {
          return source.length <= AXIOM_VAMP_MAX_INLINE_DATA_URL_LENGTH
            ? source
            : (canvasDataUrlFromImage(image) || source);
        }
        const response = await fetch(source, {
          cache: "force-cache",
          credentials: "omit",
        });
        if (!response.ok) {
          throw new Error(`Axiom image fetch failed with status ${response.status}.`);
        }
        const blob = await response.blob();
        if (!String(blob.type || "").startsWith("image/")) {
          throw new Error("Axiom image response was not an image.");
        }
        if (blob.size > 8_000_000) {
          throw new Error("Axiom image is too large.");
        }
        return compressedImageDataUrlFromBlob(blob);
      }

      async function persistAxiomVampImageCapture(capture) {
        if (
          typeof chrome === "undefined" ||
          !chrome.storage?.local
        ) {
          return "";
        }
        await cleanupStaleAxiomVampImageCaptures();
        if (!capture || !capture.dataUrl) {
          return "";
        }
        const storageKey = axiomVampCaptureStorageKey();
        await chrome.storage.local.set({
          [storageKey]: {
            ...capture,
            createdAt: Number(capture.createdAt || 0) || Date.now(),
          },
        });
        return storageKey;
      }

      async function removeStoredAxiomVampImageCapture(storageKey) {
        if (
          !storageKey ||
          typeof chrome === "undefined" ||
          !chrome.storage?.local
        ) {
          return;
        }
        try {
          await chrome.storage.local.remove(storageKey);
        } catch (_error) {
          // Best-effort cleanup; stale captures are also swept before future persists.
        }
      }

      async function cleanupStaleAxiomVampImageCaptures() {
        if (typeof chrome === "undefined" || !chrome.storage?.local) {
          return;
        }
        let stored = null;
        try {
          stored = await chrome.storage.local.get(null);
        } catch (_error) {
          return;
        }
        const now = Date.now();
        const staleKeys = Object.entries(stored || {})
          .filter(([key, value]) => {
            if (!String(key || "").startsWith(AXIOM_VAMP_CAPTURE_STORAGE_PREFIX)) {
              return false;
            }
            const createdAt = Number(value && typeof value === "object" ? value.createdAt : 0);
            return !createdAt || now - createdAt > AXIOM_VAMP_CAPTURE_TTL_MS;
          })
          .map(([key]) => key);
        if (!staleKeys.length) {
          return;
        }
        try {
          await chrome.storage.local.remove(staleKeys);
        } catch (_error) {
          // Ignore cleanup failures so the vamp flow can continue.
        }
      }


      async function captureAxiomVampImage(card, contractAddress) {
        const image = selectAxiomVampImageCandidate(card, contractAddress);
        if (!image) {
          return null;
        }
        const dataUrl = await dataUrlFromAxiomVampImage(image);
        if (!dataUrl || !dataUrl.startsWith("data:image/")) {
          return null;
        }
        if (dataUrl.length > AXIOM_VAMP_MAX_STORED_DATA_URL_LENGTH) {
          return null;
        }
        const sourceUrl = axiomVampImageSource(image);
        return {
          dataUrl,
          sourceUrl: sourceUrl.startsWith("data:") ? "" : sourceUrl,
          contractAddress,
          name: String(image.alt || "").trim(),
          createdAt: Date.now(),
          source: "axiom-pulse",
        };
      }

      async function handleAxiomPulseVampClick(card) {
        const liveRoute = await resolvePulseTradeRouteWithFallback(
          card,
          null,
          findPulseRouteLink(card)?.href || window.location.href
        );
        const contractAddress = String(liveRoute?.mint || liveRoute?.address || "").trim();
        if (!contractAddress) {
          throw new Error("Token not found.");
        }
        const mode = pulseVampResolveBehavior();
        let vampImageKey = "";
        try {
          const capturedImage = await captureAxiomVampImage(card, contractAddress);
          vampImageKey = await persistAxiomVampImageCapture(capturedImage);
        } catch (error) {
          console.warn("Axiom Vamp image capture failed; falling back to metadata import.", error);
        }
        try {
          await helpers.openLaunchdeckOverlay({
            mode: "create",
            contractAddress,
            instaLaunch: mode === "insta",
            vampImageKey,
          });
        } catch (error) {
          await removeStoredAxiomVampImageCapture(vampImageKey);
          throw error;
        }
      }

      function mountAxiomWatchlistQuickButtons() {
        document.querySelectorAll("[data-trench-tools-axiom-watchlist-inline]").forEach((element) => {
          const anchor = element.parentElement;
          if (!(anchor instanceof HTMLAnchorElement) || !isAxiomWatchlistAnchor(anchor)) {
            element.remove();
          }
        });

        findAxiomWatchlistAnchors().forEach((anchor) => ensureAxiomWatchlistQuickButton(anchor));
      }

      function reconcileAxiomWatchlistAnchor(anchor) {
        if (!(anchor instanceof HTMLAnchorElement)) {
          return;
        }
        const existingButton = anchor.querySelector("[data-trench-tools-axiom-watchlist-inline]");
        if (!isAxiomWatchlistAnchor(anchor)) {
          existingButton?.remove();
          return;
        }
        ensureAxiomWatchlistQuickButton(anchor);
      }

      function isAxiomWatchlistAnchor(anchor) {
        if (
          !(anchor instanceof HTMLAnchorElement) ||
          anchor.classList.contains("group/token") ||
          isTrenchToolsActionAnchor(anchor) ||
          anchor.closest(".w-full.pointer-events-none:not(.absolute)") ||
          anchor.closest("div#instant-trade")
        ) {
          return false;
        }
        if (findClosestAxiomWalletTrackerRow(anchor) && !isAxiomWatchlistSurface()) {
          return false;
        }
        const routeKey = String(helpers.extractMintFromUrl(anchor.href) || "").trim();
        if (!routeKey) {
          return false;
        }
        const pageAddress = resolveCurrentPageAddress();
        if (pageAddress && routeKey === pageAddress && !isAxiomListSurface()) {
          return false;
        }
        return anchor.classList.contains("group") || isAxiomWatchlistSurface() || (!pageAddress && isAxiomListSurface());
      }

      function isTrenchToolsActionAnchor(anchor) {
        return Boolean(
          anchor instanceof HTMLAnchorElement &&
          (
            anchor.hasAttribute("data-trench-tools-pulse-vamp-inline") ||
            anchor.hasAttribute("data-trench-tools-pulse-dex-inline") ||
            anchor.hasAttribute("data-trench-tools-token-detail-vamp-inline") ||
            anchor.hasAttribute("data-trench-tools-token-detail-dex-inline") ||
            anchor.hasAttribute("data-trench-tools-token-detail-action-inline")
          )
        );
      }

      function findAxiomWatchlistAnchors(root = document) {
        const anchors = new Set();
        const addAnchor = (candidate) => {
          if (candidate instanceof HTMLAnchorElement && isAxiomWatchlistAnchor(candidate)) {
            anchors.add(candidate);
          }
        };
        if (root instanceof HTMLAnchorElement) {
          addAnchor(root);
        } else if (root instanceof HTMLElement) {
          addAnchor(root.closest(MEME_LINK_SELECTOR));
        }
        root.querySelectorAll?.(MEME_LINK_SELECTOR).forEach(addAnchor);
        return Array.from(anchors);
      }

      function ensureAxiomWatchlistQuickButton(anchor) {
        if (!(anchor instanceof HTMLAnchorElement)) {
          return;
        }

        const route = buildAxiomPairRouteReferenceFromElement(anchor.href, "watchlist", anchor);
        const pairAddress = String(route?.pairAddress || route?.address || "").trim();
        const tokenMint = String(route?.mint || "").trim();
        if (!pairAddress) {
          return;
        }

        let button = anchor.querySelector("[data-trench-tools-axiom-watchlist-inline]");
        if (
          button instanceof HTMLButtonElement &&
          (
            button.getAttribute("data-route-key") !== pairAddress ||
            String(button.getAttribute("data-mint") || "") !== tokenMint ||
            String(button.getAttribute("data-pair") || "") !== ""
          )
        ) {
          button.remove();
          button = null;
        }

        if (!(button instanceof HTMLButtonElement)) {
          button = helpers.buildInlineButton(
            async () => {
              const liveRoute = routePayloadFromButton(button, {
                address: pairAddress,
                mint: tokenMint,
                pair: "",
                surface: "watchlist",
                url: anchor.href
              });
              if (!liveRoute?.address) {
                throw new Error("Token not found.");
              }
              await helpers.handleInlineTradeRequest("buy", liveRoute, "watchlist", {
                ...helpers.state.preferences,
                buyAmountSol: helpers.resolveQuickBuyAmount()
              }, anchor.href);
            },
            axiomWatchlistQuickBuyStyles(anchor)
          );
          button.setAttribute("data-trench-tools-axiom-watchlist-inline", "true");
        }

        button.setAttribute("data-route-key", pairAddress);
        if (tokenMint) {
          button.setAttribute("data-mint", tokenMint);
        } else {
          button.removeAttribute("data-mint");
        }
        button.removeAttribute("data-pair");
        attachAxiomIntentPrewarm(button, "watchlist", {
          address: pairAddress,
          mint: tokenMint,
          url: anchor.href,
          side: "buy"
        });
        helpers.setInlineButtonStyleSet(button, axiomWatchlistQuickBuyStyles(anchor));
        helpers.setInlineButtonLabel(button, helpers.quickBuyLabel());

        if (anchor.lastElementChild !== button) {
          anchor.appendChild(button);
        }
      }

      function mountAxiomWalletTrackerQuickButtons() {
        findAxiomWalletTrackerRows().forEach((row) => {
          reconcileAxiomWalletTrackerRow(row);
        });
      }

      function findAxiomWalletTrackerRows(root = document) {
        const rows = new Set();
        const addRow = (candidate) => {
          if (candidate instanceof HTMLElement && isAxiomWalletTrackerRow(candidate)) {
            rows.add(candidate);
          }
        };
        if (root instanceof HTMLElement) {
          addRow(root.closest(WALLET_ROW_SELECTOR));
          addRow(root);
        }
        root.querySelectorAll?.(WALLET_ROW_SELECTOR).forEach(addRow);
        root.querySelectorAll?.(MEME_LINK_SELECTOR).forEach((anchor) => {
          if (!(anchor instanceof HTMLAnchorElement)) {
            return;
          }
          addRow(anchor.closest(WALLET_ROW_SELECTOR));
          addRow(anchor.closest("div[class*='flex-row']"));
          addRow(anchor.closest("div[role='row']"));
        });
        return Array.from(rows);
      }

      function findClosestAxiomWalletTrackerRow(element) {
        if (!(element instanceof HTMLElement)) {
          return null;
        }
        const row = element.closest(WALLET_ROW_SELECTOR);
        return isAxiomWalletTrackerRow(row) ? row : null;
      }

      function reconcileAxiomWalletTrackerRow(row) {
        if (!(row instanceof HTMLElement)) {
          return;
        }
        const existingButton = row.querySelector("[data-trench-tools-wallet-tracker-inline]");
        if (!isAxiomWalletTrackerRow(row)) {
          if (existingButton instanceof HTMLButtonElement) {
            helpers.teardownInlineSizeSync(existingButton);
            existingButton.remove();
          }
          return;
        }
        ensureAxiomWalletTrackerQuickButton(row);
      }

      function isAxiomWalletTrackerRow(row) {
        return (
          row instanceof HTMLElement &&
          row.matches(WALLET_ROW_SELECTOR) &&
          findAxiomWalletTrackerActionSlot(row) instanceof HTMLElement &&
          row.querySelector(MEME_LINK_SELECTOR) instanceof HTMLAnchorElement
        );
      }

      function findAxiomWalletTrackerActionSlot(row) {
        if (!(row instanceof HTMLElement)) {
          return null;
        }
        const isVisibleSlot = (slot) => {
          if (!(slot instanceof HTMLElement)) {
            return false;
          }
          const rect = slot.getBoundingClientRect();
          return rect.width > 0 && rect.height > 0;
        };
        let visibleSlots = Array.from(row.querySelectorAll("div.justify-end")).filter(isVisibleSlot);
        if (!visibleSlots.length) {
          visibleSlots = Array.from(row.children).filter(isVisibleSlot);
        }
        if (!visibleSlots.length) {
          return null;
        }
        const interactiveSlot = visibleSlots
          .slice()
          .reverse()
          .find((slot) => slot.querySelector("button, [role='button'], a[href]") instanceof HTMLElement);
        return interactiveSlot || visibleSlots[visibleSlots.length - 1];
      }

      function ensureAxiomWalletTrackerQuickButton(row) {
        if (!(row instanceof HTMLElement)) {
          return;
        }

        const endSlot = findAxiomWalletTrackerActionSlot(row);
        const tokenLink = row.querySelector("a[href*='/meme/']");
        if (!(endSlot instanceof HTMLElement) || !(tokenLink instanceof HTMLAnchorElement)) {
          return;
        }

        const route = buildAxiomPairRouteReferenceFromElement(tokenLink.href, "wallet_tracker", row);
        const pairAddress = String(route?.pairAddress || route?.address || "").trim();
        const tokenMint = String(route?.mint || "").trim();
        if (!pairAddress) {
          return;
        }

        let button = row.querySelector("[data-trench-tools-wallet-tracker-inline]");
        if (
          button instanceof HTMLButtonElement &&
          (
            button.getAttribute("data-route-key") !== pairAddress ||
            String(button.getAttribute("data-mint") || "") !== tokenMint ||
            String(button.getAttribute("data-pair") || "") !== ""
          )
        ) {
          helpers.teardownInlineSizeSync(button);
          button.remove();
          button = null;
        }

        if (!(button instanceof HTMLButtonElement)) {
          button = helpers.buildInlineButton(
            async () => {
              const liveRoute = routePayloadFromButton(button, {
                address: pairAddress,
                mint: tokenMint,
                pair: "",
                surface: "wallet_tracker",
                url: tokenLink.href
              });
              if (!liveRoute?.address) {
                throw new Error("Token not found.");
              }
              await helpers.handleInlineTradeRequest("buy", liveRoute, "wallet_tracker", {
                ...helpers.state.preferences,
                buyAmountSol: helpers.resolveQuickBuyAmount()
              }, tokenLink.href);
            },
            walletTrackerQuickBuyStyles(endSlot)
          );
          button.setAttribute("data-trench-tools-wallet-tracker-inline", "true");
        }

        button.setAttribute("data-route-key", pairAddress);
        if (tokenMint) {
          button.setAttribute("data-mint", tokenMint);
        } else {
          button.removeAttribute("data-mint");
        }
        button.removeAttribute("data-pair");
        attachAxiomIntentPrewarm(button, "wallet_tracker", {
          address: pairAddress,
          mint: tokenMint,
          url: tokenLink.href,
          side: "buy"
        });
        const nextLabel = helpers.quickBuyLabel();
        if (button.getAttribute("data-style-anchor") !== "wallet-tracker-absolute-v2") {
          helpers.setInlineButtonStyleSet(button, walletTrackerQuickBuyStyles(endSlot));
          button.setAttribute("data-style-anchor", "wallet-tracker-absolute-v2");
        }
        if (button.getAttribute("data-label") !== nextLabel) {
          helpers.setInlineButtonLabel(button, nextLabel);
          button.setAttribute("data-label", nextLabel);
        }

        if (button.parentElement !== row || button.nextElementSibling !== null) {
          row.appendChild(button);
        }
      }

      function mountAxiomTokenDetailHeaderActions(routeOrAddress) {
        const route =
          routeOrAddress && typeof routeOrAddress === "object"
            ? routeOrAddress
            : buildObservedCandidate(routeOrAddress, "token_detail", window.location.href);
        const routeKey = String(route?.address || routeOrAddress || "").trim();
        const anchor = findAxiomTokenDetailHeaderActionAnchor();
        if (!routeKey || !(anchor instanceof HTMLElement)) {
          cleanupAxiomTokenDetailHeaderActions();
          return;
        }

        cleanupAxiomTokenDetailHeaderActions(anchor);

        const tokenMint = String(route?.mint || extractAxiomTokenDetailHeaderMint(anchor) || "").trim();
        const companionPair = String(route?.pair || "").trim();

        if (shouldShowAxiomVampIcon("token")) {
          let vampIcon = anchor.querySelector(":scope > [data-trench-tools-token-detail-vamp-inline]");
          if (!(vampIcon instanceof HTMLAnchorElement)) {
            vampIcon = buildAxiomTokenDetailVampIcon();
          }
          bindAxiomTokenDetailActionRoute(vampIcon, routeKey, tokenMint, companionPair);
          if (vampIcon.parentElement !== anchor) {
            anchor.appendChild(vampIcon);
          }
        } else {
          anchor.querySelector(":scope > [data-trench-tools-token-detail-vamp-inline]")?.remove();
        }

        if (shouldShowAxiomDexScreenerIcon("token")) {
          let dexIcon = anchor.querySelector(":scope > [data-trench-tools-token-detail-dex-inline]");
          if (!(dexIcon instanceof HTMLAnchorElement)) {
            dexIcon = buildAxiomTokenDetailDexScreenerIcon();
          }
          dexIcon.querySelector("[data-trench-tools-axiom-watchlist-inline]")?.remove();
          bindAxiomTokenDetailActionRoute(dexIcon, routeKey, tokenMint, companionPair);
          updateDexScreenerLink(dexIcon, tokenMint);
          if (dexIcon.parentElement !== anchor) {
            anchor.appendChild(dexIcon);
          }
        } else {
          anchor.querySelector(":scope > [data-trench-tools-token-detail-dex-inline]")?.remove();
        }
      }

      function cleanupAxiomTokenDetailHeaderActions(activeAnchor = null) {
        document.querySelectorAll("[data-trench-tools-token-detail-action-inline]").forEach((element) => {
          if (activeAnchor instanceof HTMLElement && element.parentElement === activeAnchor) {
            return;
          }
          element.remove();
        });
      }

      function bindAxiomTokenDetailActionRoute(element, routeKey, tokenMint, companionPair) {
        if (!(element instanceof HTMLElement)) {
          return;
        }
        element.setAttribute("data-trench-tools-token-detail-action-inline", "true");
        element.setAttribute("data-route-key", routeKey);
        if (tokenMint) {
          element.setAttribute("data-mint", tokenMint);
        } else {
          element.removeAttribute("data-mint");
        }
        if (companionPair) {
          element.setAttribute("data-pair", companionPair);
        } else {
          element.removeAttribute("data-pair");
        }
        element.setAttribute("data-route-url", window.location.href);
      }

      function findAxiomTokenDetailHeaderActionAnchor() {
        const tooltipRow = document.getElementById("pair-name-tooltip");
        if (isAxiomTokenDetailHeaderActionAnchor(tooltipRow)) {
          return tooltipRow;
        }

        const candidates = [];
        document
          .querySelectorAll("a[href*='pump.fun/coin/'], a[href*='x.com/search'], a[href*='twitter.com/search']")
          .forEach((link) => {
            let current = link.parentElement;
            for (let depth = 0; current instanceof HTMLElement && depth < 6; depth += 1) {
              if (String(current.className || "").includes("flex-row")) {
                candidates.push(current);
              }
              current = current.parentElement;
            }
          });
        return candidates.find((candidate) => isAxiomTokenDetailHeaderActionAnchor(candidate)) || null;
      }

      function isAxiomTokenDetailHeaderActionAnchor(anchor) {
        if (!(anchor instanceof HTMLElement) || anchor.closest("div#instant-trade")) {
          return false;
        }
        const rect = anchor.getBoundingClientRect();
        if (
          rect.width <= 0 ||
          rect.height <= 0 ||
          rect.bottom <= 0 ||
          rect.right <= 0 ||
          rect.top >= 280 ||
          rect.left >= window.innerWidth
        ) {
          return false;
        }
        return Boolean(
          anchor.querySelector("a[href*='pump.fun/coin/']") ||
          anchor.querySelector("a[href*='x.com/search'], a[href*='twitter.com/search']")
        );
      }

      function extractAxiomTokenDetailHeaderMint(root) {
        if (!(root instanceof Element)) {
          return "";
        }
        const links = Array.from(
          root.querySelectorAll("a[href*='pump.fun/coin/'], a[href*='x.com/search'], a[href*='twitter.com/search']")
        );
        for (const link of links) {
          const mint = String(helpers.extractMintFromUrl(link.href) || "").trim();
          if (mint) {
            return mint;
          }
        }
        return "";
      }

      function buildAxiomTokenDetailVampIcon() {
        const link = document.createElement("a");
        link.className = "flex items-center";
        link.href = "#";
        link.setAttribute("data-trench-tools-token-detail-vamp-inline", "true");
        link.setAttribute("aria-label", "Vamp token");
        link.title = "Vamp token";
        link.style.cursor = "pointer";

        const icon = document.createElement("img");
        icon.src = pulseVampIconUrl();
        icon.alt = "Vamp";
        icon.draggable = false;
        Object.assign(icon.style, {
          width: "16px",
          height: "16px",
          objectFit: "contain",
          display: "block",
          flexShrink: "0",
          pointerEvents: "none",
          filter: "brightness(0) invert(1)",
          opacity: "0.75",
          transition: "all 0.125s ease-in-out",
        });
        link.appendChild(icon);
        link.addEventListener("mouseenter", () => {
          icon.style.opacity = "1";
        });
        link.addEventListener("mouseleave", () => {
          icon.style.opacity = "0.75";
        });
        link.addEventListener("click", (event) => {
          event.preventDefault();
          event.stopPropagation();
          void handleAxiomTokenDetailVampClick(link).catch((error) => {
            if (isExtensionContextInvalid(error)) {
              return;
            }
            helpers.showToast?.(error?.message || "Vamp failed.", "error");
          });
        });
        return link;
      }

      function buildAxiomTokenDetailDexScreenerIcon() {
        const link = document.createElement("a");
        link.className = "flex items-center";
        link.href = "#";
        link.setAttribute("data-trench-tools-token-detail-dex-inline", "true");
        link.setAttribute("aria-label", "Open token on Dexscreener");
        link.title = "Open token on Dexscreener";
        link.style.cursor = "pointer";

        const icon = document.createElement("img");
        icon.src = dexScreenerIconUrl();
        icon.alt = "Dexscreener";
        icon.draggable = false;
        Object.assign(icon.style, {
          width: "16px",
          height: "16px",
          objectFit: "contain",
          display: "block",
          flexShrink: "0",
          pointerEvents: "none",
          opacity: "0.75",
          transition: "all 0.125s ease-in-out",
        });
        link.appendChild(icon);
        link.addEventListener("mouseenter", () => {
          icon.style.opacity = "1";
        });
        link.addEventListener("mouseleave", () => {
          icon.style.opacity = "0.75";
        });
        link.addEventListener("click", (event) => {
          event.stopPropagation();
          if (link.href && link.getAttribute("href") !== "#") {
            return;
          }
          event.preventDefault();
          const pendingTab = openPendingDexScreenerTab();
          void handleAxiomTokenDetailDexScreenerClick(link, pendingTab).catch((error) => {
            pendingTab?.close?.();
            if (isExtensionContextInvalid(error)) {
              return;
            }
            helpers.showToast?.(error?.message || "Dexscreener link failed.", "error");
          });
        });
        return link;
      }

      async function resolveTokenDetailRouteFromControl(control) {
        const route = routePayloadFromButton(control, {
          address: control?.getAttribute?.("data-route-key") || resolveCurrentPageAddress(),
          mint: control?.getAttribute?.("data-mint") || "",
          pair: control?.getAttribute?.("data-pair") || "",
          surface: "token_detail",
          url: control?.getAttribute?.("data-route-url") || window.location.href
        });
        if (!route?.address) {
          return null;
        }
        try {
          return await helpers.resolveInlineToken(route, "token_detail", route.url || window.location.href, { silent: true }) || route;
        } catch (_error) {
          return route;
        }
      }

      async function handleAxiomTokenDetailVampClick(control) {
        const route = await resolveTokenDetailRouteFromControl(control);
        const contractAddress = String(route?.mint || route?.address || "").trim();
        if (!contractAddress) {
          throw new Error("Token not found.");
        }
        await helpers.openLaunchdeckOverlay({
          mode: "create",
          contractAddress,
        });
      }

      async function handleAxiomTokenDetailDexScreenerClick(control, pendingTab = null) {
        const route = await resolveTokenDetailRouteFromControl(control);
        if (!route?.address) {
          throw new Error("Token not found.");
        }
        const mint = await resolveMintForExternalLink(route, "token_detail", route.url || window.location.href);
        openDexScreenerLink(mint, pendingTab);
      }

      function mountAxiomTokenDetailQuickButton(routeOrAddress) {
        const route =
          routeOrAddress && typeof routeOrAddress === "object"
            ? routeOrAddress
            : buildObservedCandidate(routeOrAddress, "token_detail", window.location.href);
        const routeKey = String(route?.address || routeOrAddress || "").trim();
        const tokenMint = String(route?.mint || "").trim();
        const companionPair = String(route?.pair || "").trim();
        if (!routeKey) {
          cleanupAxiomTokenDetailManualBuyButtons();
          return;
        }
        helpers.prewarmForMint?.({
          address: routeKey,
          mint: tokenMint,
          pair: companionPair,
          surface: "token_detail",
          url: window.location.href
        }, {
          surface: "token_detail",
          sourceUrl: window.location.href,
          side: "buy",
          reason: "axiom-token-detail-mount"
        });
        const mountRoute = { routeKey, tokenMint, companionPair };
        if (findAxiomTokenDetailInstantTradePanel() instanceof HTMLElement) {
          mountAxiomTokenDetailPresetButtons(mountRoute);
        } else {
          cleanupAxiomTokenDetailFloatingPresetButtons();
        }
        mountAxiomTokenDetailHardpanelManualActions(mountRoute);
      }

      function cleanupAxiomTokenDetailManualBuyButtons() {
        cleanupAxiomTokenDetailFloatingPresetButtons();
        cleanupAxiomTokenDetailHardpanelManualActions();
      }

      function cleanupAxiomTokenDetailFloatingPresetButtons() {
        disconnectAxiomTokenDetailFloatingPresetRefreshBridge();
        removeAxiomTokenDetailInitialSizeStyle();
        disconnectAxiomTokenDetailPanelSizeMemory();
        disconnectAxiomTokenDetailGroupRowSizeSync();
        disconnectAxiomTokenDetailCompactDrag();
        restoreAxiomTokenDetailManagedTransforms();
        restoreAxiomTokenDetailNativeControls();
        clearAxiomTokenDetailPanelMarkers();
        document.querySelectorAll(
          [
            "[data-trench-tools-token-detail-inline]",
            "[data-trench-tools-token-detail-preload-inline]",
            "[data-trench-tools-token-detail-panel]",
            "[data-trench-tools-token-detail-setting-row]",
            "[data-trench-tools-token-detail-setting-tooltip]",
            "[data-trench-tools-token-detail-compact-toggle]",
            "[data-trench-tools-token-detail-wallet-selector]",
            "[data-trench-tools-token-detail-wallet-menu]"
          ].join(", ")
        ).forEach((element) => {
          helpers.teardownInlineSizeSync(element);
          element._trenchInlineCleanup?.();
          element._trenchAxiomHoverBridgeCleanup?.();
          element._trenchAxiomEditableBridgeCleanup?.();
          element.remove();
        });
      }

      function cleanupAxiomTokenDetailHardpanelManualActions() {
        disconnectAxiomTokenDetailHardpanelRefreshBridge();
        document.querySelectorAll(
          "[data-trench-tools-token-detail-hardpanel-action], [data-trench-tools-token-detail-hardpanel-action-wrapper]"
        ).forEach((element) => element.remove());
      }

      function mountAxiomTokenDetailPresetButtons(route) {
        const instantTrade = findAxiomTokenDetailInstantTradePanel();
        if (!(instantTrade instanceof HTMLElement)) {
          cleanupAxiomTokenDetailFloatingPresetButtons();
          return;
        }
        ensureAxiomTokenDetailFloatingPresetRefreshBridge(instantTrade, route);
        ensureAxiomTokenDetailBloomCloneStyles();
        document.querySelectorAll("[data-trench-tools-token-detail-panel]").forEach((element) => element.remove());
        document.querySelectorAll("[data-trench-tools-token-detail-preload-inline]").forEach((element) => element.remove());
        const mountedButtons = new Set();
        const rows = findAxiomTokenDetailControlRows();
        rows.forEach((row, rowIndex) => {
          const rowSide = resolveAxiomTokenDetailRowSide(row, rowIndex, rows);
          const nativeControls = findAxiomTokenDetailNativeControls(row, rowSide);
          nativeControls.forEach(markAxiomTokenDetailNativeControl);
          const actions = nativeControls
            .map((nativeControl, index) => ({
              nativeControl,
              index,
              rowIndex,
              action: readAxiomTokenDetailAction(nativeControl, row, rowSide)
            }))
            .filter((entry) => entry.action);
          const existingButtons = Array.from(row.querySelectorAll(":scope > [data-trench-tools-token-detail-inline]"))
            .filter((element) => element instanceof HTMLElement);
          if (areAxiomTokenDetailPresetButtonsCurrent(row, existingButtons, actions, route)) {
            existingButtons.forEach((button) => {
              button.removeAttribute("data-trench-tools-token-detail-native-control");
              button.removeAttribute("data-trench-tools-token-detail-native-hidden");
              const controlIndex = Number(button.getAttribute("data-control-index"));
              const actionEntry = Number.isFinite(controlIndex)
                ? actions.find((entry) => entry.index === controlIndex)
                : actions[existingButtons.indexOf(button)];
              if (actionEntry?.action?.editable) {
                installAxiomTokenDetailEditablePresetBridge(button, actionEntry.nativeControl, actionEntry.action);
              } else {
                installAxiomTokenDetailNativeHoverBridge(button, actionEntry?.nativeControl);
              }
              mountedButtons.add(button);
            });
            return;
          }
          existingButtons.forEach((element) => {
            helpers.teardownInlineSizeSync(element);
            element._trenchInlineCleanup?.();
            element._trenchAxiomHoverBridgeCleanup?.();
            element._trenchAxiomEditableBridgeCleanup?.();
            element.remove();
          });
          const clonedControls = [];
          actions.forEach(({ nativeControl, index, rowIndex, action }) => {
            const cloneAction = {
              ...action,
              index,
              rowIndex,
              routeKey: route.routeKey,
              tokenMint: route.tokenMint,
              companionPair: route.companionPair
            };
            const button = buildAxiomTokenDetailCloneButton(nativeControl, cloneAction);
            mountedButtons.add(button);
            clonedControls.push(button);
          });
          row.append(...clonedControls);
        });
        ensureAxiomTokenDetailCompactToggle();
        ensureAxiomTokenDetailWalletSelector();
        ensureAxiomTokenDetailPresetSettingRows();
        ensureAxiomTokenDetailCompactDrag();
        applyAxiomTokenDetailCompactMode();
        ensureAxiomTokenDetailGroupRowSizeSync();

        document.querySelectorAll("[data-trench-tools-token-detail-inline]").forEach((element) => {
          if (!mountedButtons.has(element)) {
            helpers.teardownInlineSizeSync(element);
            element._trenchInlineCleanup?.();
            element._trenchAxiomHoverBridgeCleanup?.();
            element._trenchAxiomEditableBridgeCleanup?.();
            element.remove();
          }
        });
      }

      function markAxiomTokenDetailNativeControl(control) {
        if (!(control instanceof HTMLElement)) {
          return;
        }
        if (!control.hasAttribute("data-trench-tools-token-detail-native-original-min-width")) {
          control.setAttribute("data-trench-tools-token-detail-native-original-min-width", control.style.minWidth || "");
        }
        control.setAttribute("data-trench-tools-token-detail-native-control", "true");
      }

      function restoreAxiomTokenDetailNativeControls() {
        document.querySelectorAll("[data-trench-tools-token-detail-native-control]").forEach((element) => {
          if (!(element instanceof HTMLElement)) {
            return;
          }
          const originalMinWidth =
            element.getAttribute("data-trench-tools-token-detail-native-original-min-width") || "";
          if (originalMinWidth) {
            element.style.minWidth = originalMinWidth;
          } else {
            element.style.removeProperty("min-width");
          }
          element.removeAttribute("data-trench-tools-token-detail-native-hidden");
          element.removeAttribute("data-trench-tools-token-detail-native-control");
          element.removeAttribute("data-trench-tools-token-detail-native-original-min-width");
        });
      }

      function normalizeAxiomTokenDetailButtonMode(value) {
        const mode = String(value || "").trim().toLowerCase();
        return mode === "axiom" || mode === "trench" || mode === "dual" ? mode : "";
      }

      function readAxiomTokenDetailButtonModePreference() {
        try {
          const storedMode = normalizeAxiomTokenDetailButtonMode(
            window.localStorage?.getItem(AXIOM_TOKEN_DETAIL_BUTTON_MODE_STORAGE_KEY)
          );
          if (storedMode) {
            return constrainAxiomTokenDetailButtonMode(storedMode);
          }
          const migratedMode = window.localStorage?.getItem(AXIOM_TOKEN_DETAIL_COMPACT_STORAGE_KEY) === "true"
            ? "trench"
            : "dual";
          return constrainAxiomTokenDetailButtonMode(migratedMode);
        } catch (_error) {
          return constrainAxiomTokenDetailButtonMode("dual");
        }
      }

      function saveAxiomTokenDetailButtonModePreference(mode) {
        const normalizedMode = normalizeAxiomTokenDetailButtonMode(mode) || "dual";
        try {
          window.localStorage?.setItem(AXIOM_TOKEN_DETAIL_BUTTON_MODE_STORAGE_KEY, normalizedMode);
          window.localStorage?.setItem(
            AXIOM_TOKEN_DETAIL_COMPACT_STORAGE_KEY,
            normalizedMode === "trench" ? "true" : "false"
          );
        } catch (_error) {}
      }

      function setAxiomTokenDetailButtonMode(mode) {
        const instantTrade = document.querySelector("div#instant-trade");
        if (instantTrade instanceof HTMLElement) {
          rememberAxiomTokenDetailPanelSize(instantTrade, axiomTokenDetailPanelModeKey());
        }
        axiomTokenDetailButtonMode = constrainAxiomTokenDetailButtonMode(mode);
        saveAxiomTokenDetailButtonModePreference(axiomTokenDetailButtonMode);
        document.querySelectorAll("[data-trench-tools-token-detail-wallet-menu]").forEach((element) => element.remove());
        document.removeEventListener("mousedown", handleAxiomTokenDetailWalletMenuOutsideClick, true);
        applyAxiomTokenDetailCompactMode({ forcePanelSize: true });
      }

      function nextAxiomTokenDetailButtonMode(mode = axiomTokenDetailButtonMode) {
        const modes = axiomTokenDetailButtonModesForConfiguredCount();
        const currentMode = constrainAxiomTokenDetailButtonMode(mode);
        const currentIndex = modes.indexOf(currentMode);
        return modes[(currentIndex + 1) % modes.length] || modes[0] || "dual";
      }

      function axiomTokenDetailButtonModeCount() {
        const count = Number(helpers.state.siteFeatures?.axiom?.instantTradeButtonModeCount);
        return count === 1 || count === 2 || count === 3 ? count : 3;
      }

      function axiomTokenDetailButtonModesForConfiguredCount() {
        const count = axiomTokenDetailButtonModeCount();
        if (count === 1) {
          return ["trench"];
        }
        if (count === 2) {
          return ["trench", "dual"];
        }
        return ["axiom", "trench", "dual"];
      }

      function constrainAxiomTokenDetailButtonMode(mode) {
        const normalizedMode = normalizeAxiomTokenDetailButtonMode(mode) || "dual";
        const modes = axiomTokenDetailButtonModesForConfiguredCount();
        if (modes.includes(normalizedMode)) {
          return normalizedMode;
        }
        return modes.includes("dual") ? "dual" : modes[0] || "dual";
      }

      function isAxiomTokenDetailTrenchOnlyMode() {
        return axiomTokenDetailButtonMode === "trench";
      }

      function isAxiomTokenDetailAxiomOnlyMode() {
        return axiomTokenDetailButtonMode === "axiom";
      }

      function isAxiomTokenDetailSingleButtonMode() {
        return axiomTokenDetailButtonMode === "axiom" || axiomTokenDetailButtonMode === "trench";
      }

      function applyAxiomTokenDetailCompactMode(options = {}) {
        const instantTrade = document.querySelector("div#instant-trade");
        if (!(instantTrade instanceof HTMLElement)) {
          return;
        }
        const constrainedMode = constrainAxiomTokenDetailButtonMode(axiomTokenDetailButtonMode);
        if (constrainedMode !== axiomTokenDetailButtonMode) {
          axiomTokenDetailButtonMode = constrainedMode;
          saveAxiomTokenDetailButtonModePreference(axiomTokenDetailButtonMode);
        }
        const trenchOnly = isAxiomTokenDetailTrenchOnlyMode();
        const axiomOnly = isAxiomTokenDetailAxiomOnlyMode();
        const singleButtonMode = isAxiomTokenDetailSingleButtonMode();
        if (!singleButtonMode) {
          restoreAxiomTokenDetailManagedTransforms();
        }
        instantTrade.toggleAttribute("data-trench-tools-token-detail-compact", singleButtonMode);
        instantTrade.setAttribute("data-trench-tools-token-detail-button-mode", axiomTokenDetailButtonMode);
        instantTrade.querySelectorAll("[data-trench-tools-token-detail-native-control]").forEach((element) => {
          if (!(element instanceof HTMLElement)) {
            return;
          }
          if (trenchOnly) {
            element.setAttribute("data-trench-tools-token-detail-native-hidden", "true");
          } else {
            element.removeAttribute("data-trench-tools-token-detail-native-hidden");
          }
        });
        instantTrade.querySelectorAll("[data-trench-tools-token-detail-inline]").forEach((element) => {
          if (!(element instanceof HTMLElement)) {
            return;
          }
          if (axiomOnly) {
            element.setAttribute("data-trench-tools-token-detail-inline-hidden", "true");
          } else {
            element.removeAttribute("data-trench-tools-token-detail-inline-hidden");
          }
        });
        syncAxiomTokenDetailBuyCurrencyRowVisibility(instantTrade, trenchOnly);
        applyAxiomTokenDetailRememberedPanelSize(instantTrade, options);
        ensureAxiomTokenDetailCompactDrag(instantTrade);
        updateAxiomTokenDetailCompactToggle();
      }

      function clearAxiomTokenDetailPanelMarkers(instantTrade = document.querySelector("div#instant-trade")) {
        if (!(instantTrade instanceof HTMLElement)) {
          return;
        }
        instantTrade.removeAttribute("data-trench-tools-token-detail-compact");
        instantTrade.removeAttribute("data-trench-tools-token-detail-button-mode");
        instantTrade.removeAttribute("data-trench-tools-token-detail-size-mode");
        instantTrade.removeAttribute("data-trench-tools-token-detail-managed-width");
        instantTrade.removeAttribute("data-trench-tools-token-detail-managed-height");
        instantTrade.removeAttribute("data-trench-tools-token-detail-original-width");
        instantTrade.removeAttribute("data-trench-tools-token-detail-original-height");
        instantTrade.querySelectorAll("[data-trench-tools-token-detail-buy-currency-hidden]").forEach((element) => {
          element.removeAttribute("data-trench-tools-token-detail-buy-currency-hidden");
        });
        clearAxiomTokenDetailPresetSettingRows(instantTrade);
        hideAxiomTokenDetailPresetSettingTooltip();
      }

      function applyAxiomTokenDetailRememberedPanelSize(instantTrade, options = {}) {
        if (!(instantTrade instanceof HTMLElement)) {
          return;
        }
        const modeKey = axiomTokenDetailPanelModeKey();
        const alreadyApplied = instantTrade.getAttribute("data-trench-tools-token-detail-size-mode") === modeKey;
        if (alreadyApplied && options.forcePanelSize !== true) {
          removeAxiomTokenDetailInitialSizeStyle();
          ensureAxiomTokenDetailPanelSizeMemory(instantTrade);
          syncAxiomTokenDetailGroupRowWidth(instantTrade);
          return;
        }
        const size = getAxiomTokenDetailRememberedPanelSize(modeKey);
        axiomTokenDetailPanelApplyingLayout = true;
        instantTrade.style.width = `${size.width}px`;
        instantTrade.style.height = `${size.height}px`;
        instantTrade.setAttribute("data-trench-tools-token-detail-size-mode", modeKey);
        instantTrade.setAttribute("data-trench-tools-token-detail-managed-width", String(size.width));
        instantTrade.setAttribute("data-trench-tools-token-detail-managed-height", String(size.height));
        removeAxiomTokenDetailInitialSizeStyle();
        ensureAxiomTokenDetailPanelSizeMemory(instantTrade);
        syncAxiomTokenDetailGroupRowWidth(instantTrade);
        applyAxiomInstantTradeModalPositionPreference(instantTrade, size);
        window.requestAnimationFrame(() => {
          axiomTokenDetailPanelApplyingLayout = false;
        });
      }

      function axiomTokenDetailPanelModeKey() {
        return isAxiomTokenDetailSingleButtonMode() ? "compact" : "expanded";
      }

      function defaultAxiomTokenDetailPanelSizes() {
        return {
          compact: {
            width: AXIOM_TOKEN_DETAIL_DEFAULT_WIDTH,
            height: AXIOM_TOKEN_DETAIL_DEFAULT_HEIGHT
          },
          expanded: {
            width: AXIOM_TOKEN_DETAIL_DEFAULT_WIDTH * 2,
            height: AXIOM_TOKEN_DETAIL_DEFAULT_HEIGHT
          }
        };
      }

      function readAxiomTokenDetailPanelSizePreference() {
        const defaults = defaultAxiomTokenDetailPanelSizes();
        try {
          const parsed = JSON.parse(window.localStorage?.getItem(AXIOM_TOKEN_DETAIL_PANEL_SIZE_STORAGE_KEY) || "{}");
          return {
            compact: readAxiomTokenDetailPanelSizeForMode(parsed, "compact", defaults),
            expanded: readAxiomTokenDetailPanelSizeForMode(parsed, "expanded", defaults)
          };
        } catch (_error) {
          return defaults;
        }
      }

      function readAxiomTokenDetailPanelSizeForMode(parsed, modeKey, defaults) {
        const size = normalizeAxiomTokenDetailPanelSize(parsed?.[modeKey], defaults[modeKey], {
          minWidth: axiomTokenDetailPanelMinWidth(modeKey)
        });
        if (isAxiomTokenDetailMeaningfulUserSize(size, defaults[modeKey])) {
          return size;
        }
        return modeKey === "expanded"
          ? readAxiomInstantTradeModalSizePreference(modeKey, defaults) || defaults[modeKey]
          : defaults[modeKey];
      }

      function saveAxiomTokenDetailPanelSizePreference() {
        try {
          window.localStorage?.setItem(
            AXIOM_TOKEN_DETAIL_PANEL_SIZE_STORAGE_KEY,
            JSON.stringify(axiomTokenDetailPanelSizes)
          );
        } catch (_error) {}
      }

      function normalizeAxiomTokenDetailPanelSize(value, fallback, options = {}) {
        const width = Number(value?.width);
        const height = Number(value?.height);
        const minWidth = Number.isFinite(options.minWidth) ? options.minWidth : 220;
        return {
          width: Number.isFinite(width) && width >= minWidth ? Math.round(width) : fallback.width,
          height: Number.isFinite(height) && height >= 180 ? Math.round(height) : fallback.height
        };
      }

      function isAxiomTokenDetailMeaningfulUserSize(size, fallback) {
        return Math.abs(Number(size?.width) - Number(fallback?.width)) > 8 ||
          Math.abs(Number(size?.height) - Number(fallback?.height)) > 8;
      }

      function getAxiomTokenDetailRememberedPanelSize(modeKey = axiomTokenDetailPanelModeKey()) {
        const defaults = defaultAxiomTokenDetailPanelSizes();
        const size = normalizeAxiomTokenDetailPanelSize(axiomTokenDetailPanelSizes?.[modeKey], defaults[modeKey], {
          minWidth: axiomTokenDetailPanelMinWidth(modeKey)
        });
        if (isAxiomTokenDetailMeaningfulUserSize(size, defaults[modeKey])) {
          return size;
        }
        return modeKey === "expanded"
          ? readAxiomInstantTradeModalSizePreference(modeKey, defaults) || defaults[modeKey]
          : defaults[modeKey];
      }

      function readAxiomInstantTradeModalSizePreference(
        modeKey = axiomTokenDetailPanelModeKey(),
        defaults = defaultAxiomTokenDetailPanelSizes()
      ) {
        if (modeKey !== "expanded") {
          return null;
        }
        try {
          const parsed = JSON.parse(window.localStorage?.getItem(AXIOM_INSTANT_TRADE_MODAL_SIZE_KEY) || "{}");
          const width = parseAxiomInstantTradeMetric(parsed?.width);
          const height = parseAxiomInstantTradeMetric(parsed?.height);
          if (!Number.isFinite(width) || !Number.isFinite(height)) {
            return null;
          }
          const fallback = defaults[modeKey] || defaults.expanded;
          const size = normalizeAxiomTokenDetailPanelSize(
            {
              width,
              height: height >= fallback.height - 8 ? height : fallback.height
            },
            fallback,
            { minWidth: axiomTokenDetailNativeExpandedMinWidth() }
          );
          return isAxiomTokenDetailMeaningfulUserSize(size, fallback) ? size : null;
        } catch (_error) {
          return null;
        }
      }

      function readAxiomInstantTradeModalPositionPreference(size = null) {
        try {
          const parsed = JSON.parse(
            window.localStorage?.getItem(AXIOM_INSTANT_TRADE_MODAL_POSITION_KEY) || "{}"
          );
          const rawX = parseAxiomInstantTradeMetric(parsed?.x ?? parsed?.left);
          const rawY = parseAxiomInstantTradeMetric(parsed?.y ?? parsed?.top);
          if (!Number.isFinite(rawX) || !Number.isFinite(rawY)) {
            return null;
          }
          const width = Number(size?.width);
          const height = Number(size?.height);
          const maxX = Number.isFinite(width) && width > 0 ? Math.max(0, window.innerWidth - width) : window.innerWidth;
          const maxY = Number.isFinite(height) && height > 0 ? Math.max(0, window.innerHeight - height) : window.innerHeight;
          return {
            x: Math.round(clampAxiomTokenDetailDragPosition(rawX, 0, maxX)),
            y: Math.round(clampAxiomTokenDetailDragPosition(rawY, 0, maxY))
          };
        } catch (_error) {
          return null;
        }
      }

      function parseAxiomInstantTradeMetric(value) {
        if (typeof value === "number") {
          return value;
        }
        if (typeof value === "string") {
          return Number.parseFloat(value);
        }
        return Number.NaN;
      }

      function applyAxiomInstantTradeModalPositionPreference(instantTrade, size = null) {
        const container = findAxiomTokenDetailDragContainer(instantTrade);
        if (!(container instanceof HTMLElement) || axiomTokenDetailCompactDragState) {
          return;
        }
        const rect = container.getBoundingClientRect();
        const position = readAxiomInstantTradeModalPositionPreference(size || rect);
        if (!position) {
          return;
        }
        setAxiomTokenDetailDragContainerTransform(container, position.x, position.y, {
          managed: isAxiomTokenDetailSingleButtonMode()
        });
      }

      function axiomTokenDetailPanelMinWidth(modeKey) {
        return modeKey === "expanded" ? AXIOM_TOKEN_DETAIL_DEFAULT_WIDTH : 220;
      }

      function axiomTokenDetailNativeExpandedMinWidth() {
        return AXIOM_TOKEN_DETAIL_DEFAULT_WIDTH + 48;
      }

      function ensureAxiomTokenDetailInitialSizeStyle() {
        const size = getAxiomTokenDetailRememberedPanelSize();
        const style =
          document.getElementById(AXIOM_TOKEN_DETAIL_INITIAL_SIZE_STYLE_ID) || document.createElement("style");
        style.id = AXIOM_TOKEN_DETAIL_INITIAL_SIZE_STYLE_ID;
        style.textContent = buildAxiomTokenDetailInitialSizeStyleText(size, axiomTokenDetailButtonMode);
        if (!style.isConnected) {
          (document.head || document.documentElement).appendChild(style);
        }
      }

      function buildAxiomTokenDetailInitialSizeStyleText(size, buttonMode) {
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
        return `
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
          ${trenchOnlyRules}
          ${axiomOnlyRules}
        `;
      }

      function removeAxiomTokenDetailInitialSizeStyle() {
        document.getElementById(AXIOM_TOKEN_DETAIL_INITIAL_SIZE_STYLE_ID)?.remove();
      }

      function ensureAxiomTokenDetailPanelSizeMemory(instantTrade) {
        if (!(instantTrade instanceof HTMLElement)) {
          disconnectAxiomTokenDetailPanelSizeMemory();
          return;
        }
        if (axiomTokenDetailPanelSizeResizePanel !== instantTrade) {
          disconnectAxiomTokenDetailPanelSizeMemory(false);
          axiomTokenDetailPanelSizeResizePanel = instantTrade;
          if (typeof ResizeObserver === "function") {
            axiomTokenDetailPanelSizeResizeObserver = new ResizeObserver(() => {
              if (!axiomTokenDetailPanelApplyingLayout) {
                rememberAxiomTokenDetailPanelSize(instantTrade, axiomTokenDetailPanelModeKey());
              }
              syncAxiomTokenDetailGroupRowWidth(instantTrade);
            });
            axiomTokenDetailPanelSizeResizeObserver.observe(instantTrade);
          }
          if (typeof MutationObserver === "function") {
            axiomTokenDetailPanelSizeMutationObserver = new MutationObserver(() => {
              if (axiomTokenDetailPanelApplyingLayout) {
                return;
              }
              rememberAxiomTokenDetailPanelSize(instantTrade, axiomTokenDetailPanelModeKey());
              syncAxiomTokenDetailGroupRowWidth(instantTrade);
            });
            observeAxiomTokenDetailPanelSizeMutations(instantTrade);
          }
        }
      }

      function disconnectAxiomTokenDetailPanelSizeMemory(remember = true) {
        if (remember && axiomTokenDetailPanelSizeResizePanel instanceof HTMLElement) {
          rememberAxiomTokenDetailPanelSize(axiomTokenDetailPanelSizeResizePanel, axiomTokenDetailPanelModeKey());
        }
        if (axiomTokenDetailPanelSizeResizeObserver) {
          axiomTokenDetailPanelSizeResizeObserver.disconnect();
          axiomTokenDetailPanelSizeResizeObserver = null;
        }
        if (axiomTokenDetailPanelSizeMutationObserver) {
          axiomTokenDetailPanelSizeMutationObserver.disconnect();
          axiomTokenDetailPanelSizeMutationObserver = null;
        }
        axiomTokenDetailPanelSizeResizePanel = null;
      }

      function observeAxiomTokenDetailPanelSizeMutations(instantTrade) {
        if (!axiomTokenDetailPanelSizeMutationObserver || !(instantTrade instanceof HTMLElement)) {
          return;
        }
        axiomTokenDetailPanelSizeMutationObserver.observe(instantTrade, {
          attributes: true,
          attributeFilter: ["style"]
        });
      }

      function rememberAxiomTokenDetailPanelSize(instantTrade, modeKey = axiomTokenDetailPanelModeKey(), options = {}) {
        if (!(instantTrade instanceof HTMLElement) || axiomTokenDetailPanelApplyingLayout) {
          return;
        }
        const rect = instantTrade.getBoundingClientRect();
        const styleWidth = Number.parseFloat(instantTrade.style.width || "");
        const styleHeight = Number.parseFloat(instantTrade.style.height || "");
        const width = Number.isFinite(styleWidth) && styleWidth > 0 ? styleWidth : rect.width;
        const height = Number.isFinite(styleHeight) && styleHeight > 0 ? styleHeight : rect.height;
        const size = normalizeAxiomTokenDetailPanelSize(
          { width, height },
          defaultAxiomTokenDetailPanelSizes()[modeKey],
          { minWidth: axiomTokenDetailPanelMinWidth(modeKey) }
        );
        const managedWidth = Number(instantTrade.getAttribute("data-trench-tools-token-detail-managed-width"));
        const managedHeight = Number(instantTrade.getAttribute("data-trench-tools-token-detail-managed-height"));
        const changedFromManaged =
          Number.isFinite(managedWidth) &&
          Number.isFinite(managedHeight) &&
          (Math.abs(size.width - managedWidth) > 8 || Math.abs(size.height - managedHeight) > 8);
        const shouldPersist = options.userAdjusted === true ||
          changedFromManaged;
        if (!shouldPersist) {
          return;
        }
        if (options.userAdjusted === true || changedFromManaged) {
          instantTrade.setAttribute("data-trench-tools-token-detail-managed-width", String(size.width));
          instantTrade.setAttribute("data-trench-tools-token-detail-managed-height", String(size.height));
        }
        axiomTokenDetailPanelSizes = {
          ...axiomTokenDetailPanelSizes,
          [modeKey]: size
        };
        saveAxiomTokenDetailPanelSizePreference();
        if (modeKey === "expanded") {
          rememberAxiomInstantTradeModalSize(size.width, size.height);
        }
      }

      function ensureAxiomTokenDetailGroupRowSizeSync(
        instantTrade = document.querySelector("div#instant-trade")
      ) {
        if (!(instantTrade instanceof HTMLElement)) {
          disconnectAxiomTokenDetailGroupRowSizeSync();
          return;
        }
        if (axiomTokenDetailGroupRowResizePanel !== instantTrade) {
          disconnectAxiomTokenDetailGroupRowSizeSync(false);
          axiomTokenDetailGroupRowResizePanel = instantTrade;
          if (typeof ResizeObserver === "function") {
            axiomTokenDetailGroupRowResizeObserver = new ResizeObserver(() => {
              syncAxiomTokenDetailGroupRowWidth(instantTrade);
            });
            axiomTokenDetailGroupRowResizeObserver.observe(instantTrade);
          }
        }
        syncAxiomTokenDetailGroupRowWidth(instantTrade);
      }

      function disconnectAxiomTokenDetailGroupRowSizeSync(restore = true) {
        if (axiomTokenDetailGroupRowResizeObserver) {
          axiomTokenDetailGroupRowResizeObserver.disconnect();
          axiomTokenDetailGroupRowResizeObserver = null;
        }
        if (restore) {
          restoreAxiomTokenDetailGroupRowWidth();
          restoreAxiomTokenDetailPanelContainerWidth();
        }
        axiomTokenDetailGroupRowResizePanel = null;
      }

      function syncAxiomTokenDetailGroupRowWidth(instantTrade, options = {}) {
        if (!(instantTrade instanceof HTMLElement)) {
          return;
        }
        const rect = instantTrade.getBoundingClientRect();
        const width = Math.round(rect.width);
        if (width > 0) {
          syncAxiomTokenDetailPanelContainerWidth(instantTrade, width, options);
        }
        const groupRow = findAxiomTokenDetailGroupRow(instantTrade);
        if (!(groupRow instanceof HTMLElement)) {
          return;
        }
        if (!groupRow.hasAttribute("data-trench-tools-token-detail-group-row-original-width")) {
          groupRow.setAttribute("data-trench-tools-token-detail-group-row-original-width", groupRow.style.width || "");
        }
        if (!groupRow.hasAttribute("data-trench-tools-token-detail-group-row-original-max-width")) {
          groupRow.setAttribute("data-trench-tools-token-detail-group-row-original-max-width", groupRow.style.maxWidth || "");
        }
        groupRow.setAttribute("data-trench-tools-token-detail-group-row", "true");
        if (width > 0) {
          groupRow.style.width = "fit-content";
          groupRow.style.maxWidth = `${width}px`;
        }
      }

      function restoreAxiomTokenDetailGroupRowWidth() {
        document.querySelectorAll("[data-trench-tools-token-detail-group-row]").forEach((element) => {
          if (!(element instanceof HTMLElement)) {
            return;
          }
          const originalWidth = element.getAttribute("data-trench-tools-token-detail-group-row-original-width") || "";
          const originalMaxWidth = element.getAttribute("data-trench-tools-token-detail-group-row-original-max-width") || "";
          if (originalWidth) {
            element.style.width = originalWidth;
          } else {
            element.style.removeProperty("width");
          }
          if (originalMaxWidth) {
            element.style.maxWidth = originalMaxWidth;
          } else {
            element.style.removeProperty("max-width");
          }
          element.removeAttribute("data-trench-tools-token-detail-group-row");
          element.removeAttribute("data-trench-tools-token-detail-group-row-original-width");
          element.removeAttribute("data-trench-tools-token-detail-group-row-original-max-width");
        });
      }

      function syncAxiomTokenDetailPanelContainerWidth(instantTrade, width, options = {}) {
        if (!Number.isFinite(width) || width <= 0) {
          return;
        }
        findAxiomTokenDetailPanelContainers(instantTrade, width).forEach((container) => {
          if (!container.hasAttribute("data-trench-tools-token-detail-container-original-width")) {
            container.setAttribute("data-trench-tools-token-detail-container-original-width", container.style.width || "");
            container.setAttribute(
              "data-trench-tools-token-detail-container-original-min-width",
              container.style.minWidth || ""
            );
            container.setAttribute(
              "data-trench-tools-token-detail-container-original-max-width",
              container.style.maxWidth || ""
            );
          }
          container.setAttribute("data-trench-tools-token-detail-container", "true");
          container.style.width = `${width}px`;
          container.style.minWidth = `${width}px`;
          container.style.maxWidth = `${width}px`;
        });
      }

      function restoreAxiomTokenDetailPanelContainerWidth() {
        document.querySelectorAll("[data-trench-tools-token-detail-container]").forEach((element) => {
          if (!(element instanceof HTMLElement)) {
            return;
          }
          const originalWidth = element.getAttribute("data-trench-tools-token-detail-container-original-width") || "";
          const originalMinWidth =
            element.getAttribute("data-trench-tools-token-detail-container-original-min-width") || "";
          const originalMaxWidth =
            element.getAttribute("data-trench-tools-token-detail-container-original-max-width") || "";
          if (originalWidth) {
            element.style.width = originalWidth;
          } else {
            element.style.removeProperty("width");
          }
          if (originalMinWidth) {
            element.style.minWidth = originalMinWidth;
          } else {
            element.style.removeProperty("min-width");
          }
          if (originalMaxWidth) {
            element.style.maxWidth = originalMaxWidth;
          } else {
            element.style.removeProperty("max-width");
          }
          element.removeAttribute("data-trench-tools-token-detail-container");
          element.removeAttribute("data-trench-tools-token-detail-container-original-width");
          element.removeAttribute("data-trench-tools-token-detail-container-original-min-width");
          element.removeAttribute("data-trench-tools-token-detail-container-original-max-width");
        });
      }

      function findAxiomTokenDetailPanelContainers(instantTrade, width) {
        if (!(instantTrade instanceof HTMLElement)) {
          return [];
        }
        const containers = [];
        const parent = instantTrade.parentElement;
        if (!(parent instanceof HTMLElement) || parent === document.body || parent === document.documentElement) {
          return containers;
        }
        if (Array.from(parent.children).includes(instantTrade)) {
          containers.push(parent);
        }
        let current = parent.parentElement;
        for (let depth = 0; current instanceof HTMLElement && depth < 4; depth += 1) {
          if (current === document.body || current === document.documentElement) {
            break;
          }
          const computed = window.getComputedStyle?.(current);
          const positioned =
            computed?.position === "fixed" ||
            computed?.position === "absolute" ||
            computed?.position === "sticky" ||
            (computed?.transform && computed.transform !== "none");
          if (
            current.hasAttribute("data-trench-tools-token-detail-container") ||
            (positioned && isAxiomTokenDetailPanelSizedAncestor(current, instantTrade))
          ) {
            containers.push(current);
          }
          current = current.parentElement;
        }
        return containers;
      }

      function isAxiomTokenDetailPanelSizedAncestor(element, instantTrade) {
        if (!(element instanceof HTMLElement) || !(instantTrade instanceof HTMLElement)) {
          return false;
        }
        const rect = element.getBoundingClientRect();
        const panelRect = instantTrade.getBoundingClientRect();
        return rect.width > 0 &&
          rect.height >= panelRect.height &&
          rect.height <= panelRect.height + 120;
      }

      function findAxiomTokenDetailDragContainer(instantTrade) {
        if (!(instantTrade instanceof HTMLElement)) {
          return null;
        }
        const panelRect = instantTrade.getBoundingClientRect();
        let firstFixed = null;
        let current = instantTrade.parentElement;
        for (let depth = 0; current instanceof HTMLElement && depth < 6; depth += 1) {
          if (current === document.body || current === document.documentElement) {
            break;
          }
          const computed = window.getComputedStyle?.(current);
          if (computed?.position === "fixed") {
            firstFixed ||= current;
            if (isAxiomTokenDetailDragContainerCandidate(current, panelRect)) {
              return current;
            }
          }
          current = current.parentElement;
        }
        return firstFixed;
      }

      function isAxiomTokenDetailDragContainerCandidate(element, panelRect) {
        const rect = element.getBoundingClientRect();
        if (
          !Number.isFinite(rect.width) ||
          !Number.isFinite(rect.height) ||
          rect.width <= 0 ||
          rect.height <= 0
        ) {
          return false;
        }
        if (rect.width >= window.innerWidth - 8 || rect.height >= window.innerHeight - 8) {
          return false;
        }
        return rect.width >= panelRect.width - 8 &&
          rect.width <= panelRect.width + 180 &&
          rect.height >= panelRect.height - 8 &&
          rect.height <= panelRect.height + 220;
      }

      function rememberAxiomInstantTradeModalPosition(x, y) {
        if (!Number.isFinite(x) || !Number.isFinite(y)) {
          return;
        }
        try {
          const value = JSON.stringify({ x: Math.round(x), y: Math.round(y) });
          window.localStorage?.setItem(
            AXIOM_INSTANT_TRADE_MODAL_POSITION_KEY,
            value
          );
        } catch (_error) {}
      }

      function setAxiomTokenDetailDragContainerTransform(container, x, y, options = {}) {
        if (!(container instanceof HTMLElement) || !Number.isFinite(x) || !Number.isFinite(y)) {
          return;
        }
        const value = `translate(${Math.round(x)}px, ${Math.round(y)}px)`;
        if (options.managed === true) {
          if (!container.hasAttribute("data-trench-tools-token-detail-managed-transform")) {
            container.setAttribute("data-trench-tools-token-detail-original-transform", container.style.transform || "");
            container.setAttribute(
              "data-trench-tools-token-detail-original-transform-priority",
              container.style.getPropertyPriority("transform") || ""
            );
            container.setAttribute("data-trench-tools-token-detail-original-transition", container.style.transition || "");
            container.setAttribute(
              "data-trench-tools-token-detail-original-transition-priority",
              container.style.getPropertyPriority("transition") || ""
            );
          }
          container.setAttribute("data-trench-tools-token-detail-managed-transform", "true");
          container.style.setProperty("transition", "none", "important");
          container.style.setProperty("transform", value, "important");
          return;
        }
        container.style.transform = value;
      }

      function restoreAxiomTokenDetailManagedTransforms() {
        document.querySelectorAll("[data-trench-tools-token-detail-managed-transform]").forEach((element) => {
          if (!(element instanceof HTMLElement)) {
            return;
          }
          const original = element.getAttribute("data-trench-tools-token-detail-original-transform") || "";
          const priority = element.getAttribute("data-trench-tools-token-detail-original-transform-priority") || "";
          const originalTransition = element.getAttribute("data-trench-tools-token-detail-original-transition") || "";
          const transitionPriority =
            element.getAttribute("data-trench-tools-token-detail-original-transition-priority") || "";
          if (original) {
            element.style.setProperty("transform", original, priority);
          } else {
            element.style.removeProperty("transform");
          }
          if (originalTransition) {
            element.style.setProperty("transition", originalTransition, transitionPriority);
          } else {
            element.style.removeProperty("transition");
          }
          element.removeAttribute("data-trench-tools-token-detail-managed-transform");
          element.removeAttribute("data-trench-tools-token-detail-original-transform");
          element.removeAttribute("data-trench-tools-token-detail-original-transform-priority");
          element.removeAttribute("data-trench-tools-token-detail-original-transition");
          element.removeAttribute("data-trench-tools-token-detail-original-transition-priority");
        });
      }

      function rememberAxiomInstantTradeModalSize(width, height) {
        if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
          return;
        }
        try {
          window.localStorage?.setItem(
            AXIOM_INSTANT_TRADE_MODAL_SIZE_KEY,
            JSON.stringify({ width: Math.round(width), height: Math.round(height) })
          );
        } catch (_error) {}
      }

      function ensureAxiomTokenDetailCompactDrag(instantTrade = document.querySelector("div#instant-trade")) {
        if (!(instantTrade instanceof HTMLElement)) {
          disconnectAxiomTokenDetailCompactDrag();
          return;
        }
        const header = findAxiomTokenDetailPanelHeader(instantTrade);
        if (!(header instanceof HTMLElement)) {
          disconnectAxiomTokenDetailCompactDrag();
          return;
        }
        if (axiomTokenDetailCompactDragHeader === header) {
          return;
        }
        disconnectAxiomTokenDetailCompactDrag();
        axiomTokenDetailCompactDragHeader = header;
        header.addEventListener("pointerdown", handleAxiomTokenDetailNativeDragMemoryStart, true);
        document.removeEventListener("mousedown", handleAxiomTokenDetailCompactDragStart, true);
        document.addEventListener("mousedown", handleAxiomTokenDetailCompactDragStart, true);
      }

      function disconnectAxiomTokenDetailCompactDrag() {
        if (axiomTokenDetailCompactDragHeader instanceof HTMLElement) {
          axiomTokenDetailCompactDragHeader.removeEventListener(
            "pointerdown",
            handleAxiomTokenDetailNativeDragMemoryStart,
            true
          );
        }
        document.removeEventListener("mousedown", handleAxiomTokenDetailCompactDragStart, true);
        axiomTokenDetailCompactDragHeader = null;
        stopAxiomTokenDetailCompactDrag();
      }

      function handleAxiomTokenDetailNativeDragMemoryStart(event) {
        if (isAxiomTokenDetailSingleButtonMode() || event.button !== 0) {
          return;
        }
        const instantTrade = document.querySelector("div#instant-trade");
        if (!(instantTrade instanceof HTMLElement)) {
          return;
        }
        if (isAxiomTokenDetailCompactDragHardExcludedTarget(event.target, event.currentTarget)) {
          return;
        }
        const container = findAxiomTokenDetailDragContainer(instantTrade);
        if (!(container instanceof HTMLElement)) {
          return;
        }
        const startRect = container.getBoundingClientRect();
        let remembered = false;
        const cleanup = () => {
          document.removeEventListener("pointerup", remember, true);
          document.removeEventListener("pointercancel", remember, true);
          window.removeEventListener("blur", remember);
        };
        const remember = () => {
          if (remembered) {
            return;
          }
          remembered = true;
          cleanup();
          window.setTimeout(() => {
            if (!(container instanceof HTMLElement) || !container.isConnected) {
              return;
            }
            const rect = container.getBoundingClientRect();
            if (Math.abs(rect.x - startRect.x) < 2 && Math.abs(rect.y - startRect.y) < 2) {
              return;
            }
            rememberAxiomInstantTradeModalPosition(rect.x, rect.y);
          }, 0);
        };
        document.addEventListener("pointerup", remember, true);
        document.addEventListener("pointercancel", remember, true);
        window.addEventListener("blur", remember);
      }

      function handleAxiomTokenDetailCompactDragStart(event) {
        if (!isAxiomTokenDetailSingleButtonMode() || event.button !== 0) {
          return;
        }
        const instantTrade = document.querySelector("div#instant-trade");
        if (!(instantTrade instanceof HTMLElement) || !instantTrade.hasAttribute("data-trench-tools-token-detail-compact")) {
          return;
        }
        const header = findAxiomTokenDetailPanelHeader(instantTrade);
        if (!(header instanceof HTMLElement) || !(event.target instanceof Node) || !header.contains(event.target)) {
          return;
        }
        if (isAxiomTokenDetailCompactDragHardExcludedTarget(event.target, header)) {
          return;
        }
        const container = findAxiomTokenDetailDragContainer(instantTrade);
        if (!(container instanceof HTMLElement)) {
          return;
        }
        const rect = container.getBoundingClientRect();
        if (!Number.isFinite(rect.x) || !Number.isFinite(rect.y) || !Number.isFinite(rect.width) || rect.width <= 0) {
          return;
        }
        axiomTokenDetailCompactDragState = {
          container,
          overlay: null,
          pending: true,
          startClientX: event.clientX,
          startClientY: event.clientY,
          startX: rect.x,
          startY: rect.y,
          maxX: Math.max(0, window.innerWidth - rect.width),
          maxY: Math.max(0, window.innerHeight - rect.height),
          nextX: rect.x,
          nextY: rect.y
        };
        document.addEventListener("mousemove", handleAxiomTokenDetailCompactDragMove, true);
        document.addEventListener("mouseup", handleAxiomTokenDetailCompactDragEnd, true);
        document.addEventListener("pointerup", handleAxiomTokenDetailCompactDragEnd, true);
        document.addEventListener("pointercancel", handleAxiomTokenDetailCompactDragEnd, true);
        window.addEventListener("mouseup", handleAxiomTokenDetailCompactDragEnd, true);
        window.addEventListener("blur", stopAxiomTokenDetailCompactDrag, { once: true });
      }

      function handleAxiomTokenDetailCompactDragMove(event) {
        const state = axiomTokenDetailCompactDragState;
        if (!state?.container?.isConnected) {
          stopAxiomTokenDetailCompactDrag();
          return;
        }
        if (event.buttons === 0) {
          if (state.pending) {
            stopAxiomTokenDetailCompactDrag();
          } else {
            handleAxiomTokenDetailCompactDragEnd(event);
          }
          return;
        }
        if (state.pending) {
          const distance = Math.hypot(
            event.clientX - state.startClientX,
            event.clientY - state.startClientY
          );
          if (distance < 4) {
            return;
          }
          state.pending = false;
          state.overlay = buildAxiomTokenDetailCompactDragOverlay();
          document.documentElement.appendChild(state.overlay);
        }
        event.preventDefault();
        event.stopPropagation();
        event.stopImmediatePropagation?.();
        const nextX = clampAxiomTokenDetailDragPosition(
          state.startX + event.clientX - state.startClientX,
          0,
          state.maxX
        );
        const nextY = clampAxiomTokenDetailDragPosition(
          state.startY + event.clientY - state.startClientY,
          0,
          state.maxY
        );
        state.nextX = Math.round(nextX);
        state.nextY = Math.round(nextY);
        applyAxiomTokenDetailCompactDragFrame(state);
      }

      function handleAxiomTokenDetailCompactDragEnd(event) {
        const state = axiomTokenDetailCompactDragState;
        if (state) {
          if (state.pending) {
            stopAxiomTokenDetailCompactDrag();
            return;
          }
          event.preventDefault();
          event.stopPropagation();
          event.stopImmediatePropagation?.();
          if (state.container instanceof HTMLElement && state.container.isConnected) {
            applyAxiomTokenDetailCompactDragFrame(state);
            setAxiomTokenDetailDragContainerTransform(state.container, state.nextX, state.nextY, { managed: true });
            rememberAxiomInstantTradeModalPosition(state.nextX, state.nextY);
          }
        }
        stopAxiomTokenDetailCompactDrag();
      }

      function stopAxiomTokenDetailCompactDrag() {
        axiomTokenDetailCompactDragState?.overlay?.remove();
        document.removeEventListener("mousemove", handleAxiomTokenDetailCompactDragMove, true);
        document.removeEventListener("mouseup", handleAxiomTokenDetailCompactDragEnd, true);
        document.removeEventListener("pointerup", handleAxiomTokenDetailCompactDragEnd, true);
        document.removeEventListener("pointercancel", handleAxiomTokenDetailCompactDragEnd, true);
        window.removeEventListener("mouseup", handleAxiomTokenDetailCompactDragEnd, true);
        window.removeEventListener("blur", stopAxiomTokenDetailCompactDrag);
        axiomTokenDetailCompactDragState = null;
      }

      function applyAxiomTokenDetailCompactDragFrame(state) {
        if (!state?.container?.isConnected) {
          return;
        }
        setAxiomTokenDetailDragContainerTransform(state.container, state.nextX, state.nextY, { managed: true });
      }

      function buildAxiomTokenDetailCompactDragOverlay() {
        const overlay = document.createElement("div");
        overlay.setAttribute("data-trench-tools-token-detail-compact-drag-overlay", "true");
        Object.assign(overlay.style, {
          position: "fixed",
          inset: "0",
          zIndex: "2147483646",
          cursor: "grabbing",
          background: "transparent",
          userSelect: "none",
          touchAction: "none"
        });
        return overlay;
      }

      function isAxiomTokenDetailCompactDragHardExcludedTarget(target, header) {
        if (!(target instanceof Element) || !(header instanceof HTMLElement)) {
          return true;
        }
        return Boolean(
          target.closest("a, input, textarea, select") ||
          target.closest("[data-trench-tools-token-detail-compact-toggle]") ||
          isAxiomTokenDetailCloseLikeTarget(target)
        );
      }

      function isAxiomTokenDetailCloseLikeTarget(target) {
        const control = target.closest("button, [role='button'], div");
        if (!(control instanceof HTMLElement)) {
          return false;
        }
        const text = String(control.textContent || "").replace(/\s+/g, "").trim();
        const className = String(control.className || "");
        const ariaLabel = String(control.getAttribute("aria-label") || "");
        return text === "×" ||
          text === "X" ||
          /close/i.test(ariaLabel) ||
          /ri-close|ri-close-line|\\bclose\\b/i.test(className);
      }

      function clampAxiomTokenDetailDragPosition(value, min, max) {
        if (!Number.isFinite(value)) {
          return min;
        }
        return Math.min(Math.max(value, min), max);
      }

      function findAxiomTokenDetailGroupRow(instantTrade) {
        if (!(instantTrade instanceof HTMLElement)) {
          return null;
        }
        const previous = instantTrade.previousElementSibling;
        if (previous instanceof HTMLElement) {
          return previous;
        }
        const parent = instantTrade.parentElement;
        if (!(parent instanceof HTMLElement)) {
          return null;
        }
        const children = Array.from(parent.children);
        const index = children.indexOf(instantTrade);
        const candidate = index > 0 ? children[index - 1] : null;
        return candidate instanceof HTMLElement ? candidate : null;
      }

      function ensureAxiomTokenDetailCompactToggle() {
        const instantTrade = document.querySelector("div#instant-trade");
        if (!(instantTrade instanceof HTMLElement)) {
          return;
        }
        const header = findAxiomTokenDetailPanelHeader(instantTrade);
        if (!(header instanceof HTMLElement)) {
          return;
        }
        const host = findAxiomTokenDetailCompactToggleHost(header) || header;
        let button = instantTrade.querySelector("[data-trench-tools-token-detail-compact-toggle]");
        if (!(button instanceof HTMLButtonElement)) {
          button = document.createElement("button");
          button.type = "button";
          button.textContent = "TT";
          button.setAttribute("data-trench-tools-token-detail-compact-toggle", "true");
          button.setAttribute("aria-label", "Cycle Axiom instant panel button mode");
          button.addEventListener("mousedown", (event) => {
            event.preventDefault();
            event.stopPropagation();
          });
          button.addEventListener("click", (event) => {
            event.preventDefault();
            event.stopPropagation();
            setAxiomTokenDetailButtonMode(nextAxiomTokenDetailButtonMode());
          });
        }
        const closeButton = findAxiomTokenDetailCloseControl(host);
        if (closeButton instanceof HTMLElement) {
          if (button.parentElement !== host || button.nextElementSibling !== closeButton) {
            host.insertBefore(button, closeButton);
          }
        } else if (button.parentElement !== host) {
            host.appendChild(button);
        }
        updateAxiomTokenDetailCompactToggle(button);
      }

      function findAxiomTokenDetailPanelHeader(instantTrade) {
        if (!(instantTrade instanceof HTMLElement)) {
          return null;
        }
        return Array.from(instantTrade.querySelectorAll("div")).find((element) => {
          if (!(element instanceof HTMLElement)) {
            return false;
          }
          const className = String(element.className || "");
          return className.includes("cursor-move") &&
            className.includes("border-b") &&
            className.includes("justify-between");
        }) || null;
      }

      function findAxiomTokenDetailCompactToggleHost(header) {
        if (!(header instanceof HTMLElement)) {
          return null;
        }
        const headerRow = Array.from(header.children).find((element) => {
          if (!(element instanceof HTMLElement)) {
            return false;
          }
          const className = String(element.className || "");
          return className.includes("w-full") && className.includes("justify-between");
        });
        if (!(headerRow instanceof HTMLElement)) {
          return null;
        }
        const children = Array.from(headerRow.children).filter((element) => element instanceof HTMLElement);
        return children[children.length - 1] || null;
      }

      function findAxiomTokenDetailCloseControl(host) {
        if (!(host instanceof HTMLElement)) {
          return null;
        }
        return Array.from(host.children).find((element) => {
          if (!(element instanceof HTMLElement)) {
            return false;
          }
          const text = String(element.textContent || "").replace(/\s+/g, "").trim();
          const className = String(element.className || "");
          return text === "×" ||
            text === "X" ||
            /ri-close|ri-close-line|close/i.test(className) ||
            Boolean(element.querySelector("i[class*='close'], svg"));
        }) || null;
      }

      function updateAxiomTokenDetailCompactToggle(button = null) {
        const toggle =
          button instanceof HTMLButtonElement
            ? button
            : document.querySelector("[data-trench-tools-token-detail-compact-toggle]");
        if (!(toggle instanceof HTMLButtonElement)) {
          return;
        }
        toggle.setAttribute("aria-pressed", isAxiomTokenDetailSingleButtonMode() ? "true" : "false");
        toggle.setAttribute("data-trench-tools-token-detail-button-mode", axiomTokenDetailButtonMode);
        const modes = axiomTokenDetailButtonModesForConfiguredCount();
        const nextMode = nextAxiomTokenDetailButtonMode();
        const labelByMode = {
          axiom: "Axiom buttons only",
          trench: "Trench Tools buttons only",
          dual: "both button sets"
        };
        toggle.title = modes.length === 1
          ? `${labelByMode[axiomTokenDetailButtonMode] || labelByMode.dual} (locked by settings)`
          : `${labelByMode[axiomTokenDetailButtonMode] || labelByMode.dual} - click for ${labelByMode[nextMode] || labelByMode.dual}`;
        toggle.classList.toggle(
          "trench-tools-axiom-token-detail-compact-toggle-active",
          isAxiomTokenDetailSingleButtonMode()
        );
      }

      function ensureAxiomTokenDetailWalletSelector(instantTrade = document.querySelector("div#instant-trade")) {
        if (!(instantTrade instanceof HTMLElement)) {
          cleanupAxiomTokenDetailWalletSelector();
          return;
        }
        const nativeWalletControl = findAxiomTokenDetailWalletControl(instantTrade);
        if (!(nativeWalletControl instanceof HTMLElement)) {
          cleanupAxiomTokenDetailWalletSelector();
          return;
        }
        const selection = resolveAxiomTokenDetailWalletSelection();
        if (!selection) {
          cleanupAxiomTokenDetailWalletSelector();
          return;
        }
        document.querySelectorAll("[data-trench-tools-token-detail-wallet-selector]").forEach((element) => element.remove());
        if (axiomTokenDetailWalletControl === nativeWalletControl) {
          return;
        }
        cleanupAxiomTokenDetailWalletSelector();
        axiomTokenDetailWalletControl = nativeWalletControl;
        nativeWalletControl.setAttribute("data-trench-tools-token-detail-wallet-trigger", "true");
        const handleWalletMouseDown = (event) => {
          if (!isAxiomTokenDetailTrenchOnlyMode()) {
            return;
          }
          event.preventDefault();
          event.stopPropagation();
          event.stopImmediatePropagation?.();
        };
        const handleWalletClick = (event) => {
          if (isAxiomTokenDetailAxiomOnlyMode()) {
            document.querySelectorAll("[data-trench-tools-token-detail-wallet-menu]").forEach((element) => element.remove());
            document.removeEventListener("mousedown", handleAxiomTokenDetailWalletMenuOutsideClick, true);
            return;
          }
          if (isAxiomTokenDetailTrenchOnlyMode()) {
            event.preventDefault();
            event.stopPropagation();
            event.stopImmediatePropagation?.();
            toggleAxiomTokenDetailStandaloneWalletMenu(nativeWalletControl);
            return;
          }
          scheduleAxiomTokenDetailWalletSidecar(nativeWalletControl);
        };
        nativeWalletControl.addEventListener("mousedown", handleWalletMouseDown, true);
        nativeWalletControl.addEventListener("click", handleWalletClick, true);
        axiomTokenDetailWalletControlCleanup = () => {
          nativeWalletControl.removeEventListener("mousedown", handleWalletMouseDown, true);
          nativeWalletControl.removeEventListener("click", handleWalletClick, true);
          nativeWalletControl.removeAttribute("data-trench-tools-token-detail-wallet-trigger");
        };
      }

      function cleanupAxiomTokenDetailWalletSelector() {
        document.querySelectorAll(
          "[data-trench-tools-token-detail-wallet-selector], [data-trench-tools-token-detail-wallet-menu]"
        ).forEach((element) => element.remove());
        document.removeEventListener("mousedown", handleAxiomTokenDetailWalletMenuOutsideClick, true);
        axiomTokenDetailWalletControlCleanup?.();
        axiomTokenDetailWalletControlCleanup = null;
        axiomTokenDetailWalletControl = null;
      }

      function findAxiomTokenDetailWalletControl(instantTrade) {
        if (!(instantTrade instanceof HTMLElement)) {
          return null;
        }
        const candidates = Array.from(instantTrade.querySelectorAll("div, button, [role='button']"))
          .filter((element) => {
            if (!(element instanceof HTMLElement)) {
              return false;
            }
            const className = String(element.className || "");
            const text = String(element.textContent || "").replace(/\s+/g, " ").trim();
            const rect = element.getBoundingClientRect();
            const hasWalletClass = className.includes("group/wallets");
            if (!hasWalletClass && (rect.width > 260 || rect.height > 56)) {
              return false;
            }
            return (
              hasWalletClass ||
              /wallet/i.test(text) ||
              /SOLANA_PRIVATE_KEY/i.test(text) ||
              /^#\d+$/.test(text)
            ) && isVisibleAxiomNode(element);
          });
        return candidates
          .sort((a, b) => {
            const aScore = axiomTokenDetailWalletControlScore(a);
            const bScore = axiomTokenDetailWalletControlScore(b);
            return bScore - aScore;
          })[0] || null;
      }

      function axiomTokenDetailWalletControlScore(element) {
        const className = String(element?.className || "");
        const text = String(element?.textContent || "").replace(/\s+/g, " ").trim();
        let score = 0;
        if (className.includes("group/wallets")) score += 100;
        if (element?.matches?.("div.rounded-full, button.rounded-full")) score += 20;
        if (/wallet/i.test(text)) score += 10;
        if (/SOLANA_PRIVATE_KEY|^#\d+$/i.test(text)) score += 8;
        return score;
      }

      function scheduleAxiomTokenDetailWalletSidecar(nativeWalletControl) {
        window.setTimeout(() => mountAxiomTokenDetailWalletSidecar(nativeWalletControl), 0);
        window.setTimeout(() => mountAxiomTokenDetailWalletSidecar(nativeWalletControl), 80);
      }

      function scheduleAxiomTokenDetailWalletMenuPresenceCheck() {
        window.setTimeout(() => {
          const nativeWalletControl = axiomTokenDetailWalletControl;
          if (isAxiomTokenDetailTrenchOnlyMode()) {
            if (!(nativeWalletControl instanceof HTMLElement) || !document.contains(nativeWalletControl)) {
              document.querySelectorAll("[data-trench-tools-token-detail-wallet-menu]").forEach((element) => element.remove());
              document.removeEventListener("mousedown", handleAxiomTokenDetailWalletMenuOutsideClick, true);
            }
            return;
          }
          if (
            !(nativeWalletControl instanceof HTMLElement) ||
            !(findAxiomTokenDetailNativeWalletMenu(nativeWalletControl) instanceof HTMLElement)
          ) {
            document.querySelectorAll("[data-trench-tools-token-detail-wallet-menu]").forEach((element) => element.remove());
            document.removeEventListener("mousedown", handleAxiomTokenDetailWalletMenuOutsideClick, true);
          }
        }, 80);
      }

      function mountAxiomTokenDetailWalletSidecar(nativeWalletControl) {
        if (isAxiomTokenDetailAxiomOnlyMode()) {
          document.querySelectorAll("[data-trench-tools-token-detail-wallet-menu]").forEach((element) => element.remove());
          document.removeEventListener("mousedown", handleAxiomTokenDetailWalletMenuOutsideClick, true);
          return;
        }
        if (isAxiomTokenDetailTrenchOnlyMode()) {
          mountAxiomTokenDetailStandaloneWalletMenu(nativeWalletControl);
          return;
        }
        if (!(nativeWalletControl instanceof HTMLElement) || !document.contains(nativeWalletControl)) {
          cleanupAxiomTokenDetailWalletSelector();
          return;
        }
        const nativeMenu = findAxiomTokenDetailNativeWalletMenu(nativeWalletControl);
        if (!(nativeMenu instanceof HTMLElement)) {
          document.querySelectorAll("[data-trench-tools-token-detail-wallet-menu]").forEach((element) => element.remove());
          return;
        }
        const options = axiomTokenDetailWalletOptions();
        if (!options.length) {
          return;
        }
        const active = resolveAxiomTokenDetailWalletSelection();
        let menu = document.querySelector("[data-trench-tools-token-detail-wallet-menu]");
        if (!(menu instanceof HTMLElement)) {
          menu = document.createElement("div");
          menu.setAttribute("data-trench-tools-token-detail-wallet-menu", "true");
          document.body.appendChild(menu);
        }
        syncAxiomTokenDetailWalletMenuStyle(menu, nativeMenu);
        menu.innerHTML = "";
        menu.appendChild(buildAxiomTokenDetailWalletMenuContent(options, active, nativeWalletControl));
        positionAxiomTokenDetailWalletMenu(menu, nativeMenu);
        document.addEventListener("mousedown", handleAxiomTokenDetailWalletMenuOutsideClick, true);
      }

      function toggleAxiomTokenDetailStandaloneWalletMenu(nativeWalletControl) {
        const existing = document.querySelector("[data-trench-tools-token-detail-wallet-menu]");
        if (existing instanceof HTMLElement) {
          existing.remove();
          document.removeEventListener("mousedown", handleAxiomTokenDetailWalletMenuOutsideClick, true);
          return;
        }
        mountAxiomTokenDetailStandaloneWalletMenu(nativeWalletControl);
      }

      function refreshAxiomTokenDetailOpenWalletMenu() {
        const menu = document.querySelector("[data-trench-tools-token-detail-wallet-menu]");
        const nativeWalletControl = axiomTokenDetailWalletControl;
        if (!(menu instanceof HTMLElement) || !(nativeWalletControl instanceof HTMLElement) || !document.contains(nativeWalletControl)) {
          return;
        }
        if (isAxiomTokenDetailTrenchOnlyMode()) {
          mountAxiomTokenDetailStandaloneWalletMenu(nativeWalletControl);
        } else {
          mountAxiomTokenDetailWalletSidecar(nativeWalletControl);
        }
      }

      function mountAxiomTokenDetailStandaloneWalletMenu(nativeWalletControl) {
        if (!(nativeWalletControl instanceof HTMLElement) || !document.contains(nativeWalletControl)) {
          cleanupAxiomTokenDetailWalletSelector();
          return;
        }
        const options = axiomTokenDetailWalletOptions();
        if (!options.length) {
          return;
        }
        const active = resolveAxiomTokenDetailWalletSelection();
        let menu = document.querySelector("[data-trench-tools-token-detail-wallet-menu]");
        if (!(menu instanceof HTMLElement)) {
          menu = document.createElement("div");
          menu.setAttribute("data-trench-tools-token-detail-wallet-menu", "true");
          document.body.appendChild(menu);
        }
        syncAxiomTokenDetailStandaloneWalletMenuStyle(menu, nativeWalletControl);
        menu.innerHTML = "";
        menu.appendChild(buildAxiomTokenDetailWalletMenuContent(options, active, nativeWalletControl));
        positionAxiomTokenDetailStandaloneWalletMenu(menu, nativeWalletControl);
        document.addEventListener("mousedown", handleAxiomTokenDetailWalletMenuOutsideClick, true);
      }

      function findAxiomTokenDetailNativeWalletMenu(nativeWalletControl) {
        const controlRect = nativeWalletControl.getBoundingClientRect();
        const candidates = Array.from(document.querySelectorAll("body div, body [role='menu'], body [role='listbox']"))
          .filter((element) => element instanceof HTMLElement)
          .filter((element) => {
            if (
              element.hasAttribute("data-trench-tools-token-detail-wallet-menu") ||
              element.closest("[data-trench-tools-token-detail-wallet-menu]") ||
              element.hasAttribute("data-trench-tools-token-detail-wallet-trigger") ||
              element.contains(nativeWalletControl) ||
              nativeWalletControl.contains(element)
            ) {
              return false;
            }
            const rect = element.getBoundingClientRect();
            const maxMenuHeight = Math.max(520, window.innerHeight - 16);
            if (rect.width < 80 || rect.height < 28 || rect.width > 420 || rect.height > maxMenuHeight) {
              return false;
            }
            const className = String(element.className || "");
            const text = String(element.textContent || "").replace(/\s+/g, " ").trim();
            if (className.includes("primary-wallet")) {
              return false;
            }
            if (
              !/(wallet|SOLANA_PRIVATE_KEY|#\d+|Select All|Select All with Balance)/i.test(text) &&
              !/(wallet|shadow-dropdown|popover|dropdown|menu|select|radix)/i.test(className)
            ) {
              return false;
            }
            const computed = window.getComputedStyle?.(element);
            if (
              !/(absolute|fixed)/.test(String(computed?.position || "")) &&
              !/(popover|dropdown|wallet|menu|select|radix)/i.test(className)
            ) {
              return false;
            }
            if (/(Buy|Sell|AMOUNT|Adv\.?\s*strat|Instant)/i.test(text) && !/wallet/i.test(className)) {
              return false;
            }
            return rect.bottom >= controlRect.top - 16 &&
              rect.top <= controlRect.bottom + 260 &&
              rect.right >= controlRect.left - 240 &&
              rect.left <= controlRect.right + 420;
          });
        return candidates
          .sort((left, right) =>
            axiomTokenDetailNativeWalletMenuScore(right, controlRect) -
            axiomTokenDetailNativeWalletMenuScore(left, controlRect)
          )[0] || null;
      }

      function axiomTokenDetailNativeWalletMenuScore(element, controlRect) {
        const rect = element.getBoundingClientRect();
        const text = String(element.textContent || "").replace(/\s+/g, " ").trim();
        const className = String(element.className || "");
        const computed = window.getComputedStyle?.(element);
        const visibleChildren = Array.from(element.children).filter((child) => {
          if (!(child instanceof HTMLElement)) {
            return false;
          }
          const childRect = child.getBoundingClientRect();
          return childRect.width > 0 && childRect.height > 0;
        }).length;
        const walletRows = element.querySelectorAll?.(".primary-wallet").length || 0;
        let score = 0;
        if (/(popover|dropdown|wallet|menu|select|radix)/i.test(className)) score += 30;
        if (/(absolute|fixed)/.test(String(computed?.position || ""))) score += 20;
        score += Math.min(walletRows * 12, 36);
        if (/SOLANA_PRIVATE_KEY|#\d+/i.test(text)) score += 20;
        if (/wallet/i.test(text)) score += 10;
        score += Math.min(visibleChildren * 8, 40);
        if (visibleChildren === 0) score -= 20;
        score -= Math.abs(rect.top - controlRect.bottom);
        score -= Math.abs(rect.left - controlRect.left) / 2;
        return score;
      }

      function syncAxiomTokenDetailWalletMenuStyle(menu, nativeMenu) {
        menu.className = String(nativeMenu.className || "");
        menu.classList.add("trench-tools-axiom-token-detail-wallet-menu");
        const computed = window.getComputedStyle?.(nativeMenu);
        const rect = nativeMenu.getBoundingClientRect();
        const maxHeight = Math.max(220, Math.min(Math.round(rect.height || 320), window.innerHeight - 16));
        if (!computed) {
          return;
        }
        Object.assign(menu.style, {
          background: `linear-gradient(0deg, rgba(238, 167, 237, 0.06), rgba(238, 167, 237, 0.06)), ${computed.backgroundColor || "#212121"}`,
          backgroundColor: computed.backgroundColor,
          border: "1px solid rgba(238, 167, 237, 0.28)",
          borderRadius: computed.borderRadius,
          boxShadow: computed.boxShadow && computed.boxShadow !== "none"
            ? `${computed.boxShadow}, 0 0 0 1px rgba(238, 167, 237, 0.06)`
            : "0 8px 24px rgba(0, 0, 0, 0.35), 0 0 0 1px rgba(238, 167, 237, 0.10)",
          color: computed.color,
          display: "block",
          font: computed.font,
          height: "auto",
          maxHeight: `${maxHeight}px`,
          minHeight: "0",
          opacity: "1",
          overflow: "hidden",
          padding: "0",
          position: "fixed",
          transition: "none",
          transform: "none",
          animation: "none",
          zIndex: computed.zIndex === "auto" ? "2147483647" : computed.zIndex
        });
      }

      function syncAxiomTokenDetailStandaloneWalletMenuStyle(menu, nativeWalletControl) {
        menu.className = "trench-tools-axiom-token-detail-wallet-menu shadow-dropdown";
        const computed = window.getComputedStyle?.(nativeWalletControl);
        const maxHeight = Math.max(260, Math.min(520, window.innerHeight - 16));
        Object.assign(menu.style, {
          background: "linear-gradient(0deg, rgba(238, 167, 237, 0.06), rgba(238, 167, 237, 0.06)), rgb(33, 33, 33)",
          backgroundColor: "rgb(33, 33, 33)",
          border: "1px solid rgba(238, 167, 237, 0.28)",
          borderRadius: "12px",
          boxShadow: "0 8px 24px rgba(0, 0, 0, 0.35), 0 0 0 1px rgba(238, 167, 237, 0.10)",
          color: computed?.color || "rgb(255, 255, 255)",
          display: "block",
          font: computed?.font || "",
          height: "auto",
          maxHeight: `${maxHeight}px`,
          minHeight: "0",
          opacity: "1",
          overflow: "hidden",
          padding: "0",
          position: "fixed",
          transform: "none",
          zIndex: "2147483647"
        });
      }

      function buildAxiomTokenDetailWalletMenuContent(options, active, nativeWalletControl) {
        const selectedKeys = new Set(active?.manualWalletKeys || []);
        const wrapper = document.createElement("div");
        wrapper.className = "flex flex-col gap-[16px] w-[348px]";

        const outer = document.createElement("div");
        outer.className = "flex flex-col  gap-[16px] ";
        const body = document.createElement("div");
        body.className = "flex flex-col ";
        const holderOptions = options.filter((option) => Number(option.tokenBalanceValue || 0) > 0);
        const nonHolderOptions = options.filter((option) => !(Number(option.tokenBalanceValue || 0) > 0));
        if (holderOptions.length) {
          body.append(
            buildAxiomTokenDetailWalletHolderHeader(holderOptions, selectedKeys, nativeWalletControl),
            buildAxiomTokenDetailWalletMenuList(holderOptions, selectedKeys, nativeWalletControl)
          );
        }
        if (nonHolderOptions.length || !holderOptions.length) {
          body.append(
            buildAxiomTokenDetailWalletMenuHeader(nonHolderOptions, selectedKeys, nativeWalletControl, holderOptions.length > 0),
            buildAxiomTokenDetailWalletMenuList(nonHolderOptions, selectedKeys, nativeWalletControl)
          );
        }
        outer.appendChild(body);
        wrapper.appendChild(outer);
        return wrapper;
      }

      function buildAxiomTokenDetailWalletHolderHeader(options, selectedKeys, nativeWalletControl) {
        const header = document.createElement("div");
        header.className = "flex flex-row items-center justify-start gap-[8px] px-[12px] py-[8px]";
        const holderKeys = options.map((option) => option.id);
        const allSelected = holderKeys.length > 0 && holderKeys.every((key) => selectedKeys.has(key));
        header.append(
          buildAxiomTokenDetailWalletActionButton(allSelected ? "Unselect All" : "Select All", false, () => {
            const nextKeys = allSelected
              ? Array.from(selectedKeys).filter((key) => !holderKeys.includes(key))
              : Array.from(new Set([...selectedKeys, ...holderKeys]));
            saveAxiomTokenDetailWalletKeys(nextKeys);
            mountAxiomTokenDetailWalletSidecar(nativeWalletControl);
          }),
          buildAxiomTokenDetailWalletActionButtonWithIcon("Consolidate", false, () => {
            runAxiomTokenDetailWalletDistribution("consolidate", {}, nativeWalletControl);
          }, "ri-node-tree"),
          buildAxiomTokenDetailWalletActionButtonWithIcon("Split Tokens", false, () => {
            runAxiomTokenDetailWalletDistribution("split", {
              sourceWalletKeys: Array.from(selectedKeys).filter((key) => holderKeys.includes(key))
            }, nativeWalletControl);
          }, "ri-node-tree")
        );
        return header;
      }

      function buildAxiomTokenDetailWalletMenuHeader(options, selectedKeys, nativeWalletControl, separated = false) {
        const header = document.createElement("div");
        header.className = separated
          ? "flex flex-row items-center justify-between gap-[4px] border-t border-secondaryStroke px-[12px] py-[8px]"
          : "flex flex-row items-center justify-between gap-[4px] px-[12px] py-[8px]";

        const actions = document.createElement("div");
        actions.className = "flex flex-row items-center gap-[8px]";
        const allWalletKeys = options.map((option) => option.id);
        const balanceWalletKeys = options
          .filter((option) => Number(option.balanceValue || 0) > 0)
          .map((option) => option.id);
        const allSelected = allWalletKeys.length > 0 && allWalletKeys.every((key) => selectedKeys.has(key));
        const balanceSelected = balanceWalletKeys.length > 0 && balanceWalletKeys.every((key) => selectedKeys.has(key));
        actions.append(
          buildAxiomTokenDetailWalletActionButton(allSelected ? "Unselect All" : "Select All", allWalletKeys.length === 0, () => {
            const nextKeys = allSelected
              ? Array.from(selectedKeys).filter((key) => !allWalletKeys.includes(key))
              : Array.from(new Set([...selectedKeys, ...allWalletKeys]));
            saveAxiomTokenDetailWalletKeys(nextKeys);
            mountAxiomTokenDetailWalletSidecar(nativeWalletControl);
          }),
          buildAxiomTokenDetailWalletActionButton("Select All with Balance", balanceWalletKeys.length === 0, () => {
            const nextKeys = balanceSelected
              ? Array.from(selectedKeys).filter((key) => !balanceWalletKeys.includes(key))
              : Array.from(new Set([...selectedKeys, ...balanceWalletKeys]));
            saveAxiomTokenDetailWalletKeys(nextKeys);
            mountAxiomTokenDetailWalletSidecar(nativeWalletControl);
          })
        );

        const settings = document.createElement("button");
        settings.type = "button";
        settings.className = "group flex h-[24px] w-[24px] items-center justify-center rounded-[4px] transition-colors duration-150 ease-in-out hover:bg-secondaryStroke/20";
        settings.tabIndex = -1;
        const icon = document.createElement("i");
        icon.className = "ri-settings-3-line text-[13px] text-textTertiary transition-colors duration-150 ease-in-out group-hover:text-textSecondary";
        settings.appendChild(icon);
        settings.addEventListener("click", (event) => {
          event.preventDefault();
          event.stopPropagation();
        });

        header.append(actions, settings);
        return header;
      }

      function buildAxiomTokenDetailWalletActionButton(label, disabled, onClick) {
        return buildAxiomTokenDetailWalletActionButtonWithIcon(label, disabled, onClick, "");
      }

      function buildAxiomTokenDetailWalletActionButtonWithIcon(label, disabled, onClick, iconClass = "") {
        const outer = document.createElement("span");
        outer.className = "contents";
        const button = document.createElement("button");
        button.type = "button";
        button.disabled = Boolean(disabled);
        button.className = disabled
          ? "duration-125 group flex h-[24px] cursor-not-allowed flex-row items-center justify-start gap-[4px] rounded-full border-[1px] border-secondaryStroke/20 bg-secondaryStroke/30 px-[7px] text-[12px] font-medium leading-[16px] text-textPrimary opacity-50 transition-colors ease-in-out"
          : "group text-textPrimary flex flex-row gap-[4px] text-[12px] leading-[16px] font-medium justify-start items-center rounded-full px-[7px] h-[24px] disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:bg-transparent hover:border-transparent border-[1px] bg-secondaryStroke/30 border-secondaryStroke/20 hover:bg-secondaryStroke/60 transition-colors duration-125 ease-in-out";
        if (iconClass) {
          const icon = document.createElement("i");
          icon.className = `${iconClass} text-[12px] text-textTertiary`;
          button.appendChild(icon);
        }
        const text = document.createElement("span");
        text.className = "text-[12px] font-medium leading-[16px] text-textPrimary";
        text.textContent = label;
        button.appendChild(text);
        button.addEventListener("mousedown", (event) => {
          event.preventDefault();
          event.stopPropagation();
        });
        button.addEventListener("click", (event) => {
          event.preventDefault();
          event.stopPropagation();
          onClick?.();
        });
        outer.appendChild(button);
        return outer;
      }

      function runAxiomTokenDetailWalletDistribution(action, extraPayload = {}, nativeWalletControl = axiomTokenDetailWalletControl) {
        if (typeof helpers.handleTokenDistributionRequest !== "function") {
          helpers.showToast?.("Token distribution is unavailable here.", "error");
          return;
        }
        const payload = {
          ...axiomTokenDetailWalletSelectionPreferences(),
          ...extraPayload
        };
        void helpers.handleTokenDistributionRequest(action, payload, { persistPreferences: false })
          .catch((error) => helpers.showToast?.(error?.message || "Token distribution failed.", "error"));
      }

      function buildAxiomTokenDetailWalletMenuList(options, selectedKeys, nativeWalletControl) {
        const list = document.createElement("div");
        list.className = "flex flex-col border-t border-secondaryStroke  max-h-[252px] overflow-y-auto";
        options.forEach((option) => {
          list.appendChild(buildAxiomTokenDetailWalletMenuItem(option, selectedKeys, nativeWalletControl));
        });
        return list;
      }

      function buildAxiomTokenDetailWalletMenuItem(option, selectedKeys, nativeWalletControl) {
        const item = document.createElement("div");
        item.className = "group flex cursor-pointer flex-row items-center justify-start hover:bg-secondaryStroke/10";
        const active = selectedKeys.has(option.id);
        item.toggleAttribute("data-active", active);
        item.setAttribute("aria-selected", active ? "true" : "false");

        const checkbox = document.createElement("span");
        checkbox.className = "flex flex-row items-start gap-[0px] p-[16px] pr-[16px]";
        const toggleWrap = document.createElement("div");
        toggleWrap.className = "";
        const toggle = document.createElement("div");
        toggle.className = "inline-flex h-[16px] flex-row  items-center justify-start cursor-pointer";
        const box = document.createElement("div");
        box.className = "border-[1px] border-secondaryStroke flex h-[16px] w-[16px] flex-row items-center justify-center p-[2px] rounded-[4px] cursor-pointer";
        const inner = document.createElement("div");
        inner.className = active ? "h-[10px] w-[10px] bg-primaryBlue rounded-[1px]" : "h-[10px] w-[10px] bg-transparent rounded-[1px]";
        box.appendChild(inner);
        toggle.appendChild(box);
        toggleWrap.appendChild(toggle);
        checkbox.appendChild(toggleWrap);

        const content = document.createElement("div");
        content.className = "flex h-[56px] flex-1 flex-row gap-[0px] border-b-[1px] border-secondaryStroke/50 pl-[0px] items-center justify-start";

        const identity = document.createElement("div");
        identity.className = "flex min-w-[100px] flex-1 flex-col items-start justify-start gap-[4px] pr-[16px]";
        const name = document.createElement("span");
        name.className = active
          ? "text-[rgb(247,147,26)] flex items-center gap-[4px] text-nowrap text-[14px] font-medium leading-[18px]"
          : "text-textPrimary flex items-center gap-[4px] text-nowrap text-[14px] font-medium leading-[18px]";
        name.textContent = option.label;
        const meta = document.createElement("div");
        meta.className = "flex flex-row gap-[6px]";
        meta.appendChild(buildAxiomTokenDetailWalletAddressButton(option.addressLabel || option.id, option.addressValue || option.id));
        identity.append(name, meta);

        const balanceColumn = document.createElement("div");
        balanceColumn.className = "flex flex-1 flex-row items-center justify-end gap-[0px] pr-[8px]";
        balanceColumn.appendChild(buildAxiomTokenDetailWalletBalancePill(option.balanceLabel || "0"));

        const countColumn = document.createElement("div");
        countColumn.className = "flex flex-1 flex-row items-center justify-end gap-[0px] pr-[16px]";
        countColumn.appendChild(buildAxiomTokenDetailWalletTokenBalancePill(option.tokenBalanceLabel || "0"));

        content.append(identity, balanceColumn, countColumn);
        item.append(checkbox, content);
        item.addEventListener("mousedown", (event) => {
          event.preventDefault();
          event.stopPropagation();
        });
        item.addEventListener("click", (event) => {
          event.preventDefault();
          event.stopPropagation();
          const nextKeys = new Set(selectedKeys);
          if (nextKeys.has(option.id)) {
            nextKeys.delete(option.id);
          } else {
            nextKeys.add(option.id);
          }
          saveAxiomTokenDetailWalletKeys(Array.from(nextKeys));
          mountAxiomTokenDetailWalletSidecar(nativeWalletControl);
        });
        return item;
      }

      function buildAxiomTokenDetailWalletAddressButton(label, address) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "flex cursor-pointer flex-row gap-[4px] text-textTertiary transition-colors duration-[125ms] ease-in-out hover:text-textSecondary";
        button.setAttribute("aria-label", "Copy wallet address");
        const text = document.createElement("span");
        text.className = "text-[12px] font-medium leading-[16px]";
        text.textContent = label;
        const icon = document.createElement("i");
        icon.className = "ri-file-copy-line text-[12px] font-medium leading-[16px]";
        button.append(text, icon);
        button.addEventListener("mousedown", (event) => {
          event.preventDefault();
          event.stopPropagation();
        });
        button.addEventListener("click", (event) => {
          event.preventDefault();
          event.stopPropagation();
          void copyAxiomTokenDetailWalletAddress(address);
        });
        return button;
      }

      async function copyAxiomTokenDetailWalletAddress(address) {
        const value = String(address || "").trim();
        if (!value) {
          helpers.showToast?.("Wallet address unavailable.", "error");
          return;
        }
        try {
          await writeAxiomTokenDetailClipboardText(value);
          helpers.showToast?.("Address copied.", "info");
        } catch (_error) {
          helpers.showToast?.("Failed to copy address.", "error");
        }
      }

      async function writeAxiomTokenDetailClipboardText(value) {
        if (navigator.clipboard?.writeText) {
          try {
            await navigator.clipboard.writeText(value);
            return;
          } catch (_error) {
          }
        }
        const textarea = document.createElement("textarea");
        textarea.value = value;
        textarea.setAttribute("readonly", "true");
        Object.assign(textarea.style, {
          left: "-9999px",
          opacity: "0",
          position: "fixed",
          top: "0"
        });
        document.body.appendChild(textarea);
        textarea.select();
        const copied = document.execCommand?.("copy");
        textarea.remove();
        if (!copied) {
          throw new Error("Clipboard copy failed.");
        }
      }

      function buildAxiomTokenDetailWalletBalancePill(label) {
        const outer = document.createElement("span");
        outer.className = "contents";
        const pill = document.createElement("div");
        pill.className = "flex h-[26px] flex-row items-center justify-end gap-[4px] rounded-full border border-secondaryStroke/50 pl-[6px] pr-[6px]";
        const sol = buildAxiomTokenDetailSolIcon();
        const text = document.createElement("span");
        text.className = "text-[12px] font-normal leading-[16px] text-textSecondary";
        text.textContent = label;
        pill.append(sol, text);
        outer.appendChild(pill);
        return outer;
      }

      function buildAxiomTokenDetailWalletTokenBalancePill(label) {
        const outer = document.createElement("span");
        outer.className = "contents";
        const pill = document.createElement("div");
        pill.className = "flex h-[26px] flex-row items-center justify-end gap-[4px] rounded-full border border-secondaryStroke/50 pl-[7px] pr-[6px]";
        const tokenIcon = buildAxiomTokenDetailBaseTokenIcon();
        const text = document.createElement("span");
        text.className = "text-[12px] font-normal leading-[16px] text-textSecondary";
        text.textContent = label;
        pill.append(tokenIcon, text);
        outer.appendChild(pill);
        return outer;
      }

      function buildAxiomTokenDetailBaseTokenIcon() {
        const src = resolveAxiomTokenDetailBaseTokenImageUrl();
        if (src) {
          const image = document.createElement("img");
          image.src = src;
          image.alt = "";
          image.className = "h-[16px] w-[16px] rounded-full";
          return image;
        }
        const fallback = document.createElement("div");
        fallback.className = "h-[13px] w-[13px] rounded-[4px] bg-textTertiary";
        return fallback;
      }

      function resolveAxiomTokenDetailBaseTokenImageUrl() {
        const contextImage = String(
          helpers.state.tokenContext?.image ||
          helpers.state.tokenContext?.imageUrl ||
          helpers.state.tokenContext?.logo ||
          helpers.state.tokenContext?.logoURI ||
          helpers.state.panelTokenContext?.image ||
          helpers.state.panelTokenContext?.imageUrl ||
          helpers.state.panelTokenContext?.logo ||
          helpers.state.panelTokenContext?.logoURI ||
          ""
        ).trim();
        if (contextImage && !isAxiomTokenDetailKnownNonTokenIcon(contextImage)) {
          return contextImage;
        }
        const images = axiomTokenDetailBaseTokenImageRoots()
          .flatMap((root) => Array.from(root.querySelectorAll("img[src]")))
          .map((image) => image instanceof HTMLImageElement ? image : null)
          .filter(Boolean)
          .filter((image, index, list) => list.indexOf(image) === index)
          .filter(isAxiomTokenDetailBaseTokenImageCandidate)
          .sort((left, right) => axiomTokenDetailBaseTokenImageScore(right) - axiomTokenDetailBaseTokenImageScore(left));
        return images[0]?.src || "";
      }

      function axiomTokenDetailBaseTokenImageRoots() {
        const roots = [];
        const anchor = findAxiomTokenDetailHeaderActionAnchor();
        let current = anchor instanceof HTMLElement ? anchor : null;
        for (let depth = 0; current instanceof HTMLElement && depth < 5; depth += 1) {
          const rect = current.getBoundingClientRect();
          if (
            rect.width > 0 &&
            rect.height > 0 &&
            rect.top >= 0 &&
            rect.top < 300 &&
            rect.height <= 260 &&
            rect.width <= Math.min(window.innerWidth, 900)
          ) {
            roots.push(current);
          }
          current = current.parentElement;
        }
        return roots;
      }

      function isAxiomTokenDetailBaseTokenImageCandidate(image) {
        if (!(image instanceof HTMLImageElement)) {
          return false;
        }
        const rect = image.getBoundingClientRect();
        const src = String(image.src || "");
        const alt = String(image.alt || "");
        return rect.width >= 14 &&
          rect.height >= 14 &&
          rect.width <= 96 &&
          rect.height <= 96 &&
          !image.closest("div#instant-trade") &&
          !image.closest("[data-trench-tools-token-detail-wallet-menu]") &&
          !image.closest("[data-trench-tools-token-detail-action-inline]") &&
          !isAxiomTokenDetailKnownNonTokenIcon(src, alt);
      }

      function isAxiomTokenDetailKnownNonTokenIcon(src, alt = "") {
        const value = `${src} ${alt}`.toLowerCase();
        return /chrome-extension:|solana-mark|sol-fill|usd1-mark|usdc|btc-fill|eth-fill|funding-logos|bags\.svg|bundle-|trench-tools|dexscreener|vamp|modal-close|recipient-|copy|remove/.test(value);
      }

      function axiomTokenDetailBaseTokenImageScore(image) {
        if (!(image instanceof HTMLImageElement)) {
          return 0;
        }
        const rect = image.getBoundingClientRect();
        const src = String(image.src || "");
        const alt = String(image.alt || "");
        const text = String(image.closest("a, div, section, main")?.textContent || "").replace(/\s+/g, " ").trim();
        let score = 0;
        if (/axiomtrading.*\.(webp|png|jpg|jpeg)/i.test(src)) score += 40;
        if (/pump|bags|moonshot/i.test(src)) score += 8;
        if (alt && !/sol|usdc|btc|eth|vamp|dexscreener|trench/i.test(alt)) score += 20;
        if (/migrated|token|holders|top traders|pair|audit/i.test(text)) score += 10;
        if (rect.top < window.innerHeight * 0.45) score += 8;
        score -= Math.abs(rect.width - rect.height);
        score -= Math.max(0, rect.width - 40) / 4;
        return score;
      }

      function buildAxiomTokenDetailSolIcon() {
        const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
        svg.setAttribute("viewBox", "0 0 14 11");
        svg.setAttribute("aria-hidden", "true");
        svg.setAttribute("focusable", "false");
        svg.setAttribute("class", "h-[11px] w-[14px] text-textSecondary");
        [
          "M2.6 0 L13.6 0 L11.4 2.8 L0.4 2.8 Z",
          "M0.4 4.1 L11.4 4.1 L13.6 6.9 L2.6 6.9 Z",
          "M2.6 8.2 L13.6 8.2 L11.4 11 L0.4 11 Z"
        ].forEach((pathData) => {
          const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
          path.setAttribute("d", pathData);
          path.setAttribute("fill", "currentColor");
          svg.appendChild(path);
        });
        return svg;
      }

      function axiomTokenDetailWalletOptions() {
        const wallets = Array.isArray(helpers.state.bootstrap?.wallets)
          ? helpers.state.bootstrap.wallets
          : [];
        const walletStatusByKey = new Map();
        (Array.isArray(helpers.state.walletStatus?.wallets) ? helpers.state.walletStatus.wallets : [])
          .forEach((wallet) => {
            [wallet?.key, wallet?.envKey]
              .map((key) => String(key || "").trim())
              .filter(Boolean)
              .forEach((key) => walletStatusByKey.set(key, wallet));
          });
        return wallets
          .filter((wallet) => wallet?.enabled !== false)
          .map((wallet, index) => {
            const id = String(wallet?.key || "").trim();
            const status = walletStatusByKey.get(id);
            const balanceValue = Number(status?.balanceSol ?? wallet?.balanceSol ?? wallet?.solBalance ?? wallet?.balance);
            const tokenBalanceValue = Number(
              status?.tokenBalance ??
              status?.mintBalanceUi ??
              status?.mintBalance ??
              status?.holdingAmount ??
              wallet?.tokenBalance ??
              wallet?.mintBalanceUi ??
              wallet?.mintBalance ??
              wallet?.holdingAmount
            );
            return {
              type: "wallet",
              id,
              label: formatAxiomTokenDetailWalletLabel(wallet?.label || wallet?.key || `Wallet ${index + 1}`),
              addressLabel: axiomTokenDetailWalletAddressLabel(wallet, status, wallet?.key),
              addressValue: axiomTokenDetailWalletAddressValue(wallet, status),
              balanceLabel: formatAxiomTokenDetailWalletBalance(balanceValue),
              balanceValue: Number.isFinite(balanceValue) ? balanceValue : 0,
              tokenBalanceLabel: formatAxiomTokenDetailTokenBalance(tokenBalanceValue),
              tokenBalanceValue: Number.isFinite(tokenBalanceValue) ? tokenBalanceValue : 0
            };
          })
          .filter((option) => option.id && option.label)
          .map((option) => ({
            ...option,
            value: `wallet:${option.id}`
          }));
      }

      function resolveAxiomTokenDetailWalletSelection() {
        const options = axiomTokenDetailWalletOptions();
        if (!options.length) {
          return null;
        }
        const optionKeys = new Set(options.map((option) => option.id));
        const storedSelectionValue = readAxiomTokenDetailWalletSelectionValue();
        if (storedSelectionValue.startsWith("manual:")) {
          const manualWalletKeys = axiomTokenDetailWalletKeysFromStoredValue(storedSelectionValue, optionKeys);
          return {
            type: "manual",
            manualWalletKeys,
            value: `manual:${manualWalletKeys.join(",")}`
          };
        }
        const manualWalletKeys = axiomTokenDetailWalletKeysFromStoredValue(storedSelectionValue, optionKeys);
        if (manualWalletKeys.length) {
          return {
            type: "manual",
            manualWalletKeys,
            value: `manual:${manualWalletKeys.join(",")}`
          };
        }
        const preferenceKeys = axiomTokenDetailWalletKeysFromPreferences(optionKeys);
        const fallbackKeys = preferenceKeys.length ? preferenceKeys : [options[0].id];
        return {
          type: "manual",
          manualWalletKeys: fallbackKeys,
          value: `manual:${fallbackKeys.join(",")}`
        };
      }

      function axiomTokenDetailWalletKeysFromStoredValue(value, knownWalletKeys) {
        const stored = String(value || "").trim();
        if (stored.startsWith("manual:")) {
          return stored
            .slice("manual:".length)
            .split(",")
            .map((key) => String(key || "").trim())
            .filter((key) => key && knownWalletKeys.has(key));
        }
        if (stored.startsWith("wallet:")) {
          const key = stored.slice("wallet:".length).trim();
          return key && knownWalletKeys.has(key) ? [key] : [];
        }
        return [];
      }

      function axiomTokenDetailWalletKeysFromPreferences(knownWalletKeys) {
        const preferences = helpers.state.preferences || {};
        const selectionSource = String(preferences.selectionSource || "").trim().toLowerCase();
        if (selectionSource === "group") {
          const groupId = String(preferences.activeWalletGroupId || preferences.walletGroupId || "").trim();
          const groups = Array.isArray(helpers.state.bootstrap?.walletGroups) ? helpers.state.bootstrap.walletGroups : [];
          const group = groups.find((entry) => String(entry?.id || "").trim() === groupId);
          return (Array.isArray(group?.walletKeys) ? group.walletKeys : [])
            .map((key) => String(key || "").trim())
            .filter((key) => key && knownWalletKeys.has(key));
        }
        return [
          ...(Array.isArray(preferences.manualWalletKeys) ? preferences.manualWalletKeys : []),
          ...(Array.isArray(preferences.walletKeys) ? preferences.walletKeys : []),
          preferences.walletKey
        ]
          .map((key) => String(key || "").trim())
          .filter((key, index, list) => key && knownWalletKeys.has(key) && list.indexOf(key) === index);
      }

      function formatAxiomTokenDetailWalletLabel(value) {
        const label = String(value || "").trim();
        if (!label) {
          return "";
        }
        const genericMatch = label.match(/^SOLANA_PRIVATE_KEY(\d+)?$/i);
        return genericMatch ? `#${genericMatch[1] || "1"}` : label;
      }

      function axiomTokenDetailWalletAddressLabel(wallet, walletStatus, fallback = "") {
        const address = axiomTokenDetailWalletAddressValue(wallet, walletStatus);
        return truncateAxiomTokenDetailWalletValue(address || fallback, 4, 4);
      }

      function axiomTokenDetailWalletAddressValue(wallet, walletStatus) {
        return String(
          wallet?.publicKey ||
          wallet?.address ||
          walletStatus?.publicKey ||
          walletStatus?.address ||
          walletStatus?.walletAddress ||
          ""
        ).trim();
      }

      function truncateAxiomTokenDetailWalletValue(value, start = 4, end = 4) {
        const normalized = String(value || "").trim();
        if (normalized.length <= start + end + 3) {
          return normalized;
        }
        return `${normalized.slice(0, start)}...${normalized.slice(-end)}`;
      }

      function formatAxiomTokenDetailWalletBalance(value) {
        const amount = Number(value);
        if (!Number.isFinite(amount) || amount <= 0) {
          return "0";
        }
        if (amount >= 100) {
          return amount.toFixed(1);
        }
        if (amount >= 1) {
          return amount.toFixed(2);
        }
        return amount.toFixed(3).replace(/0+$/, "").replace(/\.$/, "");
      }

      function formatAxiomTokenDetailTokenBalance(value) {
        const amount = Number(value);
        if (!Number.isFinite(amount) || amount <= 0) {
          return "0";
        }
        if (amount >= 1_000_000_000) {
          return `${(amount / 1_000_000_000).toFixed(2).replace(/\.?0+$/, "")}B`;
        }
        if (amount >= 1_000_000) {
          return `${(amount / 1_000_000).toFixed(2).replace(/\.?0+$/, "")}M`;
        }
        if (amount >= 1_000) {
          return `${(amount / 1_000).toFixed(2).replace(/\.?0+$/, "")}K`;
        }
        if (amount >= 1) {
          return amount.toFixed(2).replace(/\.?0+$/, "");
        }
        return amount.toFixed(4).replace(/0+$/, "").replace(/\.$/, "");
      }

      function readAxiomTokenDetailWalletSelectionValue() {
        try {
          return String(window.localStorage?.getItem(AXIOM_TOKEN_DETAIL_WALLET_SELECTION_KEY) || "").trim();
        } catch (_error) {
          return "";
        }
      }

      function saveAxiomTokenDetailWalletSelectionValue(value) {
        try {
          window.localStorage?.setItem(AXIOM_TOKEN_DETAIL_WALLET_SELECTION_KEY, String(value || ""));
        } catch (_error) {}
      }

      function saveAxiomTokenDetailWalletKeys(walletKeys) {
        const normalized = Array.from(new Set(
          (Array.isArray(walletKeys) ? walletKeys : [])
            .map((key) => String(key || "").trim())
            .filter(Boolean)
        ));
        saveAxiomTokenDetailWalletSelectionValue(`manual:${normalized.join(",")}`);
      }

      function positionAxiomTokenDetailWalletMenu(menu, nativeMenu) {
        if (!(menu instanceof HTMLElement) || !(nativeMenu instanceof HTMLElement)) {
          return;
        }
        const rect = nativeMenu.getBoundingClientRect();
        const width = Math.max(300, Math.min(360, Math.round(rect.width || 350)));
        const gap = 6;
        const rightSideLeft = rect.right + gap;
        const leftSideLeft = rect.left - width - gap;
        const left = rightSideLeft + width <= window.innerWidth - gap
          ? rightSideLeft
          : Math.max(gap, leftSideLeft);
        menu.style.left = `${Math.round(left)}px`;
        menu.style.minWidth = `${width}px`;
        menu.style.top = `${Math.round(rect.top)}px`;
        menu.style.width = `${width}px`;
      }

      function positionAxiomTokenDetailStandaloneWalletMenu(menu, nativeWalletControl) {
        if (!(menu instanceof HTMLElement) || !(nativeWalletControl instanceof HTMLElement)) {
          return;
        }
        const rect = nativeWalletControl.getBoundingClientRect();
        const width = 350;
        const gap = 6;
        const maxHeight = Number.parseFloat(menu.style.maxHeight || "") || 320;
        const left = Math.max(gap, Math.min(Math.round(rect.right - width), window.innerWidth - width - gap));
        const belowTop = rect.bottom + gap;
        const top = belowTop + maxHeight <= window.innerHeight - gap
          ? belowTop
          : Math.max(gap, rect.top - maxHeight - gap);
        menu.style.left = `${Math.round(left)}px`;
        menu.style.minWidth = `${width}px`;
        menu.style.top = `${Math.round(top)}px`;
        menu.style.width = `${width}px`;
      }

      function handleAxiomTokenDetailWalletMenuOutsideClick(event) {
        const menu = document.querySelector("[data-trench-tools-token-detail-wallet-menu]");
        const nativeWalletControl = axiomTokenDetailWalletControl;
        const nativeMenu = nativeWalletControl instanceof HTMLElement
          ? findAxiomTokenDetailNativeWalletMenu(nativeWalletControl)
          : null;
        const target = event.target;
        if (
          target instanceof Node &&
          ((menu instanceof HTMLElement && menu.contains(target)) ||
            (nativeMenu instanceof HTMLElement && nativeMenu.contains(target)) ||
            (nativeWalletControl instanceof HTMLElement && nativeWalletControl.contains(target)))
        ) {
          if (
            (nativeMenu instanceof HTMLElement && nativeMenu.contains(target)) ||
            (nativeWalletControl instanceof HTMLElement && nativeWalletControl.contains(target))
          ) {
            scheduleAxiomTokenDetailWalletMenuPresenceCheck();
          }
          return;
        }
        menu?.remove();
        document.removeEventListener("mousedown", handleAxiomTokenDetailWalletMenuOutsideClick, true);
      }

      function axiomTokenDetailWalletSelectionPreferences() {
        const selection = resolveAxiomTokenDetailWalletSelection();
        const base = { ...(helpers.state.preferences || {}) };
        if (!selection) {
          return base;
        }
        const manualWalletKeys = Array.from(new Set((selection.manualWalletKeys || []).filter(Boolean)));
        return {
          ...base,
          selectionSource: "manual",
          activeWalletGroupId: "",
          manualWalletKeys,
          selectionTarget: {
            type: manualWalletKeys.length === 1 ? "single_wallet" : "wallet_list",
            walletKey: manualWalletKeys[0] || "",
            walletGroupId: "",
            walletKeys: manualWalletKeys
          },
          selectionMode: manualWalletKeys.length === 1 ? "single_wallet" : "wallet_list",
          walletKey: manualWalletKeys[0] || "",
          walletGroupId: "",
          walletKeys: manualWalletKeys
        };
      }

      function areAxiomTokenDetailPresetButtonsCurrent(row, existingButtons, actions, route) {
        if (!existingButtons.length || existingButtons.length !== actions.length) {
          return false;
        }
        const children = Array.from(row.children);
        const appendedButtons = children.slice(children.length - existingButtons.length);
        if (!existingButtons.every((button, index) => button === appendedButtons[index])) {
          return false;
        }
        return existingButtons.every((button, index) => {
          const action = actions[index]?.action;
          const rowIndex = actions[index]?.rowIndex;
          const amountMatches = button.getAttribute("data-amount") === action?.amount;
          const sellUnitMatches = action?.side !== "sell" ||
            String(button.getAttribute("data-sell-unit") || "") === String(action?.sellUnit || "");
          const editableMatches = button.hasAttribute("data-trench-tools-token-detail-editable") === Boolean(action?.editable);
          return action &&
            button.getAttribute("data-route-key") === route.routeKey &&
            String(button.getAttribute("data-mint") || "") === route.tokenMint &&
            String(button.getAttribute("data-pair") || "") === route.companionPair &&
            button.getAttribute("data-row-index") === String(rowIndex) &&
            button.getAttribute("data-side") === action.side &&
            amountMatches &&
            sellUnitMatches &&
            editableMatches;
        });
      }

      function ensureAxiomTokenDetailBloomCloneStyles() {
        if (document.getElementById("trench-tools-axiom-token-detail-bloom-clone-style")) {
          return;
        }
        const style = document.createElement("style");
        style.id = "trench-tools-axiom-token-detail-bloom-clone-style";
        style.textContent = `
          [data-trench-tools-token-detail-native-hidden="true"] {
            display: none !important;
          }
          [data-trench-tools-token-detail-inline-hidden="true"] {
            display: none !important;
          }
          [data-trench-tools-token-detail-buy-currency-hidden="true"] {
            display: none !important;
          }
          [data-trench-tools-token-detail-option-row] {
            display: flex !important;
            align-items: stretch !important;
            justify-content: flex-start !important;
            gap: 4px !important;
            min-width: 0;
          }
          [data-trench-tools-token-detail-option-native] {
            align-items: center;
            flex: 1 1 0 !important;
            min-width: 0;
            overflow: hidden;
            width: auto !important;
          }
          [data-trench-tools-token-detail-option-action] {
            flex: 0 0 auto;
          }
          [data-trench-tools-token-detail-setting-row] {
            align-items: center;
            color: #ffffff;
            display: flex;
            flex: 1 1 0;
            font-size: 10px;
            gap: 0;
            justify-content: space-between;
            line-height: 1;
            min-height: 22px;
            min-width: 0;
            padding: 1px 0;
          }
          [data-trench-tools-token-detail-setting-item] {
            align-items: center;
            display: inline-flex;
            gap: 2px;
            justify-content: center;
            min-width: 0;
            text-align: center;
          }
          [data-trench-tools-token-detail-setting-icon] {
            display: inline-flex;
            flex: 0 0 auto;
            filter: brightness(0) invert(1);
            height: 10px;
            object-fit: contain;
            opacity: 0.78;
            width: 10px;
          }
          [data-trench-tools-token-detail-setting-value] {
            color: #ffffff;
            font-size: 10px;
            font-weight: 600;
            line-height: 1;
            min-width: 0;
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
          }
          [data-trench-tools-token-detail-setting-separator] {
            background: rgba(255, 255, 255, 0.1);
            flex: 0 0 1px;
            height: 12px;
            margin: 0 2px;
            width: 1px;
          }
          div#instant-trade[data-trench-tools-token-detail-button-mode="dual"] [data-trench-tools-token-detail-option-native] {
            flex: 0 1 calc(50% - 52px) !important;
          }
          div#instant-trade[data-trench-tools-token-detail-button-mode="dual"] [data-trench-tools-token-detail-setting-row] {
            flex: 1 1 calc(50% - 2px);
          }
          div#instant-trade[data-trench-tools-token-detail-button-mode="trench"] [data-trench-tools-token-detail-setting-row] {
            width: 100%;
          }
          div#instant-trade[data-trench-tools-token-detail-button-mode="trench"] [data-trench-tools-token-detail-option-native],
          div#instant-trade[data-trench-tools-token-detail-button-mode="trench"] [data-trench-tools-token-detail-option-action] {
            display: none !important;
          }
          div#instant-trade[data-trench-tools-token-detail-button-mode="axiom"] [data-trench-tools-token-detail-setting-row] {
            display: none !important;
          }
          .trench-tools-axiom-token-detail-bloom-clone {
            border: 1px solid #EEA7ED;
            color: #EEA7ED;
            z-index: 1000;
          }
          .trench-tools-axiom-token-detail-bloom-clone:hover {
            background-color: #EEA7ED;
            color: hsl(var(--twc-grey-900) / var(--twc-grey-900-opacity, var(--tw-text-opacity)));
          }
          [data-trench-tools-token-detail-compact-toggle] {
            --trench-tools-axiom-toggle-color: #34D399;
            align-items: center;
            background: rgba(52, 211, 153, 0.08);
            border: 1px solid #34D399;
            border-radius: 4px;
            color: #34D399;
            cursor: pointer;
            display: inline-flex;
            flex-shrink: 0;
            font-size: 9px;
            font-weight: 700;
            height: 22px;
            justify-content: center;
            letter-spacing: -0.02em;
            line-height: 1;
            margin-left: -2px;
            min-width: 22px;
            padding: 0;
            width: 22px;
            transition: background-color 0.15s ease, color 0.15s ease, opacity 0.15s ease;
          }
          [data-trench-tools-token-detail-compact-toggle]:hover {
            background-color: var(--trench-tools-axiom-toggle-color);
            color: hsl(var(--twc-grey-900) / var(--twc-grey-900-opacity, var(--tw-text-opacity)));
          }
          [data-trench-tools-token-detail-compact-toggle][data-trench-tools-token-detail-button-mode="axiom"] {
            --trench-tools-axiom-toggle-color: #3B82F6;
            background: rgba(59, 130, 246, 0.08);
            border-color: #3B82F6;
            color: #3B82F6;
          }
          [data-trench-tools-token-detail-compact-toggle][data-trench-tools-token-detail-button-mode="trench"] {
            --trench-tools-axiom-toggle-color: #EEA7ED;
            background: rgba(238, 167, 237, 0.08);
            border-color: #EEA7ED;
            color: #EEA7ED;
          }
          [data-trench-tools-token-detail-compact-toggle][data-trench-tools-token-detail-button-mode="dual"] {
            --trench-tools-axiom-toggle-color: #34D399;
            background: rgba(52, 211, 153, 0.08);
            border-color: #34D399;
            color: #34D399;
          }
          [data-trench-tools-token-detail-compact-toggle][data-trench-tools-token-detail-button-mode]:hover {
            background: var(--trench-tools-axiom-toggle-color);
            color: hsl(var(--twc-grey-900) / var(--twc-grey-900-opacity, var(--tw-text-opacity)));
          }
          .trench-tools-axiom-token-detail-wallet-menu {
            box-sizing: border-box;
            position: fixed;
            z-index: 2147483647;
          }
        `;
        document.head.appendChild(style);
      }

      function ensureAxiomTokenDetailPresetSettingRows(instantTrade = document.querySelector("div#instant-trade")) {
        if (!(instantTrade instanceof HTMLElement)) {
          return;
        }
        const optionRows = findAxiomTokenDetailOptionRows(instantTrade);
        Object.entries(optionRows).forEach(([side, row]) => {
          if (row instanceof HTMLElement) {
            ensureAxiomTokenDetailPresetSettingRow(row, side);
          }
        });
      }

      function clearAxiomTokenDetailPresetSettingRows(instantTrade = document.querySelector("div#instant-trade")) {
        if (!(instantTrade instanceof HTMLElement)) {
          return;
        }
        instantTrade.querySelectorAll("[data-trench-tools-token-detail-setting-row]").forEach((element) => element.remove());
        instantTrade.querySelectorAll("[data-trench-tools-token-detail-option-row]").forEach((element) => {
          element.removeAttribute("data-trench-tools-token-detail-option-row");
          element.removeAttribute("data-trench-tools-token-detail-option-side");
        });
        instantTrade.querySelectorAll("[data-trench-tools-token-detail-option-native]").forEach((element) => {
          element.removeAttribute("data-trench-tools-token-detail-option-native");
        });
        instantTrade.querySelectorAll("[data-trench-tools-token-detail-option-action]").forEach((element) => {
          element.removeAttribute("data-trench-tools-token-detail-option-action");
        });
      }

      function findAxiomTokenDetailOptionRows(instantTrade) {
        const container = instantTrade?.querySelector?.(".buy-click-container");
        if (!(container instanceof HTMLElement)) {
          return {};
        }
        const rows = Array.from(container.children).filter((element) => element instanceof HTMLElement);
        return {
          buy: rows.find((row) => {
            const text = String(row.textContent || "");
            return /Adv\.?|Advanced/i.test(text);
          }) || null,
          sell: rows.find((row) => {
            const text = String(row.textContent || "");
            return /Sell\s*Init\.?/i.test(text);
          }) || null
        };
      }

      function ensureAxiomTokenDetailPresetSettingRow(row, side) {
        if (!(row instanceof HTMLElement)) {
          return;
        }
        row.setAttribute("data-trench-tools-token-detail-option-row", "true");
        row.setAttribute("data-trench-tools-token-detail-option-side", side);
        const existingSettingRow = row.querySelector(":scope > [data-trench-tools-token-detail-setting-row]");
        const nativeChildren = Array.from(row.children)
          .filter((element) =>
            element instanceof HTMLElement &&
            !element.hasAttribute("data-trench-tools-token-detail-setting-row")
          );
        nativeChildren.forEach((element, index) => {
          element.toggleAttribute("data-trench-tools-token-detail-option-native", index === 0);
          element.toggleAttribute("data-trench-tools-token-detail-option-action", index > 0);
        });

        const signature = axiomTokenDetailPresetSettingSignature(side);
        const settingRow = existingSettingRow instanceof HTMLElement
          ? existingSettingRow
          : document.createElement("div");
        if (settingRow.getAttribute("data-trench-tools-token-detail-setting-signature") !== signature) {
          renderAxiomTokenDetailPresetSettingRow(settingRow, side);
          settingRow.setAttribute("data-trench-tools-token-detail-setting-signature", signature);
        }
        settingRow.setAttribute("data-trench-tools-token-detail-setting-row", "true");
        settingRow.setAttribute("data-trench-tools-token-detail-setting-side", side);
        if (settingRow.parentElement !== row) {
          row.appendChild(settingRow);
        }
      }

      function axiomTokenDetailPresetSettingSignature(side) {
        return JSON.stringify(buildAxiomTokenDetailPresetSettingItems(side));
      }

      function renderAxiomTokenDetailPresetSettingRow(row, side) {
        row.textContent = "";
        const items = buildAxiomTokenDetailPresetSettingItems(side);
        items.forEach((item, index) => {
          row.appendChild(buildAxiomTokenDetailPresetSettingItem(item));
          if (index < items.length - 1) {
            const separator = document.createElement("span");
            separator.setAttribute("data-trench-tools-token-detail-setting-separator", "true");
            separator.setAttribute("aria-hidden", "true");
            row.appendChild(separator);
          }
        });
      }

      function buildAxiomTokenDetailPresetSettingItems(side) {
        const preset = getAxiomTokenDetailActivePreset();
        const mevMode = side === "sell" ? getAxiomTokenDetailSellMevMode(preset) : getAxiomTokenDetailBuyMevMode(preset);
        const slippage = side === "sell" ? getAxiomTokenDetailSellSlippagePercent(preset) : getAxiomTokenDetailBuySlippagePercent(preset);
        const fee = side === "sell" ? preset?.sellFeeSol : preset?.buyFeeSol;
        const tip = side === "sell" ? preset?.sellTipSol : preset?.buyTipSol;
        const autoFee = side === "sell" ? preset?.sellAutoTipEnabled : preset?.buyAutoTipEnabled;
        const feeValue = fee ? String(fee) : "";
        const tipValue = tip ? String(tip) : "";
        return [
          {
            icon: safeRuntimeGetUrl("assets/lighting-icon.png"),
            label: "Auto fee",
            value: autoFee ? "On" : "Off"
          },
          {
            icon: safeRuntimeGetUrl("assets/fuel-icon.png"),
            label: "Priority Fee",
            value: feeValue ? formatAxiomTokenDetailCompactDecimalValue(feeValue) : "Preset",
            rawValue: feeValue || "Preset"
          },
          {
            icon: safeRuntimeGetUrl("assets/tip-icon.png"),
            label: "Tip",
            value: tipValue ? formatAxiomTokenDetailCompactDecimalValue(tipValue) : "Preset",
            rawValue: tipValue || "Preset"
          },
          {
            icon: safeRuntimeGetUrl("assets/slippage-icon.png"),
            label: "Slippage",
            value: slippage ? `${slippage}%` : "Preset"
          },
          {
            icon: axiomTokenDetailMevIconUrl(mevMode),
            label: "MEV mode",
            value: formatAxiomTokenDetailMevSettingValue(mevMode)
          }
        ];
      }

      function buildAxiomTokenDetailPresetSettingItem(item) {
        const element = document.createElement("span");
        element.setAttribute("data-trench-tools-token-detail-setting-item", "true");
        const fullValue = item.rawValue || item.value;
        element.setAttribute("data-trench-tools-token-detail-setting-tooltip-label", item.label);
        element.setAttribute("aria-label", `${item.label}: ${fullValue}`);
        if (item.icon) {
          const icon = document.createElement("img");
          icon.src = item.icon;
          icon.alt = "";
          icon.setAttribute("aria-hidden", "true");
          icon.setAttribute("data-trench-tools-token-detail-setting-icon", "true");
          element.appendChild(icon);
        }
        const value = document.createElement("span");
        value.setAttribute("data-trench-tools-token-detail-setting-value", "true");
        value.textContent = item.value;
        element.appendChild(value);
        element.addEventListener("mouseenter", () => showAxiomTokenDetailPresetSettingTooltip(element));
        element.addEventListener("focus", () => showAxiomTokenDetailPresetSettingTooltip(element));
        element.addEventListener("mouseleave", hideAxiomTokenDetailPresetSettingTooltip);
        element.addEventListener("blur", hideAxiomTokenDetailPresetSettingTooltip);
        return element;
      }

      function showAxiomTokenDetailPresetSettingTooltip(anchor) {
        if (!(anchor instanceof HTMLElement)) {
          return;
        }
        const label = String(anchor.getAttribute("data-trench-tools-token-detail-setting-tooltip-label") || "").trim();
        if (!label) {
          return;
        }
        let tooltip = document.querySelector("[data-trench-tools-token-detail-setting-tooltip]");
        if (!(tooltip instanceof HTMLElement)) {
          tooltip = document.createElement("div");
          tooltip.className = "fixed translate-x-[-50%] translate-y-[-100%] z-[99999] pointer-events-none";
          tooltip.setAttribute("data-trench-tools-token-detail-setting-tooltip", "true");
          const shell = document.createElement("div");
          shell.className = "relative";
          const body = document.createElement("div");
          body.className = "border-borderSubtle bg-backgroundTertiary border rounded-[4px] py-[4px] text-xs overflow-y-auto text-center text-[11px] font-normal leading-[16px] text-textSecondary shadow-lg";
          body.setAttribute("data-trench-tools-token-detail-setting-tooltip-body", "true");
          shell.appendChild(body);
          tooltip.appendChild(shell);
          document.body.appendChild(tooltip);
        }
        const body = tooltip.querySelector("[data-trench-tools-token-detail-setting-tooltip-body]");
        if (body instanceof HTMLElement) {
          body.textContent = label;
        }
        const rect = anchor.getBoundingClientRect();
        tooltip.style.left = `${Math.round(rect.left + rect.width / 2)}px`;
        tooltip.style.top = `${Math.round(rect.top - 8)}px`;
      }

      function hideAxiomTokenDetailPresetSettingTooltip() {
        document.querySelector("[data-trench-tools-token-detail-setting-tooltip]")?.remove();
      }

      function getAxiomTokenDetailActivePreset() {
        const presets = Array.isArray(helpers.state.bootstrap?.presets) ? helpers.state.bootstrap.presets : [];
        const presetId = String(helpers.state.preferences?.presetId || "").trim();
        return presets.find((preset) => String(preset?.id || "").trim() === presetId) || presets[0] || null;
      }

      function getAxiomTokenDetailBuySlippagePercent(preset) {
        return String(preset?.buySlippagePercent ?? preset?.slippagePercent ?? "").trim();
      }

      function getAxiomTokenDetailSellSlippagePercent(preset) {
        return String(preset?.sellSlippagePercent ?? preset?.slippagePercent ?? "").trim();
      }

      function getAxiomTokenDetailBuyMevMode(preset) {
        return String(preset?.buyMevMode ?? preset?.mevMode ?? "off").trim() || "off";
      }

      function getAxiomTokenDetailSellMevMode(preset) {
        return String(preset?.sellMevMode ?? preset?.mevMode ?? "off").trim() || "off";
      }

      function axiomTokenDetailMevIconUrl(mode) {
        const normalizedMode = String(mode || "off").trim().toLowerCase();
        if (normalizedMode === "off") {
          return safeRuntimeGetUrl("assets/NOMEV-icon.png");
        }
        if (normalizedMode === "reduced") {
          return safeRuntimeGetUrl("assets/MEV-icon.png");
        }
        return safeRuntimeGetUrl("assets/mevsecure-icon.png");
      }

      function formatAxiomTokenDetailSettingLabel(value) {
        return String(value || "")
          .replace(/_/g, " ")
          .replace(/\b\w/g, (character) => character.toUpperCase());
      }

      function formatAxiomTokenDetailMevSettingValue(value) {
        const mode = String(value || "off").trim().toLowerCase();
        if (mode === "reduced") {
          return "Red.";
        }
        if (mode === "secure") {
          return "Sec.";
        }
        if (mode === "off") {
          return "Off";
        }
        return formatAxiomTokenDetailSettingLabel(mode);
      }

      function formatAxiomTokenDetailCompactDecimalValue(value) {
        const raw = String(value || "").trim();
        const match = raw.match(/^([+-]?)0\.(0{3,})([1-9][0-9]*)$/);
        if (!match) {
          return raw;
        }
        return `${match[1]}0.0${toAxiomTokenDetailSubscriptDigits(match[2].length)}${match[3]}`;
      }

      function toAxiomTokenDetailSubscriptDigits(value) {
        const digits = String(value);
        const subscriptDigits = {
          0: "₀",
          1: "₁",
          2: "₂",
          3: "₃",
          4: "₄",
          5: "₅",
          6: "₆",
          7: "₇",
          8: "₈",
          9: "₉"
        };
        return digits.replace(/\d/g, (digit) => subscriptDigits[digit] || digit);
      }

      function syncAxiomTokenDetailBuyCurrencyRowVisibility(instantTrade, hidden) {
        if (!(instantTrade instanceof HTMLElement)) {
          return;
        }
        findAxiomTokenDetailBuyCurrencyRows(instantTrade).forEach((row) => {
          if (hidden) {
            row.setAttribute("data-trench-tools-token-detail-buy-currency-hidden", "true");
          } else {
            row.removeAttribute("data-trench-tools-token-detail-buy-currency-hidden");
          }
        });
      }

      function findAxiomTokenDetailBuyCurrencyRows(instantTrade) {
        const container = instantTrade?.querySelector?.(".buy-click-container");
        if (!(container instanceof HTMLElement)) {
          return [];
        }
        return Array.from(container.querySelectorAll("div")).map((element) => {
          if (!(element instanceof HTMLElement)) {
            return null;
          }
          const compactText = String(element.textContent || "").replace(/\s+/g, "");
          if (!compactText.includes("Buy") || !compactText.includes("SOL") || !compactText.includes("USDC") || !compactText.includes("uSOL")) {
            return null;
          }
          const className = String(element.className || "");
          if (!className.includes("justify-between")) {
            return null;
          }
          const buyGroup = Array.from(element.children).find((child) => {
            const childText = String(child.textContent || "").replace(/\s+/g, "");
            return childText.includes("Buy") && childText.includes("SOL") && childText.includes("USDC");
          });
          if (!(buyGroup instanceof HTMLElement)) {
            return null;
          }
          return Array.from(buyGroup.children).find((child) => {
            const childText = String(child.textContent || "").replace(/\s+/g, "");
            return child instanceof HTMLElement &&
              !childText.includes("Buy") &&
              childText.includes("SOL") &&
              childText.includes("USDC") &&
              childText.includes("uSOL");
          }) || null;
        }).filter((element) => element instanceof HTMLElement);
      }

      function findAxiomTokenDetailInstantTradePanel() {
        const instantTrade = document.querySelector("div#instant-trade");
        return instantTrade instanceof HTMLElement ? instantTrade : null;
      }

      function findAxiomTokenDetailControlRows() {
        const instantTrade = findAxiomTokenDetailInstantTradePanel();
        const candidates = instantTrade instanceof HTMLElement
          ? Array.from(instantTrade.querySelectorAll("div.flex-row.w-full"))
          : [];
        return candidates.filter((row) =>
          row instanceof HTMLElement &&
          isAxiomTokenDetailControlRow(row)
        );
      }

      function findAxiomTokenDetailObserverTargets() {
        const targets = [];
        const instantTrade = findAxiomTokenDetailInstantTradePanel();
        if (instantTrade instanceof HTMLElement) {
          targets.push(instantTrade.parentElement || instantTrade);
        }
        const hardpanel = findAxiomTokenDetailHardpanelRoot();
        if (hardpanel instanceof HTMLElement) {
          targets.push(hardpanel.parentElement || hardpanel);
        }
        return targets.filter((target, index, list) =>
          target instanceof HTMLElement && list.indexOf(target) === index
        );
      }

      function isAxiomTokenDetailControlRow(row) {
        const controls = findAxiomTokenDetailNativeControls(row);
        if (controls.length < 2) {
          return false;
        }
        if (row.closest("div#instant-trade")) {
          return true;
        }
        const rowClass = String(row.className || "");
        if (/rounded-b|border-x|border-b/.test(rowClass)) {
          return true;
        }
        const panelText = String(row.closest("div.relative")?.textContent || row.parentElement?.textContent || "");
        return /Buy\s*Sell|AMOUNT|Adv\.?\s*strat|Instant/i.test(panelText);
      }

      function findAxiomTokenDetailNativeControls(row, rowSide = null) {
        if (!(row instanceof HTMLElement)) {
          return [];
        }
        const insideInstantTrade = Boolean(row.closest("div#instant-trade"));
        return Array.from(row.children).filter((element) =>
          element instanceof HTMLElement &&
          !element.hasAttribute("data-trench-tools-token-detail-inline") &&
          !element.hasAttribute("data-trench-tools-token-detail-preload-inline") &&
          readAxiomTokenDetailAction(element, row, rowSide) &&
          (
            isAxiomTokenDetailRoundedControl(element) ||
            isAxiomTokenDetailEditablePresetControl(element) ||
            (
              !insideInstantTrade &&
              element.matches("div.cursor-pointer") &&
              element.getBoundingClientRect().width > 0 &&
              element.getBoundingClientRect().height > 0
            )
          )
        );
      }

      function isAxiomTokenDetailRoundedControl(element) {
        if (!(element instanceof HTMLElement)) {
          return false;
        }
        return element.matches("div.rounded-full") && !String(element.className || "").includes("group/wallets");
      }

      function readAxiomTokenDetailAction(control, row = null, rowSide = null) {
        const editableInput = findAxiomTokenDetailEditablePresetInput(control);
        const text = String(
          editableInput instanceof HTMLInputElement
            ? editableInput.value || editableInput.getAttribute("value") || editableInput.placeholder || ""
            : control?.textContent || ""
        ).replace(/\s+/g, "").trim();
        if (!text && !(editableInput instanceof HTMLInputElement)) {
          return null;
        }
        const amount = text.replace("%", "").trim();
        if ((!amount || !Number.isFinite(Number(amount))) && !(editableInput instanceof HTMLInputElement)) {
          return null;
        }
        const side = resolveAxiomTokenDetailActionSide(control, row, rowSide, text);
        return {
          side,
          amount,
          editable: editableInput instanceof HTMLInputElement,
          sellUnit: side === "sell" ? resolveAxiomTokenDetailSellUnit(control, row, text) : ""
        };
      }

      function isAxiomTokenDetailEditablePresetControl(element) {
        return findAxiomTokenDetailEditablePresetInput(element) instanceof HTMLInputElement;
      }

      function findAxiomTokenDetailEditablePresetInput(element) {
        if (!(element instanceof HTMLElement)) {
          return null;
        }
        if (element instanceof HTMLInputElement && isAxiomTokenDetailEditablePresetInput(element)) {
          return element;
        }
        const inputs = Array.from(element.querySelectorAll("input"))
          .filter((input) => input instanceof HTMLInputElement);
        return inputs.find(isAxiomTokenDetailEditablePresetInput) || null;
      }

      function isAxiomTokenDetailEditablePresetInput(input) {
        if (!(input instanceof HTMLInputElement)) {
          return false;
        }
        const type = String(input.type || "text").toLowerCase();
        return type === "text" || type === "number" || type === "tel" || type === "";
      }

      function resolveAxiomTokenDetailSellUnit(control, row = null, text = "") {
        if (String(text || "").includes("%")) {
          return "percent";
        }
        const rowElement = row instanceof HTMLElement ? row : control?.parentElement;
        const rowUnit = resolveAxiomTokenDetailSellUnitFromContainer(rowElement);
        if (rowUnit) {
          return rowUnit;
        }
        const panel = control?.closest?.("div#instant-trade");
        return resolveAxiomTokenDetailSellUnitFromContainer(panel) || "sol";
      }

      function resolveAxiomTokenDetailSellUnitFromContainer(container) {
        if (!(container instanceof HTMLElement)) {
          return "";
        }
        const controls = Array.from(container.querySelectorAll("[role='button'], button, div"))
          .filter((element) =>
            element instanceof HTMLElement &&
            !element.hasAttribute("data-trench-tools-token-detail-inline") &&
            !element.hasAttribute("data-trench-tools-token-detail-preload-inline") &&
            isVisibleAxiomNode(element)
          );
        if (controls.some((element) => String(element.textContent || "").replace(/\s+/g, "").trim() === "%")) {
          return "percent";
        }
        if (controls.some((element) => element.querySelector?.("img[src*='sol-fill'], img[alt='SOL']"))) {
          return "sol";
        }
        return "";
      }

      function resolveAxiomTokenDetailActionSide(control, row, rowSide, text = "") {
        const explicitSide = inferAxiomTokenDetailSideFromElement(control) ||
          inferAxiomTokenDetailSideFromElement(row) ||
          normalizeAxiomTokenDetailSide(rowSide);
        if (explicitSide) {
          return explicitSide;
        }
        return String(text || "").includes("%") ? "sell" : "buy";
      }

      function resolveAxiomTokenDetailRowSide(row, rowIndex, rows) {
        const explicitSide = inferAxiomTokenDetailSideFromElement(row);
        if (explicitSide) {
          return explicitSide;
        }
        if (
          row instanceof HTMLElement &&
          row.closest("div#instant-trade") &&
          Array.isArray(rows) &&
          rows.length >= 2
        ) {
          return rowIndex % 2 === 0 ? "buy" : "sell";
        }
        return null;
      }

      function inferAxiomTokenDetailSideFromElement(element) {
        if (!(element instanceof HTMLElement)) {
          return null;
        }
        const values = [
          element.getAttribute("data-side"),
          element.getAttribute("data-action"),
          element.getAttribute("aria-label"),
          element.getAttribute("title"),
          element.className,
          element.textContent
        ];
        const normalized = values
          .map((value) => String(value || "").toLowerCase())
          .join(" ");
        if (/\b(sell|decrease)\b|bg-decrease|text-decrease|border-decrease/.test(normalized)) {
          return "sell";
        }
        if (/\b(buy|increase)\b|bg-increase|text-increase|border-increase/.test(normalized)) {
          return "buy";
        }
        return null;
      }

      function normalizeAxiomTokenDetailSide(value) {
        const side = String(value || "").trim().toLowerCase();
        return side === "buy" || side === "sell" ? side : null;
      }

      function isVisibleAxiomNode(element) {
        if (!(element instanceof HTMLElement)) {
          return false;
        }
        const rect = element.getBoundingClientRect();
        return rect.width > 0 && rect.height > 0;
      }

      function buildAxiomTokenDetailCloneButton(nativeControl, action) {
        const button = nativeControl.cloneNode(true);
        button.classList.add("trench-tools-axiom-token-detail-bloom-clone");
        button.setAttribute("data-trench-tools-token-detail-inline", "true");
        button.removeAttribute("data-trench-tools-token-detail-native-control");
        button.removeAttribute("data-trench-tools-token-detail-native-hidden");
        button.setAttribute("data-route-key", action.routeKey);
        button.setAttribute("data-control-index", String(action.index));
        button.setAttribute("data-row-index", String(action.rowIndex));
        button.setAttribute("data-side", action.side);
        button.setAttribute("data-amount", action.amount);
        if (action.editable) {
          button.setAttribute("data-trench-tools-token-detail-editable", "true");
        } else {
          button.removeAttribute("data-trench-tools-token-detail-editable");
        }
        if (action.side === "sell" && action.sellUnit) {
          button.setAttribute("data-sell-unit", action.sellUnit);
        } else {
          button.removeAttribute("data-sell-unit");
        }
        if (action.tokenMint) {
          button.setAttribute("data-mint", action.tokenMint);
        } else {
          button.removeAttribute("data-mint");
        }
        if (action.companionPair) {
          button.setAttribute("data-pair", action.companionPair);
        } else {
          button.removeAttribute("data-pair");
        }
        attachAxiomIntentPrewarm(button, "token_detail", {
          address: action.routeKey,
          mint: action.tokenMint,
          pair: action.companionPair,
          url: window.location.href,
          side: action.side
        });
        nativeControl.style.minWidth = "40px";
        Object.assign(button.style, {
          minWidth: "40px",
          zIndex: "1000"
        });
        if (action.editable) {
          installAxiomTokenDetailEditablePresetBridge(button, nativeControl, action);
        } else {
          installAxiomTokenDetailNativeHoverBridge(button, nativeControl);
        }
        button.addEventListener("click", (event) => {
          if (action.editable) {
            event.preventDefault();
            event.stopPropagation();
            if (!isAxiomTokenDetailEditableEventTarget(event.target)) {
              findAxiomTokenDetailEditablePresetInput(button)?.focus();
            }
            return;
          }
          if (isAxiomTokenDetailEditableEventTarget(event.target)) {
            return;
          }
          event.preventDefault();
          event.stopPropagation();
          const liveRoute = routePayloadFromButton(button, {
            address: action.routeKey,
            mint: action.tokenMint,
            pair: action.companionPair,
            surface: "token_detail",
            url: window.location.href
          });
          if (!liveRoute?.address) {
            helpers.showToast?.("Token not found.", "error");
            return;
          }
          if (!Number.isFinite(Number(action.amount)) || Number(action.amount) <= 0) {
            helpers.showToast?.("Invalid amount.", "error");
            return;
          }
          const payload = {
            ...axiomTokenDetailWalletSelectionPreferences(),
            ...(action.side === "buy"
              ? { buyAmountSol: action.amount }
              : action.sellUnit === "sol"
                ? { sellOutputSol: action.amount }
                : { sellPercent: action.amount })
          };
          void helpers.handleInlineTradeRequest(action.side, liveRoute, "token_detail", payload, window.location.href)
            .catch((error) => helpers.showToast?.(error?.message || "Trade failed.", "error"));
        });
        return button;
      }

      function installAxiomTokenDetailEditablePresetBridge(button, nativeControl, action) {
        if (!(button instanceof HTMLElement) || !(nativeControl instanceof HTMLElement)) {
          return;
        }
        button._trenchAxiomHoverBridgeCleanup?.();
        button._trenchAxiomEditableBridgeCleanup?.();
        const nativeInput = findAxiomTokenDetailEditablePresetInput(nativeControl);
        const cloneInput = findAxiomTokenDetailEditablePresetInput(button);
        if (!(nativeInput instanceof HTMLInputElement) || !(cloneInput instanceof HTMLInputElement)) {
          return;
        }

        cloneInput.value = nativeInput.value;
        button.removeAttribute("id");
        cloneInput.removeAttribute("id");
        button.querySelectorAll("[id]").forEach((element) => element.removeAttribute("id"));

        const stopEditableEvent = (event) => {
          event.stopPropagation();
        };
        const syncCloneFromNative = () => {
          if (cloneInput.value !== nativeInput.value) {
            cloneInput.value = nativeInput.value;
          }
          const value = String(nativeInput.value || "").trim();
          button.setAttribute("data-amount", value);
          action.amount = value;
        };
        const syncNativeFromClone = (event) => {
          event?.stopPropagation?.();
          const value = String(cloneInput.value || "").trim();
          button.setAttribute("data-amount", value);
          setAxiomTokenDetailNativeInputValue(nativeInput, cloneInput.value);
          dispatchAxiomTokenDetailInputEvent(nativeInput, "input");
          if (event?.type === "change") {
            dispatchAxiomTokenDetailInputEvent(nativeInput, "change");
          }
          action.amount = value;
        };

        ["click", "mousedown", "mouseup", "pointerdown", "pointerup", "keydown", "keyup"].forEach((eventType) => {
          cloneInput.addEventListener(eventType, stopEditableEvent);
        });
        cloneInput.addEventListener("input", syncNativeFromClone);
        cloneInput.addEventListener("change", syncNativeFromClone);
        nativeInput.addEventListener("input", syncCloneFromNative);
        nativeInput.addEventListener("change", syncCloneFromNative);

        button._trenchAxiomEditableBridgeCleanup = () => {
          ["click", "mousedown", "mouseup", "pointerdown", "pointerup", "keydown", "keyup"].forEach((eventType) => {
            cloneInput.removeEventListener(eventType, stopEditableEvent);
          });
          cloneInput.removeEventListener("input", syncNativeFromClone);
          cloneInput.removeEventListener("change", syncNativeFromClone);
          nativeInput.removeEventListener("input", syncCloneFromNative);
          nativeInput.removeEventListener("change", syncCloneFromNative);
          delete button._trenchAxiomEditableBridgeCleanup;
        };
      }

      function isAxiomTokenDetailEditableEventTarget(target) {
        return target instanceof HTMLElement &&
          (target.matches("input, textarea, select") || Boolean(target.closest("[contenteditable='true']")));
      }

      function setAxiomTokenDetailNativeInputValue(input, value) {
        if (!(input instanceof HTMLInputElement)) {
          return;
        }
        const descriptor = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value");
        if (descriptor?.set) {
          descriptor.set.call(input, value);
        } else {
          input.value = value;
        }
      }

      function dispatchAxiomTokenDetailInputEvent(input, eventType) {
        try {
          const event = eventType === "input" && typeof InputEvent === "function"
            ? new InputEvent("input", {
              bubbles: true,
              cancelable: true,
              composed: true,
              inputType: "insertReplacementText"
            })
            : new Event(eventType, {
              bubbles: true,
              cancelable: true,
              composed: true
            });
          input.dispatchEvent(event);
        } catch (_error) {
          input.dispatchEvent(new Event(eventType, { bubbles: true }));
        }
      }

      function ensureAxiomTokenDetailFloatingPresetRefreshBridge(instantTrade, route) {
        if (!(instantTrade instanceof HTMLElement)) {
          disconnectAxiomTokenDetailFloatingPresetRefreshBridge();
          return;
        }
        axiomTokenDetailFloatingPresetRefreshRoute = route;
        if (axiomTokenDetailFloatingPresetRefreshRoot !== instantTrade) {
          disconnectAxiomTokenDetailFloatingPresetRefreshBridge();
          axiomTokenDetailFloatingPresetRefreshRoot = instantTrade;
          axiomTokenDetailFloatingPresetRefreshRoute = route;
          axiomTokenDetailFloatingPresetRefreshObserver = new MutationObserver((mutations) => {
            if (!isAxiomTokenDetailFloatingPresetNativeMutation(mutations)) {
              return;
            }
            queueDomOperation("token-detail-floating-presets-refresh", () => {
              if (helpers.state.siteFeatures?.axiom?.instantTrade) {
                mountAxiomTokenDetailQuickButton(axiomTokenDetailFloatingPresetRefreshRoute);
              }
            }, "urgent");
          });
          axiomTokenDetailFloatingPresetRefreshObserver.observe(instantTrade, {
            childList: true,
            characterData: true,
            subtree: true
          });
        }
      }

      function disconnectAxiomTokenDetailFloatingPresetRefreshBridge() {
        axiomTokenDetailFloatingPresetRefreshObserver?.disconnect();
        axiomTokenDetailFloatingPresetRefreshObserver = null;
        axiomTokenDetailFloatingPresetRefreshRoot = null;
        axiomTokenDetailFloatingPresetRefreshRoute = null;
      }

      function isAxiomTokenDetailFloatingPresetNativeMutation(mutations) {
        return mutations.some((mutation) => {
          const target = axiomElementFromMutationNode(mutation.target);
          if (!(target instanceof HTMLElement) || !target.closest("div#instant-trade")) {
            return false;
          }
          if (target.closest("[data-trench-tools-token-detail-inline]")) {
            return false;
          }
          const changedNodes = [...mutation.addedNodes, ...mutation.removedNodes]
            .map(axiomElementFromMutationNode)
            .filter((element) => element instanceof HTMLElement);
          return !changedNodes.length ||
            !changedNodes.every((element) =>
              element.closest("[data-trench-tools-token-detail-inline]")
            );
        });
      }

      function axiomElementFromMutationNode(node) {
        if (node instanceof HTMLElement) {
          return node;
        }
        if (node instanceof Text || node instanceof Comment) {
          return node.parentElement;
        }
        return null;
      }

      function mountAxiomTokenDetailHardpanelManualActions(route) {
        const hardpanel = findAxiomTokenDetailHardpanelRoot();
        const submitButton = hardpanel instanceof HTMLElement
          ? findAxiomTokenDetailHardpanelSubmitButton(hardpanel)
          : null;
        const submitContainer = submitButton?.parentElement;
        const state = hardpanel instanceof HTMLElement ? resolveAxiomHardpanelSideAndAmount(hardpanel) : null;
        if (
          !(hardpanel instanceof HTMLElement) ||
          !(submitButton instanceof HTMLElement) ||
          !(submitContainer instanceof HTMLElement) ||
          !state?.side
        ) {
          cleanupAxiomTokenDetailHardpanelManualActions();
          return;
        }

        const existingWrapper = submitContainer.nextElementSibling;
        if (
          existingWrapper instanceof HTMLElement &&
          existingWrapper.hasAttribute("data-trench-tools-token-detail-hardpanel-action-wrapper")
        ) {
          const existingButton = existingWrapper.querySelector("[data-trench-tools-token-detail-hardpanel-action]");
          if (isAxiomTokenDetailHardpanelActionCurrent(existingButton, route, state)) {
            syncAxiomTokenDetailHardpanelActionButton(existingButton, submitButton, state.side);
            ensureAxiomTokenDetailHardpanelRefreshBridge(hardpanel, route);
            return;
          }
          existingWrapper.remove();
        } else {
          cleanupAxiomTokenDetailHardpanelManualActions();
        }

        const wrapper = document.createElement("div");
        wrapper.setAttribute("data-trench-tools-token-detail-hardpanel-action-wrapper", "true");
        wrapper.className = "flex min-h-[4px] w-full flex-row items-center justify-center px-[16px] pb-[16px]";

        const actionButton = buildAxiomTokenDetailHardpanelActionButton(submitButton, {
          ...route,
          side: state.side,
          sellUnit: state.sellUnit
        });
        wrapper.appendChild(actionButton);
        submitContainer.insertAdjacentElement("afterend", wrapper);
        ensureAxiomTokenDetailHardpanelRefreshBridge(hardpanel, route);
      }

      function ensureAxiomTokenDetailHardpanelRefreshBridge(hardpanel, route) {
        if (!(hardpanel instanceof HTMLElement)) {
          disconnectAxiomTokenDetailHardpanelRefreshBridge();
          return;
        }
        disconnectAxiomTokenDetailHardpanelRefreshBridge();
        axiomTokenDetailHardpanelRefreshRoot = hardpanel;
        const scheduleRefresh = (event) => {
          if (event?.target instanceof Element && event.target.closest("[data-trench-tools-token-detail-hardpanel-action-wrapper]")) {
            return;
          }
          window.setTimeout(() => {
            if (document.contains(hardpanel)) {
              mountAxiomTokenDetailHardpanelManualActions(route);
            }
          }, 80);
        };
        hardpanel.addEventListener("click", scheduleRefresh, true);
        hardpanel.addEventListener("input", scheduleRefresh, true);
        hardpanel.addEventListener("change", scheduleRefresh, true);
        axiomTokenDetailHardpanelRefreshObserver = new MutationObserver((mutations) => {
          if (mutations.some(isAxiomTokenDetailHardpanelNativeMutation)) {
            scheduleRefresh();
          }
        });
        axiomTokenDetailHardpanelRefreshObserver.observe(hardpanel, {
          attributes: true,
          childList: true,
          characterData: true,
          subtree: true
        });
        axiomTokenDetailHardpanelRefreshCleanup = () => {
          hardpanel.removeEventListener("click", scheduleRefresh, true);
          hardpanel.removeEventListener("input", scheduleRefresh, true);
          hardpanel.removeEventListener("change", scheduleRefresh, true);
        };
      }

      function disconnectAxiomTokenDetailHardpanelRefreshBridge() {
        axiomTokenDetailHardpanelRefreshObserver?.disconnect();
        axiomTokenDetailHardpanelRefreshObserver = null;
        axiomTokenDetailHardpanelRefreshCleanup?.();
        axiomTokenDetailHardpanelRefreshCleanup = null;
        axiomTokenDetailHardpanelRefreshRoot = null;
      }

      function isAxiomTokenDetailHardpanelNativeMutation(mutation) {
        const wrapperSelector = "[data-trench-tools-token-detail-hardpanel-action-wrapper]";
        const targetElement = mutation.target instanceof Element
          ? mutation.target
          : mutation.target?.parentElement;
        if (targetElement instanceof Element && targetElement.closest(wrapperSelector)) {
          return false;
        }
        if (mutation.type === "childList") {
          const changedElements = [...mutation.addedNodes, ...mutation.removedNodes]
            .map((node) =>
              node instanceof Element
                ? node
                : node instanceof Text || node instanceof Comment
                  ? node.parentElement
                  : null
            )
            .filter((element) => element instanceof Element);
          if (
            changedElements.length &&
            changedElements.every((element) =>
              element.matches(wrapperSelector) || element.closest(wrapperSelector)
            )
          ) {
            return false;
          }
        }
        return true;
      }

      function isAxiomTokenDetailHardpanelActionCurrent(button, route, state) {
        return button instanceof HTMLElement &&
          button.getAttribute("data-route-key") === route.routeKey &&
          String(button.getAttribute("data-mint") || "") === route.tokenMint &&
          String(button.getAttribute("data-pair") || "") === route.companionPair &&
          button.getAttribute("data-side") === state.side &&
          String(button.getAttribute("data-sell-unit") || "") === String(state.sellUnit || "");
      }

      function findAxiomTokenDetailHardpanelRoot() {
        const candidates = Array.from(document.querySelectorAll("div.relative"))
          .filter((element) => element instanceof HTMLElement)
          .filter((element) => {
            if (element.closest("div#instant-trade")) {
              return false;
            }
            const rect = element.getBoundingClientRect();
            if (rect.width <= 0 || rect.height <= 0 || rect.width > 440 || rect.height < 180) {
              return false;
            }
            const text = String(element.textContent || "").replace(/\s+/g, "");
            return /BuySell/.test(text) &&
              /AMOUNT/i.test(text) &&
              findAxiomTokenDetailHardpanelAmountInput(element) instanceof HTMLInputElement &&
              findAxiomTokenDetailHardpanelSubmitButton(element) instanceof HTMLElement;
          });
        return candidates
          .sort((left, right) =>
            axiomTokenDetailHardpanelRootScore(right) - axiomTokenDetailHardpanelRootScore(left)
          )[0] || null;
      }

      function axiomTokenDetailHardpanelRootScore(element) {
        const rect = element.getBoundingClientRect();
        const text = String(element.textContent || "");
        let score = 0;
        if (rect.right > window.innerWidth * 0.65) score += 40;
        if (rect.width >= 260 && rect.width <= 360) score += 30;
        if (/\bBuy\b/.test(text) && /\bSell\b/.test(text)) score += 20;
        if (/Advanced Trading Strategy|Sell Init\.?|Add/i.test(text)) score += 10;
        score -= Math.abs(rect.width - 320) / 4;
        return score;
      }

      function findAxiomTokenDetailHardpanelAmountInput(root) {
        if (!(root instanceof HTMLElement)) {
          return null;
        }
        return Array.from(root.querySelectorAll("input"))
          .filter((element) => element instanceof HTMLInputElement)
          .filter(isVisibleAxiomNode)
          .sort((left, right) =>
            axiomTokenDetailHardpanelAmountInputScore(right) -
            axiomTokenDetailHardpanelAmountInputScore(left)
          )[0] || null;
      }

      function axiomTokenDetailHardpanelAmountInputScore(input) {
        const rect = input.getBoundingClientRect();
        const placeholder = String(input.getAttribute("placeholder") || "");
        let score = 0;
        if (/^0(?:\.0*)?$/.test(placeholder)) score += 40;
        if (rect.width >= 120) score += 20;
        if (rect.top < window.innerHeight * 0.45) score += 10;
        return score;
      }

      function findAxiomTokenDetailHardpanelSubmitButton(root) {
        if (!(root instanceof HTMLElement)) {
          return null;
        }
        return Array.from(root.querySelectorAll("button"))
          .filter((element) =>
            element instanceof HTMLElement &&
            !element.hasAttribute("data-trench-tools-token-detail-hardpanel-action") &&
            isVisibleAxiomNode(element)
          )
          .filter((button) => /^(Buy|Sell)\b/i.test(axiomTokenDetailHardpanelButtonText(button)))
          .sort((left, right) =>
            axiomTokenDetailHardpanelSubmitButtonScore(right, root) -
            axiomTokenDetailHardpanelSubmitButtonScore(left, root)
          )[0] || null;
      }

      function axiomTokenDetailHardpanelSubmitButtonScore(button, root) {
        const rect = button.getBoundingClientRect();
        const rootRect = root.getBoundingClientRect();
        const text = axiomTokenDetailHardpanelButtonText(button);
        let score = 0;
        if (/^(Buy|Sell)\s+\S+/i.test(text)) score += 80;
        if (rect.width >= rootRect.width * 0.75) score += 40;
        if (rect.height >= 30) score += 20;
        if (/bg-(increase|decrease)/.test(String(button.className || ""))) score += 20;
        score += Math.min(Math.max(0, rect.top - rootRect.top) / 8, 40);
        return score;
      }

      function axiomTokenDetailHardpanelButtonText(button) {
        return String(button?.textContent || "").replace(/\s+/g, " ").trim();
      }

      function resolveAxiomHardpanelSideAndAmount(root) {
        if (!(root instanceof HTMLElement)) {
          return null;
        }
        const submitButton = findAxiomTokenDetailHardpanelSubmitButton(root);
        const submitText = axiomTokenDetailHardpanelButtonText(submitButton);
        const side = /^Sell\b/i.test(submitText)
          ? "sell"
          : /^Buy\b/i.test(submitText)
            ? "buy"
            : null;
        const input = findAxiomTokenDetailHardpanelAmountInput(root);
        const amount = String(input?.value || "").trim();
        return {
          side,
          amount,
          sellUnit: side === "sell" ? resolveAxiomHardpanelSellUnit(root) : ""
        };
      }

      function resolveAxiomHardpanelSellUnit(root) {
        const nativeUnitControls = Array.from(root.querySelectorAll("[role='button'], button"))
          .filter((element) =>
            element instanceof HTMLElement &&
            !element.hasAttribute("data-trench-tools-token-detail-inline") &&
            !element.closest("[data-trench-tools-token-detail-hardpanel-action-wrapper]") &&
            isVisibleAxiomNode(element)
          );
        return nativeUnitControls.some((element) => axiomTokenDetailHardpanelButtonText(element) === "%")
          ? "percent"
          : "sol";
      }

      function buildAxiomTokenDetailHardpanelActionButton(nativeSubmitButton, action) {
        const button = nativeSubmitButton.cloneNode(false);
        button.type = "button";
        button.setAttribute("data-trench-tools-token-detail-hardpanel-action", "true");
        button.setAttribute("data-route-key", action.routeKey);
        button.setAttribute("data-side", action.side);
        button.setAttribute("data-sell-unit", action.sellUnit || "");
        if (action.tokenMint) {
          button.setAttribute("data-mint", action.tokenMint);
        }
        if (action.companionPair) {
          button.setAttribute("data-pair", action.companionPair);
        }
        syncAxiomTokenDetailHardpanelActionButton(button, nativeSubmitButton, action.side);
        button.addEventListener("mousedown", (event) => {
          event.preventDefault();
          event.stopPropagation();
        });
        button.addEventListener("click", (event) => {
          event.preventDefault();
          event.stopPropagation();
          handleAxiomTokenDetailHardpanelActionClick(button);
        });
        return button;
      }

      function syncAxiomTokenDetailHardpanelActionButton(button, nativeSubmitButton, side) {
        if (!(button instanceof HTMLElement) || !(nativeSubmitButton instanceof HTMLElement)) {
          return;
        }
        button.textContent = "";
        nativeSubmitButton.childNodes.forEach((child) => {
          button.appendChild(child.cloneNode(true));
        });
        if (!button.childNodes.length) {
          const label = document.createElement("span");
          label.className = "text-[14px] font-bold leading-[18px]";
          label.textContent = side === "sell" ? "TT Sell" : "TT Buy";
          button.appendChild(label);
        }
        brandAxiomTokenDetailHardpanelActionContent(button, side);

        const rect = nativeSubmitButton.getBoundingClientRect();
        const computed = window.getComputedStyle?.(nativeSubmitButton);
        Object.assign(button.style, {
          background: "#EEA7ED",
          borderColor: "#EEA7ED",
          borderRadius: computed?.borderRadius || nativeSubmitButton.style.borderRadius || "",
          color: "#090909",
          height: rect.height > 0 ? `${Math.round(rect.height)}px` : nativeSubmitButton.style.height || "",
          maxHeight: rect.height > 0 ? `${Math.round(rect.height)}px` : nativeSubmitButton.style.maxHeight || "",
          minHeight: rect.height > 0 ? `${Math.round(rect.height)}px` : nativeSubmitButton.style.minHeight || "",
          width: "100%"
        });
      }

      function brandAxiomTokenDetailHardpanelActionContent(button, side) {
        const nativeVerb = side === "sell" ? "Sell" : "Buy";
        const brandedVerb = side === "sell" ? "TT Sell" : "TT Buy";
        const walker = document.createTreeWalker(button, NodeFilter.SHOW_TEXT);
        let textNode = walker.nextNode();
        while (textNode) {
          const value = String(textNode.nodeValue || "");
          if (value.includes(nativeVerb)) {
            textNode.nodeValue = value.replace(nativeVerb, brandedVerb);
            return;
          }
          textNode = walker.nextNode();
        }
        const prefix = document.createElement("span");
        prefix.className = "text-[14px] font-bold leading-[18px]";
        prefix.textContent = brandedVerb;
        button.prepend(prefix);
      }

      function handleAxiomTokenDetailHardpanelActionClick(button) {
        const hardpanel = findAxiomTokenDetailHardpanelRoot();
        const state = resolveAxiomHardpanelSideAndAmount(hardpanel);
        const amount = String(state?.amount || "").trim();
        const amountValue = Number(amount);
        if (!state?.side) {
          helpers.showToast?.("Axiom trade panel not found.", "error");
          return;
        }
        if (!Number.isFinite(amountValue) || amountValue <= 0) {
          helpers.showToast?.("Enter a valid amount.", "error");
          return;
        }
        const liveRoute = routePayloadFromButton(button, {
          address: button.getAttribute("data-route-key") || resolveCurrentPageAddress(),
          mint: button.getAttribute("data-mint") || "",
          pair: button.getAttribute("data-pair") || "",
          surface: "token_detail",
          url: window.location.href
        });
        if (!liveRoute?.address) {
          helpers.showToast?.("Token not found.", "error");
          return;
        }
        const payload = {
          ...axiomTokenDetailWalletSelectionPreferences(),
          ...(state.side === "buy"
            ? { buyAmountSol: amount }
            : state.sellUnit === "percent"
              ? { sellPercent: amount }
              : { sellOutputSol: amount })
        };
        void helpers.handleInlineTradeRequest(state.side, liveRoute, "token_detail", payload, window.location.href)
          .catch((error) => helpers.showToast?.(error?.message || "Trade failed.", "error"));
      }

      function installAxiomTokenDetailNativeHoverBridge(button, nativeControl) {
        if (!(button instanceof HTMLElement) || !(nativeControl instanceof HTMLElement)) {
          return;
        }
        button._trenchAxiomHoverBridgeCleanup?.();
        button._trenchAxiomEditableBridgeCleanup?.();
        const forward = (sourceEvent, eventTypes) => {
          if (!(nativeControl instanceof HTMLElement) || !nativeControl.isConnected) {
            return;
          }
          const handledByAxiom = eventTypes.includes("mouseenter")
            ? requestAxiomTokenDetailNativeHover(nativeControl, "enter", sourceEvent) ||
              invokeAxiomTokenDetailNativeReactHandler(nativeControl, "onMouseEnter", sourceEvent)
            : eventTypes.includes("mouseleave")
              ? requestAxiomTokenDetailNativeHover(nativeControl, "leave", sourceEvent) ||
                invokeAxiomTokenDetailNativeReactHandler(nativeControl, "onMouseLeave", sourceEvent)
              : false;
          if (handledByAxiom) {
            return;
          }
          eventTypes.forEach((eventType) => {
            dispatchAxiomTokenDetailNativeHoverEvent(nativeControl, sourceEvent, eventType);
          });
        };
        const handleEnter = (event) => forward(event, ["pointerover", "pointerenter", "mouseover", "mouseenter"]);
        const handleMove = (event) => forward(event, ["pointermove", "mousemove"]);
        const handleLeave = (event) => forward(event, ["pointerout", "pointerleave", "mouseout", "mouseleave"]);
        button.addEventListener("mouseenter", handleEnter);
        button.addEventListener("mousemove", handleMove);
        button.addEventListener("mouseleave", handleLeave);
        button._trenchAxiomHoverBridgeCleanup = () => {
          button.removeEventListener("mouseenter", handleEnter);
          button.removeEventListener("mousemove", handleMove);
          button.removeEventListener("mouseleave", handleLeave);
          delete button._trenchAxiomHoverBridgeCleanup;
        };
      }

      function requestAxiomTokenDetailNativeHover(nativeControl, action, sourceEvent) {
        if (!(nativeControl instanceof HTMLElement)) {
          return false;
        }
        const bridgeEvent = new CustomEvent("trench-tools:axiom-token-detail-native-hover", {
          bubbles: true,
          cancelable: true,
          composed: true,
          detail: { action }
        });
        try {
          nativeControl.dispatchEvent(bridgeEvent);
          return bridgeEvent.defaultPrevented;
        } catch (_error) {
          return false;
        }
      }

      function invokeAxiomTokenDetailNativeReactHandler(nativeControl, handlerName, sourceEvent) {
        if (!(nativeControl instanceof HTMLElement)) {
          return false;
        }
        const reactProps = axiomTokenDetailNativeReactProps(nativeControl);
        const handler = reactProps?.[handlerName];
        if (typeof handler !== "function") {
          return false;
        }
        try {
          handler({
            currentTarget: nativeControl,
            target: nativeControl,
            relatedTarget: sourceEvent?.target instanceof EventTarget ? sourceEvent.target : null,
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

      function axiomTokenDetailNativeReactProps(nativeControl) {
        if (!(nativeControl instanceof HTMLElement)) {
          return null;
        }
        const propsKey = Object.keys(nativeControl).find((key) => key.startsWith("__reactProps$"));
        return propsKey ? nativeControl[propsKey] : null;
      }

      function dispatchAxiomTokenDetailNativeHoverEvent(target, sourceEvent, eventType) {
        if (!(target instanceof HTMLElement)) {
          return;
        }
        const pointerLike = eventType.startsWith("pointer");
        const init = {
          bubbles: true,
          cancelable: true,
          composed: true,
          view: window,
          detail: sourceEvent?.detail || 0,
          screenX: sourceEvent?.screenX || 0,
          screenY: sourceEvent?.screenY || 0,
          clientX: sourceEvent?.clientX || 0,
          clientY: sourceEvent?.clientY || 0,
          ctrlKey: Boolean(sourceEvent?.ctrlKey),
          shiftKey: Boolean(sourceEvent?.shiftKey),
          altKey: Boolean(sourceEvent?.altKey),
          metaKey: Boolean(sourceEvent?.metaKey),
          button: 0,
          buttons: 0,
          relatedTarget: sourceEvent?.target instanceof EventTarget ? sourceEvent.target : null
        };
        try {
          const event = pointerLike && typeof PointerEvent === "function"
            ? new PointerEvent(eventType, {
              ...init,
              pointerId: sourceEvent?.pointerId || 1,
              pointerType: sourceEvent?.pointerType || "mouse",
              isPrimary: sourceEvent?.isPrimary ?? true,
              width: sourceEvent?.width || 1,
              height: sourceEvent?.height || 1,
              pressure: sourceEvent?.pressure || 0
            })
            : new MouseEvent(eventType, init);
          target.dispatchEvent(event);
        } catch (_error) {}
      }

      function handleMutations(mutations) {
        const axiomFeatures = helpers.state.siteFeatures?.axiom || {};
        if (!axiomFeatures.enabled) {
          return true;
        }
        const pageAddress = resolveCurrentPageAddress();
        const surfaceState = getAxiomSurfaceState(pageAddress, axiomFeatures);
        const processPulse = surfaceState.pulse &&
          (
            axiomFeatures.pulseButton ||
            axiomFeatures.pulsePanel ||
            shouldShowAxiomVampIcon("pulse") ||
            shouldShowAxiomDexScreenerIcon("pulse")
          );
        const processLaunchShell = surfaceState.pulse && axiomFeatures.launchdeckInjection;
        const processTokenDetail = surfaceState.tokenDetail;
        const processWatchlist = axiomFeatures.watchlist;
        const processWalletTracker = axiomFeatures.walletTracker;
        const pulseCards = new Set();
        const watchlistAnchors = new Set();
        const walletRows = new Set();
        let tokenDetailDirty = false;
        let launchShellDirty = false;

        const queueTokenDetailMount = (priority = "normal") => {
          queueDomOperation("token-detail", () => {
            const latestPageAddress = resolveCurrentPageAddress();
            if (latestPageAddress && !isPulseUrl(window.location.href)) {
              const tokenDetailRoute =
                buildObservedCandidate(latestPageAddress, "token_detail", window.location.href) || latestPageAddress;
              mountAxiomTokenDetailHeaderActions(tokenDetailRoute);
              if (helpers.state.siteFeatures?.axiom?.instantTrade) {
                mountAxiomTokenDetailQuickButton(tokenDetailRoute);
              }
            }
          }, priority);
        };

        function addMutationTargets(node) {
          const element =
            node instanceof HTMLElement
              ? node
              : node instanceof Text || node instanceof Comment
                ? node.parentElement
                : null;
          if (!(element instanceof HTMLElement)) {
            return;
          }

          if (processLaunchShell && !launchShellDirty) {
            if (isLaunchShellRowElement(element)) {
              launchShellDirty = true;
            } else if (element.querySelectorAll instanceof Function) {
              launchShellDirty = Array.from(element.querySelectorAll(LAUNCH_SHELL_ROW_SELECTOR)).some(
                (row) => isLaunchShellRowElement(row)
              );
            }
          }

          if (processPulse) {
            const closestPulseCard = element.closest(PULSE_CARD_SELECTOR);
            if (closestPulseCard instanceof HTMLElement) {
              pulseCards.add(closestPulseCard);
            }
            const pulseCardFromCopy = element.matches("button.group\\/copy")
              ? findAxiomPulseCardFromCopyButton(element)
              : element.querySelector?.("button.group\\/copy")
                ? findAxiomPulseCardFromCopyButton(element.querySelector("button.group\\/copy"))
                : null;
            if (pulseCardFromCopy instanceof HTMLElement) {
              pulseCards.add(pulseCardFromCopy);
            }
            if (element.matches(PULSE_CARD_SELECTOR)) {
              pulseCards.add(element);
            }
            element.querySelectorAll?.(PULSE_CARD_SELECTOR).forEach((card) => {
              if (card instanceof HTMLElement) {
                pulseCards.add(card);
              }
            });
          }

          if (processWatchlist) {
            const closestAnchor = element.closest("a");
            if (
              closestAnchor instanceof HTMLAnchorElement &&
              (closestAnchor.querySelector("[data-trench-tools-axiom-watchlist-inline]") ||
                closestAnchor.href.includes("/meme/"))
            ) {
              watchlistAnchors.add(closestAnchor);
            }
            if (element instanceof HTMLAnchorElement) {
              watchlistAnchors.add(element);
            }
            element.querySelectorAll?.("a").forEach((anchor) => {
              if (
                anchor instanceof HTMLAnchorElement &&
                (anchor.querySelector("[data-trench-tools-axiom-watchlist-inline]") ||
                  anchor.href.includes("/meme/"))
              ) {
                watchlistAnchors.add(anchor);
              }
            });
          }

          if (processWalletTracker) {
            const closestWalletRow = findClosestAxiomWalletTrackerRow(element);
            if (closestWalletRow instanceof HTMLElement) {
              walletRows.add(closestWalletRow);
            }
            findAxiomWalletTrackerRows(element).forEach((row) => {
              walletRows.add(row);
            });
          }

          if (processTokenDetail) {
            if (element.closest("div#instant-trade") || element.matches("div#instant-trade")) {
              tokenDetailDirty = true;
            } else if (
              element instanceof HTMLButtonElement &&
              /instant trade/i.test(String(element.textContent || ""))
            ) {
              tokenDetailDirty = true;
            } else if (
              element.querySelector instanceof Function &&
              element.querySelector("button") &&
              /instant trade/i.test(String(element.textContent || ""))
            ) {
              tokenDetailDirty = true;
            }
          }
        }

        mutations.forEach((mutation) => {
          addMutationTargets(mutation.target);
          mutation.addedNodes.forEach((node) => addMutationTargets(node));
          mutation.removedNodes.forEach(() => addMutationTargets(mutation.target));
        });

        if (pulseCards.size) {
          requestAxiomPulseMetadataRescan();
          pulseCards.forEach((card) => {
            queueDomOperation(`pulse:${getAxiomPulseCardId(card)}`, () => {
              mountAxiomPulseQuickButtonCard(card);
            }, "urgent");
          });
        }
        if (watchlistAnchors.size) {
          watchlistAnchors.forEach((anchor) => {
            const anchorId = getTrackedNodeId(anchor, "trenchToolsWatchlistAnchorId", "watchlist-anchor");
            queueDomOperation(`watchlist:${anchorId}`, () => {
              reconcileAxiomWatchlistAnchor(anchor);
            }, "urgent");
          });
        }
        if (walletRows.size) {
          walletRows.forEach((row) => {
            const rowId = getTrackedNodeId(row, "trenchToolsWalletRowId", "wallet-row");
            queueDomOperation(`wallet:${rowId}`, () => {
              reconcileAxiomWalletTrackerRow(row);
            }, "urgent");
          });
        }
        if (tokenDetailDirty) {
          queueTokenDetailMount("urgent");
        } else if (processTokenDetail) {
          queueTokenDetailMount();
        }
        if (launchShellDirty) {
          queueDomOperation("pulse-launch-shell", () => {
            mountAxiomLaunchShellControls();
          }, "urgent");
        }

        return true;
      }

      function getTrackedNodeId(element, datasetKey, prefix) {
        if (!(element instanceof HTMLElement)) {
          return "";
        }
        if (!element.dataset[datasetKey]) {
          element.dataset[datasetKey] = `${prefix}-${Math.random().toString(36).slice(2, 10)}`;
        }
        return element.dataset[datasetKey];
      }

      function resolvePulseMintFromCard(copyButton, card) {
        const parts = pulseCopyParts(copyButton);
        if (!parts) {
          return "";
        }
        return String(lookupPulseCacheEntry("", parts.prefix, parts.suffix)?.tokenAddress || "").trim();
      }

      function lookupPulseCacheEntry(mint = "", prefix = "", suffix = "") {
        try {
          const raw = localStorage.getItem("axiom.pulse");
          if (!raw) {
            return null;
          }

          const parsed = JSON.parse(raw);
          const entries = normalizePulseCacheEntries(parsed?.content || parsed);
          if (mint) {
            return (
              entries.find((entry) => entry.tokenAddress === mint || entry.pairAddress === mint) || null
            );
          }
          if (!prefix || !suffix) {
            return null;
          }
          return (
            entries.find(
              (entry) =>
                (
                  entry.tokenAddress.startsWith(prefix) &&
                  entry.tokenAddress.endsWith(suffix)
                )
            ) || null
          );
        } catch {
          return null;
        }
      }

      function normalizePulseCacheEntries(value) {
        const input = Array.isArray(value) ? value : [];
        return input
          .map((entry) => {
            if (Array.isArray(entry)) {
              return {
                pairAddress: String(entry[0] || "").trim(),
                tokenAddress: String(entry[1] || "").trim()
              };
            }
            if (entry && typeof entry === "object") {
              return {
                pairAddress: String(entry.pairAddress || "").trim(),
                tokenAddress: String(entry.tokenAddress || entry.mint || "").trim(),
                lastSeen: Number.isFinite(entry.lastSeen) ? entry.lastSeen : null
              };
            }
            return null;
          })
          .filter((entry) => entry?.tokenAddress);
      }

      function extractQuotedPriceFromText(value) {
        const text = String(value || "").replace(/\s+/g, " ").trim();
        if (!text) {
          return null;
        }
        const labeledMatch = /price[^$\d]{0,24}\$?\s*([0-9][0-9,]*(?:\.\d+)?(?:e-?\d+)?)/i.exec(text);
        if (labeledMatch) {
          const parsed = Number(labeledMatch[1].replace(/,/g, ""));
          if (Number.isFinite(parsed) && parsed > 0) {
            return parsed;
          }
        }
        const dollarMatches = Array.from(text.matchAll(/\$([0-9][0-9,]*(?:\.\d+)?(?:e-?\d+)?)/gi))
          .map((match) => Number(match[1].replace(/,/g, "")))
          .filter((value) => Number.isFinite(value) && value > 0 && value < 10)
          .sort((left, right) => left - right);
        if (dollarMatches.length) {
          return dollarMatches[0];
        }
        const tinyMatches = Array.from(text.matchAll(/\b0\.\d{4,}\b/g))
          .map((match) => Number(match[0]))
          .filter((value) => Number.isFinite(value) && value > 0 && value < 10)
          .sort((left, right) => left - right);
        return tinyMatches[0] || null;
      }

      function getQuotedPriceHint(tokenContext) {
        const mint = String(tokenContext?.mint || "").trim();
        const routeKey = String(
          tokenContext?.routeAddress ||
          tokenContext?.rawAddress ||
          tokenContext?.address ||
          ""
        ).trim();
        if (!mint) {
          return null;
        }
        if (String(tokenContext?.surface || "").trim() === "pulse") {
          for (const card of document.querySelectorAll(PULSE_CARD_SELECTOR)) {
            const copyButton = card.querySelector("button.group\\/copy");
            if (!(copyButton instanceof HTMLElement)) {
              continue;
            }
            if (resolvePulseMintFromCard(copyButton, card) !== mint) {
              continue;
            }
            const hinted = extractQuotedPriceFromText(card.textContent || "");
            if (hinted) {
              return hinted;
            }
          }
        }
        if (String(tokenContext?.surface || "").trim() === "watchlist") {
          const expectedRouteKey = routeKey || mint;
          const anchor = Array.from(document.querySelectorAll("a[href*='/meme/']")).find((entry) =>
            helpers.extractMintFromUrl(entry.href) === expectedRouteKey
          );
          if (anchor instanceof HTMLAnchorElement) {
            const hinted = extractQuotedPriceFromText(anchor.textContent || "");
            if (hinted) {
              return hinted;
            }
          }
        }
        if (String(tokenContext?.surface || "").trim() === "wallet_tracker") {
          const expectedRouteKey = routeKey || mint;
          const row = Array.from(document.querySelectorAll(WALLET_ROW_SELECTOR)).find((entry) => {
            const tokenLink = entry.querySelector("a[href*='/meme/']");
            return tokenLink instanceof HTMLAnchorElement && helpers.extractMintFromUrl(tokenLink.href) === expectedRouteKey;
          });
          if (row instanceof HTMLElement) {
            const hinted = extractQuotedPriceFromText(row.textContent || "");
            if (hinted) {
              return hinted;
            }
          }
        }
        const mintElement = helpers.findElementShowingMint(mint);
        const container = mintElement?.closest("main, section, article, div") || document.body;
        return extractQuotedPriceFromText(container?.textContent || "");
      }

      function readPixelValue(value) {
        const parsed = Number.parseFloat(String(value || ""));
        return Number.isFinite(parsed) ? parsed : 0;
      }

      function resolveSyncedTargetMetrics(target, fallbackHeight = 24) {
        const styles = helpers.getQuickBuyBaseStyles();
        const computed = target instanceof HTMLElement ? window.getComputedStyle(target) : null;
        const bounds = target instanceof HTMLElement ? target.getBoundingClientRect() : null;
        const targetHeightPx = Math.max(
          fallbackHeight,
          Math.round(
            bounds?.height ||
              readPixelValue(computed?.height) ||
              readPixelValue(styles.base.height) ||
              fallbackHeight
          )
        );
        const compactRadius = Math.max(8, Math.min(12, Math.round(targetHeightPx * 0.38)));
        const targetRadiusPx =
          readPixelValue(computed?.borderRadius) || readPixelValue(styles.base.borderRadius) || compactRadius;

        return {
          computed,
          targetHeight: `${targetHeightPx}px`,
          targetHeightPx,
          borderRadius: `${Math.min(targetRadiusPx, compactRadius)}px`
        };
      }

      function pulseQuickBuyStyles(target) {
        const styles = helpers.getQuickBuyBaseStyles();
        const metrics = resolveSyncedTargetMetrics(target);
        const targetFontSize = metrics.computed?.fontSize || "13px";
        const targetFontWeight = metrics.computed?.fontWeight || "500";
        const logoSize = `${Math.max(17, Math.min(20, Math.round(metrics.targetHeightPx * 0.76)))}px`;

        return {
          base: {
            ...styles.base,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            height: metrics.targetHeight,
            minHeight: metrics.targetHeight,
            width: "auto",
            minWidth: "0",
            padding: "0px 10px",
            marginLeft: "6px",
            marginRight: "0px",
            marginBottom: "0px",
            borderRadius: metrics.borderRadius,
            fontSize: targetFontSize,
            fontWeight: targetFontWeight,
            lineHeight: "1"
          },
          hover: {
            ...styles.base,
            ...styles.hover,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            height: metrics.targetHeight,
            minHeight: metrics.targetHeight,
            width: "auto",
            minWidth: "0",
            padding: "0px 10px",
            marginLeft: "6px",
            marginRight: "0px",
            marginBottom: "0px",
            borderRadius: metrics.borderRadius,
            fontSize: targetFontSize,
            fontWeight: targetFontWeight,
            lineHeight: "1"
          },
          logoSize,
          logoGap: "4px"
        };
      }

      function pulsePanelButtonStyles(target) {
        const styles = helpers.getQuickBuyBaseStyles();
        const metrics = resolveSyncedTargetMetrics(target);
        const logoSize = `${Math.max(18, Math.min(20, Math.round(metrics.targetHeightPx * 0.8)))}px`;

        return {
          base: {
            ...styles.base,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            width: "auto",
            minWidth: "0px",
            height: metrics.targetHeight,
            minHeight: metrics.targetHeight,
            marginLeft: "4px",
            marginRight: "0px",
            marginBottom: "0px",
            padding: "0px 4px",
            borderRadius: metrics.borderRadius,
            lineHeight: "1"
          },
          hover: {
            ...styles.base,
            ...styles.hover,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            width: "auto",
            minWidth: "0px",
            height: metrics.targetHeight,
            minHeight: metrics.targetHeight,
            marginLeft: "4px",
            marginRight: "0px",
            marginBottom: "0px",
            padding: "0px 4px",
            borderRadius: metrics.borderRadius,
            lineHeight: "1"
          },
          logoSize,
          logoGap: "0px"
        };
      }

      function axiomWatchlistQuickBuyStyles() {
        const styles = helpers.getQuickBuyBaseStyles();

        return {
          base: {
            ...styles.base,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: "0",
            height: "24px",
            minHeight: "24px",
            marginLeft: "6px",
            marginRight: "0px",
            marginBottom: "0px",
            padding: "0px 8px",
            borderRadius: "12px",
            fontSize: "13px",
            fontWeight: "500",
            lineHeight: "1"
          },
          hover: {
            ...styles.base,
            ...styles.hover,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: "0",
            height: "24px",
            minHeight: "24px",
            marginLeft: "6px",
            marginRight: "0px",
            marginBottom: "0px",
            padding: "0px 8px",
            borderRadius: "12px",
            fontSize: "13px",
            fontWeight: "500",
            lineHeight: "1"
          },
          logoSize: "16px",
          logoGap: "4px"
        };
      }

      function walletTrackerQuickBuyStyles(target) {
        const styles = helpers.getQuickBuyBaseStyles();
        const computed = target instanceof HTMLElement ? window.getComputedStyle(target) : null;
        const fontSize = "12px";
        const fontWeight = computed?.fontWeight || "500";
        const height = "24px";
        const stableLayout = {
          boxSizing: "border-box",
          display: "inline-flex",
          flex: "0 0 auto",
          flexGrow: "0",
          flexShrink: "0",
          alignItems: "center",
          justifyContent: "center",
          position: "absolute",
          right: "64px",
          top: "8px",
          zIndex: "2",
          whiteSpace: "nowrap",
          lineHeight: "1",
          transform: "none",
          transition: "background-color 0.2s ease"
        };

        return {
          base: {
            ...styles.base,
            ...stableLayout,
            borderRadius: "12px",
            marginLeft: "0px",
            marginRight: "0px",
            padding: "0px 4px",
            fontSize,
            fontWeight,
            minHeight: height,
            height
          },
          hover: {
            ...styles.base,
            ...styles.hover,
            ...stableLayout,
            borderRadius: "12px",
            marginLeft: "0px",
            marginRight: "0px",
            padding: "0px 4px",
            fontSize,
            fontWeight,
            minHeight: height,
            height
          },
          logoSize: "16px",
          logoGap: "4px"
        };
      }

      return {
        isEnabled(siteFeatures) {
          const axiomFeatures = siteFeatures?.axiom || {};
          return Boolean(axiomFeatures.enabled);
        },

        shouldMountLauncher(siteFeatures, tokenContext) {
          return Boolean(siteFeatures?.axiom?.enabled) &&
            Boolean(siteFeatures?.axiom?.floatingLauncher) &&
            String(tokenContext?.surface || "").trim() === "token_detail";
        },

        shouldAutoOpenPanel(tokenContext) {
          return Boolean(tokenContext) &&
            Boolean(helpers.state.siteFeatures?.axiom?.enabled) &&
            tokenContext.surface !== "pulse";
        },

        getQuickBuyStyles() {
          return helpers.getQuickBuyBaseStyles();
        },

        getQuotedPriceHint,
        handleMutations,
        handleWalletStatusChange: refreshAxiomTokenDetailOpenWalletMenu,
        getObserverOptions,
        getCurrentTokenCandidate,
        mount
      };
    }
  });
})();
