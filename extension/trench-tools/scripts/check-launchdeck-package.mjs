import { execFileSync } from "node:child_process";
import { cpSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const extensionRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(extensionRoot, "..", "..");

function run(command, args, options = {}) {
  execFileSync(resolveCommand(command), args, {
    cwd: options.cwd || repoRoot,
    stdio: "inherit",
  });
}

function resolveCommand(command) {
  if (process.platform !== "win32") return command;
  if (command === "npm") return "npm.cmd";
  return command;
}

function gitDiffAgainstTemp(label, targetPath, tempPath) {
  try {
    execFileSync(resolveCommand("git"), ["diff", "--no-index", "--quiet", "--", tempPath, targetPath], {
      cwd: repoRoot,
      stdio: "ignore",
    });
    return false;
  } catch (error) {
    if (error.status !== 1) {
      throw error;
    }
    console.error("");
    console.error(`LaunchDeck extension package is stale: ${label} changed after packaging.`);
    try {
      execFileSync(resolveCommand("git"), ["diff", "--no-index", "--", tempPath, targetPath], {
        cwd: repoRoot,
        stdio: "inherit",
      });
    } catch (diffError) {
      if (diffError.status !== 1) {
        throw diffError;
      }
    }
    return true;
  }
}

const tempRoot = mkdtempSync(path.join(tmpdir(), "launchdeck-package-check-"));
let shouldRestorePackageSnapshot = true;
let exitCode = 0;
try {
  const beforeLaunchdeck = path.join(tempRoot, "launchdeck");
  const beforeImages = path.join(tempRoot, "images");

  cpSync(path.join(extensionRoot, "launchdeck"), beforeLaunchdeck, { recursive: true, force: true });
  cpSync(path.join(extensionRoot, "images"), beforeImages, { recursive: true, force: true });

  run(process.execPath, ["./scripts/package-launchdeck-shell.mjs"], { cwd: extensionRoot });

  try {
    execFileSync(resolveCommand("git"), [
      "diff",
      "--no-index",
      "--quiet",
      "--",
      path.join(extensionRoot, "src", "launchdeck-extension-bootstrap.js"),
      path.join(extensionRoot, "launchdeck", "extension-bootstrap.js"),
    ], {
      cwd: repoRoot,
      stdio: "ignore",
    });
  } catch (error) {
    if (error.status !== 1) {
      throw error;
    }
    console.error("");
    console.error("LaunchDeck extension bootstrap does not match its canonical source.");
    console.error("Run `npm run package:launchdeck-shell` in extension/trench-tools and commit the generated files.");
    exitCode = 1;
  }

  if (!exitCode) {
    const packageChanged = [
      gitDiffAgainstTemp("launchdeck", path.join(extensionRoot, "launchdeck"), beforeLaunchdeck),
      gitDiffAgainstTemp("images", path.join(extensionRoot, "images"), beforeImages),
    ].some(Boolean);

    if (packageChanged) {
      console.error("");
      console.error("Run `npm run package:launchdeck-shell` in extension/trench-tools and commit the generated files.");
      exitCode = 1;
    }
  }
  shouldRestorePackageSnapshot = Boolean(exitCode);
} finally {
  if (shouldRestorePackageSnapshot) {
    const beforeLaunchdeck = path.join(tempRoot, "launchdeck");
    const beforeImages = path.join(tempRoot, "images");
    rmSync(path.join(extensionRoot, "launchdeck"), { recursive: true, force: true });
    rmSync(path.join(extensionRoot, "images"), { recursive: true, force: true });
    cpSync(beforeLaunchdeck, path.join(extensionRoot, "launchdeck"), { recursive: true, force: true });
    cpSync(beforeImages, path.join(extensionRoot, "images"), { recursive: true, force: true });
  }
  rmSync(tempRoot, { recursive: true, force: true });
}

if (exitCode) {
  process.exit(exitCode);
}

console.log("LaunchDeck extension package is current.");
