const { spawn } = require("child_process");
const fs = require("fs");
const path = require("path");

const action = process.argv[2];
const baseActions = new Set(["start", "stop", "restart"]);

function printUsage() {
  console.error(
    "Usage: node scripts/execution-engine-runtime-control.js <start|stop|restart>"
  );
}

const projectRoot = path.resolve(__dirname, "..");

if (!baseActions.has(action)) {
  printUsage();
  process.exit(1);
}

const isWindows = process.platform === "win32";

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

function argsFor(scriptPath) {
  if (isWindows) {
    return ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", scriptPath, "--mode", "ee"];
  }
  return [scriptPath, "--mode", "ee"];
}

function run(kind) {
  const scriptPath = scriptPathFor(kind);
  const child = spawn(command, argsFor(scriptPath), {
    cwd: projectRoot,
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
  if (action === "restart") {
    await run("stop");
    await run("start");
    return;
  }
  await run(action);
})().catch((error) => {
  if (error && error.signal) {
    process.kill(process.pid, error.signal);
    return;
  }
  console.error(error.message);
  process.exit(1);
});
