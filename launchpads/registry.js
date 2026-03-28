"use strict";

const { getPumpLaunchpad } = require("./pump");
const { getBonkLaunchpad } = require("./bonk");
const { getBagsLaunchpad } = require("./bagsapp");

function getLaunchpadRegistry() {
  const entries = [getPumpLaunchpad(), getBonkLaunchpad(), getBagsLaunchpad()];
  return Object.fromEntries(entries.map((entry) => [entry.id, entry]));
}

module.exports = {
  getLaunchpadRegistry,
};
