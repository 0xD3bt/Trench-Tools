#!/usr/bin/env node

const fs = require("fs");
const path = require("path");
const { spawn, spawnSync } = require("child_process");

const { buildCaseMatrix } = require("./cases");

const PROJECT_ROOT = path.resolve(__dirname, "..", "..");
const DEFAULT_BASE_URL = process.env.LAUNCHDECK_MATRIX_BASE_URL || "http://127.0.0.1:8789";
const DEFAULT_RESULTS_ROOT = path.join(PROJECT_ROOT, ".local", "launchdeck", "browser-matrix-fast");
const INNER_RUNNER = path.join(PROJECT_ROOT, "scripts", "browser-matrix", "run.js");

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
    includeBags: false,
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
    } else if (token === "--include-bags") {
      args.includeBags = true;
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
  console.log(`LaunchDeck fast browser matrix runner

Usage:
  node scripts/browser-matrix/run-fast.js [options]

This runner keeps the exhaustive browser matrix intact, but skips Bags by default
so Pump/Bonk verification can run faster. Use --include-bags or --launchpad bagsapp
when you intentionally want to include Bags.

Options:
  --base-url <url>        Base LaunchDeck URL. Default: ${DEFAULT_BASE_URL}
  --results-root <path>   Where fast-run batches are written.
  --launchpad <id>        Run only one launchpad.
  --mode <id>             Filter by mode.
  --case <id>             Run a single case id.
  --limit <n>             Stop after n filtered cases per sub-run.
  --headed                Show the browser instead of running headless.
  --reuse-runtime         Do not auto-start the runtime if the health check fails.
  --include-bags          Include Bags cases in the fast fan-out run.
  --list                  Print filtered case ids and exit.
  --help                  Show this message.
`);
  process.exit(code);
}

function ensureDir(dirPath) {
  fs.mkdirSync(dirPath, { recursive: true });
}

function timestampSlug() {
  return new Date().toISOString().replace(/[:.]/g, "-");
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

function baseRunnerArgs(args) {
  const runnerArgs = [];
  if (args.baseUrl) runnerArgs.push("--base-url", args.baseUrl);
  if (args.mode) runnerArgs.push("--mode", args.mode);
  if (args.caseId) runnerArgs.push("--case", args.caseId);
  if (args.limit > 0) runnerArgs.push("--limit", String(args.limit));
  if (args.headed) runnerArgs.push("--headed");
  if (args.listOnly) runnerArgs.push("--list");
  return runnerArgs;
}

function hasMatchingCases(cases, launchpad, args) {
  return cases.some((entry) => {
    if (entry.launchpad !== launchpad) return false;
    if (args.mode && entry.mode !== args.mode) return false;
    if (args.caseId && entry.id !== args.caseId) return false;
    return true;
  });
}

function selectedLaunchpads(cases, args) {
  if (args.launchpad) return [args.launchpad];
  if (args.caseId) {
    const matched = cases.find((entry) => entry.id === args.caseId);
    return matched ? [matched.launchpad] : [];
  }
  const preferred = args.includeBags ? ["pump", "bonk", "bagsapp"] : ["pump", "bonk"];
  return preferred.filter((launchpad) => hasMatchingCases(cases, launchpad, args));
}

function runChild(launchpad, runnerArgs) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [INNER_RUNNER, ...runnerArgs], {
      cwd: PROJECT_ROOT,
      stdio: "inherit",
      env: process.env,
    });
    child.on("error", reject);
    child.on("exit", (code) => {
      resolve({
        launchpad,
        exitCode: code == null ? 1 : code,
      });
    });
  });
}

function latestDirectory(dirPath) {
  if (!fs.existsSync(dirPath)) return "";
  const entries = fs.readdirSync(dirPath, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => path.join(dirPath, entry.name));
  if (!entries.length) return "";
  entries.sort((left, right) => fs.statSync(right).mtimeMs - fs.statSync(left).mtimeMs);
  return entries[0];
}

function writeSummary(sessionDir, results) {
  const summary = {
    createdAt: new Date().toISOString(),
    results,
  };
  fs.writeFileSync(path.join(sessionDir, "summary.json"), JSON.stringify(summary, null, 2));
  const lines = [
    "# Fast Browser Matrix Summary",
    "",
    `Created: ${summary.createdAt}`,
    "",
  ];
  for (const result of results) {
    lines.push(`- ${result.launchpad}: exit=${result.exitCode} batch=${result.batchDir || "(none)"}`);
  }
  fs.writeFileSync(path.join(sessionDir, "summary.md"), `${lines.join("\n")}\n`);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const cases = buildCaseMatrix();
  const launchpads = selectedLaunchpads(cases, args);

  if (!launchpads.length) {
    throw new Error("No fast-matrix cases matched the provided filters.");
  }

  if (!args.listOnly) {
    await ensureRuntime(args.baseUrl, args.reuseRuntime);
  }

  if (args.listOnly && launchpads.length === 1) {
    const runnerArgs = [...baseRunnerArgs(args), "--launchpad", launchpads[0]];
    const result = await runChild(launchpads[0], runnerArgs);
    process.exit(result.exitCode);
  }

  const sessionDir = path.join(args.resultsRoot, timestampSlug());
  ensureDir(sessionDir);

  const jobs = launchpads.map((launchpad) => {
    const subResultsRoot = path.join(sessionDir, launchpad);
    ensureDir(subResultsRoot);
    const runnerArgs = [
      ...baseRunnerArgs(args),
      "--launchpad",
      launchpad,
      "--results-root",
      subResultsRoot,
    ];
    if (!args.listOnly) runnerArgs.push("--reuse-runtime");
    return runChild(launchpad, runnerArgs);
  });

  const results = await Promise.all(jobs);
  for (const result of results) {
    result.batchDir = args.listOnly ? "" : latestDirectory(path.join(sessionDir, result.launchpad));
  }

  if (!args.listOnly) {
    writeSummary(sessionDir, results);
    console.log(`Fast matrix summary written to ${path.join(sessionDir, "summary.md")}`);
  }

  const failures = results.filter((result) => result.exitCode !== 0);
  process.exit(failures.length ? 1 : 0);
}

main().catch((error) => {
  console.error(error && error.stack ? error.stack : error);
  process.exit(1);
});
