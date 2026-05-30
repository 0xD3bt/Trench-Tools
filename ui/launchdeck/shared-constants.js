(function initLaunchdeckSharedConstants(global) {
  const namespace = global.__launchdeckShared || (global.__launchdeckShared = {});
  if (namespace.HOST_OFFLINE_PLAIN_MESSAGE) {
    return;
  }
  namespace.HOST_OFFLINE_PLAIN_MESSAGE =
    "LaunchDeck host offline - start launchdeck-engine to use Launch, Snipe and Reports.";
  namespace.HOST_OFFLINE_BANNER_HTML =
    'LaunchDeck host offline - start <code>launchdeck-engine</code> to use Launch, Snipe and Reports.';
  namespace.REPORTS_HOST_OFFLINE_CALLOUT_HTML =
    '<div class="reports-callout is-bad">Reports are unavailable while <code>launchdeck-engine</code> is offline. They refresh automatically once the host is reachable again.</div>';

  // Supported voluntary fee tiers, in basis points.
  namespace.WRAPPER_FEE_TIERS_BPS = [0, 10, 20];
  namespace.WRAPPER_FEE_MAX_BPS = 20;

  namespace.formatWrapperFeeBps = function formatWrapperFeeBps(bps) {
    const value = Number.isFinite(bps) ? Number(bps) : 0;
    if (value <= 0) return "0%";
    if (value === 10) return "0.1%";
    if (value === 20) return "0.2%";
    return `${(value / 100).toFixed(2)}%`;
  };

  namespace.estimateWrapperFeeLamports = function estimateWrapperFeeLamports(
    grossLamports,
    feeBps
  ) {
    const gross = Number(grossLamports);
    const bps = Number(feeBps);
    if (!Number.isFinite(gross) || gross <= 0) return 0;
    if (!Number.isFinite(bps) || bps <= 0) return 0;
    return Math.floor((gross * bps) / 10_000);
  };

  if (!global.TrenchNumericInput) {
    function normalizeUnsignedDecimalInput(value, options = {}) {
      const raw = String(value ?? "").trim().replace(/\s+/g, "");
      if (!raw) return "";
      const suffixMatch = options.allowSuffix === true ? raw.match(/[kmbtKMBT]$/) : null;
      const suffix = suffixMatch ? suffixMatch[0].toLowerCase() : "";
      const body = suffix ? raw.slice(0, -1) : raw;
      if (!/^[\d.,]+$/.test(body)) return "";
      const dotCount = (body.match(/\./g) || []).length;
      const commaCount = (body.match(/,/g) || []).length;
      if (dotCount && commaCount) return "";
      if (dotCount > 1 || commaCount > 1) return "";
      if (commaCount === 1 && dotCount === 0 && /^\d{1,3},\d{3}$/.test(body) && !body.startsWith("0,")) return "";
      const normalized = body.replace(",", ".");
      if (normalized === ".") return "";
      const prefixed = normalized.startsWith(".") ? `0${normalized}` : normalized;
      if (!/^\d+(?:\.\d*)?$/.test(prefixed)) return "";
      const [whole, fractional] = prefixed.split(".");
      const maxDecimals = Number.isInteger(options.maxDecimals) ? options.maxDecimals : null;
      if (maxDecimals != null && String(fractional || "").length > maxDecimals) return "";
      const cleanWhole = whole.replace(/^0+(?=\d)/, "") || "0";
      return prefixed.includes(".") ? `${cleanWhole}.${fractional ?? ""}${suffix}` : `${cleanWhole}${suffix}`;
    }

    global.TrenchNumericInput = { normalizeUnsignedDecimalInput };
  }
})(typeof window !== "undefined" ? window : globalThis);
