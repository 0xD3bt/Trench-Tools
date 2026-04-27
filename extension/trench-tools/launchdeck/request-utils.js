(function bootstrapRequestUtils(global) {
  const nativeFetch = typeof global.fetch === "function" ? global.fetch.bind(global) : null;

  function injectedBearerToken() {
    return typeof global.__ldToken === "string" ? global.__ldToken.trim() : "";
  }

  function shouldAttachInjectedAuth(requestUrl) {
    try {
      const parsed = new URL(requestUrl, global.location.origin);
      return parsed.origin === global.location.origin && parsed.pathname.startsWith("/api/");
    } catch {
      return false;
    }
  }

  if (nativeFetch) {
    global.fetch = function launchdeckTokenFetch(input, init) {
      const request = input instanceof Request ? input : new Request(input, init);
      if (!shouldAttachInjectedAuth(request.url)) {
        return nativeFetch(input, init);
      }
      const token = injectedBearerToken();
      if (!token) {
        return nativeFetch(input, init);
      }
      const headers = new Headers(request.headers);
      if (!headers.has("authorization")) {
        headers.set("authorization", `Bearer ${token}`);
      }
      return nativeFetch(new Request(request, { headers }));
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
