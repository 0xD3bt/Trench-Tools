(function registerTrenchToolsJ7Adapter() {
  const runtime = window.TrenchToolsContentRuntime;
  if (!runtime) {
    throw new Error("Trench Tools content runtime missing.");
  }

  runtime.registerPlatformAdapter("j7", {
    matchesHost(hostname) {
      return hostname === "j7tracker.io";
    },

    createAdapter(helpers) {
      const SOLANA_ADDRESS_REGEX = /\b[1-9A-HJ-NP-Za-km-z]{32,44}\b/;
      const STATUS_LINK_REGEX = /^https?:\/\/(?:www\.)?(?:x|twitter)\.com\/([^/?#\s]+)\/status\/(\d+)(?:[/?#].*)?$/i;
      const PROFILE_LINK_REGEX = /^https?:\/\/(?:www\.)?(?:x|twitter)\.com\/([^/?#\s]+)\/?(?:[?#].*)?$/i;
      const CONTRACT_SELECTOR = "span.contract-address";
      const CARD_ROW_SELECTOR = ".tweet-row, .tweet-embed, .tweet-card";
      const CARD_ROW_WITH_ID_SELECTOR =
        ".tweet-row[data-tweet-id], .tweet-embed[data-tweet-id], .tweet-card[data-tweet-id]";
      const NATIVE_VAMP_SELECTOR_PARTS = [
        "button[data-mlw-tip='Vamp']",
        ".tweet-vamp-btn-topright",
        ".tweet-vamp-btn-standalone",
        ".tweet-vamp-btn"
      ];
      const NATIVE_DEPLOY_SELECTOR_PARTS = [
        "button[data-mlw-tip='Deploy']",
        ".tweet-deploy-btn-topright",
        ".tweet-deploy-btn"
      ];
      const NATIVE_VAMP_SELECTOR = NATIVE_VAMP_SELECTOR_PARTS.join(", ");
      const NATIVE_DEPLOY_SELECTOR = NATIVE_DEPLOY_SELECTOR_PARTS.join(", ");
      const NATIVE_DEPLOY_CLOSE_SELECTOR = ".tweet-deploy-btn-close";
      const NATIVE_CARD_BUTTON_SELECTOR = `${NATIVE_VAMP_SELECTOR}, ${NATIVE_DEPLOY_SELECTOR}`;
      const DIRECT_NATIVE_CARD_BUTTON_SELECTOR = [
        ...NATIVE_VAMP_SELECTOR_PARTS.map((part) => `:scope > ${part}`),
        ...NATIVE_DEPLOY_SELECTOR_PARTS.map((part) => `:scope > ${part}`)
      ].join(", ");
      const DIRECT_TT_VAMP_SELECTOR = ":scope > [data-trench-tools-j7-card-action='vamp']";
      const DIRECT_TT_DEPLOY_SELECTOR = ":scope > [data-trench-tools-j7-card-action='deploy']";
      const ACTION_CONTAINER_ATTR = "data-trench-tools-j7-action-container";
      const ACTION_CONTAINER_SELECTOR = `[${ACTION_CONTAINER_ATTR}]`;
      const TT_CONTRACT_WRAPPER_SELECTOR = "[data-trench-tools-j7-contract-controls]";
      const TT_CARD_BUTTON_SELECTOR = "[data-trench-tools-j7-card-action]";
      const TT_FALLBACK_DEPLOY_SECTION_SELECTOR = "[data-trench-tools-j7-fallback-deploy-section]";
      const TT_FALLBACK_DEPLOY_CLASS_ATTR = "data-trench-tools-j7-fallback-deploy-class";
      const TT_BRAND_BG = "#000000";
      const TT_BRAND_FG = "#ffffff";
      const TT_BRAND_HOVER_BG = "#1a1a1a";
      const SIDE_RAIL_DEPLOY_WIDTH_PX = 56;
      const SIDE_RAIL_DEPLOY_GAP_PX = 4;
      const SIDE_RAIL_DEPLOY_WIDTH = `${SIDE_RAIL_DEPLOY_WIDTH_PX}px`;
      const SIDE_RAIL_BORDER_RADIUS = "4px";
      const SIDE_RAIL_CARD_PADDING_PX = 38;
      const SIDE_RAIL_OUTER_TRIM_PX = 6;
      const SIDE_RAIL_TT_OFFSET_PX = SIDE_RAIL_DEPLOY_WIDTH_PX + SIDE_RAIL_DEPLOY_GAP_PX;
      const VAMP_RIGHT_SIDE_GAP_PX = 4;
      const TOP_RIGHT_CONTAINER_CLASS = "tweet-topright-btn-container";
      const TOP_RIGHT_TT_GROUP_GAP_PX = 6;
      const TOP_RIGHT_NARROW_BREAKPOINT_PX = 768;
      const TT_CARD_PADDED_ATTR = "data-trench-tools-j7-card-padded";
      const TT_RAIL_RESIZED_ATTR = "data-trench-tools-j7-rail-resized";
      const TT_VAMP_SHIFTED_ATTR = "data-trench-tools-j7-vamp-shifted";
      const NO_TWITCH_STYLE_ID = "trench-tools-j7-no-twitch-style";
      const RESTAMP_DELAY_MS = 80;
      const COMPACT_BUTTON_SIZE_PX = 36;
      const COMPACT_ICON_SIZE_PX = 16;
      const J7_IMAGE_CAPTURE_MAX_DIMENSION = 1024;
      const J7_IMAGE_CAPTURE_TIMEOUT_MS = 2500;
      const COMPACT_BUTTON_PROPS = ["width", "min-width", "max-width", "flex", "padding-left", "padding-right", "gap"];
      const COMPACT_BUTTON_SELECTOR = "button[data-mlw-tip='Vamp'],button[data-mlw-tip='Deploy'],.tweet-vamp-btn-topright,.tweet-deploy-btn-topright,.tweet-vamp-btn,.tweet-deploy-btn,.tweet-vamp-btn-standalone,[data-trench-tools-j7-card-action]";
      const nativeStyleCache = new WeakMap();
      let bodyClassObserver = null;
      let restampTimer = null;
      let lastBodyClass = "";
      let compactMql = null;
      let compactMqlListener = null;

      function getCurrentTokenCandidate() {
        const address = firstSolanaAddress();

        if (!address) {
          return null;
        }

        return {
          address,
          mint: address,
          source: "page",
          surface: "contract_address",
          url: window.location.href
        };
      }

      function teardownInjectedControls() {
        document.querySelectorAll(`${TT_CONTRACT_WRAPPER_SELECTOR}, ${TT_CARD_BUTTON_SELECTOR}, ${TT_FALLBACK_DEPLOY_SECTION_SELECTOR}`).forEach((element) => {
          element.remove();
        });
        document.querySelectorAll(`[${TT_FALLBACK_DEPLOY_CLASS_ATTR}]`).forEach((element) => {
          const className = element.getAttribute(TT_FALLBACK_DEPLOY_CLASS_ATTR);
          if (className) element.classList.remove(className);
          element.removeAttribute(TT_FALLBACK_DEPLOY_CLASS_ATTR);
        });
        document.querySelectorAll("[data-trench-tools-j7-processed]").forEach((element) => {
          delete element.dataset.trenchToolsJ7Processed;
          delete element.dataset.trenchToolsJ7ControlsSignature;
        });
        document.querySelectorAll("[data-trench-tools-j7-prewarm-wired]").forEach((element) => {
          delete element.dataset.trenchToolsJ7PrewarmWired;
        });
        document.querySelectorAll(`[${TT_CARD_PADDED_ATTR}]`).forEach((element) => {
          restoreSideRailCardPadding(element);
        });
        document.querySelectorAll(`[${TT_RAIL_RESIZED_ATTR}]`).forEach((element) => {
          restoreSideRailSectionWidth(element);
        });
        document.querySelectorAll(`[${TT_VAMP_SHIFTED_ATTR}]`).forEach((element) => {
          restoreVampRightSideClearance(element);
        });
        document.querySelectorAll(ACTION_CONTAINER_SELECTOR).forEach((element) => {
          delete element.dataset.trenchToolsJ7CardProcessed;
          element.removeAttribute(ACTION_CONTAINER_ATTR);
        });
        document.querySelectorAll("[data-trench-tools-j7-compact='1']").forEach((element) => {
          clearCompactFromButton(element);
        });
        document.querySelectorAll("[data-trench-tools-j7-grouped]").forEach((element) => {
          element.removeAttribute("data-trench-tools-j7-grouped");
        });
        restoreNativeCardActions();
        removeNoTwitchStyle();
        disposeCompactObserver();
        disposeBodyClassObserver();
      }

      function mount() {
        if (!helpers.state.siteFeatures?.j7?.enabled) {
          teardownInjectedControls();
          return;
        }

        ensureNoTwitchStyle();
        ensureCompactObserver();
        mountContractAddressControls(document);
        mountCardLaunchdeckControls(document);
        ensureBodyClassObserver();
      }

      function ensureNoTwitchStyle() {
        if (!j7Features().cardLaunchdeck) {
          removeNoTwitchStyle();
          return;
        }
        if (document.getElementById(NO_TWITCH_STYLE_ID)) return;
        const style = document.createElement("style");
        style.id = NO_TWITCH_STYLE_ID;
        const vampShiftRule =
          ".tweet-card.deploy-right button[data-mlw-tip='Vamp']:not([" + TT_VAMP_SHIFTED_ATTR + "]):not([data-trench-tools-j7-card-action])," +
          ".tweet-card.deploy-right .tweet-vamp-btn-standalone:not([" + TT_VAMP_SHIFTED_ATTR + "]):not([data-trench-tools-j7-card-action])," +
          ".tweet-card.deploy-right .tweet-vamp-btn:not([" + TT_VAMP_SHIFTED_ATTR + "]):not([data-trench-tools-j7-card-action])" +
          "{visibility:hidden!important}";
        const w = COMPACT_BUTTON_SIZE_PX + "px";
        const ico = COMPACT_ICON_SIZE_PX + "px";
        const compactPrepaintRule =
          "@media (max-width:" + TOP_RIGHT_NARROW_BREAKPOINT_PX + "px){" +
            ".tweet-card.deploy-top-right .tweet-vamp-btn-topright," +
            ".tweet-card.deploy-top-right .tweet-deploy-btn-topright," +
            ".tweet-card.deploy-top-right button[data-mlw-tip='Vamp']," +
            ".tweet-card.deploy-top-right button[data-mlw-tip='Deploy']," +
            ".tweet-card.deploy-top-right [data-trench-tools-j7-card-action]" +
            "{width:" + w + "!important;min-width:" + w + "!important;max-width:" + w + "!important;flex:0 0 " + w + "!important;padding-left:0!important;padding-right:0!important;gap:0!important;font-size:0!important}" +
            ".tweet-card.deploy-top-right .tweet-vamp-btn-topright>svg," +
            ".tweet-card.deploy-top-right .tweet-deploy-btn-topright>svg," +
            ".tweet-card.deploy-top-right button[data-mlw-tip='Vamp']>svg," +
            ".tweet-card.deploy-top-right button[data-mlw-tip='Deploy']>svg," +
            ".tweet-card.deploy-top-right [data-trench-tools-j7-card-action]>svg" +
            "{width:" + ico + "!important;height:" + ico + "!important;flex:0 0 auto!important}" +
          "}";
        style.textContent = vampShiftRule + compactPrepaintRule;
        (document.head || document.documentElement).appendChild(style);
      }

      function removeNoTwitchStyle() {
        const style = document.getElementById(NO_TWITCH_STYLE_ID);
        if (style) style.remove();
      }

      function ensureCompactObserver() {
        if (typeof window.matchMedia !== "function") return;
        if (compactMql) return;
        compactMql = window.matchMedia("(max-width:" + TOP_RIGHT_NARROW_BREAKPOINT_PX + "px)");
        compactMqlListener = () => syncCompactStylesAll();
        if (typeof compactMql.addEventListener === "function") compactMql.addEventListener("change", compactMqlListener);
        else if (typeof compactMql.addListener === "function") compactMql.addListener(compactMqlListener);
      }

      function disposeCompactObserver() {
        if (!compactMql) return;
        if (compactMqlListener) {
          if (typeof compactMql.removeEventListener === "function") compactMql.removeEventListener("change", compactMqlListener);
          else if (typeof compactMql.removeListener === "function") compactMql.removeListener(compactMqlListener);
        }
        compactMql = null;
        compactMqlListener = null;
      }

      function isCompactViewport() {
        return Boolean(compactMql && compactMql.matches);
      }

      function compactStyleKey(property) {
        return property.replace(/-([a-z])/g, (_match, letter) => letter.toUpperCase());
      }

      function rememberInlineStyle(element, property) {
        if (!(element instanceof HTMLElement || element instanceof SVGElement)) return;
        const key = compactStyleKey(property);
        const valueKey = `trenchToolsJ7Orig${key}`;
        const priorityKey = `trenchToolsJ7Orig${key}Priority`;
        if (valueKey in element.dataset) return;
        element.dataset[valueKey] = element.style.getPropertyValue(property) || "";
        element.dataset[priorityKey] = element.style.getPropertyPriority(property) || "";
      }

      function restoreInlineStyle(element, property) {
        if (!(element instanceof HTMLElement || element instanceof SVGElement)) return;
        const key = compactStyleKey(property);
        const valueKey = `trenchToolsJ7Orig${key}`;
        const priorityKey = `trenchToolsJ7Orig${key}Priority`;
        if (!(valueKey in element.dataset)) return;
        const value = element.dataset[valueKey] || "";
        const priority = element.dataset[priorityKey] || "";
        element.style.removeProperty(property);
        if (value) element.style.setProperty(property, value, priority);
        delete element.dataset[valueKey];
        delete element.dataset[priorityKey];
      }

      function applyCompactToButton(button) {
        if (!(button instanceof HTMLElement)) return;
        button.dataset.trenchToolsJ7Compact = "1";
        if (button.matches(NATIVE_DEPLOY_SELECTOR) || button.getAttribute("data-trench-tools-j7-card-action") === "deploy") {
          rememberInlineStyle(button, "font-size");
          button.style.setProperty("font-size", "0", "important");
        }
        for (const property of COMPACT_BUTTON_PROPS) {
          rememberInlineStyle(button, property);
        }
        button.style.setProperty("width", COMPACT_BUTTON_SIZE_PX + "px", "important");
        button.style.setProperty("min-width", COMPACT_BUTTON_SIZE_PX + "px", "important");
        button.style.setProperty("max-width", COMPACT_BUTTON_SIZE_PX + "px", "important");
        button.style.setProperty("flex", "0 0 " + COMPACT_BUTTON_SIZE_PX + "px", "important");
        button.style.setProperty("padding-left", "0", "important");
        button.style.setProperty("padding-right", "0", "important");
        button.style.setProperty("gap", "0", "important");
        for (const child of Array.from(button.children)) {
          const tag = (child.tagName || "").toLowerCase();
          if (tag === "svg") {
            rememberInlineStyle(child, "width");
            rememberInlineStyle(child, "height");
            rememberInlineStyle(child, "flex");
            child.style.setProperty("width", COMPACT_ICON_SIZE_PX + "px", "important");
            child.style.setProperty("height", COMPACT_ICON_SIZE_PX + "px", "important");
            child.style.setProperty("flex", "0 0 auto", "important");
          } else if (child instanceof HTMLElement) {
            rememberInlineStyle(child, "display");
            child.style.setProperty("display", "none", "important");
          }
        }
      }

      function clearCompactFromButton(button) {
        if (!(button instanceof HTMLElement)) return;
        if (button.dataset.trenchToolsJ7Compact !== "1") return;
        delete button.dataset.trenchToolsJ7Compact;
        for (const prop of COMPACT_BUTTON_PROPS) restoreInlineStyle(button, prop);
        restoreInlineStyle(button, "font-size");
        for (const child of Array.from(button.children)) {
          const tag = (child.tagName || "").toLowerCase();
          if (tag === "svg") {
            restoreInlineStyle(child, "width");
            restoreInlineStyle(child, "height");
            restoreInlineStyle(child, "flex");
          } else if (child instanceof HTMLElement) {
            restoreInlineStyle(child, "display");
          }
        }
      }

      function syncCompactForButton(button) {
        if (!(button instanceof HTMLElement)) return;
        if (!button.closest("[data-trench-tools-j7-grouped]")) {
          clearCompactFromButton(button);
          return;
        }
        if (isCompactViewport()) applyCompactToButton(button);
        else clearCompactFromButton(button);
      }

      function syncCompactForRow(row) {
        if (!(row instanceof HTMLElement)) return;
        for (const btn of row.querySelectorAll(COMPACT_BUTTON_SELECTOR)) syncCompactForButton(btn);
      }

      function syncCompactStylesAll() {
        for (const btn of document.querySelectorAll(COMPACT_BUTTON_SELECTOR)) syncCompactForButton(btn);
      }

      function prehideUnshiftedNativeVampsIn(root) {
        if (!(root instanceof Element)) return;
        const features = j7Features();
        if (!features.cardLaunchdeck) return;
        const candidates = [];
        if (root.matches(NATIVE_VAMP_SELECTOR)) candidates.push(root);
        for (const node of root.querySelectorAll(NATIVE_VAMP_SELECTOR)) candidates.push(node);
        for (const vamp of candidates) {
          if (!(vamp instanceof HTMLElement)) continue;
          if (vamp.hasAttribute("data-trench-tools-j7-card-action")) continue;
          if (vamp.hasAttribute(TT_VAMP_SHIFTED_ATTR)) continue;
          const card = vamp.closest(".tweet-card.deploy-right");
          if (!card) continue;
          vamp.style.setProperty("visibility", "hidden", "important");
        }
      }

      function j7Features() {
        return helpers.state.siteFeatures?.j7 || {};
      }

      function firstSolanaAddress() {
        for (const span of document.querySelectorAll(CONTRACT_SELECTOR)) {
          const mint = extractSolanaAddress(span.textContent || span.getAttribute("data-address") || "");
          if (mint) return mint;
        }
        for (const anchor of findModernAddressAnchors(document)) {
          const mint = extractSolanaAddress(anchor.textContent || "");
          if (mint) return mint;
        }
        return helpers.extractMintFromUrl(window.location.href) || "";
      }

      function extractSolanaAddress(value) {
        const match = String(value || "").match(SOLANA_ADDRESS_REGEX);
        return match ? match[0] : "";
      }

      function mountContractAddressControls(root) {
        const features = j7Features();
        if (!features.contractQuickBuy && !features.contractQuickPanel && !features.contractVamp) {
          document.querySelectorAll(TT_CONTRACT_WRAPPER_SELECTOR).forEach((element) => element.remove());
          return;
        }

        for (const anchor of collectAddressAnchors(root)) {
          attachContractControlsToAnchor(anchor.element, anchor.mint);
        }
      }

      function collectAddressAnchors(root) {
        const seen = new Set();
        const anchors = [];
        for (const span of queryAll(root, CONTRACT_SELECTOR)) {
          if (!(span instanceof HTMLElement) || seen.has(span)) continue;
          const mint = extractSolanaAddress(span.textContent || span.getAttribute("data-address") || "");
          if (!mint) continue;
          seen.add(span);
          anchors.push({ element: span, mint });
        }
        for (const element of findModernAddressAnchors(root)) {
          if (seen.has(element)) continue;
          const mint = extractSolanaAddress(element.textContent || "");
          if (!mint) continue;
          seen.add(element);
          anchors.push({ element, mint });
        }
        return anchors;
      }

      function findModernAddressAnchors(root) {
        const scope = root instanceof Element
          ? root
          : root instanceof Document
            ? root.body || root
            : document.body;
        if (!scope) return [];
        const result = [];
        const seen = new Set();
        const walker = document.createTreeWalker(scope, NodeFilter.SHOW_TEXT, {
          acceptNode(textNode) {
            const value = textNode.nodeValue;
            if (!value || !SOLANA_ADDRESS_REGEX.test(value)) return NodeFilter.FILTER_REJECT;
            const parent = textNode.parentElement;
            if (!parent) return NodeFilter.FILTER_REJECT;
            if (parent.closest(TT_CONTRACT_WRAPPER_SELECTOR)) return NodeFilter.FILTER_REJECT;
            if (parent.matches(CONTRACT_SELECTOR)) return NodeFilter.FILTER_REJECT;
            if (parent.closest("script, style")) return NodeFilter.FILTER_REJECT;
            if (!parent.closest(CARD_ROW_SELECTOR)) return NodeFilter.FILTER_REJECT;
            return NodeFilter.FILTER_ACCEPT;
          }
        });
        let textNode;
        while ((textNode = walker.nextNode())) {
          const parent = textNode.parentElement;
          if (!parent || seen.has(parent)) continue;
          const trimmed = (parent.textContent || "").replace(/\s+/g, " ").trim();
          const match = trimmed.match(SOLANA_ADDRESS_REGEX);
          if (!match) continue;
          if (trimmed.length > match[0].length + 8) continue;
          seen.add(parent);
          result.push(parent);
        }
        return result;
      }

      function attachContractControlsToAnchor(anchor, mint) {
        const next = anchor.nextElementSibling;
        const signature = contractControlsSignature();
        if (
          anchor.dataset.trenchToolsJ7Processed === mint &&
          anchor.dataset.trenchToolsJ7ControlsSignature === signature &&
          next instanceof HTMLElement &&
          next.matches(TT_CONTRACT_WRAPPER_SELECTOR)
        ) {
          return;
        }
        if (next instanceof HTMLElement && next.matches(TT_CONTRACT_WRAPPER_SELECTOR)) {
          next.remove();
        }
        anchor.dataset.trenchToolsJ7Processed = mint;
        anchor.dataset.trenchToolsJ7ControlsSignature = signature;
        anchor.insertAdjacentElement("afterend", buildContractControls(anchor, mint));
        attachHoverPrewarm(anchor, mint);
      }

      function contractControlsSignature() {
        const features = j7Features();
        return [
          features.contractQuickBuy ? "buy" : "",
          features.contractQuickPanel ? "panel" : "",
          features.contractVamp ? "vamp" : ""
        ].filter(Boolean).join("|");
      }

      function buildContractControls(anchor, mint) {
        const features = j7Features();
        const wrapper = document.createElement("span");
        wrapper.setAttribute("data-trench-tools-j7-contract-controls", mint);
        Object.assign(wrapper.style, {
          display: "inline-flex",
          alignItems: "center",
          gap: "4px",
          marginLeft: "6px",
          verticalAlign: "middle"
        });
        if (features.contractQuickBuy) {
          const quickBuy = helpers.buildInlineButton(async () => {
            const tokenContext = await helpers.resolveInlineToken(mint, "contract_address");
            if (!tokenContext) return;
            await helpers.handleTradeRequest("buy", {
              ...helpers.state.preferences,
              buyAmountSol: helpers.resolveQuickBuyAmount()
            }, {
              persistPreferences: false,
              tokenContextOverride: tokenContext
            });
          }, contractButtonStyles());
          quickBuy.setAttribute("data-trench-tools-j7-contract-action", "quick-buy");
          wrapper.appendChild(quickBuy);
        }
        if (features.contractQuickPanel) {
          const panel = helpers.buildInlineIconButton(async () => {
            helpers.openInlinePanelForMint(mint, "contract_address", window.location.href, anchor);
          }, contractIconButtonStyles());
          panel.title = "Open Trench Tools panel";
          panel.setAttribute("data-trench-tools-j7-contract-action", "panel");
          wrapper.appendChild(panel);
        }
        if (features.contractVamp) {
          const vamp = buildMiniActionButton("Vamp", async () => {
            await helpers.openLaunchdeckOverlay({ mode: "create", contractAddress: mint });
          });
          vamp.setAttribute("data-trench-tools-j7-contract-action", "vamp");
          wrapper.appendChild(vamp);
        }
        return wrapper;
      }

      function contractButtonStyles() {
        const base = helpers.getQuickBuyBaseStyles();
        return {
          ...base,
          base: {
            ...base.base,
            height: "20px",
            minHeight: "20px",
            padding: "0 7px",
            borderRadius: "5px",
            fontSize: "11px",
            lineHeight: "1"
          },
          hover: {
            ...base.hover,
            height: "20px",
            minHeight: "20px"
          },
          logoSize: "12px",
          logoGap: "3px"
        };
      }

      function contractIconButtonStyles() {
        const styles = contractButtonStyles();
        return {
          ...styles,
          base: {
            ...styles.base,
            width: "22px",
            minWidth: "22px",
            padding: "0"
          },
          hover: {
            ...styles.hover,
            width: "22px",
            minWidth: "22px",
            padding: "0"
          },
          logoGap: "0px"
        };
      }

      function mountCardLaunchdeckControls(root) {
        const features = j7Features();
        if (!features.cardLaunchdeck) {
          document.querySelectorAll(TT_CARD_BUTTON_SELECTOR).forEach((element) => element.remove());
          document.querySelectorAll(TT_FALLBACK_DEPLOY_SECTION_SELECTOR).forEach((element) => element.remove());
          document.querySelectorAll(`[${TT_FALLBACK_DEPLOY_CLASS_ATTR}]`).forEach((element) => {
            const className = element.getAttribute(TT_FALLBACK_DEPLOY_CLASS_ATTR);
            if (className) element.classList.remove(className);
            element.removeAttribute(TT_FALLBACK_DEPLOY_CLASS_ATTR);
          });
          document.querySelectorAll(`[${TT_CARD_PADDED_ATTR}]`).forEach((element) => {
            restoreSideRailCardPadding(element);
          });
          document.querySelectorAll(`[${TT_RAIL_RESIZED_ATTR}]`).forEach((element) => {
            restoreSideRailSectionWidth(element);
          });
          document.querySelectorAll(`[${TT_VAMP_SHIFTED_ATTR}]`).forEach((element) => {
            restoreVampRightSideClearance(element);
          });
          document.querySelectorAll(ACTION_CONTAINER_SELECTOR).forEach((element) => {
            delete element.dataset.trenchToolsJ7CardProcessed;
            element.removeAttribute(ACTION_CONTAINER_ATTR);
          });
          restoreNativeCardActions();
          return;
        }

        for (const row of queryAll(root, CARD_ROW_SELECTOR)) {
          if (!(row instanceof HTMLElement)) continue;
          mountActionsForRow(row);
        }
      }

      function mountActionsForRow(row) {
        const containers = collectNativeContainers(row);
        let grouped = false;
        for (const [container, anchors] of containers) {
          if (anchors.deploy) mountDeployForContainer(container, anchors, row);
        }
        mountFallbackDeployForRow(row, containers);
        for (const [container, anchors] of containers) {
          if (anchors.vamp) mountVampForContainer(container, anchors, row);
          if (isGroupedContainer(container, anchors)) grouped = true;
        }
        for (const [container, anchors] of containers) {
          if (!anchors.vamp && !anchors.deploy) continue;
          container.dataset.trenchToolsJ7CardProcessed = "1";
          applyNativeVisibility(container);
          if (isGroupedContainer(container, anchors)) container.setAttribute("data-trench-tools-j7-grouped", "1");
          else container.removeAttribute("data-trench-tools-j7-grouped");
        }
        if (grouped) row.setAttribute("data-trench-tools-j7-grouped", "1");
        else row.removeAttribute("data-trench-tools-j7-grouped");
        syncCompactForRow(row);
      }

      function collectNativeContainers(row) {
        const map = new Map();
        for (const node of row.querySelectorAll(NATIVE_CARD_BUTTON_SELECTOR)) {
          if (!(node instanceof HTMLElement)) continue;
          if (node.hasAttribute("data-trench-tools-j7-card-action")) continue;
          const parent = node.parentElement;
          if (!parent) continue;
          if (!map.has(parent)) map.set(parent, { vamp: null, deploy: null });
          const entry = map.get(parent);
          if (!entry.vamp && node.matches(NATIVE_VAMP_SELECTOR)) entry.vamp = node;
          if (!entry.deploy && node.matches(NATIVE_DEPLOY_SELECTOR)) entry.deploy = node;
        }
        return map;
      }

      function mountDeployForContainer(container, anchors, row) {
        container.setAttribute(ACTION_CONTAINER_ATTR, "1");
        container.querySelectorAll(NATIVE_DEPLOY_CLOSE_SELECTOR).forEach((element) => element.remove());
        if (!anchors.deploy || container.querySelector(DIRECT_TT_DEPLOY_SELECTOR)) return;
        const inSideRail = isSideRailDeployButton(anchors.deploy);
        if (inSideRail) {
          applySideRailDeployWidth(anchors.deploy);
          applySideRailSectionWidth(container);
          applySideRailCardPadding(container);
        }
        const ttDeploy = cloneDeployActionButton(anchors.deploy, row);
        if (isGroupedContainer(container, anchors)) {
          container.appendChild(ttDeploy);
        } else {
          anchors.deploy.insertAdjacentElement("afterend", ttDeploy);
        }
        if (inSideRail) {
          positionTtSideRailButton(ttDeploy, container);
        }
      }

      function mountVampForContainer(container, anchors, row) {
        container.setAttribute(ACTION_CONTAINER_ATTR, "1");
        if (!anchors.vamp) return;
        applyVampRightSideClearance(anchors.vamp, row);
        if (container.querySelector(DIRECT_TT_VAMP_SELECTOR)) return;
        const ttVamp = buildVampActionButton(anchors.vamp, row);
        if (isGroupedContainer(container, anchors)) {
          const existingTtDeploy = container.querySelector(DIRECT_TT_DEPLOY_SELECTOR);
          if (existingTtDeploy) {
            container.insertBefore(ttVamp, existingTtDeploy);
          } else {
            container.appendChild(ttVamp);
          }
          ttVamp.style.setProperty("margin", `0 0 0 ${TOP_RIGHT_TT_GROUP_GAP_PX}px`, "important");
        } else {
          anchors.vamp.insertAdjacentElement("afterend", ttVamp);
        }
        if (hasRightSideDeployRail(row)) {
          positionTtVampOnRightRail(ttVamp, anchors.vamp, row);
        }
      }

      function getEffectiveZoom(element) {
        let zoom = 1;
        let el = element;
        while (el && el instanceof HTMLElement) {
          const z = parseFloat(window.getComputedStyle(el).zoom);
          if (Number.isFinite(z) && z > 0) zoom *= z;
          el = el.parentElement;
        }
        return zoom || 1;
      }

      function getRightSideDeployBlockLeft(row) {
        const card = row instanceof HTMLElement
          ? (row.matches(".tweet-card") ? row : row.closest(".tweet-card") || row.querySelector(":scope .tweet-card"))
          : null;
        if (!(card instanceof HTMLElement) || !card.classList.contains("deploy-right")) return null;
        const railSection = card.querySelector(".tweet-deploy-section");
        if (!(railSection instanceof HTMLElement)) return null;
        const railRect = railSection.getBoundingClientRect();
        let leftmost = railRect.left;
        for (const ttDeploy of card.querySelectorAll("[data-trench-tools-j7-card-action='deploy']")) {
          if (!(ttDeploy instanceof HTMLElement)) continue;
          const r = ttDeploy.getBoundingClientRect();
          if (r.width <= 0) continue;
          if (r.left < leftmost) leftmost = r.left;
        }
        return leftmost;
      }

      function positionTtVampOnRightRail(ttVamp, nativeVamp, row) {
        if (!(ttVamp instanceof HTMLElement)) return;
        const offsetParent = ttVamp.offsetParent instanceof HTMLElement ? ttVamp.offsetParent : ttVamp.parentElement;
        if (!(offsetParent instanceof HTMLElement)) return;
        const deployLeft = getRightSideDeployBlockLeft(row);
        if (deployLeft === null) return;
        const parentRect = offsetParent.getBoundingClientRect();
        const ttRect = ttVamp.getBoundingClientRect();
        const zoom = getEffectiveZoom(offsetParent);
        const gap = VAMP_RIGHT_SIDE_GAP_PX * zoom;
        const hideNative = Boolean(j7Features().hideNativeCardActions);
        let nativeWidth = 0;
        if (!hideNative && nativeVamp instanceof HTMLElement) {
          nativeWidth = nativeVamp.getBoundingClientRect().width;
        }
        const renderedDesiredRightEdge = nativeWidth > 0
          ? deployLeft - gap - nativeWidth - gap
          : deployLeft - gap;
        const renderedDesiredLeft = renderedDesiredRightEdge - ttRect.width;
        const renderedDeltaFromParent = renderedDesiredLeft - parentRect.left;
        const left = renderedDeltaFromParent / zoom;
        ttVamp.style.removeProperty("right");
        ttVamp.style.setProperty("left", `${left}px`, "important");
      }

      function buildVampActionButton(nativeButton, row) {
        const nativeStyle = window.getComputedStyle(nativeButton);
        const widthCss = nativeStyle.width;
        const heightCss = nativeStyle.height;
        const widthVal = parseFloat(widthCss) || 0;
        const heightVal = parseFloat(heightCss) || 0;
        const isAbsolute = nativeStyle.position === "absolute" || nativeStyle.position === "fixed";

        const button = document.createElement("button");
        button.type = "button";
        button.setAttribute("data-trench-tools-j7-card-action", "vamp");
        button.title = "Trench Tools Vamp from this tweet";

        const styles = [
          ["box-sizing", nativeStyle.boxSizing || "border-box"],
          ["display", "inline-flex"],
          ["align-items", "center"],
          ["justify-content", "center"],
          ["padding", nativeStyle.padding || "0"],
          ["width", widthCss],
          ["height", heightCss],
          ["min-width", widthCss],
          ["min-height", heightCss],
          ["border-width", nativeStyle.borderTopWidth || "1px"],
          ["border-style", nativeStyle.borderTopStyle && nativeStyle.borderTopStyle !== "none" ? nativeStyle.borderTopStyle : "solid"],
          ["border-color", "rgba(255,255,255,0.18)"],
          ["border-top-left-radius", "5px"],
          ["border-top-right-radius", "5px"],
          ["border-bottom-left-radius", "5px"],
          ["border-bottom-right-radius", "5px"],
          ["background", TT_BRAND_BG],
          ["color", TT_BRAND_FG],
          ["font-family", nativeStyle.fontFamily || "inherit"],
          ["font-size", nativeStyle.fontSize || "0"],
          ["line-height", "1"],
          ["cursor", "pointer"],
          ["transition", "background 0.15s, border-color 0.15s"],
          ["flex", "0 0 auto"],
          ["align-self", "center"],
          ["vertical-align", "middle"]
        ];

        if (isAbsolute) {
          const gap = 4;
          const nativeRightVal = parseFloat(nativeStyle.right);
          const nativeLeftVal = parseFloat(nativeStyle.left);
          styles.push(["position", "absolute"]);
          styles.push(["margin", "0"]);
          if (nativeStyle.top && nativeStyle.top !== "auto") styles.push(["top", nativeStyle.top]);
          else if (nativeStyle.bottom && nativeStyle.bottom !== "auto") styles.push(["bottom", nativeStyle.bottom]);
          if (Number.isFinite(nativeRightVal)) {
            styles.push(["right", `${nativeRightVal + widthVal + gap}px`]);
          } else if (Number.isFinite(nativeLeftVal)) {
            styles.push(["left", `${nativeLeftVal + widthVal + gap}px`]);
          } else {
            styles.push(["right", `${widthVal + gap}px`]);
            styles.push(["top", "0"]);
          }
          styles.push(["z-index", nativeStyle.zIndex && nativeStyle.zIndex !== "auto" ? nativeStyle.zIndex : "1"]);
        } else {
          styles.push(["margin", "0 0 0 4px"]);
        }

        for (const [prop, value] of styles) {
          if (value !== null && value !== undefined && value !== "") {
            button.style.setProperty(prop, value, "important");
          }
        }

        const padTop = parseFloat(nativeStyle.paddingTop) || 0;
        const padBottom = parseFloat(nativeStyle.paddingBottom) || 0;
        const innerHeight = Math.max(0, heightVal - padTop - padBottom);
        const iconSize = Math.max(12, Math.min(20, Math.round(innerHeight * 0.7))) || 14;
        button.innerHTML = vampIconSvg(String(iconSize));

        button.addEventListener("mouseenter", () => {
          button.style.setProperty("background", TT_BRAND_HOVER_BG, "important");
          button.style.setProperty("border-color", "rgba(255,255,255,0.32)", "important");
        });
        button.addEventListener("mouseleave", () => {
          button.style.setProperty("background", TT_BRAND_BG, "important");
          button.style.setProperty("border-color", "rgba(255,255,255,0.18)", "important");
        });
        button.addEventListener("click", async (event) => {
          event.preventDefault();
          event.stopPropagation();
          const context = await extractTweetContext(row);
          helpers
            .openLaunchdeckOverlay({ mode: "create", j7Context: context, action: "vamp-with-tweet" })
            .catch((error) => helpers.showToast?.(error?.message || "LaunchDeck action failed.", "error"));
        });

        return button;
      }

      function hasRightSideDeployRail(row) {
        const card = row instanceof HTMLElement
          ? row.matches(".tweet-card")
            ? row
            : row.closest(".tweet-card") || row.querySelector(":scope .tweet-card")
          : null;
        return card instanceof HTMLElement && card.classList.contains("deploy-right");
      }

      function applyVampRightSideClearance(nativeButton, row) {
        if (!(nativeButton instanceof HTMLElement)) return;
        if (!hasRightSideDeployRail(row)) {
          restoreVampRightSideClearance(nativeButton);
          return;
        }
        if (j7Features().hideNativeCardActions) {
          restoreVampRightSideClearance(nativeButton);
          return;
        }
        if (nativeButton.hasAttribute(TT_VAMP_SHIFTED_ATTR)) return;
        const deployLeft = getRightSideDeployBlockLeft(row);
        if (deployLeft === null) return;
        const vampRect = nativeButton.getBoundingClientRect();
        if (vampRect.width <= 0) return;
        const zoom = getEffectiveZoom(nativeButton);
        const desiredRightEdgeAbs = deployLeft - VAMP_RIGHT_SIDE_GAP_PX * zoom;
        const renderedShift = vampRect.right - desiredRightEdgeAbs;
        const logicalShift = renderedShift / zoom;
        const original = {
          transform: nativeButton.style.getPropertyValue("transform"),
          transition: nativeButton.style.getPropertyValue("transition")
        };
        nativeButton.setAttribute(TT_VAMP_SHIFTED_ATTR, JSON.stringify(original));
        nativeButton.style.setProperty("transition", "none", "important");
        if (Math.abs(logicalShift) >= 0.5) {
          nativeButton.style.setProperty("transform", `translateX(${-logicalShift}px)`, "important");
        }
        nativeButton.style.removeProperty("visibility");
      }

      function restoreVampRightSideClearance(nativeButton) {
        if (!(nativeButton instanceof HTMLElement)) return;
        nativeButton.style.removeProperty("visibility");
        const raw = nativeButton.getAttribute(TT_VAMP_SHIFTED_ATTR);
        if (raw === null) return;
        let saved = {};
        try { saved = JSON.parse(raw) || {}; } catch { saved = {}; }
        nativeButton.style.removeProperty("transform");
        nativeButton.style.removeProperty("transition");
        if (saved.transform) nativeButton.style.setProperty("transform", saved.transform);
        if (saved.transition) nativeButton.style.setProperty("transition", saved.transition);
        nativeButton.removeAttribute(TT_VAMP_SHIFTED_ATTR);
      }

      function cloneDeployActionButton(nativeButton, row) {
        const clone = nativeButton.cloneNode(false);
        clone.removeAttribute("class");
        clone.removeAttribute("id");
        clone.removeAttribute("name");
        clone.removeAttribute("data-mlw-tip");
        clone.removeAttribute("aria-label");
        clone.removeAttribute("aria-describedby");
        clone.setAttribute("data-trench-tools-j7-card-action", "deploy");
        clone.setAttribute("type", "button");
        clone.title = "Trench Tools deploy from this tweet";
        clone.style.background = TT_BRAND_BG;
        clone.style.color = TT_BRAND_FG;

        const parentClass = nativeButton.parentElement && typeof nativeButton.parentElement.className === "string"
          ? nativeButton.parentElement.className
          : "";
        const inSideRail = /\btweet-deploy-section\b/.test(parentClass);
        const ttLogoUrl = inSideRail ? safeRuntimeGetUrl("assets/TT-compact.png") : "";

        if (inSideRail && ttLogoUrl) {
          styleSideRailDeployButton(clone);
          clone.innerHTML = `<img src="${ttLogoUrl}" alt="TT" style="width:16px;height:16px;display:block;flex:none;object-fit:contain;filter:brightness(0) invert(1);pointer-events:none"/><span style="font-size:9px;font-weight:700;letter-spacing:0.04em;line-height:1">DEPLOY</span>`;
        } else {
          const nativeStyle = window.getComputedStyle(nativeButton);
          styleTopRightOrInlineDeployButton(clone, nativeStyle);
          const iconSize = nativeStyle.fontSize ? Math.max(14, Math.round(parseFloat(nativeStyle.fontSize) * 1.3)) : 18;
          clone.innerHTML = `${deployIconSvg(String(iconSize))}<span data-trench-tools-deploy-text style="font-weight:inherit;letter-spacing:0.04em">DEPLOY</span>`;
        }

        clone.addEventListener("mouseenter", () => {
          clone.style.background = TT_BRAND_HOVER_BG;
        });
        clone.addEventListener("mouseleave", () => {
          clone.style.background = TT_BRAND_BG;
        });
        clone.addEventListener("click", async (event) => {
          event.preventDefault();
          event.stopPropagation();
          const context = await extractTweetContext(row);
          helpers
            .openLaunchdeckOverlay({ mode: "create", j7Context: context })
            .catch((error) => helpers.showToast?.(error?.message || "LaunchDeck action failed.", "error"));
        });
        return clone;
      }

      function isSideRailDeployButton(button) {
        const parentClass = button.parentElement && typeof button.parentElement.className === "string"
          ? button.parentElement.className
          : "";
        return /\btweet-deploy-section\b/.test(parentClass);
      }

      function isGroupedContainer(container, anchors) {
        if (!(container instanceof HTMLElement)) return false;
        if (container.classList.contains("tweet-deploy-section")) return false;
        if (container.classList.contains(TOP_RIGHT_CONTAINER_CLASS)) return true;
        if (container.querySelector(":scope > .tweet-vamp-btn-topright, :scope > .tweet-deploy-btn-topright")) return true;
        return Boolean(anchors && anchors.vamp && anchors.deploy);
      }

      function applySideRailDeployWidth(button) {
        for (const property of ["width", "min-width", "max-width"]) {
          button.style.setProperty(property, SIDE_RAIL_DEPLOY_WIDTH, "important");
        }
        button.style.setProperty("border-radius", SIDE_RAIL_BORDER_RADIUS, "important");
      }

      function styleTopRightOrInlineDeployButton(clone, nativeStyle) {
        const important = [
          ["box-sizing", nativeStyle.boxSizing || "border-box"],
          ["display", "inline-flex"],
          ["align-items", "center"],
          ["justify-content", "center"],
          ["height", nativeStyle.height || "40px"],
          ["min-height", nativeStyle.height || "40px"],
          ["line-height", "1"],
          ["cursor", "pointer"],
          ["flex", "0 0 auto"],
          ["border", "none"],
          ["border-top-left-radius", "5px"],
          ["border-top-right-radius", "5px"],
          ["border-bottom-left-radius", "5px"],
          ["border-bottom-right-radius", "5px"],
          ["background", TT_BRAND_BG],
          ["color", TT_BRAND_FG]
        ];
        const responsive = [
          ["gap", nativeStyle.gap && nativeStyle.gap !== "normal" ? nativeStyle.gap : "8px"],
          ["padding", nativeStyle.padding || "11px 38px"],
          ["font-family", nativeStyle.fontFamily || "inherit"],
          ["font-size", nativeStyle.fontSize || "14px"],
          ["font-weight", nativeStyle.fontWeight || "700"]
        ];
        for (const [property, value] of important) {
          if (value !== null && value !== undefined && value !== "") {
            clone.style.setProperty(property, value, "important");
          }
        }
        for (const [property, value] of responsive) {
          if (value !== null && value !== undefined && value !== "") {
            clone.style.setProperty(property, value);
          }
        }
      }

      function styleSideRailDeployButton(button) {
        const declarations = [
          ["width", SIDE_RAIL_DEPLOY_WIDTH],
          ["min-width", SIDE_RAIL_DEPLOY_WIDTH],
          ["max-width", SIDE_RAIL_DEPLOY_WIDTH],
          ["height", "100%"],
          ["min-height", "100%"],
          ["display", "flex"],
          ["flex-direction", "column"],
          ["align-items", "center"],
          ["justify-content", "center"],
          ["gap", "4px"],
          ["padding", "0"],
          ["border", "none"],
          ["border-radius", SIDE_RAIL_BORDER_RADIUS],
          ["background", TT_BRAND_BG],
          ["color", TT_BRAND_FG],
          ["cursor", "pointer"],
          ["line-height", "1"]
        ];
        for (const [property, value] of declarations) {
          button.style.setProperty(property, value, "important");
        }
      }

      function getSideRailSide(section) {
        let element = section;
        while (element && element !== document.body) {
          if (element.classList) {
            if (element.classList.contains("deploy-left")) return "left";
            if (element.classList.contains("deploy-right")) return "right";
          }
          element = element.parentElement;
        }
        const cs = window.getComputedStyle(section);
        if (Number.isFinite(parseFloat(cs.right)) && parseFloat(cs.right) < 0) return "right";
        return "left";
      }

      function positionTtSideRailButton(button, section) {
        if (!(button instanceof HTMLElement) || !(section instanceof HTMLElement)) return;
        const side = getSideRailSide(section);
        button.style.setProperty("position", "absolute", "important");
        button.style.setProperty("top", "0", "important");
        button.style.setProperty("bottom", "0", "important");
        button.style.setProperty("height", "auto", "important");
        button.style.setProperty("min-height", "0", "important");
        button.style.setProperty("max-height", "none", "important");
        button.style.setProperty("width", SIDE_RAIL_DEPLOY_WIDTH, "important");
        button.style.setProperty("min-width", SIDE_RAIL_DEPLOY_WIDTH, "important");
        button.style.setProperty("max-width", SIDE_RAIL_DEPLOY_WIDTH, "important");
        button.style.setProperty("margin", "0", "important");
        if (side === "left") {
          button.style.setProperty("left", `${SIDE_RAIL_TT_OFFSET_PX}px`, "important");
          button.style.removeProperty("right");
        } else {
          button.style.setProperty("right", `${SIDE_RAIL_TT_OFFSET_PX}px`, "important");
          button.style.removeProperty("left");
        }
      }

      function applySideRailSectionWidth(section) {
        if (!(section instanceof HTMLElement)) return;
        if (section.hasAttribute(TT_RAIL_RESIZED_ATTR)) return;
        const side = getSideRailSide(section);
        const cs = window.getComputedStyle(section);
        const left = parseFloat(cs.left);
        const right = parseFloat(cs.right);
        const original = section.style.getPropertyValue("width");
        const originalLeft = section.style.getPropertyValue("left");
        const originalRight = section.style.getPropertyValue("right");
        section.setAttribute(TT_RAIL_RESIZED_ATTR, JSON.stringify({ width: original, left: originalLeft, right: originalRight }));
        section.style.setProperty("width", SIDE_RAIL_DEPLOY_WIDTH, "important");
        if (side === "left" && Number.isFinite(left)) {
          section.style.setProperty("left", `${left - SIDE_RAIL_OUTER_TRIM_PX}px`, "important");
        } else if (side === "right" && Number.isFinite(right)) {
          section.style.setProperty("right", `${right - SIDE_RAIL_OUTER_TRIM_PX}px`, "important");
        }
      }

      function restoreSideRailSectionWidth(section) {
        if (!(section instanceof HTMLElement)) return;
        const raw = section.getAttribute(TT_RAIL_RESIZED_ATTR);
        if (raw === null) return;
        let saved = {};
        try { saved = JSON.parse(raw) || {}; } catch { saved = {}; }
        section.style.removeProperty("width");
        section.style.removeProperty("left");
        section.style.removeProperty("right");
        if (saved.width) section.style.width = saved.width;
        if (saved.left) section.style.left = saved.left;
        if (saved.right) section.style.right = saved.right;
        section.removeAttribute(TT_RAIL_RESIZED_ATTR);
      }

      function applySideRailCardPadding(section) {
        const card = section instanceof HTMLElement ? section.closest(".tweet-card") : null;
        if (!(card instanceof HTMLElement)) return;
        if (card.hasAttribute(TT_CARD_PADDED_ATTR)) return;
        const isLeft = card.classList.contains("deploy-left");
        const isRight = card.classList.contains("deploy-right");
        if (!isLeft && !isRight) return;
        const property = isLeft ? "padding-left" : "padding-right";
        const original = card.style.getPropertyValue(property);
        card.setAttribute(TT_CARD_PADDED_ATTR, JSON.stringify({ property, original }));
        card.style.setProperty(property, `${SIDE_RAIL_CARD_PADDING_PX}px`, "important");
      }

      function restoreSideRailCardPadding(card) {
        if (!(card instanceof HTMLElement)) return;
        const raw = card.getAttribute(TT_CARD_PADDED_ATTR);
        if (raw === null) return;
        let saved = {};
        try { saved = JSON.parse(raw) || {}; } catch { saved = {}; }
        const property = saved.property === "padding-right" ? "padding-right" : "padding-left";
        card.style.removeProperty(property);
        if (saved.original) {
          card.style.setProperty(property, saved.original);
        }
        card.removeAttribute(TT_CARD_PADDED_ATTR);
      }

      function mountFallbackDeployForRow(row, containers) {
        const card = row.matches(".tweet-card") ? row : row.querySelector(":scope .tweet-card");
        if (!(card instanceof HTMLElement)) return;

        const settings = readJ7TrackerSettings();
        const position = String(settings.deployButtonPosition || "").toLowerCase();
        const className = position === "right" ? "deploy-right" : position === "left" ? "deploy-left" : "";
        const hasNativeDeploy = Array.from(containers.values()).some((entry) => entry.deploy) || Boolean(card.querySelector(NATIVE_DEPLOY_SELECTOR));
        const shouldMount = settings.disableDeployButton === true && Boolean(className) && !hasNativeDeploy;

        if (!shouldMount) {
          cleanupFallbackDeploy(card);
          return;
        }
        if (card.querySelector(DIRECT_TT_DEPLOY_SELECTOR) || card.querySelector(TT_FALLBACK_DEPLOY_SECTION_SELECTOR)) return;
        if (!card.querySelector(NATIVE_VAMP_SELECTOR)) return;

        card.classList.remove("deploy-left", "deploy-right", "deploy-top-right");
        card.classList.add(className);
        card.setAttribute(TT_FALLBACK_DEPLOY_CLASS_ATTR, className);

        const section = document.createElement("div");
        section.className = "tweet-deploy-section";
        section.setAttribute("data-trench-tools-j7-fallback-deploy-section", "1");
        section.setAttribute(ACTION_CONTAINER_ATTR, "1");
        section.dataset.trenchToolsJ7CardProcessed = "1";

        const button = buildFallbackDeployButton(row);
        section.appendChild(button);
        card.insertBefore(section, card.firstChild);
      }

      function cleanupFallbackDeploy(card) {
        card.querySelectorAll(TT_FALLBACK_DEPLOY_SECTION_SELECTOR).forEach((element) => element.remove());
        const className = card.getAttribute(TT_FALLBACK_DEPLOY_CLASS_ATTR);
        if (className) card.classList.remove(className);
        card.removeAttribute(TT_FALLBACK_DEPLOY_CLASS_ATTR);
      }

      function buildFallbackDeployButton(row) {
        const ttLogoUrl = safeRuntimeGetUrl("assets/TT-compact.png");
        const button = document.createElement("button");
        button.type = "button";
        button.setAttribute("data-trench-tools-j7-card-action", "deploy");
        button.title = "Trench Tools deploy from this tweet";
        styleSideRailDeployButton(button);
        button.innerHTML = ttLogoUrl
          ? `<img src="${ttLogoUrl}" alt="TT" style="width:16px;height:16px;display:block;flex:none;object-fit:contain;filter:brightness(0) invert(1);pointer-events:none"/><span style="font-size:9px;font-weight:700;letter-spacing:0.04em;line-height:1">DEPLOY</span>`
          : `${deployIconSvg("14")}<span style="font-size:9px;font-weight:700;letter-spacing:0.04em;line-height:1">DEPLOY</span>`;
        button.addEventListener("mouseenter", () => {
          button.style.background = TT_BRAND_HOVER_BG;
        });
        button.addEventListener("mouseleave", () => {
          button.style.background = TT_BRAND_BG;
        });
        button.addEventListener("click", async (event) => {
          event.preventDefault();
          event.stopPropagation();
          const context = await extractTweetContext(row);
          helpers
            .openLaunchdeckOverlay({ mode: "create", j7Context: context })
            .catch((error) => helpers.showToast?.(error?.message || "LaunchDeck action failed.", "error"));
        });
        return button;
      }

      function readJ7TrackerSettings() {
        try {
          return JSON.parse(localStorage.getItem("j7trackerSettings") || "{}") || {};
        } catch {
          return {};
        }
      }

      function safeRuntimeGetUrl(path) {
        try {
          return chrome.runtime.getURL(path);
        } catch {
          return "";
        }
      }

      function buildMiniActionButton(label, onClick) {
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = label;
        button.title = "Vamp token in LaunchDeck";
        Object.assign(button.style, {
          height: "20px",
          minHeight: "20px",
          borderRadius: "5px",
          border: "1px solid rgba(255,255,255,0.18)",
          background: "#000",
          color: "#fff",
          padding: "0 7px",
          fontSize: "11px",
          fontWeight: "700",
          cursor: "pointer",
          lineHeight: "1"
        });
        button.addEventListener("click", (event) => {
          event.preventDefault();
          event.stopPropagation();
          onClick().catch((error) => helpers.showToast?.(error?.message || "Vamp failed.", "error"));
        });
        return button;
      }

      function queryAll(root, selector) {
        const results = [];
        if (root instanceof Element && root.matches(selector)) {
          results.push(root);
        }
        const scope = root instanceof Document || root instanceof Element ? root : document;
        results.push(...scope.querySelectorAll(selector));
        return results;
      }

      function applyNativeVisibility(container) {
        container.querySelectorAll(DIRECT_NATIVE_CARD_BUTTON_SELECTOR).forEach((button) => {
          if (!(button instanceof HTMLElement)) return;
          if (button.hasAttribute("data-trench-tools-j7-card-action")) return;
          rememberNativeDisplay(button);
          if (j7Features().hideNativeCardActions) {
            button.style.display = "none";
          } else {
            restoreNativeButton(button, { keepCache: true });
          }
        });
      }

      function rememberNativeDisplay(button) {
        if (nativeStyleCache.has(button)) return;
        nativeStyleCache.set(button, { display: button.style.display });
      }

      function restoreNativeButton(button, { keepCache = false } = {}) {
        const cached = nativeStyleCache.get(button);
        if (!cached) return;
        button.style.display = cached.display;
        if (!keepCache) nativeStyleCache.delete(button);
      }

      function restoreNativeCardActions() {
        document.querySelectorAll(NATIVE_CARD_BUTTON_SELECTOR).forEach((button) => {
          if (!(button instanceof HTMLElement)) return;
          if (button.hasAttribute("data-trench-tools-j7-card-action")) return;
          restoreNativeButton(button);
        });
      }

      function ensureBodyClassObserver() {
        if (bodyClassObserver || !document.body) return;
        lastBodyClass = normalizeClassName(document.body.className);
        bodyClassObserver = new MutationObserver(() => {
          const next = normalizeClassName(document.body && document.body.className);
          if (next === lastBodyClass) return;
          lastBodyClass = next;
          scheduleRestamp();
        });
        bodyClassObserver.observe(document.body, { attributes: true, attributeFilter: ["class"] });
      }

      function disposeBodyClassObserver() {
        if (!bodyClassObserver) return;
        bodyClassObserver.disconnect();
        bodyClassObserver = null;
        lastBodyClass = "";
        if (restampTimer) {
          window.clearTimeout(restampTimer);
          restampTimer = null;
        }
      }

      function normalizeClassName(value) {
        return String(value || "")
          .split(/\s+/)
          .filter(Boolean)
          .sort()
          .join(" ");
      }

      function scheduleRestamp() {
        if (restampTimer) return;
        restampTimer = window.setTimeout(() => {
          restampTimer = null;
          if (!helpers.state.siteFeatures?.j7?.enabled) return;
          document.querySelectorAll(TT_CARD_BUTTON_SELECTOR).forEach((element) => element.remove());
          document.querySelectorAll(ACTION_CONTAINER_SELECTOR).forEach((element) => {
            delete element.dataset.trenchToolsJ7CardProcessed;
            element.removeAttribute(ACTION_CONTAINER_ATTR);
          });
          mountCardLaunchdeckControls(document);
          mountContractAddressControls(document);
        }, RESTAMP_DELAY_MS);
      }

      function closestTweetCard(element) {
        return element.closest(CARD_ROW_SELECTOR);
      }

      async function extractTweetContext(element) {
        const card = closestTweetCard(element) || element;
        const row = element.closest(CARD_ROW_WITH_ID_SELECTOR);
        const tweetId = row?.getAttribute("data-tweet-id") || card?.getAttribute("data-tweet-id") || "";
        const linkInfo = resolveTweetLinks(card, tweetId);
        const authorLink = card.querySelector("a.author-text");
        const classicHandle = extractHandle(authorLink?.getAttribute("href") || authorLink?.textContent || "");
        const handle = classicHandle || linkInfo.handle;
        const classicContextLink = card.querySelector('a.context-link[href*="/status/"]');
        const tweetUrl = classicContextLink?.href
          || linkInfo.tweetUrl
          || (tweetId && handle ? `https://twitter.com/${handle}/status/${tweetId}` : "");
        const classicText = card.querySelector(".tweet-content")?.textContent;
        const text = cleanText(classicText || card.textContent || "");
        const classicAuthorText = authorLink?.textContent;
        const authorText = cleanText(classicAuthorText || linkInfo.authorText || "");
        const externalLinks = Array.from(card.querySelectorAll("a.tweet-link.active-link[href]"))
          .map((link) => link.href)
          .filter(Boolean);
        const fallbackExternals = externalLinks.length ? externalLinks : linkInfo.externalLinks;
        return {
          source: "j7tracker",
          sourceUrl: window.location.href,
          tweetId,
          tweetUrl,
          authorText,
          handle,
          text,
          externalLinks: fallbackExternals,
          images: await extractImageCandidates(card)
        };
      }

      function resolveTweetLinks(card, tweetId) {
        const result = { handle: "", tweetUrl: "", authorText: "", externalLinks: [] };
        if (!card) return result;
        const externals = [];
        for (const link of card.querySelectorAll("a[href]")) {
          if (!(link instanceof HTMLAnchorElement)) continue;
          const href = link.href || "";
          if (!href) continue;
          const statusMatch = STATUS_LINK_REGEX.exec(href);
          if (statusMatch) {
            const candidateHandle = sanitizeHandle(statusMatch[1]);
            const candidateId = statusMatch[2];
            if (!result.tweetUrl && candidateHandle && candidateId && (!tweetId || candidateId === tweetId)) {
              result.handle = candidateHandle;
              result.tweetUrl = href;
            }
            continue;
          }
          const profileMatch = PROFILE_LINK_REGEX.exec(href);
          if (profileMatch) {
            const candidateHandle = sanitizeHandle(profileMatch[1]);
            if (!result.handle && candidateHandle) {
              result.handle = candidateHandle;
              const text = (link.textContent || "").trim();
              if (text && !result.authorText) result.authorText = text;
            }
            continue;
          }
          if (/^https?:/i.test(href)) externals.push(href);
        }
        result.externalLinks = externals;
        return result;
      }

      function sanitizeHandle(value) {
        const handle = String(value || "").replace(/^@+/, "").trim();
        if (!handle) return "";
        if (/^(home|explore|search|i|notifications|messages|settings|status)$/i.test(handle)) return "";
        return /^[A-Za-z0-9_]{1,30}$/.test(handle) ? handle : "";
      }

      async function extractImageCandidates(card) {
        const images = Array.from(card.querySelectorAll("img"))
          .filter((image) => image instanceof HTMLImageElement && image.src && isVisible(image) && !isInjectedTrenchToolsImage(image))
          .map((image, index) => buildImageCandidate(image, index));
        const settled = await Promise.allSettled(images);
        return settled
          .filter((result) => result.status === "fulfilled" && result.value)
          .map((result) => result.value);
      }

      function isInjectedTrenchToolsImage(image) {
        if (image.closest(TT_CARD_BUTTON_SELECTOR) || image.closest(TT_CONTRACT_WRAPPER_SELECTOR)) return true;
        const src = String(image.currentSrc || image.src || "");
        if (/chrome-extension:\/\/[^/]+\/(?:assets|images|launchdeck)\//i.test(src)) return true;
        if (/\bTT-compact\.png\b/i.test(src)) return true;
        return false;
      }

      async function buildImageCandidate(image, index) {
        return {
          id: `j7-${Date.now()}-${index}-${Math.random().toString(36).slice(2)}`,
          src: image.currentSrc || image.src,
          data: await dataUrlFromJ7Image(image),
          alt: image.alt || "",
          role: classifyImageRole(image),
          width: Math.round(image.getBoundingClientRect().width),
          height: Math.round(image.getBoundingClientRect().height),
          order: index
        };
      }

      function classifyImageRole(image) {
        if (image.classList.contains("profile-image")) return "profile";
        if (image.classList.contains("banner-image")) return "profile-banner";
        if (image.classList.contains("media-image")) return "media";
        const src = String(image.currentSrc || image.src || "");
        if (/\/profile_images\//i.test(src)) return "profile";
        if (/\/profile_banners\//i.test(src)) return "profile-banner";
        if (/\/(card_img|media|tweet_video_thumb|amplify_video_thumb|ext_tw_video_thumb)\//i.test(src)) return "media";
        return "external-card";
      }

      function isVisible(element) {
        const rect = element.getBoundingClientRect();
        const style = window.getComputedStyle(element);
        return rect.width > 0 && rect.height > 0 && style.display !== "none" && style.visibility !== "hidden";
      }

      function canvasDataUrlFromJ7Image(image) {
        if (!(image instanceof HTMLImageElement) || !image.complete || !image.naturalWidth || !image.naturalHeight) return "";
        try {
          const scale = Math.min(1, J7_IMAGE_CAPTURE_MAX_DIMENSION / Math.max(image.naturalWidth, image.naturalHeight, 1));
          const width = Math.max(1, Math.round(image.naturalWidth * scale));
          const height = Math.max(1, Math.round(image.naturalHeight * scale));
          const canvas = document.createElement("canvas");
          canvas.width = width;
          canvas.height = height;
          const context = canvas.getContext("2d");
          if (!context) return "";
          context.drawImage(image, 0, 0, width, height);
          return canvas.toDataURL("image/webp", 0.92);
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

      async function dataUrlFromJ7Image(image) {
        const source = String(image.currentSrc || image.src || "").trim();
        if (!source) return "";
        if (source.startsWith("data:image/")) return source;
        const canvasDataUrl = canvasDataUrlFromJ7Image(image);
        if (canvasDataUrl) return canvasDataUrl;
        try {
          const controller = new AbortController();
          const timer = window.setTimeout(() => controller.abort(), J7_IMAGE_CAPTURE_TIMEOUT_MS);
          try {
            const response = await fetch(source, {
              cache: "force-cache",
              credentials: "omit",
              signal: controller.signal
            });
            if (!response.ok) return "";
            const blob = await response.blob();
            if (!String(blob.type || "").startsWith("image/")) return "";
            return await blobToDataUrl(blob);
          } finally {
            window.clearTimeout(timer);
          }
        } catch (_error) {
          return "";
        }
      }

      function cleanText(value) {
        return String(value || "").replace(/\s+/g, " ").trim();
      }

      function extractHandle(value) {
        const match = String(value || "").match(/(?:twitter\.com|x\.com)\/@?([^/?\s]+)|@([A-Za-z0-9_]{1,30})/i);
        return match ? String(match[1] || match[2] || "").replace(/^@/, "") : "";
      }

      function deployIconSvg(size) {
        return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:${size}px;height:${size}px;pointer-events:none"><path d="M13 2L3 14h9l-1 8 10-12h-9l1-8z"></path></svg>`;
      }

      function vampIconSvg(size) {
        return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="width:${size}px;height:${size}px;pointer-events:none"><polyline points="14.5 17.5 3 6 3 3 6 3 17.5 14.5"></polyline><line x1="13" x2="19" y1="19" y2="13"></line><line x1="16" x2="20" y1="16" y2="20"></line><line x1="19" x2="21" y1="21" y2="19"></line><polyline points="14.5 6.5 18 3 21 3 21 6 17.5 9.5"></polyline><line x1="5" x2="9" y1="14" y2="18"></line><line x1="7" x2="4" y1="17" y2="20"></line><line x1="3" x2="5" y1="19" y2="21"></line></svg>`;
      }

      function attachHoverPrewarm(target, mint) {
        if (!(target instanceof HTMLElement)) {
          return;
        }
        if (target.dataset.trenchToolsJ7PrewarmWired === "1") {
          return;
        }
        target.dataset.trenchToolsJ7PrewarmWired = "1";
        let hoverTimer = null;
        const cancel = () => {
          if (hoverTimer) {
            clearTimeout(hoverTimer);
            hoverTimer = null;
          }
        };
        target.addEventListener("pointerenter", () => {
          cancel();
          hoverTimer = setTimeout(() => {
            hoverTimer = null;
            if (typeof helpers.prewarmForMint === "function") {
              helpers.prewarmForMint(mint, {
                sourceUrl: window.location.href,
                reason: "j7-hover"
              });
            }
          }, 200);
        });
        target.addEventListener("pointerleave", cancel);
      }

      return {
        isEnabled(siteFeatures) {
          return Boolean(siteFeatures?.j7?.enabled);
        },

        shouldMountLauncher(siteFeatures, tokenContext) {
          return Boolean(siteFeatures?.j7?.enabled) &&
            String(tokenContext?.surface || "").trim() === "contract_address";
        },

        shouldAutoOpenPanel(tokenContext) {
          return Boolean(tokenContext) && Boolean(helpers.state.siteFeatures?.j7?.enabled);
        },

        teardown: teardownInjectedControls,

        handleMutations(mutations) {
          const seenRows = new Set();
          for (const mutation of mutations || []) {
            for (const node of mutation.addedNodes || []) {
              if (!(node instanceof Element)) continue;
              prehideUnshiftedNativeVampsIn(node);
              mountContractAddressControls(node);
              mountCardLaunchdeckControls(node);
              const row = typeof node.closest === "function" ? node.closest(CARD_ROW_SELECTOR) : null;
              if (row instanceof HTMLElement && !seenRows.has(row)) {
                seenRows.add(row);
                mountActionsForRow(row);
              }
            }
          }
          ensureBodyClassObserver();
        },

        getQuickBuyStyles() {
          return helpers.getQuickBuyBaseStyles();
        },

        getCurrentTokenCandidate,
        mount
      };
    }
  });
})();
