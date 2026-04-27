import { SITE_FEATURES_STORAGE_KEY } from "./constants.js";

export const PULSE_VAMP_MODES = Object.freeze(["prefill", "insta"]);

function normalizePulseVampMode(value, fallback = "prefill") {
  const mode = String(value || "").trim().toLowerCase();
  return PULSE_VAMP_MODES.includes(mode) ? mode : fallback;
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
