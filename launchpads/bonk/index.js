"use strict";

function getBonkLaunchpad() {
  return {
    id: "bonk",
    label: "Bonk",
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
    reason:
      "Bonk routes through LetsBonk and Bonkers on Raydium LaunchLab with SOL/USD1 quote-asset support, auto USD1 top-up, compile/send, dev-buy, same-time snipers, dev auto-sell, and follow buy/sell automation.",
    officialSdk: "@raydium-io/raydium-sdk-v2",
  };
}

module.exports = {
  getBonkLaunchpad,
};
