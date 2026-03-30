"use strict";

require("dotenv").config({ quiet: true });

const bs58 = require("bs58");
const BN = require("bn.js");
const {
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  VersionedTransaction,
} = require("@solana/web3.js");
const { NATIVE_MINT, TOKEN_PROGRAM_ID, getAssociatedTokenAddressSync } = require("@solana/spl-token");
const {
  Curve,
  LaunchpadConfig,
  PlatformConfig,
  Raydium,
  Token,
  TokenAmount,
  LAUNCHPAD_PROGRAM,
  TxVersion,
  getPdaLaunchpadConfigId,
  getPdaLaunchpadPoolId,
  getPdaLaunchpadVaultId,
} = require("@raydium-io/raydium-sdk-v2");

const FIXED_COMPUTE_UNIT_LIMIT = 1_000_000;
const TOKEN_DECIMALS = 6;
const PACKET_LIMIT = 1232;
const LETSBONK_PLATFORM = new PublicKey("FfYek5vEz23cMkWsdJwG2oa6EphsvXSHrGpdALN4g6W1");
const BONKERS_PLATFORM = new PublicKey("82NMHVCKwehXgbXMyzL41mvv3sdkypaMCtTxvJ4CtTzm");
const USD1_MINT = new PublicKey("USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB");
const RAYDIUM_ROUTE_PROGRAM = new PublicKey("routeUGWgWzqBWFcrCfv8tritsqukccJPu3q5GPP3xS");
const PREFERRED_USD1_ROUTE_CONFIG = "E64NGkDLLCdQ2yFNPcavaKptrEgmiQaNykUuLC1Qgwyp";

function resolveQuoteAssetConfig(asset) {
  return String(asset || "").trim().toLowerCase() === "usd1"
    ? { asset: "usd1", label: "USD1", mint: USD1_MINT, decimals: 6 }
    : { asset: "sol", label: "SOL", mint: NATIVE_MINT, decimals: 9 };
}

function envFloat(name, fallback) {
  const value = Number(process.env[name]);
  return Number.isFinite(value) ? value : fallback;
}

function envInt(name, fallback) {
  const value = Number.parseInt(process.env[name] || "", 10);
  return Number.isFinite(value) ? value : fallback;
}

function getUsd1TopupPolicy() {
  return {
    maxPriceImpactPct: envFloat("BONK_USD1_MAX_PRICE_IMPACT_PCT", 5),
    minPoolTvlUsd: envFloat("BONK_USD1_MIN_POOL_TVL_USD", 100000),
    minRemainingSol: envFloat("BONK_USD1_MIN_REMAINING_SOL", 0.02),
    maxSearchIterations: envInt("BONK_USD1_MAX_INPUT_SEARCH_ITERATIONS", 24),
  };
}

function normalizeBonkLaunchMode(mode) {
  return String(mode || "").trim().toLowerCase() === "bonkers" ? "bonkers" : "regular";
}

function resolveBonkPlatform(mode) {
  const launchMode = normalizeBonkLaunchMode(mode);
  return {
    launchMode,
    platformId: launchMode === "bonkers" ? BONKERS_PLATFORM : LETSBONK_PLATFORM,
  };
}

function trimTrailingZeroes(value) {
  return value.replace(/\.?0+$/, "");
}

function formatBn(value, decimals, precision = 6) {
  const negative = value.isNeg();
  const absolute = negative ? value.neg() : value;
  const divisor = new BN(10).pow(new BN(decimals));
  const whole = absolute.div(divisor).toString(10);
  let fraction = absolute.mod(divisor).toString(10).padStart(decimals, "0");
  fraction = fraction.slice(0, precision);
  const rendered = fraction ? `${whole}.${fraction}` : whole;
  const trimmed = trimTrailingZeroes(rendered);
  return negative && trimmed !== "0" ? `-${trimmed}` : trimmed;
}

function parseDecimalToBn(raw, decimals, label) {
  const value = String(raw || "").trim();
  if (!value) throw new Error(`${label} is required.`);
  if (!/^\d+(\.\d+)?$/.test(value)) {
    throw new Error(`Invalid ${label}: ${value}`);
  }
  const [wholePart, fractionPart = ""] = value.split(".");
  const paddedFraction = `${fractionPart}${"0".repeat(decimals)}`.slice(0, decimals);
  return new BN(wholePart, 10)
    .mul(new BN(10).pow(new BN(decimals)))
    .add(new BN(paddedFraction || "0", 10));
}

function estimateSupplyPercent(amount, supply) {
  if (supply.isZero()) return "0";
  const scaled = amount.mul(new BN(100_000_000)).div(supply);
  return trimTrailingZeroes(formatBn(scaled, 6, 4));
}

function parseSecretBytes(secret) {
  const value = String(secret || "").trim();
  if (!value) throw new Error("Wallet secret was empty.");
  if (value.startsWith("[")) {
    const parsed = JSON.parse(value);
    if (!Array.isArray(parsed)) {
      throw new Error("Wallet secret JSON must be an array of bytes.");
    }
    return Uint8Array.from(parsed);
  }
  try {
    return Uint8Array.from(bs58.decode(value));
  } catch (_error) {
    return Uint8Array.from(Buffer.from(value, "base64"));
  }
}

function parseKeypair(secret) {
  return Keypair.fromSecretKey(parseSecretBytes(secret));
}

function txVersionFromFormat(format) {
  return String(format || "").trim().toLowerCase() === "legacy"
    ? TxVersion.LEGACY
    : TxVersion.V0;
}

function readTransactionBlockhash(transaction) {
  if (transaction instanceof VersionedTransaction) {
    return transaction.message.recentBlockhash;
  }
  return transaction.recentBlockhash || "";
}

function serializeTransaction(transaction) {
  if (transaction instanceof VersionedTransaction) {
    return Buffer.from(transaction.serialize()).toString("base64");
  }
  return Buffer.from(transaction.serialize()).toString("base64");
}

function normalizeTransactions(result, { labelPrefix, computeUnitLimit, computeUnitPriceMicroLamports, inlineTipLamports, inlineTipAccount, lastValidBlockHeight }) {
  const transactions = Array.isArray(result.transactions)
    ? result.transactions
    : result.transaction
      ? [result.transaction]
      : [];
  return transactions.map((transaction, index) => {
    const label = transactions.length === 1 ? labelPrefix : `${labelPrefix}-${index + 1}`;
    return {
      label,
      format: transaction instanceof VersionedTransaction ? "v0" : "legacy",
      blockhash: readTransactionBlockhash(transaction),
      lastValidBlockHeight,
      serializedBase64: serializeTransaction(transaction),
      lookupTablesUsed: [],
      computeUnitLimit: computeUnitLimit || null,
      computeUnitPriceMicroLamports: computeUnitPriceMicroLamports || null,
      inlineTipLamports: inlineTipLamports || null,
      inlineTipAccount: inlineTipLamports && inlineTipAccount ? inlineTipAccount : null,
      serializedLength: Buffer.from(serializeTransaction(transaction), "base64").length,
    };
  });
}

async function loadLaunchDefaults(raydium, connection, ownerPubkey, mode = "regular", quoteAsset = "sol") {
  const { platformId } = resolveBonkPlatform(mode);
  const quote = resolveQuoteAssetConfig(quoteAsset);
  const configId = getPdaLaunchpadConfigId(LAUNCHPAD_PROGRAM, quote.mint, 0, 0).publicKey;
  const [configAccount, platformAccount, launchConfigs] = await Promise.all([
    connection.getAccountInfo(configId),
    connection.getAccountInfo(platformId),
    raydium.api.fetchLaunchConfigs(),
  ]);
  if (!configAccount) {
    throw new Error(`Launch config account not found: ${configId.toBase58()}`);
  }
  if (!platformAccount) {
    throw new Error(`Launch platform account not found: ${platformId.toBase58()}`);
  }
  const apiConfig = launchConfigs.find((entry) => entry.key.pubKey === configId.toBase58());
  if (!apiConfig) {
    throw new Error(`Raydium launch config defaults not found for ${configId.toBase58()}`);
  }
  const configInfo = LaunchpadConfig.decode(configAccount.data);
  const platformInfo = PlatformConfig.decode(platformAccount.data);
  const supply = new BN(apiConfig.defaultParams.supplyInit);
  const totalSellA = new BN(apiConfig.defaultParams.totalSellA);
  const totalFundRaisingB = new BN(apiConfig.defaultParams.totalFundRaisingB);
  const totalLockedAmount = new BN(0);
  const init = Curve.getCurve(configInfo.curveType).getInitParam({
    supply,
    totalFundRaising: totalFundRaisingB,
    totalSell: totalSellA,
    totalLockedAmount,
    migrateFee: configInfo.migrateFee,
  });
  const dummyMint = Keypair.generate().publicKey;
  const poolInfo = {
    epoch: new BN(0),
    bump: 0,
    status: 0,
    mintDecimalsA: TOKEN_DECIMALS,
    mintDecimalsB: quote.decimals,
    supply,
    totalSellA,
    mintA: dummyMint,
    mintB: quote.mint,
    virtualA: init.a,
    virtualB: init.b,
    realA: new BN(0),
    realB: new BN(0),
    migrateFee: configInfo.migrateFee,
    migrateType: 1,
    protocolFee: new BN(0),
    platformFee: platformInfo.feeRate,
    platformId,
    configId,
    vaultA: getPdaLaunchpadVaultId(LAUNCHPAD_PROGRAM, getPdaLaunchpadPoolId(LAUNCHPAD_PROGRAM, dummyMint, quote.mint).publicKey, dummyMint).publicKey,
    vaultB: getPdaLaunchpadVaultId(LAUNCHPAD_PROGRAM, getPdaLaunchpadPoolId(LAUNCHPAD_PROGRAM, dummyMint, quote.mint).publicKey, quote.mint).publicKey,
    creator: ownerPubkey || PublicKey.default,
    totalFundRaisingB,
    vestingSchedule: {
      totalLockedAmount,
      cliffPeriod: new BN(0),
      unlockPeriod: new BN(0),
      startTime: new BN(0),
      totalAllocatedShare: new BN(0),
    },
    mintProgramFlag: 0,
    cpmmCreatorFeeOn: 0,
    platformVestingShare: platformInfo.platformVestingScale || new BN(0),
    configInfo,
    quoteAsset: quote.asset,
    quoteAssetLabel: quote.label,
    quoteMint: quote.mint,
    quoteDecimals: quote.decimals,
  };
  return {
    configId,
    configInfo,
    platformInfo,
    platformId,
    poolInfo,
    supply,
    quoteAsset: quote.asset,
    quoteAssetLabel: quote.label,
    quoteMint: quote.mint,
    quoteDecimals: quote.decimals,
  };
}

function buildPrelaunchPoolInfo(defaults, mint, creator) {
  const poolId = getPdaLaunchpadPoolId(LAUNCHPAD_PROGRAM, mint, defaults.quoteMint).publicKey;
  return {
    ...defaults.poolInfo,
    mintA: mint,
    vaultA: getPdaLaunchpadVaultId(LAUNCHPAD_PROGRAM, poolId, mint).publicKey,
    vaultB: getPdaLaunchpadVaultId(LAUNCHPAD_PROGRAM, poolId, defaults.quoteMint).publicKey,
    creator,
  };
}

function buildQuote(defaults, mode, amount) {
  const common = {
    poolInfo: defaults.poolInfo,
    protocolFeeRate: defaults.configInfo.tradeFeeRate,
    platformFeeRate: defaults.platformInfo.feeRate,
    curveType: defaults.configInfo.curveType,
    shareFeeRate: new BN(0),
    creatorFeeRate: defaults.platformInfo.creatorFeeRate,
    transferFeeConfigA: undefined,
    slot: 0,
  };
  if (mode === "tokens") {
    const tokenAmount = parseDecimalToBn(amount, TOKEN_DECIMALS, "buy amount");
    const quote = Curve.buyExactOut({
      ...common,
      amountA: tokenAmount,
    });
    return {
      estimatedTokens: formatBn(tokenAmount, TOKEN_DECIMALS, 6),
      estimatedSol: formatBn(quote.amountB, defaults.quoteDecimals, 6),
      estimatedQuoteAmount: formatBn(quote.amountB, defaults.quoteDecimals, 6),
      quoteAsset: defaults.quoteAsset,
      quoteAssetLabel: defaults.quoteAssetLabel,
      estimatedSupplyPercent: estimateSupplyPercent(tokenAmount, defaults.supply),
    };
  }
  const buyAmount = parseDecimalToBn(amount, defaults.quoteDecimals, `buy amount ${defaults.quoteAssetLabel}`);
  const quote = Curve.buyExactIn({
    ...common,
    amountB: buyAmount,
  });
  return {
    estimatedTokens: formatBn(quote.amountA.amount, TOKEN_DECIMALS, 6),
    estimatedSol: formatBn(buyAmount, defaults.quoteDecimals, 6),
    estimatedQuoteAmount: formatBn(buyAmount, defaults.quoteDecimals, 6),
    quoteAsset: defaults.quoteAsset,
    quoteAssetLabel: defaults.quoteAssetLabel,
    estimatedSupplyPercent: estimateSupplyPercent(quote.amountA.amount, defaults.supply),
  };
}

function buildComputeBudgetConfig(input) {
  if (!input || !input.computeUnitLimit) return undefined;
  return {
    units: Number(input.computeUnitLimit),
    microLamports: Number(input.computeUnitPriceMicroLamports || 0),
  };
}

function buildTipConfig(input) {
  if (!input || !input.tipLamports || !input.tipAccount) return undefined;
  return {
    address: input.tipAccount,
    amount: new BN(String(input.tipLamports)),
  };
}

function minBn(left, right) {
  return left.lte(right) ? left : right;
}

function buildMinAmountFromBps(amount, slippageBps) {
  const safeBps = Math.max(0, Math.min(10_000, Number(slippageBps || 0)));
  return amount.mul(new BN(10_000 - safeBps)).div(new BN(10_000));
}

async function fetchWalletTokenBalance(connection, owner, mint) {
  const ata = getAssociatedTokenAddressSync(mint, owner, false, TOKEN_PROGRAM_ID);
  try {
    const balance = await connection.getTokenAccountBalance(ata, "processed");
    return new BN(balance.value.amount || "0");
  } catch (_error) {
    return new BN(0);
  }
}

function selectUsd1RouteCandidates(pools) {
  return pools
    .filter((pool) => pool && pool.mintA && pool.mintB)
    .filter((pool) => {
      const mintA = pool.mintA.address || pool.mintA;
      const mintB = pool.mintB.address || pool.mintB;
      return [mintA, mintB].includes(NATIVE_MINT.toBase58()) && [mintA, mintB].includes(USD1_MINT.toBase58());
    })
    .filter((pool) => pool.type === "Concentrated" || pool.type === "Standard")
    .sort((left, right) => {
      const leftPreferred = left.config && left.config.id === PREFERRED_USD1_ROUTE_CONFIG ? 1 : 0;
      const rightPreferred = right.config && right.config.id === PREFERRED_USD1_ROUTE_CONFIG ? 1 : 0;
      if (leftPreferred !== rightPreferred) return rightPreferred - leftPreferred;
      return Number(right.tvl || 0) - Number(left.tvl || 0);
    });
}

function toBasicPoolInfo(pool) {
  const version = pool.type === "Concentrated" ? 6 : pool.type === "Standard" ? 4 : 7;
  return {
    id: new PublicKey(pool.id),
    version,
    mintA: new PublicKey(pool.mintA.address || pool.mintA),
    mintB: new PublicKey(pool.mintB.address || pool.mintB),
  };
}

async function computeDirectRouteSwap(raydium, connection, pool, inputAmountBn, slippageBps) {
  const inputMint = NATIVE_MINT;
  const outputMint = USD1_MINT;
  const basicPool = toBasicPoolInfo(pool);
  const routes = raydium.tradeV2.getAllRoute({
    inputMint,
    outputMint,
    clmmPools: basicPool.version === 6 ? [basicPool] : [],
    ammPools: basicPool.version === 4 ? [basicPool] : [],
    cpmmPools: basicPool.version === 7 ? [basicPool] : [],
  });
  const routeData = await raydium.tradeV2.fetchSwapRoutesData({
    routes,
    inputMint,
    outputMint,
  });
  const inputTokenInfo = routeData.mintInfos[inputMint.toBase58()];
  const outputTokenInfo = routeData.mintInfos[outputMint.toBase58()];
  const inputTokenAmount = new TokenAmount(
    new Token({
      mint: inputMint,
      decimals: inputTokenInfo.decimals,
      symbol: inputTokenInfo.symbol,
      name: inputTokenInfo.name,
    }),
    inputAmountBn.toString(10),
    true,
  );
  const directPath = routes.directPath
    .map((entry) =>
      routeData.computeClmmPoolInfo[entry.id.toBase58()]
      || routeData.ammSimulateCache[entry.id.toBase58()]
      || routeData.computeCpmmData[entry.id.toBase58()])
    .filter(Boolean);
  const swapCandidates = raydium.tradeV2.getAllRouteComputeAmountOut({
    directPath,
    routePathDict: routeData.routePathDict,
    simulateCache: {
      ...routeData.ammSimulateCache,
      ...routeData.computeClmmPoolInfo,
      ...routeData.computeCpmmData,
    },
    tickCache: routeData.computePoolTickData,
    mintInfos: routeData.mintInfos,
    inputTokenAmount,
    outputToken: outputTokenInfo,
    slippage: Number(slippageBps || 0) / 100,
    chainTime: Math.floor(Date.now() / 1000),
    epochInfo: await connection.getEpochInfo(),
  });
  if (!swapCandidates.length) {
    throw new Error(`No Raydium route quote found for pool ${pool.id}.`);
  }
  const swapInfo = swapCandidates[0];
  const swapPoolKeys = await raydium.api.fetchPoolKeysById({ idList: [pool.id] });
  if (!swapPoolKeys.length) {
    throw new Error(`Raydium pool keys not found for ${pool.id}.`);
  }
  return {
    swapInfo,
    swapPoolKeys,
    expectedOut: new BN(swapInfo.amountOut.amount.raw.toString()),
    minOut: new BN(swapInfo.minAmountOut.amount.raw.toString()),
    priceImpactPct: Number(swapInfo.priceImpact.toString()) * 100,
  };
}

async function buildUsd1Topup(request) {
  if (resolveQuoteAssetConfig(request.quoteAsset).asset !== "usd1") {
    return { compiledTransaction: null };
  }
  const owner = parseKeypair(request.ownerSecret);
  const connection = new Connection(request.rpcUrl, request.commitment || "confirmed");
  const raydium = await Raydium.load({
    connection,
    owner,
    disableLoadToken: true,
    disableFeatureCheck: true,
  });
  const policy = getUsd1TopupPolicy();
  const requiredQuoteAmount = parseDecimalToBn(request.requiredQuoteAmount, 6, "required USD1 amount");
  if (requiredQuoteAmount.lte(new BN(0))) {
    return { compiledTransaction: null };
  }
  const currentUsd1Balance = await fetchWalletTokenBalance(connection, owner.publicKey, USD1_MINT);
  if (currentUsd1Balance.gte(requiredQuoteAmount)) {
    return {
      compiledTransaction: null,
      requiredQuoteAmount: formatBn(requiredQuoteAmount, 6, 6),
      currentQuoteAmount: formatBn(currentUsd1Balance, 6, 6),
      shortfallQuoteAmount: "0",
    };
  }
  const shortfall = requiredQuoteAmount.sub(currentUsd1Balance);
  const balanceLamports = await connection.getBalance(owner.publicKey, "processed");
  const minRemainingLamports = parseDecimalToBn(String(policy.minRemainingSol), 9, "minimum remaining SOL");
  const maxSpendableLamports = new BN(String(balanceLamports)).sub(minRemainingLamports);
  if (maxSpendableLamports.lte(new BN(0))) {
    throw new Error(`Insufficient SOL headroom for USD1 top-up. Need at least ${policy.minRemainingSol} SOL reserved after swap.`);
  }

  const poolPage = await raydium.api.fetchPoolByMints({
    mint1: NATIVE_MINT,
    mint2: USD1_MINT,
  });
  const candidates = selectUsd1RouteCandidates(poolPage.data || []);
  let selected = null;
  for (const pool of candidates) {
    if (Number(pool.tvl || 0) < policy.minPoolTvlUsd) continue;
    const referencePrice = Number(pool.price || 0);
    if (!Number.isFinite(referencePrice) || referencePrice <= 0) continue;
    let low = new BN(1);
    let high = parseDecimalToBn(String(shortfall.toNumber() / 1_000_000 / referencePrice * 1.1 || 0.01), 9, "top-up search guess");
    if (high.lte(new BN(0))) high = parseDecimalToBn("0.01", 9, "top-up search floor");
    if (high.gt(maxSpendableLamports)) high = maxSpendableLamports.clone();
    let quote = await computeDirectRouteSwap(raydium, connection, pool, high, request.slippageBps);
    while (quote.minOut.lt(shortfall) && high.lt(maxSpendableLamports)) {
      low = high.add(new BN(1));
      high = minBn(high.mul(new BN(2)), maxSpendableLamports);
      quote = await computeDirectRouteSwap(raydium, connection, pool, high, request.slippageBps);
      if (high.eq(maxSpendableLamports)) break;
    }
    if (quote.minOut.lt(shortfall)) continue;
    for (let index = 0; index < policy.maxSearchIterations && low.lt(high); index += 1) {
      const mid = low.add(high).div(new BN(2));
      const midQuote = await computeDirectRouteSwap(raydium, connection, pool, mid, request.slippageBps);
      if (midQuote.minOut.gte(shortfall)) {
        high = mid;
        quote = midQuote;
      } else {
        low = mid.add(new BN(1));
      }
    }
    if (quote.priceImpactPct > policy.maxPriceImpactPct) continue;
    selected = { pool, inputAmount: high, quote };
    break;
  }
  if (!selected) {
    throw new Error("No safe Raydium SOL -> USD1 route satisfied the configured liquidity and price impact thresholds.");
  }
  const swapResult = await raydium.tradeV2.swap({
    txVersion: txVersionFromFormat(request.txFormat),
    swapInfo: selected.quote.swapInfo,
    swapPoolKeys: selected.quote.swapPoolKeys,
    ownerInfo: {
      associatedOnly: false,
      checkCreateATAOwner: true,
    },
    routeProgram: RAYDIUM_ROUTE_PROGRAM,
    computeBudgetConfig: buildComputeBudgetConfig(request.txConfig),
    feePayer: owner.publicKey,
  });
  const { lastValidBlockHeight } = await connection.getLatestBlockhash(request.commitment || "confirmed");
  return {
    compiledTransaction: normalizeTransactions(swapResult, {
      labelPrefix: request.labelPrefix || "usd1-topup",
      computeUnitLimit: request.txConfig && request.txConfig.computeUnitLimit,
      computeUnitPriceMicroLamports: request.txConfig && request.txConfig.computeUnitPriceMicroLamports,
      inlineTipLamports: request.txConfig && request.txConfig.tipLamports,
      inlineTipAccount: request.txConfig && request.txConfig.tipAccount,
      lastValidBlockHeight,
    })[0],
    requiredQuoteAmount: formatBn(requiredQuoteAmount, 6, 6),
    currentQuoteAmount: formatBn(currentUsd1Balance, 6, 6),
    shortfallQuoteAmount: formatBn(shortfall, 6, 6),
    inputSol: formatBn(selected.inputAmount, 9, 6),
    expectedQuoteOut: formatBn(selected.quote.expectedOut, 6, 6),
    minQuoteOut: formatBn(selected.quote.minOut, 6, 6),
    priceImpactPct: String(selected.quote.priceImpactPct),
    routePoolId: selected.pool.id,
    routeConfigId: selected.pool.config && selected.pool.config.id ? selected.pool.config.id : "",
    routePoolType: selected.pool.type,
    routePoolTvlUsd: String(selected.pool.tvl || 0),
  };
}

async function buildLaunch(request) {
  const owner = parseKeypair(request.ownerSecret);
  const connection = new Connection(request.rpcUrl, request.commitment || "confirmed");
  const raydium = await Raydium.load({
    connection,
    owner,
    disableLoadToken: true,
    disableFeatureCheck: true,
  });
  const defaults = await loadLaunchDefaults(
    raydium,
    connection,
    owner.publicKey,
    request.mode,
    request.quoteAsset,
  );
  const mintKeypair = request.vanitySecret
    ? parseKeypair(request.vanitySecret)
    : Keypair.generate();
  const txVersion = txVersionFromFormat(request.txFormat);
  let buyAmount;
  let minMintAAmount;
  let createOnly = true;
  if (request.devBuy && request.devBuy.mode && request.devBuy.amount) {
    createOnly = false;
    if (request.devBuy.mode === "tokens") {
      const quote = buildQuote(defaults, "tokens", request.devBuy.amount);
      const tokenAmount = parseDecimalToBn(request.devBuy.amount, TOKEN_DECIMALS, "dev buy tokens");
      buyAmount = parseDecimalToBn(
        quote.estimatedQuoteAmount || quote.estimatedSol,
        defaults.quoteDecimals,
        `dev buy ${defaults.quoteAssetLabel}`,
      );
      minMintAAmount = buildMinAmountFromBps(tokenAmount, request.slippageBps);
    } else {
      buyAmount = parseDecimalToBn(
        request.devBuy.amount,
        defaults.quoteDecimals,
        `dev buy ${defaults.quoteAssetLabel}`,
      );
    }
  }
  const buildResult = await raydium.launchpad.createLaunchpad({
    programId: LAUNCHPAD_PROGRAM,
    platformId: defaults.platformId,
    configId: defaults.configId,
    mintA: mintKeypair.publicKey,
    decimals: TOKEN_DECIMALS,
    name: request.token.name,
    symbol: request.token.symbol,
    uri: request.token.uri,
    migrateType: "cpmm",
    createOnly,
    buyAmount,
    minMintAAmount,
    slippage: new BN(String(request.slippageBps || 0)),
    txVersion,
    extraSigners: [mintKeypair],
    computeBudgetConfig: buildComputeBudgetConfig(request.txConfig),
    txTipConfig: buildTipConfig(request.txConfig),
  });
  const { lastValidBlockHeight } = await connection.getLatestBlockhash(request.commitment || "confirmed");
  return {
    mint: mintKeypair.publicKey.toBase58(),
    launchCreator: owner.publicKey.toBase58(),
    compiledTransactions: normalizeTransactions(buildResult, {
      labelPrefix: "launch",
      computeUnitLimit: request.txConfig && request.txConfig.computeUnitLimit,
      computeUnitPriceMicroLamports: request.txConfig && request.txConfig.computeUnitPriceMicroLamports,
      inlineTipLamports: request.txConfig && request.txConfig.tipLamports,
      inlineTipAccount: request.txConfig && request.txConfig.tipAccount,
      lastValidBlockHeight,
    }),
  };
}

async function compileFollowBuy(request, labelPrefix, atomic = false) {
  const owner = parseKeypair(request.ownerSecret);
  const connection = new Connection(request.rpcUrl, request.commitment || "confirmed");
  const raydium = await Raydium.load({
    connection,
    owner,
    disableLoadToken: true,
    disableFeatureCheck: true,
  });
  const mint = new PublicKey(request.mint);
  const quote = resolveQuoteAssetConfig(request.quoteAsset);
  const options = {
    programId: LAUNCHPAD_PROGRAM,
    mintA: mint,
    buyAmount: parseDecimalToBn(request.buyAmountSol, quote.decimals, `follow buy amount ${quote.label}`),
    slippage: new BN(String(request.slippageBps || 0)),
    txVersion: txVersionFromFormat(request.txFormat),
    computeBudgetConfig: buildComputeBudgetConfig(request.txConfig),
    txTipConfig: buildTipConfig(request.txConfig),
  };
  if (atomic) {
    const defaults = await loadLaunchDefaults(
      raydium,
      connection,
      request.launchCreator ? new PublicKey(request.launchCreator) : owner.publicKey,
      request.mode,
      request.quoteAsset,
    );
    const creator = request.launchCreator ? new PublicKey(request.launchCreator) : owner.publicKey;
    Object.assign(options, {
      poolInfo: buildPrelaunchPoolInfo(defaults, mint, creator),
      configInfo: defaults.configInfo,
      platformFeeRate: defaults.platformInfo.feeRate,
      mintAProgram: TOKEN_PROGRAM_ID,
      skipCheckMintA: true,
    });
  }
  const buildResult = await raydium.launchpad.buyToken({
    ...options,
  });
  const { lastValidBlockHeight } = await connection.getLatestBlockhash(request.commitment || "confirmed");
  return {
    compiledTransaction: normalizeTransactions(buildResult, {
      labelPrefix,
      computeUnitLimit: request.txConfig && request.txConfig.computeUnitLimit,
      computeUnitPriceMicroLamports: request.txConfig && request.txConfig.computeUnitPriceMicroLamports,
      inlineTipLamports: request.txConfig && request.txConfig.tipLamports,
      inlineTipAccount: request.txConfig && request.txConfig.tipAccount,
      lastValidBlockHeight,
    })[0],
  };
}

async function compileFollowSell(request) {
  const owner = parseKeypair(request.ownerSecret);
  const connection = new Connection(request.rpcUrl, request.commitment || "confirmed");
  const raydium = await Raydium.load({
    connection,
    owner,
    disableLoadToken: true,
    disableFeatureCheck: true,
  });
  const mint = new PublicKey(request.mint);
  const tokenAccount = getAssociatedTokenAddressSync(mint, owner.publicKey, false, TOKEN_PROGRAM_ID);
  let balanceInfo;
  try {
    balanceInfo = await connection.getTokenAccountBalance(tokenAccount, request.commitment || "processed");
  } catch (_error) {
    return { compiledTransaction: null };
  }
  const rawAmount = new BN(balanceInfo.value.amount || "0");
  if (rawAmount.isZero()) {
    return { compiledTransaction: null };
  }
  const sellAmount = rawAmount.mul(new BN(Number(request.sellPercent || 0))).div(new BN(100));
  if (sellAmount.isZero()) {
    return { compiledTransaction: null };
  }
  const buildResult = await raydium.launchpad.sellToken({
    programId: LAUNCHPAD_PROGRAM,
    mintA: mint,
    sellAmount,
    slippage: new BN(String(request.slippageBps || 0)),
    txVersion: txVersionFromFormat(request.txFormat),
    computeBudgetConfig: buildComputeBudgetConfig(request.txConfig),
    txTipConfig: buildTipConfig(request.txConfig),
  });
  const { lastValidBlockHeight } = await connection.getLatestBlockhash(request.commitment || "confirmed");
  return {
    compiledTransaction: normalizeTransactions(buildResult, {
      labelPrefix: "follow-sell",
      computeUnitLimit: request.txConfig && request.txConfig.computeUnitLimit,
      computeUnitPriceMicroLamports: request.txConfig && request.txConfig.computeUnitPriceMicroLamports,
      inlineTipLamports: request.txConfig && request.txConfig.tipLamports,
      inlineTipAccount: request.txConfig && request.txConfig.tipAccount,
      lastValidBlockHeight,
    })[0],
  };
}

async function fetchMarketSnapshot(request) {
  const connection = new Connection(request.rpcUrl, request.commitment || "processed");
  const raydium = await Raydium.load({
    connection,
    owner: null,
    disableLoadToken: true,
    disableFeatureCheck: true,
  });
  const mint = new PublicKey(request.mint);
  const quote = resolveQuoteAssetConfig(request.quoteAsset);
  const poolId = getPdaLaunchpadPoolId(LAUNCHPAD_PROGRAM, mint, quote.mint).publicKey;
  const poolInfo = await raydium.launchpad.getRpcPoolInfo({ poolId });
  const supply = new BN(poolInfo.supply.toString());
  const virtualA = new BN(poolInfo.virtualA.toString());
  const virtualB = new BN(poolInfo.virtualB.toString());
  const realA = new BN(poolInfo.realA.toString());
  const realB = new BN(poolInfo.realB.toString());
  const totalSellA = new BN(poolInfo.totalSellA.toString());
  const marketCapLamports = virtualA.isZero() ? new BN(0) : supply.mul(virtualB).div(virtualA);
  return {
    mint: mint.toBase58(),
    quoteAsset: quote.asset,
    quoteAssetLabel: quote.label,
    creator: poolInfo.creator.toBase58 ? poolInfo.creator.toBase58() : String(poolInfo.creator),
    virtualTokenReserves: virtualA.toString(10),
    virtualSolReserves: virtualB.toString(10),
    realTokenReserves: totalSellA.sub(realA).toString(10),
    realSolReserves: realB.toString(10),
    tokenTotalSupply: supply.toString(10),
    complete: Number(poolInfo.status || 0) !== 0,
    marketCapLamports: marketCapLamports.toString(10),
    marketCapSol: formatBn(marketCapLamports, quote.decimals, 6),
  };
}

async function readRequest() {
  const chunks = [];
  for await (const chunk of process.stdin) {
    chunks.push(chunk);
  }
  const raw = Buffer.concat(chunks).toString("utf8").trim();
  return raw ? JSON.parse(raw) : {};
}

async function main() {
  const request = await readRequest();
  let response;
  switch (request.action) {
    case "quote":
      response = buildQuote(
        await loadLaunchDefaults(
          await Raydium.load({
            connection: new Connection(request.rpcUrl, request.commitment || "confirmed"),
            owner: null,
            disableLoadToken: true,
            disableFeatureCheck: true,
          }),
          new Connection(request.rpcUrl, request.commitment || "confirmed"),
          null,
          request.mode,
          request.quoteAsset,
        ),
        request.mode,
        request.amount,
      );
      break;
    case "build-launch":
      response = await buildLaunch(request);
      break;
    case "compile-follow-buy":
      response = await compileFollowBuy(request, "follow-buy", false);
      break;
    case "compile-follow-buy-atomic":
      response = await compileFollowBuy(request, "follow-buy-atomic", true);
      break;
    case "compile-sol-to-usd1-topup":
      response = await buildUsd1Topup(request);
      break;
    case "compile-follow-sell":
      response = await compileFollowSell(request);
      break;
    case "fetch-market-snapshot":
      response = await fetchMarketSnapshot(request);
      break;
    default:
      throw new Error(`Unsupported bonk helper action: ${request.action || "(missing)"}`);
  }
  process.stdout.write(JSON.stringify(response));
}

main().catch((error) => {
  process.stderr.write(`${error && error.stack ? error.stack : String(error)}\n`);
  process.exit(1);
});
