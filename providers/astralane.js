"use strict";

function getAstralaneProvider() {
  return {
    id: "astralane",
    label: "Astralane",
    verified: true,
    supportsSingle: true,
    supportsSequential: true,
    supportsBundle: true,
  };
}

module.exports = {
  getAstralaneProvider,
};
