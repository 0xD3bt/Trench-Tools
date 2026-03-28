"use strict";

function getPumpLaunchpad() {
  return {
    id: "pump",
    label: "Pump",
    available: true,
    supportState: "verified",
    tokenMetadata: {
      nameMaxLength: 32,
      symbolMaxLength: 10,
    },
    supportsStrategies: {
      "snipe-own-launch": true,
      "automatic-dev-sell": true,
      "dev-buy": true,
    },
    reason: "",
  };
}

module.exports = {
  getPumpLaunchpad,
};
