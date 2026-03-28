"use strict";

function getBonkLaunchpad() {
  return {
    id: "bonk",
    label: "Bonk",
    available: true,
    supportState: "unverified",
    tokenMetadata: {
      nameMaxLength: 32,
      symbolMaxLength: 10,
    },
    supportsStrategies: {
      "snipe-own-launch": true,
      "automatic-dev-sell": true,
      "dev-buy": true,
    },
    reason: "Official Raydium-backed integration path still needs live validation.",
    officialSdk: "@raydium-io/raydium-sdk-v2",
  };
}

module.exports = {
  getBonkLaunchpad,
};
