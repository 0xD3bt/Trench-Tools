(function installTrenchToolsAxiomBlocker() {
  // `axiom-override.js` is loaded directly into the page's main world at
  // document_start from manifest.json so it can wrap WebSocket before Axiom
  // creates/caches a constructor. This stub is retained for update continuity.
})();
