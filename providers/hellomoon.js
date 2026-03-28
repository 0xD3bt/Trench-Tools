"use strict";

function getHelloMoonProvider() {
  return {
    id: "hellomoon",
    label: "Hello Moon",
    verified: false,
    supportsSingle: true,
    supportsSequential: true,
    supportsBundle: true,
  };
}

module.exports = {
  getHelloMoonProvider,
};
