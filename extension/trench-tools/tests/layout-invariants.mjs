import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, "..", "..", "..");
const layoutModulePath = path.join(repoRoot, "ui", "launchdeck", "layout.js");

await import(pathToFileURL(layoutModulePath).href);

const Layout = globalThis.LaunchDeckLayout;
if (!Layout?.TOKENS) {
  throw new Error("LaunchDeck layout module failed to initialize.");
}

function approx(actual, expected, tolerance, label) {
  assert.ok(
    Math.abs(actual - expected) <= tolerance,
    `${label}: expected ${expected} +/- ${tolerance}, got ${actual}`,
  );
}

function getPrimaryContext(browser) {
  const context = browser.contexts()[0];
  if (!context) {
    throw new Error("No Chromium browser context is available in the live Edge session.");
  }
  return context;
}

async function getExtensionId(browser) {
  const worker = browser
    .contexts()
    .flatMap((context) => context.serviceWorkers())
    .find((entry) => /chrome-extension:\/\/[^/]+\/src\/background\/index\.js/i.test(entry.url()));
  if (worker) {
    return new URL(worker.url()).hostname;
  }
  const extensionPage = browser
    .contexts()
    .flatMap((context) => context.pages())
    .find((entry) => entry.url().startsWith("chrome-extension://"));
  if (extensionPage) {
    return new URL(extensionPage.url()).hostname;
  }
  const extensionsPage = browser
    .contexts()
    .flatMap((context) => context.pages())
    .find((entry) => entry.url().startsWith("edge://extensions/"));
  if (extensionsPage) {
    const derivedId = await extensionsPage.evaluate(() => {
      const text = String(document.body?.innerText || "");
      const match = /Trench Tools[\s\S]*?ID([a-z]{32})/i.exec(text);
      return match ? match[1] : "";
    });
    if (derivedId) {
      return derivedId;
    }
  }
  const axiomPage = browser
    .contexts()
    .flatMap((context) => context.pages())
    .find((entry) => /axiom\.trade\/pulse/i.test(entry.url()));
  if (axiomPage) {
    const derivedId = await axiomPage.evaluate(() => {
      const candidates = new Set();
      for (const element of document.querySelectorAll('img[src^="chrome-extension://"], iframe[src^="chrome-extension://"], script[src^="chrome-extension://"]')) {
        const src = element.getAttribute("src") || "";
        if (/\/(launchdeck\/|src\/content\/|assets\/TT-compact\.png)/i.test(src)) {
          candidates.add(src);
        }
      }
      for (const entry of performance.getEntriesByType("resource")) {
        const url = String(entry?.name || "");
        if (/^chrome-extension:\/\/[^/]+\/(launchdeck\/|src\/content\/|assets\/TT-compact\.png)/i.test(url)) {
          candidates.add(url);
        }
      }
      const match = Array.from(candidates)
        .map((url) => /^chrome-extension:\/\/([^/]+)\//i.exec(url))
        .find(Boolean);
      return match ? match[1] : "";
    });
    if (derivedId) {
      return derivedId;
    }
  }
  throw new Error("Could not determine the active Trench Tools extension ID from the live Edge session.");
}

function buildLaunchDeckUrl(extensionId, params) {
  return `chrome-extension://${extensionId}/launchdeck/index.html?${params}`;
}

async function openLaunchDeckPage(context, url) {
  const page = await context.newPage();
  await page.goto(url, { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => !document.documentElement.classList.contains("boot-pending"), { timeout: 15000 });
  await page.waitForTimeout(250);
  return page;
}

async function getPopoutMetrics(page) {
  return page.evaluate(() => ({
    outerWidth: window.outerWidth,
    outerHeight: window.outerHeight,
    innerWidth: window.innerWidth,
    innerHeight: window.innerHeight,
    screenAvailWidth: window.screen.availWidth,
    screenAvailHeight: window.screen.availHeight,
    formWidth: document.getElementById("launch-form")?.getBoundingClientRect().width || 0,
    formHeight: document.getElementById("launch-form")?.getBoundingClientRect().height || 0,
    reportsWidth: document.getElementById("reports-terminal-section")?.getBoundingClientRect().width || 0,
    reportsVisible: (() => {
      const node = document.getElementById("reports-terminal-section");
      if (!(node instanceof HTMLElement) || node.hidden) return false;
      return getComputedStyle(node).display !== "none";
    })(),
    outputVisible: (() => {
      const node = document.getElementById("output-section");
      if (!(node instanceof HTMLElement) || node.hidden) return false;
      return getComputedStyle(node).display !== "none";
    })(),
  }));
}

async function assertDeployLayout(browser) {
  const context = getPrimaryContext(browser);
  const extensionId = await getExtensionId(browser);
  const page = await openLaunchDeckPage(
    context,
    buildLaunchDeckUrl(extensionId, "shell=overlay&mode=create"),
  );
  const stableCreate = Layout.getCreateOverlayStableSize();
  const metrics = await page.evaluate(() => ({
    formWidth: document.getElementById("launch-form")?.getBoundingClientRect().width || 0,
    formHeight: document.getElementById("launch-form")?.getBoundingClientRect().height || 0,
    cardWidth: document.querySelector(".card.launch-surface")?.getBoundingClientRect().width || 0,
    cardHeight: document.querySelector(".card.launch-surface")?.getBoundingClientRect().height || 0,
  }));
  approx(metrics.formWidth, stableCreate.width, 2, "Deploy form width");
  approx(metrics.cardWidth, stableCreate.width, 2, "Deploy card width");
  assert.ok(
    metrics.formHeight >= stableCreate.height,
    `Deploy form height should not collapse below ${stableCreate.height}, got ${metrics.formHeight}`,
  );
  assert.ok(
    metrics.cardHeight >= stableCreate.height,
    `Deploy card height should not collapse below ${stableCreate.height}, got ${metrics.cardHeight}`,
  );
  await page.close();
}

async function assertWebappLayout(browser) {
  const context = getPrimaryContext(browser);
  const extensionId = await getExtensionId(browser);
  const popoutUrl = buildLaunchDeckUrl(extensionId, "shell=popout&mode=webapp&popout=1");
  const defaultPopout = Layout.getDefaultPopoutOuterSize("webapp", {
    availWidth: 1920,
    availHeight: 1080,
  });
  const popout = await openLaunchDeckPage(context, popoutUrl);
  let initialMetrics = await getPopoutMetrics(popout);
  assert.equal(defaultPopout.width, Layout.TOKENS.popout.outerWidth, "Webapp default outer width token mismatch");
  assert.equal(defaultPopout.height, Layout.TOKENS.popout.outerHeight, "Webapp default outer height token mismatch");
  approx(initialMetrics.formWidth, Layout.TOKENS.popout.formWidth, 2, "Webapp form width");
  assert.ok(
    initialMetrics.formHeight >= Layout.TOKENS.popout.stableContentHeight - 4,
    `Webapp form height should respect the stable content floor, got ${initialMetrics.formHeight}`,
  );
  if (initialMetrics.reportsVisible) {
    await popout.locator("#toggle-reports-button").click();
    await popout.waitForTimeout(300);
    initialMetrics = await getPopoutMetrics(popout);
  }

  await popout.locator("#open-vamp-button").click();
  await popout.waitForTimeout(250);
  const vampModalMetrics = await popout.evaluate(() => {
    const overlay = document.getElementById("vamp-modal");
    const modal = document.querySelector(".vamp-modal");
    const overlayStyles = overlay instanceof HTMLElement ? getComputedStyle(overlay) : null;
    const modalRect = modal instanceof HTMLElement ? modal.getBoundingClientRect() : null;
    return {
      alignItems: overlayStyles?.alignItems || "",
      justifyContent: overlayStyles?.justifyContent || "",
      modalWidth: modalRect ? modalRect.width : 0,
      modalHeight: modalRect ? modalRect.height : 0,
      subtitle: document.querySelector(".vamp-modal .settings-modal-subtitle")?.textContent || null,
      hint: document.querySelector(".vamp-modal-hint")?.textContent || null,
    };
  });
  assert.equal(vampModalMetrics.alignItems, "center", "Webapp Vamp modal overlay should center vertically");
  assert.equal(vampModalMetrics.justifyContent, "center", "Webapp Vamp modal overlay should center horizontally");
  assert.equal(vampModalMetrics.subtitle, null, "Webapp Vamp modal should not render extra subtitle copy");
  assert.equal(vampModalMetrics.hint, null, "Webapp Vamp modal should not render extra hint copy");
  assert.ok(
    vampModalMetrics.modalWidth <= Layout.TOKENS.modal.vampWidth + 4,
    `Webapp Vamp modal should stay compact, got width ${vampModalMetrics.modalWidth}`,
  );
  assert.ok(
    vampModalMetrics.modalHeight < 260,
    `Webapp Vamp modal should stay compact vertically, got height ${vampModalMetrics.modalHeight}`,
  );

  await popout.locator("#vamp-cancel").click();
  await popout.waitForTimeout(150);

  const outputButton = popout.locator("#toggle-output-button");
  await outputButton.click();
  await popout.waitForTimeout(260);
  const afterOutput = await getPopoutMetrics(popout);
  approx(afterOutput.formWidth, initialMetrics.formWidth, 2, "Webapp output toggle should keep form width stable");
  assert.ok(
    afterOutput.formHeight >= Layout.TOKENS.popout.stableContentHeight - 4,
    `Webapp output toggle should not shrink below stable content height, got ${afterOutput.formHeight}`,
  );

  if (afterOutput.outputVisible) {
    await outputButton.click();
    await popout.waitForTimeout(220);
  }

  const reportsButton = popout.locator("#toggle-reports-button");
  await reportsButton.click();
  await popout.waitForTimeout(260);
  const afterReports = await getPopoutMetrics(popout);
  assert.equal(afterReports.reportsVisible, true, "Webapp dashboard toggle should show the reports panel");
  approx(afterReports.reportsWidth, Layout.TOKENS.popout.reportsWidth, 6, "Webapp dashboard width");
  assert.ok(
    afterReports.formHeight >= Layout.TOKENS.popout.stableContentHeight - 4,
    `Webapp dashboard toggle should not collapse below stable content height, got ${afterReports.formHeight}`,
  );
  await popout.close();
}

async function main() {
  let browser;
  try {
    browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
  } catch (error) {
    throw new Error(`Could not connect to the live Edge debug session on port 9222: ${error.message}`);
  }

  try {
    await assertDeployLayout(browser);
    await assertWebappLayout(browser);
    console.log("LaunchDeck layout invariants passed.");
  } finally {
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
