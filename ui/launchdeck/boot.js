(function initLaunchdeckBootPage() {
  try {
    document.documentElement.classList.add("boot-pending");
    const EARLY_BOOT_STORAGE_KEY = "launchdeck.earlyBootSnapshot.v1";
    const EARLY_BOOT_SESSION_STORAGE_KEY = "launchdeck.earlyBootSnapshot.session.v1";
    const LIVE_SYNC_STORAGE_KEY = "launchdeck.liveSyncEvent.v1";
    const LIVE_SYNC_SESSION_STORAGE_KEY = "launchdeck.liveSyncSnapshot.v1";
    const LIVE_SYNC_MAX_AGE_MS = 5 * 60 * 1000;
    const isFreshLiveSyncPayload = (payload) => {
      if (!payload || typeof payload !== "object") return false;
      const timestampMs = Number(payload.timestampMs);
      return Number.isFinite(timestampMs) && (Date.now() - timestampMs) <= LIVE_SYNC_MAX_AGE_MS;
    };
    const readFreshLiveSyncPayload = () => {
      try {
        const sessionRaw = window.sessionStorage.getItem(EARLY_BOOT_SESSION_STORAGE_KEY);
        if (sessionRaw) {
          const sessionPayload = JSON.parse(sessionRaw);
          if (isFreshLiveSyncPayload(sessionPayload)) return sessionPayload;
        }
      } catch (_error) {
        // Ignore session storage failures and continue with other fallbacks.
      }
      try {
        const localRaw = window.localStorage.getItem(EARLY_BOOT_STORAGE_KEY);
        if (localRaw) {
          const localPayload = JSON.parse(localRaw);
          if (isFreshLiveSyncPayload(localPayload)) return localPayload;
        }
      } catch (_error) {
        // Ignore storage access failures and continue with live sync fallback.
      }
      try {
        const sessionRaw = window.sessionStorage.getItem(LIVE_SYNC_SESSION_STORAGE_KEY);
        if (sessionRaw) {
          const sessionPayload = JSON.parse(sessionRaw);
          if (isFreshLiveSyncPayload(sessionPayload)) return sessionPayload;
        }
      } catch (_error) {
        // Ignore session storage failures and continue with localStorage fallback.
      }
      try {
        const localRaw = window.localStorage.getItem(LIVE_SYNC_STORAGE_KEY);
        if (localRaw) {
          const localPayload = JSON.parse(localRaw);
          if (isFreshLiveSyncPayload(localPayload)) return localPayload;
        }
      } catch (_error) {
        // Ignore storage access failures and keep boot functional.
      }
      return null;
    };
    const earlyLiveSyncSnapshot = readFreshLiveSyncPayload();
    if (earlyLiveSyncSnapshot) {
      window.__launchdeckEarlyLiveSyncSnapshot = earlyLiveSyncSnapshot;
    }
    if (window.localStorage.getItem("launchdeck.themeMode") === "light") {
      document.documentElement.classList.add("theme-light");
    }
    if (window.localStorage.getItem("launchdeck.outputSectionVisible") === "false") {
      document.documentElement.classList.add("output-hidden");
    }
    window.__launchdeckBootFallback = window.setTimeout(() => {
      document.documentElement.classList.add("boot-stalled");
    }, 4000);
  } catch (_error) {
    // Ignore storage access failures before app boot.
  }
})();
