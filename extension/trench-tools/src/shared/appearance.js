import { APPEARANCE_STORAGE_KEY } from "./constants.js";

export const SOUND_TEMPLATES = Object.freeze([
  { id: "notification-1", label: "Notification 1", path: "assets/Notification-1.mp3" },
  { id: "notification-2", label: "Notification 2", path: "assets/notification-2.mp3" },
  { id: "notification-3", label: "Notification 3", path: "assets/notification-3.mp3" },
  { id: "notification-4", label: "Notification 4", path: "assets/notification-4.mp3" },
  { id: "notification-5", label: "Notification 5", path: "assets/notification-5.mp3" },
  { id: "notification-6", label: "Notification 6", path: "assets/notification-6.mp3" }
]);

// Back-compat alias: some callers still import BUY_SOUND_TEMPLATES.
export const BUY_SOUND_TEMPLATES = SOUND_TEMPLATES;

export const SOUND_CUSTOM_ID = "custom";
export const BUY_SOUND_CUSTOM_ID = SOUND_CUSTOM_ID;

// Generous safety cap for stored base64 data-urls. Chrome storage.local quota
// is ~10MB total across the extension, so we cap per-file to a comfortable
// slice of that to avoid wedging everything else (wallets, presets, etc).
export const SOUND_CUSTOM_MAX_BYTES = 3 * 1024 * 1024;
export const BUY_SOUND_CUSTOM_MAX_BYTES = SOUND_CUSTOM_MAX_BYTES;

const DEFAULT_VOLUME = 70;
const DEFAULT_BUY_TEMPLATE_ID = SOUND_TEMPLATES[0].id;
const DEFAULT_SELL_TEMPLATE_ID = SOUND_TEMPLATES[1]?.id || SOUND_TEMPLATES[0].id;

export const QUICK_BUY_BUTTON_SLOTS = Object.freeze([1, 2]);
export const QUICK_BUY_BUTTON_DEFAULT_COLOR = "#ffffff";
export const QUICK_BUY_BUTTON_DEFAULT_BACKGROUND = "#000000";
export const QUICK_BUY_BUTTON_DEFAULT_BORDER = "#ffffff80";

function defaultSoundFor(side) {
  return {
    enabled: true,
    templateId: side === "sell" ? DEFAULT_SELL_TEMPLATE_ID : DEFAULT_BUY_TEMPLATE_ID,
    custom: null
  };
}

function defaultQuickBuyButtonDesign() {
  return {
    color: QUICK_BUY_BUTTON_DEFAULT_COLOR,
    backgroundColor: QUICK_BUY_BUTTON_DEFAULT_BACKGROUND,
    borderColor: QUICK_BUY_BUTTON_DEFAULT_BORDER
  };
}

function defaultQuickBuyButtons() {
  return {
    1: defaultQuickBuyButtonDesign(),
    2: defaultQuickBuyButtonDesign()
  };
}

export function defaultAppearance() {
  return {
    volume: DEFAULT_VOLUME,
    buySound: defaultSoundFor("buy"),
    sellSound: defaultSoundFor("sell"),
    quickBuyButtons: defaultQuickBuyButtons()
  };
}

function normalizeVolume(value, fallback = DEFAULT_VOLUME) {
  const num = Number(value);
  if (!Number.isFinite(num)) {
    return fallback;
  }
  return Math.min(100, Math.max(0, Math.round(num)));
}

function normalizeTemplateId(value, fallback) {
  const normalized = String(value || "").trim();
  if (normalized === SOUND_CUSTOM_ID) {
    return SOUND_CUSTOM_ID;
  }
  if (SOUND_TEMPLATES.some((tpl) => tpl.id === normalized)) {
    return normalized;
  }
  return fallback;
}

function normalizeCustom(value) {
  if (!value || typeof value !== "object") {
    return null;
  }
  const dataUrl = typeof value.dataUrl === "string" ? value.dataUrl.trim() : "";
  if (!dataUrl || !dataUrl.startsWith("data:")) {
    return null;
  }
  return {
    name: String(value.name || "Custom sound").slice(0, 128),
    dataUrl
  };
}

function normalizeSound(value, defaults) {
  const source = value || {};
  return {
    enabled: Boolean(source.enabled ?? defaults.enabled),
    templateId: normalizeTemplateId(source.templateId, defaults.templateId),
    custom: normalizeCustom(source.custom)
  };
}

const HEX6_REGEX = /^#[0-9a-f]{6}$/i;
const HEX8_REGEX = /^#[0-9a-f]{8}$/i;

export function normalizeQuickBuyButtonColor(value, fallback = QUICK_BUY_BUTTON_DEFAULT_COLOR) {
  const trimmed = String(value || "").trim().toLowerCase();
  if (HEX6_REGEX.test(trimmed)) {
    return trimmed;
  }
  return String(fallback || QUICK_BUY_BUTTON_DEFAULT_COLOR).toLowerCase();
}

export function normalizeQuickBuyButtonBackgroundColor(
  value,
  fallback = QUICK_BUY_BUTTON_DEFAULT_BACKGROUND
) {
  const trimmed = String(value || "").trim().toLowerCase();
  if (HEX6_REGEX.test(trimmed) || HEX8_REGEX.test(trimmed)) {
    return trimmed;
  }
  return String(fallback || QUICK_BUY_BUTTON_DEFAULT_BACKGROUND).toLowerCase();
}

export function normalizeQuickBuyButtonBorderColor(
  value,
  fallback = QUICK_BUY_BUTTON_DEFAULT_BORDER
) {
  const trimmed = String(value || "").trim().toLowerCase();
  if (HEX6_REGEX.test(trimmed) || HEX8_REGEX.test(trimmed)) {
    return trimmed;
  }
  return String(fallback || QUICK_BUY_BUTTON_DEFAULT_BORDER).toLowerCase();
}

function normalizeQuickBuyButtonDesign(value, defaults = defaultQuickBuyButtonDesign()) {
  const source = value || {};
  return {
    color: normalizeQuickBuyButtonColor(source.color, defaults.color),
    backgroundColor: normalizeQuickBuyButtonBackgroundColor(source.backgroundColor, defaults.backgroundColor),
    borderColor: normalizeQuickBuyButtonBorderColor(source.borderColor, defaults.borderColor)
  };
}

function normalizeQuickBuyButtons(value) {
  const defaults = defaultQuickBuyButtons();
  const source = value && typeof value === "object" ? value : {};
  const buttonOne = normalizeQuickBuyButtonDesign(source[1] ?? source["1"], defaults[1]);
  const rawButtonTwo = source[2] ?? source["2"];
  const buttonTwo = rawButtonTwo
    ? normalizeQuickBuyButtonDesign(rawButtonTwo, defaults[2])
    : { ...defaults[2] };
  return {
    1: buttonOne,
    2: buttonTwo
  };
}

// Pick the shared volume from the new top-level field, falling back to any
// legacy per-side volume so users who set a buy/sell volume in an older build
// don't get reset to 70.
function pickSharedVolume(source, defaultVolume) {
  if (Number.isFinite(Number(source?.volume))) {
    return normalizeVolume(source.volume, defaultVolume);
  }
  if (Number.isFinite(Number(source?.buySound?.volume))) {
    return normalizeVolume(source.buySound.volume, defaultVolume);
  }
  if (Number.isFinite(Number(source?.sellSound?.volume))) {
    return normalizeVolume(source.sellSound.volume, defaultVolume);
  }
  return defaultVolume;
}

export function normalizeAppearance(value) {
  const defaults = defaultAppearance();
  const source = value && typeof value === "object" ? value : {};
  return {
    ...source,
    volume: pickSharedVolume(value, defaults.volume),
    buySound: normalizeSound(value?.buySound, defaults.buySound),
    sellSound: normalizeSound(value?.sellSound, defaults.sellSound),
    quickBuyButtons: normalizeQuickBuyButtons(value?.quickBuyButtons)
  };
}

export function deriveQuickBuyButtonHoverBackground(backgroundColor) {
  const normalized = normalizeQuickBuyButtonBackgroundColor(backgroundColor);
  return `${normalized.slice(0, 7)}cc`;
}

export function resolveQuickBuyButtonDesign(appearance, index = 1) {
  const normalized = normalizeAppearance(appearance);
  const slot = QUICK_BUY_BUTTON_SLOTS.includes(Number(index)) ? Number(index) : 1;
  return normalized.quickBuyButtons[slot] || normalized.quickBuyButtons[1];
}

export async function getAppearance() {
  const stored = await chrome.storage.local.get(APPEARANCE_STORAGE_KEY);
  return normalizeAppearance(stored[APPEARANCE_STORAGE_KEY]);
}

export async function saveAppearance(next) {
  const normalized = normalizeAppearance(next);
  await chrome.storage.local.set({ [APPEARANCE_STORAGE_KEY]: normalized });
  return normalized;
}

export function resolveSoundUrl(soundSettings, runtimeGetUrl) {
  if (!soundSettings?.enabled) {
    return "";
  }
  if (soundSettings.templateId === SOUND_CUSTOM_ID) {
    return soundSettings.custom?.dataUrl || "";
  }
  const template = SOUND_TEMPLATES.find((tpl) => tpl.id === soundSettings.templateId);
  if (!template) {
    return "";
  }
  if (typeof runtimeGetUrl === "function") {
    try {
      return runtimeGetUrl(template.path);
    } catch {
      return template.path;
    }
  }
  return template.path;
}

// Back-compat: kept for callers that specifically resolve the buy sound.
export function resolveBuySoundUrl(appearance, runtimeGetUrl) {
  return resolveSoundUrl(normalizeAppearance(appearance).buySound, runtimeGetUrl);
}
