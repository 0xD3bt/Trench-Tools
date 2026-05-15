const { spawnSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");

const projectRoot = path.resolve(__dirname, "..");
const extensionSource = path.join(projectRoot, "extension", "trench-tools");
const distDir = path.join(projectRoot, "dist");
const outputZip = path.join(distDir, "trench-tools-extension.zip");
const tempRoot = path.join(distDir, ".extension-zip-tmp");
const packagedFolderName = "trench-tools-extension";
const stagedExtension = path.join(tempRoot, packagedFolderName);

const includedRuntimePaths = [
  "assets",
  "images",
  "launchdeck",
  "src/background",
  "src/content",
  "src/offscreen",
  "src/options",
  "src/panel",
  "src/popup",
  "src/shared",
  "manifest.json",
];

function copyRuntimePath(relativePath) {
  const source = path.join(extensionSource, relativePath);
  const target = path.join(stagedExtension, relativePath);
  if (!fs.existsSync(source)) {
    throw new Error(`Missing extension runtime path: ${relativePath}`);
  }
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.cpSync(source, target, { recursive: true });
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd || projectRoot,
    stdio: "inherit",
    shell: Boolean(options.shell),
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} exited with ${result.status}`);
  }
}

function packageWithZip() {
  run("zip", ["-qr", outputZip, packagedFolderName], { cwd: tempRoot });
}

function packageWithPowerShell() {
  run(
    "powershell",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-Command",
      `Compress-Archive -Path ${JSON.stringify(stagedExtension)} -DestinationPath ${JSON.stringify(outputZip)} -Force`,
    ],
    { cwd: tempRoot }
  );
}

if (!fs.existsSync(path.join(extensionSource, "manifest.json"))) {
  throw new Error(`Missing extension manifest at ${path.join(extensionSource, "manifest.json")}`);
}

fs.mkdirSync(distDir, { recursive: true });
fs.rmSync(tempRoot, { recursive: true, force: true });
fs.mkdirSync(tempRoot, { recursive: true });
fs.mkdirSync(stagedExtension, { recursive: true });
for (const relativePath of includedRuntimePaths) {
  copyRuntimePath(relativePath);
}
fs.rmSync(outputZip, { force: true });

if (os.platform() === "win32") {
  packageWithPowerShell();
} else {
  packageWithZip();
}

fs.rmSync(tempRoot, { recursive: true, force: true });

const sizeBytes = fs.statSync(outputZip).size;
console.log(`Created ${path.relative(projectRoot, outputZip)} (${(sizeBytes / 1024 / 1024).toFixed(2)} MB)`);
console.log(`Unzip it, then load the ${packagedFolderName} folder as an unpacked Chrome/Edge extension.`);
