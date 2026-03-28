"use strict";

const { getHeliusProvider } = require("./helius");
const { getJitoProvider } = require("./jito");
const { getAstralaneProvider } = require("./astralane");
const { getBloxrouteProvider } = require("./bloxroute");
const { getHelloMoonProvider } = require("./hellomoon");

function getProviderRegistry() {
  const providers = [
    {
      id: "auto",
      label: "Auto",
      verified: true,
      supportsSingle: true,
      supportsSequential: true,
      supportsBundle: true,
    },
    getHeliusProvider(),
    getJitoProvider(),
    getAstralaneProvider(),
    getBloxrouteProvider(),
    getHelloMoonProvider(),
  ];
  return Object.fromEntries(providers.map((entry) => [entry.id, entry]));
}

module.exports = {
  getProviderRegistry,
};
