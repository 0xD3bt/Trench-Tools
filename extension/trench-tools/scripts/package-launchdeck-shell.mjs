import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(__dirname, "..");
const repoRoot = path.resolve(extensionRoot, "..", "..");
const launchdeckSource = path.join(repoRoot, "ui", "launchdeck");
const imagesSource = path.join(repoRoot, "ui", "images");
const launchdeckTarget = path.join(extensionRoot, "launchdeck");
const imagesTarget = path.join(extensionRoot, "images");
const extensionBootstrapTarget = path.join(launchdeckTarget, "extension-bootstrap.js");
const extensionBootstrapSnapshot = fs.existsSync(extensionBootstrapTarget)
  ? fs.readFileSync(extensionBootstrapTarget)
  : null;

function ensureDir(targetPath) {
  fs.mkdirSync(targetPath, { recursive: true });
}

function resetDir(targetPath) {
  fs.rmSync(targetPath, { recursive: true, force: true });
  ensureDir(targetPath);
}

function copyDir(sourceDir, targetDir) {
  ensureDir(targetDir);
  for (const entry of fs.readdirSync(sourceDir, { withFileTypes: true })) {
    const sourcePath = path.join(sourceDir, entry.name);
    const targetPath = path.join(targetDir, entry.name);
    if (entry.isDirectory()) {
      copyDir(sourcePath, targetPath);
    } else if (entry.isFile()) {
      fs.copyFileSync(sourcePath, targetPath);
    }
  }
}

resetDir(launchdeckTarget);
resetDir(imagesTarget);
copyDir(launchdeckSource, launchdeckTarget);
copyDir(imagesSource, imagesTarget);
if (extensionBootstrapSnapshot) {
  fs.writeFileSync(extensionBootstrapTarget, extensionBootstrapSnapshot);
}

console.log("Packaged LaunchDeck shell assets into the extension.");
