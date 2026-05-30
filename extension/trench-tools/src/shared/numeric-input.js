(function initTrenchNumericInput(root) {
  function stripSpaces(value) {
    return String(value ?? "").trim().replace(/\s+/g, "");
  }

  function normalizeUnsignedDecimalInput(value, options = {}) {
    const raw = stripSpaces(value);
    if (!raw) return "";

    const allowSuffix = options.allowSuffix === true;
    const suffixMatch = allowSuffix ? raw.match(/[kmbtKMBT]$/) : null;
    const suffix = suffixMatch ? suffixMatch[0].toLowerCase() : "";
    const body = suffix ? raw.slice(0, -1) : raw;
    if (!/^[\d.,]+$/.test(body)) return "";

    const dotCount = (body.match(/\./g) || []).length;
    const commaCount = (body.match(/,/g) || []).length;
    if (dotCount && commaCount) return "";
    if (dotCount > 1 || commaCount > 1) return "";
    if (commaCount === 1 && dotCount === 0 && /^\d{1,3},\d{3}$/.test(body) && !body.startsWith("0,")) {
      return "";
    }

    let normalized = body.replace(",", ".");
    if (normalized === ".") return "";
    if (normalized.startsWith(".")) normalized = `0${normalized}`;
    if (!/^\d+(?:\.\d*)?$/.test(normalized)) return "";

    const [whole, fractional] = normalized.split(".");
    const cleanWhole = whole.replace(/^0+(?=\d)/, "") || "0";
    const maxDecimals = Number.isInteger(options.maxDecimals) ? options.maxDecimals : null;
    if (maxDecimals != null && String(fractional || "").length > maxDecimals) return "";
    const cleanFractional = fractional;

    return normalized.includes(".")
      ? `${cleanWhole}.${cleanFractional ?? ""}${suffix}`
      : `${cleanWhole}${suffix}`;
  }

  function parsePositiveDecimalInput(value, options = {}) {
    const normalized = normalizeUnsignedDecimalInput(value, options);
    if (!normalized || normalized.endsWith(".")) return "";
    const numeric = Number(normalized.replace(/[kmbt]$/, ""));
    return Number.isFinite(numeric) && numeric > 0 ? normalized : "";
  }

  function parsePercentInput(value, options = {}) {
    const normalized = normalizeUnsignedDecimalInput(value, {
      maxDecimals: Number.isInteger(options.maxDecimals) ? options.maxDecimals : 2
    });
    if (!normalized || normalized.endsWith(".")) return "";
    const numeric = Number(normalized);
    if (!Number.isFinite(numeric) || numeric <= 0 || numeric > 100) return "";
    if (options.integer === true && !Number.isInteger(numeric)) return "";
    return normalized;
  }

  const api = {
    normalizeUnsignedDecimalInput,
    parsePositiveDecimalInput,
    parsePercentInput
  };

  if (typeof module !== "undefined" && module.exports) {
    module.exports = api;
  }
  root.TrenchNumericInput = api;
})(typeof globalThis !== "undefined" ? globalThis : window);
