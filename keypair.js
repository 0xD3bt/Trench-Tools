"use strict";

const fs = require("fs");
const path = require("path");
const bs58 = require("bs58").default;
const { Keypair, PublicKey } = require("@solana/web3.js");

function isSolanaWalletEnvKey(key) {
  return /^SOLANA_PRIVATE_KEY\d*$/.test(String(key || ""));
}

function readKeypairBytes(raw) {
  const trimmed = raw.trim();
  if (!trimmed) {
    throw new Error("Keypair value was empty.");
  }

  if (trimmed.startsWith("[")) {
    const parsed = JSON.parse(trimmed);
    return Uint8Array.from(parsed);
  }

  return bs58.decode(trimmed);
}

function loadKeypairFromFile(filePath) {
  const resolved = path.isAbsolute(filePath) ? filePath : path.resolve(filePath);
  const raw = fs.readFileSync(resolved, "utf8");
  return Keypair.fromSecretKey(readKeypairBytes(raw));
}

function loadKeypairFromEnvOrArgs(args = {}) {
  if (args["keypair-file"]) {
    return loadKeypairFromFile(args["keypair-file"]);
  }

  if (args["secret-key"]) {
    return Keypair.fromSecretKey(readKeypairBytes(args["secret-key"]));
  }

  if (process.env.SOLANA_KEYPAIR_PATH) {
    return loadKeypairFromFile(process.env.SOLANA_KEYPAIR_PATH);
  }

  if (process.env.SOLANA_PRIVATE_KEY) {
    return Keypair.fromSecretKey(readKeypairBytes(process.env.SOLANA_PRIVATE_KEY));
  }

  return null;
}

function listSolanaEnvWallets() {
  return Object.keys(process.env)
    .filter(isSolanaWalletEnvKey)
    .sort((a, b) => {
      const aNum = Number(a.replace("SOLANA_PRIVATE_KEY", "") || "1");
      const bNum = Number(b.replace("SOLANA_PRIVATE_KEY", "") || "1");
      return aNum - bNum;
    })
    .map((envKey) => {
      const secret = process.env[envKey];
      try {
        const keypair = Keypair.fromSecretKey(readKeypairBytes(secret));
        return {
          envKey,
          publicKey: keypair.publicKey.toBase58(),
        };
      } catch (error) {
        return {
          envKey,
          publicKey: null,
          error: error.message,
        };
      }
    });
}

function loadSolanaWalletByEnvKey(envKey) {
  if (!isSolanaWalletEnvKey(envKey)) {
    throw new Error(`Invalid Solana wallet env key: ${envKey}`);
  }
  const secret = process.env[envKey];
  if (!secret) {
    throw new Error(`Missing env value for ${envKey}`);
  }
  return Keypair.fromSecretKey(readKeypairBytes(secret));
}

function loadOptionalPublicKey(value, label) {
  if (!value) return null;
  try {
    return new PublicKey(value);
  } catch (error) {
    throw new Error(`Invalid ${label}: ${value}`);
  }
}

module.exports = {
  isSolanaWalletEnvKey,
  listSolanaEnvWallets,
  loadKeypairFromEnvOrArgs,
  loadOptionalPublicKey,
  loadSolanaWalletByEnvKey,
  loadKeypairFromFile,
};
