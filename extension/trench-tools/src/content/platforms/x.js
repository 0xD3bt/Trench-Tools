(function registerTrenchToolsXAdapter() {
  const runtime = window.TrenchToolsContentRuntime;
  if (!runtime) {
    throw new Error("Trench Tools content runtime missing.");
  }

  runtime.registerPlatformAdapter("x", {
    matchesHost(hostname) {
      return hostname === "x.com" || hostname === "www.x.com";
    },

    createAdapter(helpers) {
      const SOLANA_ADDRESS_REGEX = /\b[1-9A-HJ-NP-Za-km-z]{32,44}\b/g;
      const STATUS_LINK_REGEX = /^https?:\/\/(?:www\.)?(?:x|twitter)\.com\/([^/?#\s]+)\/status\/(\d+)(?:[/?#].*)?$/i;
      const ADDRESS_TEXT_SELECTOR = 'div[data-testid="tweetText"], div[data-testid="UserDescription"]';
      const TWEET_ARTICLE_SELECTOR = "main article";
      const BYLINE_SELECTOR = 'div[data-testid="User-Name"]';
      const TT_ADDRESS_ATTR = "data-trench-tools-x-address-controls";
      const TT_ADDRESS_SELECTOR = `[${TT_ADDRESS_ATTR}]`;
      const TT_ADDRESS_SPAN_SELECTOR = "[data-trench-tools-x-address]";
      const TT_DEPLOY_ATTR = "data-trench-tools-x-deploy";
      const TT_DEPLOY_SELECTOR = `[${TT_DEPLOY_ATTR}]`;
      const TT_ADDRESS_PROCESSED_ATTR = "data-trench-tools-x-address-processed";
      const TT_DEPLOY_PROCESSED_ATTR = "data-trench-tools-x-deploy-processed";
      const TT_PREWARM_ATTR = "data-trench-tools-x-prewarm-wired";
      const TT_BRAND_BG = "#000000";
      const TT_BRAND_FG = "#ffffff";
      const TT_BRAND_HOVER_BG = "#1a1a1a";

      function features() {
        return helpers.state.siteFeatures?.x || {};
      }

      function isEnabled(siteFeatures = helpers.state.siteFeatures) {
        return Boolean(siteFeatures?.x?.enabled);
      }

      function buttonScale() {
        const platformScale = helpers.resolvePlatformButtonScale?.("x");
        const raw = platformScale ?? helpers.state.appearance?.platformButtonScales?.x ?? 1;
        const parsed = Number(raw);
        return Number.isFinite(parsed) ? Math.min(1.5, Math.max(0.75, parsed)) : 1;
      }

      function px(value) {
        return `${Math.round(Number(value || 0) * buttonScale())}px`;
      }

      function platformButtonDesign(type) {
        const design = helpers.resolvePlatformButtonDesign?.(type);
        return design && typeof design === "object"
          ? design
          : { color: TT_BRAND_FG, backgroundColor: TT_BRAND_BG, borderColor: "rgba(255, 255, 255, 0.18)" };
      }

      function platformButtonHoverBackground(type) {
        const background = String(platformButtonDesign(type).backgroundColor || TT_BRAND_BG).trim();
        return /^#[0-9a-f]{6,8}$/i.test(background) ? `${background.slice(0, 7)}cc` : TT_BRAND_HOVER_BG;
      }

      function platformButtonDesignSignature() {
        const design = platformButtonDesign("deploy");
        return [design.color || "", design.backgroundColor || "", design.borderColor || ""].join(":");
      }

      function teardownInjectedControls() {
        document.querySelectorAll(`${TT_ADDRESS_SELECTOR}, ${TT_DEPLOY_SELECTOR}`).forEach((element) => {
          element.remove();
        });
        document.querySelectorAll(`[${TT_ADDRESS_PROCESSED_ATTR}]`).forEach((element) => {
          element.removeAttribute(TT_ADDRESS_PROCESSED_ATTR);
        });
        document.querySelectorAll(`[${TT_DEPLOY_PROCESSED_ATTR}]`).forEach((element) => {
          element.removeAttribute(TT_DEPLOY_PROCESSED_ATTR);
        });
        document.querySelectorAll(`[${TT_PREWARM_ATTR}]`).forEach((element) => {
          element.removeAttribute(TT_PREWARM_ATTR);
        });
      }

      function getCurrentTokenCandidate() {
        const address = firstVisibleAddress(document);
        if (!address) {
          return null;
        }
        return {
          address,
          mint: address,
          source: "x",
          surface: "contract_address",
          url: window.location.href
        };
      }

      function mount() {
        if (!isEnabled()) {
          teardownInjectedControls();
          return;
        }
        mountAddressControls(document);
        mountTweetDeployControls(document);
      }

      function handleMutations(_mutations) {
        return false;
      }

      function mountAddressControls(root) {
        const activeFeatures = features();
        if (!activeFeatures.addressQuickBuy && !activeFeatures.addressQuickPanel && !activeFeatures.addressVamp && !activeFeatures.addressAxiom) {
          document.querySelectorAll(TT_ADDRESS_SELECTOR).forEach((element) => element.remove());
          queryAll(root, `[${TT_ADDRESS_PROCESSED_ATTR}]`).forEach((element) => {
            if (element instanceof HTMLElement) {
              element.removeAttribute(TT_ADDRESS_PROCESSED_ATTR);
            }
          });
          return;
        }
        queryAll(root, ADDRESS_TEXT_SELECTOR).forEach((container) => {
          if (!(container instanceof HTMLElement) || container.closest(TT_ADDRESS_SELECTOR)) {
            return;
          }
          processAddressContainer(container);
        });
      }

      function processAddressContainer(container) {
        if (container.getAttribute(TT_ADDRESS_PROCESSED_ATTR) === addressContainerSignature(container)) {
          return;
        }
        container.querySelectorAll(TT_ADDRESS_SELECTOR).forEach((element) => element.remove());
        container.setAttribute(TT_ADDRESS_PROCESSED_ATTR, addressContainerSignature(container));

        const existingAddressSpans = Array.from(container.querySelectorAll(TT_ADDRESS_SPAN_SELECTOR));
        if (existingAddressSpans.length) {
          for (const addressSpan of existingAddressSpans) {
            const address = addressSpan.getAttribute("data-trench-tools-x-address") || addressSpan.textContent || "";
            if (address && !addressSpan.nextElementSibling?.matches(TT_ADDRESS_SELECTOR)) {
              addressSpan.insertAdjacentElement("afterend", buildAddressControls(addressSpan, address));
            }
          }
          return;
        }

        const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT, {
          acceptNode(node) {
            if (!node.nodeValue || !SOLANA_ADDRESS_REGEX.test(node.nodeValue)) {
              SOLANA_ADDRESS_REGEX.lastIndex = 0;
              return NodeFilter.FILTER_REJECT;
            }
            SOLANA_ADDRESS_REGEX.lastIndex = 0;
            const parent = node.parentElement;
            if (!parent || shouldSkipAddressText(parent)) {
              return NodeFilter.FILTER_REJECT;
            }
            return NodeFilter.FILTER_ACCEPT;
          }
        });

        const nodes = [];
        while (walker.nextNode()) {
          nodes.push(walker.currentNode);
        }

        for (const node of nodes) {
          replaceAddressTextNode(node);
        }
      }

      function addressContainerSignature(container) {
        const activeFeatures = features();
        return [
          activeFeatures.addressQuickBuy ? "buy" : "",
          activeFeatures.addressQuickPanel ? "panel" : "",
          activeFeatures.addressVamp ? "vamp" : "",
          activeFeatures.addressAxiom ? "axiom" : "",
          buttonScale(),
          platformButtonDesignSignature(),
          naturalTextContent(container)
        ].join("|");
      }

      function naturalTextContent(container) {
        const clone = container.cloneNode(true);
        clone.querySelectorAll(TT_ADDRESS_SELECTOR).forEach((element) => element.remove());
        return clone.textContent || "";
      }

      function shouldSkipAddressText(element) {
        return Boolean(
          element.closest(
            [
              "a[href]",
              "button",
              "input",
              "textarea",
              "select",
              "[contenteditable='true']",
              TT_ADDRESS_SPAN_SELECTOR,
              TT_ADDRESS_SELECTOR,
              TT_DEPLOY_SELECTOR
            ].join(", ")
          )
        );
      }

      function replaceAddressTextNode(node) {
        const text = node.nodeValue || "";
        SOLANA_ADDRESS_REGEX.lastIndex = 0;
        let match;
        let lastIndex = 0;
        const fragment = document.createDocumentFragment();
        let replaced = false;

        while ((match = SOLANA_ADDRESS_REGEX.exec(text))) {
          const address = match[0];
          if (match.index > lastIndex) {
            fragment.appendChild(document.createTextNode(text.slice(lastIndex, match.index)));
          }
          const addressSpan = document.createElement("span");
          addressSpan.textContent = address;
          addressSpan.setAttribute("data-trench-tools-x-address", address);
          fragment.appendChild(addressSpan);
          fragment.appendChild(buildAddressControls(addressSpan, address));
          lastIndex = match.index + address.length;
          replaced = true;
        }

        if (!replaced) {
          return;
        }
        if (lastIndex < text.length) {
          fragment.appendChild(document.createTextNode(text.slice(lastIndex)));
        }
        node.replaceWith(fragment);
      }

      function buildAddressControls(anchor, mint) {
        const activeFeatures = features();
        const wrapper = document.createElement("span");
        wrapper.setAttribute(TT_ADDRESS_ATTR, mint);
        Object.assign(wrapper.style, {
          display: "inline-flex",
          alignItems: "center",
          gap: px(4),
          marginLeft: px(6),
          verticalAlign: "middle"
        });

        if (activeFeatures.addressQuickBuy) {
          const quickBuy = helpers.buildInlineButton(async () => {
            const tokenContext = await helpers.resolveInlineToken(mint, "contract_address", window.location.href, {
              source: "x"
            });
            if (!tokenContext) return;
            await helpers.handleTradeRequest("buy", {
              ...helpers.state.preferences,
              buyAmountSol: helpers.resolveQuickBuyAmount()
            }, {
              persistPreferences: false,
              tokenContextOverride: tokenContext
            });
          }, xQuickBuyButtonStyles());
          quickBuy.setAttribute("data-trench-tools-x-address-action", "quick-buy");
          wirePrewarm(quickBuy, mint, "contract_address");
          wrapper.appendChild(quickBuy);
        }

        if (activeFeatures.addressQuickPanel) {
          const panel = helpers.buildInlineIconButton(async () => {
            helpers.openInlinePanelForMint(mint, "contract_address", window.location.href, anchor, { source: "x" });
          }, xIconButtonStyles());
          panel.title = "Open Trench Tools panel";
          panel.setAttribute("data-trench-tools-x-address-action", "panel");
          wirePrewarm(panel, mint, "contract_address");
          wrapper.appendChild(panel);
        }

        if (activeFeatures.addressVamp) {
          const vamp = buildMiniActionButton("Vamp", async () => {
            await helpers.openLaunchdeckOverlay({ mode: "create", contractAddress: mint });
          }, "vamp");
          vamp.setAttribute("data-trench-tools-x-address-action", "vamp");
          wrapper.appendChild(vamp);
        }

        if (activeFeatures.addressAxiom) {
          const axiom = helpers.buildInlineIconButton(async () => {
            const tokenContext = await helpers.resolveInlineToken(mint, "contract_address", window.location.href, {
              source: "x"
            });
            if (!tokenContext) throw new Error("Token not found.");
            await helpers.openAxiomRoute(tokenContext);
          }, xIconButtonStyles());
          axiom.title = "Open on Axiom";
          axiom.setAttribute("aria-label", "Open on Axiom");
          axiom.setAttribute("data-trench-tools-x-address-action", "axiom");
          applyAxiomLogo(axiom);
          wirePrewarm(axiom, mint, "contract_address");
          wrapper.appendChild(axiom);
        }

        return wrapper;
      }

      function xQuickBuyButtonStyles() {
        const base = helpers.getQuickBuyBaseStyles();
        const styleSet = {
          ...base,
          base: {
            ...base.base,
            height: px(26),
            minHeight: px(26),
            padding: `0 ${px(8)}`,
            borderRadius: px(7),
            fontSize: px(14),
            marginLeft: "0px",
            marginRight: "0px",
            lineHeight: "1"
          },
          hover: {
            ...base.hover,
            height: px(26),
            minHeight: px(26)
          },
          logoSize: px(14),
          logoGap: px(4)
        };
        return helpers.applyPlatformButtonDesign?.(styleSet, "deploy") || styleSet;
      }

      function xIconButtonStyles() {
        const styles = xQuickBuyButtonStyles();
        const styleSet = {
          ...styles,
          base: {
            ...styles.base,
            width: px(26),
            minWidth: px(26),
            padding: "0"
          },
          hover: {
            ...styles.hover,
            width: px(26),
            minWidth: px(26),
            padding: "0"
          },
          logoGap: "0px"
        };
        return helpers.applyPlatformButtonDesign?.(styleSet, "panel") || styleSet;
      }

      function buildMiniActionButton(label, onClick, designType = "vamp") {
        const design = platformButtonDesign(designType);
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = label;
        Object.assign(button.style, {
          height: px(26),
          minHeight: px(26),
          padding: `0 ${px(8)}`,
          border: `1px solid ${design.borderColor || "rgba(255, 255, 255, 0.18)"}`,
          borderRadius: px(7),
          background: design.backgroundColor || TT_BRAND_BG,
          color: design.color || TT_BRAND_FG,
          fontSize: px(13),
          fontWeight: "600",
          lineHeight: "1",
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          cursor: "pointer",
          fontFamily: "inherit",
          boxSizing: "border-box",
          whiteSpace: "nowrap"
        });
        button.addEventListener("mouseenter", () => {
          button.style.background = platformButtonHoverBackground(designType);
        });
        button.addEventListener("mouseleave", () => {
          button.style.background = platformButtonDesign(designType).backgroundColor || TT_BRAND_BG;
        });
        button.addEventListener("click", (event) => {
          event.preventDefault();
          event.stopPropagation();
          Promise.resolve(onClick()).catch((error) => {
            helpers.showToast?.(error?.message || "Trench Tools action failed.", "error");
          });
        });
        return button;
      }

      function safeRuntimeGetUrl(path) {
        try {
          return chrome.runtime.getURL(path);
        } catch (_error) {
          return "";
        }
      }

      function applyAxiomLogo(button) {
        const logo = button?._trenchInlineLogo;
        const content = button?._trenchInlineContent;
        const logoUrl = safeRuntimeGetUrl("assets/Axiom-logo.jpg");
        if (!(logo instanceof HTMLElement) || !logoUrl) return;
        if (content instanceof HTMLElement) {
          Object.assign(content.style, {
            width: "100%",
            height: "100%"
          });
        }
        Object.assign(logo.style, {
          width: "100%",
          height: "100%",
          display: "block",
          flex: "1 1 auto",
          borderRadius: px(7),
          backgroundColor: "transparent",
          backgroundImage: `url("${logoUrl}")`,
          backgroundPosition: "center",
          backgroundRepeat: "no-repeat",
          backgroundSize: "cover",
          maskImage: "none",
          webkitMaskImage: "none"
        });
      }

      function wirePrewarm(element, mint, surface) {
        if (!(element instanceof HTMLElement) || element.hasAttribute(TT_PREWARM_ATTR)) {
          return;
        }
        element.setAttribute(TT_PREWARM_ATTR, "1");
        const prewarm = () => helpers.prewarmForMint?.(mint, { surface, sourceUrl: window.location.href });
        element.addEventListener("mouseenter", prewarm, { passive: true });
        element.addEventListener("focus", prewarm, { passive: true });
      }

      function mountTweetDeployControls(root) {
        if (!features().tweetDeploy) {
          document.querySelectorAll(TT_DEPLOY_SELECTOR).forEach((element) => element.remove());
          document.querySelectorAll(`[${TT_DEPLOY_PROCESSED_ATTR}]`).forEach((element) => {
            element.removeAttribute(TT_DEPLOY_PROCESSED_ATTR);
          });
          return;
        }
        queryAll(root, TWEET_ARTICLE_SELECTOR).forEach((article) => {
          if (article instanceof HTMLElement) {
            mountDeployForArticle(article);
          }
        });
      }

      function mountDeployForArticle(article) {
        if (article.parentElement?.closest("article")) {
          return;
        }
        if (article.getAttribute(TT_DEPLOY_PROCESSED_ATTR) === deployArticleSignature(article)) {
          return;
        }
        article.querySelectorAll(`:scope ${TT_DEPLOY_SELECTOR}`).forEach((element) => element.remove());
        article.setAttribute(TT_DEPLOY_PROCESSED_ATTR, deployArticleSignature(article));

        const byline = article.querySelector(BYLINE_SELECTOR);
        const statusLink = getArticleStatusLink(article);
        if (!(byline instanceof HTMLElement) || !statusLink) {
          return;
        }

        const button = buildDeployButton(article);
        const insertionTarget = findDeployInsertionTarget(byline);
        if (insertionTarget instanceof HTMLElement) {
          insertionTarget.appendChild(button);
        } else {
          byline.insertAdjacentElement("afterend", button);
        }
      }

      function deployArticleSignature(article) {
        const statusLink = getArticleStatusLink(article);
        return [
          statusLink?.href || "",
          buttonScale(),
          platformButtonDesignSignature(),
          features().tweetDeploy ? "deploy" : ""
        ].join("|");
      }

      function getArticleStatusLink(article) {
        const time = article.querySelector('a[href*="/status/"] time');
        const link = time?.closest("a[href]");
        if (!(link instanceof HTMLAnchorElement)) {
          return null;
        }
        const href = link.href || "";
        return STATUS_LINK_REGEX.test(href) ? link : null;
      }

      function findDeployInsertionTarget(byline) {
        if (byline.matches("[role='link']")) {
          return byline.parentElement;
        }
        const directFlex = Array.from(byline.children).find((child) => {
          if (!(child instanceof HTMLElement)) return false;
          const style = window.getComputedStyle(child);
          return style.display.includes("flex");
        });
        return directFlex instanceof HTMLElement ? directFlex : byline;
      }

      function buildDeployButton(article) {
        const button = buildMiniActionButton("Deploy", () => {
          const context = extractTweetContext(article);
          return helpers.openLaunchdeckOverlay({ mode: "create", j7Context: context });
        }, "deploy");
        button.setAttribute(TT_DEPLOY_ATTR, "1");
        button.setAttribute("aria-label", "Deploy from this X post");
        Object.assign(button.style, {
          marginLeft: px(6),
          flex: "0 0 auto"
        });
        return button;
      }

      function extractTweetContext(article) {
        const link = getArticleStatusLink(article);
        const statusMatch = link ? STATUS_LINK_REGEX.exec(link.href || "") : null;
        const handle = sanitizeHandle(statusMatch?.[1] || "");
        const tweetId = String(statusMatch?.[2] || "").trim();
        const tweetUrl = link?.href || (handle && tweetId ? `https://x.com/${handle}/status/${tweetId}` : "");
        const tweetTextElement = article.querySelector('div[data-testid="tweetText"]');
        const text = cleanText(tweetTextElement?.textContent || article.textContent || "");
        const authorText = cleanText(resolveAuthorText(article, handle));
        return {
          source: "x",
          sourceUrl: window.location.href,
          tweetId,
          tweetUrl,
          authorText,
          handle,
          text,
          externalLinks: extractExternalLinks(article, tweetUrl),
          images: extractImageCandidates(resolveImageCandidateArticles(article))
        };
      }

      function resolveImageCandidateArticles(article) {
        const connectedArticles = resolveConnectedArticleGroup(article);
        if (connectedArticles.length > 1) {
          return connectedArticles;
        }
        if (!isStatusPage()) {
          return [article];
        }
        const statusArticles = resolveStatusPageArticlePath(article);
        return statusArticles.length ? statusArticles : [article];
      }

      function resolveConnectedArticleGroup(article) {
        const main = article.closest("main") || document.querySelector("main");
        const articles = Array.from(main?.querySelectorAll("article") || [])
          .filter((candidate) =>
            candidate instanceof HTMLElement &&
            !candidate.parentElement?.closest("article") &&
            Boolean(getArticleStatusLink(candidate))
          );
        const index = articles.indexOf(article);
        if (index < 0) return [article];
        let start = index;
        while (start > 0 && articleCellConnectsToNext(articles[start - 1])) {
          start -= 1;
        }
        let end = index;
        while (end < articles.length - 1 && articleCellConnectsToNext(articles[end])) {
          end += 1;
        }
        return articles.slice(start, end + 1);
      }

      function resolveStatusPageArticlePath(article) {
        const pageStatusMatch = STATUS_LINK_REGEX.exec(window.location.href || "");
        const pageTweetId = String(pageStatusMatch?.[2] || "").trim();
        if (!pageTweetId) return [article];
        const main = article.closest("main") || document.querySelector("main");
        const articles = Array.from(main?.querySelectorAll("article") || [])
          .filter((candidate) =>
            candidate instanceof HTMLElement &&
            !candidate.parentElement?.closest("article") &&
            Boolean(getArticleStatusLink(candidate))
          );
        const clickedIndex = articles.indexOf(article);
        if (clickedIndex < 0) return [article];
        const pageIndex = articles.findIndex((candidate) => {
          const link = getArticleStatusLink(candidate);
          const match = link ? STATUS_LINK_REGEX.exec(link.href || "") : null;
          return String(match?.[2] || "").trim() === pageTweetId;
        });
        if (pageIndex < 0) return [article];
        const start = pageIndex === clickedIndex ? 0 : Math.min(pageIndex, clickedIndex);
        const end = Math.max(pageIndex, clickedIndex);
        const path = articles.slice(start, end + 1);
        return path.includes(article) ? path : [article];
      }

      function articleCellConnectsToNext(article) {
        const wrapper = article.closest('[data-testid="cellInnerDiv"]')?.firstElementChild;
        if (!(wrapper instanceof HTMLElement)) return false;
        const style = window.getComputedStyle(wrapper);
        return parseFloat(style.borderBottomWidth || "0") === 0;
      }

      function isStatusPage() {
        return STATUS_LINK_REGEX.test(window.location.href || "");
      }

      function resolveAuthorText(article, handle) {
        const byline = article.querySelector(BYLINE_SELECTOR);
        const profileLink = Array.from(byline?.querySelectorAll("a[href]") || []).find((link) => {
          if (!(link instanceof HTMLAnchorElement)) return false;
          const href = link.getAttribute("href") || "";
          return handle ? href.includes(`/${handle}`) : /^\/[A-Za-z0-9_]{1,30}/.test(href);
        });
        return profileLink?.textContent || byline?.textContent || "";
      }

      function extractExternalLinks(article, tweetUrl) {
        const urls = [];
        const seen = new Set();
        const normalizedTweetUrl = normalizeUrl(tweetUrl);
        for (const link of article.querySelectorAll("a[href]")) {
          if (!(link instanceof HTMLAnchorElement)) continue;
          const href = normalizeUrl(link.href || "");
          if (!href || href === normalizedTweetUrl || STATUS_LINK_REGEX.test(href)) continue;
          if (/^https?:\/\/(?:www\.)?(?:x|twitter)\.com\//i.test(href)) continue;
          if (seen.has(href)) continue;
          seen.add(href);
          urls.push(href);
        }
        return urls;
      }

      function extractImageCandidates(articles) {
        const candidates = [];
        const seen = new Set();
        const roots = Array.isArray(articles) && articles.length ? articles : [articles];
        for (const root of roots) {
          if (!(root instanceof HTMLElement)) continue;
          const sourceTweetUrl = getArticleStatusLink(root)?.href || "";
          for (const image of root.querySelectorAll("img")) {
            if (
              image instanceof HTMLImageElement &&
              image.src &&
              isVisible(image) &&
              isTweetImportImage(image) &&
              !isInjectedTrenchToolsImage(image)
            ) {
              addImageCandidate(candidates, seen, image.currentSrc || image.src, {
                alt: image.alt || "",
                role: classifyImageRole(image),
                sourceTweetUrl,
                width: Math.round(image.getBoundingClientRect().width),
                height: Math.round(image.getBoundingClientRect().height)
              });
            }
          }
          for (const video of root.querySelectorAll("video[poster]")) {
            if (!(video instanceof HTMLVideoElement) || !video.poster || !isVisible(video)) continue;
            addImageCandidate(candidates, seen, video.poster, {
              alt: "Video thumbnail",
              role: "video-thumbnail",
              sourceTweetUrl,
              width: Math.round(video.getBoundingClientRect().width),
              height: Math.round(video.getBoundingClientRect().height)
            });
          }
        }
        return candidates.map((candidate, index) => buildImageCandidate(candidate, index));
      }

      function addImageCandidate(candidates, seen, src, details = {}) {
        const originalSrc = String(src || "").trim();
        if (!originalSrc) return;
        const normalizedSrc = highQualityXImageUrl(originalSrc);
        const key = normalizedSrc || originalSrc;
        if (seen.has(key)) return;
        seen.add(key);
        candidates.push({
          src: normalizedSrc || originalSrc,
          originalSrc,
          alt: details.alt || "",
          role: details.role || "media",
          sourceTweetUrl: details.sourceTweetUrl || "",
          width: details.width || 0,
          height: details.height || 0
        });
      }

      function buildImageCandidate(candidate, index) {
        return {
          id: `x-${Date.now()}-${index}-${Math.random().toString(36).slice(2)}`,
          src: candidate.src,
          originalSrc: candidate.originalSrc || "",
          data: "",
          alt: candidate.alt || "",
          role: candidate.role || "media",
          sourceTweetUrl: candidate.sourceTweetUrl || "",
          width: candidate.width || 0,
          height: candidate.height || 0,
          order: index
        };
      }

      function isTweetImportImage(image) {
        const src = String(image.currentSrc || image.src || "");
        if (/abs\.twimg\.com\/emoji\//i.test(src)) {
          return false;
        }
        if (image.closest('[data-testid="tweetText"]')) {
          return false;
        }
        if (isXProfileImage(image)) {
          return true;
        }
        if (image.closest('[data-testid="tweetPhoto"], [data-testid="videoComponent"], [data-testid="card.wrapper"], [data-testid*="card."]')) {
          return true;
        }
        return /\/(media|card_img|profile_images|tweet_video_thumb|amplify_video_thumb|ext_tw_video_thumb)\//i.test(src);
      }

      function classifyImageRole(image) {
        const src = String(image.currentSrc || image.src || "");
        if (isXProfileImage(image)) return "avatar";
        if (/\/card_img\//i.test(src) || image.closest('[data-testid="card.wrapper"], [data-testid*="card."]')) return "card";
        if (/\/(media|tweet_video_thumb|amplify_video_thumb|ext_tw_video_thumb)\//i.test(src)) return "media";
        return "external-card";
      }

      function isXProfileImage(image) {
        const src = String(image.currentSrc || image.src || "");
        return /\/profile_images\//i.test(src) || Boolean(image.closest('[data-testid="UserAvatar-Container"], [data-testid="UserAvatar"]'));
      }

      function highQualityXImageUrl(src) {
        const raw = String(src || "").trim();
        if (!raw || !/^https?:\/\/[^/]*pbs\.twimg\.com\//i.test(raw)) return raw;
        try {
          const url = new URL(raw);
          if (/\/profile_images\//i.test(url.pathname)) {
            url.pathname = url.pathname.replace(/_(?:mini|normal|bigger)(\.[A-Za-z0-9]+)$/i, "$1");
            return url.toString();
          }
          if (/\/media\//i.test(url.pathname)) {
            if (url.searchParams.has("name")) url.searchParams.set("name", "orig");
            return url.toString();
          }
          if (/\/(card_img|tweet_video_thumb|amplify_video_thumb|ext_tw_video_thumb)\//i.test(url.pathname)) {
            if (url.searchParams.has("name")) url.searchParams.set("name", "large");
            return url.toString();
          }
          return url.toString();
        } catch (_error) {
          return raw;
        }
      }

      function isInjectedTrenchToolsImage(image) {
        if (image.closest(TT_ADDRESS_SELECTOR) || image.closest(TT_DEPLOY_SELECTOR)) return true;
        const src = String(image.currentSrc || image.src || "");
        if (/chrome-extension:\/\/[^/]+\/(?:assets|images|launchdeck)\//i.test(src)) return true;
        if (/\bTT-compact\.png\b/i.test(src)) return true;
        return false;
      }

      function firstVisibleAddress(root) {
        for (const container of queryAll(root, ADDRESS_TEXT_SELECTOR)) {
          if (!(container instanceof HTMLElement) || !isVisible(container)) continue;
          SOLANA_ADDRESS_REGEX.lastIndex = 0;
          const match = SOLANA_ADDRESS_REGEX.exec(container.textContent || "");
          SOLANA_ADDRESS_REGEX.lastIndex = 0;
          if (match?.[0]) return match[0];
        }
        return "";
      }

      function isVisible(element) {
        const rect = element.getBoundingClientRect();
        const style = window.getComputedStyle(element);
        return rect.width > 0 && rect.height > 0 && style.display !== "none" && style.visibility !== "hidden";
      }

      function queryAll(root, selector) {
        if (!root) return [];
        const results = [];
        if (root instanceof Element && root.matches(selector)) {
          results.push(root);
        }
        if (typeof root.querySelectorAll === "function") {
          results.push(...root.querySelectorAll(selector));
        }
        return results;
      }

      function cleanText(value) {
        return String(value || "").replace(/\s+/g, " ").trim();
      }

      function sanitizeHandle(value) {
        const handle = String(value || "").replace(/^@+/, "").trim();
        if (!handle) return "";
        if (/^(home|explore|search|i|notifications|messages|settings|status)$/i.test(handle)) return "";
        return /^[A-Za-z0-9_]{1,30}$/.test(handle) ? handle : "";
      }

      function normalizeUrl(value) {
        try {
          const url = new URL(String(value || ""), window.location.href);
          url.hash = "";
          return url.toString();
        } catch (_error) {
          return "";
        }
      }

      return {
        isEnabled,
        shouldMountLauncher() {
          return false;
        },
        shouldAutoOpenPanel() {
          return false;
        },
        mount,
        handleMutations,
        getCurrentTokenCandidate,
        teardown: teardownInjectedControls
      };
    }
  });
})();
