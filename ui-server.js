#!/usr/bin/env node
"use strict";

require("dotenv").config({ quiet: true, override: true });

const fs = require("fs");
const path = require("path");
const http = require("http");
const { Connection, LAMPORTS_PER_SOL, PublicKey } = require("@solana/web3.js");
const { OnlinePumpSdk } = require("@pump-fun/pump-sdk");
const {
  LAUNCHPADS,
  PROVIDERS,
  createDefaultPersistentConfig,
  getLocalDataDir,
  normalizePersistentConfig,
  readPersistentConfig,
  resolveProviderSupport,
  writePersistentConfig,
} = require("./config/app-config");
const { getLaunchpadRegistry } = require("./launchpads/registry");
const { getStrategyRegistry } = require("./strategies/registry");
const { getRpcUrl } = require("./rpc");
const { listSolanaEnvWallets, loadSolanaWalletByEnvKey, loadKeypairFromEnvOrArgs } = require("./keypair");
const { ENGINE_BASE_URL, getEngineBackendMode, getEngineHealth, runEngineAction } = require("./engine-client");

const HOST = "127.0.0.1";
const PORT = Number(process.env.LAUNCHDECK_PORT || 8789);
const STATIC_DIR = path.join(__dirname, "ui");
const USD1_MINT = new PublicKey("USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB");
const LOCAL_DATA_DIR = getLocalDataDir(__dirname);
const UPLOAD_DIR = path.join(LOCAL_DATA_DIR, "uploads");
const IMAGE_LIBRARY_PATH = path.join(LOCAL_DATA_DIR, "image-library.json");
const ALLOWED_IMAGE_TYPES = new Map([
  ["image/png", ".png"],
  ["image/jpeg", ".jpg"],
  ["image/webp", ".webp"],
  ["image/gif", ".gif"],
]);
const JITO_TIP_ACCOUNTS = [
  "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
  "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
  "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
  "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
  "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
  "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
  "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
  "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
];
const FIXED_COMPUTE_UNIT_LIMIT = 1_000_000n;
const BPS_DENOMINATOR = 10_000n;
const TOKEN_DECIMALS = 6;
const TOTAL_SUPPLY_TOKENS = 1_000_000_000n;
const TOTAL_SUPPLY_RAW = TOTAL_SUPPLY_TOKENS * (10n ** BigInt(TOKEN_DECIMALS));
let globalCache = {
  fetchedAt: 0,
  data: null,
};

function getProviderAvailability() {
  const status = {};
  const heliusConfigured = Boolean(process.env.HELIUS_RPC_URL || process.env.HELIUS_API_KEY);
  const astralaneConfigured = Boolean(process.env.ASTRALANE_API_KEY);
  const bloxrouteConfigured = Boolean(process.env.BLOXROUTE_AUTH_HEADER);
  const helloMoonConfigured = Boolean(process.env.HELLOMOON_API_KEY || process.env.HELLOMOON_RPC_URL);

  for (const provider of PROVIDERS) {
    const support = resolveProviderSupport(provider);
    const entry = {
      provider,
      available: false,
      verified: support.verified,
      supportState: support.supportState,
      supportsSingle: support.supportsSingle,
      supportsBundle: support.supportsBundle,
      supportsSequential: support.supportsSequential,
      reason: "",
    };

    if (provider === "auto") {
      entry.available = true;
    } else if (provider === "helius") {
      entry.available = true;
      entry.reason = heliusConfigured ? "" : "Using baseline RPC/Helius default path without explicit env override.";
    } else if (provider === "jito") {
      entry.available = true;
    } else if (provider === "astralane") {
      entry.available = astralaneConfigured;
      entry.reason = astralaneConfigured ? "" : "Missing ASTRALANE_API_KEY.";
    } else if (provider === "bloxroute") {
      entry.available = bloxrouteConfigured;
      entry.reason = bloxrouteConfigured ? "" : "Missing BLOXROUTE_AUTH_HEADER.";
    } else if (provider === "hellomoon") {
      entry.available = helloMoonConfigured;
      entry.reason = helloMoonConfigured ? "" : "Missing HELLOMOON_API_KEY or HELLOMOON_RPC_URL.";
    }

    status[provider] = entry;
  }

  return status;
}

function getLaunchpadAvailability() {
  return getLaunchpadRegistry();
}

function getLaunchpadDisplayLabel(entry, fallbackLabel) {
  const label = String(fallbackLabel || entry && entry.label || "").trim();
  return entry && entry.supportState === "unverified"
    ? `${label} (unverified)`
    : label;
}

function serializeBootstrapScriptValue(value) {
  return JSON.stringify(value)
    .replace(/</g, "\\u003c")
    .replace(/>/g, "\\u003e")
    .replace(/&/g, "\\u0026");
}

function renderIndexHtml() {
  const launchpads = getLaunchpadAvailability();
  const bootstrap = {
    config: getUiConfig(),
    launchpads,
  };
  const replacements = new Map([
    ["__PUMP_LABEL__", getLaunchpadDisplayLabel(launchpads.pump, "Pump")],
    ["__BONK_LABEL__", getLaunchpadDisplayLabel(launchpads.bonk, "Bonk")],
    ["__BAGSAPP_LABEL__", getLaunchpadDisplayLabel(launchpads.bagsapp, "Bagsapp")],
    ["__LAUNCHDECK_BOOTSTRAP__", serializeBootstrapScriptValue(bootstrap)],
  ]);
  let html = fs.readFileSync(path.join(STATIC_DIR, "index.html"), "utf8");
  for (const [token, value] of replacements.entries()) {
    html = html.split(token).join(value);
  }
  return html;
}

function getUiConfig() {
  return readPersistentConfig(__dirname);
}

function normalizeHttpUrl(rawValue) {
  const raw = String(rawValue || "").trim();
  if (!raw) return "";
  const withProtocol = /^https?:\/\//i.test(raw) ? raw : `https://${raw.replace(/^\/\//, "")}`;
  try {
    const parsed = new URL(withProtocol);
    if (!["http:", "https:"].includes(parsed.protocol)) return "";
    return parsed.toString();
  } catch (_error) {
    return "";
  }
}

function normalizeSocialUrl(rawValue, type) {
  const raw = String(rawValue || "").trim();
  if (!raw) return "";
  if (/^https?:\/\//i.test(raw) || raw.startsWith("//")) {
    return normalizeHttpUrl(raw);
  }
  const normalizedHandle = raw.replace(/^@/, "").replace(/^\/+/, "").trim();
  if (!normalizedHandle) return "";
  if (type === "twitter") return `https://x.com/${normalizedHandle}`;
  if (type === "telegram") return `https://t.me/${normalizedHandle}`;
  return normalizeHttpUrl(raw);
}

async function fetchJsonOrNull(url, init = {}) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 8_000);
  try {
    const response = await fetch(url, {
      ...init,
      headers: {
        accept: "application/json",
        ...(init.headers || {}),
      },
      signal: controller.signal,
    });
    if (!response.ok) return null;
    return await response.json();
  } catch (_error) {
    return null;
  } finally {
    clearTimeout(timeout);
  }
}

function mergeImportedTokenData(base = {}, overlay = {}) {
  return {
    name: base.name || overlay.name || "",
    symbol: base.symbol || overlay.symbol || "",
    description: base.description || overlay.description || "",
    website: base.website || overlay.website || "",
    twitter: base.twitter || overlay.twitter || "",
    telegram: base.telegram || overlay.telegram || "",
    imageUrl: base.imageUrl || overlay.imageUrl || "",
    metadataUri: base.metadataUri || overlay.metadataUri || "",
    source: base.source || overlay.source || "",
  };
}

function normalizeImportedMetadataPayload(payload = {}, source = "") {
  const websites = Array.isArray(payload.websites) ? payload.websites : [];
  const socials = Array.isArray(payload.socials) ? payload.socials : [];
  const extensions = payload.extensions && typeof payload.extensions === "object" ? payload.extensions : {};
  const properties = payload.properties && typeof payload.properties === "object" ? payload.properties : {};
  const firstWebsite = websites.find((entry) => entry && entry.url);
  const twitterSocial = socials.find((entry) => String(entry && entry.type || "").toLowerCase() === "twitter");
  const telegramSocial = socials.find((entry) => String(entry && entry.type || "").toLowerCase() === "telegram");

  return {
    name: String(payload.name || payload.token_name || "").trim(),
    symbol: String(payload.symbol || payload.ticker || "").trim(),
    description: String(payload.description || "").trim(),
    website: normalizeHttpUrl(
      payload.website
      || payload.external_url
      || extensions.website
      || properties.website
      || (firstWebsite && firstWebsite.url)
      || "",
    ),
    twitter: normalizeSocialUrl(
      payload.twitter
      || extensions.twitter
      || properties.twitter
      || (twitterSocial && twitterSocial.url)
      || "",
      "twitter",
    ),
    telegram: normalizeSocialUrl(
      payload.telegram
      || extensions.telegram
      || properties.telegram
      || (telegramSocial && telegramSocial.url)
      || "",
      "telegram",
    ),
    imageUrl: normalizeHttpUrl(
      payload.image_uri
      || payload.image
      || payload.imageUrl
      || extensions.image
      || properties.image
      || "",
    ),
    metadataUri: normalizeHttpUrl(payload.metadataUri || payload.metadata_uri || payload.uri || ""),
    source,
  };
}

async function fetchImportedTokenMetadata(contractAddress) {
  let imported = {};

  const pumpPayload = await fetchJsonOrNull(`https://frontend-api-v3.pump.fun/coins/${encodeURIComponent(contractAddress)}`);
  if (pumpPayload) {
    imported = mergeImportedTokenData(imported, normalizeImportedMetadataPayload(pumpPayload, "pump.fun"));
    const metadataPayload = imported.metadataUri ? await fetchJsonOrNull(imported.metadataUri) : null;
    if (metadataPayload) {
      imported = mergeImportedTokenData(imported, normalizeImportedMetadataPayload(metadataPayload, "metadata"));
    }
  }

  const dexPayload = await fetchJsonOrNull(`https://api.dexscreener.com/latest/dex/tokens/${encodeURIComponent(contractAddress)}`);
  const dexPair = dexPayload && Array.isArray(dexPayload.pairs) ? dexPayload.pairs.find(Boolean) : null;
  if (dexPair) {
    imported = mergeImportedTokenData(imported, normalizeImportedMetadataPayload({
      name: dexPair.baseToken && dexPair.baseToken.name,
      symbol: dexPair.baseToken && dexPair.baseToken.symbol,
      imageUrl: dexPair.info && dexPair.info.imageUrl,
      websites: dexPair.info && dexPair.info.websites,
      socials: dexPair.info && dexPair.info.socials,
    }, "DexScreener"));
  }

  if (!imported.name && !imported.symbol && !imported.imageUrl && !imported.website && !imported.twitter && !imported.telegram) {
    throw new Error("No token metadata was found for that contract address.");
  }

  return imported;
}

async function importRemoteImageToLibrary(imageUrl, { originalName = "vamped-image", recordName = "" } = {}) {
  const safeUrl = normalizeHttpUrl(imageUrl);
  if (!safeUrl) return null;

  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 10_000);
  try {
    const response = await fetch(safeUrl, { redirect: "follow", signal: controller.signal });
    if (!response.ok) {
      throw new Error("Unable to download token image.");
    }

    const contentType = String(response.headers.get("content-type") || "").split(";")[0].trim().toLowerCase();
    let extension = ALLOWED_IMAGE_TYPES.get(contentType);
    if (!extension) {
      const pathname = new URL(safeUrl).pathname.toLowerCase();
      if (pathname.endsWith(".png")) extension = ".png";
      else if (pathname.endsWith(".jpg") || pathname.endsWith(".jpeg")) extension = ".jpg";
      else if (pathname.endsWith(".webp")) extension = ".webp";
      else if (pathname.endsWith(".gif")) extension = ".gif";
    }
    if (!extension) {
      throw new Error("Imported token image format is not supported.");
    }

    const buffer = Buffer.from(await response.arrayBuffer());
    if (!buffer.length) {
      throw new Error("Imported token image was empty.");
    }
    if (buffer.length > 8_000_000) {
      throw new Error("Imported token image is too large.");
    }

    const safeBaseName = path
      .basename(originalName, path.extname(originalName))
      .replace(/[^a-zA-Z0-9_-]+/g, "-")
      .replace(/^-+|-+$/g, "") || "vamp";

    ensureDir(UPLOAD_DIR);
    const fileName = `${timestampSlug()}-${safeBaseName}${extension}`;
    const filePath = path.join(UPLOAD_DIR, fileName);
    fs.writeFileSync(filePath, buffer);

    const library = readImageLibrary();
    const record = createImageRecord({ fileName, originalName });
    if (recordName) record.name = recordName;
    library.images.unshift(record);
    writeImageLibrary(library);
    return serializeImageRecord(record);
  } finally {
    clearTimeout(timeout);
  }
}

function readImageLibrary() {
  ensureDir(LOCAL_DATA_DIR);
  if (!fs.existsSync(IMAGE_LIBRARY_PATH)) {
    return { images: [], categories: [] };
  }
  const raw = fs.readFileSync(IMAGE_LIBRARY_PATH, "utf8").trim();
  if (!raw) {
    return { images: [], categories: [] };
  }
  const parsed = JSON.parse(raw);
  const images = Array.isArray(parsed.images) ? parsed.images : [];
  const categories = Array.isArray(parsed.categories)
    ? parsed.categories.map((entry) => String(entry || "").trim()).filter(Boolean)
    : [];
  const imageCategories = images
    .map((entry) => String(entry.category || "").trim())
    .filter(Boolean);

  return {
    categories: Array.from(new Set([...categories, ...imageCategories])).sort((a, b) => a.localeCompare(b)),
    images: images
      .map((entry) => {
        const fileName = String(entry.fileName || "").trim();
        if (!fileName) return null;
        return {
          id: String(entry.id || fileName),
          fileName,
          name: String(entry.name || path.basename(fileName, path.extname(fileName))).trim(),
          tags: Array.isArray(entry.tags) ? entry.tags.map((tag) => String(tag || "").trim()).filter(Boolean) : [],
          category: String(entry.category || "").trim(),
          isFavorite: Boolean(entry.isFavorite),
          createdAt: Number(entry.createdAt || Date.now()),
          updatedAt: Number(entry.updatedAt || entry.createdAt || Date.now()),
        };
      })
      .filter(Boolean),
  };
}

function writeImageLibrary(library) {
  ensureDir(path.dirname(IMAGE_LIBRARY_PATH));
  fs.writeFileSync(IMAGE_LIBRARY_PATH, JSON.stringify(library, null, 2), "utf8");
}

function serializeImageRecord(record) {
  return {
    id: record.id,
    fileName: record.fileName,
    name: record.name,
    tags: record.tags,
    category: record.category,
    isFavorite: record.isFavorite,
    createdAt: record.createdAt,
    updatedAt: record.updatedAt,
    previewUrl: `/uploads/${encodeURIComponent(record.fileName)}`,
  };
}

function createImageRecord({ fileName, originalName }) {
  const baseName = path.basename(originalName || fileName, path.extname(originalName || fileName));
  return {
    id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    fileName,
    name: baseName || path.basename(fileName, path.extname(fileName)),
    tags: [],
    category: "",
    isFavorite: false,
    createdAt: Date.now(),
    updatedAt: Date.now(),
  };
}

function buildImageLibraryPayload({ search = "", category = "", favoritesOnly = false } = {}) {
  const library = readImageLibrary();
  const normalizedSearch = String(search || "").trim().toLowerCase();
  const normalizedCategory = String(category || "").trim().toLowerCase();
  const filtered = library.images
    .filter((record) => fs.existsSync(path.join(UPLOAD_DIR, record.fileName)))
    .filter((record) => {
      if (favoritesOnly && !record.isFavorite) return false;
      if (normalizedCategory && normalizedCategory !== "all" && normalizedCategory !== "favorites") {
        if (String(record.category || "").trim().toLowerCase() !== normalizedCategory) return false;
      }
      if (!normalizedSearch) return true;
      const haystack = [
        record.name,
        record.category,
        ...(record.tags || []),
        record.fileName,
      ].join(" ").toLowerCase();
      return haystack.includes(normalizedSearch);
    })
    .sort((a, b) => Number(b.updatedAt || 0) - Number(a.updatedAt || 0));
  return {
    ok: true,
    images: filtered.map(serializeImageRecord),
    categories: library.categories,
  };
}

function buildSettingsPayload() {
  return {
    ok: true,
    config: getUiConfig(),
    defaults: createDefaultPersistentConfig(),
    providers: getProviderAvailability(),
    launchpads: getLaunchpadAvailability(),
    strategies: getStrategyRegistry(),
    engine: {
      backend: getEngineBackendMode(),
      url: ENGINE_BASE_URL,
    },
  };
}

function sendJson(res, statusCode, payload) {
  res.writeHead(statusCode, {
    "content-type": "application/json; charset=utf-8",
    "cache-control": "no-store",
  });
  res.end(JSON.stringify(payload));
}

function sendFile(res, filePath, contentType) {
  try {
    const body = fs.readFileSync(filePath);
    res.writeHead(200, {
      "content-type": contentType,
      "cache-control": "no-store",
    });
    res.end(body);
  } catch (error) {
    sendJson(res, 404, { error: "Not found" });
  }
}

function guessContentType(filePath) {
  const extension = path.extname(filePath).toLowerCase();
  if (extension === ".html") return "text/html; charset=utf-8";
  if (extension === ".js") return "application/javascript; charset=utf-8";
  if (extension === ".css") return "text/css; charset=utf-8";
  if (extension === ".svg") return "image/svg+xml";
  if (extension === ".png") return "image/png";
  if (extension === ".jpg" || extension === ".jpeg") return "image/jpeg";
  if (extension === ".webp") return "image/webp";
  if (extension === ".gif") return "image/gif";
  return "application/octet-stream";
}

function readJsonBody(req) {
  return new Promise((resolve, reject) => {
    let raw = "";
    req.setEncoding("utf8");
    req.on("data", (chunk) => {
      raw += chunk;
      if (raw.length > 4_000_000) {
        reject(new Error("Request body too large."));
        req.destroy();
      }
    });
    req.on("end", () => {
      try {
        resolve(raw ? JSON.parse(raw) : {});
      } catch (error) {
        reject(new Error("Invalid JSON body."));
      }
    });
    req.on("error", reject);
  });
}

function ensureDir(dirPath) {
  fs.mkdirSync(dirPath, { recursive: true });
}

function timestampSlug() {
  return new Date().toISOString().replace(/[:.]/g, "-");
}

function resolveSignerSource(selectedWalletKey) {
  if (selectedWalletKey) return `env:${selectedWalletKey}`;
  if (process.env.SOLANA_PRIVATE_KEY) return "env:SOLANA_PRIVATE_KEY";
  if (process.env.SOLANA_KEYPAIR_PATH) return "env:SOLANA_KEYPAIR_PATH";
  return "unknown";
}

function parseRecipients(entries, { allowAgent = false } = {}) {
  if (!Array.isArray(entries)) return [];
  return entries.map((entry, index) => {
    const type = String(entry.type || "wallet").trim().toLowerCase();
    const shareBps = Number(entry.shareBps);
    if (!Number.isFinite(shareBps) || shareBps <= 0) {
      throw new Error(`Fee split recipient ${index + 1} must have a positive share.`);
    }
    if (allowAgent && type === "agent") {
      return {
        type: "agent",
        shareBps,
      };
    }
    if (type === "wallet") {
      const address = String(entry.address || "").trim();
      if (!address) {
        throw new Error(`Fee split recipient ${index + 1} is missing a wallet address.`);
      }
      return {
        address,
        shareBps,
      };
    }
    if (type === "github") {
      const githubUsername = String(entry.githubUsername || "").trim();
      if (!githubUsername) {
        throw new Error(`Fee split recipient ${index + 1} is missing a GitHub username.`);
      }
      return {
        githubUsername,
        githubUserId: String(entry.githubUserId || "").trim(),
        shareBps,
      };
    }
    throw new Error(`Unsupported fee split recipient type: ${type}`);
  });
}

function parseDecimalToBigInt(rawValue, decimals, label) {
  const raw = String(rawValue || "").trim();
  if (!raw) return 0n;
  if (!/^\d+(\.\d+)?$/.test(raw)) {
    throw new Error(`${label} must be a positive decimal string. Got: ${rawValue}`);
  }
  const [whole, fractional = ""] = raw.split(".");
  if (fractional.length > decimals) {
    throw new Error(`${label} supports at most ${decimals} decimal places. Got: ${rawValue}`);
  }
  const combined = `${whole}${fractional.padEnd(decimals, "0")}`.replace(/^0+(?=\d)/, "");
  return BigInt(combined || "0");
}

function formatBigIntDecimal(value, decimals, maxFractionDigits = decimals) {
  const negative = value < 0n;
  const absolute = negative ? -value : value;
  const base = 10n ** BigInt(decimals);
  const whole = absolute / base;
  const fraction = absolute % base;
  if (fraction === 0n) return `${negative ? "-" : ""}${whole.toString()}`;

  let fractionText = fraction.toString().padStart(decimals, "0").slice(0, maxFractionDigits);
  fractionText = fractionText.replace(/0+$/, "");
  return `${negative ? "-" : ""}${whole.toString()}${fractionText ? `.${fractionText}` : ""}`;
}

function formatSupplyPercent(rawTokenAmount) {
  if (rawTokenAmount <= 0n) return "0";
  const scaled = (rawTokenAmount * 100_0000n) / TOTAL_SUPPLY_RAW;
  return formatBigIntDecimal(scaled, 4, 4);
}

function ceilDiv(numerator, denominator) {
  return (numerator + denominator - 1n) / denominator;
}

function lamportsToPriorityFeeMicroLamports(priorityFeeLamports) {
  if (priorityFeeLamports <= 0n) return 0;
  return Number((priorityFeeLamports * 1_000_000n) / FIXED_COMPUTE_UNIT_LIMIT);
}

function solStringToLamports(value, label) {
  return parseDecimalToBigInt(value, 9, label);
}

function selectedWalletKeyOrDefault(requestedKey) {
  if (requestedKey) return requestedKey;
  const wallets = listSolanaEnvWallets().filter((entry) => !entry.error);
  return wallets.length > 0 ? wallets[0].envKey : null;
}

function buybackPercentToBps(rawValue) {
  const raw = String(rawValue || "").trim();
  if (!raw) return "";
  const numeric = Number(raw);
  if (!Number.isFinite(numeric) || numeric < 0 || numeric > 100) {
    throw new Error(`buyback percentage must be between 0 and 100. Got: ${rawValue}`);
  }
  return Math.round(numeric * 100);
}

function pickJitoTipAccount() {
  return JITO_TIP_ACCOUNTS[Math.floor(Math.random() * JITO_TIP_ACCOUNTS.length)];
}

async function getConnection() {
  return new Connection(getRpcUrl({}), "confirmed");
}

async function getSplTokenBalance(connection, ownerPublicKey, mintPublicKey) {
  const accounts = await connection.getParsedTokenAccountsByOwner(ownerPublicKey, { mint: mintPublicKey }, "confirmed");
  return accounts.value.reduce((sum, entry) => {
    const amount = Number(entry?.account?.data?.parsed?.info?.tokenAmount?.uiAmountString
      || entry?.account?.data?.parsed?.info?.tokenAmount?.uiAmount
      || 0);
    return Number.isFinite(amount) ? sum + amount : sum;
  }, 0);
}

async function getGlobalState() {
  const now = Date.now();
  if (globalCache.data && now - globalCache.fetchedAt < 15_000) {
    return globalCache.data;
  }

  const connection = await getConnection();
  const sdk = new OnlinePumpSdk(connection);
  const global = await sdk.fetchGlobal();
  globalCache = {
    fetchedAt: now,
    data: global,
  };
  return global;
}

function computeLaunchQuote(global, mode, rawAmount) {
  if (!rawAmount) {
    return null;
  }

  const virtualTokenReserves = BigInt(global.initialVirtualTokenReserves.toString());
  const virtualSolReserves = BigInt(global.initialVirtualSolReserves.toString());
  const protocolFeeBps = BigInt(global.feeBasisPoints.toString());
  const creatorFeeBps = BigInt(global.creatorFeeBasisPoints.toString());
  const totalFeeBps = protocolFeeBps + creatorFeeBps;

  if (mode === "sol") {
    const spendableSolIn = parseDecimalToBigInt(rawAmount, 9, "buy amount");
    if (spendableSolIn <= 0n) return null;
    let netSol = (spendableSolIn * BPS_DENOMINATOR) / (BPS_DENOMINATOR + totalFeeBps);
    const fees =
      ceilDiv(netSol * protocolFeeBps, BPS_DENOMINATOR) +
      ceilDiv(netSol * creatorFeeBps, BPS_DENOMINATOR);
    if (netSol + fees > spendableSolIn) {
      netSol -= netSol + fees - spendableSolIn;
    }
    const numerator = (netSol - 1n) * virtualTokenReserves;
    const denominator = virtualSolReserves + netSol - 1n;
    const tokensOut = denominator > 0n ? numerator / denominator : 0n;
    return {
      mode,
      input: rawAmount,
      estimatedTokens: formatBigIntDecimal(tokensOut, TOKEN_DECIMALS, 6),
      estimatedSol: formatBigIntDecimal(spendableSolIn, 9, 6),
      estimatedSupplyPercent: formatSupplyPercent(tokensOut),
    };
  }

  const tokens = parseDecimalToBigInt(rawAmount, TOKEN_DECIMALS, "buy amount");
  if (tokens <= 0n || tokens >= virtualTokenReserves) return null;
  const netSol = ceilDiv(tokens * virtualSolReserves, virtualTokenReserves - tokens) + 1n;
  const spendableSolIn = ceilDiv(netSol * (BPS_DENOMINATOR + totalFeeBps), BPS_DENOMINATOR);
  return {
    mode,
    input: rawAmount,
    estimatedTokens: formatBigIntDecimal(tokens, TOKEN_DECIMALS, 6),
    estimatedSol: formatBigIntDecimal(spendableSolIn, 9, 6),
    estimatedSupplyPercent: formatSupplyPercent(tokens),
  };
}

async function uploadMetadataToPumpFun({ name, symbol, description, website, twitter, telegram, imageFileName }) {
  const safeFileName = path.basename(String(imageFileName || "").trim());
  if (!safeFileName) {
    throw new Error("Upload an image first so metadata can be created.");
  }
  const imageLocalPath = path.join(UPLOAD_DIR, safeFileName);
  if (!fs.existsSync(imageLocalPath)) {
    throw new Error("Selected image file was not found.");
  }

  const extension = path.extname(imageLocalPath).toLowerCase();
  const mimeType = guessContentType(imageLocalPath);
  if (!ALLOWED_IMAGE_TYPES.has(mimeType)) {
    throw new Error(`Unsupported image type: ${extension}`);
  }

  const form = new FormData();
  form.append("name", name);
  form.append("symbol", symbol);
  form.append("description", description || "");
  form.append("showName", "true");
  form.append("twitter", twitter || "");
  form.append("telegram", telegram || "");
  form.append("website", website || "");
  form.append(
    "file",
    new Blob([fs.readFileSync(imageLocalPath)], { type: mimeType }),
    path.basename(imageLocalPath)
  );

  const response = await fetch("https://pump.fun/api/ipfs", {
    method: "POST",
    body: form,
  });

  const text = await response.text();
  let payload;
  try {
    payload = text ? JSON.parse(text) : {};
  } catch (error) {
    throw new Error(`Metadata provider returned non-JSON response (${response.status}).`);
  }

  if (!response.ok) {
    throw new Error(payload.error || payload.message || `Metadata upload failed with status ${response.status}.`);
  }

  const metadataUri = payload.metadataUri || payload.metadata_uri || payload.uri;
  if (!metadataUri) {
    throw new Error("Metadata upload succeeded but no metadataUri was returned.");
  }

  return {
    metadataUri,
    providerResponse: payload,
  };
}

async function resolveGitHubUser(username) {
  const normalized = String(username || "").trim().replace(/^@+/, "");
  if (!normalized) {
    throw new Error("GitHub username is required.");
  }

  const response = await fetch(`https://api.github.com/users/${encodeURIComponent(normalized)}`, {
    headers: {
      "accept": "application/vnd.github+json",
      "user-agent": "launchdeck-ui",
    },
  });

  const payload = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(payload.message || `GitHub lookup failed for ${normalized}.`);
  }
  if (!payload || !payload.id || !payload.login) {
    throw new Error(`GitHub user ${normalized} did not return a valid user profile.`);
  }
  if (String(payload.type || "").toLowerCase() !== "user") {
    throw new Error("Only GitHub user accounts are supported for creator fees. Organizations are not supported.");
  }

  return {
    username: payload.login,
    userId: String(payload.id),
    profileUrl: payload.html_url || `https://github.com/${payload.login}`,
  };
}

function formToRawConfig(form, action) {
  const appConfig = getUiConfig();
  const presetItems = appConfig && appConfig.presets && Array.isArray(appConfig.presets.items)
    ? appConfig.presets.items
    : createDefaultPersistentConfig().presets.items;
  const activePresetId = String(form.activePresetId || appConfig.defaults.activePresetId || "preset1").trim() || "preset1";
  const activePreset = presetItems.find((entry) => entry.id === activePresetId) || presetItems[0] || createDefaultPersistentConfig().presets.items[0];
  const creationDefaults = activePreset.creationSettings || {};
  const buyDefaults = activePreset.buySettings || {};
  const sellDefaults = activePreset.sellSettings || {};
  const automaticDevSellDefaults = appConfig.defaults && appConfig.defaults.automaticDevSell
    ? appConfig.defaults.automaticDevSell
    : { enabled: false, percent: 0, delaySeconds: 0 };
  const requestedDevBuyMode = String(form.devBuyMode || "").trim();
  const devBuyAmount = String(form.devBuyAmount || "").trim();
  const devBuyMode = requestedDevBuyMode || (devBuyAmount ? "sol" : "");
  const provider = String(form.provider || form.creationProvider || creationDefaults.provider || "helius").trim().toLowerCase();
  const policy = String(form.policy || creationDefaults.policy || "safe").trim().toLowerCase();
  const buyProvider = String(form.buyProvider || buyDefaults.provider || "helius").trim().toLowerCase();
  const buyPolicy = String(form.buyPolicy || buyDefaults.policy || "safe").trim().toLowerCase();
  const sellProvider = String(form.sellProvider || sellDefaults.provider || "helius").trim().toLowerCase();
  const sellPolicy = String(form.sellPolicy || sellDefaults.policy || "safe").trim().toLowerCase();
  const creationTipSol = String(form.creationTipSol || creationDefaults.tipSol || form.jitoTipSol || "").trim();
  const creationPriorityFeeSol = String(form.creationPriorityFeeSol || form.priorityFeeSol || creationDefaults.priorityFeeSol || "").trim();
  const jitoEnabled = provider === "jito" || Number(creationTipSol || 0) > 0;
  const jitoTipLamports = creationTipSol ? solStringToLamports(creationTipSol, "creation tip") : 0n;
  const mode = String(form.mode || "").trim() || "regular";
  const launchpad = String(form.launchpad || appConfig.defaults.launchpad || "pump").trim() || "pump";
  const bundleJitoTip = jitoTipLamports > 0n && mode !== "agent-unlocked";
  const priorityFeeLamports = bundleJitoTip
    ? 0n
    : solStringToLamports(creationPriorityFeeSol, "priority fee");
  const isAgentComplete = mode === "agent-locked";
  const feeSplitEnabled = mode === "regular" && Boolean(form.feeSplitEnabled);
  const isAgentCustom = mode === "agent-custom";
  const isAgentUnlocked = mode === "agent-unlocked";
  const agentFeeRecipients = isAgentCustom ? parseRecipients(form.agentSplitRecipients, { allowAgent: true }) : [];
  const agentRecipient = agentFeeRecipients.find((entry) => entry.type === "agent");
  const automaticDevSellEnabled = form.automaticDevSellEnabled !== undefined
    ? Boolean(form.automaticDevSellEnabled)
    : Boolean(automaticDevSellDefaults.enabled);
  const automaticDevSellPercent = form.automaticDevSellPercent !== undefined
    ? Number(form.automaticDevSellPercent || 0)
    : Number(automaticDevSellDefaults.percent || 0);
  const automaticDevSellDelaySeconds = form.automaticDevSellDelaySeconds !== undefined
    ? Number(form.automaticDevSellDelaySeconds || 0)
    : Number(automaticDevSellDefaults.delaySeconds || 0);
  const postLaunchStrategy = Boolean(form.sniperEnabled)
    ? "snipe-own-launch"
    : (String(form.postLaunchStrategy || activePreset.postLaunchStrategy || "none").trim() || "none");
  const snipeBuyAmountSol = String(form.snipeBuyAmountSol || buyDefaults.snipeBuyAmountSol || "").trim();
  const sniperWallets = Array.isArray(form.sniperWallets)
    ? form.sniperWallets
      .filter((entry) => entry && entry.envKey)
      .map((entry) => ({
        envKey: String(entry.envKey).trim(),
        amountSol: String(entry.amountSol || "").trim(),
      }))
    : [];

  return {
    mode,
    launchpad,
    token: {
      name: form.name,
      symbol: form.symbol,
      description: String(form.description || "").trim(),
      website: String(form.website || "").trim(),
      twitter: String(form.twitter || "").trim(),
      telegram: String(form.telegram || "").trim(),
      uri: String(form.metadataUri || "").trim(),
      mayhemMode: Boolean(form.mayhemMode),
    },
    signer: {
      keypairFile: "",
      secretKey: "",
    },
    agent: {
      authority: isAgentComplete ? "" : String(form.agentAuthority || "").trim(),
      buybackBps: isAgentComplete
        ? 10_000
        : isAgentCustom
          ? agentRecipient ? Number(agentRecipient.shareBps) : buybackPercentToBps(form.buybackPercent)
          : buybackPercentToBps(form.buybackPercent),
      splitAgentInit: mode === "agent-custom" || mode === "agent-locked",
      feeReceiver: "",
      feeRecipients: isAgentComplete || isAgentUnlocked ? [] : agentFeeRecipients,
    },
    tx: {
      computeUnitLimit: Number(FIXED_COMPUTE_UNIT_LIMIT),
      computeUnitPriceMicroLamports: lamportsToPriorityFeeMicroLamports(priorityFeeLamports),
      jitoTipLamports: Number(jitoTipLamports),
      jitoTipAccount: jitoEnabled ? pickJitoTipAccount() : "",
      useDefaultLookupTables: true,
      lookupTables: [],
      dumpBase64: false,
      writeReport: true,
    },
    execution: {
      simulate: action === "simulate",
      send: action === "send",
      txFormat: "auto",
      commitment: "confirmed",
      skipPreflight: Boolean(form.skipPreflight),
      provider,
      policy,
      autoGas: true,
      autoMode: "launchAuto",
      priorityFeeSol: creationPriorityFeeSol,
      tipSol: creationTipSol,
      maxPriorityFeeSol: creationPriorityFeeSol,
      maxTipSol: creationTipSol,
      buyProvider,
      buyPolicy,
      buyAutoGas: true,
      buyAutoMode: "buyAuto",
      buyPriorityFeeSol: String(form.buyPriorityFeeSol || buyDefaults.priorityFeeSol || "").trim(),
      buyTipSol: String(form.buyTipSol || buyDefaults.tipSol || "").trim(),
      buySlippagePercent: String(form.buySlippagePercent || buyDefaults.slippagePercent || "").trim(),
      buyMaxPriorityFeeSol: String(form.buyPriorityFeeSol || buyDefaults.priorityFeeSol || "").trim(),
      buyMaxTipSol: String(form.buyTipSol || buyDefaults.tipSol || "").trim(),
      sellProvider,
      sellPolicy,
      sellPriorityFeeSol: String(form.sellPriorityFeeSol || sellDefaults.priorityFeeSol || "").trim(),
      sellTipSol: String(form.sellTipSol || sellDefaults.tipSol || "").trim(),
      sellSlippagePercent: String(form.sellSlippagePercent || sellDefaults.slippagePercent || "").trim(),
    },
    initialBuySol: devBuyMode === "sol" ? devBuyAmount : "",
    initialBuyTokens: devBuyMode === "tokens" ? devBuyAmount : "",
    feeSharing: {
      generateLaterSetup: feeSplitEnabled,
      recipients: feeSplitEnabled ? parseRecipients(form.feeSplitRecipients) : [],
    },
    creatorFee: {
      mode: mode === "cashback" ? "cashback" : isAgentComplete ? "agent-escrow" : "deployer",
      address: "",
      githubUsername: "",
      githubUserId: "",
    },
    presets: {
      activePresetId,
      selectedLaunchPresetId: activePresetId,
      selectedSniperPresetId: activePresetId,
    },
    sniper: {
      enabled: Boolean(form.sniperEnabled),
      wallets: sniperWallets,
    },
    postLaunch: {
      strategy: postLaunchStrategy,
      snipeOwnLaunch: {
        buyAmountSol: snipeBuyAmountSol,
      },
      automaticDevSell: {
        enabled: automaticDevSellEnabled,
        percent: automaticDevSellPercent,
        delaySeconds: automaticDevSellDelaySeconds,
      },
    },
    vanity: {
      privateKey: String(form.vanityPrivateKey || "").trim(),
    },
    imageFileName,
    imageLocalPath: imageFileName ? path.join(UPLOAD_DIR, imageFileName) : "",
    selectedWalletKey: selectedWalletKeyOrDefault(String(form.selectedWalletKey || "").trim()),
  };
}

async function handleRun(req, res) {
  try {
    const body = await readJsonBody(req);
    const action = String(body.action || "build").trim().toLowerCase();
    if (!["build", "simulate", "send"].includes(action)) {
      throw new Error(`Unsupported action: ${action}`);
    }
    const backendMode = getEngineBackendMode();
    const rawConfig = formToRawConfig(body.form || {}, action);
    for (const recipient of rawConfig.feeSharing.recipients) {
      if (!recipient.githubUsername || recipient.githubUserId) continue;
      const githubUser = await resolveGitHubUser(recipient.githubUsername);
      recipient.githubUsername = githubUser.username;
      recipient.githubUserId = githubUser.userId;
    }
    for (const recipient of rawConfig.agent.feeRecipients || []) {
      if (recipient.type === "agent" || !recipient.githubUsername || recipient.githubUserId) continue;
      const githubUser = await resolveGitHubUser(recipient.githubUsername);
      recipient.githubUsername = githubUser.username;
      recipient.githubUserId = githubUser.userId;
    }
    const upload = await uploadMetadataToPumpFun({
      name: rawConfig.token.name,
      symbol: rawConfig.token.symbol,
      description: rawConfig.token.description,
      website: rawConfig.token.website,
      twitter: rawConfig.token.twitter,
      telegram: rawConfig.token.telegram,
      imageFileName: rawConfig.imageFileName,
    });
    rawConfig.token.uri = upload.metadataUri;
    const enginePayload = await runEngineAction(action, {
      action,
      form: body.form || {},
      rawConfig,
    });
    return void sendJson(res, 200, {
      ...enginePayload,
      backend: backendMode,
      metadataUri: upload.metadataUri,
      signerSource: resolveSignerSource(rawConfig.selectedWalletKey),
    });
  } catch (error) {
    sendJson(res, 400, {
      ok: false,
      error: error.message,
    });
  }
}

async function handleEngineHealth(res) {
  try {
    const payload = await getEngineHealth();
    sendJson(res, 200, {
      ok: true,
      backend: getEngineBackendMode(),
      url: ENGINE_BASE_URL,
      engine: payload,
    });
  } catch (error) {
    sendJson(res, 503, {
      ok: false,
      backend: getEngineBackendMode(),
      url: ENGINE_BASE_URL,
      error: error.message,
    });
  }
}

async function handleStatus(reqUrl, res) {
  try {
    const selectedWalletKey = selectedWalletKeyOrDefault(reqUrl.searchParams.get("wallet"));
    const rawWallets = listSolanaEnvWallets();
    const rpcUrl = getRpcUrl({});
    const providers = getProviderAvailability();
    const launchpads = getLaunchpadAvailability();
    const appConfig = getUiConfig();

    if (!selectedWalletKey) {
      sendJson(res, 200, {
        ok: true,
        connected: false,
        rpcUrl,
        signerSource: resolveSignerSource(null),
        wallets: rawWallets,
        providers,
        launchpads,
        config: appConfig,
      });
      return;
    }

    const signer = loadSolanaWalletByEnvKey(selectedWalletKey);
    const connection = await getConnection();
    const wallets = await Promise.all(rawWallets.map(async (wallet) => {
      if (!wallet || wallet.error || !wallet.envKey) return wallet;
      try {
        const walletSigner = loadSolanaWalletByEnvKey(wallet.envKey);
        const lamports = await connection.getBalance(walletSigner.publicKey, "confirmed");
        const usd1Balance = await getSplTokenBalance(connection, walletSigner.publicKey, USD1_MINT);
        return {
          ...wallet,
          balanceLamports: lamports,
          balanceSol: lamports / LAMPORTS_PER_SOL,
          usd1Balance,
        };
      } catch (error) {
        return {
          ...wallet,
          balanceError: error.message,
        };
      }
    }));
    const balanceLamports = await connection.getBalance(signer.publicKey, "confirmed");
    const usd1Balance = await getSplTokenBalance(connection, signer.publicKey, USD1_MINT);
    sendJson(res, 200, {
      ok: true,
      connected: true,
      rpcUrl,
      wallet: signer.publicKey.toBase58(),
      signerSource: resolveSignerSource(selectedWalletKey),
      selectedWalletKey,
      wallets,
      providers,
      launchpads,
      config: appConfig,
      balanceLamports,
      balanceSol: balanceLamports / LAMPORTS_PER_SOL,
      usd1Balance,
    });
  } catch (error) {
    sendJson(res, 500, {
      ok: false,
      error: error.message,
    });
  }
}

async function handleSettingsGet(res) {
  sendJson(res, 200, buildSettingsPayload());
}

async function handleSettingsSave(req, res) {
  try {
    const body = await readJsonBody(req);
    const config = body && body.config ? body.config : createDefaultPersistentConfig();
    const nextConfig = normalizePersistentConfig(config);
    const savedPath = writePersistentConfig(__dirname, nextConfig);
    sendJson(res, 200, {
      ...buildSettingsPayload(),
      savedPath,
    });
  } catch (error) {
    sendJson(res, 400, {
      ok: false,
      error: error.message,
    });
  }
}

async function handleQuote(reqUrl, res) {
  try {
    const mode = String(reqUrl.searchParams.get("mode") || "").trim();
    const amount = String(reqUrl.searchParams.get("amount") || "").trim();
    if (!mode || !amount) {
      sendJson(res, 200, {
        ok: true,
        quote: null,
      });
      return;
    }

    const global = await getGlobalState();
    const quote = computeLaunchQuote(global, mode, amount);
    sendJson(res, 200, {
      ok: true,
      quote,
    });
  } catch (error) {
    sendJson(res, 400, {
      ok: false,
      error: error.message,
    });
  }
}

async function handleUploadImage(req, res) {
  try {
    const body = await readJsonBody(req);
    const dataUrl = String(body.dataUrl || "");
    const originalName = String(body.filename || "image");
    const match = dataUrl.match(/^data:([^;]+);base64,(.+)$/);
    if (!match) {
      throw new Error("Invalid image payload.");
    }

    const mimeType = match[1];
    const extension = ALLOWED_IMAGE_TYPES.get(mimeType);
    if (!extension) {
      throw new Error("Only png, jpg, webp, and gif images are supported.");
    }

    const safeBaseName = path
      .basename(originalName, path.extname(originalName))
      .replace(/[^a-zA-Z0-9_-]+/g, "-")
      .replace(/^-+|-+$/g, "") || "image";

    ensureDir(UPLOAD_DIR);
    const fileName = `${timestampSlug()}-${safeBaseName}${extension}`;
    const filePath = path.join(UPLOAD_DIR, fileName);
    fs.writeFileSync(filePath, Buffer.from(match[2], "base64"));
    const library = readImageLibrary();
    const record = createImageRecord({ fileName, originalName });
    library.images.unshift(record);
    writeImageLibrary(library);

    sendJson(res, 200, {
      ok: true,
      ...serializeImageRecord(record),
    });
  } catch (error) {
    sendJson(res, 400, {
      ok: false,
      error: error.message,
    });
  }
}

async function handleVampImport(req, res) {
  try {
    const body = await readJsonBody(req);
    const contractAddress = String(body.contractAddress || "").trim();
    if (!contractAddress) {
      throw new Error("Contract address is required.");
    }

    let mintPublicKey;
    try {
      mintPublicKey = new PublicKey(contractAddress);
    } catch (_error) {
      throw new Error("Invalid Solana contract address.");
    }

    const imported = await fetchImportedTokenMetadata(mintPublicKey.toBase58());
    let image = null;
    let imageWarning = "";
    if (imported.imageUrl) {
      try {
        image = await importRemoteImageToLibrary(imported.imageUrl, {
          originalName: `${imported.symbol || imported.name || mintPublicKey.toBase58()}-vamp`,
          recordName: imported.name || imported.symbol || "Imported token image",
        });
      } catch (error) {
        imageWarning = error.message;
      }
    }

    sendJson(res, 200, {
      ok: true,
      source: imported.source || "metadata",
      token: {
        name: imported.name || "",
        symbol: imported.symbol || "",
        description: imported.description || "",
        website: imported.website || "",
        twitter: imported.twitter || "",
        telegram: imported.telegram || "",
      },
      image,
      warning: imageWarning,
    });
  } catch (error) {
    sendJson(res, 400, {
      ok: false,
      error: error.message,
    });
  }
}

async function handleImagesList(reqUrl, res) {
  try {
    const search = reqUrl.searchParams.get("search") || "";
    const category = reqUrl.searchParams.get("category") || "";
    const favoritesOnly = reqUrl.searchParams.get("favoritesOnly") === "true";
    sendJson(res, 200, buildImageLibraryPayload({ search, category, favoritesOnly }));
  } catch (error) {
    sendJson(res, 400, {
      ok: false,
      error: error.message,
    });
  }
}

async function handleImageUpdate(req, res) {
  try {
    const body = await readJsonBody(req);
    const id = String(body.id || "").trim();
    if (!id) throw new Error("Image id is required.");
    const library = readImageLibrary();
    const record = library.images.find((entry) => entry.id === id);
    if (!record) throw new Error("Image not found.");
    if (body.name !== undefined) record.name = String(body.name || "").trim() || record.name;
    if (body.tags !== undefined) {
      const tags = Array.isArray(body.tags)
        ? body.tags
        : String(body.tags || "").split(",").map((tag) => tag.trim()).filter(Boolean);
      record.tags = tags.slice(0, 24);
    }
    if (body.category !== undefined) {
      record.category = String(body.category || "").trim();
      if (record.category && !library.categories.includes(record.category)) {
        library.categories.push(record.category);
        library.categories.sort((a, b) => a.localeCompare(b));
      }
    }
    if (body.isFavorite !== undefined) record.isFavorite = Boolean(body.isFavorite);
    record.updatedAt = Date.now();
    writeImageLibrary(library);
    sendJson(res, 200, {
      ...buildImageLibraryPayload(),
      image: serializeImageRecord(record),
    });
  } catch (error) {
    sendJson(res, 400, {
      ok: false,
      error: error.message,
    });
  }
}

async function handleImageCategoryCreate(req, res) {
  try {
    const body = await readJsonBody(req);
    const name = String(body.name || "").trim().replace(/\s+/g, " ");
    if (!name) throw new Error("Category name is required.");
    if (name.length > 32) throw new Error("Category name must be 32 characters or fewer.");
    const library = readImageLibrary();
    const existing = library.categories.find((entry) => entry.toLowerCase() === name.toLowerCase());
    if (!existing) {
      library.categories.push(name);
      library.categories.sort((a, b) => a.localeCompare(b));
      writeImageLibrary(library);
    }
    sendJson(res, 200, {
      ...buildImageLibraryPayload(),
      category: existing || name,
    });
  } catch (error) {
    sendJson(res, 400, {
      ok: false,
      error: error.message,
    });
  }
}

async function handleImageDelete(req, res) {
  try {
    const body = await readJsonBody(req);
    const id = String(body.id || "").trim();
    if (!id) throw new Error("Image id is required.");
    const library = readImageLibrary();
    const index = library.images.findIndex((entry) => entry.id === id);
    if (index < 0) throw new Error("Image not found.");
    const [removed] = library.images.splice(index, 1);
    const filePath = path.join(UPLOAD_DIR, removed.fileName);
    if (fs.existsSync(filePath)) {
      fs.unlinkSync(filePath);
    }
    writeImageLibrary(library);
    sendJson(res, 200, buildImageLibraryPayload());
  } catch (error) {
    sendJson(res, 400, {
      ok: false,
      error: error.message,
    });
  }
}

function handleRequest(req, res) {
  const url = new URL(req.url, `http://${req.headers.host || `${HOST}:${PORT}`}`);

  if (req.method === "GET" && (url.pathname === "/" || url.pathname === "/index.html")) {
    const body = renderIndexHtml();
    res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    res.end(body);
    return;
  }

  if (req.method === "GET" && url.pathname === "/app.js") {
    return sendFile(res, path.join(STATIC_DIR, "app.js"), "application/javascript; charset=utf-8");
  }

  if (req.method === "GET" && url.pathname === "/styles.css") {
    return sendFile(res, path.join(STATIC_DIR, "styles.css"), "text/css; charset=utf-8");
  }

  if (req.method === "GET" && url.pathname === "/solana-mark.png") {
    return sendFile(res, path.join(STATIC_DIR, "solana-mark.png"), "image/png");
  }

  if (req.method === "GET" && url.pathname === "/usd1-mark.png") {
    return sendFile(res, path.join(STATIC_DIR, "usd1-mark.png"), "image/png");
  }

  if (req.method === "GET" && url.pathname === "/pump-mark.png") {
    return sendFile(res, path.join(STATIC_DIR, "pump-mark.png"), "image/png");
  }

  if (req.method === "GET" && url.pathname === "/bonk-mark.png") {
    return sendFile(res, path.join(STATIC_DIR, "bonk-mark.png"), "image/png");
  }

  if (req.method === "GET" && url.pathname === "/bagsapp-mark.png") {
    return sendFile(res, path.join(STATIC_DIR, "bagsapp-mark.png"), "image/png");
  }

  if (req.method === "GET" && url.pathname === "/api/status") {
    return void handleStatus(url, res);
  }

  if (req.method === "GET" && url.pathname === "/api/settings") {
    return void handleSettingsGet(res);
  }

  if (req.method === "GET" && url.pathname === "/api/engine/health") {
    return void handleEngineHealth(res);
  }

  if (req.method === "GET" && url.pathname === "/api/quote") {
    return void handleQuote(url, res);
  }

  if (req.method === "GET" && url.pathname === "/api/images") {
    return void handleImagesList(url, res);
  }

  if (req.method === "GET" && url.pathname.startsWith("/uploads/")) {
    const fileName = path.basename(url.pathname.slice("/uploads/".length));
    return sendFile(res, path.join(UPLOAD_DIR, fileName), guessContentType(fileName));
  }

  if (req.method === "POST" && url.pathname === "/api/run") {
    return void handleRun(req, res);
  }

  if (req.method === "POST" && url.pathname === "/api/settings") {
    return void handleSettingsSave(req, res);
  }

  if (req.method === "POST" && url.pathname === "/api/upload-image") {
    return void handleUploadImage(req, res);
  }

  if (req.method === "POST" && url.pathname === "/api/vamp") {
    return void handleVampImport(req, res);
  }

  if (req.method === "POST" && url.pathname === "/api/images/update") {
    return void handleImageUpdate(req, res);
  }

  if (req.method === "POST" && url.pathname === "/api/images/categories") {
    return void handleImageCategoryCreate(req, res);
  }

  if (req.method === "POST" && url.pathname === "/api/images/delete") {
    return void handleImageDelete(req, res);
  }

  sendJson(res, 404, { error: "Not found" });
}

function main() {
  const server = http.createServer(handleRequest);
  server.listen(PORT, HOST, () => {
    console.log(`LaunchDeck UI running at http://${HOST}:${PORT}`);
  });
}

if (require.main === module) {
  main();
}

module.exports = {
  buildSettingsPayload,
  formToRawConfig,
  handleRun,
  main,
};
