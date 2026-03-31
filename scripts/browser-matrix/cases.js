const PROVIDERS = ["helius-sender", "standard-rpc", "jito-bundle"];
const ROUTE_FEE_PROFILES = ["manual", "auto"];
const DEV_BUY_PROFILES = ["off", "on"];
const FOLLOW_PROFILES = ["off", "minimal"];

const VALID_WALLET_ADDRESS = "11111111111111111111111111111111";
const VALID_METADATA_URI = "ipfs://launchdeck-browser-matrix/metadata.json";

const SUPPORTED_MATRIX = [
  { launchpad: "pump", mode: "regular", quoteAssets: ["sol"] },
  { launchpad: "pump", mode: "cashback", quoteAssets: ["sol"] },
  { launchpad: "pump", mode: "agent-custom", quoteAssets: ["sol"] },
  { launchpad: "pump", mode: "agent-unlocked", quoteAssets: ["sol"] },
  { launchpad: "pump", mode: "agent-locked", quoteAssets: ["sol"] },
  { launchpad: "bonk", mode: "regular", quoteAssets: ["sol", "usd1"] },
  { launchpad: "bonk", mode: "bonkers", quoteAssets: ["sol", "usd1"] },
  { launchpad: "bagsapp", mode: "bags-2-2", quoteAssets: ["sol"] },
  { launchpad: "bagsapp", mode: "bags-025-1", quoteAssets: ["sol"] },
  { launchpad: "bagsapp", mode: "bags-1-025", quoteAssets: ["sol"] },
];

function makeCaseId(parts) {
  return parts
    .filter(Boolean)
    .join("__")
    .replace(/[^a-z0-9_-]+/gi, "-")
    .toLowerCase();
}

function splitProfilesFor(launchpad, mode) {
  if (launchpad === "pump" && mode === "regular") return ["none", "meaningful-fee-split"];
  if (launchpad === "bonk" && mode === "regular") return ["none"];
  if (mode === "agent-custom") return ["agent-custom-no-init", "agent-custom-meaningful"];
  if (mode === "agent-locked") return ["agent-locked"];
  if (launchpad === "bagsapp") return ["none", "bags-meaningful-fee-split"];
  return ["none"];
}

function plannedActionLabels(splitProfile) {
  if (splitProfile === "meaningful-fee-split") return ["launch", "fee-sharing setup"];
  if (splitProfile === "agent-custom-meaningful") return ["launch", "agent fee setup"];
  if (splitProfile === "agent-locked") return ["launch", "agent fee setup"];
  return ["launch"];
}

function classifyDryRunTier({ launchpad, followProfile }) {
  if (launchpad === "bagsapp") return "partially dry-runnable";
  if (followProfile !== "off") return "partially dry-runnable";
  return "fully dry-runnable";
}

function buildSupportedCase({
  launchpad,
  mode,
  quoteAsset,
  creationProvider,
  buyProvider,
  sellProvider,
  routeFeeProfile,
  devBuyProfile,
  followProfile,
  splitProfile,
}) {
  const hasFollow = followProfile !== "off";
  const hasDevBuy = devBuyProfile === "on";
  const actionLabels = plannedActionLabels(splitProfile);
  const id = makeCaseId([
    launchpad,
    mode,
    quoteAsset,
    `create-${creationProvider}`,
    `buy-${buyProvider}`,
    `sell-${sellProvider}`,
    `fees-${routeFeeProfile}`,
    `devbuy-${devBuyProfile}`,
    `follow-${followProfile}`,
    `split-${splitProfile}`,
  ]);

  return {
    id,
    kind: "supported",
    launchpad,
    mode,
    quoteAsset,
    providers: {
      creation: creationProvider,
      buy: buyProvider,
      sell: sellProvider,
    },
    routeFeeProfile,
    devBuyProfile,
    followProfile,
    splitProfile,
    fixture: {
      token: {
        name: `Matrix ${launchpad} ${mode}`,
        symbol: `${launchpad.slice(0, 2)}${mode.replace(/[^a-z]/g, "").slice(0, 6)}`.toUpperCase().slice(0, 10),
        description: `Browser matrix validation for ${launchpad}/${mode}.`,
        website: "https://example.com/launchdeck-matrix",
        twitter: "https://x.com/launchdeck_matrix",
        telegram: "https://t.me/launchdeck_matrix",
        metadataUri: VALID_METADATA_URI,
      },
      creatorFeeProfile: "deployer",
      recipientWallet: VALID_WALLET_ADDRESS,
      devBuyAmount: hasDevBuy ? "0.001" : "",
      follow: hasFollow
        ? {
            enabled: true,
            includeDevSell: hasDevBuy,
            snipeAmountSol: "0.001",
            snipeTriggerMode: "on-submit",
            snipeSubmitDelayMs: 25,
            devSellPercent: 100,
            devSellTriggerMode: "block-offset",
            devSellBlockOffset: 1,
          }
        : {
            enabled: false,
          },
      bagsIdentity: {
        mode: "wallet-only",
        agentUsername: "",
        authToken: "",
        verifiedWallet: "",
      },
    },
    expected: {
      build: "success",
      simulate: "success",
      dryRunTier: classifyDryRunTier({ launchpad, followProfile }),
      plannedActionLabels: actionLabels,
      plannedTransactionCount: actionLabels.length,
      normalizedProviders: {
        creation: creationProvider,
        buy: buyProvider,
        sell: sellProvider,
      },
      followMetadata: hasFollow ? "present-only-in-config" : "absent",
      followDaemonEnabled: hasFollow,
    },
  };
}

function buildBlockedCases() {
  return [
    {
      id: "blocked__bonk__cashback",
      kind: "blocked",
      launchpad: "bonk",
      mode: "cashback",
      quoteAsset: "sol",
      providers: { creation: "helius-sender", buy: "helius-sender", sell: "helius-sender" },
      routeFeeProfile: "manual",
      devBuyProfile: "off",
      followProfile: "off",
      splitProfile: "none",
      fixture: {
        token: {
          name: "Blocked Bonk Cashback",
          symbol: "BBLOCK",
          description: "Blocked combo verification.",
          website: "https://example.com/blocked",
          twitter: "https://x.com/blocked",
          telegram: "https://t.me/blocked",
          metadataUri: VALID_METADATA_URI,
        },
        creatorFeeProfile: "deployer",
        recipientWallet: VALID_WALLET_ADDRESS,
        devBuyAmount: "",
        follow: { enabled: false },
        bagsIdentity: { mode: "wallet-only", agentUsername: "", authToken: "", verifiedWallet: "" },
      },
      expected: {
        build: "reject",
        simulate: "skip",
        dryRunTier: "fully dry-runnable",
        errorIncludes: "Bonk currently supports only regular and bonkers modes",
      },
    },
    {
      id: "blocked__bonk__agent-custom",
      kind: "blocked",
      launchpad: "bonk",
      mode: "agent-custom",
      quoteAsset: "sol",
      providers: { creation: "standard-rpc", buy: "jito-bundle", sell: "helius-sender" },
      routeFeeProfile: "manual",
      devBuyProfile: "off",
      followProfile: "off",
      splitProfile: "agent-custom-meaningful",
      fixture: {
        token: {
          name: "Blocked Bonk Agent",
          symbol: "BAGENT",
          description: "Blocked combo verification.",
          website: "https://example.com/blocked",
          twitter: "https://x.com/blocked",
          telegram: "https://t.me/blocked",
          metadataUri: VALID_METADATA_URI,
        },
        creatorFeeProfile: "deployer",
        recipientWallet: VALID_WALLET_ADDRESS,
        devBuyAmount: "",
        follow: { enabled: false },
        bagsIdentity: { mode: "wallet-only", agentUsername: "", authToken: "", verifiedWallet: "" },
      },
      expected: {
        build: "reject",
        simulate: "skip",
        dryRunTier: "fully dry-runnable",
        errorIncludes: "Bonk currently supports only regular and bonkers modes",
      },
    },
    {
      id: "blocked__bonk__fee-sharing-setup",
      kind: "blocked",
      launchpad: "bonk",
      mode: "regular",
      quoteAsset: "sol",
      providers: { creation: "helius-sender", buy: "helius-sender", sell: "helius-sender" },
      routeFeeProfile: "manual",
      devBuyProfile: "off",
      followProfile: "off",
      splitProfile: "meaningful-fee-split",
      fixture: {
        token: {
          name: "Blocked Bonk Fee Split",
          symbol: "BFSPLIT",
          description: "Blocked combo verification.",
          website: "https://example.com/blocked",
          twitter: "https://x.com/blocked",
          telegram: "https://t.me/blocked",
          metadataUri: VALID_METADATA_URI,
        },
        creatorFeeProfile: "deployer",
        recipientWallet: VALID_WALLET_ADDRESS,
        devBuyAmount: "",
        follow: { enabled: false },
        bagsIdentity: { mode: "wallet-only", agentUsername: "", authToken: "", verifiedWallet: "" },
      },
      expected: {
        build: "reject",
        simulate: "skip",
        dryRunTier: "fully dry-runnable",
        errorIncludes: "Bonk does not support fee-sharing setup yet.",
      },
    },
    {
      id: "blocked__bagsapp__regular",
      kind: "blocked",
      launchpad: "bagsapp",
      mode: "regular",
      quoteAsset: "sol",
      providers: { creation: "helius-sender", buy: "standard-rpc", sell: "jito-bundle" },
      routeFeeProfile: "auto",
      devBuyProfile: "off",
      followProfile: "off",
      splitProfile: "none",
      fixture: {
        token: {
          name: "Blocked Bags Regular",
          symbol: "BGREG",
          description: "Blocked combo verification.",
          website: "https://example.com/blocked",
          twitter: "https://x.com/blocked",
          telegram: "https://t.me/blocked",
          metadataUri: VALID_METADATA_URI,
        },
        creatorFeeProfile: "deployer",
        recipientWallet: VALID_WALLET_ADDRESS,
        devBuyAmount: "",
        follow: { enabled: false },
        bagsIdentity: { mode: "wallet-only", agentUsername: "", authToken: "", verifiedWallet: "" },
      },
      expected: {
        build: "reject",
        simulate: "skip",
        dryRunTier: "fully dry-runnable",
        errorIncludes: "Bagsapp currently supports only bags-2-2, bags-025-1, and bags-1-025 modes",
      },
    },
    {
      id: "blocked__pump__usd1",
      kind: "blocked",
      launchpad: "pump",
      mode: "regular",
      quoteAsset: "usd1",
      providers: { creation: "standard-rpc", buy: "standard-rpc", sell: "standard-rpc" },
      routeFeeProfile: "manual",
      devBuyProfile: "off",
      followProfile: "off",
      splitProfile: "none",
      fixture: {
        token: {
          name: "Blocked Pump USD1",
          symbol: "PUSD1",
          description: "Blocked quote asset verification.",
          website: "https://example.com/blocked",
          twitter: "https://x.com/blocked",
          telegram: "https://t.me/blocked",
          metadataUri: VALID_METADATA_URI,
        },
        creatorFeeProfile: "deployer",
        recipientWallet: VALID_WALLET_ADDRESS,
        devBuyAmount: "",
        follow: { enabled: false },
        bagsIdentity: { mode: "wallet-only", agentUsername: "", authToken: "", verifiedWallet: "" },
      },
      expected: {
        build: "reject",
        simulate: "skip",
        dryRunTier: "fully dry-runnable",
        errorIncludes: "quoteAsset=usd1 is only supported for bonk right now",
      },
    },
    {
      id: "blocked__regular__fee-split__creator-wallet",
      kind: "blocked",
      launchpad: "pump",
      mode: "regular",
      quoteAsset: "sol",
      providers: { creation: "helius-sender", buy: "helius-sender", sell: "helius-sender" },
      routeFeeProfile: "manual",
      devBuyProfile: "off",
      followProfile: "off",
      splitProfile: "meaningful-fee-split",
      fixture: {
        token: {
          name: "Blocked Creator Wallet",
          symbol: "CFWAL",
          description: "Blocked creator fee verification.",
          website: "https://example.com/blocked",
          twitter: "https://x.com/blocked",
          telegram: "https://t.me/blocked",
          metadataUri: VALID_METADATA_URI,
        },
        creatorFeeProfile: "wallet",
        recipientWallet: VALID_WALLET_ADDRESS,
        devBuyAmount: "",
        follow: { enabled: false },
        bagsIdentity: { mode: "wallet-only", agentUsername: "", authToken: "", verifiedWallet: "" },
      },
      expected: {
        build: "reject",
        simulate: "skip",
        dryRunTier: "fully dry-runnable",
        errorIncludes: "Later fee-sharing setup is only supported when the regular-mode creator fee receiver is the deployer",
      },
    },
  ];
}

function buildSupportedCases() {
  const cases = [];
  for (const base of SUPPORTED_MATRIX) {
    for (const quoteAsset of base.quoteAssets) {
      for (const creationProvider of PROVIDERS) {
        for (const buyProvider of PROVIDERS) {
          for (const sellProvider of PROVIDERS) {
            for (const routeFeeProfile of ROUTE_FEE_PROFILES) {
              for (const devBuyProfile of DEV_BUY_PROFILES) {
                for (const followProfile of FOLLOW_PROFILES) {
                  for (const splitProfile of splitProfilesFor(base.launchpad, base.mode)) {
                    cases.push(
                      buildSupportedCase({
                        launchpad: base.launchpad,
                        mode: base.mode,
                        quoteAsset,
                        creationProvider,
                        buyProvider,
                        sellProvider,
                        routeFeeProfile,
                        devBuyProfile,
                        followProfile,
                        splitProfile,
                      }),
                    );
                  }
                }
              }
            }
          }
        }
      }
    }
  }
  return cases;
}

function buildCaseMatrix() {
  return [...buildSupportedCases(), ...buildBlockedCases()];
}

module.exports = {
  PROVIDERS,
  ROUTE_FEE_PROFILES,
  DEV_BUY_PROFILES,
  FOLLOW_PROFILES,
  VALID_WALLET_ADDRESS,
  VALID_METADATA_URI,
  buildCaseMatrix,
};
