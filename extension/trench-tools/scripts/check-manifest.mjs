import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const extensionRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifestPath = path.join(extensionRoot, "manifest.json");
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));

const missing = [];

function assertResourceExists(resourcePath, source) {
  if (!resourcePath || /^https?:\/\//i.test(resourcePath)) {
    return;
  }
  const normalized = resourcePath.replace(/^\/+/, "");
  if (!fs.existsSync(path.join(extensionRoot, normalized))) {
    missing.push(`${source}: ${resourcePath}`);
  }
}

for (const [size, iconPath] of Object.entries(manifest.icons || {})) {
  assertResourceExists(iconPath, `icons.${size}`);
}

for (const [size, iconPath] of Object.entries(manifest.action?.default_icon || {})) {
  assertResourceExists(iconPath, `action.default_icon.${size}`);
}

if (manifest.action?.default_popup) {
  assertResourceExists(manifest.action.default_popup, "action.default_popup");
}

if (manifest.options_page) {
  assertResourceExists(manifest.options_page, "options_page");
}

if (manifest.background?.service_worker) {
  assertResourceExists(manifest.background.service_worker, "background.service_worker");
}

for (const [index, script] of (manifest.content_scripts || []).entries()) {
  for (const jsPath of script.js || []) {
    assertResourceExists(jsPath, `content_scripts[${index}].js`);
  }
  for (const cssPath of script.css || []) {
    assertResourceExists(cssPath, `content_scripts[${index}].css`);
  }
}

for (const [index, entry] of (manifest.web_accessible_resources || []).entries()) {
  for (const resourcePath of entry.resources || []) {
    assertResourceExists(resourcePath, `web_accessible_resources[${index}]`);
  }
}

if (missing.length) {
  console.error("Manifest references missing extension resources:");
  for (const item of missing) {
    console.error(`- ${item}`);
  }
  process.exit(1);
}

console.log("manifest ok");
