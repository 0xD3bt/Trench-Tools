"use strict";

function getBloxrouteProvider() {
  return {
    id: "bloxroute",
    label: "bloXroute",
    verified: false,
    supportsSingle: true,
    supportsSequential: true,
    supportsBundle: true,
  };
}

module.exports = {
  getBloxrouteProvider,
};
