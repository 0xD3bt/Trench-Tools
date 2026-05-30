import { normalizeUnsignedDecimalInput } from "./numeric-input-module.js";

const TRADE_PREFERENCES_KEYS = {
  presetId: "presetId",
  selectionSource: "selectionSource",
  activeWalletGroupId: "activeWalletGroupId",
  manualWalletKeys: "manualWalletKeys",
  selectionRevision: "selectionRevision",
  quickBuyAmount: "quickBuyAmount",
  quickBuyAmount2: "quickBuyAmount2"
};

export function defaultTradePreferences() {
  return {
    presetId: "",
    selectionSource: "group",
    activeWalletGroupId: "",
    manualWalletKeys: [],
    selectionRevision: 0,
    selectionTarget: {
      type: "wallet_group",
      walletKey: "",
      walletGroupId: "",
      walletKeys: []
    },
    selectionMode: "wallet_group",
    walletKey: "",
    walletGroupId: "",
    walletKeys: [],
    quickBuyAmount: "",
    quickBuyAmount2: ""
  };
}

export function normalizeQuickBuyAmountInput(value) {
  return normalizeUnsignedDecimalInput(value);
}

export function normalizeSelectionTarget(value) {
  const type = String(value?.type || value?.selectionMode || "wallet_group").trim() || "wallet_group";
  return {
    type: ["wallet_group", "wallet_list", "single_wallet"].includes(type) ? type : "wallet_group",
    walletKey: String(value?.walletKey || value?.selectionTarget?.walletKey || "").trim(),
    walletGroupId: String(value?.walletGroupId || value?.selectionTarget?.walletGroupId || "").trim(),
    walletKeys: Array.isArray(value?.walletKeys)
      ? value.walletKeys.map((entry) => String(entry || "").trim()).filter(Boolean)
      : Array.isArray(value?.selectionTarget?.walletKeys)
        ? value.selectionTarget.walletKeys.map((entry) => String(entry || "").trim()).filter(Boolean)
        : []
  };
}

export function normalizeWalletSelectionPreference(value) {
  const selectionSource = String(value?.selectionSource || "").trim().toLowerCase();
  const activeWalletGroupId = String(
    value?.activeWalletGroupId ||
    value?.selectionTarget?.activeWalletGroupId ||
    value?.selectionTarget?.walletGroupId ||
    value?.walletGroupId ||
    ""
  ).trim();
  const directManualWalletKeys = Array.isArray(value?.manualWalletKeys)
    ? value.manualWalletKeys
    : Array.isArray(value?.selectionTarget?.manualWalletKeys)
      ? value.selectionTarget.manualWalletKeys
      : null;
  const normalizedManualWalletKeys = (directManualWalletKeys || []).map((entry) => String(entry || "").trim()).filter(Boolean);

  if (selectionSource === "group" || selectionSource === "manual") {
    return {
      selectionSource,
      activeWalletGroupId,
      manualWalletKeys: Array.from(new Set(normalizedManualWalletKeys))
    };
  }

  const target = normalizeSelectionTarget(value?.selectionTarget || value);
  if (target.type === "wallet_group") {
    return {
      selectionSource: "group",
      activeWalletGroupId: target.walletGroupId,
      manualWalletKeys: []
    };
  }
  const manualWalletKeys = target.type === "wallet_list"
    ? target.walletKeys
    : target.walletKey
      ? [target.walletKey]
      : [];
  return {
    selectionSource: "manual",
    activeWalletGroupId,
    manualWalletKeys: Array.from(new Set(manualWalletKeys.map((entry) => String(entry || "").trim()).filter(Boolean)))
  };
}

export function selectionTargetFromWalletSelectionPreference(selection) {
  if (selection.selectionSource === "group") {
    return {
      type: "wallet_group",
      walletKey: "",
      walletGroupId: String(selection.activeWalletGroupId || "").trim(),
      walletKeys: []
    };
  }
  const manualWalletKeys = Array.from(new Set((selection.manualWalletKeys || []).map((entry) => String(entry || "").trim()).filter(Boolean)));
  return {
    type: manualWalletKeys.length === 1 ? "single_wallet" : "wallet_list",
    walletKey: manualWalletKeys[0] || "",
    walletGroupId: "",
    walletKeys: manualWalletKeys
  };
}

export function mirrorWalletSelectionPreferenceOntoPreferences(preferences, sourceValue = preferences) {
  const selection = normalizeWalletSelectionPreference(sourceValue);
  preferences.selectionSource = selection.selectionSource;
  preferences.activeWalletGroupId = selection.activeWalletGroupId;
  preferences.manualWalletKeys = [...selection.manualWalletKeys];
  const target = selectionTargetFromWalletSelectionPreference(selection);
  preferences.selectionTarget = target;
  preferences.selectionMode = target.type;
  preferences.walletKey = target.walletKey;
  preferences.walletGroupId = target.walletGroupId;
  preferences.walletKeys = [...target.walletKeys];
}

export function normalizeTradePreferences(value) {
  const normalized = {
    ...defaultTradePreferences(),
    presetId: String(value?.presetId || "").trim(),
    selectionRevision: Math.max(0, Number(value?.selectionRevision || 0) || 0),
    quickBuyAmount: normalizeQuickBuyAmountInput(value?.quickBuyAmount || ""),
    quickBuyAmount2: normalizeQuickBuyAmountInput(value?.quickBuyAmount2 || "")
  };
  mirrorWalletSelectionPreferenceOntoPreferences(normalized, value || {});
  return normalized;
}

export function tradePreferencePatchFromValue(value = {}, fields = Object.keys(TRADE_PREFERENCES_KEYS)) {
  const normalized = normalizeTradePreferences(value);
  const patch = {};
  const requestedFields = new Set(fields);
  const hasSelectionTargetShape =
    requestedFields.has("selectionTarget") ||
    requestedFields.has("selectionMode") ||
    requestedFields.has("walletKey") ||
    requestedFields.has("walletGroupId") ||
    requestedFields.has("walletKeys");
  if (
    hasSelectionTargetShape &&
    !requestedFields.has("selectionSource") &&
    !requestedFields.has("activeWalletGroupId") &&
    !requestedFields.has("manualWalletKeys")
  ) {
    requestedFields.add("selectionSource");
    requestedFields.add("activeWalletGroupId");
    requestedFields.add("manualWalletKeys");
  }
  for (const field of requestedFields) {
    if (!Object.prototype.hasOwnProperty.call(TRADE_PREFERENCES_KEYS, field)) {
      continue;
    }
    if (field === "manualWalletKeys") {
      patch[field] = [...normalized.manualWalletKeys];
    } else {
      patch[field] = normalized[field];
    }
  }
  return patch;
}

export function buildSelectionPatch(value = {}) {
  return tradePreferencePatchFromValue(value, [
    "selectionSource",
    "activeWalletGroupId",
    "manualWalletKeys",
    "selectionRevision"
  ]);
}

export function buildPresetPatch(presetId) {
  return { presetId: String(presetId || "").trim() };
}

export function buildQuickBuyPatch(slot, amount) {
  return Number(slot) === 2
    ? { quickBuyAmount2: normalizeQuickBuyAmountInput(amount) }
    : { quickBuyAmount: normalizeQuickBuyAmountInput(amount) };
}
