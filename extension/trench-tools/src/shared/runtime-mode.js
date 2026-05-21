export function normalizeTrenchToolsMode(value) {
  const normalized = String(value || "").trim().toLowerCase();
  return normalized === "ee" || normalized === "ld" || normalized === "both"
    ? normalized
    : "both";
}

export function isEeOnlyTrenchToolsMode(value) {
  return normalizeTrenchToolsMode(value) === "ee";
}

export function isLdOnlyTrenchToolsMode(value) {
  return normalizeTrenchToolsMode(value) === "ld";
}

export function shouldProbeLaunchdeckRuntime(status) {
  return !isEeOnlyTrenchToolsMode(status?.trenchToolsMode);
}
