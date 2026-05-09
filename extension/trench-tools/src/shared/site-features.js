import { SITE_FEATURES_STORAGE_KEY } from "./constants.js";

export const PULSE_VAMP_MODES = Object.freeze(["prefill", "insta"]);
export const VAMP_ICON_MODES = Object.freeze(["both", "pulse", "token", "off"]);
export const DEXSCREENER_ICON_MODES = Object.freeze(["both", "pulse", "token", "off"]);
export const AXIOM_INSTANT_TRADE_BUTTON_MODE_COUNTS = Object.freeze([1, 2, 3]);
export const AXIOM_POST_DEPLOY_ACTIONS = Object.freeze([
  "close_modal_toast",
  "toast_only",
  "open_tab_toast",
  "open_window_toast"
]);
export const AXIOM_POST_DEPLOY_DESTINATIONS = Object.freeze(["axiom"]);

function normalizePulseVampMode(value, fallback = "prefill") {
  const mode = String(value || "").trim().toLowerCase();
  return PULSE_VAMP_MODES.includes(mode) ? mode : fallback;
}

function normalizeDexScreenerIconMode(value, fallback = "both") {
  const mode = String(value || "").trim().toLowerCase();
  return DEXSCREENER_ICON_MODES.includes(mode) ? mode : fallback;
}

function normalizeVampIconMode(value, fallback = "both") {
  const mode = String(value || "").trim().toLowerCase();
  return VAMP_ICON_MODES.includes(mode) ? mode : fallback;
}

function normalizeAxiomInstantTradeButtonModeCount(value, fallback = 3) {
  const count = Number(value);
  return AXIOM_INSTANT_TRADE_BUTTON_MODE_COUNTS.includes(count) ? count : fallback;
}

function normalizeAxiomPostDeployAction(value, fallback = "close_modal_toast") {
  const action = String(value || "").trim().toLowerCase();
  return AXIOM_POST_DEPLOY_ACTIONS.includes(action) ? action : fallback;
}

function normalizeAxiomPostDeployDestination(value, fallback = "axiom") {
  const destination = String(value || "").trim().toLowerCase();
  return AXIOM_POST_DEPLOY_DESTINATIONS.includes(destination) ? destination : fallback;
}

export function defaultSiteFeatures() {
  return {
    axiom: {
      enabled: true,
      autoOpenPanel: false,
      floatingLauncher: true,
      instantTrade: true,
      launchdeckInjection: true,
      pulseButton: true,
      pulsePanel: true,
      pulseVamp: true,
      pulseVampMode: "prefill",
      instantTradeButtonModeCount: 3,
      vampIconMode: "both",
      dexScreenerIconMode: "both",
      postDeployAction: "close_modal_toast",
      postDeployDestination: "axiom",
      walletTracker: true,
      watchlist: true
    },
    j7: {
      enabled: false
    }
  };
}

export function normalizeSiteFeatures(value) {
  const defaults = defaultSiteFeatures();
  return {
    axiom: {
      ...defaults.axiom,
      ...(value?.axiom || {}),
      enabled: value?.axiom?.enabled ?? defaults.axiom.enabled,
      instantTrade: value?.axiom?.instantTrade ?? value?.axiom?.tokenDetailButton ?? defaults.axiom.instantTrade,
      launchdeckInjection: value?.axiom?.launchdeckInjection ?? value?.axiom?.launchdeck ?? defaults.axiom.launchdeckInjection,
      pulseButton: value?.axiom?.pulseButton ?? defaults.axiom.pulseButton,
      pulsePanel: value?.axiom?.pulsePanel ?? defaults.axiom.pulsePanel,
      pulseVamp: value?.axiom?.pulseVamp ?? defaults.axiom.pulseVamp,
      pulseVampMode: normalizePulseVampMode(value?.axiom?.pulseVampMode, defaults.axiom.pulseVampMode),
      instantTradeButtonModeCount: normalizeAxiomInstantTradeButtonModeCount(
        value?.axiom?.instantTradeButtonModeCount,
        defaults.axiom.instantTradeButtonModeCount
      ),
      vampIconMode: normalizeVampIconMode(
        value?.axiom?.vampIconMode,
        value?.axiom?.pulseVamp === false ? "off" : defaults.axiom.vampIconMode
      ),
      dexScreenerIconMode: normalizeDexScreenerIconMode(
        value?.axiom?.dexScreenerIconMode,
        defaults.axiom.dexScreenerIconMode
      ),
      postDeployAction: normalizeAxiomPostDeployAction(
        value?.axiom?.postDeployAction,
        defaults.axiom.postDeployAction
      ),
      postDeployDestination: normalizeAxiomPostDeployDestination(
        value?.axiom?.postDeployDestination,
        defaults.axiom.postDeployDestination
      ),
      walletTracker: value?.axiom?.walletTracker ?? defaults.axiom.walletTracker,
      watchlist: value?.axiom?.watchlist ?? defaults.axiom.watchlist
    },
    j7: {
      ...defaults.j7,
      ...(value?.j7 || {}),
      enabled: false
    }
  };
}

export async function getSiteFeatures() {
  const stored = await chrome.storage.local.get(SITE_FEATURES_STORAGE_KEY);
  return normalizeSiteFeatures(stored[SITE_FEATURES_STORAGE_KEY]);
}

export async function saveSiteFeatures(siteFeatures) {
  const normalized = normalizeSiteFeatures(siteFeatures);
  await chrome.storage.local.set({ [SITE_FEATURES_STORAGE_KEY]: normalized });
  return normalized;
}
