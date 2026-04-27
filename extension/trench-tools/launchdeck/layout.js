(function initLaunchDeckLayout(global) {
  if (global.LaunchDeckLayout) {
    return;
  }

  const TOKENS = Object.freeze({
    createOverlay: Object.freeze({
      width: 532,
      height: 672,
      viewportGap: 64,
      sizeEpsilon: 2,
      background: "linear-gradient(180deg, rgba(13, 13, 13, 0.99), rgba(9, 9, 9, 1))",
    }),
    popout: Object.freeze({
      outerWidth: 552,
      outerHeight: 727,
      formWidth: 532,
      stableContentHeight: 651,
      reportsWidth: 560,
      workspaceGap: 12,
      minOuterWidth: 420,
      minOuterHeight: 560,
      screenMargin: 24,
      sizePadding: 4,
    }),
    modal: Object.freeze({
      overlayPadding: 24,
      popoutOverlayPadding: 16,
      vampWidth: 420,
      settingsWidth: 500,
    }),
  });

  function isHtmlElement(node) {
    if (!node || typeof node !== "object") {
      return false;
    }
    if (typeof global.HTMLElement === "function") {
      return node instanceof global.HTMLElement;
    }
    return typeof node.tagName === "string" && typeof node.style === "object";
  }

  function isElementActuallyVisible(node) {
    if (!isHtmlElement(node) || node.hidden || typeof global.getComputedStyle !== "function") {
      return false;
    }
    const styles = global.getComputedStyle(node);
    return styles.display !== "none" && styles.visibility !== "hidden";
  }

  function getVisibleChildElements(container) {
    if (!isHtmlElement(container)) {
      return [];
    }
    return Array.from(container.children).filter((node) => isElementActuallyVisible(node));
  }

  function measureElementBox(element) {
    if (!isHtmlElement(element) || typeof element.getBoundingClientRect !== "function") {
      return { width: 0, height: 0 };
    }
    const rect = element.getBoundingClientRect();
    return {
      width: Math.ceil(rect.width),
      height: Math.ceil(rect.height),
    };
  }

  function measureVisibleModalOverlayContent(root = global.document) {
    if (!root || typeof root.querySelectorAll !== "function") {
      return { width: 0, height: 0 };
    }
    const overlays = Array.from(root.querySelectorAll(".modal-overlay"));
    return overlays.reduce((largest, overlay) => {
      if (!isElementActuallyVisible(overlay)) {
        return largest;
      }
      const overlayStyles = global.getComputedStyle(overlay);
      const modal = overlay.querySelector(".modal");
      const target = isHtmlElement(modal) ? modal : overlay;
      const rect = measureElementBox(target);
      const paddingX = (Number.parseFloat(overlayStyles.paddingLeft || "0") || 0)
        + (Number.parseFloat(overlayStyles.paddingRight || "0") || 0);
      const paddingY = (Number.parseFloat(overlayStyles.paddingTop || "0") || 0)
        + (Number.parseFloat(overlayStyles.paddingBottom || "0") || 0);
      return {
        width: Math.max(largest.width, Math.ceil(rect.width + paddingX)),
        height: Math.max(largest.height, Math.ceil(rect.height + paddingY)),
      };
    }, { width: 0, height: 0 });
  }

  function getCreateOverlayStableSize() {
    return {
      width: TOKENS.createOverlay.width,
      height: TOKENS.createOverlay.height,
    };
  }

  function getPopoutBaseContentSize({ formVisible = true, reportsVisible = false } = {}) {
    let width = 0;
    if (formVisible) {
      width += TOKENS.popout.formWidth;
    }
    if (reportsVisible) {
      width += width > 0 ? TOKENS.popout.workspaceGap : 0;
      width += TOKENS.popout.reportsWidth;
    }
    return {
      width,
      height: TOKENS.popout.stableContentHeight,
    };
  }

  function clampPopupOuterSize(width, height, screenObj = global.screen) {
    const maxOuterWidth = Math.max(
      TOKENS.popout.minOuterWidth,
      (screenObj?.availWidth || TOKENS.popout.minOuterWidth) - TOKENS.popout.screenMargin,
    );
    const maxOuterHeight = Math.max(
      TOKENS.popout.minOuterHeight,
      (screenObj?.availHeight || TOKENS.popout.minOuterHeight) - TOKENS.popout.screenMargin,
    );
    return {
      width: Math.min(Math.max(TOKENS.popout.minOuterWidth, Math.round(width)), maxOuterWidth),
      height: Math.min(Math.max(TOKENS.popout.minOuterHeight, Math.round(height)), maxOuterHeight),
    };
  }

  function computeCenteredPopupPosition(width, height, screenObj = global.screen) {
    return {
      left: Math.max(0, Math.round(((screenObj?.availWidth || width) - width) / 2)),
      top: Math.max(0, Math.round(((screenObj?.availHeight || height) - height) / 2)),
    };
  }

  function getDefaultPopoutOuterSize(mode = "webapp", screenObj = global.screen) {
    if (String(mode || "").trim().toLowerCase() === "webapp") {
      return clampPopupOuterSize(TOKENS.popout.outerWidth, TOKENS.popout.outerHeight, screenObj);
    }
    return clampPopupOuterSize(1440, 960, screenObj);
  }

  function applyDocumentLayoutTokens() {
    const root = global.document?.documentElement;
    if (!isHtmlElement(root)) {
      return;
    }
    root.style.setProperty("--launchdeck-create-overlay-width", `${TOKENS.createOverlay.width}px`);
    root.style.setProperty("--launchdeck-create-overlay-height", `${TOKENS.createOverlay.height}px`);
    root.style.setProperty("--launchdeck-popout-form-width", `${TOKENS.popout.formWidth}px`);
    root.style.setProperty("--launchdeck-popout-stable-content-height", `${TOKENS.popout.stableContentHeight}px`);
    root.style.setProperty("--launchdeck-popout-reports-width", `${TOKENS.popout.reportsWidth}px`);
    root.style.setProperty("--launchdeck-popout-workspace-gap", `${TOKENS.popout.workspaceGap}px`);
    root.style.setProperty("--launchdeck-vamp-modal-width", `${TOKENS.modal.vampWidth}px`);
    root.style.setProperty("--launchdeck-settings-modal-popout-width", `${TOKENS.modal.settingsWidth}px`);
  }

  function computePopoutTargetOuterSize({
    chromeWidth = 0,
    chromeHeight = 0,
    formVisible = true,
    reportsVisible = false,
    measuredContentHeight = 0,
    modalWidth = 0,
    modalHeight = 0,
    screenObj = global.screen,
  } = {}) {
    const base = getPopoutBaseContentSize({ formVisible, reportsVisible });
    const contentWidth = Math.max(base.width, Math.ceil(modalWidth || 0));
    const contentHeight = Math.max(
      base.height,
      Math.ceil(measuredContentHeight || 0),
      Math.ceil(modalHeight || 0),
    );
    return clampPopupOuterSize(
      contentWidth + Math.max(0, chromeWidth) + TOKENS.popout.sizePadding,
      contentHeight + Math.max(0, chromeHeight) + TOKENS.popout.sizePadding,
      screenObj,
    );
  }

  global.LaunchDeckLayout = Object.freeze({
    applyDocumentLayoutTokens,
    TOKENS,
    clampPopupOuterSize,
    computeCenteredPopupPosition,
    computePopoutTargetOuterSize,
    getCreateOverlayStableSize,
    getDefaultPopoutOuterSize,
    getPopoutBaseContentSize,
    getVisibleChildElements,
    isElementActuallyVisible,
    measureElementBox,
    measureVisibleModalOverlayContent,
  });

  applyDocumentLayoutTokens();
})(globalThis);
