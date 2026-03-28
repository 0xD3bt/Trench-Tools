"use strict";

function getJitoProvider() {
  return {
    id: "jito",
    label: "Jito",
    verified: true,
    supportsSingle: true,
    supportsSequential: true,
    supportsBundle: true,
  };
}

module.exports = {
  getJitoProvider,
};
