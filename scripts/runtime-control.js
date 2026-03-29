const { spawn } = require("child_process");
const path = require("path");

const action = process.argv[2];
const validActions = new Set(["start", "stop", "restart"]);

if (!validActions.has(action)) {
  console.error(`Usage: node scripts/runtime-control.js <${Array.from(validActions).join("|")}>`);
  process.exit(1);
}

const projectRoot = path.resolve(__dirname, "..");
const isWindows = process.platform === "win32";

const scriptPath = (() => {
  if (isWindows) {
    return path.join(projectRoot, action === "stop" ? "stop.ps1" : "start.ps1");
  }
  return path.join(projectRoot, `${action}.sh`);
})();

const command = isWindows ? "powershell" : "sh";
const args = isWindows
  ? ["-ExecutionPolicy", "Bypass", "-File", scriptPath]
  : [scriptPath];

const child = spawn(command, args, {
  cwd: projectRoot,
  stdio: "inherit",
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code == null ? 1 : code);
});

child.on("error", (error) => {
  console.error(`Failed to run ${path.basename(scriptPath)}: ${error.message}`);
  process.exit(1);
});
