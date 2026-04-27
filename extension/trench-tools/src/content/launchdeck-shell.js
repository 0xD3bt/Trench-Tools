import "../../launchdeck/layout.js";

export function createLaunchdeckShellController() {
  const Layout = globalThis.LaunchDeckLayout || {};
  const layoutTokens = Layout.TOKENS || {};
  const createOverlayTokens = layoutTokens.createOverlay || {};
  const OVERLAY_WRAPPER_ID = "trench-tools-launchdeck-overlay";
  const OVERLAY_FRAME_ID = "trench-tools-launchdeck-frame";
  const CREATE_OVERLAY_RESIZE_MESSAGE_SOURCE = "trench-tools-launchdeck";
  const CREATE_OVERLAY_RESIZE_MESSAGE_TYPE = "resize-create-overlay";
  const CREATE_OVERLAY_VIEWPORT_GAP = createOverlayTokens.viewportGap || 64;
  const CREATE_OVERLAY_DEFAULT_WIDTH = createOverlayTokens.width || 532;
  const CREATE_OVERLAY_DEFAULT_HEIGHT = createOverlayTokens.height || 717;
  const CREATE_OVERLAY_SIZE_EPSILON = createOverlayTokens.sizeEpsilon || 2;
  const CREATE_OVERLAY_BACKGROUND = createOverlayTokens.background || "linear-gradient(180deg, rgba(13, 13, 13, 0.99), rgba(9, 9, 9, 1))";
  let overlayMessageListener = null;

  function applyCreateOverlaySize(iframe, width, height) {
    const numericWidth = Number(width);
    const numericHeight = Number(height);
    if (!Number.isFinite(numericWidth) || !Number.isFinite(numericHeight)) {
      return;
    }
    const maxWidth = Math.max(280, window.innerWidth - CREATE_OVERLAY_VIEWPORT_GAP);
    const maxHeight = Math.max(360, window.innerHeight - CREATE_OVERLAY_VIEWPORT_GAP);
    const nextWidth = Math.min(maxWidth, Math.max(320, Math.ceil(numericWidth)));
    const nextHeight = Math.min(maxHeight, Math.max(420, Math.ceil(numericHeight)));
    const currentWidth = Number.parseFloat(iframe.dataset.createOverlayWidth || "");
    const currentHeight = Number.parseFloat(iframe.dataset.createOverlayHeight || "");
    if (
      Number.isFinite(currentWidth)
      && Number.isFinite(currentHeight)
      && Math.abs(currentWidth - nextWidth) < CREATE_OVERLAY_SIZE_EPSILON
      && Math.abs(currentHeight - nextHeight) < CREATE_OVERLAY_SIZE_EPSILON
    ) {
      return;
    }
    iframe.dataset.createOverlayWidth = String(nextWidth);
    iframe.dataset.createOverlayHeight = String(nextHeight);
    iframe.style.width = `${nextWidth}px`;
    iframe.style.height = `${nextHeight}px`;
    iframe.style.maxWidth = `calc(100vw - ${CREATE_OVERLAY_VIEWPORT_GAP}px)`;
    iframe.style.maxHeight = `calc(100vh - ${CREATE_OVERLAY_VIEWPORT_GAP}px)`;
  }

  function attachOverlayMessageListener() {
    if (overlayMessageListener) return;
    overlayMessageListener = (event) => {
      const iframe = document.getElementById(OVERLAY_FRAME_ID);
      if (!(iframe instanceof HTMLIFrameElement)) return;
      if (event.source !== iframe.contentWindow) return;
      const data = event.data;
      if (!data || typeof data !== "object") return;
      if (data.source !== CREATE_OVERLAY_RESIZE_MESSAGE_SOURCE) return;
      if (data.type !== CREATE_OVERLAY_RESIZE_MESSAGE_TYPE) return;
      if (iframe.dataset.overlayMode !== "create") return;
      applyCreateOverlaySize(iframe, data.width, data.height);
    };
    window.addEventListener("message", overlayMessageListener);
  }

  function detachOverlayMessageListener() {
    if (!overlayMessageListener) {
      return;
    }
    window.removeEventListener("message", overlayMessageListener);
    overlayMessageListener = null;
  }

  function applyOverlayPresentation(wrapper, iframe, mode) {
    const isCreateOverlay = mode === "create";
    const storedCreateWidth = Number.parseFloat(iframe.dataset.createOverlayWidth || "");
    const storedCreateHeight = Number.parseFloat(iframe.dataset.createOverlayHeight || "");
    wrapper.dataset.overlayDisplay = isCreateOverlay ? "flex" : "block";
    iframe.dataset.overlayMode = mode;
    Object.assign(wrapper.style, {
      display: "none",
      alignItems: "center",
      justifyContent: "center",
      padding: isCreateOverlay ? "32px" : "0",
      background: isCreateOverlay ? "rgba(4, 6, 10, 0.22)" : "rgba(0, 0, 0, 0.86)",
      backdropFilter: isCreateOverlay ? "blur(18px) saturate(1.02)" : "blur(2px)",
      transition: "background 160ms ease, backdrop-filter 160ms ease, opacity 160ms ease",
    });
    Object.assign(iframe.style, {
      position: isCreateOverlay ? "relative" : "absolute",
      inset: isCreateOverlay ? "auto" : "16px",
      width: isCreateOverlay
        ? (Number.isFinite(storedCreateWidth) && storedCreateWidth > 0
            ? `${storedCreateWidth}px`
            : `min(${CREATE_OVERLAY_DEFAULT_WIDTH}px, calc(100vw - ${CREATE_OVERLAY_VIEWPORT_GAP}px))`)
        : "calc(100vw - 32px)",
      height: isCreateOverlay
        ? (Number.isFinite(storedCreateHeight) && storedCreateHeight > 0
            ? `${storedCreateHeight}px`
            : `min(${CREATE_OVERLAY_DEFAULT_HEIGHT}px, calc(100vh - ${CREATE_OVERLAY_VIEWPORT_GAP}px))`)
        : "calc(100vh - 32px)",
      maxWidth: isCreateOverlay ? `calc(100vw - ${CREATE_OVERLAY_VIEWPORT_GAP}px)` : "",
      maxHeight: isCreateOverlay ? `calc(100vh - ${CREATE_OVERLAY_VIEWPORT_GAP}px)` : "",
      borderRadius: isCreateOverlay ? "24px" : "18px",
      background: isCreateOverlay ? CREATE_OVERLAY_BACKGROUND : "transparent",
      boxShadow: isCreateOverlay
        ? "0 34px 90px rgba(0, 0, 0, 0.38)"
        : "0 28px 80px rgba(0, 0, 0, 0.52)",
      transition: isCreateOverlay
        ? "box-shadow 180ms ease, transform 180ms ease"
        : "box-shadow 180ms ease",
      willChange: isCreateOverlay ? "transform" : "auto",
    });
  }

  function buildShellUrl({ shell, mode, contractAddress = "", instaLaunch = false } = {}) {
    const url = new URL(chrome.runtime.getURL("launchdeck/index.html"));
    url.searchParams.set("shell", shell || "overlay");
    url.searchParams.set("mode", mode || "webapp");
    if (contractAddress) {
      url.searchParams.set("contractAddress", String(contractAddress).trim());
    }
    if (instaLaunch) {
      url.searchParams.set("instaLaunch", "1");
    }
    if (shell === "popout") {
      url.searchParams.set("popout", "1");
    }
    return url.toString();
  }

  function ensureOverlayFrame() {
    attachOverlayMessageListener();
    let wrapper = document.getElementById(OVERLAY_WRAPPER_ID);
    let iframe = document.getElementById(OVERLAY_FRAME_ID);
    if (!(wrapper instanceof HTMLElement)) {
      wrapper = document.createElement("div");
      wrapper.id = OVERLAY_WRAPPER_ID;
      Object.assign(wrapper.style, {
        position: "fixed",
        inset: "0",
        zIndex: "2147483647",
        display: "none",
        background: "rgba(0, 0, 0, 0.86)",
        backdropFilter: "blur(2px)",
      });
      wrapper.addEventListener("click", (event) => {
        if (event.target === wrapper) {
          setOverlayOpen(false);
        }
      });
      document.documentElement.appendChild(wrapper);
    }
    if (!(iframe instanceof HTMLIFrameElement)) {
      iframe = document.createElement("iframe");
      iframe.id = OVERLAY_FRAME_ID;
      iframe.allow = "clipboard-read; clipboard-write";
      Object.assign(iframe.style, {
        position: "absolute",
        inset: "16px",
        width: "calc(100vw - 32px)",
        height: "calc(100vh - 32px)",
        border: "0",
        borderRadius: "18px",
        background: CREATE_OVERLAY_BACKGROUND,
        boxShadow: "0 28px 80px rgba(0, 0, 0, 0.52)",
      });
      wrapper.appendChild(iframe);
    }
    return { wrapper, iframe };
  }

  function setOverlayOpen(isOpen) {
    const wrapper = document.getElementById(OVERLAY_WRAPPER_ID);
    if (!(wrapper instanceof HTMLElement)) return;
    const displayMode = wrapper.dataset.overlayDisplay || "block";
    wrapper.style.display = isOpen ? displayMode : "none";
  }

  function openOverlay({ mode = "create", contractAddress = "", instaLaunch = false } = {}) {
    const { wrapper, iframe } = ensureOverlayFrame();
    applyOverlayPresentation(wrapper, iframe, mode);
    const nextUrl = buildShellUrl({
      shell: "overlay",
      mode,
      contractAddress,
      instaLaunch,
    });
    const currentUrl = iframe.getAttribute("data-shell-url") || "";
    if (currentUrl !== nextUrl) {
      iframe.src = nextUrl;
      iframe.setAttribute("data-shell-url", nextUrl);
    }
    setOverlayOpen(true);
    try {
      iframe.contentWindow?.focus();
    } catch (_error) {
      // Ignore focus failures and keep the shell usable.
    }
  }

  function openPopout({ mode = "webapp", contractAddress = "", instaLaunch = false } = {}) {
    const popupSize = typeof Layout.getDefaultPopoutOuterSize === "function"
      ? Layout.getDefaultPopoutOuterSize(mode, window.screen)
      : { width: 552, height: 727 };
    const popupPosition = typeof Layout.computeCenteredPopupPosition === "function"
      ? Layout.computeCenteredPopupPosition(popupSize.width, popupSize.height, window.screen)
      : { left: 0, top: 0 };
    window.open(
      buildShellUrl({
        shell: "popout",
        mode,
        contractAddress,
        instaLaunch,
      }),
      "launchdeck-popout",
      `popup=yes,width=${popupSize.width},height=${popupSize.height},left=${popupPosition.left},top=${popupPosition.top},resizable=yes,scrollbars=yes`,
    );
  }

  return {
    openOverlay,
    openPopout,
    closeOverlay() {
      setOverlayOpen(false);
    },
    destroy() {
      setOverlayOpen(false);
      document.getElementById(OVERLAY_WRAPPER_ID)?.remove();
      detachOverlayMessageListener();
    },
  };
}
