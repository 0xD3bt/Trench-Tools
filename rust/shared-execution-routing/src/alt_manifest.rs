use serde::{Deserialize, Serialize};

pub const SHARED_SUPER_LOOKUP_TABLE: &str = "7CaMLcAuSskoeN7HoRwZjsSthU8sMwKqxtXkyMiMjuc";
pub const RAYDIUM_AMM_V4_PROGRAM_ID: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
pub const RAYDIUM_AMM_V4_AUTHORITY: &str = "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1";
pub const OPENBOOK_DEX_PROGRAM_ID: &str = "srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX";
pub const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
pub const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
pub const JITODONTFRONT_ACCOUNT: &str = "jitodontfront111111111111111111111111111111";
pub const RENT_SYSVAR_ID: &str = "SysvarRent111111111111111111111111111111111";

pub const PUMP_APR28_FEE_RECIPIENTS: [&str; 8] = [
    "5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD",
    "9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7",
    "GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL",
    "3BpXnfJaUTiwXnJNe7Ej1rcbzqTTQUvLShZaWazebsVR",
    "5cjcW9wExnJJiqgLjq7DEG75Pm6JBgE1hNv4B2vHXUW6",
    "EHAAiTxcdDwQ3U4bU6YcMsQGaekdzLS3B5SmYo46kJtL",
    "5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD",
    "A7hAgCzFw14fejgCp387JUJRMNyz4j89JKnhtKU8piqW",
];

pub const SELECTED_PUMP_APR28_FEE_RECIPIENT: &str = PUMP_APR28_FEE_RECIPIENTS[0];
pub const SELECTED_PUMP_APR28_WSOL_FEE_RECIPIENT_ATA: &str =
    "HjQjngTDqoHE6aaGhUqfz9aQ7WZcBRjy5xB8PScLSr8i";
pub const SELECTED_PUMP_APR28_USDC_FEE_RECIPIENT_ATA: &str =
    "6oCkp6gpyjxVTeL6ahMYcekN2x2pzt1KY8g2LqemaTNE";
pub const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub const PUMP_AMM_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
pub const PUMP_FEE_PROGRAM_ID: &str = "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ";
pub const PUMP_GLOBAL: &str = "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf";
pub const PUMP_GLOBAL_VOLUME_ACCUMULATOR: &str = "Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y";
pub const PUMP_FEE_CONFIG: &str = "8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt";
pub const WRAPPER_FEE_VAULT_WSOL_ATA: &str = "2HLoA8PQuxqUfNDVa6kCL8CZ1FkDMcqZZSE3HDEpKqSZ";
pub const ORCA_WHIRLPOOL_PROGRAM_ID: &str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";
pub const RAYDIUM_CLMM_PROGRAM_ID: &str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";
pub const METEORA_DBC_PROGRAM_ID: &str = "dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN";
pub const METEORA_DBC_POOL_AUTHORITY: &str = "FhVo3mqL8PW5pH5U2CN4XE33DokiyZnUwuGpH2hmHLuM";
pub const METEORA_DBC_EVENT_AUTHORITY: &str = "8Ks12pbrD6PXxfty1hVQiE9sc289zgU1zHkvXhrSdriF";
pub const METEORA_DAMM_V2_PROGRAM_ID: &str = "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG";
pub const METEORA_DAMM_V2_POOL_AUTHORITY: &str = "HLnpSz9h2S4hiLQ43rnSD9XkcUThA7B8hQMKmDaiTLcC";
pub const METEORA_DAMM_V2_EVENT_AUTHORITY: &str = "3rmHSu74h1ZcmAisVcWerTCiRDQbUrBKmcwptYGjHfet";
pub const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
pub const USDT_MINT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
pub const USD1_MINT: &str = "USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB";
pub const RAYDIUM_SOL_USDC_POOL: &str = "3ucNos4NbumPLZNWztqGHNFFgkHeRMBQAVemeeomsUxv";
pub const RAYDIUM_SOL_USDC_AMM_CONFIG: &str = "3h2e43PunVA5K34vwKCLHWhZF4aZpyaC9RmxvshGAQpL";
pub const RAYDIUM_SOL_USDC_VAULT_A: &str = "4ct7br2vTPzfdmY3S5HLtTxcGSBfn6pnw98hsS6v359A";
pub const RAYDIUM_SOL_USDC_VAULT_B: &str = "5it83u57VRrVgc51oNV19TTmAJuffPx5GtGwQr7gQNUo";
pub const RAYDIUM_SOL_USDC_OBSERVATION: &str = "3Y695CuQ8AP4anbwAqiEBeQF9KxqHFr8piEwvw3UePnQ";
pub const RAYDIUM_SOL_USDC_BITMAP_EXTENSION: &str = "4NFvUKqknMpoe6CWTzK758B8ojVLzURL5pC6MtiaJ8TQ";
pub const RAYDIUM_SOL_USDC_TICK_ARRAY_M23820: &str = "8YLwB5krzY8DjYv94QSFkESXfFZJnoF68e8eQTyFDUb6";
pub const RAYDIUM_SOL_USDC_TICK_ARRAY_M23760: &str = "GhGf4xRPRPQ7QzGsRRew7xwFjK7CTrX2XckXBktunPjK";
pub const RAYDIUM_SOL_USDC_TICK_ARRAY_M23700: &str = "BKNCEAtCiYaCLJGc3bQocH5YPSvm9u9mSJD8qGJ2mMjb";
pub const RAYDIUM_SOL_USDC_TICK_ARRAY_M23640: &str = "E33N1jaRJuJAubsTTJDfVYQUKwYJ6T8DMxLcC19XR41R";
pub const RAYDIUM_SOL_USDC_TICK_ARRAY_M23580: &str = "3QmSmfWJGcvRMfCWzDcnRTPvJzho6fbKrGFjvEA2XZsq";
pub const RAYDIUM_SOL_USDT_POOL: &str = "3nMFwZXwY1s1M5s8vYAHqd4wGs4iSxXE4LRoUMMYqEgF";
pub const RAYDIUM_SOL_USDT_AMM_CONFIG: &str = "9iFER3bpjf1PTTCQCfTRu17EJgvsxo9pVyA9QWwEuX4x";
pub const RAYDIUM_SOL_USDT_VAULT_A: &str = "AbcuyoPeYnddzFoFQudsiFka8qd6tTwvLgxwtpTKTpKC";
pub const RAYDIUM_SOL_USDT_VAULT_B: &str = "2n6fxuD6PA5NYgEnXXYMh2iWD1JBJ1LGf76kFJAayZmX";
pub const RAYDIUM_SOL_USDT_OBSERVATION: &str = "Cqb16WaM7dDDP8koxYASDJLWgan4STDB1R3LiSH8r3GR";
pub const RAYDIUM_SOL_USDT_BITMAP_EXTENSION: &str = "2ncinnTcJxbZ1nUHavBVJ3Ap3R4CE7p2LJ6Jtpd1vLzd";
pub const RAYDIUM_SOL_USDT_TICK_ARRAY_M23820: &str = "J4jBy6ezcsvgtYo6StM57LbfofdhTamQ35fcZ7qbUkF4";
pub const RAYDIUM_SOL_USDT_TICK_ARRAY_M23760: &str = "AX7qtgTMVjuTLSjRveuUC7QV2tfbQrxxsMqQZHWVe8SA";
pub const RAYDIUM_SOL_USDT_TICK_ARRAY_M23700: &str = "8t5n8TmSjhTnLir1FMh8R3f1sQTD2XfZ3EP9rm1sN7K7";
pub const RAYDIUM_SOL_USDT_TICK_ARRAY_M23640: &str = "tKPC2pF9pozySybuYEJSS4TNTnBuukrVEhd5JbPDbVz";
pub const RAYDIUM_SOL_USDT_TICK_ARRAY_M23580: &str = "HuigvPoXF6R8iejoHcbkGBrTQnNXufUwmXfPeAQiBzcD";
pub const RAYDIUM_SOL_USD1_POOL: &str = "AQAGYQsdU853WAKhXM79CgNdoyhrRwXvYHX6qrDyC1FS";
pub const RAYDIUM_SOL_USD1_AMM_CONFIG: &str = "E64NGkDLLCdQ2yFNPcavaKptrEgmiQaNykUuLC1Qgwyp";
pub const RAYDIUM_SOL_USD1_VAULT_A: &str = "5QpMZ6MuyKjg8Qa1X8gM5G3YMsd43rpHb2iQ6hdcRM7m";
pub const RAYDIUM_SOL_USD1_VAULT_B: &str = "DHY2efKhMcZyAgmPw82C2Gez1e98Ab7oWcXfxz9frUCr";
pub const RAYDIUM_SOL_USD1_OBSERVATION: &str = "2s8VC2vpZcoUCbvEqwUagvdyHfUeWYQZuKV376acx6iF";
pub const RAYDIUM_SOL_USD1_BITMAP_EXTENSION: &str = "HKcNrRDTFuXVTyMCkp5h3eU4cyqbzteAW1UR7m2CWbVs";
pub const RAYDIUM_SOL_USD1_TICK_ARRAY_M32400: &str = "8Ndgz7gY9zV2nAnMnPB4wgvmU8aT7FrxLdyqe6GD5zGv";
pub const RAYDIUM_SOL_USD1_TICK_ARRAY_M28800: &str = "9bcapqGioeAGybVSbtYoSZR23qZESNbToTV5C2QUJMqZ";
pub const RAYDIUM_SOL_USD1_TICK_ARRAY_M25200: &str = "DTgyhdzARWt8fiFrA5E2ECTdR5U3rpGho2AmaPWsieWg";
pub const RAYDIUM_SOL_USD1_TICK_ARRAY_M21600: &str = "B9MydCwPvBL6hWGR4MLUeUmzRC9y9SrNaJtubWuy1GSM";
pub const RAYDIUM_SOL_USD1_TICK_ARRAY_M18000: &str = "9JtS7quchTtWtprMs614GtpaKeyirgRvX84j9Zt1r1cU";
pub const RAYDIUM_USDC_USD1_POOL: &str = "BCDdHonby65iduz3Ev3c9v5XjNkzyu5e56KRFHpBM4T9";
pub const RAYDIUM_USDC_USD1_AMM_CONFIG: &str = "9iFER3bpjf1PTTCQCfTRu17EJgvsxo9pVyA9QWwEuX4x";
pub const RAYDIUM_USDC_USD1_VAULT_A: &str = "3PHtS2UAJEHq9zrJthFaSTmcTeccv9PpJEqvN6ZQyBFJ";
pub const RAYDIUM_USDC_USD1_VAULT_B: &str = "fWK4NW83YPL2rgqLHpCMYzTiXu2XW23pmxW8Nq3MrWX";
pub const RAYDIUM_USDC_USD1_OBSERVATION: &str = "5tyngLzFdHR8rezCQAJ8MMgi4XNhaciD196VVn4gC94H";
pub const RAYDIUM_USDC_USD1_BITMAP_EXTENSION: &str = "Eg5nBe8HXTvSCx4MWAwr1L3oNet5ZRFeEpou4HcS9ACR";
pub const RAYDIUM_USDC_USD1_TICK_ARRAY_M120: &str = "ByeNCwPrRqgQjEGEV5yEvW7gfkp7Pzv6voRXrAXiFiCy";
pub const RAYDIUM_USDC_USD1_TICK_ARRAY_M60: &str = "AhrM6y4pH2HhBCUaRfEMH7kYezf9KuGKTewG5mnryXtm";
pub const RAYDIUM_USDC_USD1_TICK_ARRAY_0: &str = "7BPA55AeFV5ZN85SEoUx7pjpNGH83R1GR5f4mxPKq2hS";
pub const RAYDIUM_USDC_USD1_TICK_ARRAY_60: &str = "9MEiBmimYQyGrHtsrGiDzGtgKHBRRPmzQGQoeB1nStFx";
pub const RAYDIUM_USDC_USDT_POOL: &str = "BZtgQEyS6eXUXicYPHecYQ7PybqodXQMvkjUbP4R8mUU";
pub const RAYDIUM_USDC_USDT_AMM_CONFIG: &str = "9iFER3bpjf1PTTCQCfTRu17EJgvsxo9pVyA9QWwEuX4x";
pub const RAYDIUM_USDC_USDT_VAULT_A: &str = "4iJQGzZpys4N13rGXbsB3NqehPkrgrcmDUEcLw7D6GKL";
pub const RAYDIUM_USDC_USDT_VAULT_B: &str = "7NQjToK5NenQZvhPRc1nC1URuKJ4nuLxGPdTbpTiz9EN";
pub const RAYDIUM_USDC_USDT_OBSERVATION: &str = "3YZE41e4GKm9Uqvx6H4QA1FdyxT15nEjUP8LbvdiDF2t";
pub const RAYDIUM_USDC_USDT_BITMAP_EXTENSION: &str = "HSJS4BrYCZq9DmCNFNfctqq9o2sfTY5YQiy9n4W8djcB";
pub const RAYDIUM_USDC_USDT_TICK_ARRAY_M240: &str = "4dyzCCdBV1fTB4GYk8yfeSCnM4TXhEULAc2x6syEw6Aa";
pub const RAYDIUM_USDC_USDT_TICK_ARRAY_M120: &str = "FULWc1hWdGMBGSB4Ut3QZBCU74muZmLmM9z9UqheWoUw";
pub const RAYDIUM_USDC_USDT_TICK_ARRAY_M60: &str = "2PufrkkvNj7nF32GRvoR1DEmXs8F99gYzyzCQtmbndxd";
pub const RAYDIUM_USDC_USDT_TICK_ARRAY_0: &str = "2CntbsRKrr4an5zekGb8WZyPyPzbXv9R2CErrRQSQVo2";
pub const RAYDIUM_USDC_USDT_TICK_ARRAY_60: &str = "Cxhs239pddZBnZc4adCjjfqUMRWG2JjGoRPrBvPmvGGa";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AltManifestEntry {
    pub address: String,
    pub family: String,
    pub label: String,
    pub reason: String,
    pub required: bool,
}

impl AltManifestEntry {
    pub fn required(address: impl Into<String>, family: &str, label: &str, reason: &str) -> Self {
        Self {
            address: address.into(),
            family: family.to_string(),
            label: label.to_string(),
            reason: reason.to_string(),
            required: true,
        }
    }

    pub fn optional(address: impl Into<String>, family: &str, label: &str, reason: &str) -> Self {
        Self {
            address: address.into(),
            family: family.to_string(),
            label: label.to_string(),
            reason: reason.to_string(),
            required: false,
        }
    }
}

pub fn pump_apr28_fee_recipient_manifest_entries() -> Vec<AltManifestEntry> {
    let mut entries = PUMP_APR28_FEE_RECIPIENTS
        .iter()
        .map(|address| {
            AltManifestEntry::required(
                *address,
                "pump-upgrade",
                "pump-apr28-common-fee-recipient",
                "Pump April 28 bonding-curve and AMM instructions emit one common fee recipient",
            )
        })
        .collect::<Vec<_>>();
    entries.push(AltManifestEntry::required(
        SELECTED_PUMP_APR28_WSOL_FEE_RECIPIENT_ATA,
        "pump-upgrade",
        "pump-apr28-wsol-fee-recipient-ata",
        "Execution-engine Pump AMM WSOL quote routes emit the selected April 28 fee-recipient ATA",
    ));
    entries.push(AltManifestEntry::required(
        SELECTED_PUMP_APR28_USDC_FEE_RECIPIENT_ATA,
        "pump-upgrade",
        "pump-apr28-usdc-fee-recipient-ata",
        "Pump v2/USDC routes emit the selected April 28 USDC fee-recipient ATA",
    ));
    entries.extend([
        AltManifestEntry::required(
            PUMP_PROGRAM_ID,
            "pump-upgrade",
            "pump-program",
            "Pump bonding v2 routes invoke the Pump program",
        ),
        AltManifestEntry::required(
            PUMP_AMM_PROGRAM_ID,
            "pump-upgrade",
            "pump-amm-program",
            "Migrated Pump AMM routes invoke the Pump AMM program",
        ),
        AltManifestEntry::required(
            PUMP_FEE_PROGRAM_ID,
            "pump-upgrade",
            "pump-fee-program",
            "Pump v2 and AMM routes pass the Pump fee program",
        ),
        AltManifestEntry::required(
            PUMP_GLOBAL,
            "pump-upgrade",
            "pump-global",
            "Pump bonding v2 routes pass the global account",
        ),
        AltManifestEntry::required(
            PUMP_GLOBAL_VOLUME_ACCUMULATOR,
            "pump-upgrade",
            "pump-global-volume-accumulator",
            "Pump bonding v2 buy routes pass the global volume accumulator",
        ),
        AltManifestEntry::required(
            PUMP_FEE_CONFIG,
            "pump-upgrade",
            "pump-fee-config",
            "Pump bonding v2 routes pass the Pump fee config PDA",
        ),
    ]);
    entries
}

pub fn wrapper_alt_manifest_entries() -> Vec<AltManifestEntry> {
    vec![
        AltManifestEntry::required(
            WRAPPER_FEE_VAULT_WSOL_ATA,
            "wrapper",
            "wrapper-fee-vault-wsol-ata",
            "Wrapper SolOut routes can carry the fee-vault WSOL ATA as a fixed account",
        ),
        AltManifestEntry::optional(
            ORCA_WHIRLPOOL_PROGRAM_ID,
            "trusted-stable",
            "orca-whirlpool-program",
            "Trusted stable SOL/USDC route can invoke the sealed Orca Whirlpool program",
        ),
    ]
}

pub fn trusted_stable_alt_manifest_entries() -> Vec<AltManifestEntry> {
    vec![
        AltManifestEntry::required(
            USDT_MINT,
            "trusted-stable",
            "usdt-mint",
            "Trusted stable SOL/USDT routes pass the USDT mint",
        ),
        AltManifestEntry::required(
            USD1_MINT,
            "trusted-stable",
            "usd1-mint",
            "Trusted stable USD1 routes pass the USD1 mint",
        ),
        AltManifestEntry::required(
            RAYDIUM_CLMM_PROGRAM_ID,
            "trusted-stable",
            "raydium-clmm-program",
            "Trusted stable Raydium CLMM routes invoke the sealed CLMM program",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDT_POOL,
            "trusted-stable",
            "raydium-sol-usdt-pool",
            "Trusted stable SOL/USDT swaps use this pinned Raydium CLMM pool",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDT_AMM_CONFIG,
            "trusted-stable",
            "raydium-sol-usdt-amm-config",
            "The pinned Raydium SOL/USDT CLMM swap passes this AMM config",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDT_VAULT_A,
            "trusted-stable",
            "raydium-sol-usdt-vault-a",
            "The pinned Raydium SOL/USDT CLMM swap passes the WSOL vault",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDT_VAULT_B,
            "trusted-stable",
            "raydium-sol-usdt-vault-b",
            "The pinned Raydium SOL/USDT CLMM swap passes the USDT vault",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDT_OBSERVATION,
            "trusted-stable",
            "raydium-sol-usdt-observation",
            "The pinned Raydium SOL/USDT CLMM swap passes this observation account",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDT_BITMAP_EXTENSION,
            "trusted-stable",
            "raydium-sol-usdt-bitmap-extension",
            "The pinned Raydium SOL/USDT CLMM swap passes this bitmap extension",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDT_TICK_ARRAY_M23820,
            "trusted-stable",
            "raydium-sol-usdt-tick-array--23820",
            "Nearby pinned Raydium SOL/USDT CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDT_TICK_ARRAY_M23760,
            "trusted-stable",
            "raydium-sol-usdt-tick-array--23760",
            "Nearby pinned Raydium SOL/USDT CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDT_TICK_ARRAY_M23700,
            "trusted-stable",
            "raydium-sol-usdt-tick-array--23700",
            "Current pinned Raydium SOL/USDT CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDT_TICK_ARRAY_M23640,
            "trusted-stable",
            "raydium-sol-usdt-tick-array--23640",
            "Nearby pinned Raydium SOL/USDT CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDT_TICK_ARRAY_M23580,
            "trusted-stable",
            "raydium-sol-usdt-tick-array--23580",
            "Nearby pinned Raydium SOL/USDT CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USD1_POOL,
            "trusted-stable",
            "raydium-sol-usd1-pool",
            "Trusted stable SOL/USD1 swaps use this pinned Raydium CLMM pool",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USD1_AMM_CONFIG,
            "trusted-stable",
            "raydium-sol-usd1-amm-config",
            "The pinned Raydium SOL/USD1 CLMM swap passes this AMM config",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USD1_VAULT_A,
            "trusted-stable",
            "raydium-sol-usd1-vault-a",
            "The pinned Raydium SOL/USD1 CLMM swap passes the WSOL vault",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USD1_VAULT_B,
            "trusted-stable",
            "raydium-sol-usd1-vault-b",
            "The pinned Raydium SOL/USD1 CLMM swap passes the USD1 vault",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USD1_OBSERVATION,
            "trusted-stable",
            "raydium-sol-usd1-observation",
            "The pinned Raydium SOL/USD1 CLMM swap passes this observation account",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USD1_BITMAP_EXTENSION,
            "trusted-stable",
            "raydium-sol-usd1-bitmap-extension",
            "The pinned Raydium SOL/USD1 CLMM swap passes this bitmap extension",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USD1_TICK_ARRAY_M32400,
            "trusted-stable",
            "raydium-sol-usd1-tick-array--32400",
            "Nearby pinned Raydium SOL/USD1 CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USD1_TICK_ARRAY_M28800,
            "trusted-stable",
            "raydium-sol-usd1-tick-array--28800",
            "Nearby pinned Raydium SOL/USD1 CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USD1_TICK_ARRAY_M25200,
            "trusted-stable",
            "raydium-sol-usd1-tick-array--25200",
            "Current pinned Raydium SOL/USD1 CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USD1_TICK_ARRAY_M21600,
            "trusted-stable",
            "raydium-sol-usd1-tick-array--21600",
            "Nearby pinned Raydium SOL/USD1 CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USD1_TICK_ARRAY_M18000,
            "trusted-stable",
            "raydium-sol-usd1-tick-array--18000",
            "Nearby pinned Raydium SOL/USD1 CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USD1_POOL,
            "trusted-stable",
            "raydium-usdc-usd1-pool",
            "Trusted stable USDC/USD1 swaps use this pinned Raydium CLMM pool",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USD1_AMM_CONFIG,
            "trusted-stable",
            "raydium-usdc-usd1-amm-config",
            "The pinned Raydium USDC/USD1 CLMM swap passes this AMM config",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USD1_VAULT_A,
            "trusted-stable",
            "raydium-usdc-usd1-vault-a",
            "The pinned Raydium USDC/USD1 CLMM swap passes the USD1 vault",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USD1_VAULT_B,
            "trusted-stable",
            "raydium-usdc-usd1-vault-b",
            "The pinned Raydium USDC/USD1 CLMM swap passes the USDC vault",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USD1_OBSERVATION,
            "trusted-stable",
            "raydium-usdc-usd1-observation",
            "The pinned Raydium USDC/USD1 CLMM swap passes this observation account",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USD1_BITMAP_EXTENSION,
            "trusted-stable",
            "raydium-usdc-usd1-bitmap-extension",
            "The pinned Raydium USDC/USD1 CLMM swap passes this bitmap extension",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USD1_TICK_ARRAY_M120,
            "trusted-stable",
            "raydium-usdc-usd1-tick-array--120",
            "Nearby pinned Raydium USDC/USD1 CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USD1_TICK_ARRAY_M60,
            "trusted-stable",
            "raydium-usdc-usd1-tick-array--60",
            "Current pinned Raydium USDC/USD1 CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USD1_TICK_ARRAY_0,
            "trusted-stable",
            "raydium-usdc-usd1-tick-array-0",
            "Nearby pinned Raydium USDC/USD1 CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USD1_TICK_ARRAY_60,
            "trusted-stable",
            "raydium-usdc-usd1-tick-array-60",
            "Nearby pinned Raydium USDC/USD1 CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USDT_POOL,
            "trusted-stable",
            "raydium-usdc-usdt-pool",
            "Trusted stable USDC/USDT swaps use this pinned Raydium CLMM pool",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USDT_AMM_CONFIG,
            "trusted-stable",
            "raydium-usdc-usdt-amm-config",
            "The pinned Raydium USDC/USDT CLMM swap passes this AMM config",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USDT_VAULT_A,
            "trusted-stable",
            "raydium-usdc-usdt-vault-a",
            "The pinned Raydium USDC/USDT CLMM swap passes the USDC vault",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USDT_VAULT_B,
            "trusted-stable",
            "raydium-usdc-usdt-vault-b",
            "The pinned Raydium USDC/USDT CLMM swap passes the USDT vault",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USDT_OBSERVATION,
            "trusted-stable",
            "raydium-usdc-usdt-observation",
            "The pinned Raydium USDC/USDT CLMM swap passes this observation account",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USDT_BITMAP_EXTENSION,
            "trusted-stable",
            "raydium-usdc-usdt-bitmap-extension",
            "The pinned Raydium USDC/USDT CLMM swap passes this bitmap extension",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USDT_TICK_ARRAY_M240,
            "trusted-stable",
            "raydium-usdc-usdt-tick-array--240",
            "Nearby pinned Raydium USDC/USDT CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USDT_TICK_ARRAY_M120,
            "trusted-stable",
            "raydium-usdc-usdt-tick-array--120",
            "Nearby pinned Raydium USDC/USDT CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USDT_TICK_ARRAY_M60,
            "trusted-stable",
            "raydium-usdc-usdt-tick-array--60",
            "Current pinned Raydium USDC/USDT CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USDT_TICK_ARRAY_0,
            "trusted-stable",
            "raydium-usdc-usdt-tick-array-0",
            "Nearby pinned Raydium USDC/USDT CLMM tick array",
        ),
        AltManifestEntry::required(
            RAYDIUM_USDC_USDT_TICK_ARRAY_60,
            "trusted-stable",
            "raydium-usdc-usdt-tick-array-60",
            "Nearby pinned Raydium USDC/USDT CLMM tick array",
        ),
    ]
}

pub fn meteora_dbc_damm_alt_manifest_entries() -> Vec<AltManifestEntry> {
    vec![
        AltManifestEntry::required(
            METEORA_DBC_PROGRAM_ID,
            "meteora-dbc-damm",
            "meteora-dbc-program",
            "Canonical Meteora pre-bond routes invoke the DBC program",
        ),
        AltManifestEntry::required(
            METEORA_DBC_POOL_AUTHORITY,
            "meteora-dbc-damm",
            "meteora-dbc-pool-authority",
            "DBC swap instructions pass the fixed DBC pool authority",
        ),
        AltManifestEntry::required(
            METEORA_DBC_EVENT_AUTHORITY,
            "meteora-dbc-damm",
            "meteora-dbc-event-authority",
            "DBC swap instructions pass the Anchor event authority",
        ),
        AltManifestEntry::required(
            METEORA_DAMM_V2_PROGRAM_ID,
            "meteora-dbc-damm",
            "meteora-damm-v2-program",
            "Canonical post-bond routes invoke the Meteora DAMM v2 program",
        ),
        AltManifestEntry::required(
            METEORA_DAMM_V2_POOL_AUTHORITY,
            "meteora-dbc-damm",
            "meteora-damm-v2-pool-authority",
            "DAMM v2 swap instructions pass the fixed pool authority",
        ),
        AltManifestEntry::required(
            METEORA_DAMM_V2_EVENT_AUTHORITY,
            "meteora-dbc-damm",
            "meteora-damm-v2-event-authority",
            "DAMM v2 swap instructions pass the Anchor event authority",
        ),
        AltManifestEntry::required(
            TOKEN_2022_PROGRAM_ID,
            "meteora-dbc-damm",
            "token-2022-program",
            "Meteora DBC/DAMM pools can trade Token-2022 base mints",
        ),
        AltManifestEntry::required(
            USDC_MINT,
            "meteora-dbc-damm",
            "usdc-mint",
            "USDC-quoted Meteora routes compose through the trusted SOL/USDC route",
        ),
        AltManifestEntry::required(
            RAYDIUM_CLMM_PROGRAM_ID,
            "meteora-dbc-damm",
            "raydium-clmm-program",
            "USDC-quoted Meteora routes compose through the trusted Raydium SOL/USDC CLMM",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDC_POOL,
            "meteora-dbc-damm",
            "raydium-sol-usdc-pool",
            "USDC-quoted Meteora routes use the pinned Raydium SOL/USDC CLMM pool",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDC_AMM_CONFIG,
            "meteora-dbc-damm",
            "raydium-sol-usdc-amm-config",
            "The pinned Raydium SOL/USDC CLMM swap passes this AMM config",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDC_VAULT_A,
            "meteora-dbc-damm",
            "raydium-sol-usdc-vault-a",
            "The pinned Raydium SOL/USDC CLMM swap passes the WSOL vault",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDC_VAULT_B,
            "meteora-dbc-damm",
            "raydium-sol-usdc-vault-b",
            "The pinned Raydium SOL/USDC CLMM swap passes the USDC vault",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDC_OBSERVATION,
            "meteora-dbc-damm",
            "raydium-sol-usdc-observation",
            "The pinned Raydium SOL/USDC CLMM swap passes this observation account",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDC_BITMAP_EXTENSION,
            "meteora-dbc-damm",
            "raydium-sol-usdc-bitmap-extension",
            "The pinned Raydium SOL/USDC CLMM swap passes this bitmap extension",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDC_TICK_ARRAY_M23820,
            "meteora-dbc-damm",
            "raydium-sol-usdc-tick-array--23820",
            "Nearby pinned Raydium SOL/USDC CLMM tick array for USDC conversion buys",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDC_TICK_ARRAY_M23760,
            "meteora-dbc-damm",
            "raydium-sol-usdc-tick-array--23760",
            "Nearby pinned Raydium SOL/USDC CLMM tick array for USDC conversion buys",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDC_TICK_ARRAY_M23700,
            "meteora-dbc-damm",
            "raydium-sol-usdc-tick-array--23700",
            "Current pinned Raydium SOL/USDC CLMM tick array for USDC conversion",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDC_TICK_ARRAY_M23640,
            "meteora-dbc-damm",
            "raydium-sol-usdc-tick-array--23640",
            "Nearby pinned Raydium SOL/USDC CLMM tick array for USDC conversion sells",
        ),
        AltManifestEntry::required(
            RAYDIUM_SOL_USDC_TICK_ARRAY_M23580,
            "meteora-dbc-damm",
            "raydium-sol-usdc-tick-array--23580",
            "Nearby pinned Raydium SOL/USDC CLMM tick array for USDC conversion sells",
        ),
    ]
}

pub fn raydium_amm_v4_alt_manifest_entries() -> Vec<AltManifestEntry> {
    vec![
        AltManifestEntry::required(
            RAYDIUM_AMM_V4_PROGRAM_ID,
            "raydium-amm-v4",
            "raydium-amm-v4-program",
            "Generic Raydium AMM v4 routes invoke the AMM program for every pool",
        ),
        AltManifestEntry::required(
            RAYDIUM_AMM_V4_AUTHORITY,
            "raydium-amm-v4",
            "raydium-amm-v4-authority",
            "Generic Raydium AMM v4 routes pass the fixed AMM authority",
        ),
        AltManifestEntry::required(
            OPENBOOK_DEX_PROGRAM_ID,
            "raydium-amm-v4",
            "openbook-dex-program",
            "Raydium AMM v4 market_program commonly resolves to OpenBook",
        ),
        AltManifestEntry::required(
            TOKEN_PROGRAM_ID,
            "raydium-amm-v4",
            "spl-token-program",
            "Raydium AMM v4 swaps and WSOL lifecycle instructions use SPL Token",
        ),
        AltManifestEntry::required(
            ASSOCIATED_TOKEN_PROGRAM_ID,
            "raydium-amm-v4",
            "associated-token-program",
            "Raydium AMM v4 buys create the output ATA idempotently",
        ),
        AltManifestEntry::required(
            SYSTEM_PROGRAM_ID,
            "raydium-amm-v4",
            "system-program",
            "Raydium AMM v4 WSOL setup creates a temporary token account",
        ),
        AltManifestEntry::required(
            COMPUTE_BUDGET_PROGRAM_ID,
            "raydium-amm-v4",
            "compute-budget-program",
            "Raydium AMM v4 transactions set compute limits and priority fees",
        ),
        AltManifestEntry::required(
            WSOL_MINT,
            "raydium-amm-v4",
            "wsol-mint",
            "Generic Raydium AMM v4 routing is restricted to WSOL pairs",
        ),
        AltManifestEntry::required(
            MEMO_PROGRAM_ID,
            "raydium-amm-v4",
            "memo-program",
            "Raydium AMM v4 transactions add a uniqueness memo",
        ),
        AltManifestEntry::required(
            JITODONTFRONT_ACCOUNT,
            "raydium-amm-v4",
            "jitodontfront-account",
            "Secure/reduced MEV Raydium AMM v4 routes append Jito's dont-front account",
        ),
        AltManifestEntry::required(
            RENT_SYSVAR_ID,
            "raydium-amm-v4",
            "rent-sysvar",
            "Wrapper v2 and token-account setup paths can reference the rent sysvar",
        ),
    ]
}

pub fn shared_alt_manifest_entries() -> Vec<AltManifestEntry> {
    let mut entries = vec![AltManifestEntry::required(
        SHARED_SUPER_LOOKUP_TABLE,
        "shared-alt",
        "active-shared-table",
        "Shared table used by execution-engine and LaunchDeck v0 compilers",
    )];
    entries.extend(pump_apr28_fee_recipient_manifest_entries());
    entries.extend(wrapper_alt_manifest_entries());
    entries.extend(trusted_stable_alt_manifest_entries());
    entries.extend(raydium_amm_v4_alt_manifest_entries());
    entries.extend(meteora_dbc_damm_alt_manifest_entries());
    entries
}

pub fn lookup_table_address_content_hash(addresses: &[String]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for address in addresses {
        for byte in address.as_bytes().iter().copied().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_tip::all_known_tip_accounts;

    #[test]
    fn shared_manifest_includes_all_pump_apr28_fee_recipients() {
        let entries = shared_alt_manifest_entries();
        for recipient in PUMP_APR28_FEE_RECIPIENTS {
            assert!(entries.iter().any(|entry| entry.address == recipient));
        }
        for address in [
            SELECTED_PUMP_APR28_USDC_FEE_RECIPIENT_ATA,
            PUMP_PROGRAM_ID,
            PUMP_AMM_PROGRAM_ID,
            PUMP_FEE_PROGRAM_ID,
            PUMP_GLOBAL,
            PUMP_GLOBAL_VOLUME_ACCUMULATOR,
            PUMP_FEE_CONFIG,
        ] {
            assert!(entries.iter().any(|entry| entry.address == address));
        }
    }

    #[test]
    fn shared_manifest_excludes_provider_tip_accounts() {
        let entries = shared_alt_manifest_entries();
        for tip_account in all_known_tip_accounts() {
            assert!(!entries.iter().any(|entry| entry.address == tip_account));
        }
    }

    #[test]
    fn shared_manifest_tracks_optional_trusted_stable_orca_program() {
        let entries = shared_alt_manifest_entries();
        let entry = entries
            .iter()
            .find(|entry| entry.address == ORCA_WHIRLPOOL_PROGRAM_ID)
            .expect("orca whirlpool manifest entry");
        assert!(!entry.required);
    }

    #[test]
    fn shared_manifest_includes_trusted_stable_raydium_addresses() {
        let entries = shared_alt_manifest_entries();
        for address in [
            USDT_MINT,
            USD1_MINT,
            RAYDIUM_CLMM_PROGRAM_ID,
            RAYDIUM_SOL_USDT_POOL,
            RAYDIUM_SOL_USDT_AMM_CONFIG,
            RAYDIUM_SOL_USDT_VAULT_A,
            RAYDIUM_SOL_USDT_VAULT_B,
            RAYDIUM_SOL_USDT_OBSERVATION,
            RAYDIUM_SOL_USDT_BITMAP_EXTENSION,
            RAYDIUM_SOL_USDT_TICK_ARRAY_M23820,
            RAYDIUM_SOL_USDT_TICK_ARRAY_M23760,
            RAYDIUM_SOL_USDT_TICK_ARRAY_M23700,
            RAYDIUM_SOL_USDT_TICK_ARRAY_M23640,
            RAYDIUM_SOL_USDT_TICK_ARRAY_M23580,
            RAYDIUM_SOL_USD1_POOL,
            RAYDIUM_SOL_USD1_AMM_CONFIG,
            RAYDIUM_SOL_USD1_VAULT_A,
            RAYDIUM_SOL_USD1_VAULT_B,
            RAYDIUM_SOL_USD1_OBSERVATION,
            RAYDIUM_SOL_USD1_BITMAP_EXTENSION,
            RAYDIUM_SOL_USD1_TICK_ARRAY_M32400,
            RAYDIUM_SOL_USD1_TICK_ARRAY_M28800,
            RAYDIUM_SOL_USD1_TICK_ARRAY_M25200,
            RAYDIUM_SOL_USD1_TICK_ARRAY_M21600,
            RAYDIUM_SOL_USD1_TICK_ARRAY_M18000,
            RAYDIUM_USDC_USD1_POOL,
            RAYDIUM_USDC_USD1_AMM_CONFIG,
            RAYDIUM_USDC_USD1_VAULT_A,
            RAYDIUM_USDC_USD1_VAULT_B,
            RAYDIUM_USDC_USD1_OBSERVATION,
            RAYDIUM_USDC_USD1_BITMAP_EXTENSION,
            RAYDIUM_USDC_USD1_TICK_ARRAY_M120,
            RAYDIUM_USDC_USD1_TICK_ARRAY_M60,
            RAYDIUM_USDC_USD1_TICK_ARRAY_0,
            RAYDIUM_USDC_USD1_TICK_ARRAY_60,
            RAYDIUM_USDC_USDT_POOL,
            RAYDIUM_USDC_USDT_AMM_CONFIG,
            RAYDIUM_USDC_USDT_VAULT_A,
            RAYDIUM_USDC_USDT_VAULT_B,
            RAYDIUM_USDC_USDT_OBSERVATION,
            RAYDIUM_USDC_USDT_BITMAP_EXTENSION,
            RAYDIUM_USDC_USDT_TICK_ARRAY_M240,
            RAYDIUM_USDC_USDT_TICK_ARRAY_M120,
            RAYDIUM_USDC_USDT_TICK_ARRAY_M60,
            RAYDIUM_USDC_USDT_TICK_ARRAY_0,
            RAYDIUM_USDC_USDT_TICK_ARRAY_60,
        ] {
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.family == "trusted-stable" && entry.address == address),
                "missing trusted stable reusable ALT address {address}"
            );
        }
    }

    #[test]
    fn shared_manifest_includes_meteora_reusable_addresses() {
        let entries = shared_alt_manifest_entries();
        for address in [
            METEORA_DBC_PROGRAM_ID,
            METEORA_DBC_POOL_AUTHORITY,
            METEORA_DBC_EVENT_AUTHORITY,
            METEORA_DAMM_V2_PROGRAM_ID,
            METEORA_DAMM_V2_POOL_AUTHORITY,
            METEORA_DAMM_V2_EVENT_AUTHORITY,
            TOKEN_2022_PROGRAM_ID,
            USDC_MINT,
            RAYDIUM_CLMM_PROGRAM_ID,
            RAYDIUM_SOL_USDC_POOL,
            RAYDIUM_SOL_USDC_AMM_CONFIG,
            RAYDIUM_SOL_USDC_VAULT_A,
            RAYDIUM_SOL_USDC_VAULT_B,
            RAYDIUM_SOL_USDC_OBSERVATION,
            RAYDIUM_SOL_USDC_BITMAP_EXTENSION,
            RAYDIUM_SOL_USDC_TICK_ARRAY_M23820,
            RAYDIUM_SOL_USDC_TICK_ARRAY_M23760,
            RAYDIUM_SOL_USDC_TICK_ARRAY_M23700,
            RAYDIUM_SOL_USDC_TICK_ARRAY_M23640,
            RAYDIUM_SOL_USDC_TICK_ARRAY_M23580,
        ] {
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.family == "meteora-dbc-damm" && entry.address == address),
                "missing Meteora reusable ALT address {address}"
            );
        }
    }

    #[test]
    fn shared_manifest_includes_raydium_amm_v4_reusable_addresses() {
        let entries = shared_alt_manifest_entries();
        for address in [
            RAYDIUM_AMM_V4_PROGRAM_ID,
            RAYDIUM_AMM_V4_AUTHORITY,
            OPENBOOK_DEX_PROGRAM_ID,
            TOKEN_PROGRAM_ID,
            ASSOCIATED_TOKEN_PROGRAM_ID,
            SYSTEM_PROGRAM_ID,
            COMPUTE_BUDGET_PROGRAM_ID,
            WSOL_MINT,
            MEMO_PROGRAM_ID,
            JITODONTFRONT_ACCOUNT,
            RENT_SYSVAR_ID,
        ] {
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.family == "raydium-amm-v4" && entry.address == address),
                "missing Raydium AMM v4 reusable ALT address {address}"
            );
        }
    }

    #[test]
    fn lookup_table_content_hash_changes_when_entries_change() {
        let first = lookup_table_address_content_hash(&["A".to_string(), "B".to_string()]);
        let second = lookup_table_address_content_hash(&["A".to_string(), "C".to_string()]);
        assert_ne!(first, second);
    }
}
