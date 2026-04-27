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
      const AXIOM_OVERRIDE_VERSION = "pulse-row-cache-v8";
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
      const PULSE_PANEL_OWNER_CLASS = "trench-tools-pulse-panel-owner";
      let queuedDomOperationTimer = 0;
      const queuedDomOperations = new Map();
      let overridesInjected = false;
      const targetedObservers = new Map();
      let targetedObserverSignature = "";
      let targetedObserverRetryTimer = 0;
      let targetedObserverRetryCount = 0;

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
          const target = findAxiomTokenDetailObserverTarget();
          if (target instanceof HTMLElement) {
            targets.push({
              key: `token-detail:${getTrackedNodeId(target, "trenchToolsObserverTargetId", "observer")}`,
              target
            });
          }
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

      function extractAxiomRouteKeyFromUrl(url) {
        return resolveObservedAddress(helpers.extractMintFromUrl(url));
      }

      function buildAxiomRouteReference(routeKeyOrAddress, surface, url = window.location.href) {
        const routeKey = String(routeKeyOrAddress || "").trim();
        if (!routeKey) {
          return null;
        }
        const pulseCacheEntry = lookupPulseCacheEntry(routeKey);
        const pairPool = String(pulseCacheEntry?.pairAddress || "").trim();
        const tokenMint = String(pulseCacheEntry?.tokenAddress || "").trim();
        const address = surface === "pulse" && pairPool ? pairPool : routeKey;
        const companionPair = pairPool && pairPool !== address ? pairPool : "";
        return {
          address,
          routeKey: address,
          mint: tokenMint || null,
          pair: companionPair || null,
          source: "page",
          surface,
          url
        };
      }

      function buildAxiomRouteReferenceFromUrl(url, surface) {
        return buildAxiomRouteReference(extractAxiomRouteKeyFromUrl(url), surface, url);
      }

      function extractAxiomTokenMintFromElement(root, routeKey = "") {
        if (!(root instanceof Element) && root !== document) {
          return "";
        }
        const normalizedRouteKey = String(routeKey || "").trim();
        const images = Array.from(root.querySelectorAll("img"));
        for (const image of images) {
          const candidates = [image.currentSrc, image.src, image.getAttribute("src"), image.alt];
          for (const candidate of candidates) {
            const mint = String(helpers.extractMintFromText(candidate) || "").trim();
            if (mint && mint !== normalizedRouteKey) {
              return mint;
            }
          }
        }
        return "";
      }

      function buildAxiomRouteReferenceFromElement(url, surface, root) {
        const route = buildAxiomRouteReferenceFromUrl(url, surface);
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

      function buildObservedCandidate(address, surface, url = window.location.href) {
        return buildAxiomRouteReference(address, surface, url);
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
        if (!axiomFeatures.enabled) {
          disconnectAxiomTargetedObserver();
          helpers.removeInjectedControls("[data-trench-tools-token-detail-inline]");
          helpers.removeInjectedControls("[data-trench-tools-pulse-inline]");
          helpers.removeInjectedControls("[data-trench-tools-pulse-panel-inline]");
          helpers.removeInjectedControls("[data-trench-tools-pulse-vamp-inline]");
          helpers.removeInjectedControls("[data-trench-tools-wallet-tracker-inline]");
          helpers.removeInjectedControls("[data-trench-tools-axiom-watchlist-inline]");
          helpers.removeInjectedControls("[data-trench-tools-launchdeck-shell]");
          return;
        }
        ensureAxiomPageOverrides();
        if (!axiomFeatures.instantTrade) {
          helpers.removeInjectedControls("[data-trench-tools-token-detail-inline]");
        }
        if (!axiomFeatures.pulseButton) {
          helpers.removeInjectedControls("[data-trench-tools-pulse-inline]");
        }
        if (!axiomFeatures.pulsePanel) {
          helpers.removeInjectedControls("[data-trench-tools-pulse-panel-inline]");
        }
        if (!axiomFeatures.launchdeckInjection || !axiomFeatures.pulseVamp) {
          helpers.removeInjectedControls("[data-trench-tools-pulse-vamp-inline]");
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
        if (surfaceState.tokenDetail && axiomFeatures.instantTrade) {
          mountAxiomTokenDetailQuickButton(
            buildObservedCandidate(pageAddress, "token_detail", window.location.href) || pageAddress
          );
        } else if (!surfaceState.tokenDetail) {
          helpers.removeInjectedControls("[data-trench-tools-token-detail-inline]");
        }

        if (surfaceState.pulse && axiomFeatures.launchdeckInjection) {
          mountAxiomLaunchShellControls();
        } else {
          helpers.removeInjectedControls("[data-trench-tools-launchdeck-shell]");
        }

        if (surfaceState.pulse && (axiomFeatures.pulseButton || axiomFeatures.pulsePanel)) {
          requestAxiomPulseMetadataRescan();
          mountAxiomPulseQuickButtons();
        } else if (!surfaceState.pulse) {
          helpers.removeInjectedControls("[data-trench-tools-pulse-inline]");
          helpers.removeInjectedControls("[data-trench-tools-pulse-panel-inline]");
          helpers.removeInjectedControls("[data-trench-tools-pulse-vamp-inline]");
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

        if (helpers.state.siteFeatures?.axiom?.launchdeckInjection && helpers.state.siteFeatures?.axiom?.pulseVamp) {
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
            "[data-trench-tools-pulse-inline], [data-trench-tools-pulse-panel-inline], [data-trench-tools-pulse-vamp-inline]"
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
        if (!pairAddress || !tokenAddress) {
          return null;
        }
        return {
          address: pairAddress,
          routeKey: pairAddress,
          mint: tokenAddress,
          pair: pairAddress,
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
          return buildAxiomRouteReferenceFromElement(routeLink.href, "pulse", card);
        }
        return null;
      }

      async function readAxiomPulseCopyButtonAddress(copyButton, parts) {
        if (!(copyButton instanceof HTMLElement) || !parts) {
          return "";
        }
        try {
          copyButton.click();
          const clipboardText = String(await navigator.clipboard?.readText?.() || "").trim();
          return clipboardText &&
            clipboardText.startsWith(parts.prefix) &&
            clipboardText.endsWith(parts.suffix)
            ? clipboardText
            : "";
        } catch {
          return "";
        }
      }

      async function resolvePulseTradeRouteWithFallback(card, control, tokenUrl = window.location.href) {
        const primaryRoute =
          normalizePulseRoute(resolvePulseTradeRouteForCard(card)) ||
          routePayloadFromPulseControl(control, { url: tokenUrl });
        if (primaryRoute?.address) {
          return primaryRoute;
        }
        const copyButton = card instanceof HTMLElement ? card.querySelector("button.group\\/copy") : null;
        const parts = pulseCopyParts(copyButton);
        const copiedAddress = await readAxiomPulseCopyButtonAddress(copyButton, parts);
        if (!copiedAddress) {
          return null;
        }
        return {
          address: copiedAddress,
          routeKey: copiedAddress,
          mint: copiedAddress,
          pair: null,
          source: "page",
          surface: "pulse",
          url: tokenUrl || window.location.href
        };
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
        if (String(route?.surface || "").trim() === "pulse") {
          return {
            ...route,
            mint: routeAddress
          };
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

          // Pulse rows are intentionally not prewarmed on hover/appearance.
          // Opening the manual panel is the user intent signal that starts
          // backend route resolution.
        }

        bindPulseRouteToControl(panelButton, currentRoute, currentTokenUrl);
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
          .querySelectorAll("[data-trench-tools-pulse-vamp-inline]")
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

        // If an existing vamp icon is elsewhere in the card, remove it so we
        // can re-attach it to the fresh anchor (Axiom re-renders cards often).
        card
          .querySelectorAll("[data-trench-tools-pulse-vamp-inline]")
          .forEach((element) => {
            if (element.parentElement !== anchor) {
              helpers.teardownInlineSizeSync?.(element);
              element.remove();
            }
          });

        let vampIcon = anchor.querySelector(
          ":scope > [data-trench-tools-pulse-vamp-inline]"
        );
        if (!(vampIcon instanceof HTMLAnchorElement)) {
          vampIcon = buildAxiomPulseVampIcon(card);
        }

        if (vampIcon.parentElement !== anchor || anchor.lastElementChild !== vampIcon) {
          anchor.appendChild(vampIcon);
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
        await helpers.openLaunchdeckOverlay({
          mode: "create",
          contractAddress,
          instaLaunch: mode === "insta",
        });
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

        const route = buildAxiomRouteReferenceFromElement(anchor.href, "watchlist", anchor);
        const routeKey = String(route?.address || "").trim();
        const tokenMint = String(route?.mint || "").trim();
        const companionPair = String(route?.pair || "").trim();
        if (!routeKey) {
          return;
        }

        let button = anchor.querySelector("[data-trench-tools-axiom-watchlist-inline]");
        if (
          button instanceof HTMLButtonElement &&
          (
            button.getAttribute("data-route-key") !== routeKey ||
            String(button.getAttribute("data-mint") || "") !== tokenMint ||
            String(button.getAttribute("data-pair") || "") !== companionPair
          )
        ) {
          button.remove();
          button = null;
        }

        if (!(button instanceof HTMLButtonElement)) {
          button = helpers.buildInlineButton(
            async () => {
              const liveRoute = routePayloadFromButton(button, {
                address: routeKey,
                mint: tokenMint,
                pair: companionPair,
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

        button.setAttribute("data-route-key", routeKey);
        if (tokenMint) {
          button.setAttribute("data-mint", tokenMint);
        } else {
          button.removeAttribute("data-mint");
        }
        if (companionPair) {
          button.setAttribute("data-pair", companionPair);
        } else {
          button.removeAttribute("data-pair");
        }
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

        const route = buildAxiomRouteReferenceFromElement(tokenLink.href, "wallet_tracker", row);
        const routeKey = String(route?.address || "").trim();
        const tokenMint = String(route?.mint || "").trim();
        const companionPair = String(route?.pair || "").trim();
        if (!routeKey) {
          return;
        }

        let button = row.querySelector("[data-trench-tools-wallet-tracker-inline]");
        if (
          button instanceof HTMLButtonElement &&
          (
            button.getAttribute("data-route-key") !== routeKey ||
            String(button.getAttribute("data-mint") || "") !== tokenMint ||
            String(button.getAttribute("data-pair") || "") !== companionPair
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
                address: routeKey,
                mint: tokenMint,
                pair: companionPair,
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

        button.setAttribute("data-route-key", routeKey);
        if (tokenMint) {
          button.setAttribute("data-mint", tokenMint);
        } else {
          button.removeAttribute("data-mint");
        }
        if (companionPair) {
          button.setAttribute("data-pair", companionPair);
        } else {
          button.removeAttribute("data-pair");
        }
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

      function mountAxiomTokenDetailQuickButton(routeOrAddress) {
        const route =
          routeOrAddress && typeof routeOrAddress === "object"
            ? routeOrAddress
            : buildObservedCandidate(routeOrAddress, "token_detail", window.location.href);
        const routeKey = String(route?.address || routeOrAddress || "").trim();
        const tokenMint = String(route?.mint || "").trim();
        const companionPair = String(route?.pair || "").trim();
        const nativeButton = findAxiomTokenDetailOrderButton();
        const amountInput = findAxiomTokenDetailAmountInput();
        if (!routeKey || !(nativeButton instanceof HTMLElement) || !(amountInput instanceof HTMLInputElement)) {
          cleanupAxiomTokenDetailManualBuyButtons();
          return;
        }

        document.querySelectorAll("[data-trench-tools-token-detail-inline]").forEach((element) => {
          if (element.previousElementSibling !== nativeButton) {
            helpers.teardownInlineSizeSync(element);
            element._trenchInlineCleanup?.();
            element.remove();
          }
        });

        let button = nativeButton.nextElementSibling;
        const routeMatches =
          button instanceof HTMLButtonElement &&
          button.hasAttribute("data-trench-tools-token-detail-inline") &&
          button.getAttribute("data-route-key") === routeKey &&
          String(button.getAttribute("data-mint") || "") === tokenMint &&
          String(button.getAttribute("data-pair") || "") === companionPair;
        if (!routeMatches) {
          if (button instanceof HTMLElement && button.hasAttribute("data-trench-tools-token-detail-inline")) {
            helpers.teardownInlineSizeSync(button);
            button._trenchInlineCleanup?.();
            button.remove();
          }
          button = buildAxiomTokenDetailManualBuyButton(nativeButton, amountInput, {
            routeKey,
            tokenMint,
            companionPair
          });
          nativeButton.insertAdjacentElement("afterend", button);
        }
        applyAxiomTokenDetailManualLayout(nativeButton, button);
        bindAxiomTokenDetailManualButton(nativeButton, amountInput, button);
      }

      function cleanupAxiomTokenDetailManualBuyButtons() {
        document.querySelectorAll("[data-trench-tools-token-detail-inline]").forEach((element) => {
          helpers.teardownInlineSizeSync(element);
          element.remove();
        });
      }

      function applyAxiomTokenDetailManualLayout(nativeButton, button) {
        if (!(nativeButton instanceof HTMLElement) || !(button instanceof HTMLElement)) {
          return;
        }
        const parent = nativeButton.parentElement;
        if (!button._trenchToolsManualBuyLayoutRestore) {
          const previousNativeWidth = nativeButton.style.width;
          const previousNativeMarginBottom = nativeButton.style.marginBottom;
          const previousParentFlexDirection = parent instanceof HTMLElement ? parent.style.flexDirection : "";
          button._trenchToolsManualBuyLayoutRestore = () => {
            nativeButton.style.width = previousNativeWidth;
            nativeButton.style.marginBottom = previousNativeMarginBottom;
            if (parent instanceof HTMLElement) {
              parent.style.flexDirection = previousParentFlexDirection;
            }
            button._trenchToolsManualBuyLayoutRestore = null;
          };
        }
        nativeButton.style.width = "100%";
        nativeButton.style.marginBottom = "6px";
        if (parent instanceof HTMLElement) {
          parent.style.flexDirection = "column";
        }
      }

      function findAxiomTokenDetailOrderButton() {
        const instantTrade = document.querySelector("div#instant-trade");
        const buttons = Array.from(
          (instantTrade || document).querySelectorAll(
            "button.bg-decrease.rounded-full.items-center, button.bg-increase.rounded-full.items-center"
          )
        ).filter((button) =>
          button instanceof HTMLButtonElement &&
          isVisibleAxiomNode(button) &&
          !/place\s*order/i.test(String(button.textContent || ""))
        );
        return buttons[buttons.length - 1] || null;
      }

      function findAxiomTokenDetailAmountInput() {
        const instantTrade = document.querySelector("div#instant-trade");
        const inputs = Array.from(
          (instantTrade || document).querySelectorAll("input.w-full.h-full.bg-transparent")
        ).filter((input) => input instanceof HTMLInputElement && isVisibleAxiomNode(input));
        return inputs[inputs.length - 1] || null;
      }

      function isAxiomTokenDetailBuyMode(nativeButton) {
        if (!(nativeButton instanceof HTMLElement)) {
          return false;
        }
        const text = String(nativeButton.textContent || "");
        const className = String(nativeButton.className || "");
        return /buy/i.test(text) || /bg-increase/.test(className);
      }

      function readAxiomTokenDetailBuyAmount(amountInput) {
        const value = String(amountInput?.value || "").trim();
        return Number.isFinite(Number(value)) && Number(value) > 0 ? value : "";
      }

      function updateAxiomTokenDetailManualButton(nativeButton, amountInput, button) {
        if (!(button instanceof HTMLButtonElement)) {
          return;
        }
        const buyMode = isAxiomTokenDetailBuyMode(nativeButton);
        const amount = readAxiomTokenDetailBuyAmount(amountInput);
        button.style.display = buyMode ? "flex" : "none";
        helpers.setInlineButtonLabel(button, amount ? `Buy ${amount} SOL` : "Buy");
      }

      function buildAxiomTokenDetailManualBuyButton(nativeButton, amountInput, route) {
        const button = helpers.buildInlineButton(async () => {
          const amount = readAxiomTokenDetailBuyAmount(amountInput);
          if (!amount) {
            helpers.showToast?.("Invalid buy amount.", "error");
            return;
          }
          const liveRoute = routePayloadFromButton(button, {
            address: route.routeKey,
            mint: route.tokenMint,
            pair: route.companionPair,
            surface: "token_detail",
            url: window.location.href
          });
          if (!liveRoute?.address) {
            helpers.showToast?.("Token not found.", "error");
            return;
          }
          await helpers.handleInlineTradeRequest("buy", liveRoute, "token_detail", {
            ...helpers.state.preferences,
            buyAmountSol: amount
          }, window.location.href);
        }, {
          base: {
            width: "100%",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            padding: "8px 16px",
            borderRadius: "999px",
            border: "1px solid rgba(255, 255, 255, 0.20)",
            color: "#ffffff",
            backgroundColor: "#000000",
            cursor: "pointer",
            transition: "background-color 0.2s ease",
            zIndex: "1000"
          },
          hover: {
            backgroundColor: "#18181b"
          }
        });
        button.setAttribute("data-trench-tools-token-detail-inline", "true");
        button.setAttribute("data-route-key", route.routeKey);
        if (route.tokenMint) {
          button.setAttribute("data-mint", route.tokenMint);
        }
        if (route.companionPair) {
          button.setAttribute("data-pair", route.companionPair);
        }
        updateAxiomTokenDetailManualButton(nativeButton, amountInput, button);
        return button;
      }

      function bindAxiomTokenDetailManualButton(nativeButton, amountInput, button) {
        if (!(button instanceof HTMLButtonElement)) {
          return;
        }
        if (
          button._trenchToolsManualBuyBound &&
          button._trenchToolsManualBuyNativeButton === nativeButton &&
          button._trenchToolsManualBuyAmountInput === amountInput
        ) {
          updateAxiomTokenDetailManualButton(nativeButton, amountInput, button);
          return;
        }
        button._trenchInlineCleanup?.();
        button._trenchToolsManualBuyBound = true;
        button._trenchToolsManualBuyNativeButton = nativeButton;
        button._trenchToolsManualBuyAmountInput = amountInput;
        const update = () => updateAxiomTokenDetailManualButton(nativeButton, amountInput, button);
        const nativeClickUpdate = () => window.setTimeout(update, 0);
        amountInput.addEventListener("input", update);
        amountInput.addEventListener("change", update);
        nativeButton.addEventListener("click", nativeClickUpdate);
        const observer = new MutationObserver(update);
        observer.observe(nativeButton, { childList: true, subtree: true, characterData: true, attributes: true });
        const syncInterval = window.setInterval(update, 250);
        button._trenchInlineCleanup = () => {
          window.clearInterval(syncInterval);
          amountInput.removeEventListener("input", update);
          amountInput.removeEventListener("change", update);
          nativeButton.removeEventListener("click", nativeClickUpdate);
          observer.disconnect();
          button._trenchToolsManualBuyLayoutRestore?.();
          button._trenchToolsManualBuyBound = false;
          button._trenchToolsManualBuyNativeButton = null;
          button._trenchToolsManualBuyAmountInput = null;
        };
        update();
      }

      function findAxiomTokenDetailControlRows() {
        const instantTrade = document.querySelector("div#instant-trade");
        const candidates = instantTrade instanceof HTMLElement
          ? Array.from(instantTrade.querySelectorAll("div.flex-row.w-full"))
          : findAxiomTokenDetailControlRowCandidates();
        return candidates.filter((row) =>
          row instanceof HTMLElement &&
          isAxiomTokenDetailControlRow(row)
        );
      }

      function findAxiomTokenDetailControlRowCandidates() {
        const directRows = Array.from(
          document.querySelectorAll("div[class*='rounded-b'][class*='border-x'][class*='border-b']")
        ).filter((row) => row instanceof HTMLElement);
        if (directRows.length) {
          return directRows;
        }
        const roots = findAxiomTokenDetailPanelRoots();
        return roots.flatMap((root) => Array.from(root.querySelectorAll("div")));
      }

      function findAxiomTokenDetailObserverTarget() {
        const instantTrade = document.querySelector("div#instant-trade");
        if (instantTrade instanceof HTMLElement) {
          return instantTrade.parentElement || instantTrade;
        }
        const row = findAxiomTokenDetailControlRows()[0];
        if (!(row instanceof HTMLElement)) {
          return null;
        }
        return findAxiomTokenDetailPanelRoot(row) || row.parentElement || row;
      }

      function findAxiomTokenDetailPanelRoots() {
        const roots = new Set();
        Array.from(document.querySelectorAll("div.relative")).forEach((element) => {
          if (!(element instanceof HTMLElement)) {
            return;
          }
          const text = String(element.textContent || "").replace(/\s+/g, "");
          if (/BuySell/.test(text) && /(AMOUNT|Adv\.?strat|Market|Limit|Instant)/i.test(text)) {
            roots.add(element);
          }
        });
        return Array.from(roots);
      }

      function findAxiomTokenDetailPanelRoot(element) {
        if (!(element instanceof HTMLElement)) {
          return null;
        }
        const instantTrade = element.closest("div#instant-trade");
        if (instantTrade instanceof HTMLElement) {
          return instantTrade;
        }
        let current = element;
        for (let depth = 0; current instanceof HTMLElement && depth < 10; depth += 1) {
          const text = String(current.textContent || "").replace(/\s+/g, "");
          if (/BuySell/.test(text) && /(AMOUNT|Adv\.?strat|Market|Limit|Instant)/i.test(text)) {
            return current;
          }
          current = current.parentElement;
        }
        return null;
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

      function findAxiomTokenDetailNativeControls(row) {
        if (!(row instanceof HTMLElement)) {
          return [];
        }
        return Array.from(row.children).filter((element) =>
          element instanceof HTMLElement &&
          !element.hasAttribute("data-trench-tools-token-detail-inline") &&
          readAxiomTokenDetailAction(element) &&
          (
            element.matches("div.rounded-full:not(.group\\/wallets)") ||
            (
              element.matches("div.cursor-pointer") &&
              element.getBoundingClientRect().width > 0 &&
              element.getBoundingClientRect().height > 0
            )
          )
        );
      }

      function readAxiomTokenDetailAction(control) {
        const text = String(control?.textContent || "").replace(/\s+/g, "").trim();
        if (!text) {
          return null;
        }
        const side = resolveAxiomTokenDetailSide(control) || (text.includes("%") ? "sell" : "buy");
        const amount = text.replace("%", "").trim();
        if (!amount || !Number.isFinite(Number(amount))) {
          return null;
        }
        return { side, amount };
      }

      function resolveAxiomTokenDetailSide(control) {
        if (!(control instanceof HTMLElement)) {
          return "";
        }
        const root = findAxiomTokenDetailPanelRoot(control);
        if (!(root instanceof HTMLElement)) {
          return "";
        }
        const sideLabels = Array.from(root.querySelectorAll("span, div"))
          .filter((element) => {
            if (!(element instanceof HTMLElement)) {
              return false;
            }
            const text = String(element.textContent || "").replace(/\s+/g, "").trim();
            return text === "Buy" || text === "Sell";
          })
          .sort((left, right) => {
            const leftVisible = isVisibleAxiomNode(left) ? 0 : 1;
            const rightVisible = isVisibleAxiomNode(right) ? 0 : 1;
            return leftVisible - rightVisible;
          });
        for (const label of sideLabels) {
          const labelText = String(label.textContent || "").replace(/\s+/g, "").trim();
          const side = labelText === "Sell" ? "sell" : "buy";
          if (isActiveAxiomTokenDetailSideLabel(label, side, root)) {
            return side;
          }
        }
        return "";
      }

      function isActiveAxiomTokenDetailSideLabel(label, side, root) {
        let current = label;
        for (let depth = 0; current instanceof HTMLElement && depth < 4; depth += 1) {
          const className = String(current.className || "");
          if (side === "buy" && /bg-increase/.test(className)) {
            return true;
          }
          if (side === "sell" && /bg-decrease/.test(className)) {
            return true;
          }
          if (current === label && /text-background|font-bold/.test(className)) {
            return true;
          }
          if (current === root) {
            break;
          }
          current = current.parentElement;
        }
        return false;
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
        button.setAttribute("data-trench-tools-token-detail-inline", "true");
        button.setAttribute("data-route-key", action.routeKey);
        button.setAttribute("data-control-index", String(action.index));
        button.setAttribute("data-side", action.side);
        button.setAttribute("data-amount", action.amount);
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
        nativeControl.style.minWidth = "40px";
        Object.assign(button.style, {
          minWidth: "40px",
          borderColor: "rgba(255, 255, 255, 0.20)",
          color: "#ffffff",
          backgroundColor: "#000000",
          zIndex: "1000"
        });
        button.addEventListener("mouseenter", () => {
          button.style.backgroundColor = "#18181b";
        });
        button.addEventListener("mouseleave", () => {
          button.style.backgroundColor = "#000000";
        });
        button.addEventListener("click", (event) => {
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
            ...helpers.state.preferences,
            ...(action.side === "sell"
              ? { sellPercent: action.amount }
              : { buyAmountSol: action.amount })
          };
          void helpers.handleInlineTradeRequest(action.side, liveRoute, "token_detail", payload, window.location.href)
            .catch((error) => helpers.showToast?.(error?.message || "Trade failed.", "error"));
        });
        return button;
      }

      function handleMutations(mutations) {
        const axiomFeatures = helpers.state.siteFeatures?.axiom || {};
        if (!axiomFeatures.enabled) {
          return true;
        }
        const pageAddress = resolveCurrentPageAddress();
        const surfaceState = getAxiomSurfaceState(pageAddress, axiomFeatures);
        const processPulse = surfaceState.pulse &&
          (axiomFeatures.pulseButton || axiomFeatures.pulsePanel || axiomFeatures.pulseVamp);
        const processLaunchShell = surfaceState.pulse && axiomFeatures.launchdeckInjection;
        const processTokenDetail = surfaceState.tokenDetail && axiomFeatures.instantTrade;
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
            if (latestPageAddress && !isPulseUrl(window.location.href) && helpers.state.siteFeatures?.axiom?.instantTrade) {
              mountAxiomTokenDetailQuickButton(
                buildObservedCandidate(latestPageAddress, "token_detail", window.location.href) || latestPageAddress
              );
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
        getObserverOptions,
        getCurrentTokenCandidate,
        mount
      };
    }
  });
})();
