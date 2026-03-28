"use strict";

require("dotenv").config({ quiet: true, override: true });

const { formToRawConfig } = require("../ui-server");
const { runEngineAction } = require("../engine-client");

function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 1) {
    const part = argv[i];
    if (!part.startsWith("--")) continue;
    const key = part.slice(2);
    const next = argv[i + 1];
    if (!next || next.startsWith("--")) {
      args[key] = true;
      continue;
    }
    args[key] = next;
    i += 1;
  }
  return args;
}

function percentile(sortedValues, p) {
  if (sortedValues.length === 0) return null;
  const index = Math.min(sortedValues.length - 1, Math.max(0, Math.ceil((p / 100) * sortedValues.length) - 1));
  return sortedValues[index];
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const action = String(args.action || "build").trim().toLowerCase();
  const iterations = Math.max(1, Number(args.iterations || 5));
  const maxWallMs = args["max-wall-ms"] !== undefined ? Number(args["max-wall-ms"]) : null;
  const maxEngineMs = args["max-engine-ms"] !== undefined ? Number(args["max-engine-ms"]) : null;
  const form = {
    launchpad: "pump",
    mode: "regular",
    name: "LaunchDeck",
    symbol: "LDECK",
    metadataUri: "ipfs://fixture",
    provider: "auto",
    policy: "fast",
    buyProvider: "auto",
    buyPolicy: "fast",
    skipPreflight: false,
  };
  const samples = [];

  for (let i = 0; i < iterations; i += 1) {
    const rawConfig = formToRawConfig(form, action);
    const startedAt = Date.now();
    const payload = await runEngineAction(action, {
      action,
      form,
      rawConfig,
    });
    const wallMs = Date.now() - startedAt;
    samples.push({
      iteration: i + 1,
      executor: payload.executor || "unknown",
      elapsedMs: Number(payload.elapsedMs || 0),
      wallMs,
    });
  }

  const sortedWall = [...samples].map((entry) => entry.wallMs).sort((a, b) => a - b);
  const sortedEngine = [...samples].map((entry) => entry.elapsedMs).sort((a, b) => a - b);
  const summary = {
    executor: samples[0] ? samples[0].executor : "unknown",
    wallMs: {
      min: sortedWall[0] || 0,
      p50: percentile(sortedWall, 50),
      p95: percentile(sortedWall, 95),
      max: sortedWall[sortedWall.length - 1] || 0,
    },
    engineElapsedMs: {
      min: sortedEngine[0] || 0,
      p50: percentile(sortedEngine, 50),
      p95: percentile(sortedEngine, 95),
      max: sortedEngine[sortedEngine.length - 1] || 0,
    },
  };
  const output = {
    ok: true,
    action,
    iterations,
    samples,
    summary,
  };

  if (maxWallMs !== null && summary.wallMs.p95 > maxWallMs) {
    process.stderr.write(
      `${JSON.stringify(
        {
          ...output,
          ok: false,
          error: `wallMs p95 ${summary.wallMs.p95} exceeded max-wall-ms ${maxWallMs}`,
        },
        null,
        2
      )}\n`
    );
    process.exitCode = 1;
    return;
  }
  if (maxEngineMs !== null && summary.engineElapsedMs.p95 > maxEngineMs) {
    process.stderr.write(
      `${JSON.stringify(
        {
          ...output,
          ok: false,
          error: `engineElapsedMs p95 ${summary.engineElapsedMs.p95} exceeded max-engine-ms ${maxEngineMs}`,
        },
        null,
        2
      )}\n`
    );
    process.exitCode = 1;
    return;
  }

  process.stdout.write(`${JSON.stringify(output, null, 2)}\n`);
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exitCode = 1;
});
