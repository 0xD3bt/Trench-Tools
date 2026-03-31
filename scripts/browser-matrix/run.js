#!/usr/bin/env node

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");
const { chromium } = require("playwright");

const { buildCaseMatrix } = require("./cases");

const PROJECT_ROOT = path.resolve(__dirname, "..", "..");
const DEFAULT_BASE_URL = process.env.LAUNCHDECK_MATRIX_BASE_URL || "http://127.0.0.1:8789";
const DEFAULT_RESULTS_ROOT = path.join(PROJECT_ROOT, ".local", "launchdeck", "browser-matrix");

function parseArgs(argv) {
  const args = {
    baseUrl: DEFAULT_BASE_URL,
    resultsRoot: DEFAULT_RESULTS_ROOT,
    launchpad: "",
    mode: "",
    caseId: "",
    limit: 0,
    headed: false,
    listOnly: false,
    reuseRuntime: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    const next = argv[index + 1];
    if (token === "--base-url" && next) {
      args.baseUrl = next;
      index += 1;
    } else if (token === "--results-root" && next) {
      args.resultsRoot = path.resolve(next);
      index += 1;
    } else if (token === "--launchpad" && next) {
      args.launchpad = next.trim().toLowerCase();
      index += 1;
    } else if (token === "--mode" && next) {
      args.mode = next.trim().toLowerCase();
      index += 1;
    } else if (token === "--case" && next) {
      args.caseId = next.trim().toLowerCase();
      index += 1;
    } else if (token === "--limit" && next) {
      args.limit = Math.max(0, Number.parseInt(next, 10) || 0);
      index += 1;
    } else if (token === "--headed") {
      args.headed = true;
    } else if (token === "--list") {
      args.listOnly = true;
    } else if (token === "--reuse-runtime") {
      args.reuseRuntime = true;
    } else if (token === "--help") {
      printHelpAndExit(0);
    } else {
      console.error(`Unknown argument: ${token}`);
      printHelpAndExit(1);
    }
  }

  return args;
}

function printHelpAndExit(code) {
  console.log(`LaunchDeck browser matrix runner

Usage:
  node scripts/browser-matrix/run.js [options]

Options:
  --base-url <url>        Base LaunchDeck URL. Default: ${DEFAULT_BASE_URL}
  --results-root <path>   Where result batches are written.
  --launchpad <id>        Filter by launchpad.
  --mode <id>             Filter by mode.
  --case <id>             Run a single case id.
  --limit <n>             Stop after n filtered cases.
  --headed                Show the browser instead of running headless.
  --reuse-runtime         Do not auto-start the runtime if the health check fails.
  --list                  Print filtered case ids and exit.
  --help                  Show this message.
`);
  process.exit(code);
}

function timestampSlug() {
  return new Date().toISOString().replace(/[:.]/g, "-");
}

function ensureDir(dirPath) {
  fs.mkdirSync(dirPath, { recursive: true });
}

async function fetchJson(url, options = {}) {
  const response = await fetch(url, options);
  const payload = await response.json().catch(() => null);
  return { response, payload };
}

async function isHealthy(baseUrl) {
  try {
    const { response, payload } = await fetchJson(`${baseUrl}/health`);
    return Boolean(response.ok && payload && payload.ok);
  } catch (_error) {
    return false;
  }
}

async function ensureRuntime(baseUrl, reuseRuntime) {
  if (await isHealthy(baseUrl)) return { startedRuntime: false };
  if (reuseRuntime) {
    throw new Error(`LaunchDeck runtime is not healthy at ${baseUrl} and --reuse-runtime was passed.`);
  }

  const result = spawnSync("npm", ["start"], {
    cwd: PROJECT_ROOT,
    stdio: "inherit",
    env: process.env,
  });
  if (result.status !== 0) {
    throw new Error("Failed to start LaunchDeck runtime with `npm start`.");
  }

  const startedAt = Date.now();
  while (Date.now() - startedAt < 90_000) {
    if (await isHealthy(baseUrl)) return { startedRuntime: true };
    await new Promise((resolve) => setTimeout(resolve, 1_000));
  }
  throw new Error(`LaunchDeck runtime did not become healthy at ${baseUrl} within 90s.`);
}

function filterCases(cases, args) {
  let filtered = cases.slice();
  if (args.launchpad) filtered = filtered.filter((entry) => entry.launchpad === args.launchpad);
  if (args.mode) filtered = filtered.filter((entry) => entry.mode === args.mode);
  if (args.caseId) filtered = filtered.filter((entry) => entry.id === args.caseId);
  if (args.limit > 0) filtered = filtered.slice(0, args.limit);
  return filtered;
}

function routeFeeValues(provider, profile, kind) {
  const tipEnabled = provider === "helius-sender" || provider === "jito-bundle";
  if (profile === "auto") {
    return {
      autoEnabled: true,
      priority: kind === "creation" ? "0.0045" : kind === "buy" ? "0.0075" : "0.0065",
      tip: tipEnabled ? (kind === "creation" ? "0.00025" : "0.0003") : "",
    };
  }
  return {
    autoEnabled: false,
    priority: kind === "creation" ? "0.0031" : kind === "buy" ? "0.0062" : "0.0058",
    tip: tipEnabled ? (kind === "creation" ? "0.00025" : "0.00022") : "",
  };
}

function buildCaseDirectories(batchDir, testCase) {
  const safeId = testCase.id.replace(/[^a-z0-9._-]+/gi, "-");
  const caseDir = path.join(batchDir, safeId);
  ensureDir(caseDir);
  return {
    caseDir,
    jsonPath: path.join(caseDir, "result.json"),
    screenshotPath: path.join(caseDir, "failure.png"),
  };
}

function extractReportId(sendLogPath) {
  if (!sendLogPath) return "";
  return path.basename(String(sendLogPath));
}

function extractCandidateLabels(items) {
  if (!Array.isArray(items)) return [];
  return items
    .map((entry) => (entry && typeof entry.label === "string" ? entry.label.trim() : ""))
    .filter(Boolean);
}

function unique(values) {
  return [...new Set(values.filter(Boolean))];
}

function displayLabel(label) {
  const normalized = String(label || "").trim().toLowerCase();
  if (normalized === "follow-up") return "fee-sharing setup";
  if (normalized === "agent-setup") return "agent fee setup";
  return String(label || "").trim();
}

function extractPlannedLabels(report) {
  if (!report || typeof report !== "object") return [];
  const direct = extractCandidateLabels(report.transactions);
  const executionTransactions =
    report.execution && typeof report.execution === "object"
      ? extractCandidateLabels(report.execution.transactions)
      : [];
  const benchmarkSent =
    report.execution && typeof report.execution === "object"
      ? extractCandidateLabels(report.execution.benchmarkSent)
      : [];
  const sent =
    report.execution && typeof report.execution === "object"
      ? extractCandidateLabels(report.execution.sent)
      : [];
  return unique([...direct, ...executionTransactions, ...benchmarkSent, ...sent].map(displayLabel));
}

function normalizeResponseError(payload) {
  if (!payload) return "Request failed without a JSON payload.";
  return String(payload.error || payload.message || payload.text || "Request failed.").trim();
}

function toCompactJson(value) {
  return JSON.stringify(value, null, 2);
}

function categoryForFailure(stage, failure) {
  if (!failure) return "unknown";
  const message = String(failure.message || "").toLowerCase();
  if (stage === "build" && failure.kind === "assertion" && message.includes("normalized")) {
    return "frontend serialization mismatch";
  }
  if (stage === "report" || message.includes("reports terminal") || message.includes("report labels")) {
    return "report/UI rendering mismatch";
  }
  if (stage === "simulate" || message.includes("simulate")) {
    return "compile/simulation bug";
  }
  return "backend normalization/validation bug";
}

function isRateLimitError(payload) {
  const message = normalizeResponseError(payload).toLowerCase();
  return (
    message.includes("429")
    || message.includes("too many requests")
    || message.includes("rate limited")
  );
}

async function waitForAppReady(page, baseUrl) {
  await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
  await page.waitForFunction(
    () =>
      typeof readForm === "function"
      && typeof setLaunchpad === "function"
      && typeof setMode === "function"
      && typeof selectedWalletKey === "function"
      && typeof refreshReportsTerminal === "function"
      && Boolean(selectedWalletKey()),
    null,
    { timeout: 90_000 },
  );
}

async function configureCase(page, testCase) {
  await page.evaluate(async (entry) => {
    const dispatchChange = (selector) => {
      const node = document.querySelector(selector);
      if (node) node.dispatchEvent(new Event("change", { bubbles: true }));
    };

    const setField = (selector, value) => {
      const node = document.querySelector(selector);
      if (!node) return;
      node.value = value;
      node.dispatchEvent(new Event("input", { bubbles: true }));
      node.dispatchEvent(new Event("change", { bubbles: true }));
    };

    const setChecked = (selector, checked) => {
      const node = document.querySelector(selector);
      if (!node) return;
      node.checked = Boolean(checked);
      node.dispatchEvent(new Event("change", { bubbles: true }));
    };

    if (typeof refreshWalletStatus === "function") {
      await refreshWalletStatus(true, true).catch(() => {});
    }

    setImportedCreatorFeeState(null);
    setBagsIdentityStateInputs({
      mode: entry.fixture.bagsIdentity.mode,
      agentUsername: entry.fixture.bagsIdentity.agentUsername,
      authToken: entry.fixture.bagsIdentity.authToken,
      verifiedWallet: entry.fixture.bagsIdentity.verifiedWallet,
    });
    if (typeof syncBagsIdentityUI === "function") syncBagsIdentityUI();

    applyFeeSplitDraft({ enabled: false, rows: [] });
    applyAgentSplitDraft({ rows: [] });
    if (typeof sniperFeature !== "undefined") {
      sniperFeature.setState({ enabled: false, wallets: {} });
      applySniperStateToForm();
      renderSniperUI();
    }
    applyAutoSellDraft({
      enabled: false,
      percent: 100,
      triggerMode: "block-offset",
      delayMs: 0,
      blockOffset: 0,
    });
    if (typeof setSelectedImage === "function") {
      setSelectedImage(
        entry.launchpad === "bagsapp"
          ? {
              id: "browser-matrix-token",
              fileName: "browser-matrix-token.svg",
              previewUrl: "/uploads/browser-matrix-token.svg",
            }
          : null,
      );
    }
    setNamedValue("devBuyAmount", "");
    setNamedValue("devBuyMode", "sol");
    setNamedValue("sniperEnabled", "false");
    setNamedValue("sniperConfigJson", "[]");
    setNamedValue("postLaunchStrategy", "none");
    setNamedValue("snipeBuyAmountSol", "");

    setLaunchpad(entry.launchpad, { resetMode: true, persistMode: true });
    if (entry.kind === "blocked") {
      const blockedModeInput = document.querySelector(`input[name="mode"][value="${entry.mode}"]`);
      if (blockedModeInput) {
        blockedModeInput.disabled = false;
        blockedModeInput.checked = true;
      }
    } else {
      setMode(entry.mode);
    }
    if (entry.launchpad === "bonk") {
      setNamedValue("quoteAsset", entry.quoteAsset);
      if (typeof syncBonkQuoteAssetUI === "function") syncBonkQuoteAssetUI();
    } else {
      setNamedValue("quoteAsset", "sol");
    }

    setField('input[name="name"]', entry.fixture.token.name);
    setField('input[name="symbol"]', entry.fixture.token.symbol);
    setField('textarea[name="description"]', entry.fixture.token.description);
    setField('input[name="website"]', entry.fixture.token.website);
    setField('input[name="twitter"]', entry.fixture.token.twitter);
    setField('input[name="telegram"]', entry.fixture.token.telegram);
    const metadataNode = document.getElementById("metadata-uri");
    if (metadataNode) metadataNode.value = entry.fixture.token.metadataUri;

    setField('select[name="creationProvider"]', entry.providers.creation);
    setField('select[name="buyProvider"]', entry.providers.buy);
    setField('select[name="sellProvider"]', entry.providers.sell);
    dispatchChange('select[name="creationProvider"]');
    dispatchChange('select[name="buyProvider"]');
    dispatchChange('select[name="sellProvider"]');

    const creationFees = entry.routeFeeProfile === "auto"
      ? { autoEnabled: true }
      : { autoEnabled: false };
    const currentWallet = typeof getDeployerFeeSplitAddress === "function"
      ? (getDeployerFeeSplitAddress() || entry.fixture.recipientWallet)
      : entry.fixture.recipientWallet;

    const creationRoute = {
      ...creationFees,
      ...(
        entry.routeFeeProfile === "auto"
          ? { priority: "0.0045", tip: (entry.providers.creation === "helius-sender" || entry.providers.creation === "jito-bundle") ? "0.00025" : "" }
          : { priority: "0.0031", tip: (entry.providers.creation === "helius-sender" || entry.providers.creation === "jito-bundle") ? "0.00025" : "" }
      ),
    };
    const buyRoute = {
      autoEnabled: entry.routeFeeProfile === "auto",
      priority: entry.routeFeeProfile === "auto" ? "0.0075" : "0.0062",
      tip: (entry.providers.buy === "helius-sender" || entry.providers.buy === "jito-bundle")
        ? (entry.routeFeeProfile === "auto" ? "0.0003" : "0.00022")
        : "",
    };
    const sellRoute = {
      autoEnabled: entry.routeFeeProfile === "auto",
      priority: entry.routeFeeProfile === "auto" ? "0.0065" : "0.0058",
      tip: (entry.providers.sell === "helius-sender" || entry.providers.sell === "jito-bundle")
        ? (entry.routeFeeProfile === "auto" ? "0.0003" : "0.00022")
        : "",
    };

    setChecked('input[name="creationAutoFeeEnabled"]', creationRoute.autoEnabled);
    setChecked('input[name="buyAutoFeeEnabled"]', buyRoute.autoEnabled);
    setChecked('input[name="sellAutoFeeEnabled"]', sellRoute.autoEnabled);
    setField('input[name="creationPriorityFeeSol"]', creationRoute.priority);
    setField('input[name="creationTipSol"]', creationRoute.tip);
    setField('input[name="buyPriorityFeeSol"]', buyRoute.priority);
    setField('input[name="buyTipSol"]', buyRoute.tip);
    setField('input[name="sellPriorityFeeSol"]', sellRoute.priority);
    setField('input[name="sellTipSol"]', sellRoute.tip);
    setField('input[name="buySlippagePercent"]', "90");
    setField('input[name="sellSlippagePercent"]', "90");
    setNamedValue("skipPreflight", entry.providers.creation === "helius-sender" ? "true" : "false");

    setField('input[name="agentAuthority"]', currentWallet);
    setField('input[name="agentUnlockedBuybackPercent"]', entry.mode === "agent-unlocked" ? "15" : "0");

    if (entry.fixture.creatorFeeProfile === "wallet") {
      setImportedCreatorFeeState({
        mode: "wallet",
        address: entry.fixture.recipientWallet,
      });
    } else {
      setImportedCreatorFeeState({ mode: "deployer" });
    }

    if (entry.devBuyProfile === "on") {
      setNamedValue("devBuyMode", "sol");
      setNamedValue("devBuyAmount", entry.fixture.devBuyAmount);
      const devBuySolInput = document.getElementById("dev-buy-sol-input");
      if (devBuySolInput) devBuySolInput.value = entry.fixture.devBuyAmount;
    }

    if (entry.splitProfile === "meaningful-fee-split" || entry.splitProfile === "bags-meaningful-fee-split") {
      applyFeeSplitDraft({
        enabled: entry.mode === "regular",
        suppressDefaultRow: false,
        rows: [
          {
            type: "wallet",
            value: currentWallet,
            sharePercent: "80",
            defaultReceiver: true,
            targetLocked: true,
          },
          {
            type: "wallet",
            value: entry.fixture.recipientWallet,
            sharePercent: "20",
            targetLocked: true,
          },
        ],
      });
    }

    if (entry.splitProfile === "agent-custom-no-init") {
      applyAgentSplitDraft({
        rows: [
          {
            locked: true,
            sharePercent: "0",
          },
          {
            type: "wallet",
            value: currentWallet,
            sharePercent: "100",
            targetLocked: true,
          },
        ],
      });
    }

    if (entry.splitProfile === "agent-custom-meaningful") {
      applyAgentSplitDraft({
        rows: [
          {
            locked: true,
            sharePercent: "60",
          },
          {
            type: "wallet",
            value: entry.fixture.recipientWallet,
            sharePercent: "40",
            targetLocked: true,
          },
        ],
      });
    }

    if (entry.followProfile === "minimal") {
      const walletKey = selectedWalletKey();
      if (typeof sniperFeature !== "undefined") {
        sniperFeature.setState({
          enabled: true,
          wallets: {
            [walletKey]: {
              selected: true,
              amountSol: entry.fixture.follow.snipeAmountSol,
              triggerMode: entry.fixture.follow.snipeTriggerMode,
              submitDelayMs: entry.fixture.follow.snipeSubmitDelayMs,
              targetBlockOffset: 0,
              retryOnce: false,
            },
          },
        });
        applySniperStateToForm();
        renderSniperUI();
      }
      if (entry.fixture.follow.includeDevSell) {
        applyAutoSellDraft({
          enabled: true,
          percent: entry.fixture.follow.devSellPercent,
          triggerMode: entry.fixture.follow.devSellTriggerMode,
          delayMs: 0,
          blockOffset: entry.fixture.follow.devSellBlockOffset,
        });
      }
    }

    if (entry.kind !== "blocked" && typeof updateModeVisibility === "function") {
      updateModeVisibility();
    }
  }, testCase);
}

async function runUiAction(page, action) {
  return page.evaluate(async (selectedAction) => {
    const originalFetch = window.fetch.bind(window);
    let captured = null;

    window.fetch = async (...args) => {
      const response = await originalFetch(...args);
      if (String(args[0] || "").includes("/api/run")) {
        let payload = null;
        try {
          payload = await response.clone().json();
        } catch (_error) {
          payload = null;
        }
        captured = {
          ok: Boolean(response.ok && payload && payload.ok),
          status: response.status,
          payload,
        };
      }
      return response;
    };

    try {
      if (typeof run !== "function") {
        throw new Error("UI run(action) function is unavailable.");
      }
      await run(selectedAction);
      const button = document.querySelector(`[data-action="${selectedAction}"]`);
      if (button && button.disabled) {
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      }
      return captured || {
        ok: false,
        status: 0,
        payload: {
          ok: false,
          error: document.getElementById("output")?.textContent || "UI action did not trigger /api/run.",
        },
      };
    } finally {
      window.fetch = originalFetch;
    }
  }, action);
}

async function runUiActionWithRetry(page, action, options = {}) {
  const retries = Number.isInteger(options.retries) ? options.retries : 3;
  const baseDelayMs = Number.isInteger(options.baseDelayMs) ? options.baseDelayMs : 1500;
  let lastResult = null;
  for (let attempt = 0; attempt <= retries; attempt += 1) {
    lastResult = await runUiAction(page, action);
    if (lastResult.ok || !isRateLimitError(lastResult.payload) || attempt === retries) {
      return lastResult;
    }
    await page.waitForTimeout(baseDelayMs * (attempt + 1));
  }
  return lastResult;
}

async function fetchPersistedReport(baseUrl, reportId) {
  if (!reportId) return null;
  const { response, payload } = await fetchJson(`${baseUrl}/api/reports/view?id=${encodeURIComponent(reportId)}`);
  if (!response.ok || !payload || !payload.ok) {
    throw new Error(`Failed to fetch persisted report ${reportId}: ${normalizeResponseError(payload)}`);
  }
  return payload;
}

function assertSupportedCase(testCase, buildResult, simulateResult, persistedReport, dashboardRender) {
  const normalizedConfig = buildResult.payload && buildResult.payload.normalizedConfig;
  if (!normalizedConfig) {
    throw new Error("Normalized config missing from build response.");
  }
  const normalizedProviders = normalizedConfig.execution || {};
  if (normalizedProviders.provider !== testCase.expected.normalizedProviders.creation) {
    throw new Error(`Normalized creation provider mismatch. Expected ${testCase.expected.normalizedProviders.creation}, got ${normalizedProviders.provider || "<missing>"}.`);
  }
  if (normalizedProviders.buyProvider !== testCase.expected.normalizedProviders.buy) {
    throw new Error(`Normalized buy provider mismatch. Expected ${testCase.expected.normalizedProviders.buy}, got ${normalizedProviders.buyProvider || "<missing>"}.`);
  }
  if (normalizedProviders.sellProvider !== testCase.expected.normalizedProviders.sell) {
    throw new Error(`Normalized sell provider mismatch. Expected ${testCase.expected.normalizedProviders.sell}, got ${normalizedProviders.sellProvider || "<missing>"}.`);
  }

  const followEnabled = Boolean(
    normalizedConfig.followLaunch
    && normalizedConfig.followLaunch.enabled,
  );
  if (followEnabled !== testCase.expected.followDaemonEnabled) {
    throw new Error(`Normalized follow enablement mismatch. Expected ${testCase.expected.followDaemonEnabled}, got ${followEnabled}.`);
  }

  if (!simulateResult || !simulateResult.ok) {
    throw new Error(`Simulate failed unexpectedly: ${normalizeResponseError(simulateResult && simulateResult.payload)}`);
  }

  const persistedReportPayload =
    persistedReport && persistedReport.payload && persistedReport.payload.report
      ? persistedReport.payload.report
      : null;
  const plannedLabels = extractPlannedLabels(
    persistedReportPayload || (simulateResult.payload && simulateResult.payload.report) || {},
  );
  if (testCase.launchpad === "bagsapp") {
    const hasBagsSetupLabels = plannedLabels.some(
      (label) => label.startsWith("bags-config-direct-") || label.startsWith("bags-config-bundle-"),
    );
    if (!hasBagsSetupLabels) {
      throw new Error(`Missing Bags setup labels in report. Saw: ${plannedLabels.join(", ") || "<none>"}.`);
    }
  } else {
    for (const label of testCase.expected.plannedActionLabels) {
      if (!plannedLabels.includes(label)) {
        throw new Error(`Missing planned action label "${label}" in report. Saw: ${plannedLabels.join(", ") || "<none>"}.`);
      }
    }
  }
  if (plannedLabels.length < testCase.expected.plannedActionLabels.length) {
    throw new Error(`Planned action count too small. Expected at least ${testCase.expected.plannedActionLabels.length}, saw ${plannedLabels.length}.`);
  }

  const reportPayload = persistedReportPayload || {};
  const savedFollowEnabled = Boolean(reportPayload.savedFollowLaunch && reportPayload.savedFollowLaunch.enabled);
  const expectsSavedFollow = testCase.expected.followMetadata === "present-only-in-config";
  if (savedFollowEnabled !== expectsSavedFollow) {
    throw new Error(`Persisted report savedFollowLaunch mismatch. Expected ${expectsSavedFollow}, got ${savedFollowEnabled}.`);
  }

  const dashboardOverviewText = String((dashboardRender && dashboardRender.overviewText) || "");
  if (!dashboardOverviewText.toLowerCase().includes(testCase.providers.creation.toLowerCase())) {
    throw new Error(`Reports terminal missing creation provider ${testCase.providers.creation}.`);
  }
}

async function renderReportInUi(page, reportId) {
  await page.click("#toggle-reports-button");
  await page.waitForSelector("#reports-terminal-section");
  return page.evaluate(async (id) => {
    await refreshReportsTerminal({ preserveSelection: false, preferId: id });
    const node = document.getElementById("reports-terminal-output");
    const readNode = () => ({
      text: node ? node.innerText : "",
      html: node ? node.innerHTML : "",
    });
    const overview = readNode();
    const actionsTab = document.querySelector('[data-report-tab="actions"]');
    if (actionsTab) actionsTab.click();
    const actions = readNode();
    const rawTab = document.querySelector('[data-report-tab="raw"]');
    if (rawTab) rawTab.click();
    const raw = readNode();
    return {
      overviewText: overview.text,
      overviewHtml: overview.html,
      actionsText: actions.text,
      actionsHtml: actions.html,
      rawText: raw.text,
      rawHtml: raw.html,
    };
  }, reportId);
}

async function executeCase(browser, baseUrl, batchDir, testCase) {
  const directories = buildCaseDirectories(batchDir, testCase);
  const context = await browser.newContext({ viewport: { width: 1540, height: 1100 } });
  const page = await context.newPage();
  const startedAt = new Date().toISOString();
  const result = {
    id: testCase.id,
    kind: testCase.kind,
    launchpad: testCase.launchpad,
    mode: testCase.mode,
    quoteAsset: testCase.quoteAsset,
    providers: testCase.providers,
    routeFeeProfile: testCase.routeFeeProfile,
    devBuyProfile: testCase.devBuyProfile,
    followProfile: testCase.followProfile,
    splitProfile: testCase.splitProfile,
    expected: testCase.expected,
    startedAt,
  };

  try {
    await waitForAppReady(page, baseUrl);
    await configureCase(page, testCase);
    result.preSubmitForm = await page.evaluate(() => readForm());

    const buildResult = await runUiActionWithRetry(page, "build", {
      retries: 2,
      baseDelayMs: 1000,
    });
    result.build = buildResult;

    if (testCase.kind === "blocked") {
      if (buildResult.ok) {
        throw new Error(`Build unexpectedly succeeded. Expected rejection containing: ${testCase.expected.errorIncludes}`);
      }
      const buildError = normalizeResponseError(buildResult.payload);
      if (!buildError.includes(testCase.expected.errorIncludes)) {
        throw new Error(`Build rejected with unexpected error. Expected to include "${testCase.expected.errorIncludes}", got "${buildError}".`);
      }
      result.outcome = "expected-rejection";
      result.completedAt = new Date().toISOString();
      fs.writeFileSync(directories.jsonPath, toCompactJson(result));
      await context.close();
      return result;
    }

    if (!buildResult.ok) {
      throw new Error(`Build failed unexpectedly: ${normalizeResponseError(buildResult.payload)}`);
    }

    const simulateResult = await runUiActionWithRetry(page, "simulate", {
      retries: 4,
      baseDelayMs: 2000,
    });
    result.simulate = simulateResult;

    const reportId = extractReportId(
      (simulateResult.payload && simulateResult.payload.sendLogPath)
      || (buildResult.payload && buildResult.payload.sendLogPath),
    );
    result.reportId = reportId;
    result.persistedReport = await fetchPersistedReport(baseUrl, reportId);
    result.dashboardRender = await renderReportInUi(page, reportId);

    assertSupportedCase(testCase, buildResult, simulateResult, result.persistedReport, result.dashboardRender);

    result.outcome = "passed";
    result.completedAt = new Date().toISOString();
  } catch (error) {
    result.outcome = "failed";
    result.failure = {
      message: String(error && error.message ? error.message : error),
      category: categoryForFailure(
        result.simulate && !result.simulate.ok ? "simulate" : result.build && !result.build.ok ? "build" : "report",
        { message: String(error && error.message ? error.message : error) },
      ),
    };
    try {
      await page.screenshot({ path: directories.screenshotPath, fullPage: true });
      result.failure.screenshot = directories.screenshotPath;
    } catch (_error) {
      // Ignore screenshot failures and keep the original failure.
    }
    result.completedAt = new Date().toISOString();
  } finally {
    fs.writeFileSync(directories.jsonPath, toCompactJson(result));
    await context.close();
  }

  return result;
}

function summarizeResults(results) {
  const summary = {
    total: results.length,
    passed: 0,
    failed: 0,
    expectedRejections: 0,
    byCategory: {},
    partialDryRunPassed: 0,
    sendOnlyGaps: 0,
  };

  for (const entry of results) {
    if (entry.outcome === "passed") summary.passed += 1;
    else if (entry.outcome === "expected-rejection") summary.expectedRejections += 1;
    else summary.failed += 1;

    if (entry.expected && entry.expected.dryRunTier === "partially dry-runnable" && entry.outcome === "passed") {
      summary.partialDryRunPassed += 1;
    }
    if (entry.expected && entry.expected.dryRunTier === "send-only for full confidence") {
      summary.sendOnlyGaps += 1;
    }

    if (entry.failure && entry.failure.category) {
      summary.byCategory[entry.failure.category] = (summary.byCategory[entry.failure.category] || 0) + 1;
    }
  }

  return summary;
}

function renderSummaryMarkdown(results, batchMeta) {
  const summary = summarizeResults(results);
  const passing = results.filter((entry) => entry.outcome === "passed");
  const failing = results.filter((entry) => entry.outcome === "failed");
  const blocked = results.filter((entry) => entry.outcome === "expected-rejection");

  const lines = [
    "# Browser Matrix Summary",
    "",
    `- Batch: \`${batchMeta.batchDir}\``,
    `- Base URL: \`${batchMeta.baseUrl}\``,
    `- Total cases: ${summary.total}`,
    `- Passed: ${summary.passed}`,
    `- Failed: ${summary.failed}`,
    `- Expected rejections: ${summary.expectedRejections}`,
    `- Partial dry-run passes: ${summary.partialDryRunPassed}`,
    "",
    "## Passing matrix entries",
  ];

  if (passing.length === 0) {
    lines.push("- None");
  } else {
    passing.forEach((entry) => {
      lines.push(`- \`${entry.id}\` (${entry.launchpad}/${entry.mode})`);
    });
  }

  lines.push("", "## Failing matrix entries");
  if (failing.length === 0) {
    lines.push("- None");
  } else {
    failing.forEach((entry) => {
      lines.push(`- \`${entry.id}\` | ${entry.failure.category} | ${entry.failure.message}`);
    });
  }

  lines.push("", "## Correctly blocked entries");
  if (blocked.length === 0) {
    lines.push("- None");
  } else {
    blocked.forEach((entry) => {
      lines.push(`- \`${entry.id}\``);
    });
  }

  lines.push("", "## Failure categories");
  const categories = Object.entries(summary.byCategory);
  if (categories.length === 0) {
    lines.push("- None");
  } else {
    categories
      .sort((left, right) => right[1] - left[1])
      .forEach(([category, count]) => {
        lines.push(`- ${category}: ${count}`);
      });
  }

  lines.push(
    "",
    "## Confidence notes",
    "- Cases marked `partially dry-runnable` validated frontend payload shaping, backend normalization, simulate/build flow, and persisted report rendering, but still leave send-path confidence gaps.",
    "- Live send-only confidence gaps remain for same-time orchestration and full Bags send/setup execution.",
  );

  return `${lines.join("\n")}\n`;
}

function renderResultsDashboard(results, batchMeta) {
  const payload = JSON.stringify({ batchMeta, results });
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>LaunchDeck Browser Matrix</title>
  <style>
    :root {
      --bg: #0b1020;
      --panel: rgba(14, 22, 42, 0.94);
      --panel-2: rgba(17, 28, 52, 0.94);
      --text: #edf3ff;
      --muted: #99a7c2;
      --pass: #2dd4bf;
      --fail: #fb7185;
      --warn: #fbbf24;
      --border: rgba(153, 167, 194, 0.18);
      --shadow: 0 24px 80px rgba(0, 0, 0, 0.45);
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      font-family: "IBM Plex Sans", sans-serif;
      color: var(--text);
      background:
        radial-gradient(circle at top left, rgba(45, 212, 191, 0.16), transparent 36%),
        radial-gradient(circle at top right, rgba(251, 113, 133, 0.16), transparent 30%),
        linear-gradient(180deg, #090d18, #06080f 60%);
    }
    .wrap { padding: 28px; max-width: 1600px; margin: 0 auto; }
    .hero {
      display: grid;
      grid-template-columns: 2fr 1fr;
      gap: 18px;
      margin-bottom: 18px;
    }
    .panel {
      background: var(--panel);
      border: 1px solid var(--border);
      border-radius: 22px;
      box-shadow: var(--shadow);
      padding: 20px;
      backdrop-filter: blur(14px);
    }
    .stats {
      display: grid;
      grid-template-columns: repeat(4, 1fr);
      gap: 12px;
      margin-top: 18px;
    }
    .stat {
      background: var(--panel-2);
      border: 1px solid var(--border);
      border-radius: 16px;
      padding: 14px;
    }
    .label { font-size: 12px; color: var(--muted); text-transform: uppercase; letter-spacing: 0.08em; }
    .value { font-size: 28px; font-weight: 700; margin-top: 6px; }
    .controls {
      display: grid;
      grid-template-columns: repeat(4, minmax(0, 1fr));
      gap: 12px;
      margin: 18px 0;
    }
    select, input {
      width: 100%;
      padding: 11px 12px;
      border-radius: 12px;
      border: 1px solid var(--border);
      background: rgba(255,255,255,0.04);
      color: var(--text);
    }
    table {
      width: 100%;
      border-collapse: collapse;
      font-size: 14px;
    }
    th, td {
      text-align: left;
      padding: 12px;
      border-bottom: 1px solid rgba(153, 167, 194, 0.12);
      vertical-align: top;
    }
    .chip {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 4px 10px;
      border-radius: 999px;
      font-size: 12px;
      border: 1px solid var(--border);
      background: rgba(255,255,255,0.04);
    }
    .chip.pass { color: var(--pass); }
    .chip.fail { color: var(--fail); }
    .chip.warn { color: var(--warn); }
    .muted { color: var(--muted); }
    .mono { font-family: "IBM Plex Mono", monospace; }
    @media (max-width: 1080px) {
      .hero { grid-template-columns: 1fr; }
      .stats, .controls { grid-template-columns: repeat(2, 1fr); }
    }
  </style>
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
  <link href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600&family=IBM+Plex+Sans:wght@400;500;600;700&display=swap" rel="stylesheet">
</head>
<body>
  <div class="wrap">
    <section class="hero">
      <div class="panel">
        <div class="label">Batch</div>
        <h1 style="margin:8px 0 0;font-size:36px;">LaunchDeck Browser Matrix</h1>
        <p class="muted mono" id="batchPath"></p>
        <div class="stats" id="stats"></div>
      </div>
      <div class="panel">
        <div class="label">Filters</div>
        <div class="controls">
          <select id="outcomeFilter"><option value="">All outcomes</option></select>
          <select id="launchpadFilter"><option value="">All launchpads</option></select>
          <select id="modeFilter"><option value="">All modes</option></select>
          <input id="searchFilter" placeholder="Search case id" />
        </div>
      </div>
    </section>
    <section class="panel">
      <table>
        <thead>
          <tr>
            <th>Case</th>
            <th>Outcome</th>
            <th>Combo</th>
            <th>Routes</th>
            <th>Dry run tier</th>
            <th>Failure</th>
          </tr>
        </thead>
        <tbody id="resultsTable"></tbody>
      </table>
    </section>
  </div>
  <script>
    const state = ${payload};
    const statsNode = document.getElementById("stats");
    const tableNode = document.getElementById("resultsTable");
    document.getElementById("batchPath").textContent = state.batchMeta.batchDir;
    const counts = state.results.reduce((acc, entry) => {
      acc.total += 1;
      acc[entry.outcome] = (acc[entry.outcome] || 0) + 1;
      return acc;
    }, { total: 0, passed: 0, failed: 0, "expected-rejection": 0 });
    [
      ["Total", counts.total],
      ["Passed", counts.passed],
      ["Failed", counts.failed],
      ["Blocked", counts["expected-rejection"]],
    ].forEach(([label, value]) => {
      const card = document.createElement("div");
      card.className = "stat";
      card.innerHTML = '<div class="label">' + label + '</div><div class="value">' + value + '</div>';
      statsNode.appendChild(card);
    });

    const filters = {
      outcome: document.getElementById("outcomeFilter"),
      launchpad: document.getElementById("launchpadFilter"),
      mode: document.getElementById("modeFilter"),
      search: document.getElementById("searchFilter"),
    };
    const unique = (values) => [...new Set(values.filter(Boolean))].sort();
    unique(state.results.map((entry) => entry.outcome)).forEach((value) => {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = value;
      filters.outcome.appendChild(option);
    });
    unique(state.results.map((entry) => entry.launchpad)).forEach((value) => {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = value;
      filters.launchpad.appendChild(option);
    });
    unique(state.results.map((entry) => entry.mode)).forEach((value) => {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = value;
      filters.mode.appendChild(option);
    });

    function chipClass(outcome) {
      if (outcome === "passed") return "chip pass";
      if (outcome === "expected-rejection") return "chip warn";
      return "chip fail";
    }

    function render() {
      const outcome = filters.outcome.value;
      const launchpad = filters.launchpad.value;
      const mode = filters.mode.value;
      const search = filters.search.value.trim().toLowerCase();
      const rows = state.results.filter((entry) => {
        if (outcome && entry.outcome !== outcome) return false;
        if (launchpad && entry.launchpad !== launchpad) return false;
        if (mode && entry.mode !== mode) return false;
        if (search && !entry.id.toLowerCase().includes(search)) return false;
        return true;
      });
      tableNode.innerHTML = rows.map((entry) => \`
        <tr>
          <td class="mono">\${entry.id}</td>
          <td><span class="\${chipClass(entry.outcome)}">\${entry.outcome}</span></td>
          <td>\${entry.launchpad} / \${entry.mode} / \${entry.quoteAsset}</td>
          <td class="mono">create=\${entry.providers.creation}<br>buy=\${entry.providers.buy}<br>sell=\${entry.providers.sell}</td>
          <td>\${entry.expected && entry.expected.dryRunTier ? entry.expected.dryRunTier : "-"}</td>
          <td>\${entry.failure ? entry.failure.message : '<span class="muted">-</span>'}</td>
        </tr>
      \`).join("");
    }

    Object.values(filters).forEach((node) => node.addEventListener("input", render));
    render();
  </script>
</body>
</html>`;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const allCases = buildCaseMatrix();
  const selectedCases = filterCases(allCases, args);

  if (args.listOnly) {
    selectedCases.forEach((entry) => console.log(entry.id));
    return;
  }

  if (selectedCases.length === 0) {
    throw new Error("No matrix cases matched the provided filters.");
  }

  await ensureRuntime(args.baseUrl, args.reuseRuntime);

  const batchDir = path.join(args.resultsRoot, timestampSlug());
  ensureDir(batchDir);
  fs.writeFileSync(path.join(batchDir, "matrix.json"), toCompactJson(selectedCases));

  const browser = await chromium.launch({ headless: !args.headed });
  const results = [];
  try {
    for (const [index, testCase] of selectedCases.entries()) {
      process.stdout.write(`[${index + 1}/${selectedCases.length}] ${testCase.id}\n`);
      const result = await executeCase(browser, args.baseUrl, batchDir, testCase);
      results.push(result);
      process.stdout.write(`  -> ${result.outcome}\n`);
    }
  } finally {
    await browser.close();
  }

  const batchMeta = {
    baseUrl: args.baseUrl,
    batchDir,
    createdAt: new Date().toISOString(),
  };

  const summary = summarizeResults(results);
  fs.writeFileSync(path.join(batchDir, "results.json"), toCompactJson({ batchMeta, summary, results }));
  fs.writeFileSync(path.join(batchDir, "summary.md"), renderSummaryMarkdown(results, batchMeta));
  fs.writeFileSync(path.join(batchDir, "dashboard.html"), renderResultsDashboard(results, batchMeta));

  process.stdout.write(`\nSummary written to ${path.join(batchDir, "summary.md")}\n`);
  process.stdout.write(`Dashboard written to ${path.join(batchDir, "dashboard.html")}\n`);

  if (summary.failed > 0) {
    process.exitCode = 1;
  }
}

main().catch((error) => {
  console.error(error && error.stack ? error.stack : error);
  process.exit(1);
});
