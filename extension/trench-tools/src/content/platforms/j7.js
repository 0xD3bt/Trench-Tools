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
      function getCurrentTokenCandidate() {
        const address =
          helpers.extractMintFromUrl(window.location.href) ||
          helpers.extractMintFromSelectors([
            "[data-address]",
            "[data-copy]",
            "code",
            "[title]"
          ]) ||
          helpers.extractMintFromText(document.body?.innerText || "");

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
        document.querySelectorAll("[data-trench-tools-inline]").forEach((element) => {
          const target = element.previousElementSibling;
          if (target instanceof HTMLElement) {
            delete target.dataset.trenchToolsMounted;
            delete target.dataset.trenchToolsJ7PrewarmWired;
          }
          if (typeof helpers.teardownInlineSizeSync === "function") {
            helpers.teardownInlineSizeSync(element);
          }
          element.remove();
        });
      }

      function mount() {
        if (!helpers.state.siteFeatures?.j7?.enabled) {
          teardownInjectedControls();
          return;
        }

        const mint =
          helpers.extractMintFromUrl(window.location.href) ||
          helpers.extractMintFromText(document.body?.innerText || "");
        if (!mint) {
          return;
        }

        const target = helpers.findElementShowingMint(mint);
        if (target) {
          helpers.injectInlineControls(target, mint, "contract_address");
          attachHoverPrewarm(target, mint);
        }
      }

      // Debounced hover-with-intent prewarm: a quick mouseover is
      // ignored, a sustained pointerenter (200ms) kicks off one
      // prewarm. Matches the Axiom pulse-card pattern but driven from
      // whatever DOM node holds the contract address on J7.
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

        getQuickBuyStyles() {
          return helpers.getQuickBuyBaseStyles();
        },

        getCurrentTokenCandidate,
        mount
      };
    }
  });
})();
