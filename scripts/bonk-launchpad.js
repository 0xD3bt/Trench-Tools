"use strict";

require("dotenv").config({ quiet: true });

const bs58 = require("bs58");
const BN = require("bn.js");
const {
  ComputeBudgetProgram,
  Connection,
  Keypair,
  PublicKey,
  SystemInstruction,
  SystemProgram,
  Transaction,
  TransactionMessage,
  VersionedTransaction,
} = require("@solana/web3.js");
const {
  NATIVE_MINT,
  TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountIdempotentInstruction,
  getAssociatedTokenAddressSync,
} = require("@solana/spl-token");
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
const PINNED_USD1_ROUTE_POOL_ID = "AQAGYQsdU853WAKhXM79CgNdoyhrRwXvYHX6qrDyC1FS";
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

function extractTransactions(result) {
  return Array.isArray(result && result.transactions)
    ? result.transactions
    : result && result.transaction
      ? [result.transaction]
      : [];
}

function normalizeTransactions(result, { labelPrefix, computeUnitLimit, computeUnitPriceMicroLamports, inlineTipLamports, inlineTipAccount, lastValidBlockHeight }) {
  const transactions = extractTransactions(result);
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

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitForWalletTokenAccountVisibility(raydium, owner, mint, ata, commitment) {
  if (!raydium || !raydium.account || typeof raydium.account.fetchWalletTokenAccounts !== "function") {
    return false;
  }
  for (let attempt = 0; attempt < 6; attempt += 1) {
    const refreshed = await raydium.account.fetchWalletTokenAccounts({ forceUpdate: true, commitment });
    const visible = (refreshed.tokenAccountRawInfos || []).some((entry) => (
      entry.pubkey.equals(ata) || entry.accountInfo.mint.equals(mint)
    ));
    if (visible) {
      return true;
    }
    if (attempt < 5) {
      await sleep(400 * (attempt + 1));
    }
  }
  return false;
}

async function ensureAssociatedTokenAccountExists(connection, owner, mint, request, raydium) {
  const commitment = request.commitment || "confirmed";
  const mintInfo = await connection.getAccountInfo(mint, commitment);
  if (!mintInfo) {
    throw new Error(`Token mint account not found: ${mint.toBase58()}`);
  }
  const tokenProgramId = mintInfo.owner;
  const ata = getAssociatedTokenAddressSync(mint, owner.publicKey, false, tokenProgramId);
  const existingAta = await connection.getAccountInfo(ata, commitment);
  if (existingAta) {
    const visible = await waitForWalletTokenAccountVisibility(
      raydium,
      owner.publicKey,
      mint,
      ata,
      commitment,
    );
    if (!visible) {
      throw new Error(`Associated token account exists on-chain but is not yet visible to Raydium: ${ata.toBase58()}`);
    }
    return ata;
  }
  const transaction = new Transaction();
  transaction.feePayer = owner.publicKey;
  if (request.txConfig && request.txConfig.computeUnitPriceMicroLamports) {
    transaction.add(
      ComputeBudgetProgram.setComputeUnitPrice({
        microLamports: Number(request.txConfig.computeUnitPriceMicroLamports),
      }),
    );
  }
  if (request.txConfig && request.txConfig.computeUnitLimit) {
    transaction.add(
      ComputeBudgetProgram.setComputeUnitLimit({
        units: Number(request.txConfig.computeUnitLimit),
      }),
    );
  }
  transaction.add(
    createAssociatedTokenAccountIdempotentInstruction(
      owner.publicKey,
      ata,
      owner.publicKey,
      mint,
      tokenProgramId,
    ),
  );
  const tipInstruction = buildInlineTipInstruction(
    owner.publicKey,
    request.txConfig && request.txConfig.tipAccount,
    request.txConfig && request.txConfig.tipLamports,
  );
  if (tipInstruction) {
    transaction.add(tipInstruction);
  }
  const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash(commitment);
  transaction.recentBlockhash = blockhash;
  transaction.sign(owner);
  const signature = await connection.sendRawTransaction(transaction.serialize(), {
    preflightCommitment: commitment,
  });
  const confirmation = await connection.confirmTransaction(
    { signature, blockhash, lastValidBlockHeight },
    commitment,
  );
  if (confirmation && confirmation.value && confirmation.value.err) {
    throw new Error(`USD1 ATA creation failed: ${JSON.stringify(confirmation.value.err)}`);
  }
  const visible = await waitForWalletTokenAccountVisibility(
    raydium,
    owner.publicKey,
    mint,
    ata,
    commitment,
  );
  if (!visible) {
    throw new Error(`Created associated token account is not yet visible to Raydium: ${ata.toBase58()}`);
  }
  return ata;
}

function allowAtaCreation(request) {
  return Boolean(request && request.allowAtaCreation);
}

async function resolveLookupTableAccounts(connection, transaction) {
  if (!(transaction instanceof VersionedTransaction)) {
    return [];
  }
  const lookups = transaction.message.addressTableLookups || [];
  const resolved = await Promise.all(lookups.map(async (lookup) => {
    const response = await connection.getAddressLookupTable(lookup.accountKey);
    if (!response || !response.value) {
      throw new Error(`Address lookup table not found: ${lookup.accountKey.toBase58()}`);
    }
    return response.value;
  }));
  return resolved;
}

async function decompileTransactionInstructions(connection, transaction) {
  if (transaction instanceof VersionedTransaction) {
    const addressLookupTableAccounts = await resolveLookupTableAccounts(connection, transaction);
    const message = TransactionMessage.decompile(transaction.message, { addressLookupTableAccounts });
    return {
      instructions: message.instructions,
      addressLookupTableAccounts,
    };
  }
  return {
    instructions: transaction.instructions || [],
    addressLookupTableAccounts: [],
  };
}

function mergeLookupTableAccounts(...lists) {
  const merged = new Map();
  for (const list of lists) {
    for (const account of list || []) {
      merged.set(account.key.toBase58(), account);
    }
  }
  return Array.from(merged.values());
}

function isComputeBudgetInstruction(instruction) {
  return instruction.programId && instruction.programId.equals(ComputeBudgetProgram.programId);
}

function isInlineTipInstruction(instruction, ownerPubkey, tipAccount, tipLamports) {
  if (!tipAccount || !tipLamports) return false;
  if (!instruction.programId || !instruction.programId.equals(SystemProgram.programId)) {
    return false;
  }
  try {
    if (SystemInstruction.decodeInstructionType(instruction) !== "Transfer") {
      return false;
    }
    const decoded = SystemInstruction.decodeTransfer(instruction);
    return decoded.fromPubkey.equals(ownerPubkey)
      && decoded.toPubkey.equals(new PublicKey(tipAccount))
      && Number(decoded.lamports) === Number(tipLamports);
  } catch (_error) {
    return false;
  }
}

function buildInlineTipInstruction(ownerPubkey, tipAccount, tipLamports) {
  if (!tipAccount || !tipLamports) return null;
  return SystemProgram.transfer({
    fromPubkey: ownerPubkey,
    toPubkey: new PublicKey(tipAccount),
    lamports: Number(tipLamports),
  });
}

function isAtomicMessageOverflowError(error) {
  const message = error && error.message ? error.message : String(error || "");
  return message.includes("encoding overruns Uint8Array")
    || message.includes("Transaction too large")
    || message.includes("encoding overruns");
}

async function ensureInlineTipOnTransaction(connection, owner, transaction, txConfig) {
  const tipInstruction = buildInlineTipInstruction(
    owner.publicKey,
    txConfig && txConfig.tipAccount,
    txConfig && txConfig.tipLamports,
  );
  if (!tipInstruction) {
    return transaction;
  }
  if (transaction instanceof VersionedTransaction) {
    const { instructions, addressLookupTableAccounts } = await decompileTransactionInstructions(connection, transaction);
    if (instructions.some((instruction) => (
      isInlineTipInstruction(
        instruction,
        owner.publicKey,
        txConfig && txConfig.tipAccount,
        txConfig && txConfig.tipLamports,
      )
    ))) {
      return transaction;
    }
    const rebuilt = new VersionedTransaction(
      new TransactionMessage({
        payerKey: owner.publicKey,
        recentBlockhash: readTransactionBlockhash(transaction),
        instructions: [...instructions, tipInstruction],
      }).compileToV0Message(addressLookupTableAccounts),
    );
    rebuilt.sign([owner]);
    return rebuilt;
  }
  const instructions = transaction.instructions || [];
  if (instructions.some((instruction) => (
    isInlineTipInstruction(
      instruction,
      owner.publicKey,
      txConfig && txConfig.tipAccount,
      txConfig && txConfig.tipLamports,
    )
  ))) {
    return transaction;
  }
  const rebuilt = new Transaction();
  rebuilt.feePayer = owner.publicKey;
  rebuilt.recentBlockhash = readTransactionBlockhash(transaction);
  instructions.forEach((instruction) => rebuilt.add(instruction));
  rebuilt.add(tipInstruction);
  rebuilt.sign(owner);
  return rebuilt;
}

async function ensureInlineTipOnSwapResult(connection, owner, result, txConfig) {
  const transactions = extractTransactions(result);
  if (!transactions.length || !txConfig || !txConfig.tipLamports || !txConfig.tipAccount) {
    return result;
  }
  const rebuiltTransactions = [];
  for (const transaction of transactions) {
    rebuiltTransactions.push(await ensureInlineTipOnTransaction(connection, owner, transaction, txConfig));
  }
  return rebuiltTransactions.length === 1
    ? { transaction: rebuiltTransactions[0] }
    : { transactions: rebuiltTransactions };
}

async function combineAtomicUsd1ActionTransaction(connection, owner, request, swapTransaction, actionTransaction, extraSigners = []) {
  const [swapBundle, actionBundle] = await Promise.all([
    decompileTransactionInstructions(connection, swapTransaction),
    decompileTransactionInstructions(connection, actionTransaction),
  ]);
  const actionInstructions = actionBundle.instructions.filter((instruction) => (
    !isComputeBudgetInstruction(instruction)
    && !isInlineTipInstruction(
      instruction,
      owner.publicKey,
      request.txConfig && request.txConfig.tipAccount,
      request.txConfig && request.txConfig.tipLamports,
    )
  ));
  const instructions = [...swapBundle.instructions, ...actionInstructions];
  const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash(request.commitment || "confirmed");
  const txVersion = txVersionFromFormat(request.txFormat);
  if (txVersion === TxVersion.LEGACY) {
    const transaction = new Transaction();
    transaction.feePayer = owner.publicKey;
    transaction.recentBlockhash = blockhash;
    instructions.forEach((instruction) => transaction.add(instruction));
    transaction.sign(owner, ...extraSigners);
    return { transaction, lastValidBlockHeight };
  }
  const lookupTables = mergeLookupTableAccounts(
    swapBundle.addressLookupTableAccounts,
    actionBundle.addressLookupTableAccounts,
  );
  const message = new TransactionMessage({
    payerKey: owner.publicKey,
    recentBlockhash: blockhash,
    instructions,
  }).compileToV0Message(lookupTables);
  const transaction = new VersionedTransaction(message);
  transaction.sign([owner, ...extraSigners]);
  return { transaction, lastValidBlockHeight };
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

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function loadLivePoolContext(raydium, connection, mint, quoteAsset) {
  const requestedQuote = resolveQuoteAssetConfig(quoteAsset);
  const candidateAssets = requestedQuote.asset === "usd1"
    ? [requestedQuote, resolveQuoteAssetConfig("sol")]
    : [requestedQuote, resolveQuoteAssetConfig("usd1")];
  const errors = [];
  for (const quote of candidateAssets) {
    const poolId = getPdaLaunchpadPoolId(LAUNCHPAD_PROGRAM, mint, quote.mint).publicKey;
    for (let attempt = 0; attempt < 6; attempt += 1) {
      try {
        const poolInfo = await raydium.launchpad.getRpcPoolInfo({ poolId });
        const configId = poolInfo.configId && poolInfo.configId.toBase58
          ? poolInfo.configId
          : new PublicKey(String(poolInfo.configId || ""));
        const platformId = poolInfo.platformId && poolInfo.platformId.toBase58
          ? poolInfo.platformId
          : new PublicKey(String(poolInfo.platformId || ""));
        const [configAccount, platformAccount] = await Promise.all([
          connection.getAccountInfo(configId),
          connection.getAccountInfo(platformId),
        ]);
        if (!configAccount) {
          throw new Error(`Launch config account not found: ${configId.toBase58()}`);
        }
        if (!platformAccount) {
          throw new Error(`Launch platform account not found: ${platformId.toBase58()}`);
        }
        return {
          poolId,
          poolInfo,
          configId,
          platformId,
          configInfo: LaunchpadConfig.decode(configAccount.data),
          platformInfo: PlatformConfig.decode(platformAccount.data),
          quoteAsset: quote.asset,
          quoteAssetLabel: quote.label,
          quoteMint: quote.mint,
          quoteDecimals: quote.decimals,
        };
      } catch (error) {
        errors.push(`${quote.asset}:${poolId.toBase58()}: ${error && error.message ? error.message : String(error)}`);
        if (attempt < 5) {
          await sleep(200);
        }
      }
    }
  }
  throw new Error(`Unable to resolve Bonk live pool context. Attempts: ${errors.join(" | ")}`);
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
      mode,
      input: amount,
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
    mode,
    input: amount,
    estimatedTokens: formatBn(quote.amountA.amount, TOKEN_DECIMALS, 6),
    estimatedSol: formatBn(buyAmount, defaults.quoteDecimals, 6),
    estimatedQuoteAmount: formatBn(buyAmount, defaults.quoteDecimals, 6),
    quoteAsset: defaults.quoteAsset,
    quoteAssetLabel: defaults.quoteAssetLabel,
    estimatedSupplyPercent: estimateSupplyPercent(quote.amountA.amount, defaults.supply),
  };
}

async function quoteUsd1OutputFromSolInput(raydium, connection, inputLamports, slippageBps) {
  const pool = await loadPinnedUsd1RoutePool(raydium);
  const quote = await computeDirectRouteSwap(raydium, connection, pool, inputLamports, slippageBps);
  return {
    inputLamports,
    expectedOut: new BN(quote.expectedOut.toString()),
    minOut: new BN(quote.minOut.toString()),
  };
}

async function quoteSolInputForUsd1Output(raydium, connection, requiredQuoteAmount, slippageBps) {
  const policy = getUsd1TopupPolicy();
  const pool = await loadPinnedUsd1RoutePool(raydium);
  const referencePrice = Number(pool.price || 0);
  if (!Number.isFinite(referencePrice) || referencePrice <= 0) {
    throw new Error(`Pinned USD1 route pool has invalid price metadata: ${PINNED_USD1_ROUTE_POOL_ID}`);
  }
  const maxInputLamports = parseDecimalToBn("100000", 9, "maximum SOL quote search");
  let low = new BN(1);
  let high = parseDecimalToBn(String(requiredQuoteAmount.toNumber() / 1_000_000 / referencePrice * 1.1 || 0.01), 9, "top-up search guess");
  if (high.lte(new BN(0))) high = parseDecimalToBn("0.01", 9, "top-up search floor");
  if (high.gt(maxInputLamports)) high = maxInputLamports.clone();
  let quote = await computeDirectRouteSwap(raydium, connection, pool, high, slippageBps);
  while (quote.minOut.lt(requiredQuoteAmount) && high.lt(maxInputLamports)) {
    low = high.add(new BN(1));
    high = minBn(high.mul(new BN(2)), maxInputLamports);
    quote = await computeDirectRouteSwap(raydium, connection, pool, high, slippageBps);
    if (high.eq(maxInputLamports)) break;
  }
  if (quote.minOut.lt(requiredQuoteAmount)) {
    throw new Error(
      `Pinned USD1 route pool could not satisfy required USD1 output: ${PINNED_USD1_ROUTE_POOL_ID}. `
      + `requiredUsd1=${formatBn(requiredQuoteAmount, 6, 6)} `
      + `maxQuotedSol=${formatBn(maxInputLamports, 9, 6)} `
      + `quotedUsd1=${formatBn(quote.expectedOut, 6, 6)} `
      + `minUsd1=${formatBn(quote.minOut, 6, 6)} `
      + `priceImpactPct=${quote.priceImpactPct}`
    );
  }
  for (let index = 0; index < policy.maxSearchIterations && low.lt(high); index += 1) {
    const mid = low.add(high).div(new BN(2));
    const midQuote = await computeDirectRouteSwap(raydium, connection, pool, mid, slippageBps);
    if (midQuote.minOut.gte(requiredQuoteAmount)) {
      high = mid;
      quote = midQuote;
    } else {
      low = mid.add(new BN(1));
    }
  }
  return {
    inputLamports: high,
    expectedOut: new BN(quote.expectedOut.toString()),
    minOut: new BN(quote.minOut.toString()),
  };
}

async function quoteLaunch(request) {
  const connection = new Connection(request.rpcUrl, request.commitment || "confirmed");
  const raydium = await Raydium.load({
    connection,
    owner: null,
    disableLoadToken: true,
    disableFeatureCheck: true,
  });
  const buyMode = String(request.mode || "").trim().toLowerCase();
  const defaults = await loadLaunchDefaults(
    raydium,
    connection,
    null,
    request.launchMode || "regular",
    request.quoteAsset,
  );
  if (defaults.quoteAsset === "usd1" && buyMode === "sol") {
    const solInput = parseDecimalToBn(request.amount, 9, "buy amount SOL");
    const usd1RouteQuote = await quoteUsd1OutputFromSolInput(
      raydium,
      connection,
      solInput,
      request.slippageBps,
    );
    const curveQuote = Curve.buyExactIn({
      poolInfo: defaults.poolInfo,
      protocolFeeRate: defaults.configInfo.tradeFeeRate,
      platformFeeRate: defaults.platformInfo.feeRate,
      curveType: defaults.configInfo.curveType,
      shareFeeRate: new BN(0),
      creatorFeeRate: defaults.platformInfo.creatorFeeRate,
      transferFeeConfigA: undefined,
      slot: 0,
      amountB: usd1RouteQuote.minOut,
    });
    return {
      mode: buyMode,
      input: request.amount,
      estimatedTokens: formatBn(curveQuote.amountA.amount, TOKEN_DECIMALS, 6),
      estimatedSol: formatBn(solInput, 9, 6),
      estimatedQuoteAmount: formatBn(solInput, 9, 6),
      quoteAsset: "sol",
      quoteAssetLabel: "SOL",
      estimatedSupplyPercent: estimateSupplyPercent(curveQuote.amountA.amount, defaults.supply),
    };
  }
  if (defaults.quoteAsset === "usd1" && buyMode === "tokens") {
    const tokenAmount = parseDecimalToBn(request.amount, TOKEN_DECIMALS, "buy amount");
    const curveQuote = Curve.buyExactOut({
      poolInfo: defaults.poolInfo,
      protocolFeeRate: defaults.configInfo.tradeFeeRate,
      platformFeeRate: defaults.platformInfo.feeRate,
      curveType: defaults.configInfo.curveType,
      shareFeeRate: new BN(0),
      creatorFeeRate: defaults.platformInfo.creatorFeeRate,
      transferFeeConfigA: undefined,
      slot: 0,
      amountA: tokenAmount,
    });
    const solQuote = await quoteSolInputForUsd1Output(
      raydium,
      connection,
      new BN(curveQuote.amountB.toString()),
      request.slippageBps,
    );
    return {
      mode: buyMode,
      input: request.amount,
      estimatedTokens: formatBn(tokenAmount, TOKEN_DECIMALS, 6),
      estimatedSol: formatBn(solQuote.inputLamports, 9, 6),
      estimatedQuoteAmount: formatBn(solQuote.inputLamports, 9, 6),
      quoteAsset: "sol",
      quoteAssetLabel: "SOL",
      estimatedSupplyPercent: estimateSupplyPercent(tokenAmount, defaults.supply),
    };
  }
  return buildQuote(defaults, buyMode, request.amount);
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

async function loadPinnedUsd1RoutePool(raydium) {
  const pools = await raydium.api.fetchPoolById({ ids: PINNED_USD1_ROUTE_POOL_ID });
  const pool = (pools || []).find((entry) => entry && entry.id === PINNED_USD1_ROUTE_POOL_ID);
  if (!pool) {
    throw new Error(`Pinned USD1 route pool not found: ${PINNED_USD1_ROUTE_POOL_ID}`);
  }
  const mintA = pool.mintA && (pool.mintA.address || pool.mintA);
  const mintB = pool.mintB && (pool.mintB.address || pool.mintB);
  const isExpectedPair = [mintA, mintB].includes(NATIVE_MINT.toBase58())
    && [mintA, mintB].includes(USD1_MINT.toBase58());
  if (!isExpectedPair) {
    throw new Error(`Pinned USD1 route pool no longer matches SOL/USD1: ${PINNED_USD1_ROUTE_POOL_ID}`);
  }
  if (!pool.config || pool.config.id !== PREFERRED_USD1_ROUTE_CONFIG) {
    throw new Error(`Pinned USD1 route pool config changed: ${PINNED_USD1_ROUTE_POOL_ID}`);
  }
  return pool;
}

async function prepareUsd1Topup(raydium, connection, owner, request, requiredQuoteAmountRaw) {
  if (resolveQuoteAssetConfig(request.quoteAsset).asset !== "usd1") {
    return null;
  }
  const policy = getUsd1TopupPolicy();
  const requiredQuoteAmount = parseDecimalToBn(requiredQuoteAmountRaw, 6, "required USD1 amount");
  if (requiredQuoteAmount.lte(new BN(0))) {
    return null;
  }
  const currentUsd1Balance = await fetchWalletTokenBalance(connection, owner.publicKey, USD1_MINT);
  if (currentUsd1Balance.gte(requiredQuoteAmount)) {
    return {
      swapResult: null,
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

  const pool = await loadPinnedUsd1RoutePool(raydium);
  const referencePrice = Number(pool.price || 0);
  if (!Number.isFinite(referencePrice) || referencePrice <= 0) {
    throw new Error(`Pinned USD1 route pool has invalid price metadata: ${PINNED_USD1_ROUTE_POOL_ID}`);
  }
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
  if (quote.minOut.lt(shortfall)) {
    throw new Error(
      `Pinned USD1 route pool could not satisfy required USD1 output: ${PINNED_USD1_ROUTE_POOL_ID}. `
      + `requiredUsd1=${formatBn(shortfall, 6, 6)} `
      + `maxSpendableSol=${formatBn(maxSpendableLamports, 9, 6)} `
      + `quotedUsd1=${formatBn(quote.expectedOut, 6, 6)} `
      + `minUsd1=${formatBn(quote.minOut, 6, 6)} `
      + `priceImpactPct=${quote.priceImpactPct}`
    );
  }
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
  const swapResult = await raydium.tradeV2.swap({
    txVersion: txVersionFromFormat(request.txFormat),
    swapInfo: quote.swapInfo,
    swapPoolKeys: quote.swapPoolKeys,
    ownerInfo: {
      associatedOnly: false,
      checkCreateATAOwner: true,
    },
    routeProgram: RAYDIUM_ROUTE_PROGRAM,
    computeBudgetConfig: buildComputeBudgetConfig(request.txConfig),
    txTipConfig: buildTipConfig(request.txConfig),
    feePayer: owner.publicKey,
  });
  const normalizedSwapResult = await ensureInlineTipOnSwapResult(
    connection,
    owner,
    swapResult,
    request.txConfig,
  );
  return {
    swapResult: normalizedSwapResult,
    requiredQuoteAmount: formatBn(requiredQuoteAmount, 6, 6),
    currentQuoteAmount: formatBn(currentUsd1Balance, 6, 6),
    shortfallQuoteAmount: formatBn(shortfall, 6, 6),
    inputSol: formatBn(high, 9, 6),
    expectedQuoteOut: formatBn(quote.expectedOut, 6, 6),
    minQuoteOut: formatBn(quote.minOut, 6, 6),
    priceImpactPct: String(quote.priceImpactPct),
    routePassedPolicy: Number(pool.tvl || 0) >= policy.minPoolTvlUsd
      && quote.priceImpactPct <= policy.maxPriceImpactPct,
    routePoolId: pool.id,
    routeConfigId: pool.config && pool.config.id ? pool.config.id : "",
    routePoolType: pool.type,
    routePoolTvlUsd: String(pool.tvl || 0),
  };
}

async function buildUsd1Topup(request) {
  const owner = parseKeypair(request.ownerSecret);
  const connection = new Connection(request.rpcUrl, request.commitment || "confirmed");
  const raydium = await Raydium.load({
    connection,
    owner,
    disableLoadToken: true,
    disableFeatureCheck: true,
  });
  const prepared = await prepareUsd1Topup(
    raydium,
    connection,
    owner,
    request,
    request.requiredQuoteAmount,
  );
  if (!prepared || !prepared.swapResult) {
    return {
      compiledTransaction: null,
      requiredQuoteAmount: prepared && prepared.requiredQuoteAmount ? prepared.requiredQuoteAmount : undefined,
      currentQuoteAmount: prepared && prepared.currentQuoteAmount ? prepared.currentQuoteAmount : undefined,
      shortfallQuoteAmount: prepared && prepared.shortfallQuoteAmount ? prepared.shortfallQuoteAmount : undefined,
    };
  }
  const { lastValidBlockHeight } = await connection.getLatestBlockhash(request.commitment || "confirmed");
  return {
    compiledTransaction: normalizeTransactions(prepared.swapResult, {
      labelPrefix: request.labelPrefix || "usd1-topup",
      computeUnitLimit: request.txConfig && request.txConfig.computeUnitLimit,
      computeUnitPriceMicroLamports: request.txConfig && request.txConfig.computeUnitPriceMicroLamports,
      inlineTipLamports: request.txConfig && request.txConfig.tipLamports,
      inlineTipAccount: request.txConfig && request.txConfig.tipAccount,
      lastValidBlockHeight,
    })[0],
    requiredQuoteAmount: prepared.requiredQuoteAmount,
    currentQuoteAmount: prepared.currentQuoteAmount,
    shortfallQuoteAmount: prepared.shortfallQuoteAmount,
    inputSol: prepared.inputSol,
    expectedQuoteOut: prepared.expectedQuoteOut,
    minQuoteOut: prepared.minQuoteOut,
    priceImpactPct: prepared.priceImpactPct,
    routePoolId: prepared.routePoolId,
    routeConfigId: prepared.routeConfigId,
    routePoolType: prepared.routePoolType,
    routePoolTvlUsd: prepared.routePoolTvlUsd,
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
    } else if (defaults.quoteAsset === "usd1") {
      const solInput = parseDecimalToBn(request.devBuy.amount, 9, "dev buy SOL");
      const usd1RouteQuote = await quoteUsd1OutputFromSolInput(
        raydium,
        connection,
        solInput,
        request.slippageBps,
      );
      buyAmount = usd1RouteQuote.minOut;
    } else {
      buyAmount = parseDecimalToBn(
        request.devBuy.amount,
        defaults.quoteDecimals,
        `dev buy ${defaults.quoteAssetLabel}`,
      );
    }
  }
  if (allowAtaCreation(request) && !createOnly && defaults.quoteAsset !== "sol") {
    await ensureAssociatedTokenAccountExists(connection, owner, defaults.quoteMint, request, raydium);
  }
  const usd1Topup = !createOnly && defaults.quoteAsset === "usd1" && buyAmount
    ? await prepareUsd1Topup(
      raydium,
      connection,
      owner,
      {
        ...request,
        requiredQuoteAmount: formatBn(buyAmount, defaults.quoteDecimals, 6),
      },
      formatBn(buyAmount, defaults.quoteDecimals, 6),
    )
    : null;
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
    associatedOnly: false,
    checkCreateATAOwner: true,
  });
  const launchTransactions = extractTransactions(buildResult);
  let atomicFallbackReason = null;
  if (usd1Topup && usd1Topup.swapResult) {
    const topupTransactions = extractTransactions(usd1Topup.swapResult);
    if (topupTransactions.length === 1 && launchTransactions.length === 1) {
      try {
        const combined = await combineAtomicUsd1ActionTransaction(
          connection,
          owner,
          request,
          topupTransactions[0],
          launchTransactions[0],
          [mintKeypair],
        );
        return {
          mint: mintKeypair.publicKey.toBase58(),
          launchCreator: owner.publicKey.toBase58(),
          compiledTransactions: normalizeTransactions({ transactions: [combined.transaction] }, {
            labelPrefix: "launch",
            computeUnitLimit: request.txConfig && request.txConfig.computeUnitLimit,
            computeUnitPriceMicroLamports: request.txConfig && request.txConfig.computeUnitPriceMicroLamports,
            inlineTipLamports: request.txConfig && request.txConfig.tipLamports,
            inlineTipAccount: request.txConfig && request.txConfig.tipAccount,
            lastValidBlockHeight: combined.lastValidBlockHeight,
          }),
          atomicCombined: true,
        };
      } catch (error) {
        if (!isAtomicMessageOverflowError(error)) {
          throw error;
        }
        atomicFallbackReason = error && error.message ? error.message : String(error);
      }
    }
  }
  const { lastValidBlockHeight } = await connection.getLatestBlockhash(request.commitment || "confirmed");
  const compiledTransactions = normalizeTransactions(buildResult, {
    labelPrefix: "launch",
    computeUnitLimit: request.txConfig && request.txConfig.computeUnitLimit,
    computeUnitPriceMicroLamports: request.txConfig && request.txConfig.computeUnitPriceMicroLamports,
    inlineTipLamports: request.txConfig && request.txConfig.tipLamports,
    inlineTipAccount: request.txConfig && request.txConfig.tipAccount,
    lastValidBlockHeight,
  });
  if (usd1Topup && usd1Topup.swapResult) {
    compiledTransactions.unshift(...normalizeTransactions(usd1Topup.swapResult, {
      labelPrefix: request.labelPrefix || "launch-usd1-topup",
      computeUnitLimit: request.txConfig && request.txConfig.computeUnitLimit,
      computeUnitPriceMicroLamports: request.txConfig && request.txConfig.computeUnitPriceMicroLamports,
      inlineTipLamports: request.txConfig && request.txConfig.tipLamports,
      inlineTipAccount: request.txConfig && request.txConfig.tipAccount,
      lastValidBlockHeight,
    }));
  }
  return {
    mint: mintKeypair.publicKey.toBase58(),
    launchCreator: owner.publicKey.toBase58(),
    compiledTransactions,
    atomicCombined: false,
    atomicFallbackReason,
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
  if (allowAtaCreation(request) && quote.asset !== "sol") {
    await ensureAssociatedTokenAccountExists(connection, owner, quote.mint, request, raydium);
  }
  const buyAmount = parseDecimalToBn(request.buyAmountSol, quote.decimals, `follow buy amount ${quote.label}`);
  const options = {
    programId: LAUNCHPAD_PROGRAM,
    mintA: mint,
    buyAmount,
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
  } else {
    const livePool = await loadLivePoolContext(raydium, connection, mint, request.quoteAsset);
    Object.assign(options, {
      poolInfo: livePool.poolInfo,
      configInfo: livePool.configInfo,
      platformFeeRate: livePool.platformInfo.feeRate,
      mintAProgram: TOKEN_PROGRAM_ID,
      skipCheckMintA: true,
    });
  }
  const buildResult = await raydium.launchpad.buyToken({
    ...options,
    associatedOnly: false,
    checkCreateATAOwner: true,
  });
  if (atomic && quote.asset === "usd1") {
    const usd1Topup = await prepareUsd1Topup(
      raydium,
      connection,
      owner,
      {
        ...request,
        requiredQuoteAmount: formatBn(buyAmount, quote.decimals, 6),
      },
      formatBn(buyAmount, quote.decimals, 6),
    );
    if (usd1Topup && usd1Topup.swapResult) {
      const topupTransactions = extractTransactions(usd1Topup.swapResult);
      const buyTransactions = extractTransactions(buildResult);
      if (topupTransactions.length !== 1 || buyTransactions.length !== 1) {
        throw new Error("Atomic USD1 follow buy requires exactly one top-up transaction and one buy transaction.");
      }
      const combined = await combineAtomicUsd1ActionTransaction(
        connection,
        owner,
        request,
        topupTransactions[0],
        buyTransactions[0],
      );
      return {
        compiledTransaction: normalizeTransactions({ transactions: [combined.transaction] }, {
          labelPrefix,
          computeUnitLimit: request.txConfig && request.txConfig.computeUnitLimit,
          computeUnitPriceMicroLamports: request.txConfig && request.txConfig.computeUnitPriceMicroLamports,
          inlineTipLamports: request.txConfig && request.txConfig.tipLamports,
          inlineTipAccount: request.txConfig && request.txConfig.tipAccount,
          lastValidBlockHeight: combined.lastValidBlockHeight,
        })[0],
      };
    }
  }
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
  const livePool = await loadLivePoolContext(raydium, connection, mint, request.quoteAsset);
  const buildResult = await raydium.launchpad.sellToken({
    programId: LAUNCHPAD_PROGRAM,
    mintA: mint,
    sellAmount,
    poolInfo: livePool.poolInfo,
    configInfo: livePool.configInfo,
    platformFeeRate: livePool.platformInfo.feeRate,
    slippage: new BN(String(request.slippageBps || 0)),
    txVersion: txVersionFromFormat(request.txFormat),
    computeBudgetConfig: buildComputeBudgetConfig(request.txConfig),
    txTipConfig: buildTipConfig(request.txConfig),
    associatedOnly: false,
    checkCreateATAOwner: true,
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

async function detectImportContext(request) {
  const connection = new Connection(request.rpcUrl, request.commitment || "processed");
  const raydium = await Raydium.load({
    connection,
    owner: null,
    disableLoadToken: true,
    disableFeatureCheck: true,
  });
  const mint = new PublicKey(request.mint);
  const candidates = [];
  for (const asset of ["sol", "usd1"]) {
    try {
      const quote = resolveQuoteAssetConfig(asset);
      const poolId = getPdaLaunchpadPoolId(LAUNCHPAD_PROGRAM, mint, quote.mint).publicKey;
      const poolInfo = await raydium.launchpad.getRpcPoolInfo({ poolId });
      const platformId = poolInfo.platformId && poolInfo.platformId.toBase58
        ? poolInfo.platformId.toBase58()
        : String(poolInfo.platformId || "");
      const configId = poolInfo.configId && poolInfo.configId.toBase58
        ? poolInfo.configId.toBase58()
        : String(poolInfo.configId || "");
      candidates.push({
        launchpad: "bonk",
        mode: platformId === BONKERS_PLATFORM.toBase58() ? "bonkers" : "regular",
        quoteAsset: quote.asset,
        creator: poolInfo.creator && poolInfo.creator.toBase58
          ? poolInfo.creator.toBase58()
          : String(poolInfo.creator || ""),
        platformId,
        configId,
        poolId: poolId.toBase58(),
        realQuoteReserves: poolInfo.realB ? poolInfo.realB.toString() : "0",
        complete: Number(poolInfo.status || 0) !== 0,
        detectionSource: "raydium-launchpad",
      });
    } catch (_error) {
      // Ignore missing pool shapes and keep probing the other quote asset.
    }
  }
  if (!candidates.length) {
    return null;
  }
  candidates.sort((left, right) => {
    const leftLiquidity = BigInt(left.realQuoteReserves || "0");
    const rightLiquidity = BigInt(right.realQuoteReserves || "0");
    if (leftLiquidity === rightLiquidity) {
      return left.quoteAsset === "sol" ? -1 : 1;
    }
    return rightLiquidity > leftLiquidity ? 1 : -1;
  });
  return candidates[0];
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
      response = await quoteLaunch(request);
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
    case "detect-import-context":
      response = await detectImportContext(request);
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
