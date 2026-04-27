(function initLaunchDeckReportsHistory(global) {
  function createReportsHistory(config) {
    const {
      getState,
      reportLimit,
      normalizeSavedFollowLaunchForUi,
    } = config;

    function reportsState() {
      return getState();
    }

    function pnlSnapshotCache() {
      const state = reportsState();
      if (!state.pnlSnapshotsByScope || typeof state.pnlSnapshotsByScope !== "object") {
        state.pnlSnapshotsByScope = {};
      }
      return state.pnlSnapshotsByScope;
    }

    function metadataUriToGatewayUrl(uri) {
      const raw = String(uri || "").trim();
      if (!raw) return "";
      if (/^ipfs:\/\//i.test(raw)) {
        const normalized = raw.replace(/^ipfs:\/\//i, "").replace(/^ipfs\//i, "");
        return `https://ipfs.io/ipfs/${normalized}`;
      }
      return raw;
    }

    function parseDevBuyDescription(value) {
      const raw = String(value || "").trim();
      if (!raw || raw === "none") return { mode: "sol", amount: "" };
      const [kind, amount] = raw.split(":");
      const normalizedKind = String(kind || "").trim().toLowerCase();
      return {
        mode: normalizedKind === "tokens" ? "tokens" : "sol",
        amount: String(amount || "").trim(),
      };
    }

    function launchHistoryTitle(metadata, report) {
      const symbol = String(metadata && metadata.symbol || "").trim();
      const name = String(metadata && metadata.name || "").trim();
      if (name) return name;
      if (symbol) return symbol;
      return String(report && report.mint || "Unknown launch");
    }

    function launchHistorySymbol(metadata, report) {
      const symbol = String(metadata && metadata.symbol || "").trim();
      if (symbol) return symbol;
      const mode = String(report && report.mode || "").trim();
      return mode ? mode.toUpperCase() : "LAUNCH";
    }

    function launchHistoryImageUrl(metadata) {
      const raw = String(metadata && metadata.image || "").trim();
      return metadataUriToGatewayUrl(raw);
    }

    function selectedWalletKeyFromReport(report) {
      const normalizedReport = report && typeof report === "object" ? report : {};
      const followDaemon = normalizedReport.followDaemon && typeof normalizedReport.followDaemon === "object"
        ? normalizedReport.followDaemon
        : {};
      const followJob = followDaemon.job && typeof followDaemon.job === "object"
        ? followDaemon.job
        : {};
      return String(normalizedReport.savedSelectedWalletKey || followJob.selectedWalletKey || "").trim();
    }

    async function fetchLaunchMetadataSummary(metadataUriValue) {
      const state = reportsState();
      const metadataUriValueNormalized = String(metadataUriValue || "").trim();
      if (!metadataUriValueNormalized) return null;
      if (Object.prototype.hasOwnProperty.call(state.launchMetadataByUri, metadataUriValueNormalized)) {
        return state.launchMetadataByUri[metadataUriValueNormalized];
      }
      const url = metadataUriToGatewayUrl(metadataUriValueNormalized);
      if (!url) {
        state.launchMetadataByUri[metadataUriValueNormalized] = null;
        return null;
      }
      try {
        const response = await fetch(url, { cache: "force-cache" });
        if (!response.ok) throw new Error("metadata fetch failed");
        const payload = await response.json();
        const metadata = payload && typeof payload === "object" ? payload : null;
        state.launchMetadataByUri[metadataUriValueNormalized] = metadata;
        return metadata;
      } catch (_error) {
        state.launchMetadataByUri[metadataUriValueNormalized] = null;
        return null;
      }
    }

    async function fetchReportBundleForLaunch(id) {
      const state = reportsState();
      const normalizedId = String(id || "").trim();
      if (!normalizedId) return null;
      if (state.launchBundles[normalizedId]) return state.launchBundles[normalizedId];
      const response = await fetch(`/api/reports/view?id=${encodeURIComponent(normalizedId)}`);
      const payload = await response.json();
      if (!response.ok || !payload.ok) {
        throw new Error(payload.error || "Failed to load report.");
      }
      state.launchBundles[normalizedId] = payload;
      return payload;
    }

    async function fetchPnlSnapshotSummary(walletKey, mint) {
      const normalizedWalletKey = String(walletKey || "").trim();
      const normalizedMint = String(mint || "").trim();
      if (!normalizedWalletKey || !normalizedMint) return null;
      const cacheKey = `${normalizedWalletKey}::${normalizedMint}`;
      const cache = pnlSnapshotCache();
      if (Object.prototype.hasOwnProperty.call(cache, cacheKey)) {
        return cache[cacheKey];
      }
      try {
        const response = await fetch("/api/extension/wallet-status", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            walletKey: normalizedWalletKey,
            mint: normalizedMint,
            readOnly: true,
            skipSolBalance: true,
          }),
        });
        const payload = await response.json();
        if (!response.ok || !payload || payload.ok === false) {
          throw new Error(payload && payload.error || "wallet status failed");
        }
        cache[cacheKey] = payload;
        return payload;
      } catch (_error) {
        cache[cacheKey] = null;
        return null;
      }
    }

    function getLaunchHistoryEntry(id) {
      const state = reportsState();
      const normalizedId = String(id || "").trim();
      if (!normalizedId) return null;
      return state.launches.find((entry) => entry.id === normalizedId) || null;
    }

    function buildLaunchHistoryEntry(entry, bundle, metadata, pnlSnapshot) {
      const payload = bundle && bundle.payload && typeof bundle.payload === "object" ? bundle.payload : {};
      const report = payload.report && typeof payload.report === "object" ? payload.report : {};
      const execution = report.execution && typeof report.execution === "object" ? report.execution : {};
      const followDaemon = report.followDaemon && typeof report.followDaemon === "object" ? report.followDaemon : {};
      const followJob = followDaemon.job && typeof followDaemon.job === "object" ? followDaemon.job : {};
      const savedFollowLaunch = report.savedFollowLaunch && typeof report.savedFollowLaunch === "object"
        ? normalizeSavedFollowLaunchForUi(report.savedFollowLaunch)
        : null;
      const savedBags = report.savedBags && typeof report.savedBags === "object" ? report.savedBags : null;
      const savedFeeSharingRecipients = Array.isArray(report.savedFeeSharingRecipients) ? report.savedFeeSharingRecipients : [];
      const savedAgentFeeRecipients = Array.isArray(report.savedAgentFeeRecipients) ? report.savedAgentFeeRecipients : [];
      const savedCreatorFee = report.savedCreatorFee && typeof report.savedCreatorFee === "object" ? report.savedCreatorFee : null;
      const followLaunch = savedFollowLaunch
        || (followJob.followLaunch && typeof followJob.followLaunch === "object"
          ? normalizeSavedFollowLaunchForUi(followJob.followLaunch)
          : {});
      const devBuy = parseDevBuyDescription(report.devBuyDescription);
      const selectedWalletKey = selectedWalletKeyFromReport(report);
      return {
        id: entry.id,
        traceId: String(entry && entry.traceId || followJob.traceId || "").trim(),
        entry,
        payload,
        report,
        execution,
        followJob,
        followLaunch,
        selectedWalletKey,
        quoteAsset: String(report.savedQuoteAsset || followJob.quoteAsset || "sol").trim(),
        metadata: metadata || null,
        title: launchHistoryTitle(metadata, report),
        symbol: launchHistorySymbol(metadata, report),
        imageUrl: launchHistoryImageUrl(metadata),
        metadataUri: String(report.metadataUri || "").trim(),
        devBuy,
        bags: savedBags,
        feeSharingRecipients: savedFeeSharingRecipients,
        agentFeeRecipients: savedAgentFeeRecipients,
        creatorFee: savedCreatorFee,
        pnlSnapshot: pnlSnapshot || null,
      };
    }

    async function runWithConcurrency(items, limit, task) {
      const results = new Array(items.length);
      let cursor = 0;
      const workerCount = Math.max(1, Math.min(limit, items.length));
      const workers = new Array(workerCount).fill(0).map(async () => {
        while (true) {
          const index = cursor;
          cursor += 1;
          if (index >= items.length) return;
          results[index] = await task(items[index], index);
        }
      });
      await Promise.all(workers);
      return results;
    }

    async function loadReportsTerminalLaunches() {
      const state = reportsState();
      const sourceEntries = state.allEntries
        .filter((entry) => String(entry && entry.action || "").trim().toLowerCase() === "send")
        .slice(0, reportLimit);
      const launches = await runWithConcurrency(sourceEntries, 6, async (entry) => {
        try {
          const bundle = await fetchReportBundleForLaunch(entry.id);
          const payload = bundle && bundle.payload && typeof bundle.payload === "object" ? bundle.payload : {};
          const report = payload.report && typeof payload.report === "object" ? payload.report : {};
          const metadata = await fetchLaunchMetadataSummary(report.metadataUri || "");
          const walletKey = selectedWalletKeyFromReport(report);
          const mint = String(report.mint || "").trim();
          const pnlSnapshot = walletKey && mint
            ? await fetchPnlSnapshotSummary(walletKey, mint)
            : null;
          return buildLaunchHistoryEntry(entry, bundle, metadata, pnlSnapshot);
        } catch (_error) {
          return buildLaunchHistoryEntry(entry, null, null, null);
        }
      });
      state.launches = launches;
      return launches;
    }

    return {
      fetchLaunchMetadataSummary,
      fetchReportBundleForLaunch,
      fetchPnlSnapshotSummary,
      getLaunchHistoryEntry,
      buildLaunchHistoryEntry,
      loadReportsTerminalLaunches,
    };
  }

  global.LaunchDeckReportsHistory = {
    create: createReportsHistory,
  };
})(window);
