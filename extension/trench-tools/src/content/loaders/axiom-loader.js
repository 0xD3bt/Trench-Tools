(async function trenchToolsAxiomLoader() {
  const injectTime = performance.now();
  const loadSession = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
  const RECONNECT_FALLBACK_DELAY_MS = 2500;
  const RECONNECT_STATE_KEY = "__trenchToolsLoaderReconnectState";
  const STATUS_TOAST_ID = "trench-tools-loader-status-toast";
  const RECONNECT_TITLE = "Connection lost";
  const RECONNECT_DETAIL = "Refresh to reconnect.";
  const LOAD_FAILURE_TITLE = "Extension failed to load";
  const LOAD_FAILURE_DETAIL = "Reload the extension.";

  function buildModuleUrl(path) {
    const url = new URL(chrome.runtime.getURL(path));
    url.searchParams.set("session", loadSession);
    return url.toString();
  }

  async function loadModule(path) {
    const url = buildModuleUrl(path);
    try {
      await import(url);
    } catch (error) {
      console.error(`Failed to load Trench Tools module: ${path}`, { url, error });
      throw error;
    }
  }

  async function importModule(path) {
    const url = buildModuleUrl(path);
    try {
      return await import(url);
    } catch (error) {
      console.error(`Failed to load Trench Tools module: ${path}`, { url, error });
      throw error;
    }
  }

  function removeAll(selector) {
    document.querySelectorAll(selector).forEach((element) => element.remove());
  }

  function clearMarker(selector, attributeName) {
    document.querySelectorAll(selector).forEach((element) => {
      if (element instanceof HTMLElement) {
        element.removeAttribute(attributeName);
      }
    });
  }

  function reconnectState() {
    if (!window[RECONNECT_STATE_KEY] || typeof window[RECONNECT_STATE_KEY] !== "object") {
      window[RECONNECT_STATE_KEY] = {
        session: "",
        timerId: 0
      };
    }
    return window[RECONNECT_STATE_KEY];
  }

  function ensureReconnectToastHost() {
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
    return host;
  }

  function dismissStatusToast() {
    document.getElementById(STATUS_TOAST_ID)?.remove();
  }

  function renderStatusToast({ titleText, detailText, actionLabel = "", actionHandler = null } = {}) {
    dismissStatusToast();
    const host = ensureReconnectToastHost();
    const toast = document.createElement("div");
    const header = document.createElement("div");
    const icon = document.createElement("div");
    const copy = document.createElement("div");
    const title = document.createElement("div");
    const detail = document.createElement("div");
    const action = document.createElement("a");
    const actionIcon = document.createElement("img");

    const showAction = typeof actionHandler === "function" && String(actionLabel || "").trim();

    toast.id = STATUS_TOAST_ID;
    Object.assign(toast.style, {
      width: "max-content",
      maxWidth: "min(92vw, 520px)",
      boxSizing: "border-box",
      borderRadius: "6px",
      padding: "12px",
      border: "1px solid rgba(255, 255, 255, 0.14)",
      background: "#080808",
      boxShadow: "0 10px 24px rgba(0, 0, 0, 0.28)",
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
    icon.innerHTML = '<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M12 22C6.477 22 2 17.523 2 12S6.477 2 12 2s10 4.477 10 10-4.477 10-10 10zm0-2a8 8 0 1 0 0-16 8 8 0 0 0 0 16zm-1-11h2v2h-2V7zm0 4h2v6h-2v-6z"/></svg>';
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
    title.textContent = RECONNECT_TITLE;
    Object.assign(detail.style, {
      marginTop: "2px",
      color: "#a1a1aa",
      fontSize: "12px",
      lineHeight: "1.35",
      wordBreak: "break-word",
      overflowWrap: "anywhere"
    });
    detail.textContent = RECONNECT_DETAIL;
    Object.assign(action.style, {
      display: "inline-flex",
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
    if (showAction) {
      action.href = "#";
      action.title = actionLabel;
      action.setAttribute("aria-label", actionLabel);
      actionIcon.src = chrome.runtime.getURL("assets/link-icon.png");
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
    } else {
      action.style.display = "none";
    }
    copy.append(title, detail);
    header.append(icon, copy, action);
    toast.appendChild(header);
    host.prepend(toast);
    if (showAction) {
      action.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        actionHandler();
      });
      action.addEventListener("mouseenter", () => {
        action.style.opacity = "1";
        action.style.transform = "translateY(-1px)";
      });
      action.addEventListener("mouseleave", () => {
        action.style.opacity = "0.82";
        action.style.transform = "translateY(0)";
      });
    }
    title.textContent = titleText || "";
    detail.textContent = detailText || "";
    requestAnimationFrame(() => {
      toast.style.opacity = "1";
      toast.style.transform = "translateY(0)";
    });
  }

  function clearReconnectFallback() {
    const state = reconnectState();
    if (state.session !== loadSession) {
      return false;
    }
    if (state.timerId) {
      window.clearTimeout(state.timerId);
      state.timerId = 0;
    }
    return true;
  }

  function scheduleReconnectFallback() {
    const state = reconnectState();
    if (state.timerId) {
      window.clearTimeout(state.timerId);
    }
    dismissStatusToast();
    state.session = loadSession;
    state.timerId = window.setTimeout(() => {
      const currentState = reconnectState();
      currentState.timerId = 0;
      if (currentState.session !== loadSession) {
        return;
      }
      if (window.__trenchToolsHealthyLoadSession === loadSession) {
        currentState.session = "";
        return;
      }
      renderStatusToast({
        titleText: RECONNECT_TITLE,
        detailText: RECONNECT_DETAIL,
        actionLabel: "Refresh",
        actionHandler: () => window.location.reload()
      });
    }, RECONNECT_FALLBACK_DELAY_MS);
  }

  function markLoadHealthy() {
    window.__trenchToolsHealthyLoadSession = loadSession;
    const state = reconnectState();
    if (state.session !== loadSession) {
      return;
    }
    if (state.timerId) {
      window.clearTimeout(state.timerId);
      state.timerId = 0;
    }
    state.session = "";
    dismissStatusToast();
  }

  function renderLoadFailureToast() {
    if (!clearReconnectFallback()) {
      return;
    }
    renderStatusToast({
      titleText: LOAD_FAILURE_TITLE,
      detailText: LOAD_FAILURE_DETAIL
    });
  }

  function cleanupStaleArtifacts() {
    try {
      window.__trenchToolsContentScriptInstance?.teardown?.({ reason: "reload" });
    } catch (error) {
      console.warn("Failed to tear down stale Trench Tools instance", error);
    }

    window.__trenchToolsContentScriptInstance = null;
    window.__trenchToolsContentScriptActive = false;
    window.__trenchToolsContentLoadSession = loadSession;

    [
      "#trench-tools-floating-launcher",
      "#trench-tools-panel-wrapper",
      "#trench-tools-quick-panel-wrapper",
      "#trench-tools-toast-host",
      "#trench-tools-toast-styles",
      "#trench-tools-drag-overlay",
      "#trench-tools-launchdeck-overlay",
      "#trench-tools-vamp-overlay",
      "[data-trench-tools-inline]",
      "[data-trench-tools-token-detail-inline]",
      "[data-trench-tools-pulse-inline]",
      "[data-trench-tools-pulse-panel-inline]",
      "[data-trench-tools-wallet-tracker-inline]",
      "[data-trench-tools-axiom-watchlist-inline]",
      "[data-trench-tools-launchdeck-shell]"
    ].forEach(removeAll);

    document.querySelectorAll(".trench-tools-pulse-panel-owner").forEach((element) => {
      element.classList.remove("trench-tools-pulse-panel-owner");
    });

    clearMarker("[data-trench-tools-mounted]", "data-trench-tools-mounted");
    clearMarker("[data-trench-tools-j7-prewarm-wired]", "data-trench-tools-j7-prewarm-wired");
    clearMarker("[data-trench-tools-pulse-anchor-id]", "data-trench-tools-pulse-anchor-id");
    clearMarker("[data-trench-tools-pulse-card-id]", "data-trench-tools-pulse-card-id");
    clearMarker("[data-trench-tools-watchlist-anchor-id]", "data-trench-tools-watchlist-anchor-id");
    clearMarker("[data-trench-tools-wallet-row-id]", "data-trench-tools-wallet-row-id");
  }

  try {
    cleanupStaleArtifacts();
    scheduleReconnectFallback();
    const backgroundRpc = await importModule("src/shared/background-rpc.js");
    const launchdeckShell = await importModule("src/content/launchdeck-shell.js");
    window.__trenchToolsContentModules = {
      callBackground: backgroundRpc.callBackground,
      createLaunchdeckShellController: launchdeckShell.createLaunchdeckShellController
    };
    await loadModule("src/content/runtime.js");
    await loadModule("src/content/platforms/axiom.js");
    await loadModule("src/content/index.js");
    markLoadHealthy();
    window.__trenchToolsAxiomLoadPerf = {
      injectTime,
      loadTime: performance.now() - injectTime,
      session: loadSession
    };
  } catch (error) {
    console.error("Failed to load Trench Tools Axiom bundle", error);
    renderLoadFailureToast();
  }
})();
