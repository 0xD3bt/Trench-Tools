import assert from "node:assert/strict";

import {
  isEeOnlyTrenchToolsMode,
  isLdOnlyTrenchToolsMode,
  normalizeTrenchToolsMode,
  shouldProbeLaunchdeckRuntime
} from "../src/shared/runtime-mode.js";

assert.equal(normalizeTrenchToolsMode("ee"), "ee");
assert.equal(normalizeTrenchToolsMode("LD"), "ld");
assert.equal(normalizeTrenchToolsMode(" both "), "both");
assert.equal(normalizeTrenchToolsMode(""), "both");
assert.equal(normalizeTrenchToolsMode("unknown"), "both");

assert.equal(isEeOnlyTrenchToolsMode("ee"), true);
assert.equal(isEeOnlyTrenchToolsMode("both"), false);
assert.equal(isEeOnlyTrenchToolsMode(undefined), false);
assert.equal(isLdOnlyTrenchToolsMode("ld"), true);
assert.equal(isLdOnlyTrenchToolsMode("both"), false);
assert.equal(isLdOnlyTrenchToolsMode(undefined), false);

assert.equal(shouldProbeLaunchdeckRuntime({ trenchToolsMode: "ee" }), false);
assert.equal(shouldProbeLaunchdeckRuntime({ trenchToolsMode: "both" }), true);
assert.equal(shouldProbeLaunchdeckRuntime({ trenchToolsMode: "ld" }), true);
assert.equal(shouldProbeLaunchdeckRuntime({}), true);
assert.equal(shouldProbeLaunchdeckRuntime(null), true);

console.log("runtime mode tests passed");
