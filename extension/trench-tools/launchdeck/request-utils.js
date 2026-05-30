(function bootstrapRequestUtils(global) {
  const nativeFetch = typeof global.fetch === "function" ? global.fetch.bind(global) : null;
  const STANDALONE_TOKEN_STORAGE_KEY = "launchdeck.hostAuthToken";
  const STANDALONE_TOKEN_SESSION_KEY = "launchdeck.hostAuthToken.session";
  let standaloneTokenPromptPromise = null;

  function storedStandaloneBearerToken() {
    const sources = [
      () => global.sessionStorage.getItem(STANDALONE_TOKEN_SESSION_KEY),
      () => global.localStorage.getItem(STANDALONE_TOKEN_STORAGE_KEY),
    ];
    for (const read of sources) {
      try {
        const token = read();
        if (typeof token === "string" && token.trim()) {
          return token.trim();
        }
      } catch {}
    }
    return "";
  }

  function isStandaloneLaunchDeckPage() {
    return !global.chrome || global.location.protocol !== "chrome-extension:";
  }

  function injectedBearerToken() {
    const injected = typeof global.__ldToken === "string" ? global.__ldToken.trim() : "";
    return injected || (isStandaloneLaunchDeckPage() ? storedStandaloneBearerToken() : "");
  }

  function persistStandaloneBearerToken(token, remember) {
    const trimmed = String(token || "").trim();
    try {
      if (trimmed) {
        global.sessionStorage.setItem(STANDALONE_TOKEN_SESSION_KEY, trimmed);
      } else {
        global.sessionStorage.removeItem(STANDALONE_TOKEN_SESSION_KEY);
      }
    } catch {}
    try {
      if (remember && trimmed) {
        global.localStorage.setItem(STANDALONE_TOKEN_STORAGE_KEY, trimmed);
      } else if (!remember) {
        global.localStorage.removeItem(STANDALONE_TOKEN_STORAGE_KEY);
      }
    } catch {}
  }

  function shouldAttachInjectedAuth(requestUrl) {
    try {
      const parsed = new URL(requestUrl, global.location.origin);
      return parsed.origin === global.location.origin && parsed.pathname.startsWith("/api/");
    } catch {
      return false;
    }
  }

  function removeStandaloneTokenPrompt() {
    const existing = global.document && global.document.getElementById("launchdeck-auth-overlay");
    if (existing) {
      existing.remove();
    }
  }

  function buildStandaloneTokenPrompt() {
    const overlay = global.document.createElement("div");
    overlay.id = "launchdeck-auth-overlay";
    overlay.className = "launchdeck-auth-overlay";
    overlay.innerHTML = `
      <form class="launchdeck-auth-card">
        <img src="/images/trench-tools-boot-logo.png" alt="Trench.Tools" class="launchdeck-auth-logo">
        <h1>Unlock LaunchDeck</h1>
        <p>Enter your local host auth token to connect. The token is not served in this page anymore.</p>
        <label>
          <span>Bearer token</span>
          <input name="token" type="password" autocomplete="off" spellcheck="false" required>
        </label>
        <label class="launchdeck-auth-remember">
          <input name="remember" type="checkbox">
          <span>Remember on this browser</span>
        </label>
        <div class="launchdeck-auth-actions">
          <button type="submit" class="button primary">Connect</button>
        </div>
      </form>
    `;
    return overlay;
  }

  function promptForStandaloneBearerToken() {
    if (!isStandaloneLaunchDeckPage()) {
      return Promise.resolve("");
    }
    if (standaloneTokenPromptPromise) {
      return standaloneTokenPromptPromise;
    }
    standaloneTokenPromptPromise = new Promise((resolve) => {
      const show = () => {
        removeStandaloneTokenPrompt();
        const overlay = buildStandaloneTokenPrompt();
        const form = overlay.querySelector("form");
        const input = overlay.querySelector("input[name='token']");
        const remember = overlay.querySelector("input[name='remember']");
        form.addEventListener("submit", (event) => {
          event.preventDefault();
          const token = input.value.trim();
          if (!token) {
            input.focus();
            return;
          }
          persistStandaloneBearerToken(token, remember.checked);
          removeStandaloneTokenPrompt();
          standaloneTokenPromptPromise = null;
          resolve(token);
        });
        global.document.body.appendChild(overlay);
        input.focus();
      };
      if (global.document.readyState === "loading") {
        global.document.addEventListener("DOMContentLoaded", show, { once: true });
      } else {
        show();
      }
    });
    return standaloneTokenPromptPromise;
  }

  function isAuthFailure(response) {
    return response && (response.status === 401 || response.status === 403);
  }

  if (nativeFetch) {
    global.fetch = async function launchdeckTokenFetch(input, init) {
      const request = input instanceof Request ? input : new Request(input, init);
      if (!shouldAttachInjectedAuth(request.url)) {
        return nativeFetch(input, init);
      }
      let token = injectedBearerToken();
      if (!token) {
        token = await promptForStandaloneBearerToken();
        if (!token) {
          return nativeFetch(input, init);
        }
      }
      const headers = new Headers(request.headers);
      if (!headers.has("authorization")) {
        headers.set("authorization", `Bearer ${token}`);
      }
      const authorizedRequest = new Request(request, { headers });
      const retryTemplate = authorizedRequest.clone();
      const response = await nativeFetch(authorizedRequest);
      if (isAuthFailure(response) && isStandaloneLaunchDeckPage()) {
        persistStandaloneBearerToken("", false);
        const nextToken = await promptForStandaloneBearerToken();
        if (nextToken) {
          const retryHeaders = new Headers(retryTemplate.headers);
          retryHeaders.set("authorization", `Bearer ${nextToken}`);
          return nativeFetch(new Request(retryTemplate, { headers: retryHeaders }));
        }
      }
      return response;
    };
  }

  function nowMs() {
    return typeof performance !== "undefined" && performance.now
      ? performance.now()
      : Date.now();
  }

  function ensurePerfStore() {
    if (!global.__launchdeckPerf) {
      global.__launchdeckPerf = {
        requests: {},
      };
    }
    return global.__launchdeckPerf;
  }

  function recordTiming(name, frontendMs, backendMs) {
    const store = ensurePerfStore();
    store.requests[name] = {
      frontendMs: Number(frontendMs || 0),
      backendMs: backendMs == null ? null : Number(backendMs),
      recordedAt: Date.now(),
    };
  }

  function createLatestRequestState() {
    return {
      serial: 0,
      controller: null,
      debounceTimer: null,
    };
  }

  function clearDebounce(state) {
    if (!state || !state.debounceTimer) return;
    clearTimeout(state.debounceTimer);
    state.debounceTimer = null;
  }

  function scheduleDebounced(state, delayMs, callback) {
    clearDebounce(state);
    state.debounceTimer = setTimeout(() => {
      state.debounceTimer = null;
      callback();
    }, delayMs);
  }

  async function fetchJsonLatest(name, url, options = {}, state) {
    const startedAt = nowMs();
    let serial = 0;
    let controller = null;
    if (state) {
      serial = ++state.serial;
      if (state.controller) {
        state.controller.abort();
      }
      controller = new AbortController();
      state.controller = controller;
    }

    try {
      const response = await fetch(url, {
        ...options,
        signal: controller ? controller.signal : options.signal,
      });
      const payload = await response.json();
      const frontendMs = Math.max(0, nowMs() - startedAt);
      const backendMs = payload && typeof payload === "object" ? payload.timingMs : null;
      if (name) recordTiming(name, frontendMs, backendMs);
      const isLatest = !state || state.serial === serial;
      return {
        response,
        payload,
        frontendMs,
        backendMs,
        isLatest,
        serial,
      };
    } catch (error) {
      if (error && error.name === "AbortError") {
        return {
          aborted: true,
          isLatest: false,
          frontendMs: Math.max(0, nowMs() - startedAt),
          backendMs: null,
          serial,
        };
      }
      throw error;
    } finally {
      if (state && state.controller === controller) {
        state.controller = null;
      }
    }
  }

  global.LaunchDeckRequestUtils = {
    clearDebounce,
    createLatestRequestState,
    fetchJsonLatest,
    recordTiming,
    scheduleDebounced,
  };
})(window);
