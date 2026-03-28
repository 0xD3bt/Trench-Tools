"use strict";

const { getProviderRegistry } = require("./registry");

function getProviderMeta(provider) {
  const normalized = String(provider || "auto").trim().toLowerCase() || "auto";
  const registry = getProviderRegistry();
  const entry = registry[normalized] || registry.auto;
  return {
    ...entry,
    id: normalized,
    supportState: entry.verified ? "verified" : "unverified",
  };
}

function getResolvedProvider(executionConfig, report) {
  const requested = String(executionConfig.provider || "auto").trim().toLowerCase() || "auto";
  if (requested !== "auto") {
    return requested;
  }

  if (report.transactions.length > 1 && String(executionConfig.policy || "fast").trim().toLowerCase() === "safe") {
    return "jito";
  }

  return "helius";
}

function getExecutionClass(report, executionConfig) {
  const resolvedProvider = getResolvedProvider(executionConfig, report);
  const providerMeta = getProviderMeta(resolvedProvider);
  const policy = String(executionConfig.policy || "fast").trim().toLowerCase() || "fast";

  if (report.transactions.length <= 1) {
    return "single";
  }

  if (policy === "safe" && resolvedProvider === "jito" && providerMeta.supportsBundle) {
    return "bundle";
  }

  return "sequential";
}

async function sendWithProvider({ report, executionConfig, sendTransactions, sendBundleTransactions }) {
  const resolvedProvider = getResolvedProvider(executionConfig, report);
  const providerMeta = getProviderMeta(resolvedProvider);
  const executionClass = getExecutionClass(report, executionConfig);
  const warnings = [];

  if (!providerMeta.verified) {
    warnings.push(`Provider ${resolvedProvider} is currently marked unverified in this environment.`);
  }

  if (executionClass === "bundle") {
    const sent = await sendBundleTransactions(report, executionConfig);
    return {
      resolvedProvider,
      executionClass,
      sent,
      warnings: [...warnings, ...(sent.warnings || [])],
    };
  }

  if (String(executionConfig.policy || "fast").trim().toLowerCase() === "safe" && providerMeta.supportsBundle && resolvedProvider !== "jito") {
    warnings.push(`Provider ${resolvedProvider} safe bundle execution is not wired yet; falling back to sequential execution for now.`);
  }

  const sent = await sendTransactions(report, executionConfig);
  return {
    resolvedProvider,
    executionClass,
    sent,
    warnings: [...warnings, ...(sent.warnings || [])],
  };
}

module.exports = {
  getExecutionClass,
  getProviderMeta,
  getResolvedProvider,
  sendWithProvider,
};
