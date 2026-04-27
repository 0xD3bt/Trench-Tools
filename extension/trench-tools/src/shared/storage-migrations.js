// Shared storage migration logic for Trench Tools.
//
// Consumed by:
//   * src/background/index.js (service worker, loaded as an ES module)
//   * launchdeck/extension-bootstrap.js (popout page, loaded as a classic
//     script via <script src="...">)
//
// The file is written to work in both contexts: at the top level it only does
// `globalThis.__trenchToolsStorageMigrations = {...}`. ES module imports of
// this file evaluate it for side effects and then read from globalThis;
// classic <script> loads do the same.
(function attachTrenchToolsStorageMigrations(global) {
  if (global.__trenchToolsStorageMigrations) {
    return;
  }

  const EXECUTION_HOST_STORAGE_KEY = "trenchTools.hostBaseUrl";
  const LAUNCHDECK_HOST_STORAGE_KEY = "trenchTools.launchdeckHostBaseUrl";
  const HOST_AUTH_TOKEN_STORAGE_KEY = "trenchTools.hostAuthToken";
  const LEGACY_LAUNCHDECK_HOST_AUTH_TOKEN_STORAGE_KEY = "trenchTools.launchdeckHostAuthToken";
  const HOST_PORT_MIGRATION_DONE_KEY = "trenchTools.hostPortMigrationV1Done";
  const HOST_AUTH_TOKEN_MERGE_DONE_KEY = "trenchTools.hostAuthTokenMergeV1Done";
  const HOST_AUTH_TOKEN_MERGE_WARNING_KEY = "trenchTools.hostAuthTokenMergeV1Warning";

  const DEFAULT_EXECUTION_HOST_BASE = "http://127.0.0.1:8788";
  const OLD_LOOPBACK_EXECUTION_HOSTS = new Set([
    "http://127.0.0.1:7788",
    "http://localhost:7788",
  ]);

  function normalizeLegacyExecutionHost(rawValue) {
    if (typeof rawValue !== "string") {
      return "";
    }
    return rawValue.trim().replace(/\/+$/, "").toLowerCase();
  }

  function isLegacyExecutionHost(rawValue) {
    return OLD_LOOPBACK_EXECUTION_HOSTS.has(normalizeLegacyExecutionHost(rawValue));
  }

  async function migrateStoredConnectionSettings(options = {}) {
    const { onMergeWarning, logger = console } = options;
    const storage = global.chrome?.storage?.local;
    if (!storage) {
      return {};
    }
    const stored = await storage.get([
      EXECUTION_HOST_STORAGE_KEY,
      HOST_AUTH_TOKEN_STORAGE_KEY,
      LAUNCHDECK_HOST_STORAGE_KEY,
      LEGACY_LAUNCHDECK_HOST_AUTH_TOKEN_STORAGE_KEY,
      HOST_PORT_MIGRATION_DONE_KEY,
      HOST_AUTH_TOKEN_MERGE_DONE_KEY,
    ]);
    if (stored[HOST_PORT_MIGRATION_DONE_KEY] && stored[HOST_AUTH_TOKEN_MERGE_DONE_KEY]) {
      return stored;
    }
    const updates = {};
    let shouldRemoveLegacyLaunchdeckToken = false;
    if (!stored[HOST_PORT_MIGRATION_DONE_KEY]) {
      if (isLegacyExecutionHost(stored[EXECUTION_HOST_STORAGE_KEY])) {
        updates[EXECUTION_HOST_STORAGE_KEY] = DEFAULT_EXECUTION_HOST_BASE;
      }
      updates[HOST_PORT_MIGRATION_DONE_KEY] = true;
    }
    if (!stored[HOST_AUTH_TOKEN_MERGE_DONE_KEY]) {
      const hostAuthToken = typeof stored[HOST_AUTH_TOKEN_STORAGE_KEY] === "string"
        ? stored[HOST_AUTH_TOKEN_STORAGE_KEY].trim()
        : "";
      const legacyLaunchdeckToken = typeof stored[LEGACY_LAUNCHDECK_HOST_AUTH_TOKEN_STORAGE_KEY] === "string"
        ? stored[LEGACY_LAUNCHDECK_HOST_AUTH_TOKEN_STORAGE_KEY].trim()
        : "";
      if (!hostAuthToken && legacyLaunchdeckToken) {
        updates[HOST_AUTH_TOKEN_STORAGE_KEY] = legacyLaunchdeckToken;
        shouldRemoveLegacyLaunchdeckToken = true;
      } else if (hostAuthToken && legacyLaunchdeckToken && hostAuthToken !== legacyLaunchdeckToken) {
        updates[HOST_AUTH_TOKEN_MERGE_WARNING_KEY] = {
          raisedAtUnixMs: Date.now(),
          reason: "dual-token-pre-split",
        };
        if (typeof onMergeWarning === "function") {
          try {
            onMergeWarning();
          } catch (error) {
            logger?.warn?.("Trench Tools migration onMergeWarning handler failed", error);
          }
        }
      } else if (legacyLaunchdeckToken) {
        shouldRemoveLegacyLaunchdeckToken = true;
      }
      updates[HOST_AUTH_TOKEN_MERGE_DONE_KEY] = true;
    }
    if (Object.keys(updates).length) {
      await storage.set(updates);
    }
    if (shouldRemoveLegacyLaunchdeckToken
        && stored[LEGACY_LAUNCHDECK_HOST_AUTH_TOKEN_STORAGE_KEY] !== undefined) {
      await storage.remove(LEGACY_LAUNCHDECK_HOST_AUTH_TOKEN_STORAGE_KEY);
    }
    return { ...stored, ...updates };
  }

  global.__trenchToolsStorageMigrations = {
    EXECUTION_HOST_STORAGE_KEY,
    LAUNCHDECK_HOST_STORAGE_KEY,
    HOST_AUTH_TOKEN_STORAGE_KEY,
    LEGACY_LAUNCHDECK_HOST_AUTH_TOKEN_STORAGE_KEY,
    HOST_PORT_MIGRATION_DONE_KEY,
    HOST_AUTH_TOKEN_MERGE_DONE_KEY,
    HOST_AUTH_TOKEN_MERGE_WARNING_KEY,
    DEFAULT_EXECUTION_HOST_BASE,
    normalizeLegacyExecutionHost,
    isLegacyExecutionHost,
    migrateStoredConnectionSettings,
  };
})(typeof globalThis !== "undefined" ? globalThis : self);
