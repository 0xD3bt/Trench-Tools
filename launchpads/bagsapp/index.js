"use strict";

function getBagsLaunchpad() {
  const configured = Boolean(process.env.BAGS_API_KEY);
  return {
    id: "bagsapp",
    label: "Bagsapp",
    available: configured,
    supportState: configured ? "unverified" : "configured-required",
    tokenMetadata: {
      nameMaxLength: 32,
      symbolMaxLength: 10,
    },
    supportsStrategies: {
      "snipe-own-launch": false,
      "automatic-dev-sell": false,
      "dev-buy": true,
    },
    reason: configured
      ? "Bags integration is wired for the documented launch flow but still needs live validation."
      : "Missing BAGS_API_KEY.",
  };
}

module.exports = {
  getBagsLaunchpad,
};
