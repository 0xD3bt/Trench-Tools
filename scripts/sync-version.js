const fs = require("fs");
const path = require("path");

const repoRoot = path.resolve(__dirname, "..");
const checkOnly = process.argv.includes("--check");

function readText(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
}

function writeText(relativePath, value) {
  fs.writeFileSync(path.join(repoRoot, relativePath), value);
}

function readJson(relativePath) {
  return JSON.parse(readText(relativePath));
}

function formatJson(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function updateJsonVersion(relativePath, version, packageKeys = []) {
  const json = readJson(relativePath);
  if (!Object.prototype.hasOwnProperty.call(json, "version")) {
    throw new Error(`${relativePath} is missing a top-level version field`);
  }
  json.version = version;
  for (const keyPath of packageKeys) {
    let target = json;
    for (const key of keyPath) {
      target = target && target[key];
    }
    if (!target || typeof target !== "object" || !Object.prototype.hasOwnProperty.call(target, "version")) {
      throw new Error(`${relativePath} is missing version field at ${keyPath.join(".")}`);
    }
    target.version = version;
  }
  return [relativePath, formatJson(json)];
}

function updateCargoVersion(relativePath, version) {
  const raw = readText(relativePath);
  const pattern = /(^version\s*=\s*")[^"]+(")/m;
  if (!pattern.test(raw)) {
    throw new Error(`${relativePath} is missing a Cargo package version field`);
  }
  const next = raw.replace(pattern, `$1${version}$2`);
  return [relativePath, next];
}

function updateCargoLockPackage(relativePath, packageName, version) {
  const raw = readText(relativePath);
  const escapedName = packageName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(`(\\[\\[package\\]\\]\\r?\\nname = "${escapedName}"\\r?\\nversion = ")[^"]+(")`, "m");
  if (!pattern.test(raw)) {
    throw new Error(`${relativePath} is missing package entry for ${packageName}`);
  }
  const next = raw.replace(pattern, `$1${version}$2`);
  return [relativePath, next];
}

const rootPackage = readJson("package.json");
const version = String(rootPackage.version || "").trim();
if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`Invalid root package version: ${version}`);
  process.exit(1);
}

const updates = new Map();
for (const [relativePath, contents] of [
  updateJsonVersion("package.json", version),
  updateJsonVersion("extension/trench-tools/package.json", version),
  updateJsonVersion("extension/trench-tools/manifest.json", version),
  updateJsonVersion("package-lock.json", version, [
    ["packages", ""],
    ["packages", "extension/trench-tools"],
  ]),
  updateJsonVersion("extension/trench-tools/package-lock.json", version, [
    ["packages", ""],
  ]),
  updateCargoVersion("rust/launchdeck-engine/Cargo.toml", version),
  updateCargoVersion("rust/execution-engine/Cargo.toml", version),
  updateCargoLockPackage("Cargo.lock", "launchdeck-engine", version),
  updateCargoLockPackage("Cargo.lock", "execution-engine", version),
]) {
  updates.set(relativePath, contents);
}

const stale = [];
for (const [relativePath, contents] of updates) {
  if (readText(relativePath) === contents) continue;
  stale.push(relativePath);
  if (!checkOnly) {
    writeText(relativePath, contents);
  }
}

if (stale.length && checkOnly) {
  console.error(`Version files are not synced to ${version}:`);
  for (const relativePath of stale) {
    console.error(`- ${relativePath}`);
  }
  console.error("Run `npm run sync:version` and commit the updated files.");
  process.exit(1);
}

if (stale.length) {
  console.log(`Synced ${stale.length} version file(s) to ${version}.`);
} else {
  console.log(`Version files are synced to ${version}.`);
}
