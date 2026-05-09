#![allow(non_snake_case, dead_code)]

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use shared_execution_routing::alt_manifest::lookup_table_address_content_hash;
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_sdk::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{AddressLookupTableAccount, VersionedMessage, v0},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use std::{
    cmp::Ordering,
    collections::HashMap,
    fs,
    str::FromStr,
    sync::{Mutex, OnceLock},
};
use tokio::time::Duration;
use uuid::Uuid;

use crate::{
    compiled_transaction_signers,
    config::{
        NormalizedConfig, NormalizedExecution, configured_default_dev_auto_sell_compute_unit_limit,
        configured_default_follow_up_compute_unit_limit,
        configured_default_launch_compute_unit_limit,
        configured_default_launch_usd1_topup_compute_unit_limit,
        configured_default_sniper_buy_compute_unit_limit, validate_launchpad_support,
    },
    launchpad_dispatch::{launchpad_action_backend, launchpad_action_rollout_state},
    paths,
    report::{
        BonkUsd1LaunchSummary, FeeSettings, InstructionSummary, TransactionSummary, build_report,
        render_report,
    },
    rpc::{
        CompiledTransaction, fetch_account_data, fetch_account_data_with_owner,
        fetch_latest_blockhash_cached, fetch_multiple_account_data,
    },
    transport::TransportPlan,
    vanity_pool::{
        VanityLaunchpad, VanityReservation, append_vanity_report_note, reserve_vanity_mint,
    },
    wallet::read_keypair_bytes,
    wrapper_compile::{
        ABI_VERSION as WRAPPER_ABI_VERSION, EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT,
        EXECUTE_SWAP_ROUTE_TOKEN_FEE_ACCOUNT_COUNT, EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT,
        ExecuteSwapRouteRequest, SWAP_ROUTE_NO_PATCH_OFFSET, SwapLegInputSource,
        SwapRouteDirection, SwapRouteFeeMode, SwapRouteLeg, SwapRouteMode, SwapRouteSettlement,
        build_execute_swap_route_instruction, estimate_sol_in_fee_lamports, route_wsol_pda,
        wrapper_fee_vault, wrapper_token_program_id, wrapper_wsol_mint,
    },
};

use crate::pump_native::{LaunchQuote, NativeCompileTimings};

const PACKET_LIMIT_BYTES: usize = 1232;
const PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT: u64 = 1_000_000;
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const JITODONTFRONT_ACCOUNT: &str = "jitodontfront111111111111111111111111111111";
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
const MPL_TOKEN_METADATA_PROGRAM_ID: &str = "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s";
const BONK_LAUNCHPAD_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";
const BONK_LETSBONK_PLATFORM_ID: &str = "FfYek5vEz23cMkWsdJwG2oa6EphsvXSHrGpdALN4g6W1";
const BONK_BONKERS_PLATFORM_ID: &str = "82NMHVCKwehXgbXMyzL41mvv3sdkypaMCtTxvJ4CtTzm";
const BONK_SOL_QUOTE_MINT: &str = "So11111111111111111111111111111111111111112";
const BONK_USD1_QUOTE_MINT: &str = "USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB";
const BONK_USD1_SUPER_LOOKUP_TABLE: &str = "7CaMLcAuSskoeN7HoRwZjsSthU8sMwKqxtXkyMiMjuc";
const BONK_PINNED_USD1_ROUTE_POOL_ID: &str = "AQAGYQsdU853WAKhXM79CgNdoyhrRwXvYHX6qrDyC1FS";
const BONK_PREFERRED_USD1_ROUTE_CONFIG_ID: &str = "E64NGkDLLCdQ2yFNPcavaKptrEgmiQaNykUuLC1Qgwyp";
const BONK_CLMM_PROGRAM_ID: &str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";
const BONK_CPMM_PROGRAM_ID: &str = "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C";
const BONK_MAINNET_SOL_LAUNCH_CONFIG_ID: &str = "6s1xP3hpbAfFoNtUNF8mfHsjr2Bd97JxFJRWLbL6aHuX";
const BONK_MAINNET_USD1_LAUNCH_CONFIG_ID: &str = "EPiZbnrThjyLnoQ6QQzkxeFqyL5uyg9RzNHHAudUPxBz";
const BONK_DEFAULT_SUPPLY_INIT: &str = "1000000000000000";
const BONK_DEFAULT_TOTAL_SELL_A: &str = "793100000000000";
const BONK_DEFAULT_SOL_TOTAL_FUND_RAISING_B: &str = "85000000000";
const BONK_DEFAULT_USD1_TOTAL_FUND_RAISING_B: &str = "12500000000";
const BONK_CLMM_MINT_A_OFFSET: usize = 73;
const BONK_CLMM_MINT_B_OFFSET: usize = 105;
const BONK_CPMM_TOKEN_0_MINT_OFFSET: usize = 168;
const BONK_CPMM_TOKEN_1_MINT_OFFSET: usize = 200;
const BONK_DEFAULT_LAUNCH_DEFAULTS_CACHE_TTL_MS: u64 = 30 * 60 * 1000;
const BONK_DEFAULT_USD1_ROUTE_SETUP_CACHE_TTL_MS: u64 = 10 * 60 * 1000;
const BONK_DEFAULT_LOOKUP_TABLE_CACHE_TTL_MS: u64 = 30 * 60 * 1000;
const BONK_STARTUP_WARM_DEFAULT_STAGGER_MS: u64 = 400;
const BONK_TOKEN_DECIMALS: u32 = 6;
const DEFAULT_BONK_SELL_COMPUTE_UNIT_LIMIT: u64 = 280_000;
const DEFAULT_WRAPPER_FEE_BPS: u16 = 10;
const BONK_FEE_RATE_DENOMINATOR: u64 = 1_000_000;
const BONK_SPL_TOKEN_ACCOUNT_LEN: u64 = 165;
const BONK_CLMM_TICK_ARRAY_SIZE: i32 = 60;
const BONK_CLMM_DEFAULT_BITMAP_OFFSET: i32 = 512;
const BONK_USD1_QUOTE_MAX_INPUT_LAMPORTS: u64 = 100_000 * 1_000_000_000;
const BONK_USD1_ROUTE_SLIPPAGE_BPS: u64 = 500;
const BONK_CLMM_MIN_SQRT_PRICE_X64_PLUS_ONE: u128 = 4_295_048_017;
const BONK_CLMM_MAX_SQRT_PRICE_X64_MINUS_ONE: u128 = 79_226_673_521_066_979_257_578_248_091;
const BONK_CLMM_SWAP_DISCRIMINATOR: [u8; 8] = [43, 4, 237, 11, 26, 201, 30, 98];
const BONK_CPMM_SWAP_BASE_INPUT_DISCRIMINATOR: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];
const BONK_INITIALIZE_V2_DISCRIMINATOR: [u8; 8] = [67, 153, 175, 39, 218, 16, 38, 32];
const BONK_BUY_EXACT_IN_DISCRIMINATOR: [u8; 8] = [250, 234, 13, 123, 213, 156, 19, 236];
const BONK_SELL_EXACT_IN_DISCRIMINATOR: [u8; 8] = [149, 39, 222, 155, 211, 124, 152, 26];
const BONK_CPMM_AUTH_SEED: &[u8] = b"vault_and_lp_mint_auth_seed";

fn bonk_http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("bonk http client")
    })
}

#[derive(Debug, Clone)]
pub struct NativeBonkArtifacts {
    pub compiled_transactions: Vec<CompiledTransaction>,
    pub creation_transactions: Vec<CompiledTransaction>,
    pub deferred_setup_transactions: Vec<CompiledTransaction>,
    pub report: Value,
    pub text: String,
    pub compile_timings: NativeCompileTimings,
    pub mint: String,
    pub launch_creator: String,
    pub vanity_reservation: Option<VanityReservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BonkMarketSnapshot {
    pub mint: String,
    pub creator: String,
    pub virtualTokenReserves: String,
    pub virtualSolReserves: String,
    pub realTokenReserves: String,
    pub realSolReserves: String,
    pub tokenTotalSupply: String,
    pub complete: bool,
    pub marketCapLamports: String,
    pub marketCapSol: String,
    #[serde(default)]
    pub quoteAsset: String,
    #[serde(default)]
    pub quoteAssetLabel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BonkImportContext {
    pub launchpad: String,
    pub mode: String,
    pub quoteAsset: String,
    #[serde(default)]
    pub creator: String,
    #[serde(default)]
    pub platformId: String,
    #[serde(default)]
    pub configId: String,
    #[serde(default)]
    pub poolId: String,
    #[serde(default)]
    pub detectionSource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BonkPoolAddressClassification {
    pub mint: String,
    pub pool_id: String,
    pub family: String,
    #[serde(default)]
    pub quote_asset: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HelperUsd1QuoteMetrics {
    #[serde(default)]
    quoteCalls: u64,
    #[serde(default)]
    quoteTotalMs: u64,
    #[serde(default)]
    averageQuoteMs: f64,
    #[serde(default)]
    quoteCacheHits: u64,
    #[serde(default)]
    routeSetupLocalHits: u64,
    #[serde(default)]
    routeSetupCacheHits: u64,
    #[serde(default)]
    routeSetupCacheMisses: u64,
    #[serde(default)]
    routeSetupFetchMs: u64,
    #[serde(default)]
    superAltLocalSnapshotHits: u64,
    #[serde(default)]
    superAltRpcRefreshes: u64,
    #[serde(default)]
    expansionQuoteCalls: u64,
    #[serde(default)]
    binarySearchQuoteCalls: u64,
    #[serde(default)]
    searchIterations: u64,
}

#[derive(Debug, Clone)]
struct BonkQuoteAssetConfig {
    asset: &'static str,
    label: &'static str,
    mint: &'static str,
    decimals: u32,
}

#[derive(Debug, Clone)]
struct DecodedBonkLaunchpadPool {
    creator: Pubkey,
    status: u8,
    supply: u64,
    config_id: Pubkey,
    total_sell_a: u64,
    virtual_a: u64,
    virtual_b: u64,
    real_a: u64,
    real_b: u64,
    platform_id: Pubkey,
    mint_a: Pubkey,
}

#[derive(Debug, Clone)]
struct BonkMarketCandidate {
    mode: String,
    quote_asset: String,
    quote_asset_label: String,
    creator: String,
    platform_id: String,
    config_id: String,
    pool_id: String,
    real_quote_reserves: u64,
    complete: bool,
    detection_source: String,
    launch_migrate_pool: bool,
    tvl: f64,
    pool_type: String,
    launchpad_pool: Option<DecodedBonkLaunchpadPool>,
    raydium_pool: Option<RaydiumPoolInfo>,
}

#[derive(Debug, Clone)]
struct DecodedBonkLaunchpadConfig {
    curve_type: u8,
    migrate_fee: u64,
    trade_fee_rate: u64,
}

#[derive(Debug, Clone)]
struct DecodedBonkPlatformConfig {
    fee_rate: u64,
    creator_fee_rate: u64,
}

#[derive(Debug, Clone)]
struct DecodedBonkClmmConfig {
    trade_fee_rate: u32,
    tick_spacing: u16,
}

#[derive(Debug, Clone)]
struct DecodedBonkClmmPool {
    amm_config: Pubkey,
    mint_a: Pubkey,
    mint_b: Pubkey,
    vault_a: Pubkey,
    vault_b: Pubkey,
    observation_id: Pubkey,
    mint_decimals_a: u8,
    mint_decimals_b: u8,
    tick_spacing: u16,
    liquidity: BigUint,
    sqrt_price_x64: BigUint,
    tick_current: i32,
    tick_array_bitmap: [u64; 16],
}

#[derive(Debug, Clone)]
struct DecodedBonkCpmmPool {
    config_id: Pubkey,
    vault_a: Pubkey,
    vault_b: Pubkey,
    token_0_mint: Pubkey,
    token_1_mint: Pubkey,
    token_0_program: Pubkey,
    token_1_program: Pubkey,
    observation_id: Pubkey,
    mint_decimals_a: u8,
    mint_decimals_b: u8,
    protocol_fees_mint_a: u64,
    protocol_fees_mint_b: u64,
    fund_fees_mint_a: u64,
    fund_fees_mint_b: u64,
    enable_creator_fee: bool,
    creator_fees_mint_a: u64,
    creator_fees_mint_b: u64,
}

#[derive(Debug, Clone)]
struct DecodedBonkCpmmConfig {
    trade_fee_rate: u64,
    creator_fee_rate: u64,
}

#[derive(Debug, Clone)]
struct NativeBonkCpmmPoolContext {
    pool_id: Pubkey,
    pool: DecodedBonkCpmmPool,
    config: DecodedBonkCpmmConfig,
    reserve_a: u64,
    reserve_b: u64,
    quote: BonkQuoteAssetConfig,
}

#[derive(Debug, Clone)]
struct NativeBonkClmmPoolContext {
    setup: BonkUsd1RouteSetup,
    quote: BonkQuoteAssetConfig,
    mint_program_a: Pubkey,
    mint_program_b: Pubkey,
}

#[derive(Debug, Clone)]
enum NativeBonkTradeVenueContext {
    Launchpad(NativeBonkPoolContext),
    RaydiumCpmm(NativeBonkCpmmPoolContext),
    RaydiumClmm(NativeBonkClmmPoolContext),
}

#[derive(Debug, Clone)]
struct BonkClmmTick {
    tick: i32,
    liquidity_net: i128,
    liquidity_gross: BigUint,
}

#[derive(Debug, Clone)]
struct BonkClmmTickArray {
    start_tick_index: i32,
    ticks: Vec<BonkClmmTick>,
}

#[derive(Debug, Clone)]
pub struct BonkUsd1RouteSetup {
    pool_id: Pubkey,
    program_id: Pubkey,
    amm_config: Pubkey,
    mint_a: Pubkey,
    mint_b: Pubkey,
    vault_a: Pubkey,
    vault_b: Pubkey,
    observation_id: Pubkey,
    tick_spacing: i32,
    trade_fee_rate: u32,
    sqrt_price_x64: BigUint,
    liquidity: BigUint,
    tick_current: i32,
    mint_a_decimals: u32,
    mint_b_decimals: u32,
    current_price: f64,
    tick_arrays_desc: Vec<i32>,
    tick_arrays_asc: Vec<i32>,
    tick_arrays: HashMap<i32, BonkClmmTickArray>,
}

#[derive(Debug, Clone)]
struct BonkUsd1RouteSetupCacheEntry {
    fetched_at: std::time::Instant,
    setup: BonkUsd1RouteSetup,
}

#[derive(Debug, Clone)]
struct BonkLookupTableCacheEntry {
    fetched_at: std::time::Instant,
    table: AddressLookupTableAccount,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedBonkLookupTableCache {
    tables: HashMap<String, PersistedBonkLookupTableEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedBonkLookupTableEntry {
    addresses: Vec<String>,
    #[serde(default)]
    address_count: Option<usize>,
    #[serde(default)]
    content_hash: Option<String>,
}

fn merge_persisted_bonk_lookup_table_caches(
    caches: impl IntoIterator<Item = PersistedBonkLookupTableCache>,
) -> PersistedBonkLookupTableCache {
    let mut merged = PersistedBonkLookupTableCache::default();
    for cache in caches {
        for (address, entry) in cache.tables {
            merged.tables.entry(address).or_insert(entry);
        }
    }
    merged
}

#[derive(Debug, Clone)]
struct BonkUsd1DirectQuote {
    expected_out: BigUint,
    min_out: BigUint,
    price_impact_pct: f64,
    traversed_tick_array_starts: Vec<i32>,
}

#[derive(Debug, Clone, Copy)]
struct BonkUsd1BuyAmountQuote {
    expected_amount_b: u64,
    guaranteed_amount_b: u64,
}

#[derive(Debug, Clone)]
struct NativeBonkUsd1LaunchDetails {
    compile_path: String,
    required_quote_amount: String,
    current_quote_amount: String,
    shortfall_quote_amount: String,
    input_sol: Option<String>,
    expected_quote_out: Option<String>,
    min_quote_out: Option<String>,
}

#[derive(Debug, Clone)]
struct NativeBonkLaunchResult {
    mint: String,
    launch_creator: String,
    vanity_reservation: Option<VanityReservation>,
    compiled_transactions: Vec<CompiledTransaction>,
    predicted_dev_buy_token_amount_raw: Option<String>,
    atomic_combined: bool,
    atomic_fallback_reason: Option<String>,
    usd1_launch_details: Option<NativeBonkUsd1LaunchDetails>,
    usd1_quote_metrics: Option<HelperUsd1QuoteMetrics>,
    compiled_via_native: bool,
}

#[derive(Debug, Clone)]
struct NativeBonkPreparedUsd1Topup {
    required_quote_amount: BigUint,
    current_quote_amount: BigUint,
    shortfall_quote_amount: BigUint,
    input_lamports: Option<BigUint>,
    expected_quote_out: Option<BigUint>,
    min_quote_out: Option<BigUint>,
    traversed_tick_array_starts: Vec<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BonkUsd1TopupMode {
    RespectExistingBalance,
    ForceFullAmount,
}

#[derive(Debug, Clone)]
pub struct BonkFollowBuyCompileResult {
    pub transactions: Vec<CompiledTransaction>,
    pub primary_tx_index: usize,
    pub requires_ordered_execution: bool,
    pub entry_preference_asset: Option<String>,
    pub wrapper_tx_index: Option<usize>,
    pub wrapper_gross_sol_in_lamports: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct BonkAtomicFollowBuyCompileResult {
    pub transaction: CompiledTransaction,
    pub wrapper_gross_sol_in_lamports: Option<u64>,
}

#[derive(Debug, Clone)]
struct BonkCurvePoolState {
    total_sell_a: BigUint,
    virtual_a: BigUint,
    virtual_b: BigUint,
    real_a: BigUint,
    real_b: BigUint,
}

#[derive(Debug, Clone)]
struct BonkLaunchDefaults {
    supply: BigUint,
    total_fund_raising_b: BigUint,
    quote: BonkQuoteAssetConfig,
    trade_fee_rate: BigUint,
    platform_fee_rate: BigUint,
    creator_fee_rate: BigUint,
    curve_type: u8,
    pool: BonkCurvePoolState,
}

#[derive(Debug, Clone)]
struct BonkLaunchDefaultsCacheEntry {
    fetched_at: std::time::Instant,
    defaults: BonkLaunchDefaults,
}

#[derive(Debug, Clone)]
struct NativeBonkTxConfig {
    compute_unit_limit: u32,
    compute_unit_price_micro_lamports: u64,
    tip_lamports: u64,
    tip_account: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeBonkTxFormat {
    Legacy,
    V0,
}

#[derive(Debug, Clone)]
pub struct NativeBonkPoolContext {
    pool_id: Pubkey,
    pool: DecodedBonkLaunchpadPool,
    config: DecodedBonkLaunchpadConfig,
    platform: DecodedBonkPlatformConfig,
    quote: BonkQuoteAssetConfig,
    token_program: Pubkey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BonkPredictedDevBuyEffect {
    pub requested_quote_amount_b: u64,
    pub token_amount: u64,
}

#[derive(Debug, Clone)]
struct DecomposedBonkVersionedTransaction {
    instructions: Vec<Instruction>,
    lookup_tables: Vec<AddressLookupTableAccount>,
    signer_pubkeys: Vec<Pubkey>,
}

#[derive(Debug, Clone)]
struct RaydiumLaunchConfigCacheEntry {
    fetched_at: std::time::Instant,
    configs: Vec<RaydiumLaunchConfigEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct RaydiumLaunchConfigEntry {
    key: RaydiumLaunchConfigKey,
    #[serde(default, rename = "defaultParams")]
    default_params: RaydiumLaunchConfigDefaultParams,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RaydiumLaunchConfigKey {
    #[serde(default, rename = "pubKey")]
    pubkey: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RaydiumLaunchConfigDefaultParams {
    #[serde(default, rename = "supplyInit")]
    supply_init: String,
    #[serde(default, rename = "totalFundRaisingB")]
    total_fund_raising_b: String,
    #[serde(default, rename = "totalSellA")]
    total_sell_a: String,
}

fn parse_raydium_launch_configs_payload(
    payload: Value,
) -> Result<Vec<RaydiumLaunchConfigEntry>, String> {
    let configs_value = payload
        .get("data")
        .and_then(|value| {
            if value.is_array() {
                Some(value.clone())
            } else {
                value.get("data").cloned()
            }
        })
        .ok_or_else(|| {
            "Raydium launch configs payload did not include a data array.".to_string()
        })?;
    serde_json::from_value::<Vec<RaydiumLaunchConfigEntry>>(configs_value)
        .map_err(|error| format!("Failed to decode Raydium launch configs payload: {error}"))
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RaydiumTokenAddress {
    #[serde(default)]
    address: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RaydiumConfigRef {
    #[serde(default)]
    id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RaydiumPoolInfo {
    #[serde(default)]
    id: String,
    #[serde(default)]
    price: f64,
    #[serde(default)]
    tvl: f64,
    #[serde(default, rename = "type")]
    pool_type: String,
    #[serde(default, rename = "launchMigratePool")]
    launch_migrate_pool: bool,
    #[serde(default, rename = "mintA")]
    mint_a: RaydiumTokenAddress,
    #[serde(default, rename = "mintB")]
    mint_b: RaydiumTokenAddress,
    #[serde(default)]
    config: Option<RaydiumConfigRef>,
}

#[derive(Debug, Clone, Deserialize)]
struct RaydiumPoolsResponse {
    #[serde(default, deserialize_with = "deserialize_raydium_pools_response_data")]
    data: Vec<RaydiumPoolInfo>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RaydiumPoolsResponsePage {
    #[serde(default)]
    data: Vec<RaydiumPoolInfo>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RaydiumPoolsResponseData {
    Direct(Vec<RaydiumPoolInfo>),
    Nested(RaydiumPoolsResponsePage),
}

fn deserialize_raydium_pools_response_data<'de, D>(
    deserializer: D,
) -> Result<Vec<RaydiumPoolInfo>, D::Error>
where
    D: Deserializer<'de>,
{
    let payload = RaydiumPoolsResponseData::deserialize(deserializer)?;
    Ok(match payload {
        RaydiumPoolsResponseData::Direct(entries) => entries,
        RaydiumPoolsResponseData::Nested(page) => page.data,
    })
}

#[derive(Debug, Deserialize)]
struct RpcMultipleAccountsResponse {
    result: RpcMultipleAccountsResult,
}

#[derive(Debug, Deserialize)]
struct RpcMultipleAccountsResult {
    value: Vec<Option<RpcMultipleAccountsValue>>,
}

#[derive(Debug, Deserialize)]
struct RpcMultipleAccountsValue {
    data: (String, String),
}

#[derive(Debug, Clone, Deserialize)]
struct RpcAccountValue {
    data: (String, String),
}

#[derive(Debug, Clone, Deserialize)]
struct RpcProgramAccount {
    pubkey: String,
    account: RpcAccountValue,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcResponse<T> {
    result: T,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcTokenSupplyValue {
    #[serde(default)]
    amount: String,
    #[serde(default)]
    decimals: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcTokenSupplyResult {
    value: RpcTokenSupplyValue,
}

fn render_usd1_quote_metrics_note(metrics: &HelperUsd1QuoteMetrics) -> Option<String> {
    if metrics.quoteCalls == 0
        && metrics.routeSetupLocalHits == 0
        && metrics.routeSetupCacheHits == 0
        && metrics.routeSetupCacheMisses == 0
        && metrics.superAltLocalSnapshotHits == 0
        && metrics.superAltRpcRefreshes == 0
    {
        return None;
    }
    Some(format!(
        "USD1 quote metrics: calls={} total={}ms avg={:.1}ms quote-cache-hits={} route-setup(local/ttl/miss)={}/{}/{} route-setup-fetch={}ms super-alt(local/rpc-refresh)={}/{} search(expansion/binary iters)={}/{}/{}",
        metrics.quoteCalls,
        metrics.quoteTotalMs,
        metrics.averageQuoteMs,
        metrics.quoteCacheHits,
        metrics.routeSetupLocalHits,
        metrics.routeSetupCacheHits,
        metrics.routeSetupCacheMisses,
        metrics.routeSetupFetchMs,
        metrics.superAltLocalSnapshotHits,
        metrics.superAltRpcRefreshes,
        metrics.expansionQuoteCalls,
        metrics.binarySearchQuoteCalls,
        metrics.searchIterations,
    ))
}

fn bonk_launchpad_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(BONK_LAUNCHPAD_PROGRAM_ID)
        .map_err(|error| format!("Invalid Bonk launchpad program id: {error}"))
}

fn bonk_cpmm_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(BONK_CPMM_PROGRAM_ID)
        .map_err(|error| format!("Invalid Bonk CPMM program id: {error}"))
}

fn bonk_cpmm_pool_authority() -> Result<Pubkey, String> {
    let program = bonk_cpmm_program_id()?;
    Ok(Pubkey::find_program_address(&[BONK_CPMM_AUTH_SEED], &program).0)
}

fn bonk_quote_mint(quote_asset: &str) -> Result<Pubkey, String> {
    let quote_mint = match quote_asset.trim().to_ascii_lowercase().as_str() {
        "usd1" => BONK_USD1_QUOTE_MINT,
        _ => BONK_SOL_QUOTE_MINT,
    };
    Pubkey::from_str(quote_mint)
        .map_err(|error| format!("Invalid Bonk quote mint address: {error}"))
}

fn bonk_platform_id(mode: &str) -> &'static str {
    match mode.trim().to_ascii_lowercase().as_str() {
        "bonkers" => BONK_BONKERS_PLATFORM_ID,
        _ => BONK_LETSBONK_PLATFORM_ID,
    }
}

fn bonk_u16_be_bytes(value: u16) -> [u8; 2] {
    value.to_be_bytes()
}

fn bonk_launch_config_id(quote_asset: &str) -> Result<String, String> {
    let quote_mint = bonk_quote_mint(quote_asset)?;
    let (config_id, _) = Pubkey::find_program_address(
        &[
            b"global_config",
            quote_mint.as_ref(),
            &[0],
            &bonk_u16_be_bytes(0),
        ],
        &bonk_launchpad_program_id()?,
    );
    Ok(config_id.to_string())
}

fn bonk_quote_asset_config(asset: &str) -> BonkQuoteAssetConfig {
    match asset.trim().to_ascii_lowercase().as_str() {
        "usd1" => BonkQuoteAssetConfig {
            asset: "usd1",
            label: "USD1",
            mint: BONK_USD1_QUOTE_MINT,
            decimals: 6,
        },
        _ => BonkQuoteAssetConfig {
            asset: "sol",
            label: "SOL",
            mint: BONK_SOL_QUOTE_MINT,
            decimals: 9,
        },
    }
}

fn bonk_quote_asset_from_mint_address(address: &str) -> Option<BonkQuoteAssetConfig> {
    let normalized = address.trim();
    if normalized == BONK_SOL_QUOTE_MINT {
        return Some(bonk_quote_asset_config("sol"));
    }
    if normalized == BONK_USD1_QUOTE_MINT {
        return Some(bonk_quote_asset_config("usd1"));
    }
    None
}

pub fn classify_bonk_pool_address(
    address: &str,
    owner: &Pubkey,
    data: &[u8],
) -> Result<Option<BonkPoolAddressClassification>, String> {
    let pool_id = address.trim();
    if pool_id.is_empty() {
        return Ok(None);
    }
    if *owner == bonk_launchpad_program_id()? {
        let pool = match decode_bonk_launchpad_pool(data) {
            Ok(pool) => pool,
            Err(_) => return Ok(None),
        };
        return Ok(Some(BonkPoolAddressClassification {
            mint: pool.mint_a.to_string(),
            pool_id: pool_id.to_string(),
            family: "launchpad".to_string(),
            quote_asset: String::new(),
        }));
    }
    if *owner == bonk_clmm_program_id()? {
        let pool = match decode_bonk_clmm_pool(data) {
            Ok(pool) => pool,
            Err(_) => return Ok(None),
        };
        let mint_a = pool.mint_a.to_string();
        let mint_b = pool.mint_b.to_string();
        let quote_asset = bonk_quote_asset_from_mint_address(&mint_a)
            .or_else(|| bonk_quote_asset_from_mint_address(&mint_b))
            .ok_or_else(|| {
                format!("Bonk pool {pool_id} does not contain a supported quote mint.")
            })?;
        let mint = if quote_asset.mint == mint_a {
            mint_b
        } else if quote_asset.mint == mint_b {
            mint_a
        } else {
            return Ok(None);
        };
        return Ok(Some(BonkPoolAddressClassification {
            mint,
            pool_id: pool_id.to_string(),
            family: "raydium".to_string(),
            quote_asset: quote_asset.asset.to_string(),
        }));
    }
    if *owner == bonk_cpmm_program_id()? {
        let pool = match decode_bonk_cpmm_pool(data) {
            Ok(pool) => pool,
            Err(_) => return Ok(None),
        };
        let mint_a = pool.token_0_mint.to_string();
        let mint_b = pool.token_1_mint.to_string();
        let quote_asset = bonk_quote_asset_from_mint_address(&mint_a)
            .or_else(|| bonk_quote_asset_from_mint_address(&mint_b))
            .ok_or_else(|| {
                format!("Bonk pool {pool_id} does not contain a supported quote mint.")
            })?;
        let mint = if quote_asset.mint == mint_a {
            mint_b
        } else if quote_asset.mint == mint_b {
            mint_a
        } else {
            return Ok(None);
        };
        return Ok(Some(BonkPoolAddressClassification {
            mint,
            pool_id: pool_id.to_string(),
            family: "raydium".to_string(),
            quote_asset: quote_asset.asset.to_string(),
        }));
    }
    Ok(None)
}

fn pool_type_priority(pool_type: &str) -> u8 {
    match pool_type.trim() {
        "Standard" => 0,
        "Concentrated" => 1,
        _ => 2,
    }
}

fn is_raydium_detection_source(source: &str) -> bool {
    source.trim().to_ascii_lowercase().starts_with("raydium")
}

fn bonk_launch_defaults_cache_ttl() -> Duration {
    Duration::from_millis(
        std::env::var("BONK_LAUNCH_DEFAULTS_CACHE_TTL_MS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(BONK_DEFAULT_LAUNCH_DEFAULTS_CACHE_TTL_MS),
    )
}

fn bonk_launch_defaults_cache() -> &'static Mutex<HashMap<String, BonkLaunchDefaultsCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, BonkLaunchDefaultsCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_bonk_launch_defaults(launch_mode: &str, quote_asset: &str) -> Option<BonkLaunchDefaults> {
    let normalized_mode = normalize_bonk_launch_mode(launch_mode);
    let quote = bonk_quote_asset_config(quote_asset);
    let cache_key = format!("{normalized_mode}:{}", quote.asset);
    let ttl = bonk_launch_defaults_cache_ttl();
    bonk_launch_defaults_cache()
        .lock()
        .expect("bonk launch defaults cache")
        .get(&cache_key)
        .filter(|entry| entry.fetched_at.elapsed() <= ttl)
        .cloned()
        .map(|entry| entry.defaults)
}

pub fn bonk_startup_warm_defaults_cached() -> bool {
    [
        ("regular", "sol"),
        ("regular", "usd1"),
        ("bonkers", "sol"),
        ("bonkers", "usd1"),
    ]
    .into_iter()
    .all(|(launch_mode, quote_asset)| {
        cached_bonk_launch_defaults(launch_mode, quote_asset).is_some()
    })
}

fn bonk_launch_configs_cache() -> &'static Mutex<HashMap<String, RaydiumLaunchConfigCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, RaydiumLaunchConfigCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn bonk_usd1_route_setup_cache_ttl() -> Duration {
    Duration::from_millis(
        std::env::var("BONK_USD1_ROUTE_SETUP_CACHE_TTL_MS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(BONK_DEFAULT_USD1_ROUTE_SETUP_CACHE_TTL_MS),
    )
}

fn bonk_lookup_table_cache_ttl() -> Duration {
    Duration::from_millis(
        std::env::var("BONK_LOOKUP_TABLE_CACHE_TTL_MS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(BONK_DEFAULT_LOOKUP_TABLE_CACHE_TTL_MS),
    )
}

fn bonk_usd1_route_setup_cache() -> &'static Mutex<HashMap<String, BonkUsd1RouteSetupCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, BonkUsd1RouteSetupCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn normalize_bonk_launch_mode(mode: &str) -> &'static str {
    match mode.trim().to_ascii_lowercase().as_str() {
        "bonkers" => "bonkers",
        _ => "regular",
    }
}

fn normalize_bonk_buy_funding_policy(policy: &str) -> &'static str {
    match policy
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .replace(' ', "_")
        .as_str()
    {
        "usd1_only" => "usd1_only",
        "usd1_via_sol" => "usd1_via_sol",
        "prefer_usd1_else_topup" => "prefer_usd1_else_topup",
        _ => "sol_only",
    }
}

fn select_bonk_usd1_buy_amount_from_sol_quote(
    funding_policy: &str,
    current_balance: u64,
    quote: &BonkUsd1BuyAmountQuote,
) -> u64 {
    if funding_policy == "sol_only" {
        return quote.guaranteed_amount_b;
    }
    if current_balance >= quote.expected_amount_b {
        quote.expected_amount_b
    } else if current_balance >= quote.guaranteed_amount_b {
        current_balance
    } else {
        quote.guaranteed_amount_b
    }
}

fn normalize_bonk_sell_settlement_asset(asset: &str) -> &'static str {
    match asset.trim().to_ascii_lowercase().as_str() {
        "usd1" => "usd1",
        _ => "sol",
    }
}

fn bonk_biguint_from_u64(value: u64) -> BigUint {
    BigUint::from(value)
}

fn bonk_q64() -> BigUint {
    BigUint::from(1u8) << 64usize
}

fn parse_decimal_biguint(value: &str, decimals: u32, label: &str) -> Result<BigUint, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} was empty."));
    }
    if trimmed.starts_with('-') {
        return Err(format!("{label} must be non-negative."));
    }
    let mut parts = trimmed.split('.');
    let whole_raw = parts.next().unwrap_or_default();
    let fraction_raw = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        return Err(format!("Invalid {label}: {trimmed}"));
    }
    if !whole_raw.chars().all(|ch| ch.is_ascii_digit())
        || !fraction_raw.chars().all(|ch| ch.is_ascii_digit())
    {
        return Err(format!("Invalid {label}: {trimmed}"));
    }
    let whole = if whole_raw.is_empty() { "0" } else { whole_raw };
    let mut padded_fraction = fraction_raw.to_string();
    if padded_fraction.len() > decimals as usize {
        padded_fraction.truncate(decimals as usize);
    } else if padded_fraction.len() < decimals as usize {
        padded_fraction.push_str(&"0".repeat(decimals as usize - padded_fraction.len()));
    }
    let whole_value = BigUint::parse_bytes(whole.as_bytes(), 10)
        .ok_or_else(|| format!("Invalid {label}: {trimmed}"))?;
    let factor = BigUint::from(10u8).pow(decimals);
    let fraction_value = if padded_fraction.is_empty() {
        BigUint::ZERO
    } else {
        BigUint::parse_bytes(padded_fraction.as_bytes(), 10)
            .ok_or_else(|| format!("Invalid {label}: {trimmed}"))?
    };
    Ok(whole_value * factor + fraction_value)
}

fn parse_biguint_integer(value: &str, label: &str) -> Result<BigUint, String> {
    BigUint::parse_bytes(value.trim().as_bytes(), 10)
        .ok_or_else(|| format!("Invalid {label}: {value}"))
}

fn format_biguint_decimal(value: &BigUint, decimals: u32, max_fraction_digits: u32) -> String {
    if decimals == 0 {
        return value.to_string();
    }
    let raw = value.to_string();
    let width = decimals as usize;
    let (whole, mut fraction) = if raw.len() <= width {
        ("0".to_string(), format!("{raw:0>width$}", width = width))
    } else {
        let split = raw.len() - width;
        (raw[..split].to_string(), raw[split..].to_string())
    };
    fraction.truncate(max_fraction_digits.min(decimals) as usize);
    while fraction.ends_with('0') {
        fraction.pop();
    }
    if fraction.is_empty() {
        whole
    } else {
        format!("{whole}.{fraction}")
    }
}

fn bonk_estimate_supply_percent(amount: &BigUint, supply: &BigUint) -> String {
    if supply == &BigUint::ZERO {
        return "0".to_string();
    }
    let scaled = (amount * BigUint::from(100_000_000u64)) / supply;
    format_biguint_decimal(&scaled, 6, 4)
}

fn bonk_big_sub(left: &BigUint, right: &BigUint, label: &str) -> Result<BigUint, String> {
    if left < right {
        return Err(format!("Bonk {label} underflow."));
    }
    Ok(left - right)
}

fn bonk_ceil_div(amount_a: &BigUint, amount_b: &BigUint) -> BigUint {
    if amount_a == &BigUint::ZERO {
        BigUint::ZERO
    } else {
        (amount_a + amount_b - BigUint::from(1u8)) / amount_b
    }
}

fn bonk_biguint_sqrt_floor(value: &BigUint) -> BigUint {
    if value <= &BigUint::from(1u8) {
        return value.clone();
    }
    let mut current = BigUint::from(1u8) << (value.bits() as usize).div_ceil(2);
    loop {
        let next = (&current + (value / &current)) >> 1usize;
        if next >= current {
            return current;
        }
        current = next;
    }
}

fn bonk_biguint_sqrt_round(value: &BigUint) -> BigUint {
    let floor = bonk_biguint_sqrt_floor(value);
    let floor_squared = &floor * &floor;
    let remainder = value - &floor_squared;
    if remainder > floor {
        floor + BigUint::from(1u8)
    } else {
        floor
    }
}

fn decode_bonk_launchpad_config(data: &[u8]) -> Result<DecodedBonkLaunchpadConfig, String> {
    let mut offset = 0usize;
    let _discriminator = read_bonk_u64(data, &mut offset)?;
    let _epoch = read_bonk_u64(data, &mut offset)?;
    let curve_type = read_bonk_u8(data, &mut offset)?;
    offset += 2;
    let migrate_fee = read_bonk_u64(data, &mut offset)?;
    let trade_fee_rate = read_bonk_u64(data, &mut offset)?;
    Ok(DecodedBonkLaunchpadConfig {
        curve_type,
        migrate_fee,
        trade_fee_rate,
    })
}

fn decode_bonk_platform_config(data: &[u8]) -> Result<DecodedBonkPlatformConfig, String> {
    let mut offset = 0usize;
    let _discriminator = read_bonk_u64(data, &mut offset)?;
    let _epoch = read_bonk_u64(data, &mut offset)?;
    let _platform_claim_fee_wallet = read_bonk_pubkey(data, &mut offset)?;
    let _platform_lock_nft_wallet = read_bonk_pubkey(data, &mut offset)?;
    let _platform_scale = read_bonk_u64(data, &mut offset)?;
    let _creator_scale = read_bonk_u64(data, &mut offset)?;
    let _burn_scale = read_bonk_u64(data, &mut offset)?;
    let fee_rate = read_bonk_u64(data, &mut offset)?;
    offset += 64 + 256 + 256;
    let _cp_config_id = read_bonk_pubkey(data, &mut offset)?;
    let creator_fee_rate = read_bonk_u64(data, &mut offset)?;
    Ok(DecodedBonkPlatformConfig {
        fee_rate,
        creator_fee_rate,
    })
}

fn bonk_total_fee_rate(
    trade_fee_rate: &BigUint,
    platform_fee_rate: &BigUint,
    creator_fee_rate: &BigUint,
) -> Result<BigUint, String> {
    let total = trade_fee_rate + platform_fee_rate + creator_fee_rate;
    if total > bonk_biguint_from_u64(BONK_FEE_RATE_DENOMINATOR) {
        return Err("total fee rate gt 1_000_000".to_string());
    }
    Ok(total)
}

fn bonk_calculate_fee(amount: &BigUint, fee_rate: &BigUint) -> BigUint {
    let numerator = amount * fee_rate;
    bonk_ceil_div(
        &numerator,
        &bonk_biguint_from_u64(BONK_FEE_RATE_DENOMINATOR),
    )
}

fn bonk_calculate_pre_fee(
    post_fee_amount: &BigUint,
    fee_rate: &BigUint,
) -> Result<BigUint, String> {
    if fee_rate == &BigUint::ZERO {
        return Ok(post_fee_amount.clone());
    }
    let denominator = bonk_big_sub(
        &bonk_biguint_from_u64(BONK_FEE_RATE_DENOMINATOR),
        fee_rate,
        "fee denominator",
    )?;
    if denominator == BigUint::ZERO {
        return Err("Bonk fee denominator was zero.".to_string());
    }
    let numerator = post_fee_amount * bonk_biguint_from_u64(BONK_FEE_RATE_DENOMINATOR);
    Ok((numerator + &denominator - BigUint::from(1u8)) / denominator)
}

fn bonk_curve_init_virtuals(
    curve_type: u8,
    supply: &BigUint,
    total_fund_raising: &BigUint,
    total_sell: &BigUint,
    total_locked_amount: &BigUint,
    migrate_fee: &BigUint,
) -> Result<(BigUint, BigUint), String> {
    match curve_type {
        0 => {
            if supply <= total_sell {
                return Err("supply need gt total sell".to_string());
            }
            let supply_minus_sell_locked = bonk_big_sub(
                &bonk_big_sub(supply, total_sell, "supply minus total sell")?,
                total_locked_amount,
                "supply minus locked amount",
            )?;
            if supply_minus_sell_locked == BigUint::ZERO {
                return Err("supplyMinusSellLocked <= 0".to_string());
            }
            let tf_minus_mf = bonk_big_sub(
                total_fund_raising,
                migrate_fee,
                "total fund raising minus migrate fee",
            )?;
            if tf_minus_mf == BigUint::ZERO {
                return Err("tfMinusMf <= 0".to_string());
            }
            let numerator = ((&tf_minus_mf * total_sell) * total_sell) / &supply_minus_sell_locked;
            let denominator_base = (&tf_minus_mf * total_sell) / &supply_minus_sell_locked;
            let denominator = bonk_big_sub(
                &denominator_base,
                total_fund_raising,
                "constant-product denominator",
            )?;
            if denominator == BigUint::ZERO {
                return Err("invalid input 0".to_string());
            }
            Ok((
                numerator / &denominator,
                (total_fund_raising * total_fund_raising) / denominator,
            ))
        }
        1 => {
            let supply_minus_locked =
                bonk_big_sub(supply, total_locked_amount, "supply minus locked amount")?;
            if supply_minus_locked == BigUint::ZERO {
                return Err("invalid input 1".to_string());
            }
            let denominator = bonk_big_sub(
                &(BigUint::from(2u8) * total_fund_raising),
                migrate_fee,
                "fixed-price denominator",
            )?;
            if denominator == BigUint::ZERO {
                return Err("invalid input 0".to_string());
            }
            let total_sell_expect = (total_fund_raising * supply_minus_locked) / &denominator;
            Ok((total_sell_expect, total_fund_raising.clone()))
        }
        2 => {
            let supply_minus_locked =
                bonk_big_sub(supply, total_locked_amount, "supply minus locked amount")?;
            if supply_minus_locked == BigUint::ZERO {
                return Err("supplyMinusLocked need gt 0".to_string());
            }
            let denominator = bonk_big_sub(
                &(BigUint::from(3u8) * total_fund_raising),
                migrate_fee,
                "linear-price denominator",
            )?;
            if denominator == BigUint::ZERO {
                return Err("invalid input 0".to_string());
            }
            let numerator = (BigUint::from(2u8) * total_fund_raising) * supply_minus_locked;
            let total_sell_expect = numerator / &denominator;
            let total_sell_squared = &total_sell_expect * &total_sell_expect;
            if total_sell_squared == BigUint::ZERO {
                return Err("a need gt 0".to_string());
            }
            let a = ((BigUint::from(2u8) * total_fund_raising) * bonk_q64()) / total_sell_squared;
            if a == BigUint::ZERO {
                return Err("a need gt 0".to_string());
            }
            Ok((a, BigUint::ZERO))
        }
        _ => Err("find curve error".to_string()),
    }
}

fn bonk_curve_buy_exact_in(
    pool: &BonkCurvePoolState,
    curve_type: u8,
    amount: &BigUint,
) -> Result<BigUint, String> {
    match curve_type {
        0 => {
            let input_reserve = &pool.virtual_b + &pool.real_b;
            let output_reserve =
                bonk_big_sub(&pool.virtual_a, &pool.real_a, "launch output reserve")?;
            Ok((amount * output_reserve) / (input_reserve + amount))
        }
        1 => {
            if pool.virtual_b == BigUint::ZERO {
                return Err("Bonk fixed-price virtual quote reserve was zero.".to_string());
            }
            Ok((&pool.virtual_a * amount) / &pool.virtual_b)
        }
        2 => {
            if pool.virtual_a == BigUint::ZERO {
                return Err("Bonk linear-price virtual coefficient was zero.".to_string());
            }
            let new_quote = &pool.real_b + amount;
            let term_inside_sqrt = (BigUint::from(2u8) * new_quote * bonk_q64()) / &pool.virtual_a;
            let sqrt_term = bonk_biguint_sqrt_round(&term_inside_sqrt);
            bonk_big_sub(&sqrt_term, &pool.real_a, "linear-price amount out")
        }
        _ => Err("find curve error".to_string()),
    }
}

fn bonk_curve_buy_exact_out(
    pool: &BonkCurvePoolState,
    curve_type: u8,
    amount: &BigUint,
) -> Result<BigUint, String> {
    match curve_type {
        0 => {
            let input_reserve = &pool.virtual_b + &pool.real_b;
            let output_reserve =
                bonk_big_sub(&pool.virtual_a, &pool.real_a, "launch output reserve")?;
            let denominator =
                bonk_big_sub(&output_reserve, amount, "launch remaining output reserve")?;
            if denominator == BigUint::ZERO {
                return Err("Bonk constant-product buyExactOut denominator was zero.".to_string());
            }
            Ok(bonk_ceil_div(&(input_reserve * amount), &denominator))
        }
        1 => {
            if pool.virtual_a == BigUint::ZERO {
                return Err("Bonk fixed-price virtual token reserve was zero.".to_string());
            }
            Ok(bonk_ceil_div(&(&pool.virtual_b * amount), &pool.virtual_a))
        }
        2 => {
            let new_base = &pool.real_a + amount;
            let new_base_squared = &new_base * &new_base;
            let denominator = BigUint::from(2u8) * bonk_q64();
            let new_quote = bonk_ceil_div(&(&pool.virtual_a * new_base_squared), &denominator);
            bonk_big_sub(&new_quote, &pool.real_b, "linear-price amount in")
        }
        _ => Err("find curve error".to_string()),
    }
}

fn bonk_quote_buy_exact_in_amount_a(
    defaults: &BonkLaunchDefaults,
    amount_b: &BigUint,
) -> Result<BigUint, String> {
    let fee_rate = bonk_total_fee_rate(
        &defaults.trade_fee_rate,
        &defaults.platform_fee_rate,
        &defaults.creator_fee_rate,
    )?;
    let total_fee = bonk_calculate_fee(amount_b, &fee_rate);
    let amount_less_fee_b = bonk_big_sub(amount_b, &total_fee, "buy input after fee")?;
    let quoted_amount_a =
        bonk_curve_buy_exact_in(&defaults.pool, defaults.curve_type, &amount_less_fee_b)?;
    let remaining_amount_a = bonk_big_sub(
        &defaults.pool.total_sell_a,
        &defaults.pool.real_a,
        "remaining sell amount",
    )?;
    if quoted_amount_a > remaining_amount_a {
        Ok(remaining_amount_a)
    } else {
        Ok(quoted_amount_a)
    }
}

fn bonk_quote_buy_exact_out_amount_b(
    defaults: &BonkLaunchDefaults,
    requested_amount_a: &BigUint,
) -> Result<BigUint, String> {
    let remaining_amount_a = bonk_big_sub(
        &defaults.pool.total_sell_a,
        &defaults.pool.real_a,
        "remaining sell amount",
    )?;
    let real_amount_a = if requested_amount_a > &remaining_amount_a {
        remaining_amount_a
    } else {
        requested_amount_a.clone()
    };
    let amount_in_less_fee_b =
        bonk_curve_buy_exact_out(&defaults.pool, defaults.curve_type, &real_amount_a)?;
    let fee_rate = bonk_total_fee_rate(
        &defaults.trade_fee_rate,
        &defaults.platform_fee_rate,
        &defaults.creator_fee_rate,
    )?;
    bonk_calculate_pre_fee(&amount_in_less_fee_b, &fee_rate)
}

fn bonk_curve_sell_exact_in(
    pool: &BonkCurvePoolState,
    curve_type: u8,
    amount: &BigUint,
) -> Result<BigUint, String> {
    match curve_type {
        0 => {
            let input_reserve =
                bonk_big_sub(&pool.virtual_a, &pool.real_a, "launch input reserve")?;
            let output_reserve = &pool.virtual_b + &pool.real_b;
            Ok((amount * output_reserve) / (input_reserve + amount))
        }
        1 => {
            if pool.virtual_a == BigUint::ZERO {
                return Err("Bonk fixed-price virtual token reserve was zero.".to_string());
            }
            Ok((&pool.virtual_b * amount) / &pool.virtual_a)
        }
        2 => {
            let new_base = bonk_big_sub(&pool.real_a, amount, "linear-price new base")?;
            let new_base_squared = &new_base * &new_base;
            let denominator = BigUint::from(2u8) * bonk_q64();
            let new_quote = bonk_ceil_div(&(&pool.virtual_a * new_base_squared), &denominator);
            bonk_big_sub(&pool.real_b, &new_quote, "linear-price sell output")
        }
        _ => Err("find curve error".to_string()),
    }
}

fn bonk_curve_sell_exact_out(
    pool: &BonkCurvePoolState,
    curve_type: u8,
    amount: &BigUint,
) -> Result<BigUint, String> {
    match curve_type {
        0 => {
            let input_reserve =
                bonk_big_sub(&pool.virtual_a, &pool.real_a, "launch input reserve")?;
            let output_reserve = &pool.virtual_b + &pool.real_b;
            let denominator =
                bonk_big_sub(&output_reserve, amount, "launch remaining output reserve")?;
            if denominator == BigUint::ZERO {
                return Err("Bonk constant-product sellExactOut denominator was zero.".to_string());
            }
            Ok(bonk_ceil_div(&(input_reserve * amount), &denominator))
        }
        1 => {
            if pool.virtual_b == BigUint::ZERO {
                return Err("Bonk fixed-price virtual quote reserve was zero.".to_string());
            }
            Ok(bonk_ceil_div(&(&pool.virtual_a * amount), &pool.virtual_b))
        }
        2 => {
            let new_quote = bonk_big_sub(&pool.real_b, amount, "linear-price new quote")?;
            if pool.virtual_a == BigUint::ZERO {
                return Err("Bonk linear-price virtual coefficient was zero.".to_string());
            }
            let term_inside_sqrt = (BigUint::from(2u8) * new_quote * bonk_q64()) / &pool.virtual_a;
            let sqrt_term = bonk_biguint_sqrt_round(&term_inside_sqrt);
            bonk_big_sub(&pool.real_a, &sqrt_term, "linear-price sell input")
        }
        _ => Err("find curve error".to_string()),
    }
}

fn bonk_quote_sell_exact_in_amount_b(
    pool: &BonkCurvePoolState,
    curve_type: u8,
    trade_fee_rate: &BigUint,
    platform_fee_rate: &BigUint,
    creator_fee_rate: &BigUint,
    amount_a: &BigUint,
) -> Result<BigUint, String> {
    let quoted_amount_b = bonk_curve_sell_exact_in(pool, curve_type, amount_a)?;
    let fee_rate = bonk_total_fee_rate(trade_fee_rate, platform_fee_rate, creator_fee_rate)?;
    let total_fee = bonk_calculate_fee(&quoted_amount_b, &fee_rate);
    bonk_big_sub(&quoted_amount_b, &total_fee, "sell output after fee")
}

fn bonk_quote_sell_exact_out_amount_a(
    pool: &BonkCurvePoolState,
    curve_type: u8,
    trade_fee_rate: &BigUint,
    platform_fee_rate: &BigUint,
    creator_fee_rate: &BigUint,
    amount_b: &BigUint,
) -> Result<BigUint, String> {
    let fee_rate = bonk_total_fee_rate(trade_fee_rate, platform_fee_rate, creator_fee_rate)?;
    let amount_out_with_fee_b = bonk_calculate_pre_fee(amount_b, &fee_rate)?;
    if pool.real_b < amount_out_with_fee_b {
        return Err("Insufficient liquidity".to_string());
    }
    let amount_a = bonk_curve_sell_exact_out(pool, curve_type, &amount_out_with_fee_b)?;
    if amount_a > pool.real_a {
        return Err("Insufficient launch token liquidity".to_string());
    }
    Ok(amount_a)
}

fn bonk_build_min_amount_from_bps(amount: &BigUint, slippage_bps: u64) -> BigUint {
    let safe_bps = slippage_bps.min(10_000);
    let minimum = (amount * BigUint::from(10_000u64 - safe_bps)) / BigUint::from(10_000u64);
    if amount > &BigUint::from(0u8) && minimum == BigUint::from(0u8) {
        BigUint::from(1u8)
    } else {
        minimum
    }
}

fn biguint_to_u64(value: &BigUint, label: &str) -> Result<u64, String> {
    value
        .to_string()
        .parse::<u64>()
        .map_err(|error| format!("Invalid Bonk {label}: {error}"))
}

fn biguint_to_u128(value: &BigUint, label: &str) -> Result<u128, String> {
    value
        .to_u128()
        .ok_or_else(|| format!("Invalid Bonk {label}: value exceeds u128"))
}

fn bonk_clmm_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(BONK_CLMM_PROGRAM_ID)
        .map_err(|error| format!("Invalid Bonk CLMM program id: {error}"))
}

fn bonk_clmm_q64() -> BigUint {
    BigUint::from(1u8) << 64usize
}

fn bonk_clmm_q128() -> BigUint {
    BigUint::from(1u8) << 128usize
}

fn bonk_biguint_from_u128(value: u128) -> BigUint {
    BigUint::from(value)
}

fn bonk_pow10_biguint(decimals: u32) -> BigUint {
    BigUint::from(10u8).pow(decimals)
}

fn bonk_get_tick_array_start_index_by_tick(tick_index: i32, tick_spacing: i32) -> i32 {
    let tick_count = BONK_CLMM_TICK_ARRAY_SIZE * tick_spacing;
    tick_index.div_euclid(tick_count) * tick_count
}

fn bonk_tick_array_bit_position(start_index: i32, tick_spacing: i32) -> Result<usize, String> {
    let tick_count = BONK_CLMM_TICK_ARRAY_SIZE * tick_spacing;
    if tick_count <= 0 || start_index % tick_count != 0 {
        return Err("Invalid Bonk CLMM tick array start index.".to_string());
    }
    let bit_position = start_index.div_euclid(tick_count) + BONK_CLMM_DEFAULT_BITMAP_OFFSET;
    if !(0..1024).contains(&bit_position) {
        return Err("Bonk USD1 CLMM quote exceeded default bitmap coverage.".to_string());
    }
    usize::try_from(bit_position)
        .map_err(|error| format!("Invalid Bonk CLMM bitmap index: {error}"))
}

fn bonk_bitmap_is_initialized(bitmap_words: &[u64; 16], bit_position: usize) -> bool {
    let word = bitmap_words[bit_position / 64];
    (word & (1u64 << (bit_position % 64))) != 0
}

fn bonk_derive_clmm_tick_array_address(
    program_id: &Pubkey,
    pool_id: &Pubkey,
    start_index: i32,
) -> Pubkey {
    let (address, _) = Pubkey::find_program_address(
        &[b"tick_array", pool_id.as_ref(), &start_index.to_be_bytes()],
        program_id,
    );
    address
}

fn bonk_mul_div_floor(
    left: &BigUint,
    right: &BigUint,
    denominator: &BigUint,
) -> Result<BigUint, String> {
    if denominator == &BigUint::ZERO {
        return Err("Bonk CLMM division by zero.".to_string());
    }
    Ok((left * right) / denominator)
}

fn bonk_mul_div_ceil(
    left: &BigUint,
    right: &BigUint,
    denominator: &BigUint,
) -> Result<BigUint, String> {
    if denominator == &BigUint::ZERO {
        return Err("Bonk CLMM division by zero.".to_string());
    }
    Ok(((left * right) + denominator - BigUint::from(1u8)) / denominator)
}

fn bonk_get_token_amount_a_from_liquidity(
    mut sqrt_price_a_x64: BigUint,
    mut sqrt_price_b_x64: BigUint,
    liquidity: &BigUint,
    round_up: bool,
) -> Result<BigUint, String> {
    if sqrt_price_a_x64 > sqrt_price_b_x64 {
        std::mem::swap(&mut sqrt_price_a_x64, &mut sqrt_price_b_x64);
    }
    if sqrt_price_a_x64 == BigUint::ZERO {
        return Err("Bonk CLMM sqrt price must be greater than zero.".to_string());
    }
    let numerator1 = liquidity << 64usize;
    let numerator2 = &sqrt_price_b_x64 - &sqrt_price_a_x64;
    if round_up {
        let intermediate = bonk_mul_div_ceil(&numerator1, &numerator2, &sqrt_price_b_x64)?;
        Ok(bonk_ceil_div(&intermediate, &sqrt_price_a_x64))
    } else {
        Ok(bonk_mul_div_floor(&numerator1, &numerator2, &sqrt_price_b_x64)? / &sqrt_price_a_x64)
    }
}

fn bonk_get_token_amount_b_from_liquidity(
    mut sqrt_price_a_x64: BigUint,
    mut sqrt_price_b_x64: BigUint,
    liquidity: &BigUint,
    round_up: bool,
) -> Result<BigUint, String> {
    if sqrt_price_a_x64 > sqrt_price_b_x64 {
        std::mem::swap(&mut sqrt_price_a_x64, &mut sqrt_price_b_x64);
    }
    if sqrt_price_a_x64 == BigUint::ZERO {
        return Err("Bonk CLMM sqrt price must be greater than zero.".to_string());
    }
    if round_up {
        bonk_mul_div_ceil(
            liquidity,
            &(&sqrt_price_b_x64 - &sqrt_price_a_x64),
            &bonk_clmm_q64(),
        )
    } else {
        bonk_mul_div_floor(
            liquidity,
            &(&sqrt_price_b_x64 - &sqrt_price_a_x64),
            &bonk_clmm_q64(),
        )
    }
}

fn bonk_get_next_sqrt_price_from_token_amount_a_rounding_up(
    sqrt_price_x64: &BigUint,
    liquidity: &BigUint,
    amount: &BigUint,
    add: bool,
) -> Result<BigUint, String> {
    if amount == &BigUint::ZERO {
        return Ok(sqrt_price_x64.clone());
    }
    let liquidity_left_shift = liquidity << 64usize;
    if add {
        let denominator = &liquidity_left_shift + (amount * sqrt_price_x64);
        if denominator >= liquidity_left_shift {
            bonk_mul_div_ceil(&liquidity_left_shift, sqrt_price_x64, &denominator)
        } else {
            let fallback_denominator = (&liquidity_left_shift / sqrt_price_x64) + amount;
            bonk_mul_div_ceil(
                &liquidity_left_shift,
                &BigUint::from(1u8),
                &fallback_denominator,
            )
        }
    } else {
        let amount_mul_sqrt_price = amount * sqrt_price_x64;
        if liquidity_left_shift <= amount_mul_sqrt_price {
            return Err(
                "Bonk CLMM liquidity shift must exceed amount * sqrt price for output quotes."
                    .to_string(),
            );
        }
        let denominator = &liquidity_left_shift - amount_mul_sqrt_price;
        bonk_mul_div_ceil(&liquidity_left_shift, sqrt_price_x64, &denominator)
    }
}

fn bonk_get_next_sqrt_price_from_input_zero_for_one(
    sqrt_price_x64: &BigUint,
    liquidity: &BigUint,
    amount_in: &BigUint,
) -> Result<BigUint, String> {
    bonk_get_next_sqrt_price_from_token_amount_a_rounding_up(
        sqrt_price_x64,
        liquidity,
        amount_in,
        true,
    )
}

fn bonk_get_next_sqrt_price_from_input_one_for_zero(
    sqrt_price_x64: &BigUint,
    liquidity: &BigUint,
    amount_in: &BigUint,
) -> Result<BigUint, String> {
    if liquidity == &BigUint::ZERO {
        return Err("Bonk CLMM liquidity must be greater than zero.".to_string());
    }
    Ok(sqrt_price_x64 + bonk_mul_div_floor(amount_in, &bonk_clmm_q64(), liquidity)?)
}

fn bonk_sqrt_price_from_tick(tick: i32) -> Result<BigUint, String> {
    const FACTORS: &[(u32, u64)] = &[
        (0x2, 18_444_899_583_751_176_192),
        (0x4, 18_443_055_278_223_355_904),
        (0x8, 18_439_367_220_385_607_680),
        (0x10, 18_431_993_317_065_453_568),
        (0x20, 18_417_254_355_718_170_624),
        (0x40, 18_387_811_781_193_609_216),
        (0x80, 18_329_067_761_203_558_400),
        (0x100, 18_212_142_134_806_163_456),
        (0x200, 17_980_523_815_641_700_352),
        (0x400, 17_526_086_738_831_433_728),
        (0x800, 16_651_378_430_235_570_176),
        (0x1000, 15_030_750_278_694_412_288),
        (0x2000, 12_247_334_978_884_435_968),
        (0x4000, 8_131_365_268_886_854_656),
        (0x8000, 3_584_323_654_725_218_816),
        (0x10000, 696_457_651_848_324_352),
        (0x20000, 26_294_789_957_507_116),
        (0x40000, 37_481_735_321_082),
    ];

    let tick_abs = tick.unsigned_abs();
    let mut ratio = if (tick_abs & 0x1) != 0 {
        BigUint::from(18_445_821_805_675_395_072u64)
    } else {
        BigUint::from(1u8) << 64usize
    };
    for (mask, factor) in FACTORS {
        if (tick_abs & mask) != 0 {
            ratio = (&ratio * BigUint::from(*factor)) >> 64usize;
        }
    }
    if tick > 0 {
        ratio = (bonk_clmm_q128() - BigUint::from(1u8)) / ratio;
    }
    Ok(ratio)
}

fn bonk_sqrt_price_x64_to_price(
    sqrt_price_x64: &BigUint,
    decimals_a: u32,
    decimals_b: u32,
) -> Result<f64, String> {
    let numerator = sqrt_price_x64 * sqrt_price_x64 * bonk_pow10_biguint(decimals_a);
    let denominator = bonk_clmm_q128() * bonk_pow10_biguint(decimals_b);
    let numerator_f64 = numerator
        .to_f64()
        .ok_or_else(|| "Bonk CLMM numerator was too large to format.".to_string())?;
    let denominator_f64 = denominator
        .to_f64()
        .ok_or_else(|| "Bonk CLMM denominator was too large to format.".to_string())?;
    Ok(numerator_f64 / denominator_f64)
}

fn bonk_apply_liquidity_delta(liquidity: &BigUint, liquidity_net: i128) -> Result<BigUint, String> {
    if liquidity_net >= 0 {
        bonk_big_sub(
            liquidity,
            &bonk_biguint_from_u128(liquidity_net as u128),
            "CLMM liquidity delta",
        )
    } else {
        Ok(liquidity + bonk_biguint_from_u128(liquidity_net.unsigned_abs()))
    }
}

fn bonk_usd1_search_tolerance_lamports(high: &BigUint) -> BigUint {
    let search_tolerance_bps = std::env::var("BONK_USD1_SEARCH_TOLERANCE_BPS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(50);
    let search_min_lamports = std::env::var("BONK_USD1_SEARCH_MIN_LAMPORTS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(50_000);
    let bps_lamports = (high * BigUint::from(search_tolerance_bps)) / BigUint::from(10_000u64);
    bonk_biguint_from_u64(search_min_lamports).max(bps_lamports)
}

fn bonk_usd1_min_remaining_lamports() -> Result<u64, String> {
    parse_decimal_u64(
        &std::env::var("BONK_USD1_MIN_REMAINING_SOL").unwrap_or_else(|_| "0.02".to_string()),
        9,
        "BONK_USD1_MIN_REMAINING_SOL",
    )
}

async fn bonk_rpc_get_balance_lamports(rpc_url: &str, owner: &Pubkey) -> Result<u64, String> {
    let response = bonk_http_client()
        .post(rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getBalance",
            "params": [owner.to_string(), "processed"],
        }))
        .send()
        .await
        .map_err(|error| format!("Failed to fetch Bonk owner SOL balance: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch Bonk owner SOL balance: status {}.",
            response.status()
        ));
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|error| format!("Failed to parse Bonk owner SOL balance response: {error}"))?;
    payload
        .get("result")
        .and_then(|result| result.get("value"))
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            "Bonk owner SOL balance response did not include a numeric value.".to_string()
        })
}

async fn native_prepare_bonk_usd1_topup(
    rpc_url: &str,
    commitment: &str,
    owner: &Pubkey,
    required_quote_amount: &BigUint,
    slippage_bps: u64,
    mode: BonkUsd1TopupMode,
    mut metrics: Option<&mut HelperUsd1QuoteMetrics>,
    route_setup_override: Option<&BonkUsd1RouteSetup>,
) -> Result<NativeBonkPreparedUsd1Topup, String> {
    let usd1_mint = bonk_quote_mint("usd1")?;
    let current_quote_amount = bonk_biguint_from_u64(
        fetch_bonk_owner_token_balance(rpc_url, "processed", owner, &usd1_mint)
            .await?
            .unwrap_or(0),
    );
    if matches!(mode, BonkUsd1TopupMode::RespectExistingBalance)
        && current_quote_amount >= *required_quote_amount
    {
        return Ok(NativeBonkPreparedUsd1Topup {
            required_quote_amount: required_quote_amount.clone(),
            current_quote_amount,
            shortfall_quote_amount: BigUint::ZERO,
            input_lamports: None,
            expected_quote_out: None,
            min_quote_out: None,
            traversed_tick_array_starts: vec![],
        });
    }
    let shortfall_quote_amount = if matches!(mode, BonkUsd1TopupMode::ForceFullAmount) {
        required_quote_amount.clone()
    } else {
        bonk_big_sub(
            required_quote_amount,
            &current_quote_amount,
            "Bonk USD1 shortfall amount",
        )?
    };
    let balance_lamports = bonk_rpc_get_balance_lamports(rpc_url, owner).await?;
    let min_remaining_lamports = bonk_usd1_min_remaining_lamports()?;
    let max_spendable_lamports = balance_lamports.saturating_sub(min_remaining_lamports);
    if max_spendable_lamports == 0 {
        return Err(format!(
            "Insufficient SOL headroom for USD1 top-up. Need at least {} SOL reserved after swap.",
            std::env::var("BONK_USD1_MIN_REMAINING_SOL").unwrap_or_else(|_| "0.02".to_string())
        ));
    }
    let input_lamports = native_quote_sol_input_for_usd1_output_with_max_and_metrics(
        rpc_url,
        &shortfall_quote_amount,
        slippage_bps,
        Some(BigUint::from(max_spendable_lamports)),
        metrics.as_deref_mut(),
        route_setup_override,
    )
    .await?;
    let quote = native_quote_usd1_output_from_sol_input_with_metrics(
        rpc_url,
        &input_lamports,
        slippage_bps,
        metrics.as_deref_mut(),
        route_setup_override,
    )
    .await?;
    if quote.min_out < shortfall_quote_amount {
        return Err("Native Bonk USD1 top-up quote could not satisfy required output.".to_string());
    }
    let _ = commitment;
    Ok(NativeBonkPreparedUsd1Topup {
        required_quote_amount: required_quote_amount.clone(),
        current_quote_amount,
        shortfall_quote_amount,
        input_lamports: Some(input_lamports),
        expected_quote_out: Some(quote.expected_out),
        min_quote_out: Some(quote.min_out),
        traversed_tick_array_starts: quote.traversed_tick_array_starts,
    })
}

fn gross_lamports_for_net_after_wrapper_fee(
    net_lamports: u64,
    fee_bps: u16,
) -> Result<u64, String> {
    if net_lamports == 0 || fee_bps == 0 {
        return Ok(net_lamports);
    }
    let denominator = 10_000u64
        .checked_sub(u64::from(fee_bps))
        .ok_or_else(|| "Bonk wrapper fee bps exceeded denominator.".to_string())?;
    if denominator == 0 {
        return Err("Bonk wrapper fee bps leaves zero venue input.".to_string());
    }
    let mut gross = ((u128::from(net_lamports) * 10_000u128) + u128::from(denominator - 1))
        / u128::from(denominator);
    loop {
        let gross_u64 = u64::try_from(gross)
            .map_err(|_| "Bonk wrapper gross SOL input overflowed u64.".to_string())?;
        let fee = estimate_sol_in_fee_lamports(gross_u64, fee_bps);
        let candidate_net = gross_u64
            .checked_sub(fee)
            .ok_or_else(|| "Bonk wrapper fee exceeds gross SOL input.".to_string())?;
        if candidate_net < net_lamports {
            gross = gross
                .checked_add(1)
                .ok_or_else(|| "Bonk wrapper gross SOL input overflowed.".to_string())?;
            continue;
        }
        return Ok(gross_u64);
    }
}

fn prepared_topup_with_wrapper_gross_input(
    prepared: &NativeBonkPreparedUsd1Topup,
    fee_bps: u16,
) -> Result<NativeBonkPreparedUsd1Topup, String> {
    let Some(input_lamports) = prepared.input_lamports.as_ref() else {
        return Ok(prepared.clone());
    };
    let net_lamports = biguint_to_u64(input_lamports, "Bonk USD1 top-up net input lamports")?;
    let gross_lamports = gross_lamports_for_net_after_wrapper_fee(net_lamports, fee_bps)?;
    let mut adjusted = prepared.clone();
    adjusted.input_lamports = Some(bonk_biguint_from_u64(gross_lamports));
    Ok(adjusted)
}

fn bonk_build_usd1_search_guess_lamports(
    required_quote_amount: &BigUint,
    reference_price: f64,
    max_input_lamports: &BigUint,
) -> Result<BigUint, String> {
    let guess = if reference_price.is_finite() && reference_price > 0.0 {
        let required_quote = required_quote_amount
            .to_f64()
            .ok_or_else(|| "Bonk USD1 quote amount was too large to estimate.".to_string())?
            / 1_000_000f64;
        let guess_sol = ((required_quote / reference_price) * 1.05f64).max(0.01f64);
        parse_decimal_biguint(&format!("{guess_sol:.9}"), 9, "top-up search guess")?
    } else {
        parse_decimal_biguint("0.01", 9, "top-up search floor")?
    };
    Ok(std::cmp::min(guess, max_input_lamports.clone()))
}

async fn rpc_get_multiple_accounts_data(
    rpc_url: &str,
    addresses: &[String],
    commitment: &str,
) -> Result<Vec<Vec<u8>>, String> {
    if addresses.is_empty() {
        return Ok(Vec::new());
    }
    let payload = json!({
        "jsonrpc": "2.0",
        "id": "launchdeck-bonk-multiple-accounts",
        "method": "getMultipleAccounts",
        "params": [
            addresses,
            {
                "encoding": "base64",
                "commitment": commitment,
            }
        ]
    });
    let response = bonk_http_client()
        .post(rpc_url)
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("Failed to fetch Bonk multiple accounts: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch Bonk multiple accounts: RPC returned status {}.",
            response.status()
        ));
    }
    let parsed: RpcMultipleAccountsResponse = response
        .json()
        .await
        .map_err(|error| format!("Failed to parse Bonk multiple accounts response: {error}"))?;
    parsed
        .result
        .value
        .into_iter()
        .enumerate()
        .map(|(index, maybe_value)| {
            let value = maybe_value.ok_or_else(|| {
                format!(
                    "Bonk RPC did not return account data for {}.",
                    addresses.get(index).cloned().unwrap_or_default()
                )
            })?;
            BASE64.decode(value.data.0.trim()).map_err(|error| {
                format!(
                    "Failed to decode Bonk account {}: {error}",
                    addresses[index]
                )
            })
        })
        .collect()
}

async fn load_bonk_usd1_route_setup_with_metrics(
    rpc_url: &str,
    mut metrics: Option<&mut HelperUsd1QuoteMetrics>,
    force_refresh: bool,
) -> Result<BonkUsd1RouteSetup, String> {
    let cache_key = BONK_PINNED_USD1_ROUTE_POOL_ID.to_string();
    let ttl = bonk_usd1_route_setup_cache_ttl();
    if !force_refresh {
        if let Some(entry) = bonk_usd1_route_setup_cache()
            .lock()
            .expect("bonk usd1 route setup cache")
            .get(&cache_key)
            .filter(|entry| entry.fetched_at.elapsed() <= ttl)
            .cloned()
        {
            if let Some(metrics) = metrics.as_deref_mut() {
                metrics.routeSetupCacheHits = metrics.routeSetupCacheHits.saturating_add(1);
            }
            return Ok(entry.setup);
        }
    }
    if let Some(metrics) = metrics.as_deref_mut() {
        metrics.routeSetupCacheMisses = metrics.routeSetupCacheMisses.saturating_add(1);
    }
    let route_fetch_started = std::time::Instant::now();

    let pool_id = Pubkey::from_str(BONK_PINNED_USD1_ROUTE_POOL_ID)
        .map_err(|error| format!("Invalid Bonk USD1 route pool id: {error}"))?;
    let config_id = Pubkey::from_str(BONK_PREFERRED_USD1_ROUTE_CONFIG_ID)
        .map_err(|error| format!("Invalid Bonk USD1 route config id: {error}"))?;
    let program_id = bonk_clmm_program_id()?;

    let pool_data =
        fetch_account_data(rpc_url, BONK_PINNED_USD1_ROUTE_POOL_ID, "confirmed").await?;
    let pool = decode_bonk_clmm_pool(&pool_data)?;
    if pool.amm_config != config_id {
        return Err(format!(
            "Pinned USD1 route pool config changed: {BONK_PINNED_USD1_ROUTE_POOL_ID}"
        ));
    }
    let mint_a = pool.mint_a.to_string();
    let mint_b = pool.mint_b.to_string();
    let expected_pair = (mint_a == BONK_SOL_QUOTE_MINT && mint_b == BONK_USD1_QUOTE_MINT)
        || (mint_a == BONK_USD1_QUOTE_MINT && mint_b == BONK_SOL_QUOTE_MINT);
    if !expected_pair {
        return Err(format!(
            "Pinned USD1 route pool no longer matches SOL/USD1: {BONK_PINNED_USD1_ROUTE_POOL_ID}"
        ));
    }
    if mint_a != BONK_SOL_QUOTE_MINT || mint_b != BONK_USD1_QUOTE_MINT {
        return Err(
            "Native Bonk USD1 quote currently only supports SOL as CLMM mintA.".to_string(),
        );
    }
    let current_array_start =
        bonk_get_tick_array_start_index_by_tick(pool.tick_current, i32::from(pool.tick_spacing));
    let current_bit_position =
        bonk_tick_array_bit_position(current_array_start, i32::from(pool.tick_spacing))?;
    if !bonk_bitmap_is_initialized(&pool.tick_array_bitmap, current_bit_position) {
        return Err("Pinned Bonk USD1 CLMM current tick array is not initialized.".to_string());
    }

    let tick_count = BONK_CLMM_TICK_ARRAY_SIZE * i32::from(pool.tick_spacing);
    let initialized_bit_positions = (0..(pool.tick_array_bitmap.len() * 64))
        .filter(|bit_position| bonk_bitmap_is_initialized(&pool.tick_array_bitmap, *bit_position))
        .collect::<Vec<_>>();
    let tick_array_starts_desc = initialized_bit_positions
        .iter()
        .rev()
        .map(|bit_position| ((*bit_position as i32) - BONK_CLMM_DEFAULT_BITMAP_OFFSET) * tick_count)
        .collect::<Vec<_>>();
    let tick_array_starts_asc = initialized_bit_positions
        .iter()
        .map(|bit_position| ((*bit_position as i32) - BONK_CLMM_DEFAULT_BITMAP_OFFSET) * tick_count)
        .collect::<Vec<_>>();
    if tick_array_starts_desc.is_empty() {
        return Err("Pinned Bonk USD1 CLMM had no initialized tick arrays.".to_string());
    }

    let tick_array_addresses = tick_array_starts_desc
        .iter()
        .map(|start_index| {
            bonk_derive_clmm_tick_array_address(&program_id, &pool_id, *start_index).to_string()
        })
        .collect::<Vec<_>>();
    let tick_array_account_datas =
        rpc_get_multiple_accounts_data(rpc_url, &tick_array_addresses, "confirmed").await?;
    let tick_arrays = tick_array_account_datas
        .into_iter()
        .map(|data| decode_bonk_clmm_tick_array(&data))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|tick_array| (tick_array.start_tick_index, tick_array))
        .collect::<HashMap<_, _>>();
    if !tick_arrays.contains_key(&current_array_start) {
        return Err("Pinned Bonk USD1 CLMM current tick array could not be decoded.".to_string());
    }

    let config_data =
        fetch_account_data(rpc_url, BONK_PREFERRED_USD1_ROUTE_CONFIG_ID, "confirmed").await?;
    let config = decode_bonk_clmm_config(&config_data)?;
    if config.tick_spacing != pool.tick_spacing {
        return Err("Pinned Bonk USD1 CLMM tick spacing no longer matches its config.".to_string());
    }

    let setup = BonkUsd1RouteSetup {
        pool_id,
        program_id,
        amm_config: config_id,
        mint_a: pool.mint_a,
        mint_b: pool.mint_b,
        vault_a: pool.vault_a,
        vault_b: pool.vault_b,
        observation_id: pool.observation_id,
        tick_spacing: i32::from(pool.tick_spacing),
        trade_fee_rate: config.trade_fee_rate,
        sqrt_price_x64: pool.sqrt_price_x64.clone(),
        liquidity: pool.liquidity.clone(),
        tick_current: pool.tick_current,
        mint_a_decimals: u32::from(pool.mint_decimals_a),
        mint_b_decimals: u32::from(pool.mint_decimals_b),
        current_price: bonk_sqrt_price_x64_to_price(
            &pool.sqrt_price_x64,
            u32::from(pool.mint_decimals_a),
            u32::from(pool.mint_decimals_b),
        )?,
        tick_arrays_desc: tick_array_starts_desc,
        tick_arrays_asc: tick_array_starts_asc,
        tick_arrays,
    };
    bonk_usd1_route_setup_cache()
        .lock()
        .expect("bonk usd1 route setup cache")
        .insert(
            cache_key,
            BonkUsd1RouteSetupCacheEntry {
                fetched_at: std::time::Instant::now(),
                setup: setup.clone(),
            },
        );
    if let Some(metrics) = metrics.as_deref_mut() {
        metrics.routeSetupFetchMs = metrics
            .routeSetupFetchMs
            .saturating_add(route_fetch_started.elapsed().as_millis() as u64);
    }
    Ok(setup)
}

async fn load_bonk_usd1_route_setup(rpc_url: &str) -> Result<BonkUsd1RouteSetup, String> {
    load_bonk_usd1_route_setup_with_metrics(rpc_url, None, false).await
}

async fn load_bonk_usd1_route_setup_fresh(rpc_url: &str) -> Result<BonkUsd1RouteSetup, String> {
    load_bonk_usd1_route_setup_with_metrics(rpc_url, None, true).await
}

fn bonk_find_next_initialized_tick_zero_for_one(
    setup: &BonkUsd1RouteSetup,
    current_tick: i32,
) -> Result<BonkClmmTick, String> {
    let current_array_start =
        bonk_get_tick_array_start_index_by_tick(current_tick, setup.tick_spacing);
    let current_array = setup.tick_arrays.get(&current_array_start).ok_or_else(|| {
        format!("Missing Bonk CLMM tick array for start index {current_array_start}.")
    })?;
    let current_tick_position = (current_tick - current_array_start).div_euclid(setup.tick_spacing);
    for tick_index in (0..=current_tick_position).rev() {
        let tick = current_array
            .ticks
            .get(
                usize::try_from(tick_index)
                    .map_err(|error| format!("Invalid Bonk tick index: {error}"))?,
            )
            .ok_or_else(|| "Bonk CLMM current tick array index overflowed.".to_string())?;
        if tick.liquidity_gross > BigUint::ZERO {
            return Ok(tick.clone());
        }
    }
    let current_array_position = setup
        .tick_arrays_desc
        .iter()
        .position(|start_index| *start_index == current_array_start)
        .ok_or_else(|| {
            "Bonk CLMM current tick array was not present in the route setup.".to_string()
        })?;
    setup
        .tick_arrays_desc
        .iter()
        .skip(current_array_position + 1)
        .find_map(|start_index| {
            let tick_array = setup.tick_arrays.get(start_index)?;
            tick_array
                .ticks
                .iter()
                .rev()
                .find(|tick| tick.liquidity_gross > BigUint::ZERO)
                .cloned()
        })
        .ok_or_else(|| "swapCompute LiquidityInsufficient".to_string())
}

fn bonk_find_next_initialized_tick_one_for_zero(
    setup: &BonkUsd1RouteSetup,
    current_tick: i32,
) -> Result<BonkClmmTick, String> {
    let current_array_start =
        bonk_get_tick_array_start_index_by_tick(current_tick, setup.tick_spacing);
    let current_array = setup.tick_arrays.get(&current_array_start).ok_or_else(|| {
        format!("Missing Bonk CLMM tick array for start index {current_array_start}.")
    })?;
    let current_tick_position = (current_tick - current_array_start).div_euclid(setup.tick_spacing);
    for tick_index in (current_tick_position + 1)..BONK_CLMM_TICK_ARRAY_SIZE {
        let tick = current_array
            .ticks
            .get(
                usize::try_from(tick_index)
                    .map_err(|error| format!("Invalid Bonk tick index: {error}"))?,
            )
            .ok_or_else(|| "Bonk CLMM current tick array index overflowed.".to_string())?;
        if tick.liquidity_gross > BigUint::ZERO {
            return Ok(tick.clone());
        }
    }
    let current_array_position = setup
        .tick_arrays_asc
        .iter()
        .position(|start_index| *start_index == current_array_start)
        .ok_or_else(|| {
            "Bonk CLMM current tick array was not present in the route setup.".to_string()
        })?;
    setup
        .tick_arrays_asc
        .iter()
        .skip(current_array_position + 1)
        .find_map(|start_index| {
            let tick_array = setup.tick_arrays.get(start_index)?;
            tick_array
                .ticks
                .iter()
                .find(|tick| tick.liquidity_gross > BigUint::ZERO)
                .cloned()
        })
        .ok_or_else(|| "swapCompute LiquidityInsufficient".to_string())
}

fn bonk_clmm_swap_step_exact_in_zero_for_one(
    sqrt_price_current_x64: &BigUint,
    sqrt_price_target_x64: &BigUint,
    liquidity: &BigUint,
    amount_remaining: &BigUint,
    fee_rate: u32,
) -> Result<(BigUint, BigUint, BigUint, BigUint), String> {
    let fee_denominator = bonk_biguint_from_u64(BONK_FEE_RATE_DENOMINATOR);
    let fee_rate_big = bonk_biguint_from_u64(u64::from(fee_rate));
    let amount_remaining_less_fee =
        (amount_remaining * (&fee_denominator - &fee_rate_big)) / &fee_denominator;
    let amount_in_to_target = bonk_get_token_amount_a_from_liquidity(
        sqrt_price_target_x64.clone(),
        sqrt_price_current_x64.clone(),
        liquidity,
        true,
    )?;
    let next_sqrt_price_x64 = if amount_remaining_less_fee >= amount_in_to_target {
        sqrt_price_target_x64.clone()
    } else {
        bonk_get_next_sqrt_price_from_input_zero_for_one(
            sqrt_price_current_x64,
            liquidity,
            &amount_remaining_less_fee,
        )?
    };
    let reach_target_price = next_sqrt_price_x64 == *sqrt_price_target_x64;
    let amount_in = if reach_target_price {
        amount_in_to_target
    } else {
        bonk_get_token_amount_a_from_liquidity(
            next_sqrt_price_x64.clone(),
            sqrt_price_current_x64.clone(),
            liquidity,
            true,
        )?
    };
    let amount_out = bonk_get_token_amount_b_from_liquidity(
        next_sqrt_price_x64.clone(),
        sqrt_price_current_x64.clone(),
        liquidity,
        false,
    )?;
    let fee_amount = if !reach_target_price {
        bonk_big_sub(amount_remaining, &amount_in, "CLMM swap fee amount")?
    } else {
        bonk_mul_div_ceil(
            &amount_in,
            &fee_rate_big,
            &(&fee_denominator - &fee_rate_big),
        )?
    };
    Ok((next_sqrt_price_x64, amount_in, amount_out, fee_amount))
}

fn bonk_clmm_swap_step_exact_in_one_for_zero(
    sqrt_price_current_x64: &BigUint,
    sqrt_price_target_x64: &BigUint,
    liquidity: &BigUint,
    amount_remaining: &BigUint,
    fee_rate: u32,
) -> Result<(BigUint, BigUint, BigUint, BigUint), String> {
    let fee_denominator = bonk_biguint_from_u64(BONK_FEE_RATE_DENOMINATOR);
    let fee_rate_big = bonk_biguint_from_u64(u64::from(fee_rate));
    let amount_remaining_less_fee =
        (amount_remaining * (&fee_denominator - &fee_rate_big)) / &fee_denominator;
    let amount_in_to_target = bonk_get_token_amount_b_from_liquidity(
        sqrt_price_current_x64.clone(),
        sqrt_price_target_x64.clone(),
        liquidity,
        true,
    )?;
    let next_sqrt_price_x64 = if amount_remaining_less_fee >= amount_in_to_target {
        sqrt_price_target_x64.clone()
    } else {
        bonk_get_next_sqrt_price_from_input_one_for_zero(
            sqrt_price_current_x64,
            liquidity,
            &amount_remaining_less_fee,
        )?
    };
    let reach_target_price = next_sqrt_price_x64 == *sqrt_price_target_x64;
    let amount_in = if reach_target_price {
        amount_in_to_target
    } else {
        bonk_get_token_amount_b_from_liquidity(
            sqrt_price_current_x64.clone(),
            next_sqrt_price_x64.clone(),
            liquidity,
            true,
        )?
    };
    let amount_out = bonk_get_token_amount_a_from_liquidity(
        sqrt_price_current_x64.clone(),
        next_sqrt_price_x64.clone(),
        liquidity,
        false,
    )?;
    let fee_amount = if !reach_target_price {
        bonk_big_sub(amount_remaining, &amount_in, "CLMM swap fee amount")?
    } else {
        bonk_mul_div_ceil(
            &amount_in,
            &fee_rate_big,
            &(&fee_denominator - &fee_rate_big),
        )?
    };
    Ok((next_sqrt_price_x64, amount_in, amount_out, fee_amount))
}

fn bonk_quote_usd1_from_exact_sol_input(
    setup: &BonkUsd1RouteSetup,
    input_lamports: &BigUint,
    slippage_bps: u64,
) -> Result<BonkUsd1DirectQuote, String> {
    if input_lamports == &BigUint::ZERO {
        return Ok(BonkUsd1DirectQuote {
            expected_out: BigUint::ZERO,
            min_out: BigUint::ZERO,
            price_impact_pct: 0.0,
            traversed_tick_array_starts: vec![],
        });
    }
    let mut amount_remaining = input_lamports.clone();
    let mut amount_out_total = BigUint::ZERO;
    let mut sqrt_price_x64 = setup.sqrt_price_x64.clone();
    let mut liquidity = setup.liquidity.clone();
    let mut current_tick = setup.tick_current;
    let mut traversed_tick_array_starts = Vec::new();
    let min_sqrt_price = bonk_biguint_from_u128(BONK_CLMM_MIN_SQRT_PRICE_X64_PLUS_ONE);

    while amount_remaining > BigUint::ZERO && sqrt_price_x64 > min_sqrt_price {
        let current_array_start =
            bonk_get_tick_array_start_index_by_tick(current_tick, setup.tick_spacing);
        if traversed_tick_array_starts
            .last()
            .copied()
            .map(|value| value != current_array_start)
            .unwrap_or(true)
        {
            traversed_tick_array_starts.push(current_array_start);
        }
        let next_tick = bonk_find_next_initialized_tick_zero_for_one(setup, current_tick)?;
        let next_tick_sqrt_price = bonk_sqrt_price_from_tick(next_tick.tick)?;
        let target_sqrt_price = if next_tick_sqrt_price < min_sqrt_price {
            min_sqrt_price.clone()
        } else {
            next_tick_sqrt_price
        };
        let (step_next_sqrt_price, step_amount_in, step_amount_out, step_fee_amount) =
            bonk_clmm_swap_step_exact_in_zero_for_one(
                &sqrt_price_x64,
                &target_sqrt_price,
                &liquidity,
                &amount_remaining,
                setup.trade_fee_rate,
            )?;
        amount_remaining = bonk_big_sub(
            &amount_remaining,
            &(step_amount_in.clone() + &step_fee_amount),
            "CLMM remaining input",
        )?;
        amount_out_total += &step_amount_out;
        sqrt_price_x64 = step_next_sqrt_price;
        if sqrt_price_x64 == target_sqrt_price {
            liquidity = bonk_apply_liquidity_delta(&liquidity, next_tick.liquidity_net)?;
            current_tick = next_tick.tick.saturating_sub(1);
        }
    }

    let execution_price = bonk_sqrt_price_x64_to_price(
        &sqrt_price_x64,
        setup.mint_a_decimals,
        setup.mint_b_decimals,
    )?;
    let price_impact_pct = if !setup.current_price.is_finite() || setup.current_price <= 0.0 {
        0.0
    } else {
        ((execution_price - setup.current_price).abs() / setup.current_price) * 100.0
    };
    Ok(BonkUsd1DirectQuote {
        min_out: bonk_build_min_amount_from_bps(&amount_out_total, slippage_bps),
        expected_out: amount_out_total,
        price_impact_pct,
        traversed_tick_array_starts,
    })
}

fn bonk_quote_sol_from_exact_usd1_input(
    setup: &BonkUsd1RouteSetup,
    input_lamports: &BigUint,
    slippage_bps: u64,
) -> Result<BonkUsd1DirectQuote, String> {
    if input_lamports == &BigUint::ZERO {
        return Ok(BonkUsd1DirectQuote {
            expected_out: BigUint::ZERO,
            min_out: BigUint::ZERO,
            price_impact_pct: 0.0,
            traversed_tick_array_starts: vec![],
        });
    }
    let mut amount_remaining = input_lamports.clone();
    let mut amount_out_total = BigUint::ZERO;
    let mut sqrt_price_x64 = setup.sqrt_price_x64.clone();
    let mut liquidity = setup.liquidity.clone();
    let mut current_tick = setup.tick_current;
    let mut traversed_tick_array_starts = Vec::new();
    let max_sqrt_price = bonk_biguint_from_u128(BONK_CLMM_MAX_SQRT_PRICE_X64_MINUS_ONE);

    while amount_remaining > BigUint::ZERO && sqrt_price_x64 < max_sqrt_price {
        let current_array_start =
            bonk_get_tick_array_start_index_by_tick(current_tick, setup.tick_spacing);
        if traversed_tick_array_starts
            .last()
            .copied()
            .map(|value| value != current_array_start)
            .unwrap_or(true)
        {
            traversed_tick_array_starts.push(current_array_start);
        }
        let next_tick = bonk_find_next_initialized_tick_one_for_zero(setup, current_tick)?;
        let next_tick_sqrt_price = bonk_sqrt_price_from_tick(next_tick.tick)?;
        let target_sqrt_price = if next_tick_sqrt_price > max_sqrt_price {
            max_sqrt_price.clone()
        } else {
            next_tick_sqrt_price
        };
        let (step_next_sqrt_price, step_amount_in, step_amount_out, step_fee_amount) =
            bonk_clmm_swap_step_exact_in_one_for_zero(
                &sqrt_price_x64,
                &target_sqrt_price,
                &liquidity,
                &amount_remaining,
                setup.trade_fee_rate,
            )?;
        amount_remaining = bonk_big_sub(
            &amount_remaining,
            &(step_amount_in.clone() + &step_fee_amount),
            "CLMM remaining input",
        )?;
        amount_out_total += &step_amount_out;
        sqrt_price_x64 = step_next_sqrt_price;
        if sqrt_price_x64 == target_sqrt_price {
            liquidity = bonk_apply_liquidity_delta(&liquidity, next_tick.liquidity_net)?;
            current_tick = next_tick.tick;
        }
    }

    let execution_price = bonk_sqrt_price_x64_to_price(
        &sqrt_price_x64,
        setup.mint_a_decimals,
        setup.mint_b_decimals,
    )?;
    let price_impact_pct = if !setup.current_price.is_finite() || setup.current_price <= 0.0 {
        0.0
    } else {
        ((execution_price - setup.current_price).abs() / setup.current_price) * 100.0
    };
    Ok(BonkUsd1DirectQuote {
        min_out: bonk_build_min_amount_from_bps(&amount_out_total, slippage_bps),
        expected_out: amount_out_total,
        price_impact_pct,
        traversed_tick_array_starts,
    })
}

async fn native_quote_usd1_output_from_sol_input_with_metrics(
    rpc_url: &str,
    input_lamports: &BigUint,
    slippage_bps: u64,
    mut metrics: Option<&mut HelperUsd1QuoteMetrics>,
    route_setup_override: Option<&BonkUsd1RouteSetup>,
) -> Result<BonkUsd1DirectQuote, String> {
    let setup = if let Some(setup) = route_setup_override {
        setup.clone()
    } else {
        load_bonk_usd1_route_setup_with_metrics(rpc_url, metrics.as_deref_mut(), false).await?
    };
    let quote_started = std::time::Instant::now();
    let quote = bonk_quote_usd1_from_exact_sol_input(&setup, input_lamports, slippage_bps)?;
    if let Some(metrics) = metrics.as_deref_mut() {
        metrics.quoteCalls = metrics.quoteCalls.saturating_add(1);
        metrics.quoteTotalMs = metrics
            .quoteTotalMs
            .saturating_add(quote_started.elapsed().as_millis() as u64);
        metrics.averageQuoteMs = if metrics.quoteCalls == 0 {
            0.0
        } else {
            metrics.quoteTotalMs as f64 / metrics.quoteCalls as f64
        };
    }
    Ok(quote)
}

async fn native_quote_usd1_output_from_sol_input(
    rpc_url: &str,
    input_lamports: &BigUint,
    slippage_bps: u64,
) -> Result<BonkUsd1DirectQuote, String> {
    native_quote_usd1_output_from_sol_input_with_metrics(
        rpc_url,
        input_lamports,
        slippage_bps,
        None,
        None,
    )
    .await
}

async fn native_quote_usd1_buy_amounts_from_sol_input(
    rpc_url: &str,
    buy_amount_sol: &str,
    route_setup_override: Option<&BonkUsd1RouteSetup>,
) -> Result<BonkUsd1BuyAmountQuote, String> {
    let input_sol = parse_decimal_biguint(buy_amount_sol, 9, "follow buy amount SOL")?;
    if input_sol == BigUint::from(0u8) {
        return Err("Follow buy amount SOL must be greater than zero.".to_string());
    }
    let quote = native_quote_usd1_output_from_sol_input_with_metrics(
        rpc_url,
        &input_sol,
        BONK_USD1_ROUTE_SLIPPAGE_BPS,
        None,
        route_setup_override,
    )
    .await?;
    Ok(BonkUsd1BuyAmountQuote {
        expected_amount_b: biguint_to_u64(&quote.expected_out, "follow buy expected USD1 amount")?,
        guaranteed_amount_b: biguint_to_u64(&quote.min_out, "follow buy guaranteed USD1 amount")?,
    })
}

async fn native_quote_sol_input_for_usd1_output(
    rpc_url: &str,
    required_quote_amount: &BigUint,
    slippage_bps: u64,
) -> Result<BigUint, String> {
    native_quote_sol_input_for_usd1_output_with_max_and_metrics(
        rpc_url,
        required_quote_amount,
        slippage_bps,
        None,
        None,
        None,
    )
    .await
}

async fn native_quote_sol_input_for_usd1_output_with_max_and_metrics(
    rpc_url: &str,
    required_quote_amount: &BigUint,
    slippage_bps: u64,
    max_input_lamports_override: Option<BigUint>,
    mut metrics: Option<&mut HelperUsd1QuoteMetrics>,
    route_setup_override: Option<&BonkUsd1RouteSetup>,
) -> Result<BigUint, String> {
    let quote_started = std::time::Instant::now();
    let setup = if let Some(setup) = route_setup_override {
        setup.clone()
    } else {
        load_bonk_usd1_route_setup_with_metrics(rpc_url, metrics.as_deref_mut(), false).await?
    };
    if !setup.current_price.is_finite() || setup.current_price <= 0.0 {
        return Err(format!(
            "Pinned USD1 route pool has invalid price metadata: {BONK_PINNED_USD1_ROUTE_POOL_ID}"
        ));
    }
    let max_input_lamports = max_input_lamports_override
        .unwrap_or_else(|| bonk_biguint_from_u64(BONK_USD1_QUOTE_MAX_INPUT_LAMPORTS));
    let mut low = BigUint::from(1u8);
    let mut high = bonk_build_usd1_search_guess_lamports(
        required_quote_amount,
        setup.current_price,
        &max_input_lamports,
    )?;
    let mut quote = bonk_quote_usd1_from_exact_sol_input(&setup, &high, slippage_bps)?;
    if let Some(metrics) = metrics.as_deref_mut() {
        metrics.quoteCalls = metrics.quoteCalls.saturating_add(1);
        metrics.expansionQuoteCalls = metrics.expansionQuoteCalls.saturating_add(1);
    }
    while quote.min_out < *required_quote_amount && high < max_input_lamports {
        low = &high + BigUint::from(1u8);
        high = std::cmp::min(high * BigUint::from(2u8), max_input_lamports.clone());
        quote = bonk_quote_usd1_from_exact_sol_input(&setup, &high, slippage_bps)?;
        if let Some(metrics) = metrics.as_deref_mut() {
            metrics.quoteCalls = metrics.quoteCalls.saturating_add(1);
            metrics.expansionQuoteCalls = metrics.expansionQuoteCalls.saturating_add(1);
        }
        if high == max_input_lamports {
            break;
        }
    }
    if quote.min_out < *required_quote_amount {
        return Err(format!(
            "Pinned USD1 route pool could not satisfy required USD1 output: {BONK_PINNED_USD1_ROUTE_POOL_ID}."
        ));
    }
    let max_search_iterations = std::env::var("BONK_USD1_MAX_INPUT_SEARCH_ITERATIONS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(10);
    for _ in 0..max_search_iterations {
        if low >= high || (&high - &low) <= bonk_usd1_search_tolerance_lamports(&high) {
            break;
        }
        let mid = (&low + &high) / BigUint::from(2u8);
        let mid_quote = bonk_quote_usd1_from_exact_sol_input(&setup, &mid, slippage_bps)?;
        if let Some(metrics) = metrics.as_deref_mut() {
            metrics.quoteCalls = metrics.quoteCalls.saturating_add(1);
            metrics.binarySearchQuoteCalls = metrics.binarySearchQuoteCalls.saturating_add(1);
            metrics.searchIterations = metrics.searchIterations.saturating_add(1);
        }
        if mid_quote.min_out >= *required_quote_amount {
            high = mid;
            quote = mid_quote;
        } else {
            low = mid + BigUint::from(1u8);
        }
    }
    if quote.min_out < *required_quote_amount {
        return Err(format!(
            "Pinned USD1 route pool could not satisfy required USD1 output: {BONK_PINNED_USD1_ROUTE_POOL_ID}."
        ));
    }
    if let Some(metrics) = metrics.as_deref_mut() {
        metrics.quoteTotalMs = metrics
            .quoteTotalMs
            .saturating_add(quote_started.elapsed().as_millis() as u64);
        metrics.averageQuoteMs = if metrics.quoteCalls == 0 {
            0.0
        } else {
            metrics.quoteTotalMs as f64 / metrics.quoteCalls as f64
        };
    }
    Ok(high)
}

async fn native_quote_sol_input_for_usd1_output_with_max(
    rpc_url: &str,
    required_quote_amount: &BigUint,
    slippage_bps: u64,
    max_input_lamports_override: Option<BigUint>,
    route_setup_override: Option<&BonkUsd1RouteSetup>,
) -> Result<BigUint, String> {
    native_quote_sol_input_for_usd1_output_with_max_and_metrics(
        rpc_url,
        required_quote_amount,
        slippage_bps,
        max_input_lamports_override,
        None,
        route_setup_override,
    )
    .await
}

fn local_bonk_launch_default_params(config_id: &str) -> Option<RaydiumLaunchConfigDefaultParams> {
    let total_fund_raising_b = match config_id {
        BONK_MAINNET_SOL_LAUNCH_CONFIG_ID => BONK_DEFAULT_SOL_TOTAL_FUND_RAISING_B,
        BONK_MAINNET_USD1_LAUNCH_CONFIG_ID => BONK_DEFAULT_USD1_TOTAL_FUND_RAISING_B,
        _ => return None,
    };
    Some(RaydiumLaunchConfigDefaultParams {
        supply_init: BONK_DEFAULT_SUPPLY_INIT.to_string(),
        total_fund_raising_b: total_fund_raising_b.to_string(),
        total_sell_a: BONK_DEFAULT_TOTAL_SELL_A.to_string(),
    })
}

async fn load_bonk_launch_defaults(
    rpc_url: &str,
    launch_mode: &str,
    quote_asset: &str,
) -> Result<BonkLaunchDefaults, String> {
    if let Some(defaults) = cached_bonk_launch_defaults(launch_mode, quote_asset) {
        return Ok(defaults);
    }
    let normalized_mode = normalize_bonk_launch_mode(launch_mode);
    let quote = bonk_quote_asset_config(quote_asset);
    let cache_key = format!("{normalized_mode}:{}", quote.asset);
    let config_id = bonk_launch_config_id(quote.asset)?;
    let platform_id = bonk_platform_id(normalized_mode);
    let (config_data, platform_data) = tokio::try_join!(
        fetch_account_data(rpc_url, &config_id, "confirmed"),
        fetch_account_data(rpc_url, platform_id, "confirmed"),
    )?;
    let config_info = decode_bonk_launchpad_config(&config_data)?;
    let platform_info = decode_bonk_platform_config(&platform_data)?;
    let default_params = local_bonk_launch_default_params(&config_id)
        .ok_or_else(|| format!("Local Bonk launch config defaults not found for {config_id}"))?;
    let supply = parse_biguint_integer(&default_params.supply_init, "Bonk launch supply")?;
    let total_sell_a =
        parse_biguint_integer(&default_params.total_sell_a, "Bonk launch total sell")?;
    let total_fund_raising_b = parse_biguint_integer(
        &default_params.total_fund_raising_b,
        "Bonk launch total fund raising",
    )?;
    if config_info.curve_type != 0
        || config_info.migrate_fee != 0
        || config_info.trade_fee_rate != 2500
    {
        return Err(format!(
            "Unsupported Bonk launch config {config_id}; local defaults require the verified constant-product config."
        ));
    }
    let (virtual_a, virtual_b) = bonk_curve_init_virtuals(
        config_info.curve_type,
        &supply,
        &total_fund_raising_b,
        &total_sell_a,
        &BigUint::ZERO,
        &bonk_biguint_from_u64(config_info.migrate_fee),
    )?;
    let defaults = BonkLaunchDefaults {
        supply,
        total_fund_raising_b: total_fund_raising_b.clone(),
        quote: quote.clone(),
        trade_fee_rate: bonk_biguint_from_u64(config_info.trade_fee_rate),
        platform_fee_rate: bonk_biguint_from_u64(platform_info.fee_rate),
        creator_fee_rate: bonk_biguint_from_u64(platform_info.creator_fee_rate),
        curve_type: config_info.curve_type,
        pool: BonkCurvePoolState {
            total_sell_a,
            virtual_a,
            virtual_b,
            real_a: BigUint::ZERO,
            real_b: BigUint::ZERO,
        },
    };
    bonk_launch_defaults_cache()
        .lock()
        .expect("bonk launch defaults cache")
        .insert(
            cache_key,
            BonkLaunchDefaultsCacheEntry {
                fetched_at: std::time::Instant::now(),
                defaults: defaults.clone(),
            },
        );
    Ok(defaults)
}

async fn load_bonk_launch_defaults_with_startup_stagger(
    rpc_url: &str,
    launch_mode: &str,
    quote_asset: &str,
    index: u64,
) -> Result<BonkLaunchDefaults, String> {
    if let Some(defaults) = cached_bonk_launch_defaults(launch_mode, quote_asset) {
        return Ok(defaults);
    }
    if index > 0 {
        tokio::time::sleep(Duration::from_millis(
            BONK_STARTUP_WARM_DEFAULT_STAGGER_MS.saturating_mul(index),
        ))
        .await;
    }
    load_bonk_launch_defaults(rpc_url, launch_mode, quote_asset).await
}

fn build_native_bonk_quote_from_defaults(
    defaults: &BonkLaunchDefaults,
    mode: &str,
    amount: &str,
) -> Result<LaunchQuote, String> {
    let normalized_mode = mode.trim().to_ascii_lowercase();
    if normalized_mode == "tokens" {
        let token_amount = parse_decimal_biguint(amount, BONK_TOKEN_DECIMALS, "buy amount")?;
        let quote_amount = bonk_quote_buy_exact_out_amount_b(defaults, &token_amount)?;
        return Ok(LaunchQuote {
            mode: normalized_mode,
            input: amount.to_string(),
            estimatedTokens: format_biguint_decimal(&token_amount, BONK_TOKEN_DECIMALS, 6),
            estimatedSol: format_biguint_decimal(&quote_amount, defaults.quote.decimals, 6),
            estimatedQuoteAmount: format_biguint_decimal(&quote_amount, defaults.quote.decimals, 6),
            quoteAsset: defaults.quote.asset.to_string(),
            quoteAssetLabel: defaults.quote.label.to_string(),
            estimatedSupplyPercent: bonk_estimate_supply_percent(&token_amount, &defaults.supply),
        });
    }
    let buy_amount = parse_decimal_biguint(
        amount,
        defaults.quote.decimals,
        &format!("buy amount {}", defaults.quote.label),
    )?;
    let token_amount = bonk_quote_buy_exact_in_amount_a(defaults, &buy_amount)?;
    Ok(LaunchQuote {
        mode: normalized_mode,
        input: amount.to_string(),
        estimatedTokens: format_biguint_decimal(&token_amount, BONK_TOKEN_DECIMALS, 6),
        estimatedSol: format_biguint_decimal(&buy_amount, defaults.quote.decimals, 6),
        estimatedQuoteAmount: format_biguint_decimal(&buy_amount, defaults.quote.decimals, 6),
        quoteAsset: defaults.quote.asset.to_string(),
        quoteAssetLabel: defaults.quote.label.to_string(),
        estimatedSupplyPercent: bonk_estimate_supply_percent(&token_amount, &defaults.supply),
    })
}

async fn native_quote_launch(
    rpc_url: &str,
    quote_asset: &str,
    launch_mode: &str,
    mode: &str,
    amount: &str,
) -> Result<LaunchQuote, String> {
    let defaults = load_bonk_launch_defaults(rpc_url, launch_mode, quote_asset).await?;
    if defaults.quote.asset == "usd1" {
        let slippage_bps = 0u64;
        let normalized_mode = mode.trim().to_ascii_lowercase();
        if normalized_mode == "tokens" {
            let token_amount = parse_decimal_biguint(amount, BONK_TOKEN_DECIMALS, "buy amount")?;
            let required_quote_amount =
                bonk_quote_buy_exact_out_amount_b(&defaults, &token_amount)?;
            let quoted_sol_input = native_quote_sol_input_for_usd1_output(
                rpc_url,
                &required_quote_amount,
                slippage_bps,
            )
            .await?;
            return Ok(LaunchQuote {
                mode: normalized_mode,
                input: amount.to_string(),
                estimatedTokens: format_biguint_decimal(&token_amount, BONK_TOKEN_DECIMALS, 6),
                estimatedSol: format_biguint_decimal(&quoted_sol_input, 9, 6),
                estimatedQuoteAmount: format_biguint_decimal(&quoted_sol_input, 9, 6),
                quoteAsset: "sol".to_string(),
                quoteAssetLabel: "SOL".to_string(),
                estimatedSupplyPercent: bonk_estimate_supply_percent(
                    &token_amount,
                    &defaults.supply,
                ),
            });
        }
        let input_sol = parse_decimal_biguint(amount, 9, "buy amount SOL")?;
        let usd1_route_quote =
            native_quote_usd1_output_from_sol_input(rpc_url, &input_sol, slippage_bps).await?;
        let token_amount = bonk_quote_buy_exact_in_amount_a(&defaults, &usd1_route_quote.min_out)?;
        return Ok(LaunchQuote {
            mode: normalized_mode,
            input: amount.to_string(),
            estimatedTokens: format_biguint_decimal(&token_amount, BONK_TOKEN_DECIMALS, 6),
            estimatedSol: format_biguint_decimal(&input_sol, 9, 6),
            estimatedQuoteAmount: format_biguint_decimal(&input_sol, 9, 6),
            quoteAsset: "sol".to_string(),
            quoteAssetLabel: "SOL".to_string(),
            estimatedSupplyPercent: bonk_estimate_supply_percent(&token_amount, &defaults.supply),
        });
    }
    build_native_bonk_quote_from_defaults(&defaults, mode, amount)
}

async fn native_predict_dev_buy_effect(
    rpc_url: &str,
    config: &NormalizedConfig,
) -> Result<Option<BonkPredictedDevBuyEffect>, String> {
    let Some(dev_buy) = config.devBuy.as_ref() else {
        return Ok(None);
    };
    let dev_buy_mode = dev_buy.mode.trim().to_ascii_lowercase();
    if dev_buy_mode.is_empty() || dev_buy.amount.trim().is_empty() {
        return Ok(None);
    }
    let defaults = load_bonk_launch_defaults(rpc_url, &config.mode, &config.quoteAsset).await?;
    let requested_amount_b = if dev_buy_mode == "tokens" {
        let requested_tokens =
            parse_decimal_biguint(&dev_buy.amount, BONK_TOKEN_DECIMALS, "dev buy tokens")?;
        bonk_quote_buy_exact_out_amount_b(&defaults, &requested_tokens)?
    } else if defaults.quote.asset == "usd1" {
        let input_sol = parse_decimal_biguint(&dev_buy.amount, 9, "dev buy SOL")?;
        native_quote_usd1_output_from_sol_input(rpc_url, &input_sol, BONK_USD1_ROUTE_SLIPPAGE_BPS)
            .await?
            .min_out
    } else {
        parse_decimal_biguint(
            &dev_buy.amount,
            defaults.quote.decimals,
            &format!("dev buy {}", defaults.quote.label),
        )?
    };
    let mint = Pubkey::new_unique();
    let creator = Pubkey::new_unique();
    let pool_context = build_prelaunch_bonk_pool_context(&defaults, &mint, &creator, &config.mode)?;
    let details = bonk_follow_buy_quote_details(
        &pool_context,
        biguint_to_u64(&requested_amount_b, "predicted dev buy quote amount")?,
        slippage_bps_from_percent(&config.execution.buySlippagePercent)?,
    )?;
    Ok(Some(BonkPredictedDevBuyEffect {
        requested_quote_amount_b: details.gross_input_b,
        token_amount: details.amount_a,
    }))
}

async fn native_predict_dev_buy_token_amount(
    rpc_url: &str,
    config: &NormalizedConfig,
) -> Result<Option<u64>, String> {
    Ok(native_predict_dev_buy_effect(rpc_url, config)
        .await?
        .map(|effect| effect.token_amount))
}

fn read_bonk_u8(data: &[u8], offset: &mut usize) -> Result<u8, String> {
    let value = data
        .get(*offset)
        .copied()
        .ok_or_else(|| "Bonk launchpad account was too short.".to_string())?;
    *offset += 1;
    Ok(value)
}

fn read_bonk_u64(data: &[u8], offset: &mut usize) -> Result<u64, String> {
    let bytes = data
        .get(*offset..(*offset + 8))
        .ok_or_else(|| "Bonk launchpad account was too short.".to_string())?;
    *offset += 8;
    let array: [u8; 8] = bytes
        .try_into()
        .map_err(|_| "Bonk launchpad account returned an invalid u64 field.".to_string())?;
    Ok(u64::from_le_bytes(array))
}

fn read_bonk_u16(data: &[u8], offset: &mut usize) -> Result<u16, String> {
    let bytes = data
        .get(*offset..(*offset + 2))
        .ok_or_else(|| "Bonk account was too short.".to_string())?;
    *offset += 2;
    let array: [u8; 2] = bytes
        .try_into()
        .map_err(|_| "Bonk account returned an invalid u16 field.".to_string())?;
    Ok(u16::from_le_bytes(array))
}

fn read_bonk_bool(data: &[u8], offset: &mut usize) -> Result<bool, String> {
    Ok(read_bonk_u8(data, offset)? != 0)
}

fn read_bonk_u32(data: &[u8], offset: &mut usize) -> Result<u32, String> {
    let bytes = data
        .get(*offset..(*offset + 4))
        .ok_or_else(|| "Bonk account was too short.".to_string())?;
    *offset += 4;
    let array: [u8; 4] = bytes
        .try_into()
        .map_err(|_| "Bonk account returned an invalid u32 field.".to_string())?;
    Ok(u32::from_le_bytes(array))
}

fn read_bonk_i32(data: &[u8], offset: &mut usize) -> Result<i32, String> {
    let bytes = data
        .get(*offset..(*offset + 4))
        .ok_or_else(|| "Bonk account was too short.".to_string())?;
    *offset += 4;
    let array: [u8; 4] = bytes
        .try_into()
        .map_err(|_| "Bonk account returned an invalid i32 field.".to_string())?;
    Ok(i32::from_le_bytes(array))
}

fn read_bonk_u128(data: &[u8], offset: &mut usize) -> Result<BigUint, String> {
    let bytes = data
        .get(*offset..(*offset + 16))
        .ok_or_else(|| "Bonk account was too short.".to_string())?;
    *offset += 16;
    Ok(BigUint::from_bytes_le(bytes))
}

fn read_bonk_i128(data: &[u8], offset: &mut usize) -> Result<i128, String> {
    let bytes = data
        .get(*offset..(*offset + 16))
        .ok_or_else(|| "Bonk account was too short.".to_string())?;
    *offset += 16;
    let array: [u8; 16] = bytes
        .try_into()
        .map_err(|_| "Bonk account returned an invalid i128 field.".to_string())?;
    Ok(i128::from_le_bytes(array))
}

fn read_bonk_pubkey(data: &[u8], offset: &mut usize) -> Result<Pubkey, String> {
    let bytes = data
        .get(*offset..(*offset + 32))
        .ok_or_else(|| "Bonk launchpad account was too short.".to_string())?;
    *offset += 32;
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "Bonk launchpad account returned an invalid pubkey field.".to_string())?;
    Ok(Pubkey::new_from_array(array))
}

fn decode_bonk_clmm_config(data: &[u8]) -> Result<DecodedBonkClmmConfig, String> {
    let mut offset = 0usize;
    offset += 8;
    let _bump = read_bonk_u8(data, &mut offset)?;
    let _index = read_bonk_u16(data, &mut offset)?;
    let _fund_owner = read_bonk_pubkey(data, &mut offset)?;
    let _protocol_fee_rate = read_bonk_u32(data, &mut offset)?;
    let trade_fee_rate = read_bonk_u32(data, &mut offset)?;
    let tick_spacing = read_bonk_u16(data, &mut offset)?;
    Ok(DecodedBonkClmmConfig {
        trade_fee_rate,
        tick_spacing,
    })
}

fn decode_bonk_clmm_pool(data: &[u8]) -> Result<DecodedBonkClmmPool, String> {
    let mut offset = 0usize;
    offset += 8;
    let _bump = read_bonk_u8(data, &mut offset)?;
    let amm_config = read_bonk_pubkey(data, &mut offset)?;
    let _creator = read_bonk_pubkey(data, &mut offset)?;
    let mint_a = read_bonk_pubkey(data, &mut offset)?;
    let mint_b = read_bonk_pubkey(data, &mut offset)?;
    let vault_a = read_bonk_pubkey(data, &mut offset)?;
    let vault_b = read_bonk_pubkey(data, &mut offset)?;
    let observation_id = read_bonk_pubkey(data, &mut offset)?;
    let mint_decimals_a = read_bonk_u8(data, &mut offset)?;
    let mint_decimals_b = read_bonk_u8(data, &mut offset)?;
    let tick_spacing = read_bonk_u16(data, &mut offset)?;
    let liquidity = read_bonk_u128(data, &mut offset)?;
    let sqrt_price_x64 = read_bonk_u128(data, &mut offset)?;
    let tick_current = read_bonk_i32(data, &mut offset)?;
    let _padding = read_bonk_u32(data, &mut offset)?;
    offset += 16 + 16;
    offset += 8 + 8;
    offset += 16 + 16 + 16 + 16;
    let _status = read_bonk_u8(data, &mut offset)?;
    offset += 7;
    offset += 3 * 169;
    let mut tick_array_bitmap = [0u64; 16];
    for word in &mut tick_array_bitmap {
        *word = read_bonk_u64(data, &mut offset)?;
    }
    Ok(DecodedBonkClmmPool {
        amm_config,
        mint_a,
        mint_b,
        vault_a,
        vault_b,
        observation_id,
        mint_decimals_a,
        mint_decimals_b,
        tick_spacing,
        liquidity,
        sqrt_price_x64,
        tick_current,
        tick_array_bitmap,
    })
}

fn decode_bonk_cpmm_pool(data: &[u8]) -> Result<DecodedBonkCpmmPool, String> {
    let mut offset = 0usize;
    offset += 8;
    let config_id = read_bonk_pubkey(data, &mut offset)?;
    let _pool_creator = read_bonk_pubkey(data, &mut offset)?;
    let vault_a = read_bonk_pubkey(data, &mut offset)?;
    let vault_b = read_bonk_pubkey(data, &mut offset)?;
    let _lp_mint = read_bonk_pubkey(data, &mut offset)?;
    let token_0_mint = read_bonk_pubkey(data, &mut offset)?;
    let token_1_mint = read_bonk_pubkey(data, &mut offset)?;
    let token_0_program = read_bonk_pubkey(data, &mut offset)?;
    let token_1_program = read_bonk_pubkey(data, &mut offset)?;
    let observation_id = read_bonk_pubkey(data, &mut offset)?;
    let _bump = read_bonk_u8(data, &mut offset)?;
    let _status = read_bonk_u8(data, &mut offset)?;
    let _lp_decimals = read_bonk_u8(data, &mut offset)?;
    let mint_decimals_a = read_bonk_u8(data, &mut offset)?;
    let mint_decimals_b = read_bonk_u8(data, &mut offset)?;
    let _lp_amount = read_bonk_u64(data, &mut offset)?;
    let protocol_fees_mint_a = read_bonk_u64(data, &mut offset)?;
    let protocol_fees_mint_b = read_bonk_u64(data, &mut offset)?;
    let fund_fees_mint_a = read_bonk_u64(data, &mut offset)?;
    let fund_fees_mint_b = read_bonk_u64(data, &mut offset)?;
    let _open_time = read_bonk_u64(data, &mut offset)?;
    let _epoch = read_bonk_u64(data, &mut offset)?;
    let _fee_on = read_bonk_u8(data, &mut offset)?;
    let enable_creator_fee = read_bonk_bool(data, &mut offset)?;
    offset += 6;
    let creator_fees_mint_a = read_bonk_u64(data, &mut offset)?;
    let creator_fees_mint_b = read_bonk_u64(data, &mut offset)?;
    Ok(DecodedBonkCpmmPool {
        config_id,
        vault_a,
        vault_b,
        token_0_mint,
        token_1_mint,
        token_0_program,
        token_1_program,
        observation_id,
        mint_decimals_a,
        mint_decimals_b,
        protocol_fees_mint_a,
        protocol_fees_mint_b,
        fund_fees_mint_a,
        fund_fees_mint_b,
        enable_creator_fee,
        creator_fees_mint_a,
        creator_fees_mint_b,
    })
}

fn decode_bonk_cpmm_config(data: &[u8]) -> Result<DecodedBonkCpmmConfig, String> {
    let mut offset = 0usize;
    offset += 8;
    let _bump = read_bonk_u8(data, &mut offset)?;
    let _disable_create_pool = read_bonk_bool(data, &mut offset)?;
    let _index = read_bonk_u16(data, &mut offset)?;
    let trade_fee_rate = read_bonk_u64(data, &mut offset)?;
    let _protocol_fee_rate = read_bonk_u64(data, &mut offset)?;
    let _fund_fee_rate = read_bonk_u64(data, &mut offset)?;
    let _create_pool_fee = read_bonk_u64(data, &mut offset)?;
    let _protocol_owner = read_bonk_pubkey(data, &mut offset)?;
    let _fund_owner = read_bonk_pubkey(data, &mut offset)?;
    let creator_fee_rate = read_bonk_u64(data, &mut offset)?;
    Ok(DecodedBonkCpmmConfig {
        trade_fee_rate,
        creator_fee_rate,
    })
}

fn decode_bonk_clmm_tick_array(data: &[u8]) -> Result<BonkClmmTickArray, String> {
    let mut offset = 0usize;
    offset += 8;
    let _pool_id = read_bonk_pubkey(data, &mut offset)?;
    let start_tick_index = read_bonk_i32(data, &mut offset)?;
    let mut ticks = Vec::with_capacity(usize::try_from(BONK_CLMM_TICK_ARRAY_SIZE).unwrap_or(60));
    for _ in 0..BONK_CLMM_TICK_ARRAY_SIZE {
        let tick = read_bonk_i32(data, &mut offset)?;
        let liquidity_net = read_bonk_i128(data, &mut offset)?;
        let liquidity_gross = read_bonk_u128(data, &mut offset)?;
        offset += 16 + 16 + (3 * 16) + (13 * 4);
        ticks.push(BonkClmmTick {
            tick,
            liquidity_net,
            liquidity_gross,
        });
    }
    let _initialized_tick_count = read_bonk_u8(data, &mut offset)?;
    Ok(BonkClmmTickArray {
        start_tick_index,
        ticks,
    })
}

fn decode_bonk_launchpad_pool(data: &[u8]) -> Result<DecodedBonkLaunchpadPool, String> {
    let mut offset = 0usize;
    let _discriminator = read_bonk_u64(data, &mut offset)?;
    let _epoch = read_bonk_u64(data, &mut offset)?;
    let _bump = read_bonk_u8(data, &mut offset)?;
    let status = read_bonk_u8(data, &mut offset)?;
    let _mint_decimals_a = read_bonk_u8(data, &mut offset)?;
    let _mint_decimals_b = read_bonk_u8(data, &mut offset)?;
    let _migrate_type = read_bonk_u8(data, &mut offset)?;
    let supply = read_bonk_u64(data, &mut offset)?;
    let total_sell_a = read_bonk_u64(data, &mut offset)?;
    let virtual_a = read_bonk_u64(data, &mut offset)?;
    let virtual_b = read_bonk_u64(data, &mut offset)?;
    let real_a = read_bonk_u64(data, &mut offset)?;
    let real_b = read_bonk_u64(data, &mut offset)?;
    let _total_fund_raising_b = read_bonk_u64(data, &mut offset)?;
    let _protocol_fee = read_bonk_u64(data, &mut offset)?;
    let _platform_fee = read_bonk_u64(data, &mut offset)?;
    let _migrate_fee = read_bonk_u64(data, &mut offset)?;
    for _ in 0..5 {
        let _ = read_bonk_u64(data, &mut offset)?;
    }
    let config_id = read_bonk_pubkey(data, &mut offset)?;
    let platform_id = read_bonk_pubkey(data, &mut offset)?;
    let mint_a = read_bonk_pubkey(data, &mut offset)?;
    let _mint_b = read_bonk_pubkey(data, &mut offset)?;
    let _vault_a = read_bonk_pubkey(data, &mut offset)?;
    let _vault_b = read_bonk_pubkey(data, &mut offset)?;
    let creator = read_bonk_pubkey(data, &mut offset)?;
    Ok(DecodedBonkLaunchpadPool {
        creator,
        status,
        supply,
        config_id,
        total_sell_a,
        virtual_a,
        virtual_b,
        real_a,
        real_b,
        platform_id,
        mint_a,
    })
}

async fn fetch_launchpad_pool_candidate(
    rpc_url: &str,
    mint: &Pubkey,
    asset: &str,
) -> Result<Option<BonkMarketCandidate>, String> {
    let quote = bonk_quote_asset_config(asset);
    let pool_id = derive_canonical_pool_id(quote.asset, &mint.to_string()).await?;
    let account_data = match fetch_account_data(rpc_url, &pool_id, "processed").await {
        Ok(data) => data,
        Err(error) if error.contains("was not found.") => return Ok(None),
        Err(error) => return Err(error),
    };
    let pool = decode_bonk_launchpad_pool(&account_data)?;
    Ok(Some(BonkMarketCandidate {
        mode: if pool.platform_id.to_string() == BONK_BONKERS_PLATFORM_ID {
            "bonkers".to_string()
        } else {
            "regular".to_string()
        },
        quote_asset: quote.asset.to_string(),
        quote_asset_label: quote.label.to_string(),
        creator: pool.creator.to_string(),
        platform_id: pool.platform_id.to_string(),
        config_id: pool.config_id.to_string(),
        pool_id,
        real_quote_reserves: pool.real_b,
        complete: pool.status != 0,
        detection_source: "raydium-launchpad".to_string(),
        launch_migrate_pool: false,
        tvl: 0.0,
        pool_type: "LaunchLab".to_string(),
        launchpad_pool: Some(pool),
        raydium_pool: None,
    }))
}

async fn fetch_migrated_raydium_candidates(
    rpc_url: &str,
    mint: &Pubkey,
    preferred_quote_asset: &str,
    launchpad_candidates: &[BonkMarketCandidate],
) -> Result<Vec<BonkMarketCandidate>, String> {
    let mut candidates = Vec::new();
    let requested_quote = preferred_quote_asset.trim().to_ascii_lowercase();
    let assets = if requested_quote.is_empty() {
        vec!["sol", "usd1"]
    } else {
        vec![bonk_quote_asset_config(&requested_quote).asset]
    };
    for asset in assets {
        let quote = bonk_quote_asset_config(asset);
        let Some(launchpad_candidate) = launchpad_candidates
            .iter()
            .find(|candidate| candidate.quote_asset == quote.asset && candidate.complete)
        else {
            continue;
        };
        let quote_mint = bonk_quote_mint(quote.asset)?;
        for (pool_id, owner, data) in
            rpc_fetch_bonk_raydium_pools_for_pair(rpc_url, mint, &quote_mint, "processed").await?
        {
            let (config_id, pool_type) = if owner == bonk_clmm_program_id()? {
                let pool = decode_bonk_clmm_pool(&data)?;
                if pool.mint_a != *mint && pool.mint_b != *mint {
                    continue;
                }
                if pool.mint_a != quote_mint && pool.mint_b != quote_mint {
                    continue;
                }
                if pool.vault_a == Pubkey::default()
                    || pool.vault_b == Pubkey::default()
                    || pool.liquidity == BigUint::ZERO
                {
                    continue;
                }
                (pool.amm_config.to_string(), "Concentrated")
            } else if owner == bonk_cpmm_program_id()? {
                let pool = decode_bonk_cpmm_pool(&data)?;
                if pool.token_0_mint != *mint && pool.token_1_mint != *mint {
                    continue;
                }
                if pool.token_0_mint != quote_mint && pool.token_1_mint != quote_mint {
                    continue;
                }
                if pool.vault_a == Pubkey::default() || pool.vault_b == Pubkey::default() {
                    continue;
                }
                (pool.config_id.to_string(), "Standard")
            } else {
                continue;
            };
            candidates.push(BonkMarketCandidate {
                mode: launchpad_candidate.mode.clone(),
                quote_asset: quote.asset.to_string(),
                quote_asset_label: quote.label.to_string(),
                creator: launchpad_candidate.creator.clone(),
                platform_id: launchpad_candidate.platform_id.clone(),
                config_id,
                pool_id: pool_id.to_string(),
                real_quote_reserves: 0,
                complete: true,
                detection_source: "raydium-migrated-rpc".to_string(),
                launch_migrate_pool: true,
                tvl: 0.0,
                pool_type: pool_type.to_string(),
                launchpad_pool: None,
                raydium_pool: None,
            });
        }
    }
    Ok(candidates)
}

async fn rpc_fetch_bonk_raydium_pools_for_pair(
    rpc_url: &str,
    mint: &Pubkey,
    quote_mint: &Pubkey,
    commitment: &str,
) -> Result<Vec<(Pubkey, Pubkey, Vec<u8>)>, String> {
    let mut pools = HashMap::new();
    for (program_id, left_offset, right_offset) in [
        (
            BONK_CLMM_PROGRAM_ID,
            BONK_CLMM_MINT_A_OFFSET,
            BONK_CLMM_MINT_B_OFFSET,
        ),
        (
            BONK_CPMM_PROGRAM_ID,
            BONK_CPMM_TOKEN_0_MINT_OFFSET,
            BONK_CPMM_TOKEN_1_MINT_OFFSET,
        ),
    ] {
        for (left, right) in [(mint, quote_mint), (quote_mint, mint)] {
            for (pool_id, data) in rpc_fetch_bonk_raydium_pools_for_ordered_pair(
                rpc_url,
                program_id,
                left_offset,
                right_offset,
                left,
                right,
                commitment,
            )
            .await?
            {
                pools.entry(pool_id).or_insert((
                    Pubkey::from_str(program_id)
                        .map_err(|error| format!("Invalid Bonk Raydium program id: {error}"))?,
                    data,
                ));
            }
        }
    }
    Ok(pools
        .into_iter()
        .map(|(pool_id, (owner, data))| (pool_id, owner, data))
        .collect())
}

async fn rpc_fetch_bonk_raydium_pools_for_ordered_pair(
    rpc_url: &str,
    program_id: &str,
    left_offset: usize,
    right_offset: usize,
    left_mint: &Pubkey,
    right_mint: &Pubkey,
    commitment: &str,
) -> Result<Vec<(Pubkey, Vec<u8>)>, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": "launchdeck-bonk-raydium-pools",
        "method": "getProgramAccounts",
        "params": [
            program_id,
            {
                "commitment": commitment,
                "encoding": "base64",
                "filters": [
                    {
                        "memcmp": {
                            "offset": left_offset,
                            "bytes": left_mint.to_string()
                        }
                    },
                    {
                        "memcmp": {
                            "offset": right_offset,
                            "bytes": right_mint.to_string()
                        }
                    }
                ]
            }
        ]
    });
    let response = bonk_http_client()
        .post(rpc_url)
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("Failed to fetch Bonk Raydium pools from RPC: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch Bonk Raydium pools from RPC: status {}.",
            response.status()
        ));
    }
    let parsed: RpcResponse<Vec<RpcProgramAccount>> = response
        .json()
        .await
        .map_err(|error| format!("Failed to parse Bonk Raydium pool RPC response: {error}"))?;
    let mut pools = Vec::with_capacity(parsed.result.len());
    for account in parsed.result {
        let Ok(pool_id) = Pubkey::from_str(&account.pubkey) else {
            continue;
        };
        let Ok(data) = BASE64.decode(account.account.data.0.trim()) else {
            continue;
        };
        pools.push((pool_id, data));
    }
    Ok(pools)
}

async fn detect_bonk_market_candidates(
    rpc_url: &str,
    mint: &Pubkey,
    preferred_quote_asset: &str,
) -> Result<Vec<BonkMarketCandidate>, String> {
    let mut launchpad_candidates = Vec::new();
    let requested_quote = preferred_quote_asset.trim().to_ascii_lowercase();
    let assets = if requested_quote.is_empty() {
        vec!["sol", "usd1"]
    } else {
        vec![bonk_quote_asset_config(&requested_quote).asset]
    };
    for asset in assets {
        if let Some(candidate) = fetch_launchpad_pool_candidate(rpc_url, mint, asset).await? {
            launchpad_candidates.push(candidate);
        }
    }
    if !launchpad_candidates
        .iter()
        .any(|candidate| candidate.complete)
    {
        return Ok(launchpad_candidates);
    }
    let migrated_candidates = fetch_migrated_raydium_candidates(
        rpc_url,
        mint,
        preferred_quote_asset,
        &launchpad_candidates,
    )
    .await?;
    if migrated_candidates.is_empty() {
        Ok(launchpad_candidates)
    } else {
        Ok(migrated_candidates)
    }
}

fn compare_bonk_market_candidates(
    left: &BonkMarketCandidate,
    right: &BonkMarketCandidate,
    preferred_quote_asset: &str,
) -> Ordering {
    let left_canonical = if left.launch_migrate_pool { 1 } else { 0 };
    let right_canonical = if right.launch_migrate_pool { 1 } else { 0 };
    right_canonical
        .cmp(&left_canonical)
        .then_with(|| {
            let left_liquidity = if left.tvl > 0.0 {
                left.tvl
            } else {
                left.real_quote_reserves as f64
            };
            let right_liquidity = if right.tvl > 0.0 {
                right.tvl
            } else {
                right.real_quote_reserves as f64
            };
            right_liquidity
                .partial_cmp(&left_liquidity)
                .unwrap_or(Ordering::Equal)
        })
        .then_with(|| {
            let left_requested = (!preferred_quote_asset.is_empty()
                && left.quote_asset == preferred_quote_asset)
                as u8;
            let right_requested = (!preferred_quote_asset.is_empty()
                && right.quote_asset == preferred_quote_asset)
                as u8;
            right_requested.cmp(&left_requested)
        })
        .then_with(|| {
            pool_type_priority(&left.pool_type).cmp(&pool_type_priority(&right.pool_type))
        })
        .then_with(|| {
            if left.quote_asset == right.quote_asset {
                Ordering::Equal
            } else if left.quote_asset == "sol" {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        })
}

fn select_preferred_bonk_market_candidate<'a>(
    candidates: &'a [BonkMarketCandidate],
    preferred_quote_asset: &str,
) -> Option<&'a BonkMarketCandidate> {
    let normalized_preferred = preferred_quote_asset.trim().to_ascii_lowercase();
    let eligible = if normalized_preferred.is_empty() {
        candidates.iter().collect::<Vec<_>>()
    } else {
        candidates
            .iter()
            .filter(|candidate| {
                candidate
                    .quote_asset
                    .eq_ignore_ascii_case(&normalized_preferred)
            })
            .collect::<Vec<_>>()
    };
    eligible.into_iter().min_by(|left, right| {
        compare_bonk_market_candidates(left, right, normalized_preferred.as_str())
    })
}

async fn fetch_token_supply_value(
    rpc_url: &str,
    mint: &Pubkey,
    commitment: &str,
) -> Result<RpcTokenSupplyValue, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": "launchdeck-bonk-token-supply",
        "method": "getTokenSupply",
        "params": [
            mint.to_string(),
            {
                "commitment": commitment,
            }
        ]
    });
    let response = bonk_http_client()
        .post(rpc_url)
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("Failed to fetch Bonk token supply: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch Bonk token supply: RPC returned status {}.",
            response.status()
        ));
    }
    let parsed: RpcResponse<RpcTokenSupplyResult> = response
        .json()
        .await
        .map_err(|error| format!("Failed to parse Bonk token supply response: {error}"))?;
    Ok(parsed.result.value)
}

fn format_decimal_u128(value: u128, decimals: u32, max_fraction_digits: u32) -> String {
    let base = 10u128.pow(decimals);
    let whole = value / base;
    let fraction = value % base;
    if fraction == 0 {
        return whole.to_string();
    }
    let width = decimals as usize;
    let mut fraction_text = format!("{fraction:0width$}");
    fraction_text.truncate(max_fraction_digits.min(decimals) as usize);
    while fraction_text.ends_with('0') {
        fraction_text.pop();
    }
    if fraction_text.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{fraction_text}")
    }
}

fn build_launchpad_market_snapshot(
    candidate: &BonkMarketCandidate,
) -> Result<BonkMarketSnapshot, String> {
    let pool = candidate
        .launchpad_pool
        .as_ref()
        .ok_or_else(|| "Missing Bonk launchpad pool candidate.".to_string())?;
    let market_cap_lamports = if pool.virtual_a == 0 {
        0
    } else {
        (u128::from(pool.supply) * u128::from(pool.virtual_b)) / u128::from(pool.virtual_a)
    };
    Ok(BonkMarketSnapshot {
        mint: pool.mint_a.to_string(),
        creator: candidate.creator.clone(),
        virtualTokenReserves: pool.virtual_a.to_string(),
        virtualSolReserves: pool.virtual_b.to_string(),
        realTokenReserves: pool.total_sell_a.saturating_sub(pool.real_a).to_string(),
        realSolReserves: pool.real_b.to_string(),
        tokenTotalSupply: pool.supply.to_string(),
        complete: candidate.complete,
        marketCapLamports: market_cap_lamports.to_string(),
        marketCapSol: format_decimal_u128(
            market_cap_lamports,
            bonk_quote_asset_config(&candidate.quote_asset).decimals,
            6,
        ),
        quoteAsset: candidate.quote_asset.clone(),
        quoteAssetLabel: candidate.quote_asset_label.clone(),
    })
}

fn market_cap_from_raydium_pool_price(
    pool: &RaydiumPoolInfo,
    token_supply: u128,
    token_decimals: u32,
    quote: &BonkQuoteAssetConfig,
) -> Result<u128, String> {
    let price = pool.price;
    if !price.is_finite() || price <= 0.0 {
        return Err(format!(
            "Invalid Raydium migrated pool price for {}: {}",
            pool.id, pool.price
        ));
    }
    let scale = 10f64.powi(18);
    let scaled_price = (price * scale).round();
    if !scaled_price.is_finite() || scaled_price <= 0.0 {
        return Err(format!(
            "Invalid Raydium migrated pool price for {}: {}",
            pool.id, pool.price
        ));
    }
    let scaled_price = scaled_price as u128;
    let token_supply_big = bonk_biguint_from_u128(token_supply);
    let scaled_price_big = bonk_biguint_from_u128(scaled_price);
    let token_scale_big = bonk_pow10_biguint(token_decimals);
    let quote_scale_big = bonk_pow10_biguint(quote.decimals);
    let price_scale_big = bonk_pow10_biguint(18);
    if pool.mint_a.address == quote.mint {
        let market_cap = (((&token_supply_big * &price_scale_big) * &quote_scale_big)
            / &scaled_price_big)
            / &token_scale_big;
        return biguint_to_u128(&market_cap, &format!("migrated market cap for {}", pool.id));
    }
    if pool.mint_b.address == quote.mint {
        let market_cap = (((&token_supply_big * &scaled_price_big) * &quote_scale_big)
            / &price_scale_big)
            / &token_scale_big;
        return biguint_to_u128(&market_cap, &format!("migrated market cap for {}", pool.id));
    }
    Err(format!(
        "Migrated Raydium pool {} does not match requested quote asset {}.",
        pool.id, quote.asset
    ))
}

async fn build_migrated_raydium_market_snapshot(
    rpc_url: &str,
    mint: &Pubkey,
    candidate: &BonkMarketCandidate,
) -> Result<BonkMarketSnapshot, String> {
    let supply = fetch_token_supply_value(rpc_url, mint, "processed").await?;
    let token_supply = supply
        .amount
        .trim()
        .parse::<u128>()
        .map_err(|error| format!("Invalid Bonk token supply amount for {}: {error}", mint))?;
    let quote = bonk_quote_asset_config(&candidate.quote_asset);
    let venue_context = load_bonk_trade_venue_context_by_pool_id(
        rpc_url,
        &candidate.pool_id,
        quote.asset,
        "processed",
    )
    .await?;
    let market_cap_lamports = match venue_context {
        NativeBonkTradeVenueContext::RaydiumCpmm(context) => {
            let (token_reserve, quote_reserve) = if context.pool.token_0_mint == *mint {
                (context.reserve_a, context.reserve_b)
            } else {
                (context.reserve_b, context.reserve_a)
            };
            if token_reserve == 0 {
                0
            } else {
                let market_cap = (BigUint::from(token_supply) * BigUint::from(quote_reserve))
                    / BigUint::from(token_reserve);
                biguint_to_u128(&market_cap, "Bonk CPMM migrated market cap")?
            }
        }
        NativeBonkTradeVenueContext::RaydiumClmm(context) => {
            let setup = &context.setup;
            let token_is_a = setup.mint_a == *mint;
            let price_quote_per_token = if token_is_a {
                setup.current_price
            } else if setup.current_price > 0.0 {
                1.0 / setup.current_price
            } else {
                0.0
            };
            let token_amount = token_supply as f64 / 10f64.powi(supply.decimals as i32);
            let quote_atoms =
                token_amount * price_quote_per_token * 10f64.powi(quote.decimals as i32);
            if !quote_atoms.is_finite() || quote_atoms <= 0.0 {
                0
            } else {
                quote_atoms.round() as u128
            }
        }
        NativeBonkTradeVenueContext::Launchpad(_) => {
            return Err("Bonk migrated Raydium snapshot resolved to a launchpad pool.".to_string());
        }
    };
    Ok(BonkMarketSnapshot {
        mint: mint.to_string(),
        creator: candidate.creator.clone(),
        virtualTokenReserves: "0".to_string(),
        virtualSolReserves: "0".to_string(),
        realTokenReserves: "0".to_string(),
        realSolReserves: "0".to_string(),
        tokenTotalSupply: token_supply.to_string(),
        complete: true,
        marketCapLamports: market_cap_lamports.to_string(),
        marketCapSol: format_decimal_u128(market_cap_lamports, quote.decimals, 6),
        quoteAsset: candidate.quote_asset.clone(),
        quoteAssetLabel: candidate.quote_asset_label.clone(),
    })
}

async fn native_fetch_bonk_market_snapshot(
    rpc_url: &str,
    mint: &str,
    quote_asset: &str,
) -> Result<BonkMarketSnapshot, String> {
    let mint_pubkey =
        Pubkey::from_str(mint).map_err(|error| format!("Invalid Bonk mint address: {error}"))?;
    let candidates = detect_bonk_market_candidates(rpc_url, &mint_pubkey, quote_asset).await?;
    let preferred = select_preferred_bonk_market_candidate(&candidates, quote_asset)
        .ok_or_else(|| format!("No Bonk market candidate found for {mint}."))?;
    if is_raydium_detection_source(&preferred.detection_source) {
        build_migrated_raydium_market_snapshot(rpc_url, &mint_pubkey, preferred).await
    } else {
        build_launchpad_market_snapshot(preferred)
    }
}

async fn native_detect_bonk_import_context_with_quote_asset(
    rpc_url: &str,
    mint: &str,
    quote_asset: &str,
) -> Result<Option<BonkImportContext>, String> {
    let mint_pubkey =
        Pubkey::from_str(mint).map_err(|error| format!("Invalid Bonk mint address: {error}"))?;
    let candidates = detect_bonk_market_candidates(rpc_url, &mint_pubkey, quote_asset).await?;
    let Some(preferred) = select_preferred_bonk_market_candidate(&candidates, quote_asset) else {
        return Ok(None);
    };
    Ok(Some(BonkImportContext {
        launchpad: "bonk".to_string(),
        mode: preferred.mode.clone(),
        quoteAsset: preferred.quote_asset.clone(),
        creator: preferred.creator.clone(),
        platformId: preferred.platform_id.clone(),
        configId: preferred.config_id.clone(),
        poolId: preferred.pool_id.clone(),
        detectionSource: preferred.detection_source.clone(),
    }))
}

async fn native_detect_bonk_import_context(
    rpc_url: &str,
    mint: &str,
) -> Result<Option<BonkImportContext>, String> {
    native_detect_bonk_import_context_with_quote_asset(rpc_url, mint, "").await
}

fn uses_single_bundle_tip_last_tx(provider: &str, mev_mode: &str) -> bool {
    provider.trim().eq_ignore_ascii_case("hellomoon")
        && mev_mode.trim().eq_ignore_ascii_case("secure")
}

fn provider_uses_follow_tip(provider: &str) -> bool {
    matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "helius-sender" | "hellomoon" | "jito-bundle"
    )
}

const HELLOMOON_MIN_FOLLOW_TIP_LAMPORTS: u64 = 1_000_000;

fn resolve_follow_tip_lamports(provider: &str, tip_sol: &str, label: &str) -> Result<u64, String> {
    if !provider_uses_follow_tip(provider) {
        return Ok(0);
    }
    if provider.trim().eq_ignore_ascii_case("hellomoon") && tip_sol.trim().is_empty() {
        return Err(format!(
            "{label} cannot be empty when using Hello Moon for follow / snipe / auto-sell."
        ));
    }
    let tip_lamports = parse_decimal_u64(tip_sol, 9, label)?;
    if provider.trim().eq_ignore_ascii_case("hellomoon")
        && tip_lamports < HELLOMOON_MIN_FOLLOW_TIP_LAMPORTS
    {
        return Err(format!(
            "{label} must be at least 0.001 SOL when using Hello Moon for follow / snipe / auto-sell."
        ));
    }
    Ok(tip_lamports)
}

fn parse_decimal_u64(value: &str, decimals: u32, label: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    let parsed = trimmed
        .parse::<f64>()
        .map_err(|error| format!("Invalid {label}: {error}"))?;
    if !parsed.is_finite() || parsed < 0.0 {
        return Err(format!("Invalid {label}: expected a non-negative decimal."));
    }
    let scale = 10u64.saturating_pow(decimals);
    let scaled = parsed * scale as f64;
    if scaled > u64::MAX as f64 {
        return Err(format!("{label} is too large."));
    }
    Ok(scaled.round() as u64)
}

fn priority_fee_sol_to_micro_lamports(priority_fee_sol: &str) -> Result<u64, String> {
    let lamports = parse_decimal_u64(priority_fee_sol, 9, "priority fee")?;
    if lamports == 0 {
        Ok(0)
    } else {
        Ok((lamports.saturating_mul(1_000_000)) / PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT)
    }
}

fn slippage_bps_from_percent(slippage_percent: &str) -> Result<u64, String> {
    let trimmed = slippage_percent.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    if trimmed.starts_with('-') {
        return Err("Invalid slippage percent: expected a non-negative decimal.".to_string());
    }
    let parts = trimmed.split('.').collect::<Vec<_>>();
    if parts.len() > 2 {
        return Err("Invalid slippage percent: expected a decimal value.".to_string());
    }
    let whole = parts[0];
    let fractional = parts.get(1).copied().unwrap_or("");
    if (whole.is_empty() && fractional.is_empty())
        || !whole.chars().all(|ch| ch.is_ascii_digit())
        || !fractional.chars().all(|ch| ch.is_ascii_digit())
    {
        return Err("Invalid slippage percent: expected a non-negative decimal.".to_string());
    }
    if fractional.len() > 2 {
        return Err("slippage percent supports at most 2 decimal places.".to_string());
    }
    let whole_bps = if whole.is_empty() {
        0
    } else {
        whole
            .parse::<u64>()
            .map_err(|error| format!("Invalid slippage percent: {error}"))?
            .checked_mul(100)
            .ok_or_else(|| "slippage percent is too large.".to_string())?
    };
    let fractional_bps = if fractional.is_empty() {
        0
    } else {
        let mut padded = fractional.to_string();
        while padded.len() < 2 {
            padded.push('0');
        }
        padded
            .parse::<u64>()
            .map_err(|error| format!("Invalid slippage percent: {error}"))?
    };
    let bps = whole_bps
        .checked_add(fractional_bps)
        .ok_or_else(|| "slippage percent is too large.".to_string())?;
    if bps > 10_000 {
        return Err("slippage percent must be between 0 and 100.".to_string());
    }
    Ok(bps)
}

fn decode_secret_base64(secret: &[u8]) -> String {
    format!("base64:{}", BASE64.encode(secret))
}

async fn normalize_vanity_secret_for_helper(
    rpc_url: &str,
    raw_secret: &str,
) -> Result<Option<String>, String> {
    let trimmed = raw_secret.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let bytes = read_keypair_bytes(trimmed)
        .map_err(|error| format!("Invalid vanity private key: {error}"))?;
    let keypair = solana_sdk::signature::Keypair::try_from(bytes.as_slice())
        .map_err(|error| format!("Invalid vanity private key: {error}"))?;
    let public_key = bs58::encode(&keypair.to_bytes()[32..]).into_string();
    match fetch_account_data(rpc_url, &public_key, "confirmed").await {
        Ok(_) => {
            return Err(format!(
                "This vanity address has already been used on-chain. Generate a fresh one. ({})",
                public_key
            ));
        }
        Err(error) if error.contains("was not found.") => {}
        Err(error) => {
            return Err(format!(
                "Failed to verify vanity private key availability: {error}"
            ));
        }
    }
    Ok(Some(format!(
        "base58:{}",
        bs58::encode(keypair.to_bytes()).into_string()
    )))
}

fn parse_owner_keypair(secret: &[u8]) -> Result<Keypair, String> {
    Keypair::try_from(secret).map_err(|error| format!("Invalid owner secret key: {error}"))
}

fn compute_budget_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(COMPUTE_BUDGET_PROGRAM_ID)
        .map_err(|error| format!("Invalid Compute Budget program id: {error}"))
}

fn bonk_follow_tx_config(
    compute_unit_limit: u64,
    compute_unit_price_micro_lamports: u64,
    tip_lamports: u64,
    tip_account: &str,
) -> Result<NativeBonkTxConfig, String> {
    Ok(NativeBonkTxConfig {
        compute_unit_limit: u32::try_from(compute_unit_limit)
            .map_err(|_| "Bonk compute unit limit exceeded u32.".to_string())?,
        compute_unit_price_micro_lamports,
        tip_lamports,
        tip_account: tip_account.to_string(),
    })
}

fn configured_default_bonk_sell_compute_unit_limit() -> u64 {
    configured_default_dev_auto_sell_compute_unit_limit()
        .max(configured_default_follow_up_compute_unit_limit())
        .max(DEFAULT_BONK_SELL_COMPUTE_UNIT_LIMIT)
}

fn configured_default_bonk_launchpad_buy_compute_unit_limit() -> u64 {
    configured_default_sniper_buy_compute_unit_limit()
        .max(configured_default_follow_up_compute_unit_limit())
}

fn configured_default_bonk_usd1_dynamic_buy_compute_unit_limit() -> u64 {
    configured_default_launch_compute_unit_limit()
        .max(configured_default_sniper_buy_compute_unit_limit())
        .max(configured_default_follow_up_compute_unit_limit())
}

fn configured_default_bonk_usd1_sell_to_sol_compute_unit_limit() -> u64 {
    std::env::var("LAUNCHDECK_BONK_USD1_SELL_TO_SOL_COMPUTE_UNIT_LIMIT")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_BONK_SELL_COMPUTE_UNIT_LIMIT)
        .max(configured_default_bonk_sell_compute_unit_limit())
}

fn configured_bonk_sell_compute_unit_limit(quote_asset: &str, sell_settlement_asset: &str) -> u64 {
    let settlement_asset = normalize_bonk_sell_settlement_asset(sell_settlement_asset);
    if quote_asset == "usd1" && settlement_asset == "sol" {
        configured_default_bonk_usd1_sell_to_sol_compute_unit_limit()
    } else {
        configured_default_bonk_sell_compute_unit_limit()
    }
}

fn bonk_launch_tx_config(config: &NormalizedConfig) -> Result<NativeBonkTxConfig, String> {
    Ok(NativeBonkTxConfig {
        compute_unit_limit: u32::try_from(
            config
                .tx
                .computeUnitLimit
                .and_then(|value| u64::try_from(value).ok())
                .unwrap_or_else(configured_default_launch_compute_unit_limit),
        )
        .map_err(|error| format!("Invalid Bonk launch compute unit limit: {error}"))?,
        compute_unit_price_micro_lamports: u64::try_from(
            config
                .tx
                .computeUnitPriceMicroLamports
                .unwrap_or_default()
                .max(0),
        )
        .unwrap_or_default(),
        tip_lamports: u64::try_from(config.tx.jitoTipLamports.max(0)).unwrap_or_default(),
        tip_account: config.tx.jitoTipAccount.clone(),
    })
}

fn select_bonk_native_tx_format(requested: &str) -> NativeBonkTxFormat {
    let _ = requested;
    NativeBonkTxFormat::V0
}

fn bonk_bundle_tx_config_for_index(
    tx_config: &NativeBonkTxConfig,
    index: usize,
    total: usize,
    single_bundle_tip_last_tx: bool,
) -> NativeBonkTxConfig {
    if !single_bundle_tip_last_tx || total <= 1 || index + 1 == total {
        return tx_config.clone();
    }
    let mut adjusted = tx_config.clone();
    adjusted.tip_lamports = 0;
    adjusted.tip_account.clear();
    adjusted
}

fn bonk_label_for_bundle_index(label_prefix: &str, index: usize, total: usize) -> String {
    if total <= 1 {
        label_prefix.to_string()
    } else {
        format!("{label_prefix}-{}", index + 1)
    }
}

fn build_compute_unit_limit_instruction(compute_unit_limit: u32) -> Result<Instruction, String> {
    let mut data = vec![2];
    data.extend_from_slice(&compute_unit_limit.to_le_bytes());
    Ok(Instruction {
        program_id: compute_budget_program_id()?,
        accounts: vec![],
        data,
    })
}

fn build_compute_unit_price_instruction(micro_lamports: u64) -> Result<Instruction, String> {
    let mut data = vec![3];
    data.extend_from_slice(&micro_lamports.to_le_bytes());
    Ok(Instruction {
        program_id: compute_budget_program_id()?,
        accounts: vec![],
        data,
    })
}

fn apply_jitodontfront(
    mut instructions: Vec<Instruction>,
    enabled: bool,
    payer: &Pubkey,
) -> Result<Vec<Instruction>, String> {
    if !enabled {
        return Ok(instructions);
    }
    let dontfront = Pubkey::from_str(JITODONTFRONT_ACCOUNT)
        .map_err(|error| format!("Invalid jitodontfront account: {error}"))?;
    if instructions.iter().any(|instruction| {
        instruction
            .accounts
            .iter()
            .any(|meta| meta.pubkey == dontfront)
    }) {
        return Ok(instructions);
    }
    let mut instruction = solana_system_interface::instruction::transfer(payer, payer, 0);
    instruction
        .accounts
        .push(AccountMeta::new_readonly(dontfront, false));
    instructions.insert(0, instruction);
    Ok(instructions)
}

fn with_bonk_tx_settings(
    core_instructions: Vec<Instruction>,
    tx_config: &NativeBonkTxConfig,
    payer: &Pubkey,
    jitodontfront_enabled: bool,
) -> Result<Vec<Instruction>, String> {
    let mut instructions = vec![build_compute_unit_limit_instruction(
        tx_config.compute_unit_limit,
    )?];
    if tx_config.compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            tx_config.compute_unit_price_micro_lamports,
        )?);
    }
    instructions.extend(apply_jitodontfront(
        core_instructions,
        jitodontfront_enabled,
        payer,
    )?);
    if tx_config.tip_lamports > 0 && !tx_config.tip_account.trim().is_empty() {
        let tip_account = Pubkey::from_str(tx_config.tip_account.trim())
            .map_err(|error| format!("Invalid Jito tip account: {error}"))?;
        instructions.push(solana_system_interface::instruction::transfer(
            payer,
            &tip_account,
            tx_config.tip_lamports,
        ));
    }
    Ok(instructions)
}

fn build_bonk_compiled_transaction(
    label: &str,
    tx_format: NativeBonkTxFormat,
    blockhash: &str,
    last_valid_block_height: u64,
    payer: &Keypair,
    extra_signers: &[&Keypair],
    instructions: Vec<Instruction>,
    tx_config: &NativeBonkTxConfig,
) -> Result<CompiledTransaction, String> {
    if tx_format == NativeBonkTxFormat::Legacy {
        return Err(
            "Bonk shared-ALT-only compilation no longer supports legacy transaction format."
                .to_string(),
        );
    }
    let hash = Hash::from_str(blockhash).map_err(|error| error.to_string())?;
    let mut instructions = instructions;
    append_bonk_uniqueness_memo_if_needed(&mut instructions, label)?;
    let mut signers = Vec::with_capacity(1 + extra_signers.len());
    signers.push(payer);
    signers.extend(extra_signers.iter().copied());
    let message = v0::Message::try_compile(&payer.pubkey(), &instructions, &[], hash)
        .map_err(|error| error.to_string())?;
    let transaction = VersionedTransaction::try_new(VersionedMessage::V0(message), &signers)
        .map_err(|error| error.to_string())?;
    let serialized = bincode::serialize(&transaction).map_err(|error| error.to_string())?;
    let serialized_base64 = BASE64.encode(serialized);
    compiled_transaction_signers::remember_compiled_transaction_signers(
        &serialized_base64,
        extra_signers,
    );
    let signature = crate::rpc::precompute_transaction_signature(&serialized_base64);
    Ok(CompiledTransaction {
        label: label.to_string(),
        format: "v0".to_string(),
        blockhash: blockhash.to_string(),
        lastValidBlockHeight: last_valid_block_height,
        serializedBase64: serialized_base64,
        signature,
        lookupTablesUsed: vec![],
        computeUnitLimit: Some(u64::from(tx_config.compute_unit_limit)),
        computeUnitPriceMicroLamports: if tx_config.compute_unit_price_micro_lamports > 0 {
            Some(tx_config.compute_unit_price_micro_lamports)
        } else {
            None
        },
        inlineTipLamports: if tx_config.tip_lamports > 0 {
            Some(tx_config.tip_lamports)
        } else {
            None
        },
        inlineTipAccount: if tx_config.tip_lamports > 0 && !tx_config.tip_account.trim().is_empty()
        {
            Some(tx_config.tip_account.clone())
        } else {
            None
        },
    })
}

fn bonk_instruction_required_extra_signers<'a>(
    payer: &Keypair,
    instructions: &[Instruction],
    extra_signers: &'a [&'a Keypair],
) -> Vec<&'a Keypair> {
    let mut required = Vec::new();
    for signer in extra_signers {
        if signer.pubkey() == payer.pubkey()
            || required
                .iter()
                .any(|entry: &&Keypair| entry.pubkey() == signer.pubkey())
        {
            continue;
        }
        if instructions.iter().any(|instruction| {
            instruction
                .accounts
                .iter()
                .any(|meta| meta.is_signer && meta.pubkey == signer.pubkey())
        }) {
            required.push(*signer);
        }
    }
    required
}

fn bonk_compiled_transaction_fits(
    tx_format: NativeBonkTxFormat,
    blockhash: &str,
    last_valid_block_height: u64,
    payer: &Keypair,
    extra_signers: &[&Keypair],
    instructions: Vec<Instruction>,
    tx_config: &NativeBonkTxConfig,
) -> Result<bool, String> {
    match build_bonk_compiled_transaction(
        "__size-check__",
        tx_format,
        blockhash,
        last_valid_block_height,
        payer,
        extra_signers,
        instructions,
        tx_config,
    ) {
        Ok(compiled) => {
            let raw = BASE64
                .decode(compiled.serializedBase64.as_bytes())
                .map_err(|error| format!("Failed to decode Bonk compiled transaction: {error}"))?;
            Ok(raw.len() <= PACKET_LIMIT_BYTES)
        }
        Err(error) => Err(error),
    }
}

fn compile_bonk_instruction_bundle(
    label_prefix: &str,
    tx_format: NativeBonkTxFormat,
    blockhash: &str,
    last_valid_block_height: u64,
    payer: &Keypair,
    extra_signers: &[&Keypair],
    instruction_groups: Vec<Vec<Instruction>>,
    tx_config: &NativeBonkTxConfig,
    jitodontfront_enabled: bool,
    single_bundle_tip_last_tx: bool,
    preferred_lookup_tables: &[AddressLookupTableAccount],
) -> Result<Vec<CompiledTransaction>, String> {
    let total = instruction_groups.len();
    instruction_groups
        .into_iter()
        .enumerate()
        .map(|(index, instructions)| {
            let group_tx_config =
                bonk_bundle_tx_config_for_index(tx_config, index, total, single_bundle_tip_last_tx);
            let tx_instructions = with_bonk_tx_settings(
                instructions.clone(),
                &group_tx_config,
                &payer.pubkey(),
                jitodontfront_enabled,
            )?;
            let required_signers =
                bonk_instruction_required_extra_signers(payer, &instructions, extra_signers);
            build_bonk_compiled_transaction_with_lookup_preference(
                &bonk_label_for_bundle_index(label_prefix, index, total),
                tx_format,
                blockhash,
                last_valid_block_height,
                payer,
                &required_signers,
                tx_instructions,
                &group_tx_config,
                &[],
                preferred_lookup_tables,
            )
        })
        .collect()
}

fn split_bonk_instruction_bundle(
    label_prefix: &str,
    tx_format: NativeBonkTxFormat,
    blockhash: &str,
    last_valid_block_height: u64,
    payer: &Keypair,
    extra_signers: &[&Keypair],
    instructions: Vec<Instruction>,
    tx_config: &NativeBonkTxConfig,
    jitodontfront_enabled: bool,
    single_bundle_tip_last_tx: bool,
    preferred_lookup_tables: &[AddressLookupTableAccount],
) -> Result<Vec<CompiledTransaction>, String> {
    let mut groups: Vec<Vec<Instruction>> = Vec::new();
    let mut queue: Vec<Instruction> = Vec::new();
    for instruction in instructions {
        if queue.is_empty() {
            queue.push(instruction);
            continue;
        }
        let mut candidate = queue.clone();
        candidate.push(instruction.clone());
        let preview_instructions = with_bonk_tx_settings(
            candidate.clone(),
            tx_config,
            &payer.pubkey(),
            jitodontfront_enabled,
        )?;
        let preview_signers =
            bonk_instruction_required_extra_signers(payer, &candidate, extra_signers);
        let fits = preview_instructions.len() <= 12
            && bonk_compiled_transaction_fits_with_lookup_preference(
                "__size-check__",
                tx_format,
                blockhash,
                last_valid_block_height,
                payer,
                &preview_signers,
                preview_instructions,
                tx_config,
                &[],
                preferred_lookup_tables,
            )?;
        if fits {
            queue = candidate;
        } else {
            if queue.is_empty() {
                return Err(
                    "Bonk launch instruction bundle contained an oversized instruction."
                        .to_string(),
                );
            }
            groups.push(queue);
            queue = vec![instruction];
        }
    }
    if !queue.is_empty() {
        groups.push(queue);
    }
    compile_bonk_instruction_bundle(
        label_prefix,
        tx_format,
        blockhash,
        last_valid_block_height,
        payer,
        extra_signers,
        groups,
        tx_config,
        jitodontfront_enabled,
        single_bundle_tip_last_tx,
        preferred_lookup_tables,
    )
}

fn build_bonk_v0_compiled_transaction_with_lookup_tables(
    label: &str,
    blockhash: &str,
    last_valid_block_height: u64,
    payer: &Keypair,
    extra_signers: &[&Keypair],
    instructions: Vec<Instruction>,
    tx_config: &NativeBonkTxConfig,
    lookup_tables: &[AddressLookupTableAccount],
) -> Result<CompiledTransaction, String> {
    let hash = Hash::from_str(blockhash).map_err(|error| error.to_string())?;
    let mut instructions = instructions;
    append_bonk_uniqueness_memo_if_needed(&mut instructions, label)?;
    let message = v0::Message::try_compile(&payer.pubkey(), &instructions, lookup_tables, hash)
        .map_err(|error| error.to_string())?;
    let lookup_tables_used = message
        .address_table_lookups
        .iter()
        .map(|lookup| lookup.account_key.to_string())
        .collect::<Vec<_>>();
    if lookup_tables_used.is_empty() {
        return Err(format!(
            "{label} compiled as shared-ALT Bonk v0 but did not actually use {BONK_USD1_SUPER_LOOKUP_TABLE}."
        ));
    }
    let message_for_diagnostics = message.clone();
    let mut signers = Vec::with_capacity(1 + extra_signers.len());
    signers.push(payer);
    signers.extend(extra_signers.iter().copied());
    let transaction = VersionedTransaction::try_new(VersionedMessage::V0(message), &signers)
        .map_err(|error| error.to_string())?;
    let serialized = bincode::serialize(&transaction).map_err(|error| error.to_string())?;
    if serialized.len() > PACKET_LIMIT_BYTES {
        return Err(format!(
            "Atomic USD1 action exceeded packet limits after serialize: raw {} > {} bytes",
            serialized.len(),
            PACKET_LIMIT_BYTES
        ));
    }
    crate::alt_diagnostics::emit_alt_coverage_diagnostics(
        "launchdeck-engine",
        label,
        &instructions,
        lookup_tables,
        &message_for_diagnostics,
        Some(serialized.len()),
        &[],
    );
    let serialized_base64 = BASE64.encode(serialized);
    compiled_transaction_signers::remember_compiled_transaction_signers(
        &serialized_base64,
        extra_signers,
    );
    let signature = crate::rpc::precompute_transaction_signature(&serialized_base64);
    Ok(CompiledTransaction {
        label: label.to_string(),
        format: "v0-alt".to_string(),
        blockhash: blockhash.to_string(),
        lastValidBlockHeight: last_valid_block_height,
        serializedBase64: serialized_base64,
        signature,
        lookupTablesUsed: lookup_tables_used,
        computeUnitLimit: Some(u64::from(tx_config.compute_unit_limit)),
        computeUnitPriceMicroLamports: if tx_config.compute_unit_price_micro_lamports > 0 {
            Some(tx_config.compute_unit_price_micro_lamports)
        } else {
            None
        },
        inlineTipLamports: if tx_config.tip_lamports > 0 {
            Some(tx_config.tip_lamports)
        } else {
            None
        },
        inlineTipAccount: if tx_config.tip_lamports > 0 && !tx_config.tip_account.trim().is_empty()
        {
            Some(tx_config.tip_account.clone())
        } else {
            None
        },
    })
}

fn is_compute_budget_instruction(instruction: &Instruction) -> bool {
    instruction.program_id == compute_budget_program_id().unwrap_or_default()
}

fn build_bonk_uniqueness_memo_instruction(label: &str) -> Result<Instruction, String> {
    Ok(Instruction {
        program_id: Pubkey::from_str(MEMO_PROGRAM_ID)
            .map_err(|error| format!("Invalid Bonk memo program id: {error}"))?,
        accounts: vec![],
        data: format!("{label}:{}", Uuid::new_v4()).into_bytes(),
    })
}

fn append_bonk_uniqueness_memo_if_needed(
    instructions: &mut Vec<Instruction>,
    label: &str,
) -> Result<(), String> {
    // Launch transactions already contain the fresh mint signer/key, so the memo is
    // redundant and can be the difference between atomic USD1 launch fitting or not.
    if label == "launch" {
        return Ok(());
    }
    instructions.push(build_bonk_uniqueness_memo_instruction(label)?);
    Ok(())
}

fn is_inline_tip_instruction(
    instruction: &Instruction,
    owner_pubkey: &Pubkey,
    tip_account: &str,
    tip_lamports: u64,
) -> bool {
    if tip_account.trim().is_empty() || tip_lamports == 0 {
        return false;
    }
    if instruction.program_id != solana_system_interface::program::ID
        || instruction.accounts.len() < 2
    {
        return false;
    }
    let Ok(system_instruction) = bincode::deserialize::<
        solana_system_interface::instruction::SystemInstruction,
    >(&instruction.data) else {
        return false;
    };
    match system_instruction {
        solana_system_interface::instruction::SystemInstruction::Transfer { lamports } => {
            instruction.accounts[0].pubkey == *owner_pubkey
                && instruction.accounts[0].is_signer
                && instruction.accounts[1].pubkey
                    == match Pubkey::from_str(tip_account.trim()) {
                        Ok(value) => value,
                        Err(_) => return false,
                    }
                && lamports == tip_lamports
        }
        _ => false,
    }
}

async fn load_lookup_table_account_for_bonk_transaction(
    rpc_url: &str,
    address: &Pubkey,
    commitment: &str,
) -> Result<AddressLookupTableAccount, String> {
    let data = fetch_account_data(rpc_url, &address.to_string(), commitment).await?;
    let table = AddressLookupTable::deserialize(&data)
        .map_err(|error| format!("Failed to decode address lookup table {address}: {error}"))?;
    Ok(AddressLookupTableAccount {
        key: *address,
        addresses: table.addresses.to_vec(),
    })
}

async fn resolve_lookup_table_accounts_for_bonk_transaction(
    rpc_url: &str,
    transaction: &VersionedTransaction,
    commitment: &str,
) -> Result<Vec<AddressLookupTableAccount>, String> {
    let Some(lookups) = transaction.message.address_table_lookups() else {
        return Ok(vec![]);
    };
    let mut resolved = Vec::with_capacity(lookups.len());
    for lookup in lookups {
        resolved.push(
            load_lookup_table_account_for_bonk_transaction(
                rpc_url,
                &lookup.account_key,
                commitment,
            )
            .await?,
        );
    }
    Ok(resolved)
}

fn resolve_bonk_transaction_account_keys(
    transaction: &VersionedTransaction,
    lookup_tables: &[AddressLookupTableAccount],
) -> Result<Vec<Pubkey>, String> {
    let mut account_keys = transaction.message.static_account_keys().to_vec();
    let Some(lookups) = transaction.message.address_table_lookups() else {
        return Ok(account_keys);
    };
    let mut writable = Vec::new();
    let mut readonly = Vec::new();
    for lookup in lookups {
        let table = lookup_tables
            .iter()
            .find(|table| table.key == lookup.account_key)
            .ok_or_else(|| format!("Address lookup table not found: {}", lookup.account_key))?;
        for index in &lookup.writable_indexes {
            let address = table.addresses.get(usize::from(*index)).ok_or_else(|| {
                format!(
                    "Writable ALT index {index} was out of bounds for {}",
                    table.key
                )
            })?;
            writable.push(*address);
        }
        for index in &lookup.readonly_indexes {
            let address = table.addresses.get(usize::from(*index)).ok_or_else(|| {
                format!(
                    "Readonly ALT index {index} was out of bounds for {}",
                    table.key
                )
            })?;
            readonly.push(*address);
        }
    }
    account_keys.extend(writable);
    account_keys.extend(readonly);
    Ok(account_keys)
}

fn decompile_bonk_versioned_transaction_instructions(
    transaction: &VersionedTransaction,
    lookup_tables: &[AddressLookupTableAccount],
) -> Result<Vec<Instruction>, String> {
    let account_keys = resolve_bonk_transaction_account_keys(transaction, lookup_tables)?;
    let mut instructions = Vec::new();
    for compiled in transaction.message.instructions() {
        let program_id = account_keys
            .get(usize::from(compiled.program_id_index))
            .copied()
            .ok_or_else(|| "Bonk transaction referenced a missing program account.".to_string())?;
        let mut accounts = Vec::with_capacity(compiled.accounts.len());
        for account_index in &compiled.accounts {
            let index = usize::from(*account_index);
            let pubkey = account_keys
                .get(index)
                .copied()
                .ok_or_else(|| "Bonk transaction referenced a missing account meta.".to_string())?;
            accounts.push(AccountMeta {
                pubkey,
                is_signer: transaction.message.is_signer(index),
                is_writable: transaction.message.is_maybe_writable(index, None),
            });
        }
        instructions.push(Instruction {
            program_id,
            accounts,
            data: compiled.data.clone(),
        });
    }
    Ok(instructions)
}

fn decode_bonk_versioned_transaction(encoded: &str) -> Result<VersionedTransaction, String> {
    let bytes = BASE64
        .decode(encoded.trim())
        .map_err(|error| format!("Failed to decode Bonk transaction payload: {error}"))?;
    bincode::deserialize::<VersionedTransaction>(&bytes)
        .map_err(|error| format!("Failed to deserialize Bonk versioned transaction: {error}"))
}

async fn decompose_bonk_compiled_v0_transaction(
    rpc_url: &str,
    transaction: &CompiledTransaction,
    commitment: &str,
) -> Result<DecomposedBonkVersionedTransaction, String> {
    let decoded = decode_bonk_versioned_transaction(&transaction.serializedBase64)?;
    let lookup_tables =
        resolve_lookup_table_accounts_for_bonk_transaction(rpc_url, &decoded, commitment).await?;
    let signer_pubkeys = decoded
        .message
        .static_account_keys()
        .iter()
        .take(usize::from(
            decoded.message.header().num_required_signatures,
        ))
        .copied()
        .collect::<Vec<_>>();
    let instructions = decompile_bonk_versioned_transaction_instructions(&decoded, &lookup_tables)?;
    Ok(DecomposedBonkVersionedTransaction {
        instructions,
        lookup_tables,
        signer_pubkeys,
    })
}

fn is_bonk_shared_lookup_table(table: &AddressLookupTableAccount) -> bool {
    table.key.to_string() == BONK_USD1_SUPER_LOOKUP_TABLE
}

fn validate_bonk_shared_lookup_tables_only(
    label: &str,
    tables: &[AddressLookupTableAccount],
) -> Result<Vec<AddressLookupTableAccount>, String> {
    let mut shared_tables = Vec::new();
    let mut rejected = Vec::new();
    for table in tables {
        if is_bonk_shared_lookup_table(table) {
            if !shared_tables
                .iter()
                .any(|existing: &AddressLookupTableAccount| existing.key == table.key)
            {
                shared_tables.push(table.clone());
            }
        } else {
            rejected.push(table.key.to_string());
        }
    }
    if !rejected.is_empty() {
        return Err(format!(
            "{label} encountered unsupported non-shared Bonk lookup tables: {}",
            rejected.join(", ")
        ));
    }
    Ok(shared_tables)
}

fn rewrite_missing_bonk_instruction_signers(
    owner: &Pubkey,
    instructions: &mut [Instruction],
    extra_signers: &[&Keypair],
    allowed_original_signers: &[Pubkey],
) -> Result<Vec<Keypair>, String> {
    let known_signers = extra_signers
        .iter()
        .map(|signer| signer.pubkey())
        .collect::<Vec<_>>();
    let allowed_missing_signers = allowed_original_signers
        .iter()
        .copied()
        .filter(|pubkey| *pubkey != *owner && !known_signers.contains(pubkey))
        .collect::<Vec<_>>();
    let mut missing_signers = Vec::<Pubkey>::new();
    for instruction in instructions.iter() {
        for meta in &instruction.accounts {
            if !meta.is_signer || meta.pubkey == *owner || known_signers.contains(&meta.pubkey) {
                continue;
            }
            if !allowed_missing_signers.contains(&meta.pubkey) {
                return Err(format!(
                    "Atomic Bonk composition encountered unexpected signer {} that was not present in the child transactions.",
                    meta.pubkey
                ));
            }
            if !missing_signers.contains(&meta.pubkey) {
                missing_signers.push(meta.pubkey);
            }
        }
    }
    let replacements = missing_signers
        .into_iter()
        .map(|original| (original, Keypair::new()))
        .collect::<Vec<_>>();
    for instruction in instructions.iter_mut() {
        for meta in &mut instruction.accounts {
            if let Some((_, replacement)) = replacements
                .iter()
                .find(|(original, _)| *original == meta.pubkey)
            {
                meta.pubkey = replacement.pubkey();
            }
        }
    }
    Ok(replacements
        .into_iter()
        .map(|(_, replacement)| replacement)
        .collect())
}

fn bonk_lookup_table_cache() -> &'static Mutex<HashMap<String, BonkLookupTableCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, BonkLookupTableCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn persisted_bonk_lookup_table_cache() -> &'static Mutex<PersistedBonkLookupTableCache> {
    static CACHE: OnceLock<Mutex<PersistedBonkLookupTableCache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let cache = merge_persisted_bonk_lookup_table_caches(
            [
                paths::shared_lookup_table_cache_path(),
                paths::legacy_bonk_lookup_table_cache_path(),
            ]
            .into_iter()
            .filter_map(|path| {
                fs::read_to_string(path).ok().and_then(|raw| {
                    serde_json::from_str::<PersistedBonkLookupTableCache>(&raw).ok()
                })
            }),
        );
        Mutex::new(cache)
    })
}

fn is_persisted_bonk_lookup_table_address(address: &str) -> bool {
    address == BONK_USD1_SUPER_LOOKUP_TABLE
}

fn persist_bonk_lookup_table_account(
    address: &str,
    table: &AddressLookupTableAccount,
) -> Result<(), String> {
    if !is_persisted_bonk_lookup_table_address(address) {
        return Ok(());
    }
    let mut cache = persisted_bonk_lookup_table_cache()
        .lock()
        .map_err(|error| error.to_string())?;
    let addresses = table
        .addresses
        .iter()
        .map(|entry| entry.to_string())
        .collect::<Vec<_>>();
    let address_count = addresses.len();
    let content_hash = lookup_table_address_content_hash(&addresses);
    cache.tables.insert(
        address.to_string(),
        PersistedBonkLookupTableEntry {
            addresses,
            address_count: Some(address_count),
            content_hash: Some(content_hash),
        },
    );
    let serialized = serde_json::to_string_pretty(&*cache).map_err(|error| error.to_string())?;
    let path = paths::shared_lookup_table_cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, serialized).map_err(|error| error.to_string())?;
    Ok(())
}

fn load_persisted_bonk_lookup_table_account(address: &str) -> Option<AddressLookupTableAccount> {
    if !is_persisted_bonk_lookup_table_address(address) {
        return None;
    }
    let cache = persisted_bonk_lookup_table_cache().lock().ok()?;
    let entry = cache.tables.get(address)?;
    if entry.address_count != Some(entry.addresses.len()) {
        eprintln!(
            "[launchdeck-engine][alt-cache] ignoring stale Bonk ALT snapshot {} due to missing/mismatched address count",
            address
        );
        return None;
    }
    let content_hash = lookup_table_address_content_hash(&entry.addresses);
    if entry.content_hash.as_deref() != Some(content_hash.as_str()) {
        eprintln!(
            "[launchdeck-engine][alt-cache] ignoring stale Bonk ALT snapshot {} due to content hash mismatch",
            address
        );
        return None;
    }
    let key = Pubkey::from_str(address).ok()?;
    let addresses = entry
        .addresses
        .iter()
        .map(|entry| Pubkey::from_str(entry))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some(AddressLookupTableAccount { key, addresses })
}

async fn load_bonk_preferred_usd1_lookup_tables_with_metrics(
    rpc_url: &str,
    commitment: &str,
    mut metrics: Option<&mut HelperUsd1QuoteMetrics>,
) -> Result<Vec<AddressLookupTableAccount>, String> {
    let ttl = bonk_lookup_table_cache_ttl();
    if let Ok(cache) = bonk_lookup_table_cache().lock() {
        if let Some(entry) = cache
            .get(BONK_USD1_SUPER_LOOKUP_TABLE)
            .filter(|entry| entry.fetched_at.elapsed() <= ttl)
        {
            if let Some(metrics) = metrics.as_deref_mut() {
                metrics.superAltLocalSnapshotHits =
                    metrics.superAltLocalSnapshotHits.saturating_add(1);
            }
            return Ok(vec![entry.table.clone()]);
        }
    }
    let Ok(address) = Pubkey::from_str(BONK_USD1_SUPER_LOOKUP_TABLE) else {
        return Err(format!(
            "Invalid Bonk shared lookup table address: {BONK_USD1_SUPER_LOOKUP_TABLE}"
        ));
    };
    if let Some(table) = load_persisted_bonk_lookup_table_account(BONK_USD1_SUPER_LOOKUP_TABLE) {
        if let Some(metrics) = metrics.as_deref_mut() {
            metrics.superAltLocalSnapshotHits = metrics.superAltLocalSnapshotHits.saturating_add(1);
        }
        if let Ok(mut cache) = bonk_lookup_table_cache().lock() {
            cache.insert(
                BONK_USD1_SUPER_LOOKUP_TABLE.to_string(),
                BonkLookupTableCacheEntry {
                    fetched_at: std::time::Instant::now(),
                    table: table.clone(),
                },
            );
        }
        return Ok(vec![table]);
    }
    let table = load_lookup_table_account_for_bonk_transaction(rpc_url, &address, commitment)
        .await
        .map_err(|error| {
            format!(
                "Failed to load Bonk shared lookup table {BONK_USD1_SUPER_LOOKUP_TABLE}: {error}"
            )
        })?;
    if let Ok(mut cache) = bonk_lookup_table_cache().lock() {
        cache.insert(
            BONK_USD1_SUPER_LOOKUP_TABLE.to_string(),
            BonkLookupTableCacheEntry {
                fetched_at: std::time::Instant::now(),
                table: table.clone(),
            },
        );
    }
    let _ = persist_bonk_lookup_table_account(BONK_USD1_SUPER_LOOKUP_TABLE, &table);
    if let Some(metrics) = metrics.as_deref_mut() {
        metrics.superAltRpcRefreshes = metrics.superAltRpcRefreshes.saturating_add(1);
    }
    Ok(vec![table])
}

async fn load_bonk_preferred_usd1_lookup_tables(
    rpc_url: &str,
    commitment: &str,
) -> Result<Vec<AddressLookupTableAccount>, String> {
    load_bonk_preferred_usd1_lookup_tables_with_metrics(rpc_url, commitment, None).await
}

fn bonk_compiled_transaction_size_bytes(compiled: &CompiledTransaction) -> Result<usize, String> {
    BASE64
        .decode(compiled.serializedBase64.as_bytes())
        .map(|raw| raw.len())
        .map_err(|error| format!("Failed to decode Bonk compiled transaction: {error}"))
}

fn build_bonk_compiled_transaction_with_lookup_preference(
    label: &str,
    tx_format: NativeBonkTxFormat,
    blockhash: &str,
    last_valid_block_height: u64,
    payer: &Keypair,
    extra_signers: &[&Keypair],
    instructions: Vec<Instruction>,
    tx_config: &NativeBonkTxConfig,
    base_lookup_tables: &[AddressLookupTableAccount],
    preferred_lookup_tables: &[AddressLookupTableAccount],
) -> Result<CompiledTransaction, String> {
    if tx_format == NativeBonkTxFormat::Legacy {
        return Err(
            "Bonk shared-ALT-only compilation no longer supports legacy transaction format."
                .to_string(),
        );
    }
    let _ = validate_bonk_shared_lookup_tables_only(label, base_lookup_tables)?;
    let lookup_tables = validate_bonk_shared_lookup_tables_only(label, preferred_lookup_tables)?;
    if lookup_tables.is_empty() {
        return Err(format!(
            "{label} requires the shared Bonk lookup table {BONK_USD1_SUPER_LOOKUP_TABLE} for v0 compilation."
        ));
    }
    build_bonk_v0_compiled_transaction_with_lookup_tables(
        label,
        blockhash,
        last_valid_block_height,
        payer,
        extra_signers,
        instructions,
        tx_config,
        &lookup_tables,
    )
}

fn bonk_compiled_transaction_fits_with_lookup_preference(
    label: &str,
    tx_format: NativeBonkTxFormat,
    blockhash: &str,
    last_valid_block_height: u64,
    payer: &Keypair,
    extra_signers: &[&Keypair],
    instructions: Vec<Instruction>,
    tx_config: &NativeBonkTxConfig,
    base_lookup_tables: &[AddressLookupTableAccount],
    preferred_lookup_tables: &[AddressLookupTableAccount],
) -> Result<bool, String> {
    match build_bonk_compiled_transaction_with_lookup_preference(
        label,
        tx_format,
        blockhash,
        last_valid_block_height,
        payer,
        extra_signers,
        instructions,
        tx_config,
        base_lookup_tables,
        preferred_lookup_tables,
    ) {
        Ok(compiled) => Ok(bonk_compiled_transaction_size_bytes(&compiled)? <= PACKET_LIMIT_BYTES),
        Err(_error) if tx_format == NativeBonkTxFormat::V0 => Ok(false),
        Err(error) => Err(error),
    }
}

fn filter_atomic_bonk_instructions(
    instructions: Vec<Instruction>,
    owner_pubkey: &Pubkey,
    tx_config: &NativeBonkTxConfig,
) -> Vec<Instruction> {
    instructions
        .into_iter()
        .filter(|instruction| {
            !is_compute_budget_instruction(instruction)
                && !is_memo_instruction(instruction)
                && !is_inline_tip_instruction(
                    instruction,
                    owner_pubkey,
                    &tx_config.tip_account,
                    tx_config.tip_lamports,
                )
        })
        .collect()
}

fn build_bonk_atomic_tx_instructions(
    core_instructions: Vec<Instruction>,
    tx_config: &NativeBonkTxConfig,
    payer: &Pubkey,
    jitodontfront_enabled: bool,
) -> Result<Vec<Instruction>, String> {
    let mut instructions = Vec::new();
    if tx_config.compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            tx_config.compute_unit_price_micro_lamports,
        )?);
    }
    if tx_config.compute_unit_limit > 0 {
        instructions.push(build_compute_unit_limit_instruction(
            tx_config.compute_unit_limit,
        )?);
    }
    instructions.extend(apply_jitodontfront(
        core_instructions,
        jitodontfront_enabled,
        payer,
    )?);
    if tx_config.tip_lamports > 0 && !tx_config.tip_account.trim().is_empty() {
        let tip_account = Pubkey::from_str(tx_config.tip_account.trim())
            .map_err(|error| format!("Invalid Jito tip account: {error}"))?;
        instructions.push(solana_system_interface::instruction::transfer(
            payer,
            &tip_account,
            tx_config.tip_lamports,
        ));
    }
    Ok(instructions)
}

fn bonk_launchpad_auth_pda() -> Result<Pubkey, String> {
    let program = bonk_launchpad_program_id()?;
    Ok(Pubkey::find_program_address(&[b"vault_auth_seed"], &program).0)
}

fn bonk_token_2022_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(TOKEN_2022_PROGRAM_ID)
        .map_err(|error| format!("Invalid Token-2022 program id: {error}"))
}

pub fn derive_follow_owner_token_account(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    spl_associated_token_account::get_associated_token_address_with_program_id(
        owner,
        mint,
        &spl_token::id(),
    )
}

fn bonk_memo_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(MEMO_PROGRAM_ID).map_err(|error| format!("Invalid Memo program id: {error}"))
}

fn is_memo_instruction(instruction: &Instruction) -> bool {
    instruction.program_id == bonk_memo_program_id().unwrap_or_default()
}

fn bonk_launchpad_cpi_event_pda() -> Result<Pubkey, String> {
    let program = bonk_launchpad_program_id()?;
    Ok(Pubkey::find_program_address(&[b"__event_authority"], &program).0)
}

fn bonk_launchpad_pool_vault_pda(pool_id: &Pubkey, mint: &Pubkey) -> Result<Pubkey, String> {
    let program = bonk_launchpad_program_id()?;
    Ok(Pubkey::find_program_address(&[b"pool_vault", pool_id.as_ref(), mint.as_ref()], &program).0)
}

fn bonk_platform_fee_vault_pda(platform_id: &Pubkey, mint: &Pubkey) -> Result<Pubkey, String> {
    let program = bonk_launchpad_program_id()?;
    Ok(Pubkey::find_program_address(&[platform_id.as_ref(), mint.as_ref()], &program).0)
}

fn bonk_creator_fee_vault_pda(creator: &Pubkey, mint: &Pubkey) -> Result<Pubkey, String> {
    let program = bonk_launchpad_program_id()?;
    Ok(Pubkey::find_program_address(&[creator.as_ref(), mint.as_ref()], &program).0)
}

fn bonk_clmm_pool_vault_pda(pool_id: &Pubkey, mint: &Pubkey) -> Result<Pubkey, String> {
    let program = bonk_clmm_program_id()?;
    Ok(Pubkey::find_program_address(&[b"pool_vault", pool_id.as_ref(), mint.as_ref()], &program).0)
}

fn bonk_clmm_ex_bitmap_pda(pool_id: &Pubkey) -> Result<Pubkey, String> {
    let program = bonk_clmm_program_id()?;
    Ok(Pubkey::find_program_address(
        &[b"pool_tick_array_bitmap_extension", pool_id.as_ref()],
        &program,
    )
    .0)
}

fn bonk_clmm_observation_pda(pool_id: &Pubkey) -> Result<Pubkey, String> {
    let program = bonk_clmm_program_id()?;
    Ok(Pubkey::find_program_address(&[b"observation", pool_id.as_ref()], &program).0)
}

fn bonk_metadata_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(MPL_TOKEN_METADATA_PROGRAM_ID)
        .map_err(|error| format!("Invalid token metadata program id: {error}"))
}

fn bonk_metadata_account_pda(mint: &Pubkey) -> Result<Pubkey, String> {
    let metadata_program = bonk_metadata_program_id()?;
    Ok(Pubkey::find_program_address(
        &[b"metadata", metadata_program.as_ref(), mint.as_ref()],
        &metadata_program,
    )
    .0)
}

fn bonk_append_string_layout(data: &mut Vec<u8>, value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    let length = u32::try_from(bytes.len())
        .map_err(|_| "Bonk string field exceeded u32 length.".to_string())?;
    data.extend_from_slice(&length.to_le_bytes());
    data.extend_from_slice(bytes);
    Ok(())
}

fn build_bonk_initialize_v2_instruction(
    owner: &Pubkey,
    mint: &Pubkey,
    launch_mode: &str,
    token_name: &str,
    token_symbol: &str,
    token_uri: &str,
    defaults: &BonkLaunchDefaults,
) -> Result<Instruction, String> {
    let program_id = bonk_launchpad_program_id()?;
    let quote_mint = bonk_quote_mint(defaults.quote.asset)?;
    let config_id = Pubkey::from_str(&bonk_launch_config_id(defaults.quote.asset)?)
        .map_err(|error| format!("Invalid Bonk config id: {error}"))?;
    let platform_id = Pubkey::from_str(bonk_platform_id(launch_mode))
        .map_err(|error| format!("Invalid Bonk platform id: {error}"))?;
    let pool_id =
        Pubkey::find_program_address(&[b"pool", mint.as_ref(), quote_mint.as_ref()], &program_id).0;
    let vault_a = bonk_launchpad_pool_vault_pda(&pool_id, mint)?;
    let vault_b = bonk_launchpad_pool_vault_pda(&pool_id, &quote_mint)?;
    let metadata_id = bonk_metadata_account_pda(mint)?;
    let mut data = Vec::new();
    data.extend_from_slice(&BONK_INITIALIZE_V2_DISCRIMINATOR);
    data.push(
        u8::try_from(BONK_TOKEN_DECIMALS)
            .map_err(|_| "Invalid Bonk token decimals.".to_string())?,
    );
    bonk_append_string_layout(&mut data, token_name)?;
    bonk_append_string_layout(&mut data, token_symbol)?;
    bonk_append_string_layout(&mut data, token_uri)?;
    data.push(defaults.curve_type);
    data.extend_from_slice(&biguint_to_u64(&defaults.supply, "launch supply")?.to_le_bytes());
    if defaults.curve_type == 0 {
        data.extend_from_slice(
            &biguint_to_u64(&defaults.pool.total_sell_a, "launch total sell")?.to_le_bytes(),
        );
    }
    data.extend_from_slice(
        &biguint_to_u64(&defaults.total_fund_raising_b, "launch total fund raising")?.to_le_bytes(),
    );
    data.push(1u8);
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    data.push(0u8);
    Ok(Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(config_id, false),
            AccountMeta::new_readonly(platform_id, false),
            AccountMeta::new_readonly(bonk_launchpad_auth_pda()?, false),
            AccountMeta::new(pool_id, false),
            AccountMeta::new(*mint, true),
            AccountMeta::new_readonly(quote_mint, false),
            AccountMeta::new(vault_a, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new(metadata_id, false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(bonk_metadata_program_id()?, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::rent::id(), false),
            AccountMeta::new_readonly(bonk_launchpad_cpi_event_pda()?, false),
            AccountMeta::new_readonly(program_id, false),
        ],
        data,
    })
}

fn build_bonk_clmm_swap_exact_in_instruction(
    owner: &Pubkey,
    user_input_account: &Pubkey,
    user_output_account: &Pubkey,
    amount_in: u64,
    min_out: u64,
    traversed_tick_array_starts: &[i32],
) -> Result<Instruction, String> {
    let setup = pinned_bonk_usd1_route_setup()?;
    build_bonk_clmm_swap_exact_in_instruction_for_setup(
        owner,
        &setup,
        user_input_account,
        user_output_account,
        amount_in,
        min_out,
        traversed_tick_array_starts,
        &setup.mint_a,
        &setup.mint_b,
    )
}

fn build_bonk_clmm_swap_exact_in_instruction_with_assets(
    owner: &Pubkey,
    user_input_account: &Pubkey,
    user_output_account: &Pubkey,
    amount_in: u64,
    min_out: u64,
    traversed_tick_array_starts: &[i32],
    input_asset: &str,
    output_asset: &str,
) -> Result<Instruction, String> {
    let setup = pinned_bonk_usd1_route_setup()?;
    build_bonk_clmm_swap_exact_in_instruction_for_setup(
        owner,
        &setup,
        user_input_account,
        user_output_account,
        amount_in,
        min_out,
        traversed_tick_array_starts,
        &bonk_quote_mint(input_asset)?,
        &bonk_quote_mint(output_asset)?,
    )
}

fn build_bonk_clmm_swap_exact_in_instruction_for_setup(
    owner: &Pubkey,
    setup: &BonkUsd1RouteSetup,
    user_input_account: &Pubkey,
    user_output_account: &Pubkey,
    amount_in: u64,
    min_out: u64,
    traversed_tick_array_starts: &[i32],
    input_mint: &Pubkey,
    output_mint: &Pubkey,
) -> Result<Instruction, String> {
    let ex_bitmap = bonk_clmm_ex_bitmap_pda(&setup.pool_id)?;
    let tick_arrays = traversed_tick_array_starts
        .iter()
        .map(|start_index| {
            bonk_derive_clmm_tick_array_address(&setup.program_id, &setup.pool_id, *start_index)
        })
        .collect::<Vec<_>>();
    let mut data = Vec::with_capacity(8 + 8 + 8 + 16 + 1);
    data.extend_from_slice(&BONK_CLMM_SWAP_DISCRIMINATOR);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_out.to_le_bytes());
    let (input_vault, output_vault, sqrt_price_limit) =
        if *input_mint == setup.mint_a && *output_mint == setup.mint_b {
            (
                setup.vault_a,
                setup.vault_b,
                BONK_CLMM_MIN_SQRT_PRICE_X64_PLUS_ONE,
            )
        } else if *input_mint == setup.mint_b && *output_mint == setup.mint_a {
            // Raydium rejects the hard max bound for the live USD1 -> SOL unwind,
            // but accepts the default branch when `sqrt_price_limit_x64` is zero.
            (setup.vault_b, setup.vault_a, 0u128)
        } else {
            return Err(
                "Bonk CLMM swap input/output mints do not match the selected pool.".to_string(),
            );
        };
    data.extend_from_slice(&sqrt_price_limit.to_le_bytes());
    // Raydium CLMM exact-in swaps always set `is_base_input = true`.
    // Direction still comes from the input/output mint ordering above.
    data.push(1u8);
    let mut accounts = vec![
        AccountMeta::new_readonly(*owner, true),
        AccountMeta::new_readonly(setup.amm_config, false),
        AccountMeta::new(setup.pool_id, false),
        AccountMeta::new(*user_input_account, false),
        AccountMeta::new(*user_output_account, false),
        AccountMeta::new(input_vault, false),
        AccountMeta::new(output_vault, false),
        AccountMeta::new(setup.observation_id, false),
        AccountMeta::new_readonly(spl_token::id(), false),
        AccountMeta::new_readonly(bonk_token_2022_program_id()?, false),
        AccountMeta::new_readonly(bonk_memo_program_id()?, false),
        AccountMeta::new_readonly(*input_mint, false),
        AccountMeta::new_readonly(*output_mint, false),
        AccountMeta::new(ex_bitmap, false),
    ];
    accounts.extend(
        tick_arrays
            .into_iter()
            .map(|pubkey| AccountMeta::new(pubkey, false)),
    );
    Ok(Instruction {
        program_id: setup.program_id,
        accounts,
        data,
    })
}

fn pinned_bonk_usd1_route_setup() -> Result<BonkUsd1RouteSetup, String> {
    let pool_id = Pubkey::from_str(BONK_PINNED_USD1_ROUTE_POOL_ID)
        .map_err(|error| format!("Invalid pinned Bonk USD1 route pool id: {error}"))?;
    let program_id = bonk_clmm_program_id()?;
    let amm_config = Pubkey::from_str(BONK_PREFERRED_USD1_ROUTE_CONFIG_ID)
        .map_err(|error| format!("Invalid pinned Bonk USD1 route config id: {error}"))?;
    let mint_a = bonk_quote_mint("sol")?;
    let mint_b = bonk_quote_mint("usd1")?;
    Ok(BonkUsd1RouteSetup {
        pool_id,
        program_id,
        amm_config,
        mint_a,
        mint_b,
        vault_a: bonk_clmm_pool_vault_pda(&pool_id, &mint_a)?,
        vault_b: bonk_clmm_pool_vault_pda(&pool_id, &mint_b)?,
        observation_id: bonk_clmm_observation_pda(&pool_id)?,
        tick_spacing: 0,
        trade_fee_rate: 0,
        sqrt_price_x64: BigUint::ZERO,
        liquidity: BigUint::ZERO,
        tick_current: 0,
        mint_a_decimals: 0,
        mint_b_decimals: 0,
        current_price: 0.0,
        tick_arrays_desc: vec![],
        tick_arrays_asc: vec![],
        tick_arrays: HashMap::new(),
    })
}

fn build_bonk_cpmm_swap_exact_in_instruction(
    owner: &Pubkey,
    context: &NativeBonkCpmmPoolContext,
    user_input_account: &Pubkey,
    user_output_account: &Pubkey,
    amount_in: u64,
    min_out: u64,
    input_mint: &Pubkey,
    output_mint: &Pubkey,
) -> Result<Instruction, String> {
    let authority = bonk_cpmm_pool_authority()?;
    let input_is_a =
        *input_mint == context.pool.token_0_mint && *output_mint == context.pool.token_1_mint;
    let input_is_b =
        *input_mint == context.pool.token_1_mint && *output_mint == context.pool.token_0_mint;
    if !input_is_a && !input_is_b {
        return Err(
            "Bonk CPMM swap input/output mints do not match the selected pool.".to_string(),
        );
    }
    let (input_vault, output_vault, input_token_program, output_token_program) = if input_is_a {
        (
            context.pool.vault_a,
            context.pool.vault_b,
            context.pool.token_0_program,
            context.pool.token_1_program,
        )
    } else {
        (
            context.pool.vault_b,
            context.pool.vault_a,
            context.pool.token_1_program,
            context.pool.token_0_program,
        )
    };
    let mut data = Vec::with_capacity(8 + 8 + 8);
    data.extend_from_slice(&BONK_CPMM_SWAP_BASE_INPUT_DISCRIMINATOR);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_out.to_le_bytes());
    Ok(Instruction {
        program_id: bonk_cpmm_program_id()?,
        accounts: vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new_readonly(authority, false),
            AccountMeta::new_readonly(context.pool.config_id, false),
            AccountMeta::new(context.pool_id, false),
            AccountMeta::new(*user_input_account, false),
            AccountMeta::new(*user_output_account, false),
            AccountMeta::new(input_vault, false),
            AccountMeta::new(output_vault, false),
            AccountMeta::new_readonly(input_token_program, false),
            AccountMeta::new_readonly(output_token_program, false),
            AccountMeta::new_readonly(*input_mint, false),
            AccountMeta::new_readonly(*output_mint, false),
            AccountMeta::new(context.pool.observation_id, false),
        ],
        data,
    })
}

fn build_bonk_buy_exact_in_instruction(
    owner: &Pubkey,
    pool_context: &NativeBonkPoolContext,
    user_token_account_a: &Pubkey,
    user_token_account_b: &Pubkey,
    amount_b: u64,
    min_amount_a: u64,
) -> Result<Instruction, String> {
    let launchpad_program = bonk_launchpad_program_id()?;
    let auth = bonk_launchpad_auth_pda()?;
    let vault_a = bonk_launchpad_pool_vault_pda(&pool_context.pool_id, &pool_context.pool.mint_a)?;
    let quote_mint = bonk_quote_mint(pool_context.quote.asset)?;
    let vault_b = bonk_launchpad_pool_vault_pda(&pool_context.pool_id, &quote_mint)?;
    let platform_claim_fee_vault =
        bonk_platform_fee_vault_pda(&pool_context.pool.platform_id, &quote_mint)?;
    let creator_claim_fee_vault =
        bonk_creator_fee_vault_pda(&pool_context.pool.creator, &quote_mint)?;
    let cpi_event = bonk_launchpad_cpi_event_pda()?;
    let token_program = pool_context.token_program;
    let quote_token_program = spl_token::id();
    let mut data = Vec::with_capacity(32);
    data.extend_from_slice(&BONK_BUY_EXACT_IN_DISCRIMINATOR);
    data.extend_from_slice(&amount_b.to_le_bytes());
    data.extend_from_slice(&min_amount_a.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    Ok(Instruction {
        program_id: launchpad_program,
        accounts: vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(auth, false),
            AccountMeta::new_readonly(pool_context.pool.config_id, false),
            AccountMeta::new_readonly(pool_context.pool.platform_id, false),
            AccountMeta::new(pool_context.pool_id, false),
            AccountMeta::new(*user_token_account_a, false),
            AccountMeta::new(*user_token_account_b, false),
            AccountMeta::new(vault_a, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(pool_context.pool.mint_a, false),
            AccountMeta::new_readonly(quote_mint, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(quote_token_program, false),
            AccountMeta::new_readonly(cpi_event, false),
            AccountMeta::new_readonly(launchpad_program, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new(platform_claim_fee_vault, false),
            AccountMeta::new(creator_claim_fee_vault, false),
        ],
        data,
    })
}

fn build_bonk_sell_exact_in_instruction(
    owner: &Pubkey,
    pool_context: &NativeBonkPoolContext,
    user_token_account_a: &Pubkey,
    user_token_account_b: &Pubkey,
    amount_a: u64,
    min_amount_b: u64,
) -> Result<Instruction, String> {
    let launchpad_program = bonk_launchpad_program_id()?;
    let auth = bonk_launchpad_auth_pda()?;
    let vault_a = bonk_launchpad_pool_vault_pda(&pool_context.pool_id, &pool_context.pool.mint_a)?;
    let quote_mint = bonk_quote_mint(pool_context.quote.asset)?;
    let vault_b = bonk_launchpad_pool_vault_pda(&pool_context.pool_id, &quote_mint)?;
    let platform_claim_fee_vault =
        bonk_platform_fee_vault_pda(&pool_context.pool.platform_id, &quote_mint)?;
    let creator_claim_fee_vault =
        bonk_creator_fee_vault_pda(&pool_context.pool.creator, &quote_mint)?;
    let cpi_event = bonk_launchpad_cpi_event_pda()?;
    let token_program = pool_context.token_program;
    let quote_token_program = spl_token::id();
    let mut data = Vec::with_capacity(32);
    data.extend_from_slice(&BONK_SELL_EXACT_IN_DISCRIMINATOR);
    data.extend_from_slice(&amount_a.to_le_bytes());
    data.extend_from_slice(&min_amount_b.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    Ok(Instruction {
        program_id: launchpad_program,
        accounts: vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(auth, false),
            AccountMeta::new_readonly(pool_context.pool.config_id, false),
            AccountMeta::new_readonly(pool_context.pool.platform_id, false),
            AccountMeta::new(pool_context.pool_id, false),
            AccountMeta::new(*user_token_account_a, false),
            AccountMeta::new(*user_token_account_b, false),
            AccountMeta::new(vault_a, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(pool_context.pool.mint_a, false),
            AccountMeta::new_readonly(quote_mint, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(quote_token_program, false),
            AccountMeta::new_readonly(cpi_event, false),
            AccountMeta::new_readonly(launchpad_program, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new(platform_claim_fee_vault, false),
            AccountMeta::new(creator_claim_fee_vault, false),
        ],
        data,
    })
}

fn build_bonk_wrapped_sol_open_instructions(
    owner: &Pubkey,
    wrapped_account: &Pubkey,
    lamports: u64,
) -> Result<Vec<Instruction>, String> {
    let token_program = spl_token::id();
    Ok(vec![
        solana_system_interface::instruction::create_account(
            owner,
            wrapped_account,
            lamports,
            BONK_SPL_TOKEN_ACCOUNT_LEN,
            &token_program,
        ),
        spl_token::instruction::initialize_account3(
            &token_program,
            wrapped_account,
            &bonk_quote_mint("sol")?,
            owner,
        )
        .map_err(|error| format!("Failed to build wrapped SOL initialize instruction: {error}"))?,
        spl_token::instruction::sync_native(&token_program, wrapped_account)
            .map_err(|error| format!("Failed to build sync-native instruction: {error}"))?,
    ])
}

fn build_bonk_wrapped_sol_close_instruction(
    owner: &Pubkey,
    wrapped_account: &Pubkey,
) -> Result<Instruction, String> {
    spl_token::instruction::close_account(&spl_token::id(), wrapped_account, owner, owner, &[])
        .map_err(|error| format!("Failed to build wrapped SOL close instruction: {error}"))
}

fn route_account_index(
    route_accounts: &[AccountMeta],
    pubkey: &Pubkey,
    context: &str,
) -> Result<u16, String> {
    route_accounts
        .iter()
        .position(|meta| meta.pubkey == *pubkey)
        .ok_or_else(|| format!("{context} account {pubkey} is missing from route accounts"))?
        .try_into()
        .map_err(|_| format!("{context} route account index does not fit in u16"))
}

fn route_len_u16(len: usize, context: &str) -> Result<u16, String> {
    len.try_into()
        .map_err(|_| format!("{context} route account count does not fit in u16"))
}

#[allow(clippy::too_many_arguments)]
async fn build_bonk_dynamic_usd1_buy_from_sol_route(
    rpc_url: &str,
    commitment: &str,
    owner: &Keypair,
    allow_ata_creation: bool,
    tx_config: &NativeBonkTxConfig,
    jitodontfront_enabled: bool,
    mint_pubkey: &Pubkey,
    gross_sol_in_lamports: u64,
    net_sol_in_lamports: u64,
    min_usd1_out: u64,
    route_setup: &BonkUsd1RouteSetup,
    buy_ix: Instruction,
    token_account: &Pubkey,
    token_program: &Pubkey,
    min_token_out: u64,
    fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let owner_pubkey = owner.pubkey();
    let quote_token_program = spl_token::id();
    let usd1_mint = bonk_quote_mint("usd1")?;
    let usd1_account = spl_associated_token_account::get_associated_token_address_with_program_id(
        &owner_pubkey,
        &usd1_mint,
        &quote_token_program,
    );
    let route_wsol_account = route_wsol_pda(&owner_pubkey, 0);
    let route_quote = bonk_quote_usd1_from_exact_sol_input(
        route_setup,
        &bonk_biguint_from_u64(net_sol_in_lamports),
        BONK_USD1_ROUTE_SLIPPAGE_BPS,
    )?;
    let first_leg_ix = build_bonk_clmm_swap_exact_in_instruction_for_setup(
        &owner_pubkey,
        route_setup,
        &route_wsol_account,
        &usd1_account,
        net_sol_in_lamports,
        min_usd1_out,
        &route_quote.traversed_tick_array_starts,
        &route_setup.mint_a,
        &route_setup.mint_b,
    )?;

    let mut route_accounts = vec![
        AccountMeta::new_readonly(first_leg_ix.program_id, false),
        AccountMeta::new_readonly(buy_ix.program_id, false),
    ];
    let first_program_index = 0u16;
    let buy_program_index = 1u16;
    let first_accounts_start =
        route_len_u16(route_accounts.len(), "Bonk USD1 buy route first leg")?;
    route_accounts.extend(first_leg_ix.accounts.iter().cloned());
    let first_accounts_len =
        route_len_u16(first_leg_ix.accounts.len(), "Bonk USD1 buy route first leg")?;
    let first_output_index = route_account_index(
        &route_accounts,
        &usd1_account,
        "Bonk USD1 buy route USD1 output",
    )?;
    let buy_accounts_start = route_len_u16(route_accounts.len(), "Bonk USD1 buy route venue leg")?;
    route_accounts.extend(buy_ix.accounts.iter().cloned());
    let buy_accounts_len = route_len_u16(buy_ix.accounts.len(), "Bonk USD1 buy route venue leg")?;
    let buy_output_index = route_account_index(
        &route_accounts,
        token_account,
        "Bonk USD1 buy route token output",
    )?;

    let zeroed_wsol = Pubkey::new_from_array([0; 32]);
    let request = ExecuteSwapRouteRequest {
        version: WRAPPER_ABI_VERSION,
        route_mode: SwapRouteMode::Mixed,
        direction: SwapRouteDirection::Buy,
        settlement: SwapRouteSettlement::Token,
        fee_mode: SwapRouteFeeMode::SolPre,
        wsol_lane: 0,
        fee_bps,
        gross_sol_in_lamports,
        gross_token_in_amount: 0,
        min_net_output: min_token_out,
        route_accounts_offset: EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT
            + EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT,
        intermediate_account_index: first_output_index,
        token_fee_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
        legs: vec![
            SwapRouteLeg {
                program_account_index: first_program_index,
                accounts_start: first_accounts_start,
                accounts_len: first_accounts_len,
                input_source: SwapLegInputSource::GrossSolNetOfFee,
                input_amount: net_sol_in_lamports,
                input_patch_offset: 8,
                output_account_index: first_output_index,
                ix_data: first_leg_ix.data,
            },
            SwapRouteLeg {
                program_account_index: buy_program_index,
                accounts_start: buy_accounts_start,
                accounts_len: buy_accounts_len,
                input_source: SwapLegInputSource::PreviousTokenDelta,
                input_amount: min_usd1_out,
                input_patch_offset: 8,
                output_account_index: buy_output_index,
                ix_data: buy_ix.data,
            },
        ],
    };
    let wrapper_ix = build_execute_swap_route_instruction(
        &owner_pubkey,
        &zeroed_wsol,
        &route_wsol_account,
        &first_leg_ix.program_id,
        &request,
        &route_accounts,
        None,
    )?;

    let mut instructions = Vec::new();
    if allow_ata_creation {
        instructions.push(build_bonk_create_ata_instruction(
            &owner_pubkey,
            &usd1_mint,
            &quote_token_program,
        ));
        instructions.push(build_bonk_create_ata_instruction(
            &owner_pubkey,
            mint_pubkey,
            token_program,
        ));
    }
    instructions.push(wrapper_ix);
    let tx_instructions = build_bonk_atomic_tx_instructions(
        instructions,
        tx_config,
        &owner_pubkey,
        jitodontfront_enabled,
    )?;
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, commitment).await?;
    let preferred_lookup_tables =
        load_bonk_preferred_usd1_lookup_tables(rpc_url, commitment).await?;
    build_bonk_compiled_transaction_with_lookup_preference(
        "follow-buy-atomic",
        NativeBonkTxFormat::V0,
        &blockhash,
        last_valid_block_height,
        owner,
        &[],
        tx_instructions,
        tx_config,
        &[],
        &preferred_lookup_tables,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_bonk_dynamic_usd1_sell_to_sol_route(
    owner: &Pubkey,
    usd1_account: &Pubkey,
    first_leg_ix: Instruction,
    first_leg_input_amount: u64,
    min_usd1_out: u64,
    expected_usd1_out: u64,
    route_setup: &BonkUsd1RouteSetup,
    fee_bps: u16,
) -> Result<Vec<Instruction>, String> {
    if expected_usd1_out < min_usd1_out {
        return Err("Bonk USD1 dynamic sell expected output was below its minimum.".to_string());
    }
    let route_wsol_account = route_wsol_pda(owner, 0);
    let unwind_quote = bonk_quote_sol_from_exact_usd1_input(
        route_setup,
        &bonk_biguint_from_u64(expected_usd1_out),
        BONK_USD1_ROUTE_SLIPPAGE_BPS,
    )?;
    let unwind_min_out =
        biguint_to_u64(&unwind_quote.min_out, "Bonk USD1 dynamic unwind min output")?;
    let unwind_ix = build_bonk_clmm_swap_exact_in_instruction_for_setup(
        owner,
        route_setup,
        usd1_account,
        &route_wsol_account,
        expected_usd1_out,
        unwind_min_out,
        &unwind_quote.traversed_tick_array_starts,
        &route_setup.mint_b,
        &route_setup.mint_a,
    )?;

    let mut route_accounts = vec![
        AccountMeta::new_readonly(first_leg_ix.program_id, false),
        AccountMeta::new_readonly(unwind_ix.program_id, false),
    ];
    let first_program_index = 0u16;
    let unwind_program_index = 1u16;
    let first_accounts_start = route_len_u16(route_accounts.len(), "Bonk USD1 first leg")?;
    route_accounts.extend(first_leg_ix.accounts.iter().cloned());
    let first_accounts_len = route_len_u16(first_leg_ix.accounts.len(), "Bonk USD1 first leg")?;
    let first_output_index =
        route_account_index(&route_accounts, usd1_account, "Bonk USD1 first leg output")?;
    let unwind_accounts_start = route_len_u16(route_accounts.len(), "Bonk USD1 unwind leg")?;
    route_accounts.extend(unwind_ix.accounts.iter().cloned());
    let unwind_accounts_len = route_len_u16(unwind_ix.accounts.len(), "Bonk USD1 unwind leg")?;
    let unwind_output_index = route_account_index(
        &route_accounts,
        &route_wsol_account,
        "Bonk USD1 unwind output",
    )?;

    let min_net_sol_out = unwind_min_out
        .checked_sub(estimate_sol_in_fee_lamports(unwind_min_out, fee_bps))
        .ok_or_else(|| "Bonk USD1 dynamic unwind min output fee underflowed".to_string())?;
    let fee_vault_wsol_ata =
        spl_associated_token_account::get_associated_token_address_with_program_id(
            &wrapper_fee_vault(),
            &wrapper_wsol_mint(),
            &wrapper_token_program_id(),
        );
    let request = ExecuteSwapRouteRequest {
        version: WRAPPER_ABI_VERSION,
        route_mode: SwapRouteMode::Mixed,
        direction: SwapRouteDirection::Sell,
        settlement: SwapRouteSettlement::Wsol,
        fee_mode: SwapRouteFeeMode::WsolPost,
        wsol_lane: 0,
        fee_bps,
        gross_sol_in_lamports: 0,
        gross_token_in_amount: 0,
        min_net_output: min_net_sol_out,
        route_accounts_offset: EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT
            + EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT,
        intermediate_account_index: first_output_index,
        token_fee_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
        legs: vec![
            SwapRouteLeg {
                program_account_index: first_program_index,
                accounts_start: first_accounts_start,
                accounts_len: first_accounts_len,
                input_source: SwapLegInputSource::Fixed,
                input_amount: first_leg_input_amount,
                input_patch_offset: SWAP_ROUTE_NO_PATCH_OFFSET,
                output_account_index: first_output_index,
                ix_data: first_leg_ix.data,
            },
            SwapRouteLeg {
                program_account_index: unwind_program_index,
                accounts_start: unwind_accounts_start,
                accounts_len: unwind_accounts_len,
                input_source: SwapLegInputSource::PreviousTokenDelta,
                input_amount: expected_usd1_out,
                input_patch_offset: 8,
                output_account_index: unwind_output_index,
                ix_data: unwind_ix.data,
            },
        ],
    };
    let wrapper_ix = build_execute_swap_route_instruction(
        owner,
        &fee_vault_wsol_ata,
        &route_wsol_account,
        &first_leg_ix.program_id,
        &request,
        &route_accounts,
        None,
    )?;

    let mut instructions = Vec::new();
    instructions.push(wrapper_ix);
    Ok(instructions)
}

#[allow(clippy::too_many_arguments)]
fn build_bonk_token_fee_route_instruction(
    owner: &Pubkey,
    fee_bps: u16,
    fee_mint: &Pubkey,
    fee_token_program: &Pubkey,
    gross_token_in_amount: u64,
    min_net_output: u64,
    direction: SwapRouteDirection,
    fee_mode: SwapRouteFeeMode,
    venue_ix: Instruction,
    token_fee_account: &Pubkey,
    output_account: &Pubkey,
) -> Result<Instruction, String> {
    let fee_vault = wrapper_fee_vault();
    let token_fee_vault_ata =
        spl_associated_token_account::get_associated_token_address_with_program_id(
            &fee_vault,
            fee_mint,
            fee_token_program,
        );
    let zeroed_wsol = Pubkey::new_from_array([0; 32]);
    let mut route_accounts = vec![AccountMeta::new_readonly(venue_ix.program_id, false)];
    route_accounts.extend(venue_ix.accounts.iter().cloned());
    let token_fee_account_index = route_account_index(
        &route_accounts,
        token_fee_account,
        "Bonk token-fee route fee account",
    )?;
    let output_account_index = route_account_index(
        &route_accounts,
        output_account,
        "Bonk token-fee route output account",
    )?;
    let accounts_len = route_len_u16(venue_ix.accounts.len(), "Bonk token-fee route leg")?;
    let request = ExecuteSwapRouteRequest {
        version: WRAPPER_ABI_VERSION,
        route_mode: SwapRouteMode::TokenOnly,
        direction,
        settlement: SwapRouteSettlement::Token,
        fee_mode,
        wsol_lane: 0,
        fee_bps,
        gross_sol_in_lamports: 0,
        gross_token_in_amount,
        min_net_output,
        route_accounts_offset: EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT
            + EXECUTE_SWAP_ROUTE_TOKEN_FEE_ACCOUNT_COUNT,
        intermediate_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
        token_fee_account_index,
        legs: vec![SwapRouteLeg {
            program_account_index: 0,
            accounts_start: 1,
            accounts_len,
            input_source: if matches!(fee_mode, SwapRouteFeeMode::TokenPre) {
                SwapLegInputSource::GrossTokenNetOfFee
            } else {
                SwapLegInputSource::Fixed
            },
            input_amount: gross_token_in_amount,
            input_patch_offset: SWAP_ROUTE_NO_PATCH_OFFSET,
            output_account_index,
            ix_data: venue_ix.data,
        }],
    };
    build_execute_swap_route_instruction(
        owner,
        &zeroed_wsol,
        &zeroed_wsol,
        &venue_ix.program_id,
        &request,
        &route_accounts,
        Some(&token_fee_vault_ata),
    )
}

fn build_bonk_create_ata_instruction(
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Instruction {
    spl_associated_token_account::instruction::create_associated_token_account_idempotent(
        owner,
        owner,
        mint,
        token_program,
    )
}

fn build_bonk_follow_pool_state(pool: &DecodedBonkLaunchpadPool) -> BonkCurvePoolState {
    BonkCurvePoolState {
        total_sell_a: bonk_biguint_from_u64(pool.total_sell_a),
        virtual_a: bonk_biguint_from_u64(pool.virtual_a),
        virtual_b: bonk_biguint_from_u64(pool.virtual_b),
        real_a: bonk_biguint_from_u64(pool.real_a),
        real_b: bonk_biguint_from_u64(pool.real_b),
    }
}

fn build_prelaunch_bonk_pool_context(
    defaults: &BonkLaunchDefaults,
    mint: &Pubkey,
    creator: &Pubkey,
    launch_mode: &str,
) -> Result<NativeBonkPoolContext, String> {
    let quote_mint = bonk_quote_mint(defaults.quote.asset)?;
    let launchpad_program = bonk_launchpad_program_id()?;
    let pool_id = Pubkey::find_program_address(
        &[b"pool", mint.as_ref(), quote_mint.as_ref()],
        &launchpad_program,
    )
    .0;
    let config_id = Pubkey::from_str(&bonk_launch_config_id(defaults.quote.asset)?)
        .map_err(|error| format!("Invalid Bonk config id: {error}"))?;
    let platform_id = Pubkey::from_str(bonk_platform_id(launch_mode))
        .map_err(|error| format!("Invalid Bonk platform id: {error}"))?;
    Ok(NativeBonkPoolContext {
        pool_id,
        pool: DecodedBonkLaunchpadPool {
            creator: *creator,
            status: 0,
            supply: biguint_to_u64(&defaults.supply, "prelaunch supply")?,
            config_id,
            total_sell_a: biguint_to_u64(&defaults.pool.total_sell_a, "prelaunch total sell")?,
            virtual_a: biguint_to_u64(&defaults.pool.virtual_a, "prelaunch virtual A")?,
            virtual_b: biguint_to_u64(&defaults.pool.virtual_b, "prelaunch virtual B")?,
            real_a: 0,
            real_b: 0,
            platform_id,
            mint_a: *mint,
        },
        config: DecodedBonkLaunchpadConfig {
            curve_type: defaults.curve_type,
            migrate_fee: 0,
            trade_fee_rate: biguint_to_u64(&defaults.trade_fee_rate, "prelaunch trade fee rate")?,
        },
        platform: DecodedBonkPlatformConfig {
            fee_rate: biguint_to_u64(&defaults.platform_fee_rate, "prelaunch platform fee rate")?,
            creator_fee_rate: biguint_to_u64(
                &defaults.creator_fee_rate,
                "prelaunch creator fee rate",
            )?,
        },
        quote: defaults.quote.clone(),
        token_program: spl_token::id(),
    })
}

async fn load_bonk_pool_context_by_pool_id(
    rpc_url: &str,
    pool_id_input: &str,
    quote_asset: &str,
    commitment: &str,
) -> Result<NativeBonkPoolContext, String> {
    let pool_id = Pubkey::from_str(pool_id_input)
        .map_err(|error| format!("Invalid Bonk pool id: {error}"))?;
    let pool_data = fetch_account_data(rpc_url, pool_id_input, commitment).await?;
    let pool = decode_bonk_launchpad_pool(&pool_data)?;
    let config_id = pool.config_id.to_string();
    let platform_id = pool.platform_id.to_string();
    let mint_a = pool.mint_a.to_string();
    let (config_data, platform_data, (_, mint_owner)) = tokio::try_join!(
        fetch_account_data(rpc_url, &config_id, commitment),
        fetch_account_data(rpc_url, &platform_id, commitment),
        fetch_account_data_with_owner(rpc_url, &mint_a, commitment),
    )?;
    let token_program = Pubkey::from_str(&mint_owner)
        .map_err(|error| format!("Invalid Bonk mint owner: {error}"))?;
    Ok(NativeBonkPoolContext {
        pool_id,
        pool,
        config: decode_bonk_launchpad_config(&config_data)?,
        platform: decode_bonk_platform_config(&platform_data)?,
        quote: bonk_quote_asset_config(quote_asset),
        token_program,
    })
}

async fn load_live_bonk_pool_context(
    rpc_url: &str,
    mint: &Pubkey,
    quote_asset: &str,
    commitment: &str,
) -> Result<NativeBonkPoolContext, String> {
    let requested_quote = bonk_quote_asset_config(quote_asset);
    let mut errors = Vec::new();
    let pool_id = derive_canonical_pool_id(requested_quote.asset, &mint.to_string()).await?;
    for attempt in 0..6 {
        match load_bonk_pool_context_by_pool_id(
            rpc_url,
            &pool_id,
            requested_quote.asset,
            commitment,
        )
        .await
        {
            Ok(context) => return Ok(context),
            Err(error) => {
                errors.push(format!("{}:{}: {}", requested_quote.asset, pool_id, error));
                if attempt < 5 {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
    }
    Err(format!(
        "Unable to resolve Bonk live pool context. Attempts: {}",
        errors.join(" | ")
    ))
}

fn resolve_bonk_supported_quote_asset(
    requested_quote_asset: &str,
    mint_a: &Pubkey,
    mint_b: &Pubkey,
) -> Result<BonkQuoteAssetConfig, String> {
    let requested = bonk_quote_asset_config(requested_quote_asset);
    if requested.mint == mint_a.to_string() || requested.mint == mint_b.to_string() {
        return Ok(requested);
    }
    Err(format!(
        "Bonk Raydium pool does not contain the requested {} quote mint.",
        requested.label
    ))
}

fn net_cpmm_reserve(
    vault_amount: u64,
    protocol_fees: u64,
    fund_fees: u64,
    creator_fees: u64,
) -> u64 {
    vault_amount
        .saturating_sub(protocol_fees)
        .saturating_sub(fund_fees)
        .saturating_sub(creator_fees)
}

async fn build_bonk_cpmm_pool_context_from_data(
    rpc_url: &str,
    pool_id: &Pubkey,
    quote_asset: &str,
    commitment: &str,
    pool_data: &[u8],
) -> Result<NativeBonkCpmmPoolContext, String> {
    let pool = decode_bonk_cpmm_pool(pool_data)?;
    let quote =
        resolve_bonk_supported_quote_asset(quote_asset, &pool.token_0_mint, &pool.token_1_mint)?;
    let config_id = pool.config_id.to_string();
    let vault_accounts = vec![pool.vault_a.to_string(), pool.vault_b.to_string()];
    let (config_data, vault_datas) = tokio::try_join!(
        fetch_account_data(rpc_url, &config_id, commitment),
        fetch_multiple_account_data(rpc_url, &vault_accounts, commitment),
    )?;
    let config = decode_bonk_cpmm_config(&config_data)?;
    if vault_datas.len() != 2 {
        return Err(
            "Bonk CPMM vault lookup returned an unexpected number of accounts.".to_string(),
        );
    }
    let vault_a_data = vault_datas
        .first()
        .and_then(|value| value.as_ref())
        .ok_or_else(|| "Bonk CPMM vault A account was missing.".to_string())?;
    let vault_b_data = vault_datas
        .get(1)
        .and_then(|value| value.as_ref())
        .ok_or_else(|| "Bonk CPMM vault B account was missing.".to_string())?;
    let creator_fees_a = if pool.enable_creator_fee {
        pool.creator_fees_mint_a
    } else {
        0
    };
    let creator_fees_b = if pool.enable_creator_fee {
        pool.creator_fees_mint_b
    } else {
        0
    };
    Ok(NativeBonkCpmmPoolContext {
        pool_id: *pool_id,
        reserve_a: net_cpmm_reserve(
            read_spl_token_account_amount(vault_a_data)?,
            pool.protocol_fees_mint_a,
            pool.fund_fees_mint_a,
            creator_fees_a,
        ),
        reserve_b: net_cpmm_reserve(
            read_spl_token_account_amount(vault_b_data)?,
            pool.protocol_fees_mint_b,
            pool.fund_fees_mint_b,
            creator_fees_b,
        ),
        pool,
        config,
        quote,
    })
}

async fn load_bonk_cpmm_pool_context_by_pool_id(
    rpc_url: &str,
    pool_id_input: &str,
    quote_asset: &str,
    commitment: &str,
) -> Result<NativeBonkCpmmPoolContext, String> {
    let pool_id = Pubkey::from_str(pool_id_input)
        .map_err(|error| format!("Invalid Bonk CPMM pool id: {error}"))?;
    let pool_data = fetch_account_data(rpc_url, pool_id_input, commitment).await?;
    build_bonk_cpmm_pool_context_from_data(rpc_url, &pool_id, quote_asset, commitment, &pool_data)
        .await
}

async fn build_bonk_clmm_pool_context_from_data(
    rpc_url: &str,
    pool_id: &Pubkey,
    quote_asset: &str,
    commitment: &str,
    pool_data: &[u8],
) -> Result<NativeBonkClmmPoolContext, String> {
    let pool = decode_bonk_clmm_pool(pool_data)?;
    let quote = resolve_bonk_supported_quote_asset(quote_asset, &pool.mint_a, &pool.mint_b)?;
    let current_array_start =
        bonk_get_tick_array_start_index_by_tick(pool.tick_current, i32::from(pool.tick_spacing));
    let current_bit_position =
        bonk_tick_array_bit_position(current_array_start, i32::from(pool.tick_spacing))?;
    if !bonk_bitmap_is_initialized(&pool.tick_array_bitmap, current_bit_position) {
        return Err("Bonk CLMM current tick array is not initialized.".to_string());
    }
    let tick_count = BONK_CLMM_TICK_ARRAY_SIZE * i32::from(pool.tick_spacing);
    let initialized_bit_positions = (0..(pool.tick_array_bitmap.len() * 64))
        .filter(|bit_position| bonk_bitmap_is_initialized(&pool.tick_array_bitmap, *bit_position))
        .collect::<Vec<_>>();
    let tick_array_starts_desc = initialized_bit_positions
        .iter()
        .rev()
        .map(|bit_position| ((*bit_position as i32) - BONK_CLMM_DEFAULT_BITMAP_OFFSET) * tick_count)
        .collect::<Vec<_>>();
    let tick_array_starts_asc = initialized_bit_positions
        .iter()
        .map(|bit_position| ((*bit_position as i32) - BONK_CLMM_DEFAULT_BITMAP_OFFSET) * tick_count)
        .collect::<Vec<_>>();
    if tick_array_starts_desc.is_empty() {
        return Err("Bonk CLMM had no initialized tick arrays.".to_string());
    }
    let program_id = bonk_clmm_program_id()?;
    let tick_array_addresses = tick_array_starts_desc
        .iter()
        .map(|start_index| {
            bonk_derive_clmm_tick_array_address(&program_id, pool_id, *start_index).to_string()
        })
        .collect::<Vec<_>>();
    let tick_array_account_datas =
        rpc_get_multiple_accounts_data(rpc_url, &tick_array_addresses, commitment).await?;
    let tick_arrays = tick_array_account_datas
        .into_iter()
        .map(|data| decode_bonk_clmm_tick_array(&data))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|tick_array| (tick_array.start_tick_index, tick_array))
        .collect::<HashMap<_, _>>();
    if !tick_arrays.contains_key(&current_array_start) {
        return Err("Bonk CLMM current tick array could not be decoded.".to_string());
    }
    let config_data = fetch_account_data(rpc_url, &pool.amm_config.to_string(), commitment).await?;
    let config = decode_bonk_clmm_config(&config_data)?;
    if config.tick_spacing != pool.tick_spacing {
        return Err("Bonk CLMM tick spacing no longer matches its config.".to_string());
    }
    let (_, mint_a_owner) =
        fetch_account_data_with_owner(rpc_url, &pool.mint_a.to_string(), commitment).await?;
    let (_, mint_b_owner) =
        fetch_account_data_with_owner(rpc_url, &pool.mint_b.to_string(), commitment).await?;
    Ok(NativeBonkClmmPoolContext {
        quote,
        mint_program_a: Pubkey::from_str(&mint_a_owner)
            .map_err(|error| format!("Invalid Bonk CLMM mint A owner: {error}"))?,
        mint_program_b: Pubkey::from_str(&mint_b_owner)
            .map_err(|error| format!("Invalid Bonk CLMM mint B owner: {error}"))?,
        setup: BonkUsd1RouteSetup {
            pool_id: *pool_id,
            program_id,
            amm_config: pool.amm_config,
            mint_a: pool.mint_a,
            mint_b: pool.mint_b,
            vault_a: pool.vault_a,
            vault_b: pool.vault_b,
            observation_id: pool.observation_id,
            tick_spacing: i32::from(pool.tick_spacing),
            trade_fee_rate: config.trade_fee_rate,
            sqrt_price_x64: pool.sqrt_price_x64.clone(),
            liquidity: pool.liquidity.clone(),
            tick_current: pool.tick_current,
            mint_a_decimals: u32::from(pool.mint_decimals_a),
            mint_b_decimals: u32::from(pool.mint_decimals_b),
            current_price: bonk_sqrt_price_x64_to_price(
                &pool.sqrt_price_x64,
                u32::from(pool.mint_decimals_a),
                u32::from(pool.mint_decimals_b),
            )?,
            tick_arrays_desc: tick_array_starts_desc,
            tick_arrays_asc: tick_array_starts_asc,
            tick_arrays,
        },
    })
}

async fn load_bonk_clmm_pool_context_by_pool_id(
    rpc_url: &str,
    pool_id_input: &str,
    quote_asset: &str,
    commitment: &str,
) -> Result<NativeBonkClmmPoolContext, String> {
    let pool_id = Pubkey::from_str(pool_id_input)
        .map_err(|error| format!("Invalid Bonk CLMM pool id: {error}"))?;
    let pool_data = fetch_account_data(rpc_url, pool_id_input, commitment).await?;
    build_bonk_clmm_pool_context_from_data(rpc_url, &pool_id, quote_asset, commitment, &pool_data)
        .await
}

async fn load_bonk_trade_venue_context_by_pool_id(
    rpc_url: &str,
    pool_id_input: &str,
    quote_asset: &str,
    commitment: &str,
) -> Result<NativeBonkTradeVenueContext, String> {
    let pool_id = Pubkey::from_str(pool_id_input)
        .map_err(|error| format!("Invalid Bonk pool id: {error}"))?;
    let (pool_data, owner) =
        fetch_account_data_with_owner(rpc_url, pool_id_input, commitment).await?;
    let owner_pubkey =
        Pubkey::from_str(&owner).map_err(|error| format!("Invalid Bonk pool owner: {error}"))?;
    if owner_pubkey == bonk_launchpad_program_id()? {
        return Ok(NativeBonkTradeVenueContext::Launchpad(
            load_bonk_pool_context_by_pool_id(rpc_url, pool_id_input, quote_asset, commitment)
                .await?,
        ));
    }
    if owner_pubkey == bonk_cpmm_program_id()? {
        return Ok(NativeBonkTradeVenueContext::RaydiumCpmm(
            build_bonk_cpmm_pool_context_from_data(
                rpc_url,
                &pool_id,
                quote_asset,
                commitment,
                &pool_data,
            )
            .await?,
        ));
    }
    if owner_pubkey == bonk_clmm_program_id()? {
        return Ok(NativeBonkTradeVenueContext::RaydiumClmm(
            build_bonk_clmm_pool_context_from_data(
                rpc_url,
                &pool_id,
                quote_asset,
                commitment,
                &pool_data,
            )
            .await?,
        ));
    }
    Err(format!(
        "Unsupported Bonk pool owner {} for pool {}.",
        owner_pubkey, pool_id_input
    ))
}

async fn load_live_bonk_trade_venue_context(
    rpc_url: &str,
    mint: &Pubkey,
    quote_asset: &str,
    commitment: &str,
) -> Result<NativeBonkTradeVenueContext, String> {
    let import_context =
        native_detect_bonk_import_context_with_quote_asset(rpc_url, &mint.to_string(), quote_asset)
            .await?;
    if let Some(context) = import_context {
        if is_raydium_detection_source(&context.detectionSource) {
            return load_bonk_trade_venue_context_by_pool_id(
                rpc_url,
                &context.poolId,
                &context.quoteAsset,
                commitment,
            )
            .await;
        }
    }
    Ok(NativeBonkTradeVenueContext::Launchpad(
        load_live_bonk_pool_context(rpc_url, mint, quote_asset, commitment).await?,
    ))
}

fn read_spl_token_account_amount(data: &[u8]) -> Result<u64, String> {
    if data.len() < 72 {
        return Err("Token account data was shorter than expected.".to_string());
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&data[64..72]);
    Ok(u64::from_le_bytes(raw))
}

async fn fetch_bonk_owner_token_balance(
    rpc_url: &str,
    commitment: &str,
    owner: &Pubkey,
    mint: &Pubkey,
) -> Result<Option<u64>, String> {
    fetch_bonk_owner_token_balance_with_token_program(
        rpc_url,
        commitment,
        owner,
        mint,
        &spl_token::id(),
    )
    .await
}

async fn fetch_bonk_owner_token_balance_with_token_program(
    rpc_url: &str,
    commitment: &str,
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Result<Option<u64>, String> {
    let token_account = spl_associated_token_account::get_associated_token_address_with_program_id(
        owner,
        mint,
        token_program,
    );
    let data = match fetch_account_data(rpc_url, &token_account.to_string(), commitment).await {
        Ok(data) => data,
        Err(error) if error.contains("was not found.") => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(Some(read_spl_token_account_amount(&data)?))
}

async fn rpc_get_minimum_balance_for_rent_exemption(
    rpc_url: &str,
    commitment: &str,
    data_len: u64,
) -> Result<u64, String> {
    #[derive(Deserialize)]
    struct RentExemptionResponse {
        result: u64,
    }

    let payload = json!({
        "jsonrpc": "2.0",
        "id": "launchdeck-bonk-rent-exemption",
        "method": "getMinimumBalanceForRentExemption",
        "params": [
            data_len,
            {
                "commitment": commitment,
            }
        ]
    });
    let response = bonk_http_client()
        .post(rpc_url)
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("Failed to fetch Bonk rent exemption: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch Bonk rent exemption: RPC returned status {}.",
            response.status()
        ));
    }
    let parsed: RentExemptionResponse = response
        .json()
        .await
        .map_err(|error| format!("Failed to parse Bonk rent exemption response: {error}"))?;
    Ok(parsed.result)
}

fn bonk_follow_buy_amounts(
    pool_context: &NativeBonkPoolContext,
    requested_amount_b: u64,
    slippage_bps: u64,
) -> Result<(u64, u64), String> {
    let details = bonk_follow_buy_quote_details(pool_context, requested_amount_b, slippage_bps)?;
    Ok((details.gross_input_b, details.min_amount_a))
}

fn bonk_cpmm_fee_amount(amount: u64, fee_rate: u64) -> u64 {
    if fee_rate == 0 || amount == 0 {
        return 0;
    }
    let numerator = u128::from(amount).saturating_mul(u128::from(fee_rate))
        + u128::from(BONK_FEE_RATE_DENOMINATOR - 1);
    (numerator / u128::from(BONK_FEE_RATE_DENOMINATOR)) as u64
}

fn bonk_cpmm_quote_exact_input(
    pool_context: &NativeBonkCpmmPoolContext,
    input_mint: &Pubkey,
    amount_in: u64,
    slippage_bps: u64,
) -> Result<(u64, u64), String> {
    if amount_in == 0 {
        return Ok((0, 0));
    }
    let (input_reserve, output_reserve) = if *input_mint == pool_context.pool.token_0_mint {
        (pool_context.reserve_a, pool_context.reserve_b)
    } else if *input_mint == pool_context.pool.token_1_mint {
        (pool_context.reserve_b, pool_context.reserve_a)
    } else {
        return Err("Bonk CPMM quote input mint does not match the selected pool.".to_string());
    };
    if input_reserve == 0 || output_reserve == 0 {
        return Err("Bonk CPMM pool had zero reserves.".to_string());
    }
    let trade_fee = bonk_cpmm_fee_amount(amount_in, pool_context.config.trade_fee_rate);
    let input_after_trade_fee = amount_in.saturating_sub(trade_fee);
    let output_swapped = (u128::from(input_after_trade_fee) * u128::from(output_reserve))
        / u128::from(input_reserve.saturating_add(input_after_trade_fee));
    let creator_fee = if pool_context.pool.enable_creator_fee {
        bonk_cpmm_fee_amount(
            u64::try_from(output_swapped)
                .map_err(|error| format!("Bonk CPMM output exceeded u64: {error}"))?,
            pool_context.config.creator_fee_rate,
        )
    } else {
        0
    };
    let expected_out = u64::try_from(output_swapped)
        .map_err(|error| format!("Bonk CPMM output exceeded u64: {error}"))?
        .saturating_sub(creator_fee);
    let min_out = biguint_to_u64(
        &bonk_build_min_amount_from_bps(&bonk_biguint_from_u64(expected_out), slippage_bps),
        "Bonk CPMM min output",
    )?;
    Ok((expected_out, min_out))
}

fn bonk_quote_clmm_exact_input(
    setup: &BonkUsd1RouteSetup,
    input_mint: &Pubkey,
    input_amount: &BigUint,
    slippage_bps: u64,
) -> Result<BonkUsd1DirectQuote, String> {
    if *input_mint == setup.mint_a {
        bonk_quote_usd1_from_exact_sol_input(setup, input_amount, slippage_bps)
    } else if *input_mint == setup.mint_b {
        bonk_quote_sol_from_exact_usd1_input(setup, input_amount, slippage_bps)
    } else {
        Err("Bonk CLMM input mint does not match the selected pool.".to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BonkFollowBuyQuoteDetails {
    gross_input_b: u64,
    net_input_b: u64,
    amount_a: u64,
    min_amount_a: u64,
}

fn bonk_follow_buy_quote_details(
    pool_context: &NativeBonkPoolContext,
    requested_amount_b: u64,
    slippage_bps: u64,
) -> Result<BonkFollowBuyQuoteDetails, String> {
    let pool = build_bonk_follow_pool_state(&pool_context.pool);
    let fee_rate = bonk_total_fee_rate(
        &bonk_biguint_from_u64(pool_context.config.trade_fee_rate),
        &bonk_biguint_from_u64(pool_context.platform.fee_rate),
        &bonk_biguint_from_u64(pool_context.platform.creator_fee_rate),
    )?;
    let requested_amount_b_big = bonk_biguint_from_u64(requested_amount_b);
    let total_fee = bonk_calculate_fee(&requested_amount_b_big, &fee_rate);
    let amount_less_fee_b =
        bonk_big_sub(&requested_amount_b_big, &total_fee, "buy input after fee")?;
    let quoted_amount_a =
        bonk_curve_buy_exact_in(&pool, pool_context.config.curve_type, &amount_less_fee_b)?;
    let remaining_amount_a =
        bonk_big_sub(&pool.total_sell_a, &pool.real_a, "remaining sell amount")?;
    let (gross_input_b, net_input_b, amount_a) = if quoted_amount_a > remaining_amount_a {
        let capped_net_input_b =
            bonk_curve_buy_exact_out(&pool, pool_context.config.curve_type, &remaining_amount_a)?;
        let gross_input_b = bonk_calculate_pre_fee(&capped_net_input_b, &fee_rate)?;
        (gross_input_b, capped_net_input_b, remaining_amount_a)
    } else {
        (requested_amount_b_big, amount_less_fee_b, quoted_amount_a)
    };
    let min_amount_a = bonk_build_min_amount_from_bps(&amount_a, slippage_bps);
    Ok(BonkFollowBuyQuoteDetails {
        gross_input_b: biguint_to_u64(&gross_input_b, "follow buy spend amount")?,
        net_input_b: biguint_to_u64(&net_input_b, "follow buy pool input amount")?,
        amount_a: biguint_to_u64(&amount_a, "follow buy quoted output")?,
        min_amount_a: biguint_to_u64(&min_amount_a, "follow buy min output")?,
    })
}

fn advance_prelaunch_bonk_pool_context_after_buy(
    pool_context: &NativeBonkPoolContext,
    requested_amount_b: u64,
    slippage_bps: u64,
) -> Result<NativeBonkPoolContext, String> {
    let details = bonk_follow_buy_quote_details(pool_context, requested_amount_b, slippage_bps)?;
    let mut next = pool_context.clone();
    next.pool.real_a = next.pool.real_a.saturating_add(details.amount_a);
    next.pool.real_b = next.pool.real_b.saturating_add(details.net_input_b);
    Ok(next)
}

fn bonk_follow_sell_amounts(
    pool_context: &NativeBonkPoolContext,
    sell_amount_a: u64,
    slippage_bps: u64,
) -> Result<u64, String> {
    Ok(bonk_follow_sell_quote_amounts(pool_context, sell_amount_a, slippage_bps)?.1)
}

fn bonk_follow_sell_quote_amounts(
    pool_context: &NativeBonkPoolContext,
    sell_amount_a: u64,
    slippage_bps: u64,
) -> Result<(u64, u64), String> {
    let pool = build_bonk_follow_pool_state(&pool_context.pool);
    let quoted_amount_b = bonk_quote_sell_exact_in_amount_b(
        &pool,
        pool_context.config.curve_type,
        &bonk_biguint_from_u64(pool_context.config.trade_fee_rate),
        &bonk_biguint_from_u64(pool_context.platform.fee_rate),
        &bonk_biguint_from_u64(pool_context.platform.creator_fee_rate),
        &bonk_biguint_from_u64(sell_amount_a),
    )?;
    let expected_amount_b = biguint_to_u64(&quoted_amount_b, "follow sell expected output")?;
    let min_amount_b = bonk_build_min_amount_from_bps(&quoted_amount_b, slippage_bps);
    Ok((
        expected_amount_b,
        biguint_to_u64(&min_amount_b, "follow sell min output")?,
    ))
}

async fn native_compile_bonk_buy_transaction_with_pool_context(
    rpc_url: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    owner: &Keypair,
    mint_pubkey: &Pubkey,
    pool_context: &NativeBonkPoolContext,
    requested_amount_b: u64,
    allow_ata_creation: bool,
    tx_format: NativeBonkTxFormat,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let owner_pubkey = owner.pubkey();
    let slippage_bps = slippage_bps_from_percent(&execution.buySlippagePercent)?;
    let token_fee_bps = wrapper_fee_bps;
    let token_fee_route = pool_context.quote.asset == "usd1" && token_fee_bps > 0;
    let venue_requested_amount_b = if token_fee_route {
        requested_amount_b
            .checked_sub(estimate_sol_in_fee_lamports(
                requested_amount_b,
                token_fee_bps,
            ))
            .ok_or_else(|| "Bonk USD1 token-fee buy amount underflowed".to_string())?
    } else {
        requested_amount_b
    };
    let (instruction_amount_b, min_amount_a) =
        bonk_follow_buy_amounts(pool_context, venue_requested_amount_b, slippage_bps)?;
    let tip_lamports =
        resolve_follow_tip_lamports(&execution.buyProvider, &execution.buyTipSol, "buy tip")?;
    let tx_config = bonk_follow_tx_config(
        configured_default_bonk_launchpad_buy_compute_unit_limit(),
        priority_fee_sol_to_micro_lamports(&execution.buyPriorityFeeSol)?,
        tip_lamports,
        jito_tip_account,
    )?;
    let token_program = pool_context.token_program;
    let quote_token_program = spl_token::id();
    let user_token_account_a =
        spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            mint_pubkey,
            &token_program,
        );
    let mut instructions = vec![
        spl_associated_token_account::instruction::create_associated_token_account_idempotent(
            &owner_pubkey,
            &owner_pubkey,
            mint_pubkey,
            &token_program,
        ),
    ];
    let mut extra_signers = Vec::new();
    let user_token_account_b = if pool_context.quote.asset == "sol" {
        let wrapped_signer = Keypair::new();
        let rent_exempt_lamports = rpc_get_minimum_balance_for_rent_exemption(
            rpc_url,
            &execution.commitment,
            BONK_SPL_TOKEN_ACCOUNT_LEN,
        )
        .await?;
        instructions.extend(build_bonk_wrapped_sol_open_instructions(
            &owner_pubkey,
            &wrapped_signer.pubkey(),
            rent_exempt_lamports.saturating_add(requested_amount_b),
        )?);
        extra_signers.push(wrapped_signer);
        extra_signers.last().expect("wrapped SOL signer").pubkey()
    } else {
        let quote_mint = bonk_quote_mint(pool_context.quote.asset)?;
        let quote_ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            &quote_mint,
            &quote_token_program,
        );
        if allow_ata_creation {
            instructions.push(
                spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                    &owner_pubkey,
                    &owner_pubkey,
                    &quote_mint,
                    &quote_token_program,
                ),
            );
        }
        quote_ata
    };
    let buy_ix = build_bonk_buy_exact_in_instruction(
        &owner_pubkey,
        pool_context,
        &user_token_account_a,
        &user_token_account_b,
        instruction_amount_b,
        min_amount_a,
    )?;
    if token_fee_route {
        instructions.push(build_bonk_token_fee_route_instruction(
            &owner_pubkey,
            token_fee_bps,
            &bonk_quote_mint(pool_context.quote.asset)?,
            &quote_token_program,
            requested_amount_b,
            min_amount_a,
            SwapRouteDirection::Buy,
            SwapRouteFeeMode::TokenPre,
            buy_ix,
            &user_token_account_b,
            &user_token_account_a,
        )?);
    } else {
        instructions.push(buy_ix);
    }
    if pool_context.quote.asset == "sol" {
        instructions.push(build_bonk_wrapped_sol_close_instruction(
            &owner_pubkey,
            &user_token_account_b,
        )?);
    }
    let tx_instructions = with_bonk_tx_settings(
        instructions,
        &tx_config,
        &owner_pubkey,
        execution.buyJitodontfront,
    )?;
    let extra_signer_refs = extra_signers.iter().collect::<Vec<_>>();
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, &execution.commitment).await?;
    let preferred_lookup_tables = if tx_format == NativeBonkTxFormat::V0 {
        load_bonk_preferred_usd1_lookup_tables(rpc_url, &execution.commitment).await?
    } else {
        vec![]
    };
    build_bonk_compiled_transaction_with_lookup_preference(
        "follow-buy",
        tx_format,
        &blockhash,
        last_valid_block_height,
        owner,
        &extra_signer_refs,
        tx_instructions,
        &tx_config,
        &[],
        &preferred_lookup_tables,
    )
}

async fn native_compile_bonk_buy_transaction_with_cpmm_context(
    rpc_url: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    owner: &Keypair,
    mint_pubkey: &Pubkey,
    pool_context: &NativeBonkCpmmPoolContext,
    requested_amount_b: u64,
    allow_ata_creation: bool,
    tx_format: NativeBonkTxFormat,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let owner_pubkey = owner.pubkey();
    let slippage_bps = slippage_bps_from_percent(&execution.buySlippagePercent)?;
    let tip_lamports =
        resolve_follow_tip_lamports(&execution.buyProvider, &execution.buyTipSol, "buy tip")?;
    let tx_config = bonk_follow_tx_config(
        configured_default_sniper_buy_compute_unit_limit(),
        priority_fee_sol_to_micro_lamports(&execution.buyPriorityFeeSol)?,
        tip_lamports,
        jito_tip_account,
    )?;
    let quote_mint = bonk_quote_mint(pool_context.quote.asset)?;
    let token_fee_bps = wrapper_fee_bps;
    let token_fee_route = pool_context.quote.asset == "usd1" && token_fee_bps > 0;
    let venue_requested_amount_b = if token_fee_route {
        requested_amount_b
            .checked_sub(estimate_sol_in_fee_lamports(
                requested_amount_b,
                token_fee_bps,
            ))
            .ok_or_else(|| "Bonk CPMM USD1 token-fee buy amount underflowed".to_string())?
    } else {
        requested_amount_b
    };
    let (token_mint, token_program, quote_program) = if pool_context.pool.token_0_mint == quote_mint
    {
        (
            pool_context.pool.token_1_mint,
            pool_context.pool.token_1_program,
            pool_context.pool.token_0_program,
        )
    } else if pool_context.pool.token_1_mint == quote_mint {
        (
            pool_context.pool.token_0_mint,
            pool_context.pool.token_0_program,
            pool_context.pool.token_1_program,
        )
    } else {
        return Err("Bonk CPMM quote mint did not match the selected pool.".to_string());
    };
    if *mint_pubkey != token_mint {
        return Err("Bonk CPMM token mint did not match the selected pool.".to_string());
    }
    let user_token_account_a =
        spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            mint_pubkey,
            &token_program,
        );
    let mut instructions = Vec::new();
    if allow_ata_creation {
        instructions.push(build_bonk_create_ata_instruction(
            &owner_pubkey,
            mint_pubkey,
            &token_program,
        ));
    }
    let mut extra_signers = Vec::new();
    let user_quote_account = if pool_context.quote.asset == "sol" {
        let wrapped_signer = Keypair::new();
        let rent_exempt_lamports = rpc_get_minimum_balance_for_rent_exemption(
            rpc_url,
            &execution.commitment,
            BONK_SPL_TOKEN_ACCOUNT_LEN,
        )
        .await?;
        instructions.extend(build_bonk_wrapped_sol_open_instructions(
            &owner_pubkey,
            &wrapped_signer.pubkey(),
            rent_exempt_lamports.saturating_add(requested_amount_b),
        )?);
        extra_signers.push(wrapped_signer);
        extra_signers.last().expect("wrapped SOL signer").pubkey()
    } else {
        let quote_ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            &quote_mint,
            &quote_program,
        );
        if allow_ata_creation {
            instructions.push(build_bonk_create_ata_instruction(
                &owner_pubkey,
                &quote_mint,
                &quote_program,
            ));
        }
        quote_ata
    };
    let (_, min_amount_a) = bonk_cpmm_quote_exact_input(
        pool_context,
        &quote_mint,
        venue_requested_amount_b,
        slippage_bps,
    )?;
    let buy_ix = build_bonk_cpmm_swap_exact_in_instruction(
        &owner_pubkey,
        pool_context,
        &user_quote_account,
        &user_token_account_a,
        venue_requested_amount_b,
        min_amount_a,
        &quote_mint,
        mint_pubkey,
    )?;
    if token_fee_route {
        instructions.push(build_bonk_token_fee_route_instruction(
            &owner_pubkey,
            token_fee_bps,
            &quote_mint,
            &quote_program,
            requested_amount_b,
            min_amount_a,
            SwapRouteDirection::Buy,
            SwapRouteFeeMode::TokenPre,
            buy_ix,
            &user_quote_account,
            &user_token_account_a,
        )?);
    } else {
        instructions.push(buy_ix);
    }
    if pool_context.quote.asset == "sol" {
        instructions.push(build_bonk_wrapped_sol_close_instruction(
            &owner_pubkey,
            &user_quote_account,
        )?);
    }
    let tx_instructions = with_bonk_tx_settings(
        instructions,
        &tx_config,
        &owner_pubkey,
        execution.buyJitodontfront,
    )?;
    let extra_signer_refs = extra_signers.iter().collect::<Vec<_>>();
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, &execution.commitment).await?;
    let preferred_lookup_tables = if tx_format == NativeBonkTxFormat::V0 {
        load_bonk_preferred_usd1_lookup_tables(rpc_url, &execution.commitment).await?
    } else {
        vec![]
    };
    build_bonk_compiled_transaction_with_lookup_preference(
        "follow-buy",
        tx_format,
        &blockhash,
        last_valid_block_height,
        owner,
        &extra_signer_refs,
        tx_instructions,
        &tx_config,
        &[],
        &preferred_lookup_tables,
    )
}

async fn native_compile_bonk_buy_transaction_with_clmm_context(
    rpc_url: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    owner: &Keypair,
    mint_pubkey: &Pubkey,
    pool_context: &NativeBonkClmmPoolContext,
    requested_amount_b: u64,
    allow_ata_creation: bool,
    tx_format: NativeBonkTxFormat,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let owner_pubkey = owner.pubkey();
    let slippage_bps = slippage_bps_from_percent(&execution.buySlippagePercent)?;
    let tip_lamports =
        resolve_follow_tip_lamports(&execution.buyProvider, &execution.buyTipSol, "buy tip")?;
    let tx_config = bonk_follow_tx_config(
        configured_default_sniper_buy_compute_unit_limit(),
        priority_fee_sol_to_micro_lamports(&execution.buyPriorityFeeSol)?,
        tip_lamports,
        jito_tip_account,
    )?;
    let quote_mint = bonk_quote_mint(pool_context.quote.asset)?;
    let token_fee_bps = wrapper_fee_bps;
    let token_fee_route = pool_context.quote.asset == "usd1" && token_fee_bps > 0;
    let venue_requested_amount_b = if token_fee_route {
        requested_amount_b
            .checked_sub(estimate_sol_in_fee_lamports(
                requested_amount_b,
                token_fee_bps,
            ))
            .ok_or_else(|| "Bonk CLMM USD1 token-fee buy amount underflowed".to_string())?
    } else {
        requested_amount_b
    };
    let (token_program, quote_program) = if pool_context.setup.mint_a == quote_mint {
        (pool_context.mint_program_b, pool_context.mint_program_a)
    } else if pool_context.setup.mint_b == quote_mint {
        (pool_context.mint_program_a, pool_context.mint_program_b)
    } else {
        return Err("Bonk CLMM quote mint did not match the selected pool.".to_string());
    };
    let token_mint = if pool_context.setup.mint_a == quote_mint {
        pool_context.setup.mint_b
    } else {
        pool_context.setup.mint_a
    };
    if *mint_pubkey != token_mint {
        return Err("Bonk CLMM token mint did not match the selected pool.".to_string());
    }
    let user_token_account_a =
        spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            mint_pubkey,
            &token_program,
        );
    let mut instructions = Vec::new();
    if allow_ata_creation {
        instructions.push(build_bonk_create_ata_instruction(
            &owner_pubkey,
            mint_pubkey,
            &token_program,
        ));
    }
    let mut extra_signers = Vec::new();
    let user_quote_account = if pool_context.quote.asset == "sol" {
        let wrapped_signer = Keypair::new();
        let rent_exempt_lamports = rpc_get_minimum_balance_for_rent_exemption(
            rpc_url,
            &execution.commitment,
            BONK_SPL_TOKEN_ACCOUNT_LEN,
        )
        .await?;
        instructions.extend(build_bonk_wrapped_sol_open_instructions(
            &owner_pubkey,
            &wrapped_signer.pubkey(),
            rent_exempt_lamports.saturating_add(requested_amount_b),
        )?);
        extra_signers.push(wrapped_signer);
        extra_signers.last().expect("wrapped SOL signer").pubkey()
    } else {
        let quote_ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            &quote_mint,
            &quote_program,
        );
        if allow_ata_creation {
            instructions.push(build_bonk_create_ata_instruction(
                &owner_pubkey,
                &quote_mint,
                &quote_program,
            ));
        }
        quote_ata
    };
    let quote = bonk_quote_clmm_exact_input(
        &pool_context.setup,
        &quote_mint,
        &bonk_biguint_from_u64(venue_requested_amount_b),
        slippage_bps,
    )?;
    let min_amount_a = biguint_to_u64(&quote.min_out, "Bonk CLMM buy min output")?;
    let buy_ix = build_bonk_clmm_swap_exact_in_instruction_for_setup(
        &owner_pubkey,
        &pool_context.setup,
        &user_quote_account,
        &user_token_account_a,
        venue_requested_amount_b,
        min_amount_a,
        &quote.traversed_tick_array_starts,
        &quote_mint,
        mint_pubkey,
    )?;
    if token_fee_route {
        instructions.push(build_bonk_token_fee_route_instruction(
            &owner_pubkey,
            token_fee_bps,
            &quote_mint,
            &quote_program,
            requested_amount_b,
            min_amount_a,
            SwapRouteDirection::Buy,
            SwapRouteFeeMode::TokenPre,
            buy_ix,
            &user_quote_account,
            &user_token_account_a,
        )?);
    } else {
        instructions.push(buy_ix);
    }
    if pool_context.quote.asset == "sol" {
        instructions.push(build_bonk_wrapped_sol_close_instruction(
            &owner_pubkey,
            &user_quote_account,
        )?);
    }
    let tx_instructions = with_bonk_tx_settings(
        instructions,
        &tx_config,
        &owner_pubkey,
        execution.buyJitodontfront,
    )?;
    let extra_signer_refs = extra_signers.iter().collect::<Vec<_>>();
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, &execution.commitment).await?;
    let preferred_lookup_tables = if tx_format == NativeBonkTxFormat::V0 {
        load_bonk_preferred_usd1_lookup_tables(rpc_url, &execution.commitment).await?
    } else {
        vec![]
    };
    build_bonk_compiled_transaction_with_lookup_preference(
        "follow-buy",
        tx_format,
        &blockhash,
        last_valid_block_height,
        owner,
        &extra_signer_refs,
        tx_instructions,
        &tx_config,
        &[],
        &preferred_lookup_tables,
    )
}

async fn native_compile_bonk_buy_transaction_with_venue_context(
    rpc_url: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    owner: &Keypair,
    mint_pubkey: &Pubkey,
    venue_context: &NativeBonkTradeVenueContext,
    requested_amount_b: u64,
    allow_ata_creation: bool,
    tx_format: NativeBonkTxFormat,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    match venue_context {
        NativeBonkTradeVenueContext::Launchpad(context) => {
            native_compile_bonk_buy_transaction_with_pool_context(
                rpc_url,
                execution,
                jito_tip_account,
                owner,
                mint_pubkey,
                context,
                requested_amount_b,
                allow_ata_creation,
                tx_format,
                wrapper_fee_bps,
            )
            .await
        }
        NativeBonkTradeVenueContext::RaydiumCpmm(context) => {
            native_compile_bonk_buy_transaction_with_cpmm_context(
                rpc_url,
                execution,
                jito_tip_account,
                owner,
                mint_pubkey,
                context,
                requested_amount_b,
                allow_ata_creation,
                tx_format,
                wrapper_fee_bps,
            )
            .await
        }
        NativeBonkTradeVenueContext::RaydiumClmm(context) => {
            native_compile_bonk_buy_transaction_with_clmm_context(
                rpc_url,
                execution,
                jito_tip_account,
                owner,
                mint_pubkey,
                context,
                requested_amount_b,
                allow_ata_creation,
                tx_format,
                wrapper_fee_bps,
            )
            .await
        }
    }
}

async fn native_compile_bonk_sell_transaction_with_cpmm_context(
    rpc_url: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    owner: &Keypair,
    mint_pubkey: &Pubkey,
    pool_context: &NativeBonkCpmmPoolContext,
    sell_amount: u64,
    sell_settlement_asset: &str,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let owner_pubkey = owner.pubkey();
    let slippage_bps = slippage_bps_from_percent(&execution.sellSlippagePercent)?;
    let tip_lamports =
        resolve_follow_tip_lamports(&execution.sellProvider, &execution.sellTipSol, "sell tip")?;
    let tx_config = bonk_follow_tx_config(
        configured_bonk_sell_compute_unit_limit(&pool_context.quote.asset, sell_settlement_asset),
        priority_fee_sol_to_micro_lamports(&execution.sellPriorityFeeSol)?,
        tip_lamports,
        jito_tip_account,
    )?;
    let quote_mint = bonk_quote_mint(pool_context.quote.asset)?;
    let (token_program, quote_program) = if pool_context.pool.token_0_mint == quote_mint {
        (
            pool_context.pool.token_1_program,
            pool_context.pool.token_0_program,
        )
    } else if pool_context.pool.token_1_mint == quote_mint {
        (
            pool_context.pool.token_0_program,
            pool_context.pool.token_1_program,
        )
    } else {
        return Err("Bonk CPMM quote mint did not match the selected pool.".to_string());
    };
    let user_token_account_a =
        spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            mint_pubkey,
            &token_program,
        );
    let mut instructions = Vec::new();
    let mut extra_signers = Vec::new();
    let user_quote_account = if pool_context.quote.asset == "sol" {
        let wrapped_signer = Keypair::new();
        let rent_exempt_lamports = rpc_get_minimum_balance_for_rent_exemption(
            rpc_url,
            &execution.commitment,
            BONK_SPL_TOKEN_ACCOUNT_LEN,
        )
        .await?;
        instructions.extend(build_bonk_wrapped_sol_open_instructions(
            &owner_pubkey,
            &wrapped_signer.pubkey(),
            rent_exempt_lamports,
        )?);
        extra_signers.push(wrapped_signer);
        extra_signers.last().expect("wrapped SOL signer").pubkey()
    } else {
        let quote_ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            &quote_mint,
            &quote_program,
        );
        instructions.push(build_bonk_create_ata_instruction(
            &owner_pubkey,
            &quote_mint,
            &quote_program,
        ));
        quote_ata
    };
    let (expected_amount_b, min_amount_b) =
        bonk_cpmm_quote_exact_input(pool_context, mint_pubkey, sell_amount, slippage_bps)?;
    let sell_ix = build_bonk_cpmm_swap_exact_in_instruction(
        &owner_pubkey,
        pool_context,
        &user_token_account_a,
        &user_quote_account,
        sell_amount,
        min_amount_b,
        mint_pubkey,
        &quote_mint,
    )?;
    let settlement_asset = normalize_bonk_sell_settlement_asset(sell_settlement_asset);
    if pool_context.quote.asset == "usd1" && settlement_asset == "sol" {
        let route_setup = load_bonk_usd1_route_setup(rpc_url).await?;
        let route_instructions = build_bonk_dynamic_usd1_sell_to_sol_route(
            &owner_pubkey,
            &user_quote_account,
            sell_ix,
            sell_amount,
            min_amount_b,
            expected_amount_b,
            &route_setup,
            wrapper_fee_bps,
        )?;
        instructions.extend(route_instructions);
    } else if pool_context.quote.asset == "sol" {
        instructions.push(sell_ix);
        instructions.push(build_bonk_wrapped_sol_close_instruction(
            &owner_pubkey,
            &user_quote_account,
        )?);
    } else if pool_context.quote.asset == "usd1" {
        let min_net_amount_b = min_amount_b
            .checked_sub(estimate_sol_in_fee_lamports(min_amount_b, wrapper_fee_bps))
            .ok_or_else(|| "Bonk CPMM USD1 token-fee sell min output underflowed".to_string())?;
        instructions.push(build_bonk_token_fee_route_instruction(
            &owner_pubkey,
            wrapper_fee_bps,
            &quote_mint,
            &quote_program,
            0,
            min_net_amount_b,
            SwapRouteDirection::Sell,
            SwapRouteFeeMode::TokenPost,
            sell_ix,
            &user_quote_account,
            &user_quote_account,
        )?);
    } else {
        instructions.push(sell_ix);
    }
    let tx_instructions = with_bonk_tx_settings(
        instructions,
        &tx_config,
        &owner_pubkey,
        execution.sellJitodontfront,
    )?;
    let extra_signer_refs = extra_signers.iter().collect::<Vec<_>>();
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, &execution.commitment).await?;
    let preferred_lookup_tables =
        load_bonk_preferred_usd1_lookup_tables(rpc_url, &execution.commitment).await?;
    build_bonk_compiled_transaction_with_lookup_preference(
        "follow-sell",
        select_bonk_native_tx_format(&execution.txFormat),
        &blockhash,
        last_valid_block_height,
        owner,
        &extra_signer_refs,
        tx_instructions,
        &tx_config,
        &[],
        &preferred_lookup_tables,
    )
}

async fn native_compile_bonk_sell_transaction_with_clmm_context(
    rpc_url: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    owner: &Keypair,
    mint_pubkey: &Pubkey,
    pool_context: &NativeBonkClmmPoolContext,
    sell_amount: u64,
    sell_settlement_asset: &str,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let owner_pubkey = owner.pubkey();
    let slippage_bps = slippage_bps_from_percent(&execution.sellSlippagePercent)?;
    let tip_lamports =
        resolve_follow_tip_lamports(&execution.sellProvider, &execution.sellTipSol, "sell tip")?;
    let tx_config = bonk_follow_tx_config(
        configured_bonk_sell_compute_unit_limit(&pool_context.quote.asset, sell_settlement_asset),
        priority_fee_sol_to_micro_lamports(&execution.sellPriorityFeeSol)?,
        tip_lamports,
        jito_tip_account,
    )?;
    let quote_mint = bonk_quote_mint(pool_context.quote.asset)?;
    let (token_program, quote_program) = if pool_context.setup.mint_a == quote_mint {
        (pool_context.mint_program_b, pool_context.mint_program_a)
    } else if pool_context.setup.mint_b == quote_mint {
        (pool_context.mint_program_a, pool_context.mint_program_b)
    } else {
        return Err("Bonk CLMM quote mint did not match the selected pool.".to_string());
    };
    let user_token_account_a =
        spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            mint_pubkey,
            &token_program,
        );
    let mut instructions = Vec::new();
    let mut extra_signers = Vec::new();
    let user_quote_account = if pool_context.quote.asset == "sol" {
        let wrapped_signer = Keypair::new();
        let rent_exempt_lamports = rpc_get_minimum_balance_for_rent_exemption(
            rpc_url,
            &execution.commitment,
            BONK_SPL_TOKEN_ACCOUNT_LEN,
        )
        .await?;
        instructions.extend(build_bonk_wrapped_sol_open_instructions(
            &owner_pubkey,
            &wrapped_signer.pubkey(),
            rent_exempt_lamports,
        )?);
        extra_signers.push(wrapped_signer);
        extra_signers.last().expect("wrapped SOL signer").pubkey()
    } else {
        let quote_ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            &quote_mint,
            &quote_program,
        );
        instructions.push(build_bonk_create_ata_instruction(
            &owner_pubkey,
            &quote_mint,
            &quote_program,
        ));
        quote_ata
    };
    let quote = bonk_quote_clmm_exact_input(
        &pool_context.setup,
        mint_pubkey,
        &bonk_biguint_from_u64(sell_amount),
        slippage_bps,
    )?;
    let sell_ix = build_bonk_clmm_swap_exact_in_instruction_for_setup(
        &owner_pubkey,
        &pool_context.setup,
        &user_token_account_a,
        &user_quote_account,
        sell_amount,
        biguint_to_u64(&quote.min_out, "Bonk CLMM sell min output")?,
        &quote.traversed_tick_array_starts,
        mint_pubkey,
        &quote_mint,
    )?;
    let settlement_asset = normalize_bonk_sell_settlement_asset(sell_settlement_asset);
    if pool_context.quote.asset == "usd1" && settlement_asset == "sol" {
        let route_setup = load_bonk_usd1_route_setup(rpc_url).await?;
        let unwind_input = biguint_to_u64(&quote.min_out, "Bonk CLMM sell minimum output")?;
        let expected_unwind_input =
            biguint_to_u64(&quote.expected_out, "Bonk CLMM sell expected output")?;
        let route_instructions = build_bonk_dynamic_usd1_sell_to_sol_route(
            &owner_pubkey,
            &user_quote_account,
            sell_ix,
            sell_amount,
            unwind_input,
            expected_unwind_input,
            &route_setup,
            wrapper_fee_bps,
        )?;
        instructions.extend(route_instructions);
    } else if pool_context.quote.asset == "sol" {
        instructions.push(sell_ix);
        instructions.push(build_bonk_wrapped_sol_close_instruction(
            &owner_pubkey,
            &user_quote_account,
        )?);
    } else if pool_context.quote.asset == "usd1" {
        let min_amount_b = biguint_to_u64(&quote.min_out, "Bonk CLMM sell min output")?;
        let min_net_amount_b = min_amount_b
            .checked_sub(estimate_sol_in_fee_lamports(min_amount_b, wrapper_fee_bps))
            .ok_or_else(|| "Bonk CLMM USD1 token-fee sell min output underflowed".to_string())?;
        instructions.push(build_bonk_token_fee_route_instruction(
            &owner_pubkey,
            wrapper_fee_bps,
            &quote_mint,
            &quote_program,
            0,
            min_net_amount_b,
            SwapRouteDirection::Sell,
            SwapRouteFeeMode::TokenPost,
            sell_ix,
            &user_quote_account,
            &user_quote_account,
        )?);
    } else {
        instructions.push(sell_ix);
    }
    let tx_instructions = with_bonk_tx_settings(
        instructions,
        &tx_config,
        &owner_pubkey,
        execution.sellJitodontfront,
    )?;
    let extra_signer_refs = extra_signers.iter().collect::<Vec<_>>();
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, &execution.commitment).await?;
    let preferred_lookup_tables =
        load_bonk_preferred_usd1_lookup_tables(rpc_url, &execution.commitment).await?;
    build_bonk_compiled_transaction_with_lookup_preference(
        "follow-sell",
        select_bonk_native_tx_format(&execution.txFormat),
        &blockhash,
        last_valid_block_height,
        owner,
        &extra_signer_refs,
        tx_instructions,
        &tx_config,
        &[],
        &preferred_lookup_tables,
    )
}

async fn native_compile_follow_sell_launchpad_transaction(
    rpc_url: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    owner: &Keypair,
    mint_pubkey: &Pubkey,
    pool_context: &NativeBonkPoolContext,
    sell_amount: u64,
    sell_settlement_asset: &str,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let owner_pubkey = owner.pubkey();
    let slippage_bps = slippage_bps_from_percent(&execution.sellSlippagePercent)?;
    let (expected_amount_b, min_amount_b) =
        bonk_follow_sell_quote_amounts(pool_context, sell_amount, slippage_bps)?;
    let tip_lamports =
        resolve_follow_tip_lamports(&execution.sellProvider, &execution.sellTipSol, "sell tip")?;
    let tx_config = bonk_follow_tx_config(
        configured_bonk_sell_compute_unit_limit(&pool_context.quote.asset, sell_settlement_asset),
        priority_fee_sol_to_micro_lamports(&execution.sellPriorityFeeSol)?,
        tip_lamports,
        jito_tip_account,
    )?;
    let tx_format = select_bonk_native_tx_format(&execution.txFormat);
    let token_program = pool_context.token_program;
    let quote_token_program = spl_token::id();
    let settlement_asset = normalize_bonk_sell_settlement_asset(sell_settlement_asset);
    let user_token_account_a =
        spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            mint_pubkey,
            &token_program,
        );
    let mut instructions = Vec::new();
    let mut extra_signers = Vec::new();
    let user_token_account_b = if pool_context.quote.asset == "sol" {
        let wrapped_signer = Keypair::new();
        let rent_exempt_lamports = rpc_get_minimum_balance_for_rent_exemption(
            rpc_url,
            &execution.commitment,
            BONK_SPL_TOKEN_ACCOUNT_LEN,
        )
        .await?;
        instructions.extend(build_bonk_wrapped_sol_open_instructions(
            &owner_pubkey,
            &wrapped_signer.pubkey(),
            rent_exempt_lamports,
        )?);
        extra_signers.push(wrapped_signer);
        extra_signers.last().expect("wrapped SOL signer").pubkey()
    } else {
        let quote_mint = bonk_quote_mint(pool_context.quote.asset)?;
        let quote_ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            &quote_mint,
            &quote_token_program,
        );
        instructions.push(build_bonk_create_ata_instruction(
            &owner_pubkey,
            &quote_mint,
            &quote_token_program,
        ));
        quote_ata
    };
    let sell_ix = build_bonk_sell_exact_in_instruction(
        &owner_pubkey,
        pool_context,
        &user_token_account_a,
        &user_token_account_b,
        sell_amount,
        min_amount_b,
    )?;
    if pool_context.quote.asset == "usd1" && settlement_asset == "sol" {
        let route_setup = load_bonk_usd1_route_setup(rpc_url).await?;
        let route_instructions = build_bonk_dynamic_usd1_sell_to_sol_route(
            &owner_pubkey,
            &user_token_account_b,
            sell_ix,
            sell_amount,
            min_amount_b,
            expected_amount_b,
            &route_setup,
            wrapper_fee_bps,
        )?;
        instructions.extend(route_instructions);
    } else if pool_context.quote.asset == "sol" {
        instructions.push(sell_ix);
        instructions.push(build_bonk_wrapped_sol_close_instruction(
            &owner_pubkey,
            &user_token_account_b,
        )?);
    } else if pool_context.quote.asset == "usd1" {
        let min_net_amount_b = min_amount_b
            .checked_sub(estimate_sol_in_fee_lamports(min_amount_b, wrapper_fee_bps))
            .ok_or_else(|| {
                "Bonk launchpad USD1 token-fee sell min output underflowed".to_string()
            })?;
        instructions.push(build_bonk_token_fee_route_instruction(
            &owner_pubkey,
            wrapper_fee_bps,
            &bonk_quote_mint(pool_context.quote.asset)?,
            &quote_token_program,
            0,
            min_net_amount_b,
            SwapRouteDirection::Sell,
            SwapRouteFeeMode::TokenPost,
            sell_ix,
            &user_token_account_b,
            &user_token_account_b,
        )?);
    } else {
        instructions.push(sell_ix);
    }
    let tx_instructions = with_bonk_tx_settings(
        instructions,
        &tx_config,
        &owner_pubkey,
        execution.sellJitodontfront,
    )?;
    let extra_signer_refs = extra_signers.iter().collect::<Vec<_>>();
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, &execution.commitment).await?;
    let preferred_lookup_tables = if tx_format == NativeBonkTxFormat::V0 {
        load_bonk_preferred_usd1_lookup_tables(rpc_url, &execution.commitment).await?
    } else {
        vec![]
    };
    build_bonk_compiled_transaction_with_lookup_preference(
        "follow-sell",
        tx_format,
        &blockhash,
        last_valid_block_height,
        owner,
        &extra_signer_refs,
        tx_instructions,
        &tx_config,
        &[],
        &preferred_lookup_tables,
    )
}

async fn combine_atomic_bonk_usd1_follow_buy_transactions(
    rpc_url: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    owner: &Keypair,
    topup_transaction: &CompiledTransaction,
    action_transaction: &CompiledTransaction,
) -> Result<CompiledTransaction, String> {
    let tip_lamports =
        resolve_follow_tip_lamports(&execution.buyProvider, &execution.buyTipSol, "buy tip")?;
    let tx_config = bonk_follow_tx_config(
        configured_atomic_bonk_usd1_follow_buy_compute_unit_limit(
            topup_transaction.computeUnitLimit,
            action_transaction.computeUnitLimit,
        ),
        priority_fee_sol_to_micro_lamports(&execution.buyPriorityFeeSol)?,
        tip_lamports,
        jito_tip_account,
    )?;
    combine_atomic_bonk_transactions(
        rpc_url,
        &execution.commitment,
        owner,
        "follow-buy-atomic",
        &tx_config,
        execution.buyJitodontfront,
        &[],
        topup_transaction,
        action_transaction,
    )
    .await
}

fn configured_atomic_bonk_usd1_follow_buy_compute_unit_limit(
    topup_compute_unit_limit: Option<u64>,
    action_compute_unit_limit: Option<u64>,
) -> u64 {
    let child_default = configured_default_sniper_buy_compute_unit_limit();
    let merged_limit = topup_compute_unit_limit
        .unwrap_or(child_default)
        .saturating_add(action_compute_unit_limit.unwrap_or(child_default));
    merged_limit.max(configured_default_follow_up_compute_unit_limit())
}

async fn native_compile_follow_buy_transaction(
    rpc_url: &str,
    quote_asset: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    buy_amount: &str,
    allow_ata_creation: bool,
    pool_context_override: Option<&NativeBonkPoolContext>,
    pool_id_override: Option<&str>,
    usd1_route_setup_override: Option<&BonkUsd1RouteSetup>,
    wrapper_fee_bps: u16,
) -> Result<BonkFollowBuyCompileResult, String> {
    let owner = parse_owner_keypair(wallet_secret)?;
    let mint_pubkey =
        Pubkey::from_str(mint).map_err(|error| format!("Invalid Bonk mint address: {error}"))?;
    let venue_context = if let Some(pool_context) = pool_context_override {
        NativeBonkTradeVenueContext::Launchpad(pool_context.clone())
    } else if let Some(pool_id) = pool_id_override {
        load_bonk_trade_venue_context_by_pool_id(
            rpc_url,
            pool_id,
            quote_asset,
            &execution.commitment,
        )
        .await?
    } else {
        load_live_bonk_trade_venue_context(
            rpc_url,
            &mint_pubkey,
            quote_asset,
            &execution.commitment,
        )
        .await?
    };
    let quote = bonk_quote_asset_config(quote_asset);
    if quote.asset == "usd1" {
        let funding_policy = normalize_bonk_buy_funding_policy(&execution.buyFundingPolicy);
        let sol_input_quote = if matches!(funding_policy, "usd1_only" | "usd1_via_sol") {
            None
        } else {
            Some(
                native_quote_usd1_buy_amounts_from_sol_input(
                    rpc_url,
                    buy_amount,
                    usd1_route_setup_override,
                )
                .await?,
            )
        };
        let current_balance = match funding_policy {
            "sol_only" | "usd1_via_sol" => 0,
            _ => fetch_bonk_owner_token_balance(
                rpc_url,
                &execution.commitment,
                &owner.pubkey(),
                &bonk_quote_mint("usd1")?,
            )
            .await?
            .unwrap_or_default(),
        };
        let requested_amount_b = if matches!(funding_policy, "usd1_only" | "usd1_via_sol") {
            parse_decimal_u64(
                buy_amount,
                quote.decimals,
                &format!("follow buy amount {}", quote.label),
            )?
        } else {
            select_bonk_usd1_buy_amount_from_sol_quote(
                funding_policy,
                current_balance,
                sol_input_quote
                    .as_ref()
                    .ok_or_else(|| "Bonk USD1 SOL input quote was not prepared.".to_string())?,
            )
        };
        if funding_policy == "usd1_only" && current_balance < requested_amount_b {
            return Err(format!(
                "USD1-only buy requires {} USD1 available, but wallet only had {}.",
                format_biguint_decimal(&bonk_biguint_from_u64(requested_amount_b), 6, 6),
                format_biguint_decimal(&bonk_biguint_from_u64(current_balance), 6, 6),
            ));
        }
        if matches!(funding_policy, "sol_only" | "usd1_via_sol")
            || current_balance < requested_amount_b
        {
            let tip_lamports = resolve_follow_tip_lamports(
                &execution.buyProvider,
                &execution.buyTipSol,
                "buy tip",
            )?;
            let buy_tx_config = bonk_follow_tx_config(
                configured_default_bonk_usd1_dynamic_buy_compute_unit_limit(),
                priority_fee_sol_to_micro_lamports(&execution.buyPriorityFeeSol)?,
                tip_lamports,
                jito_tip_account,
            )?;
            if funding_policy == "sol_only" {
                let owner_pubkey = owner.pubkey();
                let slippage_bps = slippage_bps_from_percent(&execution.buySlippagePercent)?;
                let route_setup = if let Some(setup) = usd1_route_setup_override {
                    setup.clone()
                } else {
                    load_bonk_usd1_route_setup(rpc_url).await?
                };
                let gross_sol_in_lamports =
                    parse_decimal_u64(buy_amount, 9, "Bonk USD1 dynamic buy SOL input")?;
                let net_sol_in_lamports = gross_sol_in_lamports
                    .checked_sub(estimate_sol_in_fee_lamports(
                        gross_sol_in_lamports,
                        wrapper_fee_bps,
                    ))
                    .ok_or_else(|| "Bonk USD1 dynamic buy fee exceeded input SOL".to_string())?;
                let dynamic_usd1_quote = native_quote_usd1_output_from_sol_input_with_metrics(
                    rpc_url,
                    &bonk_biguint_from_u64(net_sol_in_lamports),
                    BONK_USD1_ROUTE_SLIPPAGE_BPS,
                    None,
                    Some(&route_setup),
                )
                .await?;
                let dynamic_min_amount_b = biguint_to_u64(
                    &dynamic_usd1_quote.min_out,
                    "Bonk USD1 dynamic buy guaranteed USD1 output",
                )?;
                let dynamic_expected_amount_b = biguint_to_u64(
                    &dynamic_usd1_quote.expected_out,
                    "Bonk USD1 dynamic buy expected USD1 output",
                )?;
                let (buy_ix, token_account, token_program, min_amount_a) = match &venue_context {
                    NativeBonkTradeVenueContext::Launchpad(pool_context) => {
                        let token_account =
                            spl_associated_token_account::get_associated_token_address_with_program_id(
                                &owner_pubkey,
                                &mint_pubkey,
                                &pool_context.token_program,
                            );
                        let usd1_account =
                            spl_associated_token_account::get_associated_token_address_with_program_id(
                                &owner_pubkey,
                                &bonk_quote_mint("usd1")?,
                                &spl_token::id(),
                            );
                        let (instruction_amount_b, min_amount_a) = bonk_follow_buy_amounts(
                            pool_context,
                            dynamic_expected_amount_b,
                            slippage_bps,
                        )?;
                        (
                            build_bonk_buy_exact_in_instruction(
                                &owner_pubkey,
                                pool_context,
                                &token_account,
                                &usd1_account,
                                instruction_amount_b,
                                min_amount_a,
                            )?,
                            token_account,
                            pool_context.token_program,
                            min_amount_a,
                        )
                    }
                    NativeBonkTradeVenueContext::RaydiumCpmm(pool_context) => {
                        let quote_mint = bonk_quote_mint(pool_context.quote.asset)?;
                        let (token_mint, token_program, quote_program) =
                            if pool_context.pool.token_0_mint == quote_mint {
                                (
                                    pool_context.pool.token_1_mint,
                                    pool_context.pool.token_1_program,
                                    pool_context.pool.token_0_program,
                                )
                            } else if pool_context.pool.token_1_mint == quote_mint {
                                (
                                    pool_context.pool.token_0_mint,
                                    pool_context.pool.token_0_program,
                                    pool_context.pool.token_1_program,
                                )
                            } else {
                                return Err(
                                    "Bonk CPMM quote mint did not match the selected pool."
                                        .to_string(),
                                );
                            };
                        if mint_pubkey != token_mint {
                            return Err(
                                "Bonk CPMM token mint did not match the selected pool.".to_string()
                            );
                        }
                        let token_account =
                            spl_associated_token_account::get_associated_token_address_with_program_id(
                                &owner_pubkey,
                                &mint_pubkey,
                                &token_program,
                            );
                        let usd1_account =
                            spl_associated_token_account::get_associated_token_address_with_program_id(
                                &owner_pubkey,
                                &quote_mint,
                                &quote_program,
                            );
                        let (_, min_amount_a) = bonk_cpmm_quote_exact_input(
                            pool_context,
                            &quote_mint,
                            dynamic_expected_amount_b,
                            slippage_bps,
                        )?;
                        (
                            build_bonk_cpmm_swap_exact_in_instruction(
                                &owner_pubkey,
                                pool_context,
                                &usd1_account,
                                &token_account,
                                dynamic_expected_amount_b,
                                min_amount_a,
                                &quote_mint,
                                &mint_pubkey,
                            )?,
                            token_account,
                            token_program,
                            min_amount_a,
                        )
                    }
                    NativeBonkTradeVenueContext::RaydiumClmm(pool_context) => {
                        let quote_mint = bonk_quote_mint(pool_context.quote.asset)?;
                        let (token_program, quote_program) = if pool_context.setup.mint_a
                            == quote_mint
                        {
                            (pool_context.mint_program_b, pool_context.mint_program_a)
                        } else if pool_context.setup.mint_b == quote_mint {
                            (pool_context.mint_program_a, pool_context.mint_program_b)
                        } else {
                            return Err(
                                "Bonk CLMM quote mint did not match the selected pool.".to_string()
                            );
                        };
                        let token_mint = if pool_context.setup.mint_a == quote_mint {
                            pool_context.setup.mint_b
                        } else {
                            pool_context.setup.mint_a
                        };
                        if mint_pubkey != token_mint {
                            return Err(
                                "Bonk CLMM token mint did not match the selected pool.".to_string()
                            );
                        }
                        let token_account =
                            spl_associated_token_account::get_associated_token_address_with_program_id(
                                &owner_pubkey,
                                &mint_pubkey,
                                &token_program,
                            );
                        let usd1_account =
                            spl_associated_token_account::get_associated_token_address_with_program_id(
                                &owner_pubkey,
                                &quote_mint,
                                &quote_program,
                            );
                        let quote = bonk_quote_clmm_exact_input(
                            &pool_context.setup,
                            &quote_mint,
                            &bonk_biguint_from_u64(dynamic_expected_amount_b),
                            slippage_bps,
                        )?;
                        let min_amount_a =
                            biguint_to_u64(&quote.min_out, "Bonk CLMM buy min output")?;
                        (
                            build_bonk_clmm_swap_exact_in_instruction_for_setup(
                                &owner_pubkey,
                                &pool_context.setup,
                                &usd1_account,
                                &token_account,
                                dynamic_expected_amount_b,
                                min_amount_a,
                                &quote.traversed_tick_array_starts,
                                &quote_mint,
                                &mint_pubkey,
                            )?,
                            token_account,
                            token_program,
                            min_amount_a,
                        )
                    }
                };
                let transaction = build_bonk_dynamic_usd1_buy_from_sol_route(
                    rpc_url,
                    &execution.commitment,
                    &owner,
                    allow_ata_creation,
                    &buy_tx_config,
                    execution.buyJitodontfront,
                    &mint_pubkey,
                    gross_sol_in_lamports,
                    net_sol_in_lamports,
                    dynamic_min_amount_b,
                    &route_setup,
                    buy_ix,
                    &token_account,
                    &token_program,
                    min_amount_a,
                    wrapper_fee_bps,
                )
                .await?;
                return Ok(BonkFollowBuyCompileResult {
                    transactions: vec![transaction],
                    primary_tx_index: 0,
                    requires_ordered_execution: false,
                    entry_preference_asset: Some("sol".to_string()),
                    wrapper_tx_index: Some(0),
                    wrapper_gross_sol_in_lamports: Some(gross_sol_in_lamports),
                });
            }
            let prepared_topup = native_prepare_bonk_usd1_topup(
                rpc_url,
                &execution.commitment,
                &owner.pubkey(),
                &bonk_biguint_from_u64(requested_amount_b),
                BONK_USD1_ROUTE_SLIPPAGE_BPS,
                if matches!(funding_policy, "sol_only" | "usd1_via_sol") {
                    BonkUsd1TopupMode::ForceFullAmount
                } else {
                    BonkUsd1TopupMode::RespectExistingBalance
                },
                None,
                usd1_route_setup_override,
            )
            .await?;
            let prepared_topup =
                prepared_topup_with_wrapper_gross_input(&prepared_topup, wrapper_fee_bps)?;
            let topup_transaction = native_compile_bonk_usd1_topup_from_prepared(
                rpc_url,
                &execution.commitment,
                &owner,
                allow_ata_creation,
                "usd1-topup",
                NativeBonkTxFormat::V0,
                &buy_tx_config,
                execution.buyJitodontfront,
                &prepared_topup,
            )
            .await?
            .ok_or_else(|| {
                "Native Bonk live USD1 follow buy could not prepare a required top-up transaction."
                    .to_string()
            })?;
            let wrapper_gross_sol_in_lamports = prepared_topup
                .input_lamports
                .as_ref()
                .ok_or_else(|| {
                    "Native Bonk live USD1 follow buy top-up was missing SOL input lamports."
                        .to_string()
                })
                .and_then(|value| biguint_to_u64(value, "Bonk USD1 top-up input lamports"))?;
            let action_transaction = native_compile_bonk_buy_transaction_with_venue_context(
                rpc_url,
                execution,
                jito_tip_account,
                &owner,
                &mint_pubkey,
                &venue_context,
                requested_amount_b,
                allow_ata_creation,
                NativeBonkTxFormat::V0,
                if funding_policy == "usd1_via_sol" {
                    0
                } else {
                    wrapper_fee_bps
                },
            )
            .await?;
            return match combine_atomic_bonk_usd1_follow_buy_transactions(
                rpc_url,
                execution,
                jito_tip_account,
                &owner,
                &topup_transaction,
                &action_transaction,
            )
            .await
            {
                Ok(transaction) => Ok(BonkFollowBuyCompileResult {
                    transactions: vec![transaction],
                    primary_tx_index: 0,
                    requires_ordered_execution: false,
                    entry_preference_asset: Some("sol".to_string()),
                    wrapper_tx_index: Some(0),
                    wrapper_gross_sol_in_lamports: Some(wrapper_gross_sol_in_lamports),
                }),
                Err(error) => {
                    let transport_plan = crate::transport::build_transport_plan(execution, 2);
                    if transport_plan.executionClass != "bundle"
                        && transport_plan.ordering != "bundle"
                    {
                        return Err(format!(
                            "Bonk USD1 follow buy could not be compiled atomically, and the resolved transport {} cannot safely carry the required dependent split: {}",
                            transport_plan.transportType, error
                        ));
                    }
                    Ok(BonkFollowBuyCompileResult {
                        transactions: vec![topup_transaction, action_transaction],
                        primary_tx_index: 1,
                        requires_ordered_execution: true,
                        entry_preference_asset: Some("sol".to_string()),
                        wrapper_tx_index: Some(0),
                        wrapper_gross_sol_in_lamports: Some(wrapper_gross_sol_in_lamports),
                    })
                }
            };
        }
        return Ok(BonkFollowBuyCompileResult {
            transactions: vec![
                native_compile_bonk_buy_transaction_with_venue_context(
                    rpc_url,
                    execution,
                    jito_tip_account,
                    &owner,
                    &mint_pubkey,
                    &venue_context,
                    requested_amount_b,
                    allow_ata_creation,
                    NativeBonkTxFormat::V0,
                    wrapper_fee_bps,
                )
                .await?,
            ],
            primary_tx_index: 0,
            requires_ordered_execution: false,
            entry_preference_asset: Some("usd1".to_string()),
            wrapper_tx_index: None,
            wrapper_gross_sol_in_lamports: None,
        });
    }
    let requested_amount_b = parse_decimal_u64(
        buy_amount,
        quote.decimals,
        &format!("follow buy amount {}", quote.label),
    )?;
    Ok(BonkFollowBuyCompileResult {
        transactions: vec![
            native_compile_bonk_buy_transaction_with_venue_context(
                rpc_url,
                execution,
                jito_tip_account,
                &owner,
                &mint_pubkey,
                &venue_context,
                requested_amount_b,
                allow_ata_creation,
                select_bonk_native_tx_format(&execution.txFormat),
                wrapper_fee_bps,
            )
            .await?,
        ],
        primary_tx_index: 0,
        requires_ordered_execution: false,
        entry_preference_asset: Some("sol".to_string()),
        wrapper_tx_index: None,
        wrapper_gross_sol_in_lamports: None,
    })
}

async fn native_compile_follow_sell_transaction_with_token_amount(
    rpc_url: &str,
    quote_asset: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    sell_percent: u8,
    token_amount_override: Option<u64>,
    pool_id_override: Option<&str>,
    launch_mode_override: Option<&str>,
    launch_creator_override: Option<&str>,
    sell_settlement_asset: &str,
    wrapper_fee_bps: u16,
) -> Result<Option<CompiledTransaction>, String> {
    let owner = parse_owner_keypair(wallet_secret)?;
    let owner_pubkey = owner.pubkey();
    let mint_pubkey =
        Pubkey::from_str(mint).map_err(|error| format!("Invalid Bonk mint address: {error}"))?;
    if should_use_prelaunch_follow_sell_override(
        quote_asset,
        &mint_pubkey,
        token_amount_override,
        pool_id_override,
        launch_mode_override,
        launch_creator_override,
    )? {
        let defaults = load_bonk_launch_defaults(
            rpc_url,
            launch_mode_override.unwrap_or_default(),
            quote_asset,
        )
        .await?;
        let creator = Pubkey::from_str(launch_creator_override.unwrap_or_default())
            .map_err(|error| format!("Invalid Bonk launch creator: {error}"))?;
        let pool_context = build_prelaunch_bonk_pool_context(
            &defaults,
            &mint_pubkey,
            &creator,
            launch_mode_override.unwrap_or_default(),
        )?;
        let raw_amount = token_amount_override.unwrap_or_default();
        if raw_amount == 0 {
            return Ok(None);
        }
        let sell_amount = (u128::from(raw_amount) * u128::from(sell_percent) / 100u128) as u64;
        if sell_amount == 0 {
            return Ok(None);
        }
        return Ok(Some(
            native_compile_follow_sell_launchpad_transaction(
                rpc_url,
                execution,
                jito_tip_account,
                &owner,
                &mint_pubkey,
                &pool_context,
                sell_amount,
                sell_settlement_asset,
                wrapper_fee_bps,
            )
            .await?,
        ));
    }

    let venue_context = if let Some(pool_id) = pool_id_override {
        load_bonk_trade_venue_context_by_pool_id(
            rpc_url,
            pool_id,
            quote_asset,
            &execution.commitment,
        )
        .await?
    } else {
        load_live_bonk_trade_venue_context(
            rpc_url,
            &mint_pubkey,
            quote_asset,
            &execution.commitment,
        )
        .await?
    };
    let token_program = match &venue_context {
        NativeBonkTradeVenueContext::Launchpad(context) => context.token_program,
        NativeBonkTradeVenueContext::RaydiumCpmm(context) => {
            if context.pool.token_0_mint == mint_pubkey {
                context.pool.token_0_program
            } else if context.pool.token_1_mint == mint_pubkey {
                context.pool.token_1_program
            } else {
                return Err("Bonk CPMM token mint did not match the selected pool.".to_string());
            }
        }
        NativeBonkTradeVenueContext::RaydiumClmm(context) => {
            if context.setup.mint_a == mint_pubkey {
                context.mint_program_a
            } else if context.setup.mint_b == mint_pubkey {
                context.mint_program_b
            } else {
                return Err("Bonk CLMM token mint did not match the selected pool.".to_string());
            }
        }
    };
    let raw_amount = if let Some(value) = token_amount_override {
        value
    } else {
        match fetch_bonk_owner_token_balance_with_token_program(
            rpc_url,
            &execution.commitment,
            &owner_pubkey,
            &mint_pubkey,
            &token_program,
        )
        .await?
        {
            Some(value) => value,
            None => return Ok(None),
        }
    };
    if raw_amount == 0 {
        return Ok(None);
    }
    let sell_amount = (u128::from(raw_amount) * u128::from(sell_percent) / 100u128) as u64;
    if sell_amount == 0 {
        return Ok(None);
    }
    Ok(Some(match &venue_context {
        NativeBonkTradeVenueContext::Launchpad(pool_context) => {
            native_compile_follow_sell_launchpad_transaction(
                rpc_url,
                execution,
                jito_tip_account,
                &owner,
                &mint_pubkey,
                pool_context,
                sell_amount,
                sell_settlement_asset,
                wrapper_fee_bps,
            )
            .await?
        }
        NativeBonkTradeVenueContext::RaydiumCpmm(pool_context) => {
            native_compile_bonk_sell_transaction_with_cpmm_context(
                rpc_url,
                execution,
                jito_tip_account,
                &owner,
                &mint_pubkey,
                pool_context,
                sell_amount,
                sell_settlement_asset,
                wrapper_fee_bps,
            )
            .await?
        }
        NativeBonkTradeVenueContext::RaydiumClmm(pool_context) => {
            native_compile_bonk_sell_transaction_with_clmm_context(
                rpc_url,
                execution,
                jito_tip_account,
                &owner,
                &mint_pubkey,
                pool_context,
                sell_amount,
                sell_settlement_asset,
                wrapper_fee_bps,
            )
            .await?
        }
    }))
}

async fn native_compile_atomic_follow_buy_transaction(
    rpc_url: &str,
    launch_mode: &str,
    quote_asset: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    launch_creator: &str,
    buy_amount: &str,
    allow_ata_creation: bool,
    predicted_prior_buy_quote_amount_b: Option<u64>,
    wrapper_fee_bps: u16,
) -> Result<BonkAtomicFollowBuyCompileResult, String> {
    let owner = parse_owner_keypair(wallet_secret)?;
    let mint_pubkey =
        Pubkey::from_str(mint).map_err(|error| format!("Invalid Bonk mint address: {error}"))?;
    let launch_creator_pubkey = Pubkey::from_str(launch_creator)
        .map_err(|error| format!("Invalid Bonk launch creator: {error}"))?;
    let defaults = load_bonk_launch_defaults(rpc_url, launch_mode, quote_asset).await?;
    let buy_slippage_bps = slippage_bps_from_percent(&execution.buySlippagePercent)?;
    let mut pool_context = build_prelaunch_bonk_pool_context(
        &defaults,
        &mint_pubkey,
        &launch_creator_pubkey,
        launch_mode,
    )?;
    if let Some(requested_amount_b) = predicted_prior_buy_quote_amount_b {
        pool_context = advance_prelaunch_bonk_pool_context_after_buy(
            &pool_context,
            requested_amount_b,
            buy_slippage_bps,
        )?;
    }
    let funding_policy = normalize_bonk_buy_funding_policy(&execution.buyFundingPolicy);
    let (requested_amount_b, current_usd1_balance) = if defaults.quote.asset == "usd1" {
        if funding_policy == "usd1_via_sol" {
            (
                parse_decimal_u64(
                    buy_amount,
                    defaults.quote.decimals,
                    &format!("follow buy amount {}", defaults.quote.label),
                )?,
                0,
            )
        } else {
            let sol_input_quote =
                native_quote_usd1_buy_amounts_from_sol_input(rpc_url, buy_amount, None).await?;
            let current_balance = if funding_policy == "sol_only" {
                0
            } else {
                fetch_bonk_owner_token_balance(
                    rpc_url,
                    &execution.commitment,
                    &owner.pubkey(),
                    &bonk_quote_mint("usd1")?,
                )
                .await?
                .unwrap_or_default()
            };
            (
                select_bonk_usd1_buy_amount_from_sol_quote(
                    funding_policy,
                    current_balance,
                    &sol_input_quote,
                ),
                current_balance,
            )
        }
    } else {
        (
            parse_decimal_u64(
                buy_amount,
                defaults.quote.decimals,
                &format!("follow buy amount {}", defaults.quote.label),
            )?,
            0,
        )
    };
    if defaults.quote.asset == "usd1" {
        let current_balance = current_usd1_balance;
        if current_balance < requested_amount_b {
            if funding_policy == "sol_only" {
                let tip_lamports = resolve_follow_tip_lamports(
                    &execution.buyProvider,
                    &execution.buyTipSol,
                    "buy tip",
                )?;
                let buy_tx_config = bonk_follow_tx_config(
                    configured_default_bonk_usd1_dynamic_buy_compute_unit_limit(),
                    priority_fee_sol_to_micro_lamports(&execution.buyPriorityFeeSol)?,
                    tip_lamports,
                    jito_tip_account,
                )?;
                let owner_pubkey = owner.pubkey();
                let token_account =
                    spl_associated_token_account::get_associated_token_address_with_program_id(
                        &owner_pubkey,
                        &mint_pubkey,
                        &pool_context.token_program,
                    );
                let usd1_account =
                    spl_associated_token_account::get_associated_token_address_with_program_id(
                        &owner_pubkey,
                        &bonk_quote_mint("usd1")?,
                        &spl_token::id(),
                    );
                let route_setup = load_bonk_usd1_route_setup(rpc_url).await?;
                let gross_sol_in_lamports =
                    parse_decimal_u64(buy_amount, 9, "Bonk USD1 dynamic buy SOL input")?;
                let net_sol_in_lamports = gross_sol_in_lamports
                    .checked_sub(estimate_sol_in_fee_lamports(
                        gross_sol_in_lamports,
                        wrapper_fee_bps,
                    ))
                    .ok_or_else(|| "Bonk USD1 dynamic buy fee exceeded input SOL".to_string())?;
                let dynamic_usd1_quote = native_quote_usd1_output_from_sol_input_with_metrics(
                    rpc_url,
                    &bonk_biguint_from_u64(net_sol_in_lamports),
                    BONK_USD1_ROUTE_SLIPPAGE_BPS,
                    None,
                    Some(&route_setup),
                )
                .await?;
                let dynamic_min_amount_b = biguint_to_u64(
                    &dynamic_usd1_quote.min_out,
                    "Bonk USD1 dynamic buy guaranteed USD1 output",
                )?;
                let dynamic_expected_amount_b = biguint_to_u64(
                    &dynamic_usd1_quote.expected_out,
                    "Bonk USD1 dynamic buy expected USD1 output",
                )?;
                let (instruction_amount_b, min_amount_a) = bonk_follow_buy_amounts(
                    &pool_context,
                    dynamic_expected_amount_b,
                    buy_slippage_bps,
                )?;
                let buy_ix = build_bonk_buy_exact_in_instruction(
                    &owner_pubkey,
                    &pool_context,
                    &token_account,
                    &usd1_account,
                    instruction_amount_b,
                    min_amount_a,
                )?;
                let transaction = build_bonk_dynamic_usd1_buy_from_sol_route(
                    rpc_url,
                    &execution.commitment,
                    &owner,
                    allow_ata_creation,
                    &buy_tx_config,
                    execution.buyJitodontfront,
                    &mint_pubkey,
                    gross_sol_in_lamports,
                    net_sol_in_lamports,
                    dynamic_min_amount_b,
                    &route_setup,
                    buy_ix,
                    &token_account,
                    &pool_context.token_program,
                    min_amount_a,
                    wrapper_fee_bps,
                )
                .await?;
                return Ok(BonkAtomicFollowBuyCompileResult {
                    transaction,
                    wrapper_gross_sol_in_lamports: Some(gross_sol_in_lamports),
                });
            }
            let required_quote_amount_b = bonk_biguint_from_u64(requested_amount_b);
            let prepared_topup = native_prepare_bonk_usd1_topup(
                rpc_url,
                &execution.commitment,
                &owner.pubkey(),
                &required_quote_amount_b,
                BONK_USD1_ROUTE_SLIPPAGE_BPS,
                BonkUsd1TopupMode::ForceFullAmount,
                None,
                None,
            )
            .await?;
            let prepared_topup =
                prepared_topup_with_wrapper_gross_input(&prepared_topup, wrapper_fee_bps)?;
            let wrapper_gross_sol_in_lamports = prepared_topup
                .input_lamports
                .as_ref()
                .ok_or_else(|| {
                    "Native Bonk atomic USD1 follow buy top-up was missing SOL input lamports."
                        .to_string()
                })
                .and_then(|value| biguint_to_u64(value, "Bonk USD1 top-up input lamports"))?;
            let tip_lamports = resolve_follow_tip_lamports(
                &execution.buyProvider,
                &execution.buyTipSol,
                "buy tip",
            )?;
            let buy_tx_config = bonk_follow_tx_config(
                configured_default_sniper_buy_compute_unit_limit(),
                priority_fee_sol_to_micro_lamports(&execution.buyPriorityFeeSol)?,
                tip_lamports,
                jito_tip_account,
            )?;
            let topup_transaction = native_compile_bonk_usd1_topup_from_prepared(
                rpc_url,
                &execution.commitment,
                &owner,
                allow_ata_creation,
                "usd1-topup",
                NativeBonkTxFormat::V0,
                &buy_tx_config,
                execution.buyJitodontfront,
                &prepared_topup,
            )
            .await?
            .ok_or_else(|| {
                "Native Bonk atomic USD1 follow buy could not prepare a required top-up transaction."
                    .to_string()
            })?;
            let action_transaction = native_compile_bonk_buy_transaction_with_pool_context(
                rpc_url,
                execution,
                jito_tip_account,
                &owner,
                &mint_pubkey,
                &pool_context,
                requested_amount_b,
                allow_ata_creation,
                NativeBonkTxFormat::V0,
                if funding_policy == "usd1_via_sol" {
                    0
                } else {
                    wrapper_fee_bps
                },
            )
            .await?;
            let transaction = combine_atomic_bonk_usd1_follow_buy_transactions(
                rpc_url,
                execution,
                jito_tip_account,
                &owner,
                &topup_transaction,
                &action_transaction,
            )
            .await?;
            return Ok(BonkAtomicFollowBuyCompileResult {
                transaction,
                wrapper_gross_sol_in_lamports: Some(wrapper_gross_sol_in_lamports),
            });
        }
    }
    let tx_format = if defaults.quote.asset == "usd1" {
        NativeBonkTxFormat::V0
    } else {
        select_bonk_native_tx_format(&execution.txFormat)
    };
    let transaction = native_compile_bonk_buy_transaction_with_pool_context(
        rpc_url,
        execution,
        jito_tip_account,
        &owner,
        &mint_pubkey,
        &pool_context,
        requested_amount_b,
        allow_ata_creation,
        tx_format,
        wrapper_fee_bps,
    )
    .await?;
    Ok(BonkAtomicFollowBuyCompileResult {
        transaction,
        wrapper_gross_sol_in_lamports: None,
    })
}

fn build_transaction_summaries(
    compiled_transactions: &[CompiledTransaction],
    dump_base64: bool,
) -> Vec<TransactionSummary> {
    compiled_transactions
        .iter()
        .map(|transaction| {
            let serialized_len = BASE64
                .decode(transaction.serializedBase64.as_bytes())
                .ok()
                .map(|bytes| bytes.len());
            let encoded_len = Some(transaction.serializedBase64.len());
            let mut summary = TransactionSummary {
                label: transaction.label.clone(),
                instructionSummary: Vec::<InstructionSummary>::new(),
                legacyLength: None,
                legacyBase64Length: None,
                v0Length: None,
                v0Base64Length: None,
                v0AltLength: None,
                v0AltBase64Length: None,
                legacyError: None,
                v0Error: None,
                v0AltError: None,
                lookupTablesUsed: transaction.lookupTablesUsed.clone(),
                fitsWithAlts: serialized_len
                    .map(|length| length <= PACKET_LIMIT_BYTES)
                    .unwrap_or(true),
                exceedsPacketLimit: serialized_len
                    .map(|length| length > PACKET_LIMIT_BYTES)
                    .unwrap_or(false),
                feeSettings: FeeSettings {
                    computeUnitLimit: transaction.computeUnitLimit.map(|value| value as i64),
                    computeUnitPriceMicroLamports: transaction
                        .computeUnitPriceMicroLamports
                        .map(|value| value as i64),
                    jitoTipLamports: transaction.inlineTipLamports.unwrap_or_default() as i64,
                    jitoTipAccount: transaction.inlineTipAccount.clone(),
                },
                base64: if dump_base64 {
                    Some(Value::String(transaction.serializedBase64.clone()))
                } else {
                    None
                },
                warnings: vec![],
            };
            match transaction.format.as_str() {
                "legacy" => {
                    summary.legacyLength = serialized_len;
                    summary.legacyBase64Length = encoded_len;
                }
                _ => {
                    summary.v0Length = serialized_len;
                    summary.v0Base64Length = encoded_len;
                }
            }
            summary
        })
        .collect()
}

fn validate_bonk_config(config: &NormalizedConfig) -> Result<(), String> {
    validate_launchpad_support(config).map_err(|error| error.to_string())
}

pub fn supports_native_bonk_compile(config: &NormalizedConfig) -> bool {
    config.launchpad == "bonk" && matches!(config.mode.as_str(), "regular" | "bonkers")
}

pub async fn quote_launch(
    rpc_url: &str,
    quote_asset: &str,
    launch_mode: &str,
    mode: &str,
    amount: &str,
) -> Result<Option<LaunchQuote>, String> {
    if amount.trim().is_empty() {
        return Ok(None);
    }
    let trimmed_mode = mode.trim().to_lowercase();
    let normalized_mode = if trimmed_mode.is_empty() {
        "sol"
    } else {
        trimmed_mode.as_str()
    };
    Ok(Some(
        native_quote_launch(rpc_url, quote_asset, launch_mode, normalized_mode, amount).await?,
    ))
}

async fn native_compile_sol_to_usd1_topup_transaction_with_format(
    rpc_url: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    required_quote_amount: &str,
    label_prefix: &str,
    tx_format_override: Option<&str>,
    route_setup_override: Option<&BonkUsd1RouteSetup>,
) -> Result<Option<CompiledTransaction>, String> {
    let owner = parse_owner_keypair(wallet_secret)?;
    let owner_pubkey = owner.pubkey();
    let required_quote_amount =
        parse_decimal_biguint(required_quote_amount, 6, "required USD1 amount")?;
    if required_quote_amount == BigUint::ZERO {
        return Ok(None);
    }
    let slippage_bps = BONK_USD1_ROUTE_SLIPPAGE_BPS;
    let usd1_mint = bonk_quote_mint("usd1")?;
    let current_quote_amount = bonk_biguint_from_u64(
        fetch_bonk_owner_token_balance(rpc_url, "processed", &owner_pubkey, &usd1_mint)
            .await?
            .unwrap_or(0),
    );
    if current_quote_amount >= required_quote_amount {
        return Ok(None);
    }
    let shortfall_quote_amount = bonk_big_sub(
        &required_quote_amount,
        &current_quote_amount,
        "Bonk USD1 shortfall amount",
    )?;
    let balance_lamports = bonk_rpc_get_balance_lamports(rpc_url, &owner_pubkey).await?;
    let min_remaining_lamports = bonk_usd1_min_remaining_lamports()?;
    let max_spendable_lamports = balance_lamports.saturating_sub(min_remaining_lamports);
    if max_spendable_lamports == 0 {
        return Err(format!(
            "Insufficient SOL headroom for USD1 top-up. Need at least {} SOL reserved after swap.",
            std::env::var("BONK_USD1_MIN_REMAINING_SOL").unwrap_or_else(|_| "0.02".to_string())
        ));
    }
    let input_lamports = native_quote_sol_input_for_usd1_output_with_max(
        rpc_url,
        &shortfall_quote_amount,
        slippage_bps,
        Some(BigUint::from(max_spendable_lamports)),
        route_setup_override,
    )
    .await?;
    let quote = native_quote_usd1_output_from_sol_input_with_metrics(
        rpc_url,
        &input_lamports,
        slippage_bps,
        None,
        route_setup_override,
    )
    .await?;
    if quote.min_out < shortfall_quote_amount {
        return Err("Native Bonk USD1 top-up quote could not satisfy required output.".to_string());
    }
    let amount_in = biguint_to_u64(&input_lamports, "Bonk USD1 top-up input lamports")?;
    let min_out = biguint_to_u64(&quote.min_out, "Bonk USD1 top-up minimum output")?;
    let tx_format = select_bonk_native_tx_format(tx_format_override.unwrap_or(&execution.txFormat));
    let tip_lamports = parse_decimal_u64(&execution.buyTipSol, 9, "buy tip")?;
    let tx_config = NativeBonkTxConfig {
        compute_unit_limit:
            u32::try_from(configured_default_launch_usd1_topup_compute_unit_limit())
                .map_err(|error| format!("Invalid USD1 top-up compute unit limit: {error}"))?,
        compute_unit_price_micro_lamports: priority_fee_sol_to_micro_lamports(
            &execution.buyPriorityFeeSol,
        )?,
        tip_lamports,
        tip_account: if tip_lamports > 0 {
            Pubkey::from_str(jito_tip_account)
                .map_err(|error| format!("Invalid Jito tip account: {error}"))?
                .to_string()
        } else {
            String::new()
        },
    };
    let token_program = spl_token::id();
    let user_output_account =
        spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            &usd1_mint,
            &token_program,
        );
    let wrapped_signer = Keypair::new();
    let rent_exempt_lamports = rpc_get_minimum_balance_for_rent_exemption(
        rpc_url,
        &execution.commitment,
        BONK_SPL_TOKEN_ACCOUNT_LEN,
    )
    .await?;
    let mut instructions = vec![
        spl_associated_token_account::instruction::create_associated_token_account_idempotent(
            &owner_pubkey,
            &owner_pubkey,
            &usd1_mint,
            &token_program,
        ),
    ];
    instructions.extend(build_bonk_wrapped_sol_open_instructions(
        &owner_pubkey,
        &wrapped_signer.pubkey(),
        rent_exempt_lamports.saturating_add(amount_in),
    )?);
    instructions.push(build_bonk_clmm_swap_exact_in_instruction(
        &owner_pubkey,
        &wrapped_signer.pubkey(),
        &user_output_account,
        amount_in,
        min_out,
        &quote.traversed_tick_array_starts,
    )?);
    instructions.push(build_bonk_wrapped_sol_close_instruction(
        &owner_pubkey,
        &wrapped_signer.pubkey(),
    )?);
    let tx_instructions = with_bonk_tx_settings(
        instructions,
        &tx_config,
        &owner_pubkey,
        execution.buyJitodontfront,
    )?;
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, &execution.commitment).await?;
    let preferred_lookup_tables = if tx_format == NativeBonkTxFormat::V0 {
        load_bonk_preferred_usd1_lookup_tables(rpc_url, &execution.commitment).await?
    } else {
        vec![]
    };
    build_bonk_compiled_transaction_with_lookup_preference(
        label_prefix,
        tx_format,
        &blockhash,
        last_valid_block_height,
        &owner,
        &[&wrapped_signer],
        tx_instructions,
        &tx_config,
        &[],
        &preferred_lookup_tables,
    )
    .map(Some)
}

async fn compile_sol_to_usd1_topup_transaction_with_format(
    rpc_url: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    required_quote_amount: &str,
    label_prefix: &str,
    tx_format_override: Option<&str>,
) -> Result<Option<CompiledTransaction>, String> {
    native_compile_sol_to_usd1_topup_transaction_with_format(
        rpc_url,
        execution,
        jito_tip_account,
        wallet_secret,
        required_quote_amount,
        label_prefix,
        tx_format_override,
        None,
    )
    .await
}

pub async fn compile_sol_to_usd1_topup_transaction(
    rpc_url: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    required_quote_amount: &str,
    label_prefix: &str,
) -> Result<Option<CompiledTransaction>, String> {
    compile_sol_to_usd1_topup_transaction_with_format(
        rpc_url,
        execution,
        jito_tip_account,
        wallet_secret,
        required_quote_amount,
        label_prefix,
        None,
    )
    .await
}

fn build_native_bonk_artifacts_from_launch_result(
    config: &NormalizedConfig,
    transport_plan: &TransportPlan,
    built_at: String,
    rpc_url: &str,
    creator_public_key: String,
    config_path: Option<String>,
    result: NativeBonkLaunchResult,
) -> Result<NativeBonkArtifacts, String> {
    let compiled_transactions = result.compiled_transactions;
    let mut report = build_report(
        config,
        transport_plan,
        built_at,
        rpc_url.to_string(),
        creator_public_key,
        result.mint.clone(),
        None,
        config_path,
        vec![],
    );
    report.execution.notes.push(if result.compiled_via_native {
        "Bonk launch assembly uses the native Rust compile path.".to_string()
    } else {
        "Bonk launch assembly uses the Raydium LaunchLab SDK-backed compile bridge.".to_string()
    });
    if let Some(backend) = launchpad_action_backend("bonk", "build-launch") {
        let rollout_state =
            launchpad_action_rollout_state("bonk", "build-launch").unwrap_or("unknown");
        report.execution.notes.push(format!(
            "Launchpad backend owner: {backend} ({rollout_state})."
        ));
    }
    if result.atomic_combined {
        report
            .execution
            .notes
            .push("USD1 dev buy was assembled atomically with the launch transaction.".to_string());
    } else if let Some(reason) = result.atomic_fallback_reason.as_ref() {
        report.execution.notes.push(format!(
            "USD1 dev buy uses split launch transactions: {reason}"
        ));
    }
    if let Some(details) = result.usd1_launch_details.as_ref() {
        report.bonkUsd1Launch = Some(BonkUsd1LaunchSummary {
            compilePath: details.compile_path.clone(),
            currentQuoteAmount: details.current_quote_amount.clone(),
            requiredQuoteAmount: details.required_quote_amount.clone(),
            shortfallQuoteAmount: details.shortfall_quote_amount.clone(),
            inputSol: details.input_sol.clone(),
            expectedQuoteOut: details.expected_quote_out.clone(),
            minQuoteOut: details.min_quote_out.clone(),
            atomicFallbackReason: result.atomic_fallback_reason.clone(),
        });
    }
    if let Some(metrics_note) = result
        .usd1_quote_metrics
        .as_ref()
        .and_then(render_usd1_quote_metrics_note)
    {
        report.execution.notes.push(metrics_note);
    }
    report.transactions = build_transaction_summaries(&compiled_transactions, config.tx.dumpBase64);
    let text = render_report(&report);
    let mut report = serde_json::to_value(report).map_err(|error| error.to_string())?;
    let mint_pubkey = Pubkey::from_str(&result.mint)
        .map_err(|error| format!("Invalid Bonk mint in launch report: {error}"))?;
    let pool_id = bonk_canonical_pool_id_for_mint(&config.quoteAsset, &mint_pubkey)?;
    report["pairAddress"] = serde_json::Value::String(pool_id.clone());
    report["routeAddress"] = serde_json::Value::String(pool_id.clone());
    report["poolAddress"] = serde_json::Value::String(pool_id);
    append_vanity_report_note(&mut report, result.vanity_reservation.as_ref());
    Ok(NativeBonkArtifacts {
        creation_transactions: compiled_transactions.clone(),
        deferred_setup_transactions: vec![],
        compiled_transactions,
        report,
        text,
        compile_timings: NativeCompileTimings::default(),
        mint: result.mint,
        launch_creator: result.launch_creator,
        vanity_reservation: result.vanity_reservation,
    })
}

#[derive(Debug)]
struct ResolvedBonkLaunchMint {
    keypair: Keypair,
    vanity_reservation: Option<VanityReservation>,
    requires_unused_check: bool,
}

async fn resolve_bonk_launch_mint_keypair(
    rpc_url: &str,
    vanity_private_key: &str,
) -> Result<ResolvedBonkLaunchMint, String> {
    let trimmed = vanity_private_key.trim();
    if trimmed.is_empty() {
        return Ok(ResolvedBonkLaunchMint {
            keypair: Keypair::new(),
            vanity_reservation: None,
            requires_unused_check: false,
        });
    }
    let bytes = read_keypair_bytes(trimmed)
        .map_err(|error| format!("Invalid vanity private key: {error}"))?;
    let keypair = Keypair::try_from(bytes.as_slice())
        .map_err(|error| format!("Invalid vanity private key: {error}"))?;
    let _ = rpc_url;
    Ok(ResolvedBonkLaunchMint {
        keypair,
        vanity_reservation: None,
        requires_unused_check: true,
    })
}

async fn resolve_bonk_launch_mint_keypair_for_launch(
    rpc_url: &str,
    vanity_private_key: &str,
    allow_queued_vanity: bool,
) -> Result<ResolvedBonkLaunchMint, String> {
    if !vanity_private_key.trim().is_empty() {
        return resolve_bonk_launch_mint_keypair(rpc_url, vanity_private_key).await;
    }
    if allow_queued_vanity
        && let Some(reserved) = reserve_vanity_mint(VanityLaunchpad::Bonk, rpc_url).await?
    {
        return Ok(ResolvedBonkLaunchMint {
            keypair: reserved.keypair,
            vanity_reservation: Some(reserved.reservation),
            requires_unused_check: false,
        });
    }
    Ok(ResolvedBonkLaunchMint {
        keypair: Keypair::new(),
        vanity_reservation: None,
        requires_unused_check: false,
    })
}

async fn ensure_bonk_launch_mint_unused(
    rpc_url: &str,
    keypair: &Keypair,
    requires_unused_check: bool,
) -> Result<(), String> {
    if !requires_unused_check {
        return Ok(());
    }
    match fetch_account_data(rpc_url, &keypair.pubkey().to_string(), "confirmed").await {
        Ok(_) => Err(format!(
            "This vanity address has already been used on-chain. Generate a fresh one. ({})",
            keypair.pubkey()
        )),
        Err(error) if error.contains("was not found.") => Ok(()),
        Err(error) => Err(format!(
            "Failed to verify vanity private key availability: {error}"
        )),
    }
}

fn build_native_bonk_launch_dev_buy_instructions(
    owner: &Pubkey,
    mint: &Pubkey,
    pool_context: &NativeBonkPoolContext,
    requested_amount_b: &BigUint,
    slippage_bps: u64,
    min_amount_a_override: Option<&BigUint>,
    allow_ata_creation: bool,
) -> Result<Vec<Instruction>, String> {
    let token_program = pool_context.token_program;
    let quote_token_program = spl_token::id();
    let user_token_account_a =
        spl_associated_token_account::get_associated_token_address_with_program_id(
            owner,
            mint,
            &token_program,
        );
    let mut instructions = vec![
        spl_associated_token_account::instruction::create_associated_token_account_idempotent(
            owner,
            owner,
            mint,
            &token_program,
        ),
    ];
    let (instruction_amount_b, min_amount_a) = if let Some(min_override) = min_amount_a_override {
        (
            biguint_to_u64(requested_amount_b, "launch dev buy amount")?,
            biguint_to_u64(min_override, "launch dev buy min token output")?,
        )
    } else {
        bonk_follow_buy_amounts(
            pool_context,
            biguint_to_u64(requested_amount_b, "launch dev buy amount")?,
            slippage_bps,
        )?
    };
    let user_token_account_b = if pool_context.quote.asset == "sol" {
        let wrapped_ata =
            spl_associated_token_account::get_associated_token_address_with_program_id(
                owner,
                &bonk_quote_mint("sol")?,
                &quote_token_program,
            );
        instructions.push(
            spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                owner,
                owner,
                &bonk_quote_mint("sol")?,
                &quote_token_program,
            ),
        );
        instructions.push(solana_system_interface::instruction::transfer(
            owner,
            &wrapped_ata,
            instruction_amount_b,
        ));
        instructions.push(
            spl_token::instruction::sync_native(&quote_token_program, &wrapped_ata).map_err(
                |error| format!("Failed to build launch sync-native instruction: {error}"),
            )?,
        );
        wrapped_ata
    } else {
        let quote_mint = bonk_quote_mint(pool_context.quote.asset)?;
        let quote_ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            owner,
            &quote_mint,
            &quote_token_program,
        );
        if allow_ata_creation {
            instructions.push(
                spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                    owner,
                    owner,
                    &quote_mint,
                    &quote_token_program,
                ),
            );
        }
        quote_ata
    };
    instructions.push(build_bonk_buy_exact_in_instruction(
        owner,
        pool_context,
        &user_token_account_a,
        &user_token_account_b,
        instruction_amount_b,
        min_amount_a,
    )?);
    Ok(instructions)
}

async fn native_compile_bonk_usd1_topup_from_prepared(
    rpc_url: &str,
    commitment: &str,
    owner: &Keypair,
    allow_ata_creation: bool,
    label_prefix: &str,
    tx_format: NativeBonkTxFormat,
    tx_config: &NativeBonkTxConfig,
    jitodontfront_enabled: bool,
    prepared: &NativeBonkPreparedUsd1Topup,
) -> Result<Option<CompiledTransaction>, String> {
    let Some(input_lamports) = prepared.input_lamports.as_ref() else {
        return Ok(None);
    };
    let owner_pubkey = owner.pubkey();
    let token_program = spl_token::id();
    let usd1_mint = bonk_quote_mint("usd1")?;
    let user_output_account =
        spl_associated_token_account::get_associated_token_address_with_program_id(
            &owner_pubkey,
            &usd1_mint,
            &token_program,
        );
    let wrapped_signer = Keypair::new();
    let rent_exempt_lamports =
        rpc_get_minimum_balance_for_rent_exemption(rpc_url, commitment, BONK_SPL_TOKEN_ACCOUNT_LEN)
            .await?;
    let mut instructions = Vec::new();
    if allow_ata_creation {
        instructions.push(
            spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                &owner_pubkey,
                &owner_pubkey,
                &usd1_mint,
                &token_program,
            ),
        );
    }
    instructions.extend(build_bonk_wrapped_sol_open_instructions(
        &owner_pubkey,
        &wrapped_signer.pubkey(),
        rent_exempt_lamports.saturating_add(biguint_to_u64(
            input_lamports,
            "Bonk USD1 top-up input lamports",
        )?),
    )?);
    instructions.push(build_bonk_clmm_swap_exact_in_instruction(
        &owner_pubkey,
        &wrapped_signer.pubkey(),
        &user_output_account,
        biguint_to_u64(input_lamports, "Bonk USD1 top-up input lamports")?,
        biguint_to_u64(
            prepared
                .min_quote_out
                .as_ref()
                .ok_or_else(|| "Bonk USD1 top-up minimum output was missing.".to_string())?,
            "Bonk USD1 top-up minimum output",
        )?,
        &prepared.traversed_tick_array_starts,
    )?);
    instructions.push(build_bonk_wrapped_sol_close_instruction(
        &owner_pubkey,
        &wrapped_signer.pubkey(),
    )?);
    let tx_instructions = with_bonk_tx_settings(
        instructions,
        tx_config,
        &owner_pubkey,
        jitodontfront_enabled,
    )?;
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, commitment).await?;
    let preferred_lookup_tables = if tx_format == NativeBonkTxFormat::V0 {
        load_bonk_preferred_usd1_lookup_tables(rpc_url, commitment).await?
    } else {
        vec![]
    };
    Ok(Some(
        build_bonk_compiled_transaction_with_lookup_preference(
            label_prefix,
            tx_format,
            &blockhash,
            last_valid_block_height,
            owner,
            &[&wrapped_signer],
            tx_instructions,
            tx_config,
            &[],
            &preferred_lookup_tables,
        )?,
    ))
}

async fn combine_atomic_bonk_transactions(
    rpc_url: &str,
    commitment: &str,
    owner: &Keypair,
    label: &str,
    tx_config: &NativeBonkTxConfig,
    jitodontfront_enabled: bool,
    extra_signers: &[&Keypair],
    topup_transaction: &CompiledTransaction,
    action_transaction: &CompiledTransaction,
) -> Result<CompiledTransaction, String> {
    let topup =
        decompose_bonk_compiled_v0_transaction(rpc_url, topup_transaction, commitment).await?;
    let action =
        decompose_bonk_compiled_v0_transaction(rpc_url, action_transaction, commitment).await?;
    let _ = validate_bonk_shared_lookup_tables_only(label, &topup.lookup_tables)?;
    let _ = validate_bonk_shared_lookup_tables_only(label, &action.lookup_tables)?;
    let owner_pubkey = owner.pubkey();
    let swap_instructions =
        filter_atomic_bonk_instructions(topup.instructions, &owner_pubkey, tx_config);
    let action_instructions =
        filter_atomic_bonk_instructions(action.instructions, &owner_pubkey, tx_config);
    let mut merged_instructions = build_bonk_atomic_tx_instructions(
        swap_instructions
            .into_iter()
            .chain(action_instructions.into_iter())
            .collect(),
        tx_config,
        &owner_pubkey,
        jitodontfront_enabled,
    )?;
    let allowed_child_signers = topup
        .signer_pubkeys
        .iter()
        .chain(action.signer_pubkeys.iter())
        .copied()
        .collect::<Vec<_>>();
    let generated_signers = rewrite_missing_bonk_instruction_signers(
        &owner_pubkey,
        &mut merged_instructions,
        extra_signers,
        &allowed_child_signers,
    )?;
    let mut merged_signers = extra_signers.to_vec();
    let generated_signer_refs = generated_signers.iter().collect::<Vec<_>>();
    merged_signers.extend(generated_signer_refs.iter().copied());
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, commitment).await?;
    let preferred_lookup_tables =
        load_bonk_preferred_usd1_lookup_tables(rpc_url, commitment).await?;
    build_bonk_compiled_transaction_with_lookup_preference(
        label,
        NativeBonkTxFormat::V0,
        &blockhash,
        last_valid_block_height,
        owner,
        &merged_signers,
        merged_instructions,
        tx_config,
        &[],
        &preferred_lookup_tables,
    )
}

async fn native_build_launch_result(
    rpc_url: &str,
    config: &NormalizedConfig,
    wallet_secret: &[u8],
    allow_ata_creation: bool,
) -> Result<NativeBonkLaunchResult, String> {
    let owner = parse_owner_keypair(wallet_secret)?;
    let owner_pubkey = owner.pubkey();
    let defaults = load_bonk_launch_defaults(rpc_url, &config.mode, &config.quoteAsset).await?;
    let slippage_bps = slippage_bps_from_percent(&config.execution.buySlippagePercent)?;
    let resolved_mint = resolve_bonk_launch_mint_keypair_for_launch(
        rpc_url,
        &config.vanityPrivateKey,
        allow_ata_creation,
    )
    .await?;
    let mint_keypair = resolved_mint.keypair;
    ensure_bonk_launch_mint_unused(rpc_url, &mint_keypair, resolved_mint.requires_unused_check)
        .await?;
    let vanity_reservation = resolved_mint.vanity_reservation;
    let mint_pubkey = mint_keypair.pubkey();
    let predicted_dev_buy_token_amount_raw = native_predict_dev_buy_token_amount(rpc_url, config)
        .await?
        .map(|value| value.to_string());
    let create_only = config
        .devBuy
        .as_ref()
        .map(|dev_buy| dev_buy.mode.trim().is_empty() || dev_buy.amount.trim().is_empty())
        .unwrap_or(true);
    let tx_format = if defaults.quote.asset == "usd1" {
        NativeBonkTxFormat::V0
    } else {
        select_bonk_native_tx_format(&config.execution.txFormat)
    };
    let launch_tx_config = bonk_launch_tx_config(config)?;
    let mut usd1_quote_metrics = if defaults.quote.asset == "usd1" {
        Some(HelperUsd1QuoteMetrics::default())
    } else {
        None
    };
    let preferred_lookup_tables = if tx_format == NativeBonkTxFormat::V0 {
        load_bonk_preferred_usd1_lookup_tables_with_metrics(
            rpc_url,
            &config.execution.commitment,
            usd1_quote_metrics.as_mut(),
        )
        .await?
    } else {
        vec![]
    };
    let single_bundle_tip_last_tx =
        uses_single_bundle_tip_last_tx(&config.execution.provider, &config.execution.mevMode);
    let mut launch_instructions = vec![build_bonk_initialize_v2_instruction(
        &owner_pubkey,
        &mint_pubkey,
        &config.mode,
        &config.token.name,
        &config.token.symbol,
        &config.token.uri,
        &defaults,
    )?];
    let mut prepared_usd1_topup = None;
    let mut usd1_launch_details = None;
    if !create_only {
        let dev_buy = config
            .devBuy
            .as_ref()
            .ok_or_else(|| "Bonk dev buy was missing after create-only detection.".to_string())?;
        let prelaunch_pool_context = build_prelaunch_bonk_pool_context(
            &defaults,
            &mint_pubkey,
            &owner_pubkey,
            &config.mode,
        )?;
        let mut min_mint_a_amount = None;
        let requested_amount_b = if dev_buy.mode.trim().eq_ignore_ascii_case("tokens") {
            let requested_tokens =
                parse_decimal_biguint(&dev_buy.amount, BONK_TOKEN_DECIMALS, "dev buy tokens")?;
            let required_quote_amount =
                bonk_quote_buy_exact_out_amount_b(&defaults, &requested_tokens)?;
            min_mint_a_amount = Some(bonk_build_min_amount_from_bps(
                &requested_tokens,
                slippage_bps,
            ));
            required_quote_amount
        } else if defaults.quote.asset == "usd1" {
            let input_sol = parse_decimal_biguint(&dev_buy.amount, 9, "dev buy SOL")?;
            let usd1_route_quote = native_quote_usd1_output_from_sol_input_with_metrics(
                rpc_url,
                &input_sol,
                BONK_USD1_ROUTE_SLIPPAGE_BPS,
                usd1_quote_metrics.as_mut(),
                None,
            )
            .await?;
            usd1_route_quote.min_out
        } else {
            parse_decimal_biguint(
                &dev_buy.amount,
                defaults.quote.decimals,
                &format!("dev buy {}", defaults.quote.label),
            )?
        };
        if defaults.quote.asset == "usd1" {
            let prepared = native_prepare_bonk_usd1_topup(
                rpc_url,
                &config.execution.commitment,
                &owner_pubkey,
                &requested_amount_b,
                BONK_USD1_ROUTE_SLIPPAGE_BPS,
                BonkUsd1TopupMode::RespectExistingBalance,
                usd1_quote_metrics.as_mut(),
                None,
            )
            .await?;
            prepared_usd1_topup = Some(prepared.clone());
            usd1_launch_details = Some(NativeBonkUsd1LaunchDetails {
                compile_path: if prepared.input_lamports.is_some() {
                    "split-topup+launch".to_string()
                } else {
                    "launch-only".to_string()
                },
                required_quote_amount: format_biguint_decimal(
                    &prepared.required_quote_amount,
                    6,
                    6,
                ),
                current_quote_amount: format_biguint_decimal(&prepared.current_quote_amount, 6, 6),
                shortfall_quote_amount: format_biguint_decimal(
                    &prepared.shortfall_quote_amount,
                    6,
                    6,
                ),
                input_sol: prepared
                    .input_lamports
                    .as_ref()
                    .map(|value| format_biguint_decimal(value, 9, 6)),
                expected_quote_out: prepared
                    .expected_quote_out
                    .as_ref()
                    .map(|value| format_biguint_decimal(value, 6, 6)),
                min_quote_out: prepared
                    .min_quote_out
                    .as_ref()
                    .map(|value| format_biguint_decimal(value, 6, 6)),
            });
        }
        launch_instructions.extend(build_native_bonk_launch_dev_buy_instructions(
            &owner_pubkey,
            &mint_pubkey,
            &prelaunch_pool_context,
            &requested_amount_b,
            slippage_bps,
            min_mint_a_amount.as_ref(),
            allow_ata_creation,
        )?);
    }
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, &config.execution.commitment).await?;
    let mint_signer_refs = [&mint_keypair];
    let mut compiled_launch_transactions = split_bonk_instruction_bundle(
        "launch",
        tx_format,
        &blockhash,
        last_valid_block_height,
        &owner,
        &mint_signer_refs,
        launch_instructions,
        &launch_tx_config,
        config.execution.jitodontfront,
        single_bundle_tip_last_tx,
        &preferred_lookup_tables,
    )?;
    let mut atomic_combined = false;
    let mut atomic_fallback_reason = None;
    if let Some(prepared) = prepared_usd1_topup.as_ref() {
        if let Some(topup_transaction) = native_compile_bonk_usd1_topup_from_prepared(
            rpc_url,
            &config.execution.commitment,
            &owner,
            allow_ata_creation,
            "launch-usd1-topup",
            NativeBonkTxFormat::V0,
            &bonk_bundle_tx_config_for_index(
                &launch_tx_config,
                0,
                compiled_launch_transactions.len() + 1,
                single_bundle_tip_last_tx,
            ),
            config.execution.jitodontfront,
            prepared,
        )
        .await?
        {
            if compiled_launch_transactions.len() == 1 {
                match combine_atomic_bonk_transactions(
                    rpc_url,
                    &config.execution.commitment,
                    &owner,
                    "launch",
                    &launch_tx_config,
                    config.execution.jitodontfront,
                    &mint_signer_refs,
                    &topup_transaction,
                    compiled_launch_transactions.first().expect("launch tx"),
                )
                .await
                {
                    Ok(combined) => {
                        atomic_combined = true;
                        compiled_launch_transactions = vec![combined];
                        if let Some(details) = usd1_launch_details.as_mut() {
                            details.compile_path = "atomic-topup+launch".to_string();
                        }
                    }
                    Err(error) => {
                        atomic_fallback_reason =
                            Some(format!("Atomic USD1 launch fallback: {error}"));
                        compiled_launch_transactions.insert(0, topup_transaction);
                    }
                }
            } else {
                atomic_fallback_reason = Some(
                    "Atomic USD1 launch requires exactly one top-up transaction and one launch transaction."
                        .to_string(),
                );
                compiled_launch_transactions.insert(0, topup_transaction);
            }
            if !atomic_combined && atomic_fallback_reason.is_none() {
                atomic_fallback_reason = Some(
                    "USD1 launch path is using split top-up plus launch transactions.".to_string(),
                );
            }
        }
    }
    Ok(NativeBonkLaunchResult {
        mint: mint_pubkey.to_string(),
        launch_creator: owner_pubkey.to_string(),
        vanity_reservation,
        compiled_transactions: compiled_launch_transactions,
        predicted_dev_buy_token_amount_raw,
        atomic_combined,
        atomic_fallback_reason,
        usd1_launch_details,
        usd1_quote_metrics: usd1_quote_metrics.and_then(|metrics| {
            if render_usd1_quote_metrics_note(&metrics).is_some() {
                Some(metrics)
            } else {
                None
            }
        }),
        compiled_via_native: true,
    })
}

pub async fn try_compile_native_bonk(
    rpc_url: &str,
    config: &NormalizedConfig,
    transport_plan: &TransportPlan,
    wallet_secret: &[u8],
    built_at: String,
    creator_public_key: String,
    config_path: Option<String>,
    allow_ata_creation: bool,
) -> Result<Option<NativeBonkArtifacts>, String> {
    if config.launchpad != "bonk" {
        return Ok(None);
    }
    validate_bonk_config(config)?;
    let launch_result =
        native_build_launch_result(rpc_url, config, wallet_secret, allow_ata_creation).await?;
    Ok(Some(build_native_bonk_artifacts_from_launch_result(
        config,
        transport_plan,
        built_at,
        rpc_url,
        creator_public_key,
        config_path,
        launch_result,
    )?))
}

pub async fn compile_follow_buy_transaction(
    rpc_url: &str,
    quote_asset: &str,
    execution: &NormalizedExecution,
    _token_mayhem_mode: bool,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    _launch_creator: &str,
    buy_amount_sol: &str,
    allow_ata_creation: bool,
    pool_context_override: Option<&NativeBonkPoolContext>,
    pool_id_override: Option<&str>,
    usd1_route_setup_override: Option<&BonkUsd1RouteSetup>,
) -> Result<CompiledTransaction, String> {
    let result = native_compile_follow_buy_transaction(
        rpc_url,
        quote_asset,
        execution,
        jito_tip_account,
        wallet_secret,
        mint,
        buy_amount_sol,
        allow_ata_creation,
        pool_context_override,
        pool_id_override,
        usd1_route_setup_override,
        DEFAULT_WRAPPER_FEE_BPS,
    )
    .await?;
    if result.transactions.len() != 1 {
        return Err(
            "Bonk follow buy produced multiple transactions; use the metadata/multi-transaction API."
                .to_string(),
        );
    }
    result
        .transactions
        .into_iter()
        .next()
        .ok_or_else(|| "Bonk follow buy produced no transactions.".to_string())
}

pub async fn compile_follow_buy_transaction_with_metadata(
    rpc_url: &str,
    quote_asset: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    buy_amount_sol: &str,
    allow_ata_creation: bool,
    pool_context_override: Option<&NativeBonkPoolContext>,
    pool_id_override: Option<&str>,
    usd1_route_setup_override: Option<&BonkUsd1RouteSetup>,
    wrapper_fee_bps: u16,
) -> Result<BonkFollowBuyCompileResult, String> {
    native_compile_follow_buy_transaction(
        rpc_url,
        quote_asset,
        execution,
        jito_tip_account,
        wallet_secret,
        mint,
        buy_amount_sol,
        allow_ata_creation,
        pool_context_override,
        pool_id_override,
        usd1_route_setup_override,
        wrapper_fee_bps,
    )
    .await
}

pub async fn compile_atomic_follow_buy_transaction(
    rpc_url: &str,
    launch_mode: &str,
    quote_asset: &str,
    execution: &NormalizedExecution,
    _token_mayhem_mode: bool,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    launch_creator: &str,
    buy_amount_sol: &str,
    allow_ata_creation: bool,
    predicted_prior_buy_quote_amount_b: Option<u64>,
) -> Result<CompiledTransaction, String> {
    compile_atomic_follow_buy_transaction_with_metadata(
        rpc_url,
        launch_mode,
        quote_asset,
        execution,
        _token_mayhem_mode,
        jito_tip_account,
        wallet_secret,
        mint,
        launch_creator,
        buy_amount_sol,
        allow_ata_creation,
        predicted_prior_buy_quote_amount_b,
        DEFAULT_WRAPPER_FEE_BPS,
    )
    .await
    .map(|result| result.transaction)
}

pub async fn compile_atomic_follow_buy_transaction_with_metadata(
    rpc_url: &str,
    launch_mode: &str,
    quote_asset: &str,
    execution: &NormalizedExecution,
    _token_mayhem_mode: bool,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    launch_creator: &str,
    buy_amount_sol: &str,
    allow_ata_creation: bool,
    predicted_prior_buy_quote_amount_b: Option<u64>,
    wrapper_fee_bps: u16,
) -> Result<BonkAtomicFollowBuyCompileResult, String> {
    native_compile_atomic_follow_buy_transaction(
        rpc_url,
        launch_mode,
        quote_asset,
        execution,
        jito_tip_account,
        wallet_secret,
        mint,
        launch_creator,
        buy_amount_sol,
        allow_ata_creation,
        predicted_prior_buy_quote_amount_b,
        wrapper_fee_bps,
    )
    .await
}

pub async fn compile_follow_sell_transaction(
    rpc_url: &str,
    quote_asset: &str,
    execution: &NormalizedExecution,
    _token_mayhem_mode: bool,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    _launch_creator: &str,
    sell_percent: u8,
    _prefer_post_setup_creator_vault: bool,
) -> Result<Option<CompiledTransaction>, String> {
    compile_follow_sell_transaction_with_token_amount_and_settlement(
        rpc_url,
        quote_asset,
        execution,
        jito_tip_account,
        wallet_secret,
        mint,
        sell_percent,
        None,
        None,
        None,
        None,
        quote_asset,
        DEFAULT_WRAPPER_FEE_BPS,
    )
    .await
}

pub async fn compile_follow_sell_transaction_with_token_amount(
    rpc_url: &str,
    quote_asset: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    sell_percent: u8,
    token_amount_override: Option<u64>,
    pool_id_override: Option<&str>,
    launch_mode_override: Option<&str>,
    launch_creator_override: Option<&str>,
) -> Result<Option<CompiledTransaction>, String> {
    compile_follow_sell_transaction_with_token_amount_and_settlement(
        rpc_url,
        quote_asset,
        execution,
        jito_tip_account,
        wallet_secret,
        mint,
        sell_percent,
        token_amount_override,
        pool_id_override,
        launch_mode_override,
        launch_creator_override,
        quote_asset,
        DEFAULT_WRAPPER_FEE_BPS,
    )
    .await
}

pub async fn compile_follow_sell_transaction_with_token_amount_and_settlement(
    rpc_url: &str,
    quote_asset: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    sell_percent: u8,
    token_amount_override: Option<u64>,
    pool_id_override: Option<&str>,
    launch_mode_override: Option<&str>,
    launch_creator_override: Option<&str>,
    sell_settlement_asset: &str,
    wrapper_fee_bps: u16,
) -> Result<Option<CompiledTransaction>, String> {
    native_compile_follow_sell_transaction_with_token_amount(
        rpc_url,
        quote_asset,
        execution,
        jito_tip_account,
        wallet_secret,
        mint,
        sell_percent,
        token_amount_override,
        pool_id_override,
        launch_mode_override,
        launch_creator_override,
        sell_settlement_asset,
        wrapper_fee_bps,
    )
    .await
}

pub async fn predict_dev_buy_token_amount(
    rpc_url: &str,
    config: &NormalizedConfig,
) -> Result<Option<u64>, String> {
    native_predict_dev_buy_token_amount(rpc_url, config).await
}

pub async fn predict_dev_buy_effect(
    rpc_url: &str,
    config: &NormalizedConfig,
) -> Result<Option<BonkPredictedDevBuyEffect>, String> {
    native_predict_dev_buy_effect(rpc_url, config).await
}

pub async fn load_live_follow_buy_pool_context(
    rpc_url: &str,
    mint: &str,
    quote_asset: &str,
    commitment: &str,
) -> Result<NativeBonkPoolContext, String> {
    let mint =
        Pubkey::from_str(mint).map_err(|error| format!("Invalid Bonk mint address: {error}"))?;
    load_live_bonk_pool_context(rpc_url, &mint, quote_asset, commitment).await
}

pub async fn load_live_follow_buy_usd1_route_setup(
    rpc_url: &str,
) -> Result<BonkUsd1RouteSetup, String> {
    load_bonk_usd1_route_setup_fresh(rpc_url).await
}

pub async fn quote_sol_lamports_for_exact_usd1_input(
    rpc_url: &str,
    usd1_raw: u64,
) -> Result<u64, String> {
    if usd1_raw == 0 {
        return Ok(0);
    }
    let setup = load_bonk_usd1_route_setup_fresh(rpc_url).await?;
    let quote = bonk_quote_sol_from_exact_usd1_input(&setup, &BigUint::from(usd1_raw), 0)?;
    biguint_to_u64(&quote.min_out, "Bonk USD1 route quote output lamports")
}

fn bonk_canonical_pool_id_for_mint(
    quote_asset: &str,
    mint_pubkey: &Pubkey,
) -> Result<String, String> {
    let launchpad_program = bonk_launchpad_program_id()?;
    let quote_pubkey = bonk_quote_mint(quote_asset)?;
    let (pool_id, _) = Pubkey::find_program_address(
        &[b"pool", mint_pubkey.as_ref(), quote_pubkey.as_ref()],
        &launchpad_program,
    );
    Ok(pool_id.to_string())
}

fn should_use_prelaunch_follow_sell_override(
    quote_asset: &str,
    mint_pubkey: &Pubkey,
    token_amount_override: Option<u64>,
    pool_id_override: Option<&str>,
    launch_mode_override: Option<&str>,
    launch_creator_override: Option<&str>,
) -> Result<bool, String> {
    if token_amount_override.is_none()
        || launch_mode_override.is_none()
        || launch_creator_override.is_none()
    {
        return Ok(false);
    }
    let Some(pool_id_override) = pool_id_override.filter(|pool_id| !pool_id.trim().is_empty())
    else {
        return Ok(false);
    };
    Ok(pool_id_override == bonk_canonical_pool_id_for_mint(quote_asset, mint_pubkey)?)
}

pub async fn derive_canonical_pool_id(quote_asset: &str, mint: &str) -> Result<String, String> {
    let mint_pubkey =
        Pubkey::from_str(mint).map_err(|error| format!("Invalid Bonk mint address: {error}"))?;
    bonk_canonical_pool_id_for_mint(quote_asset, &mint_pubkey)
}

pub async fn fetch_bonk_market_snapshot(
    rpc_url: &str,
    mint: &str,
    quote_asset: &str,
) -> Result<BonkMarketSnapshot, String> {
    native_fetch_bonk_market_snapshot(rpc_url, mint, quote_asset).await
}

pub async fn detect_bonk_import_context(
    rpc_url: &str,
    mint: &str,
) -> Result<Option<BonkImportContext>, String> {
    native_detect_bonk_import_context(rpc_url, mint).await
}

pub async fn detect_bonk_import_context_with_quote_asset(
    rpc_url: &str,
    mint: &str,
    quote_asset: &str,
) -> Result<Option<BonkImportContext>, String> {
    native_detect_bonk_import_context_with_quote_asset(rpc_url, mint, quote_asset).await
}

pub async fn poll_bonk_market_cap_lamports(
    rpc_url: &str,
    mint: &str,
    quote_asset: &str,
) -> Result<Option<u64>, String> {
    let snapshot = fetch_bonk_market_snapshot(rpc_url, mint, quote_asset).await?;
    let value = snapshot
        .marketCapLamports
        .parse::<u64>()
        .map_err(|error| format!("Invalid Bonk market cap response: {error}"))?;
    Ok(Some(value))
}

pub async fn warm_bonk_state(rpc_url: &str) -> Result<Value, String> {
    let (regular_sol, regular_usd1, bonkers_sol, bonkers_usd1) = tokio::try_join!(
        load_bonk_launch_defaults_with_startup_stagger(rpc_url, "regular", "sol", 0),
        load_bonk_launch_defaults_with_startup_stagger(rpc_url, "regular", "usd1", 1),
        load_bonk_launch_defaults_with_startup_stagger(rpc_url, "bonkers", "sol", 2),
        load_bonk_launch_defaults_with_startup_stagger(rpc_url, "bonkers", "usd1", 3),
    )?;
    let helper_launch_defaults = vec![
        json!({
            "mode": "regular",
            "quoteAsset": regular_sol.quote.asset,
            "platformId": bonk_platform_id("regular"),
            "configId": bonk_launch_config_id(regular_sol.quote.asset)?,
            "quoteMint": bonk_quote_mint(regular_sol.quote.asset)?.to_string(),
        }),
        json!({
            "mode": "regular",
            "quoteAsset": regular_usd1.quote.asset,
            "platformId": bonk_platform_id("regular"),
            "configId": bonk_launch_config_id(regular_usd1.quote.asset)?,
            "quoteMint": bonk_quote_mint(regular_usd1.quote.asset)?.to_string(),
        }),
        json!({
            "mode": "bonkers",
            "quoteAsset": bonkers_sol.quote.asset,
            "platformId": bonk_platform_id("bonkers"),
            "configId": bonk_launch_config_id(bonkers_sol.quote.asset)?,
            "quoteMint": bonk_quote_mint(bonkers_sol.quote.asset)?.to_string(),
        }),
        json!({
            "mode": "bonkers",
            "quoteAsset": bonkers_usd1.quote.asset,
            "platformId": bonk_platform_id("bonkers"),
            "configId": bonk_launch_config_id(bonkers_usd1.quote.asset)?,
            "quoteMint": bonk_quote_mint(bonkers_usd1.quote.asset)?.to_string(),
        }),
    ];
    let preview_launch_defaults = vec![
        json!({
            "mode": "regular",
            "quoteAsset": regular_sol.quote.asset,
            "quoteAssetLabel": regular_sol.quote.label,
            "quoteDecimals": regular_sol.quote.decimals,
            "supply": regular_sol.supply.to_string(),
            "totalFundRaisingB": regular_sol.total_fund_raising_b.to_string(),
            "tradeFeeRate": regular_sol.trade_fee_rate.to_string(),
            "platformFeeRate": regular_sol.platform_fee_rate.to_string(),
            "creatorFeeRate": regular_sol.creator_fee_rate.to_string(),
            "curveType": regular_sol.curve_type,
            "pool": {
                "totalSellA": regular_sol.pool.total_sell_a.to_string(),
                "virtualA": regular_sol.pool.virtual_a.to_string(),
                "virtualB": regular_sol.pool.virtual_b.to_string(),
                "realA": regular_sol.pool.real_a.to_string(),
                "realB": regular_sol.pool.real_b.to_string(),
            }
        }),
        json!({
            "mode": "regular",
            "quoteAsset": regular_usd1.quote.asset,
            "quoteAssetLabel": regular_usd1.quote.label,
            "quoteDecimals": regular_usd1.quote.decimals,
            "supply": regular_usd1.supply.to_string(),
            "totalFundRaisingB": regular_usd1.total_fund_raising_b.to_string(),
            "tradeFeeRate": regular_usd1.trade_fee_rate.to_string(),
            "platformFeeRate": regular_usd1.platform_fee_rate.to_string(),
            "creatorFeeRate": regular_usd1.creator_fee_rate.to_string(),
            "curveType": regular_usd1.curve_type,
            "pool": {
                "totalSellA": regular_usd1.pool.total_sell_a.to_string(),
                "virtualA": regular_usd1.pool.virtual_a.to_string(),
                "virtualB": regular_usd1.pool.virtual_b.to_string(),
                "realA": regular_usd1.pool.real_a.to_string(),
                "realB": regular_usd1.pool.real_b.to_string(),
            }
        }),
        json!({
            "mode": "bonkers",
            "quoteAsset": bonkers_sol.quote.asset,
            "quoteAssetLabel": bonkers_sol.quote.label,
            "quoteDecimals": bonkers_sol.quote.decimals,
            "supply": bonkers_sol.supply.to_string(),
            "totalFundRaisingB": bonkers_sol.total_fund_raising_b.to_string(),
            "tradeFeeRate": bonkers_sol.trade_fee_rate.to_string(),
            "platformFeeRate": bonkers_sol.platform_fee_rate.to_string(),
            "creatorFeeRate": bonkers_sol.creator_fee_rate.to_string(),
            "curveType": bonkers_sol.curve_type,
            "pool": {
                "totalSellA": bonkers_sol.pool.total_sell_a.to_string(),
                "virtualA": bonkers_sol.pool.virtual_a.to_string(),
                "virtualB": bonkers_sol.pool.virtual_b.to_string(),
                "realA": bonkers_sol.pool.real_a.to_string(),
                "realB": bonkers_sol.pool.real_b.to_string(),
            }
        }),
        json!({
            "mode": "bonkers",
            "quoteAsset": bonkers_usd1.quote.asset,
            "quoteAssetLabel": bonkers_usd1.quote.label,
            "quoteDecimals": bonkers_usd1.quote.decimals,
            "supply": bonkers_usd1.supply.to_string(),
            "totalFundRaisingB": bonkers_usd1.total_fund_raising_b.to_string(),
            "tradeFeeRate": bonkers_usd1.trade_fee_rate.to_string(),
            "platformFeeRate": bonkers_usd1.platform_fee_rate.to_string(),
            "creatorFeeRate": bonkers_usd1.creator_fee_rate.to_string(),
            "curveType": bonkers_usd1.curve_type,
            "pool": {
                "totalSellA": bonkers_usd1.pool.total_sell_a.to_string(),
                "virtualA": bonkers_usd1.pool.virtual_a.to_string(),
                "virtualB": bonkers_usd1.pool.virtual_b.to_string(),
                "realA": bonkers_usd1.pool.real_a.to_string(),
                "realB": bonkers_usd1.pool.real_b.to_string(),
            }
        }),
    ];
    let payload = json!({
        "ok": true,
        "backend": launchpad_action_backend("bonk", "startup-warm"),
        "rolloutState": launchpad_action_rollout_state("bonk", "startup-warm"),
        "launchDefaults": helper_launch_defaults.clone(),
        "warmedLaunchDefaults": helper_launch_defaults,
        "previewBasis": {
            "launchDefaults": preview_launch_defaults,
        },
        "usd1RoutePoolId": BONK_PINNED_USD1_ROUTE_POOL_ID,
        "usd1RouteConfigId": BONK_PREFERRED_USD1_ROUTE_CONFIG_ID,
        "usd1QuoteMetrics": HelperUsd1QuoteMetrics::default(),
    });
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use borsh::BorshDeserialize;

    #[tokio::test]
    async fn resolve_bonk_launch_mint_keypair_uses_vanity_private_key_when_present() {
        let vanity_keypair = Keypair::new();
        let encoded = bs58::encode(vanity_keypair.to_bytes()).into_string();

        let resolved =
            resolve_bonk_launch_mint_keypair_for_launch("http://127.0.0.1:1", &encoded, true)
                .await
                .expect("vanity mint keypair");

        assert_eq!(resolved.keypair.pubkey(), vanity_keypair.pubkey());
        assert!(resolved.vanity_reservation.is_none());
        assert!(resolved.requires_unused_check);
    }

    #[tokio::test]
    async fn resolve_bonk_launch_mint_keypair_rejects_invalid_vanity_private_key() {
        let error = resolve_bonk_launch_mint_keypair_for_launch(
            "http://127.0.0.1:1",
            "not-a-keypair",
            true,
        )
        .await
        .expect_err("invalid vanity key should fail");

        assert!(error.contains("Invalid vanity private key"));
    }

    #[test]
    fn usd1_sol_input_buy_amount_uses_expected_only_when_already_available() {
        let quote = BonkUsd1BuyAmountQuote {
            expected_amount_b: 1_000_000,
            guaranteed_amount_b: 900_000,
        };

        assert_eq!(
            select_bonk_usd1_buy_amount_from_sol_quote("sol_only", 2_000_000, &quote),
            900_000
        );
        assert_eq!(
            select_bonk_usd1_buy_amount_from_sol_quote("prefer_usd1_else_topup", 1_500_000, &quote),
            1_000_000
        );
        assert_eq!(
            select_bonk_usd1_buy_amount_from_sol_quote("prefer_usd1_else_topup", 950_000, &quote),
            950_000
        );
        assert_eq!(
            select_bonk_usd1_buy_amount_from_sol_quote("prefer_usd1_else_topup", 500_000, &quote),
            900_000
        );
    }

    #[test]
    fn usd1_topup_gross_input_covers_wrapper_fee() {
        let net_lamports = 1_000_000_000;
        let gross_lamports =
            gross_lamports_for_net_after_wrapper_fee(net_lamports, 10).expect("gross input");
        let fee_lamports = estimate_sol_in_fee_lamports(gross_lamports, 10);

        assert!(gross_lamports > net_lamports);
        assert!(gross_lamports - fee_lamports >= net_lamports);
    }

    fn push_test_pubkey(bytes: &mut Vec<u8>, value: &Pubkey) {
        bytes.extend_from_slice(value.as_ref());
    }

    fn encode_test_cpmm_pool(token_0_mint: &Pubkey, token_1_mint: &Pubkey) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&[0u8; 8]);
        for _ in 0..5 {
            push_test_pubkey(&mut data, &Pubkey::new_unique());
        }
        push_test_pubkey(&mut data, token_0_mint);
        push_test_pubkey(&mut data, token_1_mint);
        push_test_pubkey(&mut data, &spl_token::id());
        push_test_pubkey(&mut data, &spl_token::id());
        push_test_pubkey(&mut data, &Pubkey::new_unique());
        data.push(0);
        data.push(1);
        data.push(9);
        data.push(9);
        data.push(6);
        data.extend_from_slice(&0u64.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        data.push(0);
        data.push(0);
        data.extend_from_slice(&[0u8; 6]);
        data.extend_from_slice(&0u64.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        data
    }

    fn encode_test_clmm_pool(mint_a: &Pubkey, mint_b: &Pubkey) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&[0u8; 8]);
        data.push(0);
        push_test_pubkey(&mut data, &Pubkey::new_unique());
        push_test_pubkey(&mut data, &Pubkey::new_unique());
        push_test_pubkey(&mut data, mint_a);
        push_test_pubkey(&mut data, mint_b);
        push_test_pubkey(&mut data, &Pubkey::new_unique());
        push_test_pubkey(&mut data, &Pubkey::new_unique());
        push_test_pubkey(&mut data, &Pubkey::new_unique());
        data.push(9);
        data.push(6);
        data.extend_from_slice(&60u16.to_le_bytes());
        data.extend_from_slice(&1u128.to_le_bytes());
        data.extend_from_slice(&1u128.to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&[0u8; 16 + 16]);
        data.extend_from_slice(&[0u8; 8 + 8]);
        data.extend_from_slice(&[0u8; 16 + 16 + 16 + 16]);
        data.push(0);
        data.extend_from_slice(&[0u8; 7]);
        data.extend_from_slice(&[0u8; 3 * 169]);
        data.extend_from_slice(&[0u8; 16 * 8]);
        data
    }

    #[test]
    fn standard_rpc_follow_tip_is_ignored_when_blank() {
        let tip_lamports =
            resolve_follow_tip_lamports("standard-rpc", "", "buy tip").expect("standard rpc tip");
        assert_eq!(tip_lamports, 0);
    }

    #[test]
    fn standard_rpc_follow_tip_is_ignored_even_when_present() {
        let tip_lamports = resolve_follow_tip_lamports("standard-rpc", "0.01", "sell tip")
            .expect("standard rpc tip");
        assert_eq!(tip_lamports, 0);
    }

    #[test]
    fn jito_follow_tip_preserves_non_blank_tip_value() {
        let tip_lamports =
            resolve_follow_tip_lamports("jito-bundle", "0.01", "buy tip").expect("jito tip");
        assert!(tip_lamports > 0);
    }

    #[test]
    fn hellomoon_follow_tip_requires_at_least_point_zero_zero_one_sol() {
        assert_eq!(
            resolve_follow_tip_lamports("hellomoon", "0.001", "buy tip").expect("tip"),
            1_000_000
        );
        resolve_follow_tip_lamports("hellomoon", "", "buy tip").expect_err("empty tip");
        let error = resolve_follow_tip_lamports("hellomoon", "0.0001", "buy tip")
            .expect_err("sub-minimum tip");
        assert!(error.contains("0.001 SOL"), "unexpected: {error}");
    }

    #[test]
    fn slippage_percent_maps_to_expected_bps() {
        assert_eq!(slippage_bps_from_percent("20").expect("20%"), 2_000);
        assert_eq!(slippage_bps_from_percent("0.5").expect("0.5%"), 50);
        assert_eq!(slippage_bps_from_percent("99.99").expect("99.99%"), 9_999);
        assert_eq!(slippage_bps_from_percent("100").expect("100%"), 10_000);
        slippage_bps_from_percent("99.999").expect_err("too many decimals");
        slippage_bps_from_percent("100.01").expect_err("above 100%");
    }

    #[test]
    fn migrated_launch_pool_is_preferred_over_non_canonical_pool() {
        let migrated = BonkMarketCandidate {
            mode: "regular".to_string(),
            quote_asset: "sol".to_string(),
            quote_asset_label: "SOL".to_string(),
            creator: String::new(),
            platform_id: String::new(),
            config_id: String::new(),
            pool_id: "migrated".to_string(),
            real_quote_reserves: 0,
            complete: true,
            detection_source: "raydium-standard".to_string(),
            launch_migrate_pool: true,
            tvl: 10.0,
            pool_type: "Standard".to_string(),
            launchpad_pool: None,
            raydium_pool: None,
        };
        let launchpad = BonkMarketCandidate {
            launch_migrate_pool: false,
            pool_type: "LaunchLab".to_string(),
            pool_id: "launchpad".to_string(),
            ..migrated.clone()
        };
        let candidates = vec![launchpad, migrated];
        let preferred =
            select_preferred_bonk_market_candidate(&candidates, "sol").expect("preferred");
        assert_eq!(preferred.pool_id, "migrated");
    }

    #[test]
    fn raydium_pools_response_accepts_nested_data_shape() {
        let payload: RaydiumPoolsResponse = serde_json::from_value(json!({
            "id": "resp",
            "success": true,
            "data": {
                "count": 1,
                "data": [
                    {
                        "id": "pool-a",
                        "price": 123.45,
                        "tvl": 999.0,
                        "type": "Standard",
                        "launchMigratePool": true,
                        "mintA": { "address": "So11111111111111111111111111111111111111112" },
                        "mintB": { "address": "mint-b" },
                        "config": { "id": "cfg" }
                    }
                ],
                "hasNextPage": false
            }
        }))
        .expect("nested response should decode");
        assert_eq!(payload.data.len(), 1);
        assert_eq!(payload.data[0].id, "pool-a");
        assert!(payload.data[0].launch_migrate_pool);
        assert_eq!(
            payload.data[0].mint_a.address,
            "So11111111111111111111111111111111111111112"
        );
    }

    #[test]
    fn migrated_raydium_market_cap_avoids_u128_overflow() {
        let pool = RaydiumPoolInfo {
            id: "pool-a".to_string(),
            price: 105_103.454_806_931_16,
            tvl: 999.0,
            pool_type: "Standard".to_string(),
            launch_migrate_pool: true,
            mint_a: RaydiumTokenAddress {
                address: "So11111111111111111111111111111111111111112".to_string(),
            },
            mint_b: RaydiumTokenAddress {
                address: "HtTYHz1Kf3rrQo6AqDLmss7gq5WrkWAaXn3tupUZbonk".to_string(),
            },
            config: None,
        };
        let market_cap = market_cap_from_raydium_pool_price(
            &pool,
            999_866_905_447_231,
            6,
            &bonk_quote_asset_config("sol"),
        )
        .expect("market cap");
        assert_eq!(market_cap, 9_513_168_784_831);
    }

    #[test]
    fn requested_quote_asset_breaks_ties_between_migrated_pools() {
        let sol = BonkMarketCandidate {
            mode: "regular".to_string(),
            quote_asset: "sol".to_string(),
            quote_asset_label: "SOL".to_string(),
            creator: String::new(),
            platform_id: String::new(),
            config_id: String::new(),
            pool_id: "sol".to_string(),
            real_quote_reserves: 0,
            complete: true,
            detection_source: "raydium-standard".to_string(),
            launch_migrate_pool: false,
            tvl: 25.0,
            pool_type: "Standard".to_string(),
            launchpad_pool: None,
            raydium_pool: None,
        };
        let usd1 = BonkMarketCandidate {
            quote_asset: "usd1".to_string(),
            quote_asset_label: "USD1".to_string(),
            pool_id: "usd1".to_string(),
            ..sol.clone()
        };
        let candidates = vec![sol, usd1];
        let preferred =
            select_preferred_bonk_market_candidate(&candidates, "usd1").expect("preferred");
        assert_eq!(preferred.pool_id, "usd1");
    }

    #[test]
    fn requested_quote_asset_rejects_more_liquid_wrong_quote_pool() {
        let sol = BonkMarketCandidate {
            mode: "regular".to_string(),
            quote_asset: "sol".to_string(),
            quote_asset_label: "SOL".to_string(),
            creator: String::new(),
            platform_id: String::new(),
            config_id: String::new(),
            pool_id: "sol".to_string(),
            real_quote_reserves: 0,
            complete: true,
            detection_source: "raydium-standard".to_string(),
            launch_migrate_pool: true,
            tvl: 1_000_000.0,
            pool_type: "Standard".to_string(),
            launchpad_pool: None,
            raydium_pool: None,
        };
        let usd1 = BonkMarketCandidate {
            quote_asset: "usd1".to_string(),
            quote_asset_label: "USD1".to_string(),
            pool_id: "usd1".to_string(),
            tvl: 10.0,
            ..sol.clone()
        };
        let candidates = vec![sol, usd1];
        let preferred =
            select_preferred_bonk_market_candidate(&candidates, "usd1").expect("preferred");
        assert_eq!(preferred.pool_id, "usd1");
    }

    #[test]
    fn requested_quote_asset_returns_none_when_candidate_quote_is_missing() {
        let sol = BonkMarketCandidate {
            mode: "regular".to_string(),
            quote_asset: "sol".to_string(),
            quote_asset_label: "SOL".to_string(),
            creator: String::new(),
            platform_id: String::new(),
            config_id: String::new(),
            pool_id: "sol".to_string(),
            real_quote_reserves: 0,
            complete: true,
            detection_source: "raydium-standard".to_string(),
            launch_migrate_pool: true,
            tvl: 25.0,
            pool_type: "Standard".to_string(),
            launchpad_pool: None,
            raydium_pool: None,
        };
        assert!(select_preferred_bonk_market_candidate(&[sol], "usd1").is_none());
    }

    #[test]
    fn prelaunch_follow_sell_override_requires_canonical_launchpad_pool_id() {
        let mint = Pubkey::new_unique();
        let canonical_pool_id =
            bonk_canonical_pool_id_for_mint("sol", &mint).expect("canonical pool id");

        assert!(
            should_use_prelaunch_follow_sell_override(
                "sol",
                &mint,
                Some(1_000),
                Some(&canonical_pool_id),
                Some("regular"),
                Some(&Pubkey::new_unique().to_string()),
            )
            .expect("canonical pool should be accepted")
        );
        assert!(
            !should_use_prelaunch_follow_sell_override(
                "sol",
                &mint,
                Some(1_000),
                Some(&Pubkey::new_unique().to_string()),
                Some("regular"),
                Some(&Pubkey::new_unique().to_string()),
            )
            .expect("raydium pool id should be rejected")
        );
    }

    #[test]
    fn classify_bonk_pool_address_accepts_cpmm_owner() {
        let mint = Pubkey::new_unique();
        let sol = Pubkey::from_str(BONK_SOL_QUOTE_MINT).expect("sol mint");
        let data = encode_test_cpmm_pool(&mint, &sol);

        let classified = classify_bonk_pool_address(
            "pool-1",
            &bonk_cpmm_program_id().expect("cpmm program"),
            &data,
        )
        .expect("classification")
        .expect("cpmm classification");

        assert_eq!(classified.mint, mint.to_string());
        assert_eq!(classified.pool_id, "pool-1");
        assert_eq!(classified.family, "raydium");
        assert_eq!(classified.quote_asset, "sol");
    }

    #[test]
    fn classify_bonk_pool_address_accepts_clmm_owner() {
        let usd1 = Pubkey::from_str(BONK_USD1_QUOTE_MINT).expect("usd1 mint");
        let mint = Pubkey::new_unique();
        let data = encode_test_clmm_pool(&usd1, &mint);

        let classified = classify_bonk_pool_address(
            "pool-2",
            &bonk_clmm_program_id().expect("clmm program"),
            &data,
        )
        .expect("classification")
        .expect("clmm classification");

        assert_eq!(classified.mint, mint.to_string());
        assert_eq!(classified.pool_id, "pool-2");
        assert_eq!(classified.family, "raydium");
        assert_eq!(classified.quote_asset, "usd1");
    }

    #[test]
    fn classify_bonk_pool_address_rejects_unrelated_raydium_owner() {
        let mint = Pubkey::new_unique();
        let sol = Pubkey::from_str(BONK_SOL_QUOTE_MINT).expect("sol mint");
        let data = encode_test_cpmm_pool(&mint, &sol);
        let unrelated_raydium_owner =
            Pubkey::from_str("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8")
                .expect("raydium amm owner");

        let classified =
            classify_bonk_pool_address("pool-3", &unrelated_raydium_owner, &data).expect("result");

        assert!(classified.is_none());
    }

    #[test]
    fn bonk_launch_config_id_matches_raydium_sdk_pda_layout() {
        assert_eq!(
            bonk_launch_config_id("sol").expect("sol config"),
            "6s1xP3hpbAfFoNtUNF8mfHsjr2Bd97JxFJRWLbL6aHuX"
        );
        assert_eq!(
            bonk_launch_config_id("usd1").expect("usd1 config"),
            "EPiZbnrThjyLnoQ6QQzkxeFqyL5uyg9RzNHHAudUPxBz"
        );
    }

    #[test]
    fn parse_raydium_launch_configs_payload_accepts_nested_api_shape() {
        let configs = parse_raydium_launch_configs_payload(json!({
            "success": true,
            "data": {
                "data": [{
                    "key": { "pubKey": "6s1xP3hpbAfFoNtUNF8mfHsjr2Bd97JxFJRWLbL6aHuX" },
                    "defaultParams": {
                        "supplyInit": "1000",
                        "totalFundRaisingB": "2000",
                        "totalSellA": "3000"
                    }
                }]
            }
        }))
        .expect("configs");
        assert_eq!(configs.len(), 1);
        assert_eq!(
            configs[0].key.pubkey,
            "6s1xP3hpbAfFoNtUNF8mfHsjr2Bd97JxFJRWLbL6aHuX"
        );
        assert_eq!(configs[0].default_params.supply_init, "1000");
    }

    fn test_launch_defaults() -> BonkLaunchDefaults {
        BonkLaunchDefaults {
            supply: BigUint::from(1_000_000_000u64),
            total_fund_raising_b: BigUint::from(1_000_000_000u64),
            quote: bonk_quote_asset_config("sol"),
            trade_fee_rate: BigUint::ZERO,
            platform_fee_rate: BigUint::ZERO,
            creator_fee_rate: BigUint::ZERO,
            curve_type: 1,
            pool: BonkCurvePoolState {
                total_sell_a: BigUint::from(500_000_000u64),
                virtual_a: BigUint::from(500_000_000u64),
                virtual_b: BigUint::from(1_000_000_000u64),
                real_a: BigUint::ZERO,
                real_b: BigUint::ZERO,
            },
        }
    }

    fn test_usd1_route_setup() -> BonkUsd1RouteSetup {
        let tick_spacing = 60;
        let start_tick_index = 0;
        let mut ticks = Vec::new();
        for index in 0..BONK_CLMM_TICK_ARRAY_SIZE {
            let tick = start_tick_index + index * tick_spacing;
            ticks.push(BonkClmmTick {
                tick,
                liquidity_net: 0,
                liquidity_gross: if matches!(index, 0 | 3) {
                    BigUint::from(1u8)
                } else {
                    BigUint::ZERO
                },
            });
        }
        let sqrt_price_x64 = bonk_sqrt_price_from_tick(120).expect("current sqrt price");
        BonkUsd1RouteSetup {
            pool_id: Pubkey::new_unique(),
            program_id: Pubkey::new_unique(),
            amm_config: Pubkey::new_unique(),
            mint_a: bonk_quote_mint("sol").expect("sol mint"),
            mint_b: bonk_quote_mint("usd1").expect("usd1 mint"),
            vault_a: Pubkey::new_unique(),
            vault_b: Pubkey::new_unique(),
            observation_id: Pubkey::new_unique(),
            tick_spacing,
            trade_fee_rate: 2_500,
            sqrt_price_x64: sqrt_price_x64.clone(),
            liquidity: BigUint::from(1_000_000_000_000u64),
            tick_current: 120,
            mint_a_decimals: 9,
            mint_b_decimals: 6,
            current_price: bonk_sqrt_price_x64_to_price(&sqrt_price_x64, 9, 6)
                .expect("current price"),
            tick_arrays_desc: vec![start_tick_index],
            tick_arrays_asc: vec![start_tick_index],
            tick_arrays: HashMap::from([(
                start_tick_index,
                BonkClmmTickArray {
                    start_tick_index,
                    ticks,
                },
            )]),
        }
    }

    #[test]
    fn native_bonk_sol_quote_matches_fixed_price_defaults() {
        let quote = build_native_bonk_quote_from_defaults(&test_launch_defaults(), "sol", "0.25")
            .expect("quote");
        assert_eq!(quote.estimatedTokens, "125");
        assert_eq!(quote.estimatedSol, "0.25");
        assert_eq!(quote.estimatedQuoteAmount, "0.25");
        assert_eq!(quote.quoteAsset, "sol");
        assert_eq!(quote.quoteAssetLabel, "SOL");
        assert_eq!(quote.estimatedSupplyPercent, "12.5");
    }

    #[test]
    fn native_bonk_token_quote_matches_fixed_price_defaults() {
        let quote = build_native_bonk_quote_from_defaults(&test_launch_defaults(), "tokens", "125")
            .expect("quote");
        assert_eq!(quote.estimatedTokens, "125");
        assert_eq!(quote.estimatedSol, "0.25");
        assert_eq!(quote.estimatedQuoteAmount, "0.25");
        assert_eq!(quote.quoteAsset, "sol");
        assert_eq!(quote.estimatedSupplyPercent, "12.5");
    }

    #[test]
    fn native_bonk_fixed_price_follow_sell_matches_expected_quote() {
        let defaults = test_launch_defaults();
        let context = build_prelaunch_bonk_pool_context(
            &defaults,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            "regular",
        )
        .expect("context");
        let min_amount_b = bonk_follow_sell_amounts(&context, 125_000_000, 0).expect("sell quote");
        assert_eq!(min_amount_b, 250_000_000);
    }

    #[test]
    fn atomic_usd1_buy_final_min_uses_expected_intermediate_output() {
        let setup = test_usd1_route_setup();
        let route_quote = bonk_quote_usd1_from_exact_sol_input(
            &setup,
            &BigUint::from(1_000_000u64),
            BONK_USD1_ROUTE_SLIPPAGE_BPS,
        )
        .expect("USD1 route quote");
        let expected_usd1 =
            biguint_to_u64(&route_quote.expected_out, "expected USD1").expect("expected USD1");
        let min_usd1 = biguint_to_u64(&route_quote.min_out, "minimum USD1").expect("minimum USD1");
        assert!(expected_usd1 > min_usd1);

        let mut defaults = test_launch_defaults();
        defaults.quote = bonk_quote_asset_config("usd1");
        let context = build_prelaunch_bonk_pool_context(
            &defaults,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            "regular",
        )
        .expect("context");
        let (_, expected_based_min) =
            bonk_follow_buy_amounts(&context, expected_usd1, 100).expect("expected-based buy");
        let (_, min_based_min) =
            bonk_follow_buy_amounts(&context, min_usd1, 100).expect("min-based buy");

        assert!(expected_based_min > min_based_min);
    }

    #[test]
    fn atomic_usd1_sell_unwind_min_uses_expected_intermediate_output() {
        let owner = Pubkey::new_unique();
        let usd1_account = Pubkey::new_unique();
        let setup = test_usd1_route_setup();
        let min_usd1_out = 950_000;
        let expected_usd1_out = 1_000_000;
        let fee_bps = 10;
        let first_leg_ix = Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![AccountMeta::new(usd1_account, false)],
            data: vec![0; 24],
        };

        let route_instructions = build_bonk_dynamic_usd1_sell_to_sol_route(
            &owner,
            &usd1_account,
            first_leg_ix,
            123_456,
            min_usd1_out,
            expected_usd1_out,
            &setup,
            fee_bps,
        )
        .expect("route instructions");
        let request = ExecuteSwapRouteRequest::try_from_slice(&route_instructions[0].data[1..])
            .expect("decode wrapper request");
        let expected_unwind_quote = bonk_quote_sol_from_exact_usd1_input(
            &setup,
            &BigUint::from(expected_usd1_out),
            BONK_USD1_ROUTE_SLIPPAGE_BPS,
        )
        .expect("expected unwind quote");
        let min_based_unwind_quote = bonk_quote_sol_from_exact_usd1_input(
            &setup,
            &BigUint::from(min_usd1_out),
            BONK_USD1_ROUTE_SLIPPAGE_BPS,
        )
        .expect("minimum unwind quote");
        let expected_min_out = biguint_to_u64(
            &expected_unwind_quote.min_out,
            "expected unwind minimum output",
        )
        .expect("expected unwind minimum output");
        let min_based_out = biguint_to_u64(
            &min_based_unwind_quote.min_out,
            "min-based unwind minimum output",
        )
        .expect("min-based unwind minimum output");
        let expected_min_net = expected_min_out
            .checked_sub(estimate_sol_in_fee_lamports(expected_min_out, fee_bps))
            .expect("expected fee");
        let min_based_net = min_based_out
            .checked_sub(estimate_sol_in_fee_lamports(min_based_out, fee_bps))
            .expect("min-based fee");

        assert_eq!(request.min_net_output, expected_min_net);
        assert!(request.min_net_output > min_based_net);
        assert_eq!(
            request.legs[1].input_source,
            SwapLegInputSource::PreviousTokenDelta
        );
        assert_eq!(request.legs[1].input_amount, expected_usd1_out);
        assert_eq!(
            u64::from_le_bytes(
                request.legs[1].ix_data[8..16]
                    .try_into()
                    .expect("amount bytes")
            ),
            expected_usd1_out
        );
        assert_eq!(
            u64::from_le_bytes(
                request.legs[1].ix_data[16..24]
                    .try_into()
                    .expect("min bytes")
            ),
            expected_min_out
        );
    }

    #[test]
    fn advancing_prelaunch_bonk_pool_context_reduces_next_buy_quote() {
        let defaults = test_launch_defaults();
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let context = build_prelaunch_bonk_pool_context(&defaults, &mint, &creator, "regular")
            .expect("context");
        let base_quote =
            bonk_follow_buy_quote_details(&context, 800_000_000, 0).expect("base quote");
        let advanced = advance_prelaunch_bonk_pool_context_after_buy(&context, 800_000_000, 0)
            .expect("advanced context");
        let next_quote =
            bonk_follow_buy_quote_details(&advanced, 800_000_000, 0).expect("next quote");

        assert!(advanced.pool.real_a > context.pool.real_a);
        assert!(advanced.pool.real_b > context.pool.real_b);
        assert!(next_quote.amount_a < base_quote.amount_a);
    }

    #[test]
    fn bonk_follow_tx_format_is_shared_alt_only() {
        assert_eq!(
            select_bonk_native_tx_format("legacy"),
            NativeBonkTxFormat::V0
        );
        assert_eq!(select_bonk_native_tx_format("v0"), NativeBonkTxFormat::V0);
        assert_eq!(select_bonk_native_tx_format("auto"), NativeBonkTxFormat::V0);
        assert_eq!(
            select_bonk_native_tx_format("v0-alt"),
            NativeBonkTxFormat::V0
        );
    }

    #[test]
    fn rewrite_missing_bonk_instruction_signers_rebinds_ephemeral_signer_accounts() {
        let owner = Keypair::new();
        let missing = Pubkey::new_unique();
        let mut instructions = vec![Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![
                AccountMeta::new_readonly(owner.pubkey(), true),
                AccountMeta::new(missing, true),
                AccountMeta::new(Pubkey::new_unique(), false),
            ],
            data: vec![],
        }];
        let generated = rewrite_missing_bonk_instruction_signers(
            &owner.pubkey(),
            &mut instructions,
            &[],
            &[missing],
        )
        .expect("allowed missing signer should be rewritten");
        assert_eq!(generated.len(), 1);
        assert_ne!(generated[0].pubkey(), missing);
        assert_eq!(instructions[0].accounts[1].pubkey, generated[0].pubkey());
        assert!(instructions[0].accounts[1].is_signer);
    }

    #[test]
    fn rewrite_missing_bonk_instruction_signers_rejects_unexpected_signers() {
        let owner = Keypair::new();
        let unexpected = Pubkey::new_unique();
        let mut instructions = vec![Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![
                AccountMeta::new_readonly(owner.pubkey(), true),
                AccountMeta::new(unexpected, true),
            ],
            data: vec![],
        }];
        let error =
            rewrite_missing_bonk_instruction_signers(&owner.pubkey(), &mut instructions, &[], &[])
                .expect_err("unexpected signer should fail");
        assert!(error.contains("unexpected signer"));
    }

    #[test]
    fn atomic_bonk_usd1_follow_buy_uses_merged_child_compute_budget() {
        assert_eq!(
            configured_atomic_bonk_usd1_follow_buy_compute_unit_limit(Some(120_000), Some(120_000)),
            280_000
        );
        assert!(
            configured_atomic_bonk_usd1_follow_buy_compute_unit_limit(None, None)
                >= configured_default_follow_up_compute_unit_limit()
        );
        assert_eq!(
            configured_atomic_bonk_usd1_follow_buy_compute_unit_limit(Some(50_000), Some(60_000)),
            configured_default_follow_up_compute_unit_limit()
        );
    }

    #[test]
    fn bonk_usd1_dynamic_buys_use_launch_grade_compute_budget() {
        assert_eq!(
            configured_default_bonk_usd1_dynamic_buy_compute_unit_limit(),
            configured_default_launch_compute_unit_limit()
        );
        assert!(configured_default_bonk_usd1_dynamic_buy_compute_unit_limit() >= 340_000);
    }

    #[test]
    fn bonk_sells_use_larger_compute_budget() {
        assert_eq!(configured_default_bonk_sell_compute_unit_limit(), 280_000);
        assert_eq!(
            configured_bonk_sell_compute_unit_limit("sol", "sol"),
            280_000
        );
        assert!(configured_bonk_sell_compute_unit_limit("usd1", "sol") >= 280_000);
        assert!(
            configured_bonk_sell_compute_unit_limit("usd1", "sol")
                >= configured_default_bonk_sell_compute_unit_limit()
        );
    }

    #[test]
    fn atomic_bonk_tx_envelope_orders_price_limit_core_then_tip() {
        let owner = Pubkey::new_unique();
        let tip_account = Pubkey::new_unique();
        let core_instruction = Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![AccountMeta::new(owner, true)],
            data: vec![42],
        };
        let instructions = build_bonk_atomic_tx_instructions(
            vec![core_instruction.clone()],
            &NativeBonkTxConfig {
                compute_unit_limit: 500_000,
                compute_unit_price_micro_lamports: 1_234,
                tip_lamports: 5_000,
                tip_account: tip_account.to_string(),
            },
            &owner,
            false,
        )
        .expect("atomic instructions");
        assert_eq!(instructions.len(), 4);
        assert_eq!(
            instructions[0].program_id,
            compute_budget_program_id().expect("compute budget")
        );
        assert_eq!(instructions[0].data.first().copied(), Some(3));
        assert_eq!(
            instructions[1].program_id,
            compute_budget_program_id().expect("compute budget")
        );
        assert_eq!(instructions[1].data.first().copied(), Some(2));
        assert_eq!(instructions[2].program_id, core_instruction.program_id);
        assert_eq!(
            instructions[3].program_id,
            solana_system_interface::program::ID
        );
    }

    #[test]
    fn atomic_bonk_filter_drops_budget_tip_and_memo() {
        let owner = Pubkey::new_unique();
        let tip_account = Pubkey::new_unique();
        let core_instruction = Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![AccountMeta::new(owner, true)],
            data: vec![7],
        };
        let filtered = filter_atomic_bonk_instructions(
            vec![
                build_compute_unit_limit_instruction(400_000).expect("cu"),
                build_bonk_uniqueness_memo_instruction("follow-buy-atomic").expect("memo"),
                solana_system_interface::instruction::transfer(&owner, &tip_account, 5_000),
                core_instruction.clone(),
            ],
            &owner,
            &NativeBonkTxConfig {
                compute_unit_limit: 400_000,
                compute_unit_price_micro_lamports: 0,
                tip_lamports: 5_000,
                tip_account: tip_account.to_string(),
            },
        );
        assert_eq!(filtered, vec![core_instruction]);
    }

    #[test]
    fn bonk_launch_bundle_tip_only_applies_to_last_transaction_when_requested() {
        let config = NativeBonkTxConfig {
            compute_unit_limit: 400_000,
            compute_unit_price_micro_lamports: 1_000,
            tip_lamports: 9_999,
            tip_account: Pubkey::new_unique().to_string(),
        };
        let first = bonk_bundle_tx_config_for_index(&config, 0, 3, true);
        let last = bonk_bundle_tx_config_for_index(&config, 2, 3, true);
        assert_eq!(first.tip_lamports, 0);
        assert!(first.tip_account.is_empty());
        assert_eq!(last.tip_lamports, 9_999);
        assert_eq!(last.tip_account, config.tip_account);
    }

    #[test]
    fn native_bonk_launch_initialize_v2_instruction_uses_expected_accounts() {
        let mint = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let instruction = build_bonk_initialize_v2_instruction(
            &owner,
            &mint,
            "regular",
            "Launch Token",
            "LAUNCH",
            "https://example.invalid/meta.json",
            &test_launch_defaults(),
        )
        .expect("initialize");
        assert_eq!(
            instruction.program_id,
            bonk_launchpad_program_id().expect("launchpad")
        );
        assert_eq!(&instruction.data[..8], &BONK_INITIALIZE_V2_DISCRIMINATOR);
        assert_eq!(instruction.accounts.len(), 18);
        assert_eq!(instruction.accounts[0].pubkey, owner);
        assert_eq!(instruction.accounts[6].pubkey, mint);
        assert!(instruction.accounts[6].is_signer);
        assert_eq!(
            instruction.accounts[10].pubkey,
            bonk_metadata_account_pda(&mint).expect("metadata pda")
        );
    }

    #[test]
    fn native_bonk_launch_dev_buy_sol_uses_owner_wsol_ata_path() {
        let owner = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let context =
            build_prelaunch_bonk_pool_context(&test_launch_defaults(), &mint, &owner, "regular")
                .expect("context");
        let instructions = build_native_bonk_launch_dev_buy_instructions(
            &owner,
            &mint,
            &context,
            &BigUint::from(250_000_000u64),
            0,
            None,
            true,
        )
        .expect("instructions");
        assert_eq!(instructions.len(), 5);
        assert_eq!(
            instructions[0].program_id,
            spl_associated_token_account::id()
        );
        assert_eq!(
            instructions[1].program_id,
            spl_associated_token_account::id()
        );
        assert_eq!(
            instructions[2].program_id,
            solana_system_interface::program::ID
        );
        assert_eq!(instructions[3].program_id, spl_token::id());
        assert_eq!(
            instructions[4].program_id,
            bonk_launchpad_program_id().expect("launchpad")
        );
    }

    #[test]
    fn native_bonk_usd1_topup_swap_instruction_matches_pinned_clmm_layout() {
        let owner = Pubkey::new_unique();
        let input_account = Pubkey::new_unique();
        let output_account = Pubkey::new_unique();
        let instruction = build_bonk_clmm_swap_exact_in_instruction(
            &owner,
            &input_account,
            &output_account,
            123_456_789,
            45_000_000,
            &[0, -3_600],
        )
        .expect("swap instruction");
        assert_eq!(
            instruction.program_id,
            bonk_clmm_program_id().expect("clmm program")
        );
        assert_eq!(instruction.accounts[0].pubkey, owner);
        assert_eq!(instruction.accounts[3].pubkey, input_account);
        assert_eq!(instruction.accounts[4].pubkey, output_account);
        assert_eq!(
            instruction.accounts[13].pubkey,
            bonk_clmm_ex_bitmap_pda(
                &Pubkey::from_str(BONK_PINNED_USD1_ROUTE_POOL_ID).expect("pool"),
            )
            .expect("bitmap")
        );
        assert_eq!(
            instruction.accounts[14].pubkey,
            bonk_derive_clmm_tick_array_address(
                &bonk_clmm_program_id().expect("clmm"),
                &Pubkey::from_str(BONK_PINNED_USD1_ROUTE_POOL_ID).expect("pool"),
                0,
            )
        );
        assert_eq!(
            instruction.accounts[15].pubkey,
            bonk_derive_clmm_tick_array_address(
                &bonk_clmm_program_id().expect("clmm"),
                &Pubkey::from_str(BONK_PINNED_USD1_ROUTE_POOL_ID).expect("pool"),
                -3_600,
            )
        );
        assert_eq!(&instruction.data[..8], &BONK_CLMM_SWAP_DISCRIMINATOR);
        assert_eq!(
            u64::from_le_bytes(instruction.data[8..16].try_into().expect("amount in")),
            123_456_789
        );
        assert_eq!(
            u64::from_le_bytes(instruction.data[16..24].try_into().expect("min out")),
            45_000_000
        );
        assert_eq!(
            u128::from_le_bytes(instruction.data[24..40].try_into().expect("sqrt limit")),
            BONK_CLMM_MIN_SQRT_PRICE_X64_PLUS_ONE
        );
        assert_eq!(instruction.data[40], 1);
    }

    #[test]
    fn native_bonk_usd1_unwind_swap_instruction_marks_reverse_direction_as_exact_input() {
        let owner = Pubkey::new_unique();
        let input_account = Pubkey::new_unique();
        let output_account = Pubkey::new_unique();
        let instruction = build_bonk_clmm_swap_exact_in_instruction_with_assets(
            &owner,
            &input_account,
            &output_account,
            45_000_000,
            123_456_789,
            &[0, 3_600],
            "usd1",
            "sol",
        )
        .expect("swap instruction");
        assert_eq!(
            u128::from_le_bytes(instruction.data[24..40].try_into().expect("sqrt limit")),
            0
        );
        assert_eq!(
            instruction.data[40], 1,
            "reverse-direction exact-in swaps must still set is_base_input"
        );
    }

    #[test]
    fn clmm_swap_instruction_for_setup_uses_live_pool_keys() {
        let owner = Pubkey::new_unique();
        let input_account = Pubkey::new_unique();
        let output_account = Pubkey::new_unique();
        let setup = BonkUsd1RouteSetup {
            pool_id: Pubkey::new_unique(),
            program_id: Pubkey::new_unique(),
            amm_config: Pubkey::new_unique(),
            mint_a: Pubkey::new_unique(),
            mint_b: Pubkey::new_unique(),
            vault_a: Pubkey::new_unique(),
            vault_b: Pubkey::new_unique(),
            observation_id: Pubkey::new_unique(),
            tick_spacing: 60,
            trade_fee_rate: 2_500,
            sqrt_price_x64: BigUint::from(1u8),
            liquidity: BigUint::from(1u8),
            tick_current: 0,
            mint_a_decimals: 9,
            mint_b_decimals: 6,
            current_price: 1.0,
            tick_arrays_desc: vec![],
            tick_arrays_asc: vec![],
            tick_arrays: HashMap::new(),
        };
        let instruction = build_bonk_clmm_swap_exact_in_instruction_for_setup(
            &owner,
            &setup,
            &input_account,
            &output_account,
            1_000,
            900,
            &[],
            &setup.mint_b,
            &setup.mint_a,
        )
        .expect("swap instruction");

        assert_eq!(instruction.accounts[5].pubkey, setup.vault_b);
        assert_eq!(instruction.accounts[6].pubkey, setup.vault_a);
        assert_eq!(instruction.accounts[7].pubkey, setup.observation_id);
        assert_eq!(
            u128::from_le_bytes(instruction.data[24..40].try_into().expect("sqrt limit")),
            0
        );
    }

    #[test]
    fn clmm_zero_tick_sqrt_price_matches_q64() {
        assert_eq!(bonk_sqrt_price_from_tick(0).expect("sqrt"), bonk_clmm_q64());
    }

    #[test]
    fn clmm_exact_input_quote_applies_slippage_to_min_out() {
        let tick_spacing = 60;
        let start_tick_index = 0;
        let mut ticks = Vec::new();
        for index in 0..BONK_CLMM_TICK_ARRAY_SIZE {
            let tick = start_tick_index + index * tick_spacing;
            ticks.push(BonkClmmTick {
                tick,
                liquidity_net: 0,
                liquidity_gross: if index == 0 {
                    BigUint::from(1u8)
                } else {
                    BigUint::ZERO
                },
            });
        }
        let sqrt_price_x64 = bonk_sqrt_price_from_tick(120).expect("current sqrt price");
        let setup = BonkUsd1RouteSetup {
            pool_id: Pubkey::new_unique(),
            program_id: Pubkey::new_unique(),
            amm_config: Pubkey::new_unique(),
            mint_a: Pubkey::new_unique(),
            mint_b: Pubkey::new_unique(),
            vault_a: Pubkey::new_unique(),
            vault_b: Pubkey::new_unique(),
            observation_id: Pubkey::new_unique(),
            tick_spacing,
            trade_fee_rate: 2_500,
            sqrt_price_x64: sqrt_price_x64.clone(),
            liquidity: BigUint::from(1_000_000_000_000u64),
            tick_current: 120,
            mint_a_decimals: 9,
            mint_b_decimals: 6,
            current_price: bonk_sqrt_price_x64_to_price(&sqrt_price_x64, 9, 6)
                .expect("current price"),
            tick_arrays_desc: vec![start_tick_index],
            tick_arrays_asc: vec![start_tick_index],
            tick_arrays: HashMap::from([(
                start_tick_index,
                BonkClmmTickArray {
                    start_tick_index,
                    ticks,
                },
            )]),
        };
        let quote = bonk_quote_usd1_from_exact_sol_input(&setup, &BigUint::from(1_000_000u64), 50)
            .expect("quote");
        assert!(quote.expected_out > BigUint::ZERO);
        assert!(quote.expected_out > quote.min_out);
        assert_eq!(
            quote.min_out,
            (&quote.expected_out * BigUint::from(9_950u64)) / BigUint::from(10_000u64)
        );
    }

    #[test]
    fn merge_persisted_bonk_lookup_table_caches_keeps_entries_from_both_sources() {
        let merged = merge_persisted_bonk_lookup_table_caches([
            PersistedBonkLookupTableCache {
                tables: HashMap::from([(
                    "shared".to_string(),
                    PersistedBonkLookupTableEntry {
                        addresses: vec!["A".to_string()],
                        address_count: None,
                        content_hash: None,
                    },
                )]),
            },
            PersistedBonkLookupTableCache {
                tables: HashMap::from([(
                    "legacy".to_string(),
                    PersistedBonkLookupTableEntry {
                        addresses: vec!["B".to_string()],
                        address_count: None,
                        content_hash: None,
                    },
                )]),
            },
        ]);

        assert_eq!(merged.tables.len(), 2);
        assert_eq!(merged.tables["shared"].addresses, vec!["A".to_string()]);
        assert_eq!(merged.tables["legacy"].addresses, vec!["B".to_string()]);
    }
}
