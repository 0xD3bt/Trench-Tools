(function initTrenchToolsContentRuntime() {
  if (window.TrenchToolsContentRuntime) {
    return;
  }

  const platformDescriptors = new Map();

  window.TrenchToolsContentRuntime = {
    registerPlatformAdapter(platformId, descriptor) {
      if (!platformId || typeof descriptor?.createAdapter !== "function") {
        throw new Error("Invalid platform adapter registration.");
      }
      platformDescriptors.set(platformId, descriptor);
    },

    detectPlatform(hostname) {
      for (const [platformId, descriptor] of platformDescriptors.entries()) {
        if (typeof descriptor.matchesHost === "function" && descriptor.matchesHost(hostname)) {
          return platformId;
        }
      }
      return null;
    },

    createPlatformAdapter(platformId, helpers) {
      const descriptor = platformDescriptors.get(platformId);
      if (!descriptor) {
        return null;
      }
      return descriptor.createAdapter(helpers);
    }
  };
})();
