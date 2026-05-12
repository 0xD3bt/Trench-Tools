const { spawn } = require("child_process");
const fs = require("fs");
const path = require("path");
const { printStartupDiagnostics } = require("./runtime-diagnostics");

const action = process.argv[2];
const validActions = new Set(["start", "stop", "restart"]);

if (!validActions.has(action)) {
  console.error(`Usage: node scripts/runtime-control.js <${Array.from(validActions).join("|")}>`);
  process.exit(1);
}

const projectRoot = path.resolve(__dirname, "..");
const isWindows = process.platform === "win32";

function parseEnvFile() {
  const envPath = path.join(projectRoot, ".env");
  const values = {};
  let contents = "";
  try {
    contents = fs.readFileSync(envPath, "utf8");
  } catch {
    return values;
  }
  for (const rawLine of contents.split(/\r?\n/)) {
    let line = rawLine.replace(/\r$/, "").trim();
    if (!line || line.startsWith("#")) {
      continue;
    }
    if (line.startsWith("export ")) {
      line = line.slice("export ".length);
    }
    const separatorIndex = line.indexOf("=");
    if (separatorIndex < 1) {
      continue;
    }
    const name = line.slice(0, separatorIndex).trim();
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
      continue;
    }
    let value = line.slice(separatorIndex + 1);
    if (value.length >= 2) {
      const first = value.slice(0, 1);
      const last = value.slice(-1);
      if ((first === '"' && last === '"') || (first === "'" && last === "'")) {
        value = value.slice(1, -1);
      }
    }
    values[name] = value;
  }
  return values;
}

function resolveMode() {
  const env = { ...process.env, ...parseEnvFile() };
  const candidate = String(env.TRENCH_TOOLS_MODE || "").trim() || "both";
  const normalized = candidate.toLowerCase();
  if (normalized === "ee" || normalized === "ld" || normalized === "both") {
    return normalized;
  }
  throw new Error("mode must be ee, ld, or both.");
}

function diagnosticsOptionsForMode(mode) {
  return {
    includeExecution: mode !== "ld",
    includeLaunchdeck: mode !== "ee",
  };
}

function resolvePosixShell() {
  const candidates = ["/bin/bash", "/usr/bin/bash", "/usr/local/bin/bash"];
  for (const candidate of candidates) {
    try {
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    } catch {
    }
  }
  return "bash";
}

const command = isWindows ? "powershell" : resolvePosixShell();

function scriptPathFor(kind) {
  if (isWindows) {
    return path.join(projectRoot, kind === "stop" ? "trench-tools-stop.ps1" : "trench-tools-start.ps1");
  }
  return path.join(projectRoot, kind === "stop" ? "trench-tools-stop.sh" : "trench-tools-start.sh");
}

function argsFor(scriptPath, selectedMode) {
  if (isWindows) {
    // `-NoProfile` skips user profile scripts, which add noise/latency and
    // occasionally break on minimal/pristine Windows VPS images.
    return ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", scriptPath, "--mode", selectedMode];
  }
  return [scriptPath, "--mode", selectedMode];
}

function run(kind, selectedMode) {
  const scriptPath = scriptPathFor(kind);
  const child = spawn(command, argsFor(scriptPath, selectedMode), {
    cwd: projectRoot,
    env: { ...process.env, TRENCH_TOOLS_FINAL_DIAGNOSTICS: "1" },
    stdio: "inherit",
  });

  const forwardSignal = (signal) => {
    try {
      child.kill(signal);
    } catch {
    }
  };
  const sigintHandler = () => forwardSignal("SIGINT");
  const sigtermHandler = () => forwardSignal("SIGTERM");
  process.on("SIGINT", sigintHandler);
  process.on("SIGTERM", sigtermHandler);

  return new Promise((resolve, reject) => {
    const cleanup = () => {
      process.off("SIGINT", sigintHandler);
      process.off("SIGTERM", sigtermHandler);
    };
    child.on("exit", (code, signal) => {
      cleanup();
      if (signal) {
        reject(Object.assign(new Error(`${path.basename(scriptPath)} exited with signal ${signal}`), { signal }));
        return;
      }
      if (code && code !== 0) {
        reject(new Error(`${path.basename(scriptPath)} exited with code ${code}`));
        return;
      }
      resolve();
    });

    child.on("error", (error) => {
      cleanup();
      reject(new Error(`Failed to run ${path.basename(scriptPath)}: ${error.message}`));
    });
  });
}

(async () => {
  const selectedMode = resolveMode();
  if (action === "restart") {
    await run("start", selectedMode);
    await printStartupDiagnostics(diagnosticsOptionsForMode(selectedMode));
    return;
  }
  await run(action, selectedMode);
  if (action === "start") {
    await printStartupDiagnostics(diagnosticsOptionsForMode(selectedMode));
  }
})().catch((error) => {
  if (error && error.signal) {
    process.kill(process.pid, error.signal);
    return;
  }
  console.error(error.message);
  process.exit(1);
});
