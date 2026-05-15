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
const extensionBootstrapSource = path.join(extensionRoot, "src", "launchdeck-extension-bootstrap.js");
const extensionBootstrapTarget = path.join(launchdeckTarget, "extension-bootstrap.js");
const imageAssetSnapshot = new Map();

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

function snapshotDir(sourceDir, relativePrefix = "") {
  if (!fs.existsSync(sourceDir)) return;
  for (const entry of fs.readdirSync(sourceDir, { withFileTypes: true })) {
    const sourcePath = path.join(sourceDir, entry.name);
    const relativePath = path.join(relativePrefix, entry.name);
    if (entry.isDirectory()) {
      snapshotDir(sourcePath, relativePath);
    } else if (entry.isFile()) {
      imageAssetSnapshot.set(relativePath.replaceAll(path.sep, "/"), fs.readFileSync(sourcePath));
    }
  }
}

function collectReferencedImageAssets(sourceDir) {
  const references = new Set();
  const imageReferencePattern = /(?:(?:\.\.\/)+images\/|\/images\/|(?<![A-Za-z0-9:/._-])images\/)([A-Za-z0-9._/-]+\.(?:png|svg|jpg|jpeg|webp|gif|avif))/gi;
  function scanDir(targetDir) {
    for (const entry of fs.readdirSync(targetDir, { withFileTypes: true })) {
      const targetPath = path.join(targetDir, entry.name);
      if (entry.isDirectory()) {
        if (entry.name === "node_modules" || entry.name === "tests") continue;
        scanDir(targetPath);
        continue;
      }
      if (!entry.isFile() || !/\.(?:html|css|js|json)$/i.test(entry.name)) continue;
      const contents = fs.readFileSync(targetPath, "utf8");
      for (const match of contents.matchAll(imageReferencePattern)) {
        const precedingText = contents.slice(Math.max(0, match.index - 128), match.index);
        if (/https?:\/\/[^"'`\s)]*$/i.test(precedingText)) continue;
        references.add(match[1].replaceAll("\\", "/"));
      }
    }
  }
  scanDir(sourceDir);
  return references;
}

function copyReferencedImages(references) {
  ensureDir(imagesTarget);
  for (const relativeName of references) {
    const sourcePath = path.join(imagesSource, relativeName);
    const targetPath = path.join(imagesTarget, relativeName);
    ensureDir(path.dirname(targetPath));
    if (fs.existsSync(sourcePath)) {
      fs.copyFileSync(sourcePath, targetPath);
    } else if (imageAssetSnapshot.has(relativeName)) {
      fs.writeFileSync(targetPath, imageAssetSnapshot.get(relativeName));
    } else {
      throw new Error(`Referenced image asset is missing: ${relativeName}`);
    }
  }
}

function injectExtensionOnlyScripts() {
  const indexPath = path.join(launchdeckTarget, "index.html");
  const bootstrapScript = '    <script src="/launchdeck/extension-bootstrap.js"></script>';
  const migrationsScript = '    <script src="/src/shared/storage-migrations.js"></script>';
  let contents = fs.readFileSync(indexPath, "utf8");
  if (contents.includes(migrationsScript)) return;
  if (!contents.includes(bootstrapScript)) {
    throw new Error("LaunchDeck index is missing extension bootstrap script.");
  }
  contents = contents.replace(bootstrapScript, `${migrationsScript}\n${bootstrapScript}`);
  fs.writeFileSync(indexPath, contents);
}

snapshotDir(imagesTarget);
resetDir(launchdeckTarget);
resetDir(imagesTarget);
copyDir(launchdeckSource, launchdeckTarget);
injectExtensionOnlyScripts();
copyReferencedImages(collectReferencedImageAssets(extensionRoot));
fs.copyFileSync(extensionBootstrapSource, extensionBootstrapTarget);

console.log("Packaged LaunchDeck shell assets into the extension.");
