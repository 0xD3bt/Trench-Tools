// Standalone LaunchDeck UI shim.
//
// The extension popout ships its own `extension-bootstrap.js` (under
// `extension/trench-tools/launchdeck/`) that performs host/token migrations
// and proxies fetches to the right backend. The standalone LaunchDeck host
// (`launchdeck-engine` serving this directory) never runs from a
// `chrome-extension://` origin and does not need that plumbing, so this file
// is intentionally empty aside from an origin guard. Keeping it as a no-op
// means `index.html` does not need a separate template for extension vs.
// standalone builds.
(function initLaunchDeckExtensionBootstrap(global) {
  const isExtensionOrigin = global.location && global.location.protocol === "chrome-extension:";
  if (!isExtensionOrigin) {
    return;
  }
  // If this file is ever loaded under a `chrome-extension://` origin the
  // extension's own bootstrap should already be loading in its place. Fall
  // through without touching globals.
})(window);
