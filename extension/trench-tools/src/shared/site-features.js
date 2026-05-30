import { SITE_FEATURES_STORAGE_KEY } from "./constants.js";

export const PULSE_VAMP_MODES = Object.freeze(["prefill", "insta"]);
export const VAMP_ICON_MODES = Object.freeze(["both", "pulse", "token", "off"]);
export const DEXSCREENER_ICON_MODES = Object.freeze(["both", "pulse", "token", "off"]);
export const AXIOM_INSTANT_TRADE_BUTTON_MODE_COUNTS = Object.freeze([1, 2, 3]);
export const AXIOM_PULSE_QUICK_BUY_BUTTON_COUNTS = Object.freeze([1, 2]);
export const AXIOM_PULSE_SECOND_BUTTON_TABLES = Object.freeze([
  "new_pairs",
  "final_stretch",
  "migrated"
]);
export const AXIOM_POST_DEPLOY_ACTIONS = Object.freeze([
  "close_modal_toast",
  "toast_only",
  "open_tab_toast",
  "open_window_toast"
]);
export const AXIOM_POST_DEPLOY_DESTINATIONS = Object.freeze(["axiom"]);
export const AXIOM_AFTER_BUY_ACTIONS = Object.freeze(["nothing", "open_tab", "open_window"]);
export const PLATFORM_AFTER_BUY_ACTIONS = Object.freeze(["toast", "open_axiom_tab", "open_axiom_window"]);

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

function normalizeAxiomPulseQuickBuyButtonCount(value, fallback = 1) {
  const count = Number(value);
  return AXIOM_PULSE_QUICK_BUY_BUTTON_COUNTS.includes(count) ? count : fallback;
}

function defaultPulseSecondButtonTables() {
  return AXIOM_PULSE_SECOND_BUTTON_TABLES.reduce((accumulator, tableId) => {
    accumulator[tableId] = true;
    return accumulator;
  }, {});
}

function normalizeAxiomPulseSecondButtonTables(value) {
  const defaults = defaultPulseSecondButtonTables();
  if (!value || typeof value !== "object") {
    return defaults;
  }
  const result = {};
  for (const tableId of AXIOM_PULSE_SECOND_BUTTON_TABLES) {
    result[tableId] = value[tableId] === undefined ? defaults[tableId] : Boolean(value[tableId]);
  }
  return result;
}

function normalizeAxiomPostDeployAction(value, fallback = "close_modal_toast") {
  const action = String(value || "").trim().toLowerCase();
  return AXIOM_POST_DEPLOY_ACTIONS.includes(action) ? action : fallback;
}

function normalizeAxiomPostDeployDestination(value, fallback = "axiom") {
  const destination = String(value || "").trim().toLowerCase();
  return AXIOM_POST_DEPLOY_DESTINATIONS.includes(destination) ? destination : fallback;
}

function normalizeAxiomAfterBuyAction(value, fallback = "nothing") {
  const action = String(value || "").trim().toLowerCase();
  return AXIOM_AFTER_BUY_ACTIONS.includes(action) ? action : fallback;
}

function normalizePlatformAfterBuyAction(value, fallback = "toast") {
  const action = String(value || "").trim().toLowerCase();
  return PLATFORM_AFTER_BUY_ACTIONS.includes(action) ? action : fallback;
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
      pulseQuickBuyButtonCount: 1,
      pulseSecondButtonTables: defaultPulseSecondButtonTables(),
      instantTradeButtonModeCount: 3,
      vampIconMode: "both",
      dexScreenerIconMode: "both",
      postDeployAction: "close_modal_toast",
      postDeployDestination: "axiom",
      afterBuyAction: "nothing",
      afterListBuyAction: "nothing",
      walletTracker: true,
      watchlist: true
    },
    j7: {
      enabled: false,
      contractQuickBuy: true,
      contractQuickPanel: true,
      contractVamp: true,
      contractAxiom: true,
      cardLaunchdeck: true,
      hideNativeCardActions: false,
      postDeployAction: "close_modal_toast",
      postDeployDestination: "axiom",
      afterBuyAction: "toast"
    },
    x: {
      enabled: false,
      addressQuickBuy: true,
      addressQuickPanel: true,
      addressVamp: true,
      addressAxiom: true,
      tweetDeploy: true,
      postDeployAction: "close_modal_toast",
      postDeployDestination: "axiom",
      afterBuyAction: "toast"
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
      pulseQuickBuyButtonCount: normalizeAxiomPulseQuickBuyButtonCount(
        value?.axiom?.pulseQuickBuyButtonCount,
        defaults.axiom.pulseQuickBuyButtonCount
      ),
      pulseSecondButtonTables: normalizeAxiomPulseSecondButtonTables(
        value?.axiom?.pulseSecondButtonTables
      ),
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
      afterBuyAction: normalizeAxiomAfterBuyAction(
        value?.axiom?.afterBuyAction,
        defaults.axiom.afterBuyAction
      ),
      afterListBuyAction: normalizeAxiomAfterBuyAction(
        value?.axiom?.afterListBuyAction,
        defaults.axiom.afterListBuyAction
      ),
      walletTracker: value?.axiom?.walletTracker ?? defaults.axiom.walletTracker,
      watchlist: value?.axiom?.watchlist ?? defaults.axiom.watchlist
    },
    j7: {
      ...defaults.j7,
      ...(value?.j7 || {}),
      enabled: value?.j7?.enabled ?? defaults.j7.enabled,
      contractQuickBuy: value?.j7?.contractQuickBuy ?? defaults.j7.contractQuickBuy,
      contractQuickPanel: value?.j7?.contractQuickPanel ?? defaults.j7.contractQuickPanel,
      contractVamp: value?.j7?.contractVamp ?? defaults.j7.contractVamp,
      contractAxiom: value?.j7?.contractAxiom ?? defaults.j7.contractAxiom,
      cardLaunchdeck: value?.j7?.cardLaunchdeck ?? defaults.j7.cardLaunchdeck,
      hideNativeCardActions: value?.j7?.hideNativeCardActions ?? defaults.j7.hideNativeCardActions,
      postDeployAction: normalizeAxiomPostDeployAction(
        value?.j7?.postDeployAction,
        defaults.j7.postDeployAction
      ),
      postDeployDestination: normalizeAxiomPostDeployDestination(
        value?.j7?.postDeployDestination,
        defaults.j7.postDeployDestination
      ),
      afterBuyAction: normalizePlatformAfterBuyAction(
        value?.j7?.afterBuyAction,
        defaults.j7.afterBuyAction
      )
    },
    x: {
      ...defaults.x,
      ...(value?.x || {}),
      enabled: value?.x?.enabled ?? defaults.x.enabled,
      addressQuickBuy: value?.x?.addressQuickBuy ?? defaults.x.addressQuickBuy,
      addressQuickPanel: value?.x?.addressQuickPanel ?? defaults.x.addressQuickPanel,
      addressVamp: value?.x?.addressVamp ?? defaults.x.addressVamp,
      addressAxiom: value?.x?.addressAxiom ?? defaults.x.addressAxiom,
      tweetDeploy: value?.x?.tweetDeploy ?? defaults.x.tweetDeploy,
      postDeployAction: normalizeAxiomPostDeployAction(
        value?.x?.postDeployAction,
        defaults.x.postDeployAction
      ),
      postDeployDestination: normalizeAxiomPostDeployDestination(
        value?.x?.postDeployDestination,
        defaults.x.postDeployDestination
      ),
      afterBuyAction: normalizePlatformAfterBuyAction(
        value?.x?.afterBuyAction,
        defaults.x.afterBuyAction
      )
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
