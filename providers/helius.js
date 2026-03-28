"use strict";

function getHeliusProvider() {
  return {
    id: "helius",
    label: "Helius",
    verified: true,
    supportsSingle: true,
    supportsSequential: true,
    supportsBundle: false,
  };
}

module.exports = {
  getHeliusProvider,
};
