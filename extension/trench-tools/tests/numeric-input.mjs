import assert from "node:assert/strict";

await import("../src/shared/numeric-input.js");
const numeric = globalThis.TrenchNumericInput;

const { normalizeUnsignedDecimalInput, parsePercentInput } = numeric;

assert.equal(normalizeUnsignedDecimalInput("1,5"), "1.5");
assert.equal(normalizeUnsignedDecimalInput(".5"), "0.5");
assert.equal(normalizeUnsignedDecimalInput("001.50"), "1.50");

assert.equal(normalizeUnsignedDecimalInput("1.234,56"), "");
assert.equal(normalizeUnsignedDecimalInput("1,234.56"), "");
assert.equal(normalizeUnsignedDecimalInput("1,234"), "");
assert.equal(normalizeUnsignedDecimalInput("1,2,3"), "");
assert.equal(normalizeUnsignedDecimalInput("1.2.3"), "");

assert.equal(normalizeUnsignedDecimalInput("1,5m", { allowSuffix: true, maxDecimals: 6 }), "1.5m");
assert.equal(normalizeUnsignedDecimalInput("1,500m", { allowSuffix: true, maxDecimals: 6 }), "");

assert.equal(parsePercentInput("50", { integer: true }), "50");
assert.equal(parsePercentInput("0,5", { integer: true }), "");
assert.equal(parsePercentInput("150", { integer: true }), "");

console.log("numeric-input tests passed");
