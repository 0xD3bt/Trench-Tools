(function initLaunchDeckExtensionBootstrap(global) {
  const isExtensionOrigin = global.location && global.location.protocol === "chrome-extension:";
  if (!isExtensionOrigin || !global.chrome || !global.chrome.storage) {
    return;
  }

  const sharedMigrations = global.__trenchToolsStorageMigrations || {};
  const DEFAULT_EXECUTION_HOST_BASE = sharedMigrations.DEFAULT_EXECUTION_HOST_BASE
    || "http://127.0.0.1:8788";
  const EXECUTION_HOST_STORAGE_KEY = sharedMigrations.EXECUTION_HOST_STORAGE_KEY
    || "trenchTools.hostBaseUrl";
  const DEFAULT_LAUNCHDECK_HOST_BASE = "http://127.0.0.1:8789";
  const LAUNCHDECK_HOST_STORAGE_KEY = sharedMigrations.LAUNCHDECK_HOST_STORAGE_KEY
    || "trenchTools.launchdeckHostBaseUrl";
  const HOST_AUTH_TOKEN_STORAGE_KEY = sharedMigrations.HOST_AUTH_TOKEN_STORAGE_KEY
    || "trenchTools.hostAuthToken";
  const EXECUTION_OWNED_PATH_PREFIXES = ["/api/extension/", "/api/launchdeck/"];
  const LAUNCHDECK_OWNED_PATH_PREFIXES = ["/api/", "/uploads/"];
  const search = new URLSearchParams(global.location.search);
  global.__launchdeckExtensionShell = {
    shell: search.get("shell") || (search.get("popout") === "1" ? "popout" : "overlay"),
    mode: search.get("mode") || "webapp",
    contractAddress: search.get("contractAddress") || "",
    vampImageKey: search.get("vampImageKey") || "",
    j7ContextKey: search.get("j7ContextKey") || "",
    action: search.get("action") || "",
    sourcePlatform: search.get("sourcePlatform") || "",
    parentOrigin: search.get("parentOrigin") || "",
    instaLaunch: search.get("instaLaunch") === "1",
  };

  let backendStatePromise = null;

  function isLoopback(url) {
    try {
      const parsed = new URL(url);
      return ["127.0.0.1", "localhost", "::1"].includes(parsed.hostname);
    } catch {
      return false;
    }
  }

  function normalizeBaseUrl(url, fallback) {
    const normalized = String(url || fallback).trim().replace(/\/+$/, "");
    return normalized || fallback;
  }

  function ensureSecureTransport(baseUrl, label) {
    if (isLoopback(baseUrl)) {
      return;
    }
    let protocol = "";
    try {
      protocol = new URL(baseUrl).protocol;
    } catch {
      throw new Error(`Configured ${label} URL is invalid.`);
    }
    if (protocol !== "https:") {
      throw new Error(`Remote ${label} URLs must use HTTPS. Non-local connections over plain HTTP are blocked.`);
    }
  }

  async function ensureOriginPermission(baseUrl, label) {
    if (isLoopback(baseUrl) || !chrome.permissions?.contains) {
      return;
    }
    const parsed = new URL(baseUrl);
    const originPattern = `${parsed.origin}/*`;
    const granted = await chrome.permissions.contains({ origins: [originPattern] });
    if (!granted) {
      throw new Error(
        `Remote ${label} permission is missing for ${parsed.origin}. Open Connection settings and grant access first.`,
      );
    }
  }

  async function loadBackendState() {
    if (!backendStatePromise) {
      backendStatePromise = (async () => {
        let merged = {};
        if (typeof sharedMigrations.migrateStoredConnectionSettings === "function") {
          try {
            merged = await sharedMigrations.migrateStoredConnectionSettings();
          } catch (error) {
            console.warn("Trench Tools popout storage migration failed", error);
          }
        }
        const stored = Object.keys(merged).length
          ? merged
          : await chrome.storage.local.get([
            EXECUTION_HOST_STORAGE_KEY,
            LAUNCHDECK_HOST_STORAGE_KEY,
            HOST_AUTH_TOKEN_STORAGE_KEY,
          ]);
        const executionBaseUrl = normalizeBaseUrl(
          stored[EXECUTION_HOST_STORAGE_KEY],
          DEFAULT_EXECUTION_HOST_BASE,
        );
        const launchdeckBaseUrl = normalizeBaseUrl(
          stored[LAUNCHDECK_HOST_STORAGE_KEY],
          DEFAULT_LAUNCHDECK_HOST_BASE,
        );
        const authToken = typeof stored[HOST_AUTH_TOKEN_STORAGE_KEY] === "string"
          ? stored[HOST_AUTH_TOKEN_STORAGE_KEY].trim()
          : "";
        return { executionBaseUrl, launchdeckBaseUrl, authToken };
      })();
    }
    return backendStatePromise;
  }

  chrome.storage.onChanged.addListener((changes, areaName) => {
    if (areaName !== "local") {
      return;
    }
    if (
      Object.prototype.hasOwnProperty.call(changes, EXECUTION_HOST_STORAGE_KEY)
      || Object.prototype.hasOwnProperty.call(changes, LAUNCHDECK_HOST_STORAGE_KEY)
      || Object.prototype.hasOwnProperty.call(changes, HOST_AUTH_TOKEN_STORAGE_KEY)
    ) {
      backendStatePromise = null;
    }
  });

  function absolutizeUploadUrls(value, launchdeckBaseUrl) {
    if (!value) return value;
    if (Array.isArray(value)) {
      return value.map((entry) => absolutizeUploadUrls(entry, launchdeckBaseUrl));
    }
    if (typeof value !== "object") {
      return value;
    }
    const clone = { ...value };
    for (const [key, entry] of Object.entries(clone)) {
      if (key === "previewUrl" && typeof entry === "string" && entry.startsWith("/uploads/")) {
        clone[key] = `${launchdeckBaseUrl}${entry}`;
      } else {
        clone[key] = absolutizeUploadUrls(entry, launchdeckBaseUrl);
      }
    }
    return clone;
  }

  function routeBackendByFeature(pathname, backendState) {
    if (EXECUTION_OWNED_PATH_PREFIXES.some((prefix) => pathname.startsWith(prefix))) {
      return {
        baseUrl: backendState.executionBaseUrl,
        label: "execution host",
      };
    }
    if (LAUNCHDECK_OWNED_PATH_PREFIXES.some((prefix) => pathname.startsWith(prefix))) {
      return {
        baseUrl: backendState.launchdeckBaseUrl,
        label: "LaunchDeck host",
      };
    }
    return null;
  }

  const nativeFetch = global.fetch.bind(global);
  global.fetch = async function launchDeckExtensionFetch(input, init) {
    const request = input instanceof Request ? input : new Request(input, init);
    const requestUrl = new URL(request.url, global.location.origin);
    const isBackendPath = requestUrl.origin === global.location.origin
      && (requestUrl.pathname.startsWith("/api/") || requestUrl.pathname.startsWith("/uploads/"));
    if (!isBackendPath) {
      return nativeFetch(input, init);
    }

    const backendState = await loadBackendState();
    const targetConfig = routeBackendByFeature(requestUrl.pathname, backendState);
    if (!targetConfig) {
      return nativeFetch(input, init);
    }
    ensureSecureTransport(targetConfig.baseUrl, targetConfig.label);
    await ensureOriginPermission(targetConfig.baseUrl, targetConfig.label);
    const targetUrl = new URL(`${requestUrl.pathname}${requestUrl.search}`, targetConfig.baseUrl);
    const headers = new Headers(request.headers);
    if (backendState.authToken && !headers.has("authorization")) {
      headers.set("authorization", `Bearer ${backendState.authToken}`);
    }
    const body = request.method === "GET" || request.method === "HEAD"
      ? undefined
      : await request.clone().blob();
    const response = await nativeFetch(targetUrl.toString(), {
      method: request.method,
      headers,
      body,
      cache: request.cache,
      credentials: "omit",
      redirect: request.redirect,
      mode: "cors",
      signal: request.signal,
    });
    const contentType = response.headers.get("content-type") || "";
    if (!contentType.includes("application/json") && requestUrl.pathname.startsWith("/api/")) {
      const errorText = await response.clone().text().catch(() => "");
      return new Response(
        JSON.stringify({
          ok: response.ok,
          error: errorText || response.statusText || "Request failed.",
        }),
        {
          status: response.status,
          statusText: response.statusText,
          headers: {
            "content-type": "application/json",
          },
        },
      );
    }
    if (!contentType.includes("application/json")) {
      return response;
    }
    const payload = await response.clone().json().catch(() => null);
    if (!payload || typeof payload !== "object") {
      return response;
    }
    const normalizedPayload = absolutizeUploadUrls(payload, backendState.launchdeckBaseUrl);
    return new Response(JSON.stringify(normalizedPayload), {
      status: response.status,
      statusText: response.statusText,
      headers: response.headers,
    });
  };

  const displayUrlCache = new Map();
  const displayUrlInFlight = new Map();
  const DISPLAY_URL_CACHE_MAX = 64;

  function rememberDisplayUrl(uploadPath, objectUrl) {
    while (displayUrlCache.size >= DISPLAY_URL_CACHE_MAX) {
      const oldestKey = displayUrlCache.keys().next().value;
      if (oldestKey === undefined) break;
      const oldestUrl = displayUrlCache.get(oldestKey);
      displayUrlCache.delete(oldestKey);
      try { URL.revokeObjectURL(oldestUrl); } catch (_error) {}
    }
    displayUrlCache.set(uploadPath, objectUrl);
  }

  function uploadPathFromDisplayUrl(value) {
    const raw = String(value || "").trim();
    if (!raw) return "";
    if (raw.startsWith("/uploads/")) return raw;
    try {
      const parsed = new URL(raw, global.location.origin);
      if (parsed.pathname.startsWith("/uploads/")) {
        return `${parsed.pathname}${parsed.search}`;
      }
    } catch {
      return "";
    }
    return "";
  }

  global.__launchdeckResolveDisplayUrl = async function launchdeckResolveDisplayUrl(value) {
    const raw = String(value || "").trim();
    const uploadPath = uploadPathFromDisplayUrl(raw);
    if (!uploadPath) return raw;
    if (displayUrlCache.has(uploadPath)) {
      return displayUrlCache.get(uploadPath);
    }
    if (displayUrlInFlight.has(uploadPath)) {
      return displayUrlInFlight.get(uploadPath);
    }
    const request = (async () => {
      const response = await global.fetch(uploadPath);
      if (!response.ok) {
        throw new Error(`Image request failed with status ${response.status}`);
      }
      const blob = await response.blob();
      const objectUrl = URL.createObjectURL(blob);
      rememberDisplayUrl(uploadPath, objectUrl);
      return objectUrl;
    })().finally(() => {
      displayUrlInFlight.delete(uploadPath);
    });
    displayUrlInFlight.set(uploadPath, request);
    return request;
  };

  global.__launchdeckSetDisplayImageSrc = function launchdeckSetDisplayImageSrc(image, value) {
    if (!(image instanceof HTMLImageElement)) return;
    const raw = String(value || "").trim();
    if (!raw) {
      image.removeAttribute("src");
      return;
    }
    const token = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
    image.dataset.launchdeckDisplayUrlToken = token;
    global.__launchdeckResolveDisplayUrl(raw)
      .then((resolved) => {
        if (image.dataset.launchdeckDisplayUrlToken !== token) return;
        image.src = resolved;
      })
      .catch(() => {
        if (image.dataset.launchdeckDisplayUrlToken !== token) return;
        if (uploadPathFromDisplayUrl(raw)) {
          image.removeAttribute("src");
          return;
        }
        image.src = raw;
      });
  };
})(window);
