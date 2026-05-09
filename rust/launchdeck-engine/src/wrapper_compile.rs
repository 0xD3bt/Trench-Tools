use std::{
    str::FromStr,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use borsh::{BorshDeserialize, BorshSerialize};
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_sdk::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{AddressLookupTableAccount, VersionedMessage, v0},
    pubkey::Pubkey,
    rent::Rent,
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use spl_associated_token_account::get_associated_token_address_with_program_id;

use crate::{
    compiled_transaction_signers,
    rpc::{CompiledTransaction, fetch_account_data},
};

const SHARED_SUPER_LOOKUP_TABLE: &str = "7CaMLcAuSskoeN7HoRwZjsSthU8sMwKqxtXkyMiMjuc";
const PACKET_LIMIT_BYTES: usize = 1232;
const MAX_FEE_BPS: u16 = 20;
pub const ABI_VERSION: u8 = 3;
#[allow(dead_code)]
const EXECUTE_FIXED_ACCOUNT_COUNT: u16 = 8;
pub const EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT: u16 = 8;
pub const EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT: u16 = 3;
pub const EXECUTE_SWAP_ROUTE_TOKEN_FEE_ACCOUNT_COUNT: u16 = 1;
const BPS_DENOMINATOR: u128 = 10_000;
const SPL_TOKEN_ACCOUNT_LEN: usize = 165;
const SYSTEM_CREATE_ACCOUNT_DISCRIMINATOR: u32 = 0;
const SYSTEM_TRANSFER_DISCRIMINATOR: u32 = 2;
const SPL_TOKEN_CLOSE_ACCOUNT_DISCRIMINATOR: u8 = 9;
const SPL_TOKEN_SYNC_NATIVE_DISCRIMINATOR: u8 = 17;
const SPL_TOKEN_INITIALIZE_ACCOUNT3_DISCRIMINATOR: u8 = 18;

const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const WRAPPER_PROGRAM_ID: &str = "TRENCHCfkCTud86C8ZC9kk2CFWJErYz4oZFaYttoxJF";
const WRAPPER_FEE_VAULT: &str = "7HKc2NAi2Q2ZG3eSN7VJrtBgGi7dNFAz9DLnPNDUncM2";
const CONFIG_SEED: &[u8] = b"config";
const ROUTE_WSOL_SEED: &[u8] = b"route-wsol";
pub const ROUTE_WSOL_LANE_COUNT: u8 = 8;
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
const RAYDIUM_CLMM_PROGRAM_ID: &str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";
const RAYDIUM_CPMM_PROGRAM_ID: &str = "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C";
const RAYDIUM_AMM_V4_PROGRAM_ID: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
const RAYDIUM_CLMM_SWAP_V2_DISCRIMINATOR: [u8; 8] = [43, 4, 237, 11, 26, 201, 30, 98];
const RAYDIUM_CPMM_SWAP_BASE_INPUT_DISCRIMINATOR: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];
const PUMP_BUY_DISCRIMINATOR: [u8; 8] = [102, 6, 61, 18, 1, 218, 235, 234];
const PUMP_BUY_EXACT_SOL_IN_DISCRIMINATOR: [u8; 8] = [56, 252, 116, 8, 158, 223, 205, 95];
const PUMP_SELL_DISCRIMINATOR: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];
const BONK_BUY_EXACT_IN_DISCRIMINATOR: [u8; 8] = [250, 234, 13, 123, 213, 156, 19, 236];
const BONK_SELL_EXACT_IN_DISCRIMINATOR: [u8; 8] = [149, 39, 222, 155, 211, 124, 152, 26];
const PUMP_AMM_BUY_EXACT_QUOTE_IN_DISCRIMINATOR: [u8; 8] = [198, 46, 21, 82, 180, 217, 232, 112];
const BAGS_SWAP_DISCRIMINATOR: [u8; 8] = [248, 198, 158, 145, 225, 117, 135, 200];

const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMP_AMM_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const MAYHEM_PROGRAM_ID: &str = "MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e";
const BONK_LAUNCHPAD_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";
const BAGS_DBC_PROGRAM_ID: &str = "dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN";
const BAGS_DAMM_V2_PROGRAM_ID: &str = "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG";
const WRAPPED_COMPUTE_UNIT_LIMIT_FLOOR: u32 = 280_000;
const WRAPPED_COMPUTE_UNIT_LIMIT_OVERHEAD: u32 = 60_000;
const SHARED_LOOKUP_TABLE_CACHE_TTL: Duration = Duration::from_secs(300);
#[allow(dead_code)]
const EXECUTE_DISCRIMINATOR: u8 = 1;
#[allow(dead_code)]
const EXECUTE_AMM_WSOL_DISCRIMINATOR: u8 = 7;
const EXECUTE_SWAP_ROUTE_DISCRIMINATOR: u8 = 8;

pub const ZEROED_WSOL_ATA_SENTINEL: Pubkey = Pubkey::new_from_array([0; 32]);
pub const SWAP_ROUTE_NO_PATCH_OFFSET: u16 = u16::MAX;

#[derive(Clone)]
struct CachedLookupTables {
    rpc_url: String,
    loaded_at: Instant,
    tables: Vec<AddressLookupTableAccount>,
}

static SHARED_LOOKUP_TABLE_CACHE: OnceLock<Mutex<Option<CachedLookupTables>>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum WrapperRouteKind {
    SolIn = 0,
    SolOut = 1,
    SolThrough = 2,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ExecuteRequest {
    pub version: u8,
    pub route_kind: WrapperRouteKind,
    pub fee_bps: u16,
    pub gross_sol_in_lamports: u64,
    pub min_net_output: u64,
    pub inner_accounts_offset: u16,
    pub inner_ix_data: Vec<u8>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum WsolAccountMode {
    CreateOrReuse = 0,
    ReuseOnly = 1,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ExecuteAmmWsolRequest {
    pub version: u8,
    pub route_kind: WrapperRouteKind,
    pub fee_bps: u16,
    pub gross_sol_in_lamports: u64,
    pub min_net_output: u64,
    pub inner_accounts_offset: u16,
    pub wsol_account_mode: WsolAccountMode,
    pub pda_wsol_lamports: u64,
    pub inner_wsol_account_index: u16,
    pub inner_ix_data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum SwapRouteDirection {
    Buy = 0,
    Sell = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum SwapRouteSettlement {
    Token = 0,
    NativeSol = 1,
    Wsol = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum SwapRouteMode {
    SolIn = 0,
    SolOut = 1,
    TokenOnly = 2,
    Mixed = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum SwapRouteFeeMode {
    SolPre = 0,
    NativeSolPost = 1,
    WsolPost = 2,
    TokenPre = 3,
    TokenPost = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum SwapLegInputSource {
    Fixed = 0,
    GrossSolNetOfFee = 1,
    PreviousTokenDelta = 2,
    GrossTokenNetOfFee = 3,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SwapRouteLeg {
    pub program_account_index: u16,
    pub accounts_start: u16,
    pub accounts_len: u16,
    pub input_source: SwapLegInputSource,
    pub input_amount: u64,
    pub input_patch_offset: u16,
    pub output_account_index: u16,
    pub ix_data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ExecuteSwapRouteRequest {
    pub version: u8,
    pub route_mode: SwapRouteMode,
    pub direction: SwapRouteDirection,
    pub settlement: SwapRouteSettlement,
    pub fee_mode: SwapRouteFeeMode,
    pub wsol_lane: u8,
    pub fee_bps: u16,
    pub gross_sol_in_lamports: u64,
    pub gross_token_in_amount: u64,
    pub min_net_output: u64,
    pub route_accounts_offset: u16,
    pub intermediate_account_index: u16,
    pub token_fee_account_index: u16,
    pub legs: Vec<SwapRouteLeg>,
}

#[derive(Debug, Clone, Copy)]
pub struct LaunchdeckWrapRequest {
    pub route_kind: WrapperRouteKind,
    pub fee_bps: u16,
    pub gross_sol_in_lamports: u64,
    pub infer_gross_sol_in_from_inner: bool,
    pub min_net_output: u64,
    pub select_first_allowlisted_venue_instruction: bool,
    pub select_last_allowlisted_venue_instruction: bool,
}

fn parse_pubkey(value: &str, label: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|error| format!("Invalid {label}: {error}"))
}

fn wrapper_program_id() -> Pubkey {
    parse_pubkey(WRAPPER_PROGRAM_ID, "wrapper program id").expect("wrapper program id")
}

fn compute_budget_program_id() -> Pubkey {
    parse_pubkey(COMPUTE_BUDGET_PROGRAM_ID, "compute budget program id")
        .expect("compute budget program id")
}

pub fn wrapper_fee_vault() -> Pubkey {
    parse_pubkey(WRAPPER_FEE_VAULT, "wrapper fee vault").expect("wrapper fee vault")
}

fn token_program_id() -> Pubkey {
    parse_pubkey(TOKEN_PROGRAM_ID, "token program id").expect("token program id")
}

fn wsol_mint() -> Pubkey {
    parse_pubkey(WSOL_MINT, "WSOL mint").expect("WSOL mint")
}

fn instructions_sysvar_id() -> Pubkey {
    solana_sdk::sysvar::instructions::ID
}

fn system_program_id() -> Pubkey {
    solana_system_interface::program::ID
}

fn config_pda() -> Pubkey {
    Pubkey::find_program_address(&[CONFIG_SEED], &wrapper_program_id()).0
}

pub fn route_wsol_pda(user: &Pubkey, lane: u8) -> Pubkey {
    Pubkey::find_program_address(
        &[ROUTE_WSOL_SEED, user.as_ref(), &[lane]],
        &wrapper_program_id(),
    )
    .0
}

fn swap_route_uses_wsol(request: &ExecuteSwapRouteRequest) -> bool {
    matches!(
        (request.route_mode, request.fee_mode),
        (SwapRouteMode::SolOut, SwapRouteFeeMode::WsolPost)
            | (SwapRouteMode::Mixed, SwapRouteFeeMode::SolPre)
            | (SwapRouteMode::Mixed, SwapRouteFeeMode::WsolPost)
    )
}

fn swap_route_uses_token_fee(request: &ExecuteSwapRouteRequest) -> bool {
    matches!(
        request.fee_mode,
        SwapRouteFeeMode::TokenPre | SwapRouteFeeMode::TokenPost
    )
}

fn rent_sysvar_id() -> Pubkey {
    solana_sdk::sysvar::rent::ID
}

fn memo_program_id() -> Pubkey {
    parse_pubkey(MEMO_PROGRAM_ID, "memo program id").expect("memo program id")
}

fn raydium_clmm_program_id() -> Pubkey {
    parse_pubkey(RAYDIUM_CLMM_PROGRAM_ID, "Raydium CLMM program id").expect("Raydium CLMM")
}

fn raydium_cpmm_program_id() -> Pubkey {
    parse_pubkey(RAYDIUM_CPMM_PROGRAM_ID, "Raydium CPMM program id").expect("Raydium CPMM")
}

fn raydium_amm_v4_program_id() -> Pubkey {
    parse_pubkey(RAYDIUM_AMM_V4_PROGRAM_ID, "Raydium AMM v4 program id").expect("Raydium AMM v4")
}

fn bonk_launchpad_program_id() -> Pubkey {
    parse_pubkey(BONK_LAUNCHPAD_PROGRAM_ID, "Bonk Launchpad program id")
        .expect("Bonk Launchpad program id")
}

fn pump_amm_program_id() -> Pubkey {
    parse_pubkey(PUMP_AMM_PROGRAM_ID, "Pump AMM program id").expect("Pump AMM program id")
}

fn bags_dbc_program_id() -> Pubkey {
    parse_pubkey(BAGS_DBC_PROGRAM_ID, "Bags DBC program id").expect("Bags DBC program id")
}

fn bags_damm_v2_program_id() -> Pubkey {
    parse_pubkey(BAGS_DAMM_V2_PROGRAM_ID, "Bags DAMM v2 program id")
        .expect("Bags DAMM v2 program id")
}

fn allowed_inner_programs() -> Vec<Pubkey> {
    [
        PUMP_PROGRAM_ID,
        PUMP_AMM_PROGRAM_ID,
        MAYHEM_PROGRAM_ID,
        BONK_LAUNCHPAD_PROGRAM_ID,
        RAYDIUM_CLMM_PROGRAM_ID,
        RAYDIUM_CPMM_PROGRAM_ID,
        RAYDIUM_AMM_V4_PROGRAM_ID,
        BAGS_DBC_PROGRAM_ID,
        BAGS_DAMM_V2_PROGRAM_ID,
    ]
    .into_iter()
    .filter_map(|value| Pubkey::from_str(value).ok())
    .collect()
}

pub async fn load_shared_lookup_tables(
    rpc_url: &str,
) -> Result<Vec<AddressLookupTableAccount>, String> {
    let cache = SHARED_LOOKUP_TABLE_CACHE.get_or_init(|| Mutex::new(None));
    if let Some(cached) = cache
        .lock()
        .map_err(|_| "LaunchDeck wrapper ALT cache lock poisoned".to_string())?
        .as_ref()
        .filter(|entry| {
            entry.rpc_url == rpc_url && entry.loaded_at.elapsed() <= SHARED_LOOKUP_TABLE_CACHE_TTL
        })
        .cloned()
    {
        return Ok(cached.tables);
    }

    refresh_shared_lookup_tables(rpc_url).await
}

pub async fn refresh_shared_lookup_tables(
    rpc_url: &str,
) -> Result<Vec<AddressLookupTableAccount>, String> {
    let cache = SHARED_LOOKUP_TABLE_CACHE.get_or_init(|| Mutex::new(None));
    let data = fetch_account_data(rpc_url, SHARED_SUPER_LOOKUP_TABLE, "confirmed").await?;
    let table = AddressLookupTable::deserialize(&data).map_err(|error| {
        format!("Failed to decode shared ALT {SHARED_SUPER_LOOKUP_TABLE}: {error}")
    })?;
    let tables = vec![AddressLookupTableAccount {
        key: parse_pubkey(SHARED_SUPER_LOOKUP_TABLE, "shared ALT")?,
        addresses: table.addresses.to_vec(),
    }];
    *cache
        .lock()
        .map_err(|_| "LaunchDeck wrapper ALT cache lock poisoned".to_string())? =
        Some(CachedLookupTables {
            rpc_url: rpc_url.to_string(),
            loaded_at: Instant::now(),
            tables: tables.clone(),
        });
    Ok(tables)
}

pub fn parse_sol_amount_to_lamports(value: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    let (whole, frac) = trimmed.split_once('.').unwrap_or((trimmed, ""));
    let whole_lamports = whole
        .parse::<u64>()
        .map_err(|error| format!("Invalid SOL amount {value}: {error}"))?
        .checked_mul(1_000_000_000)
        .ok_or_else(|| format!("SOL amount {value} is too large"))?;
    let mut frac_digits = frac.chars().take(9).collect::<String>();
    while frac_digits.len() < 9 {
        frac_digits.push('0');
    }
    let frac_lamports = if frac_digits.is_empty() {
        0
    } else {
        frac_digits
            .parse::<u64>()
            .map_err(|error| format!("Invalid SOL amount {value}: {error}"))?
    };
    whole_lamports
        .checked_add(frac_lamports)
        .ok_or_else(|| format!("SOL amount {value} is too large"))
}

pub fn estimate_sol_in_fee_lamports(gross_lamports: u64, fee_bps: u16) -> u64 {
    if gross_lamports == 0 || fee_bps == 0 {
        return 0;
    }
    ((gross_lamports as u128 * fee_bps as u128) / BPS_DENOMINATOR) as u64
}

fn build_compute_unit_limit_instruction(compute_unit_limit: u32) -> Instruction {
    let mut data = vec![2];
    data.extend_from_slice(&compute_unit_limit.to_le_bytes());
    Instruction {
        program_id: compute_budget_program_id(),
        accounts: vec![],
        data,
    }
}

fn recommended_wrapped_compute_unit_limit(source_limit: Option<u64>) -> u32 {
    let source_limit = source_limit
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_default();
    source_limit
        .saturating_add(WRAPPED_COMPUTE_UNIT_LIMIT_OVERHEAD)
        .max(WRAPPED_COMPUTE_UNIT_LIMIT_FLOOR)
}

fn raise_wrapped_compute_unit_limit(
    instructions: &mut Vec<Instruction>,
    source_limit: Option<u64>,
) -> u64 {
    let target = recommended_wrapped_compute_unit_limit(source_limit);
    for instruction in instructions.iter_mut() {
        if instruction.program_id != compute_budget_program_id()
            || instruction.data.first().copied() != Some(2)
            || instruction.data.len() != 5
        {
            continue;
        }
        let current = u32::from_le_bytes([
            instruction.data[1],
            instruction.data[2],
            instruction.data[3],
            instruction.data[4],
        ]);
        let raised = current.max(target);
        instruction.data[1..5].copy_from_slice(&raised.to_le_bytes());
        return u64::from(raised);
    }
    instructions.insert(0, build_compute_unit_limit_instruction(target));
    u64::from(target)
}

fn validate_shared_lookup_table_usage(
    label: &str,
    lookup_tables_used: &[String],
) -> Result<(), String> {
    if lookup_tables_used.len() == 1 && lookup_tables_used[0] == SHARED_SUPER_LOOKUP_TABLE {
        return Ok(());
    }
    Err(format!(
        "LaunchDeck wrapper transaction {label} must use exactly the shared ALT {SHARED_SUPER_LOOKUP_TABLE}; used [{}].",
        lookup_tables_used.join(", ")
    ))
}

pub fn wrapper_token_program_id() -> Pubkey {
    token_program_id()
}

pub fn wrapper_wsol_mint() -> Pubkey {
    wsol_mint()
}

pub fn build_execute_swap_route_instruction(
    user: &Pubkey,
    fee_vault_wsol_ata: &Pubkey,
    route_wsol_or_sentinel: &Pubkey,
    inner_program: &Pubkey,
    request: &ExecuteSwapRouteRequest,
    route_accounts: &[AccountMeta],
    token_fee_vault_ata: Option<&Pubkey>,
) -> Result<Instruction, String> {
    if request.version != ABI_VERSION {
        return Err(format!(
            "LaunchDeck wrapper ABI version mismatch: got {}, expected {}",
            request.version, ABI_VERSION
        ));
    }
    let mut expected_offset = EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT;
    if swap_route_uses_wsol(request) {
        if request.wsol_lane >= ROUTE_WSOL_LANE_COUNT {
            return Err(format!(
                "LaunchDeck wrapper route WSOL lane {} outside supported range 0..{}",
                request.wsol_lane, ROUTE_WSOL_LANE_COUNT
            ));
        }
        expected_offset = expected_offset
            .checked_add(EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT)
            .ok_or_else(|| "LaunchDeck route prefix overflowed".to_string())?;
    }
    if swap_route_uses_token_fee(request) {
        expected_offset = expected_offset
            .checked_add(EXECUTE_SWAP_ROUTE_TOKEN_FEE_ACCOUNT_COUNT)
            .ok_or_else(|| "LaunchDeck route prefix overflowed".to_string())?;
    }
    if request.route_accounts_offset != expected_offset {
        return Err(format!(
            "route_accounts_offset must equal expected v3 swap-route prefix ({}), got {}",
            expected_offset, request.route_accounts_offset
        ));
    }
    if request.fee_bps > MAX_FEE_BPS {
        return Err(format!(
            "LaunchDeck wrapper fee_bps {} exceeds cap {}",
            request.fee_bps, MAX_FEE_BPS
        ));
    }
    let mut accounts = vec![
        AccountMeta::new(*user, true),
        AccountMeta::new_readonly(config_pda(), false),
        AccountMeta::new(wrapper_fee_vault(), false),
        AccountMeta::new(*fee_vault_wsol_ata, false),
        AccountMeta::new(*route_wsol_or_sentinel, false),
        AccountMeta::new_readonly(instructions_sysvar_id(), false),
        AccountMeta::new_readonly(*inner_program, false),
        AccountMeta::new_readonly(token_program_id(), false),
    ];
    if swap_route_uses_wsol(request) {
        accounts.extend([
            AccountMeta::new_readonly(wsol_mint(), false),
            AccountMeta::new_readonly(system_program_id(), false),
            AccountMeta::new_readonly(rent_sysvar_id(), false),
        ]);
    }
    if swap_route_uses_token_fee(request) {
        let token_fee_vault_ata = token_fee_vault_ata
            .ok_or_else(|| "token fee route requires token_fee_vault_ata".to_string())?;
        accounts.push(AccountMeta::new(*token_fee_vault_ata, false));
    }
    accounts.extend_from_slice(route_accounts);
    let mut data = Vec::with_capacity(
        96 + request
            .legs
            .iter()
            .map(|leg| leg.ix_data.len())
            .sum::<usize>(),
    );
    data.push(EXECUTE_SWAP_ROUTE_DISCRIMINATOR);
    request
        .serialize(&mut data)
        .map_err(|error| format!("Failed to serialize ExecuteSwapRoute request: {error}"))?;
    Ok(Instruction {
        program_id: wrapper_program_id(),
        accounts,
        data,
    })
}

fn is_wrapper_swap_route_instruction(instruction: &Instruction) -> bool {
    if instruction.program_id != wrapper_program_id() {
        return false;
    }
    matches!(
        instruction.data.first().copied(),
        Some(
            EXECUTE_DISCRIMINATOR
                | EXECUTE_AMM_WSOL_DISCRIMINATOR
                | EXECUTE_SWAP_ROUTE_DISCRIMINATOR
        )
    )
}

pub fn wrap_compiled_transaction(
    source: &CompiledTransaction,
    payer: &Keypair,
    lookup_tables: &[AddressLookupTableAccount],
    request: LaunchdeckWrapRequest,
) -> Result<CompiledTransaction, String> {
    if request.route_kind == WrapperRouteKind::SolThrough {
        return Err("LaunchDeck wrapper does not support SolThrough".to_string());
    }
    if lookup_tables.is_empty() {
        return Err("LaunchDeck wrapper requires the shared ALT".to_string());
    }

    let bytes = BASE64
        .decode(source.serializedBase64.as_bytes())
        .map_err(|error| format!("Failed to decode native tx: {error}"))?;
    let transaction: VersionedTransaction = bincode::deserialize(&bytes)
        .map_err(|error| format!("Failed to deserialize native tx: {error}"))?;
    let blockhash = match &transaction.message {
        VersionedMessage::V0(message) => message.recent_blockhash,
        _ => return Err("LaunchDeck wrapper only supports v0 native transactions".to_string()),
    };
    let (instructions, _keys) = decompile_v0_transaction(&transaction, lookup_tables)?;
    if instructions.iter().any(is_wrapper_swap_route_instruction) {
        return Err("transaction is already wrapped".to_string());
    }

    let allowed = allowed_inner_programs();
    let venue_positions = instructions
        .iter()
        .enumerate()
        .filter_map(|(idx, ix)| allowed.contains(&ix.program_id).then_some(idx))
        .collect::<Vec<_>>();
    if venue_positions.is_empty() {
        return Err("no allowlisted venue instruction found".to_string());
    }
    let preferred_v2_idx =
        find_amm_wsol_v2_candidate_index(&payer.pubkey(), request, &instructions, &venue_positions);
    let venue_idx = if let Some(idx) = preferred_v2_idx {
        idx
    } else {
        match venue_positions.as_slice() {
            [] => unreachable!("empty venue positions handled above"),
            [only] => *only,
            multiple if request.select_first_allowlisted_venue_instruction => multiple[0],
            multiple if request.select_last_allowlisted_venue_instruction => {
                *multiple.last().unwrap()
            }
            multiple => {
                return Err(format!(
                    "found {} venue instructions; selection policy did not choose one",
                    multiple.len()
                ));
            }
        }
    };

    let mut venue_ix = instructions[venue_idx].clone();
    if matches!(request.route_kind, WrapperRouteKind::SolIn)
        && !request.infer_gross_sol_in_from_inner
        && is_bonk_non_sol_quote_buy(&venue_ix)
    {
        return Err("selected venue instruction does not consume SOL".to_string());
    }
    let mut request = apply_inferred_sol_in_amount(request, &venue_ix)?;
    if matches!(request.route_kind, WrapperRouteKind::SolOut) && request.min_net_output == 0 {
        let gross_min_output = infer_sol_out_min_lamports_from_venue_instruction(&venue_ix)
            .ok_or_else(|| {
                format!(
                    "Could not infer SOL output minimum for {}",
                    venue_ix.program_id
                )
            })?;
        request.min_net_output = gross_min_output
            .checked_sub(estimate_sol_in_fee_lamports(
                gross_min_output,
                request.fee_bps,
            ))
            .ok_or_else(|| "LaunchDeck wrapper fee exceeds minimum SOL output".to_string())?;
    }
    match request.route_kind {
        WrapperRouteKind::SolIn if request.gross_sol_in_lamports == 0 => {
            return Err("LaunchDeck SolIn wrapper requires gross_sol_in_lamports > 0".to_string());
        }
        WrapperRouteKind::SolOut if request.gross_sol_in_lamports != 0 => {
            return Err("LaunchDeck SolOut wrapper must not set gross_sol_in_lamports".to_string());
        }
        _ => {}
    }
    let (preamble, rest) = instructions.split_at(venue_idx);
    let preamble = preamble
        .iter()
        .filter(|instruction| !is_memo_instruction(instruction))
        .cloned()
        .collect::<Vec<_>>();
    let postamble = rest[1..]
        .iter()
        .filter(|instruction| !is_memo_instruction(instruction))
        .cloned()
        .collect::<Vec<_>>();
    let (user_wsol_ata, fee_vault_wsol_ata) = derive_wsol_route_accounts(
        &payer.pubkey(),
        &wrapper_fee_vault(),
        request.route_kind,
        &venue_ix.program_id,
        &venue_ix.accounts,
        &preamble,
        &postamble,
    )?;
    let mut inner_accounts = venue_ix.accounts.clone();
    append_system_program_inner_account(&mut inner_accounts);
    let v3_wsol_plan =
        try_build_amm_wsol_v2_plan(&payer.pubkey(), request, &venue_ix, &preamble, &postamble)
            .or_else(|| {
                try_build_wsol_out_v3_plan(
                    &payer.pubkey(),
                    request,
                    &venue_ix,
                    &preamble,
                    &postamble,
                    user_wsol_ata,
                )
            });
    let (wrapper_ix, preamble, postamble, mode) = if let Some(plan) = v3_wsol_plan {
        (
            build_v3_wsol_swap_route_instruction(
                payer,
                venue_ix.program_id,
                venue_ix.data,
                request,
                &plan,
                fee_vault_wsol_ata,
            )?,
            plan.preamble,
            plan.postamble,
            "v3-route-wsol",
        )
    } else {
        if matches!(request.route_kind, WrapperRouteKind::SolIn) {
            patch_sol_in_venue_instruction_to_net(&mut venue_ix, request)?;
        }
        (
            build_v3_direct_swap_route_instruction(
                payer,
                venue_ix.program_id,
                venue_ix.data,
                inner_accounts,
                request,
                user_wsol_ata,
                fee_vault_wsol_ata,
            )?,
            preamble,
            postamble,
            "v3-native",
        )
    };

    let mut new_instructions = Vec::with_capacity(preamble.len() + 1 + postamble.len());
    new_instructions.extend(preamble);
    new_instructions.push(wrapper_ix);
    new_instructions.extend(postamble);
    let wrapped_compute_unit_limit =
        raise_wrapped_compute_unit_limit(&mut new_instructions, source.computeUnitLimit);

    let message = v0::Message::try_compile(
        &payer.pubkey(),
        &new_instructions,
        lookup_tables,
        blockhash_from_string(&source.blockhash).unwrap_or(blockhash),
    )
    .map_err(|error| format!("Failed to compile LaunchDeck wrapper tx: {error}"))?;
    let required_signer_count = usize::from(message.header.num_required_signatures);
    let required_signers = message
        .account_keys
        .iter()
        .take(required_signer_count)
        .copied()
        .collect::<Vec<_>>();
    if required_signers.as_slice() != [payer.pubkey()] {
        return Err(format!(
            "v3 wrapper transactions must require exactly one signer (user); got [{}]",
            required_signers
                .iter()
                .map(Pubkey::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let restored = compiled_transaction_signers::restore_compiled_transaction_signers(
        &source.serializedBase64,
    );
    let mut signers = Vec::with_capacity(1 + restored.len());
    signers.push(payer);
    signers.extend(
        restored
            .iter()
            .filter(|signer| required_signers.contains(&signer.pubkey())),
    );

    let wrapped = VersionedTransaction::try_new(VersionedMessage::V0(message), &signers)
        .map_err(|error| format!("Failed to sign LaunchDeck wrapper tx: {error}"))?;
    let serialized = bincode::serialize(&wrapped)
        .map_err(|error| format!("Failed to serialize wrapper tx: {error}"))?;
    if serialized.len() > PACKET_LIMIT_BYTES {
        return Err(format!(
            "LaunchDeck wrapper tx exceeded packet limit: {} > {} bytes",
            serialized.len(),
            PACKET_LIMIT_BYTES
        ));
    }
    eprintln!(
        "[launchdeck-engine][wrapper-wrap] label={} mode={} route={:?} gross_sol_in={} bytes={}",
        source.label,
        mode,
        request.route_kind,
        request.gross_sol_in_lamports,
        serialized.len()
    );
    let lookup_tables_used = wrapped
        .message
        .address_table_lookups()
        .into_iter()
        .flatten()
        .map(|lookup| lookup.account_key.to_string())
        .collect::<Vec<_>>();
    validate_shared_lookup_table_usage(&source.label, &lookup_tables_used)?;
    Ok(CompiledTransaction {
        label: source.label.clone(),
        format: "v0-alt-wrapper".to_string(),
        blockhash: source.blockhash.clone(),
        lastValidBlockHeight: source.lastValidBlockHeight,
        serializedBase64: BASE64.encode(serialized),
        signature: wrapped.signatures.first().map(|sig| sig.to_string()),
        lookupTablesUsed: lookup_tables_used,
        computeUnitLimit: Some(wrapped_compute_unit_limit),
        computeUnitPriceMicroLamports: source.computeUnitPriceMicroLamports,
        inlineTipLamports: source.inlineTipLamports,
        inlineTipAccount: source.inlineTipAccount.clone(),
    })
}

fn apply_inferred_sol_in_amount(
    mut request: LaunchdeckWrapRequest,
    venue_ix: &Instruction,
) -> Result<LaunchdeckWrapRequest, String> {
    if !request.infer_gross_sol_in_from_inner
        || !matches!(request.route_kind, WrapperRouteKind::SolIn)
    {
        return Ok(request);
    }
    request.gross_sol_in_lamports = infer_sol_in_lamports_from_venue_instruction(venue_ix)
        .ok_or_else(|| {
            format!(
                "Could not infer SOL input from selected venue instruction {} for token-mode dev-buy",
                venue_ix.program_id
            )
        })?;
    if request.gross_sol_in_lamports == 0 {
        return Err("Inferred zero SOL input from selected venue instruction".to_string());
    }
    Ok(request)
}

fn infer_sol_in_lamports_from_venue_instruction(instruction: &Instruction) -> Option<u64> {
    let data = instruction.data.as_slice();
    if instruction.program_id == parse_pubkey(PUMP_PROGRAM_ID, "PUMP program id").ok()?
        && data.get(0..8)? == PUMP_BUY_DISCRIMINATOR
    {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(data.get(16..24)?);
        return Some(u64::from_le_bytes(buf));
    }
    if instruction.program_id == parse_pubkey(PUMP_PROGRAM_ID, "PUMP program id").ok()?
        && data.get(0..8)? == PUMP_BUY_EXACT_SOL_IN_DISCRIMINATOR
    {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(data.get(8..16)?);
        return Some(u64::from_le_bytes(buf));
    }
    if instruction.program_id == parse_pubkey(BONK_LAUNCHPAD_PROGRAM_ID, "Bonk program id").ok()?
        && data.get(0..8)? == BONK_BUY_EXACT_IN_DISCRIMINATOR
    {
        let quote_mint = instruction.accounts.get(10)?.pubkey;
        if quote_mint != wsol_mint() {
            return None;
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(data.get(8..16)?);
        return Some(u64::from_le_bytes(buf));
    }
    None
}

fn read_u64_le(data: &[u8], range: std::ops::Range<usize>) -> Option<u64> {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(data.get(range)?);
    Some(u64::from_le_bytes(buf))
}

fn infer_sol_out_min_lamports_from_venue_instruction(instruction: &Instruction) -> Option<u64> {
    let data = instruction.data.as_slice();
    let discriminator = data.get(0..8)?;
    let program_id = instruction.program_id;
    if program_id == parse_pubkey(PUMP_PROGRAM_ID, "PUMP program id").ok()?
        && discriminator == PUMP_SELL_DISCRIMINATOR
    {
        return read_u64_le(data, 16..24);
    }
    if program_id == parse_pubkey(BONK_LAUNCHPAD_PROGRAM_ID, "Bonk program id").ok()?
        && discriminator == BONK_SELL_EXACT_IN_DISCRIMINATOR
    {
        return read_u64_le(data, 16..24);
    }
    if (program_id == raydium_cpmm_program_id()
        && discriminator == RAYDIUM_CPMM_SWAP_BASE_INPUT_DISCRIMINATOR)
        || (program_id == raydium_clmm_program_id()
            && discriminator == RAYDIUM_CLMM_SWAP_V2_DISCRIMINATOR)
        || ((program_id == bags_dbc_program_id() || program_id == bags_damm_v2_program_id())
            && discriminator == BAGS_SWAP_DISCRIMINATOR)
    {
        return read_u64_le(data, 16..24);
    }
    None
}

fn patch_amm_wsol_input_amount(
    inner_program: Pubkey,
    inner_ix_data: &[u8],
    net_input_lamports: u64,
) -> Vec<u8> {
    let mut patched = inner_ix_data.to_vec();
    let supports_amount_patch = (inner_program == raydium_clmm_program_id()
        && patched.get(0..8) == Some(RAYDIUM_CLMM_SWAP_V2_DISCRIMINATOR.as_slice()))
        || (inner_program == raydium_cpmm_program_id()
            && patched.get(0..8) == Some(RAYDIUM_CPMM_SWAP_BASE_INPUT_DISCRIMINATOR.as_slice()))
        || (inner_program == bonk_launchpad_program_id()
            && patched.get(0..8) == Some(BONK_BUY_EXACT_IN_DISCRIMINATOR.as_slice()))
        || (inner_program == pump_amm_program_id()
            && patched.get(0..8) == Some(PUMP_AMM_BUY_EXACT_QUOTE_IN_DISCRIMINATOR.as_slice()))
        || ((inner_program == bags_dbc_program_id() || inner_program == bags_damm_v2_program_id())
            && patched.get(0..8) == Some(BAGS_SWAP_DISCRIMINATOR.as_slice()));
    if supports_amount_patch && patched.len() >= 16 {
        patched[8..16].copy_from_slice(&net_input_lamports.to_le_bytes());
    }
    patched
}

fn patch_sol_in_venue_instruction_to_net(
    instruction: &mut Instruction,
    request: LaunchdeckWrapRequest,
) -> Result<(), String> {
    let fee_lamports = estimate_sol_in_fee_lamports(request.gross_sol_in_lamports, request.fee_bps);
    let net_lamports = request
        .gross_sol_in_lamports
        .checked_sub(fee_lamports)
        .ok_or_else(|| "LaunchDeck wrapper fee exceeds gross SOL input".to_string())?;
    if net_lamports == 0 {
        return Err("LaunchDeck wrapper net venue input resolves to zero".to_string());
    }

    let patch_offset = sol_in_amount_patch_offset(instruction)
        .ok_or_else(|| format!("Could not patch SOL input for {}", instruction.program_id))?;
    let end = patch_offset
        .checked_add(8)
        .ok_or_else(|| "SOL input patch offset overflowed".to_string())?;
    if end > instruction.data.len() {
        return Err("SOL input patch offset is out of bounds".to_string());
    }
    instruction.data[patch_offset..end].copy_from_slice(&net_lamports.to_le_bytes());
    Ok(())
}

fn sol_in_amount_patch_offset(instruction: &Instruction) -> Option<usize> {
    let data = instruction.data.as_slice();
    if instruction.program_id == parse_pubkey(PUMP_PROGRAM_ID, "PUMP program id").ok()?
        && data.get(0..8)? == PUMP_BUY_DISCRIMINATOR
    {
        return Some(16);
    }
    if instruction.program_id == parse_pubkey(PUMP_PROGRAM_ID, "PUMP program id").ok()?
        && data.get(0..8)? == PUMP_BUY_EXACT_SOL_IN_DISCRIMINATOR
    {
        return Some(8);
    }
    if instruction.program_id == parse_pubkey(BONK_LAUNCHPAD_PROGRAM_ID, "Bonk program id").ok()?
        && data.get(0..8)? == BONK_BUY_EXACT_IN_DISCRIMINATOR
    {
        let quote_mint = instruction.accounts.get(10)?.pubkey;
        if quote_mint != wsol_mint() {
            return None;
        }
        return Some(8);
    }
    infer_sol_in_lamports_from_venue_instruction(instruction).map(|_| 8)
}

fn is_bonk_non_sol_quote_buy(instruction: &Instruction) -> bool {
    if Some(instruction.program_id)
        != parse_pubkey(BONK_LAUNCHPAD_PROGRAM_ID, "Bonk program id").ok()
        || instruction.data.get(0..8) != Some(BONK_BUY_EXACT_IN_DISCRIMINATOR.as_slice())
    {
        return false;
    }
    instruction
        .accounts
        .get(10)
        .map(|meta| meta.pubkey != wsol_mint())
        .unwrap_or(false)
}

fn blockhash_from_string(value: &str) -> Option<Hash> {
    value.parse::<Hash>().ok()
}

fn find_amm_wsol_v2_candidate_index(
    payer: &Pubkey,
    request: LaunchdeckWrapRequest,
    instructions: &[Instruction],
    venue_positions: &[usize],
) -> Option<usize> {
    if !matches!(request.route_kind, WrapperRouteKind::SolIn) {
        return None;
    }
    venue_positions.iter().copied().find(|idx| {
        let (preamble, rest) = instructions.split_at(*idx);
        let Some(venue_ix) = rest.first() else {
            return false;
        };
        let postamble = &rest[1..];
        try_build_amm_wsol_v2_plan(payer, request, venue_ix, preamble, postamble).is_some()
    })
}

#[derive(Debug, Clone)]
struct AmmWsolV2Plan {
    inner_wsol_account_index: usize,
    amm_wsol_account: Pubkey,
    pda_wsol_lamports: u64,
    inner_accounts: Vec<AccountMeta>,
    preamble: Vec<Instruction>,
    postamble: Vec<Instruction>,
}

fn try_build_amm_wsol_v2_plan(
    payer: &Pubkey,
    request: LaunchdeckWrapRequest,
    venue_ix: &Instruction,
    preamble: &[Instruction],
    postamble: &[Instruction],
) -> Option<AmmWsolV2Plan> {
    if !matches!(request.route_kind, WrapperRouteKind::SolIn) {
        return None;
    }
    let inner_wsol_account_index =
        derive_amm_wsol_input_account_index(&venue_ix.program_id, &venue_ix.accounts, preamble)?;
    let original_wsol_account = venue_ix.accounts.get(inner_wsol_account_index)?.pubkey;
    let source_wsol_lamports =
        find_preamble_wsol_funding_lamports(preamble, &original_wsol_account)?;
    let fee_lamports = estimate_sol_in_fee_lamports(request.gross_sol_in_lamports, request.fee_bps);
    let pda_wsol_lamports = request
        .gross_sol_in_lamports
        .checked_sub(fee_lamports)
        .filter(|net| *net <= source_wsol_lamports)?;
    if pda_wsol_lamports == 0 {
        return None;
    }
    let preamble = strip_temp_wsol_lifecycle_instructions(preamble, &original_wsol_account)?;
    let postamble = strip_temp_wsol_lifecycle_instructions(postamble, &original_wsol_account)?;

    let amm_wsol_account = route_wsol_pda(payer, 0);
    let mut inner_accounts = venue_ix.accounts.clone();
    let meta = inner_accounts.get_mut(inner_wsol_account_index)?;
    meta.pubkey = amm_wsol_account;
    meta.is_signer = false;
    meta.is_writable = true;
    append_system_program_inner_account(&mut inner_accounts);

    Some(AmmWsolV2Plan {
        inner_wsol_account_index,
        amm_wsol_account,
        pda_wsol_lamports,
        inner_accounts,
        preamble,
        postamble,
    })
}

fn try_build_wsol_out_v3_plan(
    payer: &Pubkey,
    request: LaunchdeckWrapRequest,
    venue_ix: &Instruction,
    preamble: &[Instruction],
    postamble: &[Instruction],
    user_wsol_account: Option<Pubkey>,
) -> Option<AmmWsolV2Plan> {
    if !matches!(request.route_kind, WrapperRouteKind::SolOut) {
        return None;
    }
    let original_wsol_account = user_wsol_account?;
    let route_wsol_account = route_wsol_pda(payer, 0);
    let mut inner_accounts = venue_ix.accounts.clone();
    let output_index = inner_accounts
        .iter()
        .position(|meta| meta.pubkey == original_wsol_account && meta.is_writable)?;
    let meta = inner_accounts.get_mut(output_index)?;
    meta.pubkey = route_wsol_account;
    meta.is_signer = false;
    meta.is_writable = true;
    let preamble = strip_temp_wsol_lifecycle_instructions(preamble, &original_wsol_account)?;
    let postamble = strip_temp_wsol_lifecycle_instructions(postamble, &original_wsol_account)?;
    append_system_program_inner_account(&mut inner_accounts);
    Some(AmmWsolV2Plan {
        inner_wsol_account_index: output_index,
        amm_wsol_account: route_wsol_account,
        pda_wsol_lamports: 0,
        inner_accounts,
        preamble,
        postamble,
    })
}

fn build_v3_wsol_swap_route_instruction(
    payer: &Keypair,
    inner_program: Pubkey,
    inner_ix_data: Vec<u8>,
    request: LaunchdeckWrapRequest,
    plan: &AmmWsolV2Plan,
    fee_vault_wsol_ata: Option<Pubkey>,
) -> Result<Instruction, String> {
    let fee_bps = request.fee_bps;
    if fee_bps > MAX_FEE_BPS {
        return Err(format!(
            "wrapper fee_bps {fee_bps} exceeds cap {MAX_FEE_BPS}"
        ));
    }
    let direction = match request.route_kind {
        WrapperRouteKind::SolIn => SwapRouteDirection::Buy,
        WrapperRouteKind::SolOut => SwapRouteDirection::Sell,
        WrapperRouteKind::SolThrough => return Err("SolThrough is unsupported".to_string()),
    };
    let settlement = match request.route_kind {
        WrapperRouteKind::SolIn => SwapRouteSettlement::Token,
        WrapperRouteKind::SolOut => SwapRouteSettlement::Wsol,
        WrapperRouteKind::SolThrough => return Err("SolThrough is unsupported".to_string()),
    };
    let fee_mode = match request.route_kind {
        WrapperRouteKind::SolIn => SwapRouteFeeMode::SolPre,
        WrapperRouteKind::SolOut => SwapRouteFeeMode::WsolPost,
        WrapperRouteKind::SolThrough => return Err("SolThrough is unsupported".to_string()),
    };
    let route_mode = match request.route_kind {
        WrapperRouteKind::SolIn => SwapRouteMode::Mixed,
        WrapperRouteKind::SolOut => SwapRouteMode::SolOut,
        WrapperRouteKind::SolThrough => return Err("SolThrough is unsupported".to_string()),
    };
    let route_accounts_len = u16::try_from(plan.inner_accounts.len())
        .map_err(|_| "route account count does not fit u16".to_string())?;
    let output_account_index = if matches!(request.route_kind, WrapperRouteKind::SolOut) {
        u16::try_from(1usize.saturating_add(plan.inner_wsol_account_index))
            .map_err(|_| "route output account index does not fit u16".to_string())?
    } else {
        SWAP_ROUTE_NO_PATCH_OFFSET
    };
    let execute_request = ExecuteSwapRouteRequest {
        version: ABI_VERSION,
        route_mode,
        direction,
        settlement,
        fee_mode,
        wsol_lane: 0,
        fee_bps,
        gross_sol_in_lamports: request.gross_sol_in_lamports,
        gross_token_in_amount: 0,
        min_net_output: request.min_net_output,
        route_accounts_offset: EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT
            + EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT,
        intermediate_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
        token_fee_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
        legs: vec![SwapRouteLeg {
            program_account_index: 0,
            accounts_start: 1,
            accounts_len: route_accounts_len,
            input_source: if matches!(request.route_kind, WrapperRouteKind::SolIn) {
                SwapLegInputSource::GrossSolNetOfFee
            } else {
                SwapLegInputSource::Fixed
            },
            input_amount: plan.pda_wsol_lamports,
            input_patch_offset: SWAP_ROUTE_NO_PATCH_OFFSET,
            output_account_index,
            ix_data: if matches!(request.route_kind, WrapperRouteKind::SolIn) {
                patch_amm_wsol_input_amount(inner_program, &inner_ix_data, plan.pda_wsol_lamports)
            } else {
                inner_ix_data
            },
        }],
    };
    let mut route_accounts = Vec::with_capacity(1 + plan.inner_accounts.len());
    route_accounts.push(AccountMeta::new_readonly(inner_program, false));
    route_accounts.extend(plan.inner_accounts.iter().cloned());
    let zero = ZEROED_WSOL_ATA_SENTINEL;
    let fee_vault_wsol_ata = fee_vault_wsol_ata.unwrap_or(zero);
    build_execute_swap_route_instruction(
        &payer.pubkey(),
        &fee_vault_wsol_ata,
        &plan.amm_wsol_account,
        &inner_program,
        &execute_request,
        &route_accounts,
        None,
    )
}

fn build_v3_direct_swap_route_instruction(
    payer: &Keypair,
    inner_program: Pubkey,
    inner_ix_data: Vec<u8>,
    inner_accounts: Vec<AccountMeta>,
    request: LaunchdeckWrapRequest,
    user_wsol_ata: Option<Pubkey>,
    fee_vault_wsol_ata: Option<Pubkey>,
) -> Result<Instruction, String> {
    let fee_bps = request.fee_bps;
    if fee_bps > MAX_FEE_BPS {
        return Err(format!(
            "wrapper fee_bps {fee_bps} exceeds cap {MAX_FEE_BPS}"
        ));
    }
    if user_wsol_ata.is_some() || fee_vault_wsol_ata.is_some() {
        return Err("WSOL routes must compile through v3 route WSOL PDA".to_string());
    }
    let direction = match request.route_kind {
        WrapperRouteKind::SolIn => SwapRouteDirection::Buy,
        WrapperRouteKind::SolOut => SwapRouteDirection::Sell,
        WrapperRouteKind::SolThrough => return Err("SolThrough is unsupported".to_string()),
    };
    let settlement = match request.route_kind {
        WrapperRouteKind::SolIn => SwapRouteSettlement::Token,
        WrapperRouteKind::SolOut => SwapRouteSettlement::NativeSol,
        WrapperRouteKind::SolThrough => return Err("SolThrough is unsupported".to_string()),
    };
    let fee_mode = match request.route_kind {
        WrapperRouteKind::SolIn => SwapRouteFeeMode::SolPre,
        WrapperRouteKind::SolOut => SwapRouteFeeMode::NativeSolPost,
        WrapperRouteKind::SolThrough => return Err("SolThrough is unsupported".to_string()),
    };
    let route_mode = match request.route_kind {
        WrapperRouteKind::SolIn => SwapRouteMode::SolIn,
        WrapperRouteKind::SolOut => SwapRouteMode::SolOut,
        WrapperRouteKind::SolThrough => return Err("SolThrough is unsupported".to_string()),
    };
    let route_accounts_len = u16::try_from(inner_accounts.len())
        .map_err(|_| "route account count does not fit u16".to_string())?;
    let execute_request = ExecuteSwapRouteRequest {
        version: ABI_VERSION,
        route_mode,
        direction,
        settlement,
        fee_mode,
        wsol_lane: 0,
        fee_bps,
        gross_sol_in_lamports: request.gross_sol_in_lamports,
        gross_token_in_amount: 0,
        min_net_output: if matches!(request.route_kind, WrapperRouteKind::SolIn) {
            0
        } else {
            request.min_net_output
        },
        route_accounts_offset: EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT,
        intermediate_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
        token_fee_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
        legs: vec![SwapRouteLeg {
            program_account_index: 0,
            accounts_start: 1,
            accounts_len: route_accounts_len,
            input_source: if matches!(request.route_kind, WrapperRouteKind::SolIn) {
                SwapLegInputSource::GrossSolNetOfFee
            } else {
                SwapLegInputSource::Fixed
            },
            input_amount: 0,
            input_patch_offset: SWAP_ROUTE_NO_PATCH_OFFSET,
            output_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
            ix_data: inner_ix_data,
        }],
    };
    let mut route_accounts = Vec::with_capacity(1 + inner_accounts.len());
    route_accounts.push(AccountMeta::new_readonly(inner_program, false));
    route_accounts.extend(inner_accounts);
    let zero = ZEROED_WSOL_ATA_SENTINEL;
    build_execute_swap_route_instruction(
        &payer.pubkey(),
        &zero,
        &zero,
        &inner_program,
        &execute_request,
        &route_accounts,
        None,
    )
}

fn decompile_v0_transaction(
    transaction: &VersionedTransaction,
    lookup_tables: &[AddressLookupTableAccount],
) -> Result<(Vec<Instruction>, Vec<Pubkey>), String> {
    let mut account_keys = transaction.message.static_account_keys().to_vec();
    if let Some(lookups) = transaction.message.address_table_lookups() {
        let mut writable = Vec::new();
        let mut readonly = Vec::new();
        for lookup in lookups {
            let table = lookup_tables
                .iter()
                .find(|table| table.key == lookup.account_key)
                .ok_or_else(|| format!("ALT {} was not supplied", lookup.account_key))?;
            for index in &lookup.writable_indexes {
                writable.push(
                    *table
                        .addresses
                        .get(usize::from(*index))
                        .ok_or_else(|| format!("ALT {} index {} missing", table.key, index))?,
                );
            }
            for index in &lookup.readonly_indexes {
                readonly.push(
                    *table
                        .addresses
                        .get(usize::from(*index))
                        .ok_or_else(|| format!("ALT {} index {} missing", table.key, index))?,
                );
            }
        }
        account_keys.extend(writable);
        account_keys.extend(readonly);
    }

    let mut instructions = Vec::new();
    for compiled in transaction.message.instructions() {
        let program_id = *account_keys
            .get(usize::from(compiled.program_id_index))
            .ok_or_else(|| "compiled instruction program index out of bounds".to_string())?;
        let mut accounts = Vec::with_capacity(compiled.accounts.len());
        for account_index in &compiled.accounts {
            let index = usize::from(*account_index);
            let pubkey = *account_keys
                .get(index)
                .ok_or_else(|| "compiled instruction account index out of bounds".to_string())?;
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
    Ok((instructions, account_keys))
}

fn derive_wsol_route_accounts(
    payer: &Pubkey,
    fee_vault: &Pubkey,
    route_kind: WrapperRouteKind,
    inner_program: &Pubkey,
    venue_accounts: &[AccountMeta],
    preamble: &[Instruction],
    postamble: &[Instruction],
) -> Result<(Option<Pubkey>, Option<Pubkey>), String> {
    let payer_wsol_ata =
        get_associated_token_address_with_program_id(payer, &wsol_mint(), &token_program_id());
    let user_wsol_account = if venue_accounts
        .iter()
        .any(|meta| meta.pubkey == payer_wsol_ata)
    {
        Some(payer_wsol_ata)
    } else if let Some(account) = derive_temp_wsol_output_account(inner_program, venue_accounts) {
        Some(account)
    } else {
        derive_closed_temp_wsol_output_account(
            payer,
            route_kind,
            venue_accounts,
            preamble,
            postamble,
        )?
    };
    let fee_vault_wsol_ata = user_wsol_account.map(|_| {
        get_associated_token_address_with_program_id(fee_vault, &wsol_mint(), &token_program_id())
    });
    Ok((user_wsol_account, fee_vault_wsol_ata))
}

fn derive_closed_temp_wsol_output_account(
    payer: &Pubkey,
    route_kind: WrapperRouteKind,
    venue_accounts: &[AccountMeta],
    preamble: &[Instruction],
    postamble: &[Instruction],
) -> Result<Option<Pubkey>, String> {
    if !matches!(route_kind, WrapperRouteKind::SolOut) {
        return Ok(None);
    }
    let mut close_candidates = Vec::new();
    for instruction in postamble {
        if instruction.program_id != token_program_id()
            || instruction.data.first().copied() != Some(SPL_TOKEN_CLOSE_ACCOUNT_DISCRIMINATOR)
        {
            continue;
        }
        let Some(closed_account) = instruction.accounts.first().map(|meta| meta.pubkey) else {
            continue;
        };
        let Some(destination) = instruction.accounts.get(1).map(|meta| meta.pubkey) else {
            continue;
        };
        let Some(owner) = instruction.accounts.get(2).map(|meta| meta.pubkey) else {
            continue;
        };
        if destination != *payer || owner != *payer {
            continue;
        }
        let referenced_by_venue = venue_accounts
            .iter()
            .any(|meta| meta.pubkey == closed_account && meta.is_writable);
        if referenced_by_venue && !close_candidates.contains(&closed_account) {
            close_candidates.push(closed_account);
        }
    }
    if close_candidates.is_empty() {
        return Ok(None);
    }

    let proven_wsol = close_candidates
        .iter()
        .copied()
        .filter(|account| preamble_initializes_wsol_account(preamble, account))
        .collect::<Vec<_>>();
    match proven_wsol.as_slice() {
        [account] => Ok(Some(*account)),
        [] => Err(format!(
            "SolOut close-account candidate(s) [{}] could not be proven to be WSOL",
            close_candidates
                .iter()
                .map(Pubkey::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )),
        multiple => Err(format!(
            "ambiguous SolOut WSOL close-account candidates [{}]",
            multiple
                .iter()
                .map(Pubkey::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn preamble_initializes_wsol_account(preamble: &[Instruction], account: &Pubkey) -> bool {
    preamble.iter().any(|instruction| {
        instruction.program_id == token_program_id()
            && instruction.data.first().copied()
                == Some(SPL_TOKEN_INITIALIZE_ACCOUNT3_DISCRIMINATOR)
            && instruction.accounts.first().map(|meta| meta.pubkey) == Some(*account)
            && instruction.accounts.get(1).map(|meta| meta.pubkey) == Some(wsol_mint())
    })
}

fn derive_temp_wsol_output_account(
    inner_program: &Pubkey,
    venue_accounts: &[AccountMeta],
) -> Option<Pubkey> {
    if *inner_program == raydium_clmm_program_id() {
        let user_output_account = venue_accounts.get(4)?;
        let output_mint = venue_accounts.get(12)?;
        return (user_output_account.is_writable && output_mint.pubkey == wsol_mint())
            .then_some(user_output_account.pubkey);
    }
    if *inner_program == raydium_cpmm_program_id() {
        let user_output_account = venue_accounts.get(5)?;
        let output_mint = venue_accounts.get(11)?;
        return (user_output_account.is_writable && output_mint.pubkey == wsol_mint())
            .then_some(user_output_account.pubkey);
    }
    if *inner_program == raydium_amm_v4_program_id() {
        let user_output_account = venue_accounts.get(16)?;
        let user_owner = venue_accounts.get(17)?;
        return (user_output_account.is_writable && user_owner.is_signer)
            .then_some(user_output_account.pubkey);
    }
    None
}

fn derive_amm_wsol_input_account_index(
    inner_program: &Pubkey,
    venue_accounts: &[AccountMeta],
    preamble: &[Instruction],
) -> Option<usize> {
    if *inner_program == raydium_clmm_program_id() {
        let user_input_account = venue_accounts.get(3)?;
        let input_mint = venue_accounts.get(11)?;
        return (user_input_account.is_writable && input_mint.pubkey == wsol_mint()).then_some(3);
    }
    if *inner_program == raydium_cpmm_program_id() {
        let user_input_account = venue_accounts.get(4)?;
        let input_mint = venue_accounts.get(10)?;
        return (user_input_account.is_writable && input_mint.pubkey == wsol_mint()).then_some(4);
    }
    if *inner_program == raydium_amm_v4_program_id() {
        let user_input_account = venue_accounts.get(15)?;
        let user_owner = venue_accounts.get(17)?;
        return (user_input_account.is_writable && user_owner.is_signer).then_some(15);
    }
    if *inner_program == bonk_launchpad_program_id() {
        let user_input_account = venue_accounts.get(6)?;
        let input_mint = venue_accounts.get(10)?;
        return (user_input_account.is_writable && input_mint.pubkey == wsol_mint()).then_some(6);
    }
    derive_generic_wsol_input_account_index(venue_accounts, preamble)
}

fn derive_generic_wsol_input_account_index(
    venue_accounts: &[AccountMeta],
    preamble: &[Instruction],
) -> Option<usize> {
    let candidates = venue_accounts
        .iter()
        .enumerate()
        .filter_map(|(index, meta)| {
            (meta.is_writable && preamble_prepares_wsol_input_account(preamble, &meta.pubkey))
                .then_some(index)
        })
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [index] => Some(*index),
        _ => None,
    }
}

fn find_temp_wsol_create_lamports(
    instructions: &[Instruction],
    temp_wsol_account: &Pubkey,
) -> Option<u64> {
    instructions.iter().find_map(|instruction| {
        if !is_system_create_account_for(instruction, temp_wsol_account) {
            return None;
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(instruction.data.get(4..12)?);
        Some(u64::from_le_bytes(buf))
    })
}

fn find_preamble_wsol_funding_lamports(
    instructions: &[Instruction],
    wsol_account: &Pubkey,
) -> Option<u64> {
    if let Some(create_lamports) = find_temp_wsol_create_lamports(instructions, wsol_account) {
        let rent_lamports = Rent::default().minimum_balance(SPL_TOKEN_ACCOUNT_LEN);
        return create_lamports.checked_sub(rent_lamports);
    }
    find_system_transfer_lamports_to(instructions, wsol_account)
}

fn find_system_transfer_lamports_to(
    instructions: &[Instruction],
    destination: &Pubkey,
) -> Option<u64> {
    instructions.iter().find_map(|instruction| {
        if !is_system_transfer_to(instruction, destination) {
            return None;
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(instruction.data.get(4..12)?);
        Some(u64::from_le_bytes(buf))
    })
}

fn preamble_prepares_wsol_input_account(preamble: &[Instruction], account: &Pubkey) -> bool {
    find_preamble_wsol_funding_lamports(preamble, account).is_some()
        && preamble.iter().any(|instruction| {
            (instruction.program_id == token_program_id()
                && instruction.data.first().copied() == Some(SPL_TOKEN_SYNC_NATIVE_DISCRIMINATOR)
                && instruction.accounts.first().map(|meta| meta.pubkey) == Some(*account))
                || preamble_initializes_wsol_account(preamble, account)
        })
}

fn strip_temp_wsol_lifecycle_instructions(
    instructions: &[Instruction],
    temp_wsol_account: &Pubkey,
) -> Option<Vec<Instruction>> {
    let mut retained = Vec::with_capacity(instructions.len());
    for instruction in instructions {
        if !instruction_references_pubkey(instruction, temp_wsol_account) {
            retained.push(instruction.clone());
            continue;
        }
        if is_temp_wsol_lifecycle_instruction(instruction, temp_wsol_account) {
            continue;
        }
        return None;
    }
    Some(retained)
}

fn instruction_references_pubkey(instruction: &Instruction, pubkey: &Pubkey) -> bool {
    instruction
        .accounts
        .iter()
        .any(|account| account.pubkey == *pubkey)
}

fn is_temp_wsol_lifecycle_instruction(
    instruction: &Instruction,
    temp_wsol_account: &Pubkey,
) -> bool {
    is_system_create_account_for(instruction, temp_wsol_account)
        || is_system_transfer_to(instruction, temp_wsol_account)
        || is_associated_token_account_create_for_wsol(instruction, temp_wsol_account)
        || is_spl_token_wsol_lifecycle_instruction(instruction, temp_wsol_account)
}

fn is_system_create_account_for(instruction: &Instruction, temp_wsol_account: &Pubkey) -> bool {
    if instruction.program_id != system_program_id()
        || instruction.accounts.get(1).map(|meta| meta.pubkey) != Some(*temp_wsol_account)
        || instruction.data.len() < 52
    {
        return false;
    }
    let mut discriminator = [0u8; 4];
    discriminator.copy_from_slice(&instruction.data[0..4]);
    if u32::from_le_bytes(discriminator) != SYSTEM_CREATE_ACCOUNT_DISCRIMINATOR {
        return false;
    }
    let mut space = [0u8; 8];
    space.copy_from_slice(&instruction.data[12..20]);
    if u64::from_le_bytes(space) != SPL_TOKEN_ACCOUNT_LEN as u64 {
        return false;
    }
    let mut owner = [0u8; 32];
    owner.copy_from_slice(&instruction.data[20..52]);
    Pubkey::new_from_array(owner) == token_program_id()
}

fn is_system_transfer_to(instruction: &Instruction, destination: &Pubkey) -> bool {
    if instruction.program_id != system_program_id()
        || instruction.accounts.get(1).map(|meta| meta.pubkey) != Some(*destination)
        || instruction.data.len() < 12
    {
        return false;
    }
    let mut discriminator = [0u8; 4];
    discriminator.copy_from_slice(&instruction.data[0..4]);
    u32::from_le_bytes(discriminator) == SYSTEM_TRANSFER_DISCRIMINATOR
}

fn is_associated_token_account_create_for_wsol(
    instruction: &Instruction,
    wsol_account: &Pubkey,
) -> bool {
    instruction.program_id == spl_associated_token_account::id()
        && instruction.accounts.get(1).map(|meta| meta.pubkey) == Some(*wsol_account)
        && instruction.accounts.get(3).map(|meta| meta.pubkey) == Some(wsol_mint())
}

fn is_spl_token_wsol_lifecycle_instruction(
    instruction: &Instruction,
    temp_wsol_account: &Pubkey,
) -> bool {
    if instruction.program_id != token_program_id()
        || instruction.accounts.first().map(|meta| meta.pubkey) != Some(*temp_wsol_account)
    {
        return false;
    }
    matches!(
        instruction.data.first().copied(),
        Some(SPL_TOKEN_INITIALIZE_ACCOUNT3_DISCRIMINATOR)
            | Some(SPL_TOKEN_SYNC_NATIVE_DISCRIMINATOR)
            | Some(SPL_TOKEN_CLOSE_ACCOUNT_DISCRIMINATOR)
    )
}

fn append_system_program_inner_account(inner_accounts: &mut Vec<AccountMeta>) {
    let system_program = system_program_id();
    if inner_accounts
        .iter()
        .any(|meta| meta.pubkey == system_program)
    {
        return;
    }
    inner_accounts.push(AccountMeta::new_readonly(system_program, false));
}

fn is_memo_instruction(instruction: &Instruction) -> bool {
    instruction.program_id == memo_program_id()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sol_amount_supports_lamport_precision() {
        assert_eq!(parse_sol_amount_to_lamports("1").unwrap(), 1_000_000_000);
        assert_eq!(parse_sol_amount_to_lamports("0.1").unwrap(), 100_000_000);
        assert_eq!(parse_sol_amount_to_lamports("0.000000001").unwrap(), 1);
    }

    #[test]
    fn execute_request_serializes_with_legacy_discriminator() {
        let request = ExecuteRequest {
            version: ABI_VERSION,
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            gross_sol_in_lamports: 1_000_000,
            min_net_output: 0,
            inner_accounts_offset: EXECUTE_FIXED_ACCOUNT_COUNT,
            inner_ix_data: vec![1, 2, 3],
        };
        let mut data = vec![1];
        request.serialize(&mut data).unwrap();
        let decoded = ExecuteRequest::try_from_slice(&data[1..]).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn build_v3_direct_swap_route_instruction_uses_v3_request_fee_bps() {
        let payer = Keypair::new();
        let instruction = build_v3_direct_swap_route_instruction(
            &payer,
            parse_pubkey(PUMP_PROGRAM_ID, "pump").unwrap(),
            vec![9, 8, 7],
            vec![AccountMeta::new(payer.pubkey(), true)],
            LaunchdeckWrapRequest {
                route_kind: WrapperRouteKind::SolIn,
                fee_bps: 20,
                gross_sol_in_lamports: 1_000_000,
                infer_gross_sol_in_from_inner: false,
                min_net_output: 0,
                select_first_allowlisted_venue_instruction: false,
                select_last_allowlisted_venue_instruction: false,
            },
            None,
            None,
        )
        .expect("build wrapper execute");
        assert_eq!(instruction.data[0], EXECUTE_SWAP_ROUTE_DISCRIMINATOR);
        let decoded = ExecuteSwapRouteRequest::try_from_slice(&instruction.data[1..]).unwrap();
        assert_eq!(decoded.fee_bps, 20);
        assert_eq!(decoded.gross_sol_in_lamports, 1_000_000);
        assert_eq!(decoded.route_mode, SwapRouteMode::SolIn);
        assert_eq!(decoded.fee_mode, SwapRouteFeeMode::SolPre);
    }

    #[test]
    fn infers_sol_out_minimum_from_known_sell_instructions() {
        let mut pump_sell_data = PUMP_SELL_DISCRIMINATOR.to_vec();
        pump_sell_data.extend_from_slice(&250_000u64.to_le_bytes());
        pump_sell_data.extend_from_slice(&1_000_000u64.to_le_bytes());
        let pump_sell = Instruction {
            program_id: parse_pubkey(PUMP_PROGRAM_ID, "pump").unwrap(),
            accounts: vec![],
            data: pump_sell_data,
        };

        assert_eq!(
            infer_sol_out_min_lamports_from_venue_instruction(&pump_sell),
            Some(1_000_000)
        );
    }

    #[test]
    fn already_wrapped_detection_includes_legacy_discriminators() {
        for discriminator in [
            EXECUTE_DISCRIMINATOR,
            EXECUTE_AMM_WSOL_DISCRIMINATOR,
            EXECUTE_SWAP_ROUTE_DISCRIMINATOR,
        ] {
            let instruction = Instruction {
                program_id: wrapper_program_id(),
                accounts: vec![],
                data: vec![discriminator],
            };
            assert!(is_wrapper_swap_route_instruction(&instruction));
        }
    }

    #[test]
    fn derives_temp_wsol_output_account_from_postamble_close() {
        let payer = Keypair::new();
        let fee_vault = Pubkey::new_unique();
        let temp_wsol = Pubkey::new_unique();
        let venue_accounts = vec![
            AccountMeta::new(Pubkey::new_unique(), false),
            AccountMeta::new(temp_wsol, false),
        ];
        let postamble = vec![Instruction {
            program_id: token_program_id(),
            accounts: vec![
                AccountMeta::new(temp_wsol, false),
                AccountMeta::new(payer.pubkey(), false),
                AccountMeta::new_readonly(payer.pubkey(), true),
            ],
            data: vec![SPL_TOKEN_CLOSE_ACCOUNT_DISCRIMINATOR],
        }];
        let preamble = vec![
            spl_token::instruction::initialize_account3(
                &token_program_id(),
                &temp_wsol,
                &wsol_mint(),
                &payer.pubkey(),
            )
            .expect("initialize temp wsol"),
        ];

        let (user_wsol, fee_vault_wsol) = derive_wsol_route_accounts(
            &payer.pubkey(),
            &fee_vault,
            WrapperRouteKind::SolOut,
            &Pubkey::new_unique(),
            &venue_accounts,
            &preamble,
            &postamble,
        )
        .expect("derive wsol route accounts");

        assert_eq!(user_wsol, Some(temp_wsol));
        assert_eq!(
            fee_vault_wsol,
            Some(get_associated_token_address_with_program_id(
                &fee_vault,
                &wsol_mint(),
                &token_program_id(),
            ))
        );
    }

    #[test]
    fn rejects_unproven_temp_wsol_output_close() {
        let payer = Keypair::new();
        let fee_vault = Pubkey::new_unique();
        let temp_account = Pubkey::new_unique();
        let venue_accounts = vec![AccountMeta::new(temp_account, false)];
        let postamble = vec![Instruction {
            program_id: token_program_id(),
            accounts: vec![
                AccountMeta::new(temp_account, false),
                AccountMeta::new(payer.pubkey(), false),
                AccountMeta::new_readonly(payer.pubkey(), true),
            ],
            data: vec![SPL_TOKEN_CLOSE_ACCOUNT_DISCRIMINATOR],
        }];

        let error = derive_wsol_route_accounts(
            &payer.pubkey(),
            &fee_vault,
            WrapperRouteKind::SolOut,
            &Pubkey::new_unique(),
            &venue_accounts,
            &[],
            &postamble,
        )
        .expect_err("unproven close must fail");

        assert!(error.contains("could not be proven to be WSOL"));
    }

    #[test]
    fn generic_temp_wsol_input_uses_route_pda() {
        let payer = Pubkey::new_unique();
        let temp_wsol = Pubkey::new_unique();
        let route_wsol = route_wsol_pda(&payer, 0);
        let rent_lamports = Rent::default().minimum_balance(SPL_TOKEN_ACCOUNT_LEN);
        let swap_lamports = 100_000_000;
        let mut venue_accounts = (0..23)
            .map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false))
            .collect::<Vec<_>>();
        venue_accounts[1] = AccountMeta::new_readonly(payer, true);
        venue_accounts[6] = AccountMeta::new(temp_wsol, false);
        let venue_ix = Instruction {
            program_id: pump_amm_program_id(),
            accounts: venue_accounts,
            data: PUMP_AMM_BUY_EXACT_QUOTE_IN_DISCRIMINATOR.to_vec(),
        };
        let preamble = vec![
            solana_system_interface::instruction::create_account(
                &payer,
                &temp_wsol,
                rent_lamports + swap_lamports,
                SPL_TOKEN_ACCOUNT_LEN as u64,
                &token_program_id(),
            ),
            spl_token::instruction::initialize_account3(
                &token_program_id(),
                &temp_wsol,
                &wsol_mint(),
                &payer,
            )
            .expect("initialize temp wsol"),
            spl_token::instruction::sync_native(&token_program_id(), &temp_wsol)
                .expect("sync temp wsol"),
        ];
        let postamble = vec![
            spl_token::instruction::close_account(
                &token_program_id(),
                &temp_wsol,
                &payer,
                &payer,
                &[],
            )
            .expect("close temp wsol"),
        ];
        let request = LaunchdeckWrapRequest {
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            gross_sol_in_lamports: swap_lamports,
            infer_gross_sol_in_from_inner: false,
            min_net_output: 1,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };

        let plan =
            try_build_amm_wsol_v2_plan(&payer, request, &venue_ix, &preamble, &postamble).unwrap();

        assert_eq!(plan.inner_wsol_account_index, 6);
        assert_eq!(plan.inner_accounts[6].pubkey, route_wsol);
        assert!(!plan.inner_accounts[6].is_signer);
        assert!(plan.inner_accounts[6].is_writable);
        assert!(plan.preamble.is_empty());
        assert!(plan.postamble.is_empty());
    }

    #[test]
    fn generic_wsol_ata_input_uses_route_pda() {
        let payer = Pubkey::new_unique();
        let input_wsol_ata =
            get_associated_token_address_with_program_id(&payer, &wsol_mint(), &token_program_id());
        let route_wsol = route_wsol_pda(&payer, 0);
        let gross_lamports = 100_000_000;
        let fee_bps = 10;
        let net_lamports = gross_lamports - estimate_sol_in_fee_lamports(gross_lamports, fee_bps);
        let mut venue_accounts = (0..15)
            .map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false))
            .collect::<Vec<_>>();
        venue_accounts[3] = AccountMeta::new(input_wsol_ata, false);
        venue_accounts[9] = AccountMeta::new_readonly(payer, true);
        let venue_ix = Instruction {
            program_id: bags_dbc_program_id(),
            accounts: venue_accounts,
            data: BAGS_SWAP_DISCRIMINATOR.to_vec(),
        };
        let preamble = vec![
            spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                &payer,
                &payer,
                &wsol_mint(),
                &token_program_id(),
            ),
            solana_system_interface::instruction::transfer(&payer, &input_wsol_ata, net_lamports),
            spl_token::instruction::sync_native(&token_program_id(), &input_wsol_ata)
                .expect("sync wsol ata"),
        ];
        let postamble = vec![
            spl_token::instruction::close_account(
                &token_program_id(),
                &input_wsol_ata,
                &payer,
                &payer,
                &[],
            )
            .expect("close wsol ata"),
        ];
        let request = LaunchdeckWrapRequest {
            route_kind: WrapperRouteKind::SolIn,
            fee_bps,
            gross_sol_in_lamports: gross_lamports,
            infer_gross_sol_in_from_inner: false,
            min_net_output: 1,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };

        let plan =
            try_build_amm_wsol_v2_plan(&payer, request, &venue_ix, &preamble, &postamble).unwrap();

        assert_eq!(plan.inner_wsol_account_index, 3);
        assert_eq!(plan.inner_accounts[3].pubkey, route_wsol);
        assert_eq!(plan.pda_wsol_lamports, net_lamports);
        assert!(plan.preamble.is_empty());
        assert!(plan.postamble.is_empty());
    }

    #[test]
    fn patches_net_input_amount_for_pump_amm_and_bags_buys() {
        let net_lamports = 99_900_000u64;
        let mut pump_data = PUMP_AMM_BUY_EXACT_QUOTE_IN_DISCRIMINATOR.to_vec();
        pump_data.extend_from_slice(&123u64.to_le_bytes());
        pump_data.extend_from_slice(&456u64.to_le_bytes());
        let patched_pump =
            patch_amm_wsol_input_amount(pump_amm_program_id(), &pump_data, net_lamports);
        assert_eq!(&patched_pump[8..16], &net_lamports.to_le_bytes());

        let mut bags_data = BAGS_SWAP_DISCRIMINATOR.to_vec();
        bags_data.extend_from_slice(&123u64.to_le_bytes());
        bags_data.extend_from_slice(&456u64.to_le_bytes());
        let patched_bags =
            patch_amm_wsol_input_amount(bags_damm_v2_program_id(), &bags_data, net_lamports);
        assert_eq!(&patched_bags[8..16], &net_lamports.to_le_bytes());
    }

    #[test]
    fn shared_lookup_table_usage_requires_exact_shared_alt() {
        assert!(
            validate_shared_lookup_table_usage("wrapped", &[SHARED_SUPER_LOOKUP_TABLE.to_string()])
                .is_ok()
        );

        let missing =
            validate_shared_lookup_table_usage("wrapped", &[]).expect_err("missing shared ALT");
        assert!(missing.contains("must use exactly the shared ALT"));

        let extra = validate_shared_lookup_table_usage(
            "wrapped",
            &[
                SHARED_SUPER_LOOKUP_TABLE.to_string(),
                Pubkey::new_unique().to_string(),
            ],
        )
        .expect_err("extra ALT");
        assert!(extra.contains("must use exactly the shared ALT"));
    }

    #[test]
    fn wrapped_compute_budget_gets_safety_floor_and_overhead() {
        assert_eq!(
            recommended_wrapped_compute_unit_limit(Some(120_000)),
            280_000
        );
        assert_eq!(
            recommended_wrapped_compute_unit_limit(Some(240_000)),
            300_000
        );

        let mut instructions = vec![build_compute_unit_limit_instruction(145_000)];
        assert_eq!(
            raise_wrapped_compute_unit_limit(&mut instructions, Some(145_000)),
            280_000
        );
        assert_eq!(
            u32::from_le_bytes([
                instructions[0].data[1],
                instructions[0].data[2],
                instructions[0].data[3],
                instructions[0].data[4],
            ]),
            280_000
        );

        let mut instructions = Vec::new();
        assert_eq!(
            raise_wrapped_compute_unit_limit(&mut instructions, None),
            280_000
        );
        assert_eq!(instructions.len(), 1);
        assert_eq!(instructions[0].program_id, compute_budget_program_id());
    }

    #[test]
    fn infers_token_mode_pump_buy_sol_input_from_inner_data() {
        let mut data = Vec::new();
        data.extend_from_slice(&PUMP_BUY_DISCRIMINATOR);
        data.extend_from_slice(&123u64.to_le_bytes());
        data.extend_from_slice(&456u64.to_le_bytes());
        data.push(1);
        let instruction = Instruction {
            program_id: parse_pubkey(PUMP_PROGRAM_ID, "pump").unwrap(),
            accounts: vec![],
            data,
        };
        assert_eq!(
            infer_sol_in_lamports_from_venue_instruction(&instruction),
            Some(456)
        );
    }

    #[test]
    fn infers_token_mode_pump_buy_exact_sol_input_from_inner_data() {
        let mut data = Vec::new();
        data.extend_from_slice(&PUMP_BUY_EXACT_SOL_IN_DISCRIMINATOR);
        data.extend_from_slice(&456u64.to_le_bytes());
        data.extend_from_slice(&123u64.to_le_bytes());
        data.push(1);
        let instruction = Instruction {
            program_id: parse_pubkey(PUMP_PROGRAM_ID, "pump").unwrap(),
            accounts: vec![],
            data,
        };
        assert_eq!(
            infer_sol_in_lamports_from_venue_instruction(&instruction),
            Some(456)
        );
    }

    #[test]
    fn infers_token_mode_bonk_sol_quote_buy_input_from_inner_data() {
        let mut data = Vec::new();
        data.extend_from_slice(&BONK_BUY_EXACT_IN_DISCRIMINATOR);
        data.extend_from_slice(&789u64.to_le_bytes());
        data.extend_from_slice(&456u64.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        let mut accounts = vec![AccountMeta::new_readonly(Pubkey::new_unique(), false); 11];
        accounts[10] = AccountMeta::new_readonly(wsol_mint(), false);
        let instruction = Instruction {
            program_id: parse_pubkey(BONK_LAUNCHPAD_PROGRAM_ID, "bonk").unwrap(),
            accounts,
            data,
        };
        assert_eq!(
            infer_sol_in_lamports_from_venue_instruction(&instruction),
            Some(789)
        );
    }

    #[test]
    fn does_not_infer_bonk_non_sol_quote_buy_input() {
        let mut data = Vec::new();
        data.extend_from_slice(&BONK_BUY_EXACT_IN_DISCRIMINATOR);
        data.extend_from_slice(&789u64.to_le_bytes());
        data.extend_from_slice(&456u64.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        let accounts = vec![AccountMeta::new_readonly(Pubkey::new_unique(), false); 11];
        let instruction = Instruction {
            program_id: parse_pubkey(BONK_LAUNCHPAD_PROGRAM_ID, "bonk").unwrap(),
            accounts,
            data,
        };
        assert_eq!(
            infer_sol_in_lamports_from_venue_instruction(&instruction),
            None
        );
    }
}
