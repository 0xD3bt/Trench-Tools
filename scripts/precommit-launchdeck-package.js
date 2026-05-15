const { spawnSync } = require("node:child_process");

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: process.cwd(),
    shell: process.platform === "win32",
    stdio: "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status || 1);
}

run("npm", ["--workspace", "extension/trench-tools", "run", "package:launchdeck-shell"]);
run("git", [
  "add",
  "ui/launchdeck",
  "extension/trench-tools/launchdeck",
]);
run("git", ["add", "-u", "ui/images", "extension/trench-tools/images"]);
