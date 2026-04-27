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
})(typeof window !== "undefined" ? window : globalThis);
