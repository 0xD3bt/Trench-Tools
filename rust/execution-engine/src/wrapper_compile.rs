//! Compile and wrap SOL-touching transactions for the fee-routing
//! program.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use borsh::BorshDeserialize;
use shared_transaction_submit::compiled_transaction_signers;
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
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    rpc_client::CompiledTransaction,
    wrapper_abi::{
        ABI_VERSION, EXECUTE_PUMP_BONDING_V2_DISCRIMINATOR,
        EXECUTE_PUMP_BONDING_V2_FIXED_ACCOUNT_COUNT, EXECUTE_SWAP_ROUTE_DISCRIMINATOR,
        EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT, EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT,
        ExecuteAccounts, ExecutePumpBondingV2Accounts, ExecutePumpBondingV2Request,
        ExecuteSwapRouteAccounts, ExecuteSwapRouteRequest, MAX_FEE_BPS, PROGRAM_ID,
        PumpBondingV2QuoteFeeMode, PumpBondingV2Side, SWAP_ROUTE_NO_PATCH_OFFSET,
        SwapLegInputSource, SwapRouteDirection, SwapRouteFeeMode, SwapRouteLeg, SwapRouteMode,
        SwapRouteSettlement, TOKEN_PROGRAM_ID, WSOL_MINT, WrapperRouteKind,
        build_execute_pump_bonding_v2_instruction, build_execute_swap_route_instruction,
        config_pda, instructions_sysvar_id, rent_sysvar_id, route_wsol_pda, system_program_id,
    },
};

/// Placeholder pubkey used when a route does not involve a WSOL ATA.
pub const ZEROED_WSOL_ATA_SENTINEL: Pubkey = Pubkey::new_from_array([0; 32]);

const BPS_DENOMINATOR: u128 = 10_000;
const PACKET_LIMIT_BYTES: usize = 1232;
const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
const RAYDIUM_CLMM_PROGRAM_ID: &str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";
const RAYDIUM_CPMM_PROGRAM_ID: &str = "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C";
const RAYDIUM_AMM_V4_PROGRAM_ID: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const BONK_LAUNCHPAD_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";
const PUMP_AMM_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const BAGS_DBC_PROGRAM_ID: &str = "dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN";
const BAGS_DAMM_V2_PROGRAM_ID: &str = "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG";
const ORCA_WHIRLPOOL_PROGRAM_ID: &str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";
const RAYDIUM_CLMM_SWAP_V2_DISCRIMINATOR: [u8; 8] = [43, 4, 237, 11, 26, 201, 30, 98];
const RAYDIUM_CPMM_SWAP_BASE_INPUT_DISCRIMINATOR: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];
const ORCA_WHIRLPOOL_SWAP_DISCRIMINATOR: [u8; 8] = [248, 198, 158, 145, 225, 117, 135, 200];
const PUMP_SELL_V2_DISCRIMINATOR: [u8; 8] = [93, 246, 130, 60, 231, 233, 64, 178];
const PUMP_BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR: [u8; 8] = [194, 171, 28, 70, 104, 77, 91, 47];
const BONK_BUY_EXACT_IN_DISCRIMINATOR: [u8; 8] = [250, 234, 13, 123, 213, 156, 19, 236];
const BONK_SELL_EXACT_IN_DISCRIMINATOR: [u8; 8] = [149, 39, 222, 155, 211, 124, 152, 26];
const PUMP_AMM_BUY_EXACT_QUOTE_IN_DISCRIMINATOR: [u8; 8] = [198, 46, 21, 82, 180, 217, 232, 112];
const PUMP_AMM_SELL_DISCRIMINATOR: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];
const BAGS_SWAP_DISCRIMINATOR: [u8; 8] = [248, 198, 158, 145, 225, 117, 135, 200];
const RAYDIUM_AMM_V4_SWAP_BASE_IN_DISCRIMINATOR: u8 = 9;
const SPL_TOKEN_ACCOUNT_LEN: usize = 165;
const SYSTEM_CREATE_ACCOUNT_DISCRIMINATOR: u32 = 0;
const SYSTEM_TRANSFER_DISCRIMINATOR: u32 = 2;
const SPL_TOKEN_CLOSE_ACCOUNT_DISCRIMINATOR: u8 = 9;
const SPL_TOKEN_SYNC_NATIVE_DISCRIMINATOR: u8 = 17;
const SPL_TOKEN_INITIALIZE_ACCOUNT3_DISCRIMINATOR: u8 = 18;
const RETIRED_EXECUTE_DISCRIMINATOR: u8 = 1;
const RETIRED_EXECUTE_AMM_WSOL_DISCRIMINATOR: u8 = 7;
const PUMP_V2_BUY_ACCOUNT_COUNT: usize = 27;
const PUMP_V2_SELL_ACCOUNT_COUNT: usize = 26;
const PUMP_V2_BASE_MINT_INDEX: usize = 1;
const PUMP_V2_QUOTE_MINT_INDEX: usize = 2;
const PUMP_V2_BASE_TOKEN_PROGRAM_INDEX: usize = 3;
const PUMP_V2_USER_INDEX: usize = 13;
const PUMP_V2_ASSOCIATED_BASE_USER_INDEX: usize = 14;
const PUMP_V2_ASSOCIATED_QUOTE_USER_INDEX: usize = 15;
const PUMP_V2_BUY_PROGRAM_INDEX: usize = 26;
const PUMP_V2_SELL_PROGRAM_INDEX: usize = 25;

/// Inputs the compile layer needs to build a signed wrapper transaction.
#[derive(Debug)]
pub struct WrapperCompileRequest<'a> {
    pub label: String,
    pub payer: &'a Keypair,
    pub blockhash: Hash,
    pub lookup_tables: Vec<AddressLookupTableAccount>,
    pub additional_signers: Vec<&'a Keypair>,
    pub preamble: Vec<Instruction>,
    pub postamble: Vec<Instruction>,
    pub inner_program: Pubkey,
    pub inner_ix_data: Vec<u8>,
    pub inner_accounts: Vec<AccountMeta>,
    pub fee_vault: Pubkey,
    pub fee_vault_wsol_ata: Option<Pubkey>,
    pub user_wsol_ata: Option<Pubkey>,
    pub route_kind: WrapperRouteKind,
    pub fee_bps: u16,
    pub gross_sol_in_lamports: u64,
    pub min_net_output: u64,
    pub compute_unit_limit: Option<u32>,
    pub compute_unit_price_micro_lamports: Option<u64>,
    pub inline_tip_lamports: Option<u64>,
    pub inline_tip_account: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapperCompileError {
    FeeBpsOverCap { requested: u16, cap: u16 },
    MissingLookupTables,
    WrongLookupTablesUsed { used: Vec<String>, expected: String },
    SolInRequiresGrossInput,
    SolOutRejectsGrossInput,
    InnerProgramZero,
    AccountMetaOverflow { count: usize },
    MessageCompile(String),
    SigningFailed(String),
    Serialize(String),
    InvalidRouteKind,
}

impl std::fmt::Display for WrapperCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FeeBpsOverCap { requested, cap } => {
                write!(f, "wrapper fee_bps {requested} exceeds hardcoded cap {cap}")
            }
            Self::MissingLookupTables => {
                write!(f, "wrapper compile requires at least one ALT to fit the tx")
            }
            Self::WrongLookupTablesUsed { used, expected } => write!(
                f,
                "wrapper compile did not reference expected ALT {expected}; used [{}]",
                used.join(", ")
            ),
            Self::SolInRequiresGrossInput => {
                write!(f, "SolIn route requires gross_sol_in_lamports > 0")
            }
            Self::SolOutRejectsGrossInput => write!(
                f,
                "SolOut route must set gross_sol_in_lamports to 0; fee is derived from inner CPI delta"
            ),
            Self::InnerProgramZero => write!(f, "inner_program must be non-zero"),
            Self::AccountMetaOverflow { count } => {
                write!(f, "wrapper tx account meta count {count} exceeds v0 limit")
            }
            Self::MessageCompile(error) => write!(f, "wrapper v0 compile failed: {error}"),
            Self::SigningFailed(error) => write!(f, "wrapper tx signing failed: {error}"),
            Self::Serialize(error) => write!(f, "wrapper tx serialize failed: {error}"),
            Self::InvalidRouteKind => write!(f, "wrapper route kind is not supported yet"),
        }
    }
}

impl std::error::Error for WrapperCompileError {}

/// Estimate the fee in lamports for `SolIn` routes.
pub fn estimate_sol_in_fee_lamports(gross_lamports: u64, fee_bps: u16) -> u64 {
    if gross_lamports == 0 || fee_bps == 0 {
        return 0;
    }
    let product = gross_lamports as u128 * fee_bps as u128;
    (product / BPS_DENOMINATOR) as u64
}

/// Build the production wrapper route instruction from a compile request.
pub fn build_wrapper_execute_instruction(
    request: &WrapperCompileRequest<'_>,
) -> Result<Instruction, WrapperCompileError> {
    if request.fee_bps > MAX_FEE_BPS {
        return Err(WrapperCompileError::FeeBpsOverCap {
            requested: request.fee_bps,
            cap: MAX_FEE_BPS,
        });
    }
    if request.inner_program == Pubkey::default() {
        return Err(WrapperCompileError::InnerProgramZero);
    }
    match request.route_kind {
        WrapperRouteKind::SolIn if request.gross_sol_in_lamports == 0 => {
            return Err(WrapperCompileError::SolInRequiresGrossInput);
        }
        WrapperRouteKind::SolOut if request.gross_sol_in_lamports != 0 => {
            return Err(WrapperCompileError::SolOutRejectsGrossInput);
        }
        WrapperRouteKind::SolThrough => return Err(WrapperCompileError::InvalidRouteKind),
        _ => {}
    }

    if request.user_wsol_ata.is_some() || request.fee_vault_wsol_ata.is_some() {
        // Production WSOL routes must go through `wrap_compiled_transaction`,
        // which replaces temp/user WSOL accounts with the deterministic route PDA.
        return Err(WrapperCompileError::InvalidRouteKind);
    }

    let user_pubkey = request.payer.pubkey();
    let (config_pda_pubkey, _bump) = config_pda();
    let sentinel = ZEROED_WSOL_ATA_SENTINEL;
    let instructions_sysvar = instructions_sysvar_id();
    let token_program = TOKEN_PROGRAM_ID;

    let execute_accounts = ExecuteAccounts {
        user: &user_pubkey,
        config_pda: &config_pda_pubkey,
        fee_vault: &request.fee_vault,
        fee_vault_wsol_ata: &sentinel,
        user_wsol_ata: &sentinel,
        instructions_sysvar: &instructions_sysvar,
        inner_program: &request.inner_program,
        token_program: &token_program,
    };
    let accounts = ExecuteSwapRouteAccounts {
        execute: execute_accounts,
        token_fee_vault_ata: None,
    };

    let direction = match request.route_kind {
        WrapperRouteKind::SolIn => SwapRouteDirection::Buy,
        WrapperRouteKind::SolOut => SwapRouteDirection::Sell,
        WrapperRouteKind::SolThrough => return Err(WrapperCompileError::InvalidRouteKind),
    };
    let fee_mode = match request.route_kind {
        WrapperRouteKind::SolIn => SwapRouteFeeMode::SolPre,
        WrapperRouteKind::SolOut => SwapRouteFeeMode::NativeSolPost,
        WrapperRouteKind::SolThrough => return Err(WrapperCompileError::InvalidRouteKind),
    };
    let settlement = match request.route_kind {
        WrapperRouteKind::SolIn => SwapRouteSettlement::Token,
        WrapperRouteKind::SolOut => SwapRouteSettlement::NativeSol,
        WrapperRouteKind::SolThrough => return Err(WrapperCompileError::InvalidRouteKind),
    };
    let mut route_accounts = Vec::with_capacity(1 + request.inner_accounts.len());
    route_accounts.push(AccountMeta::new_readonly(request.inner_program, false));
    route_accounts.extend(request.inner_accounts.iter().cloned());
    let accounts_len = u16::try_from(request.inner_accounts.len()).map_err(|_| {
        WrapperCompileError::AccountMetaOverflow {
            count: request.inner_accounts.len(),
        }
    })?;
    let execute_request = ExecuteSwapRouteRequest {
        version: ABI_VERSION,
        route_mode: match request.route_kind {
            WrapperRouteKind::SolIn => SwapRouteMode::SolIn,
            WrapperRouteKind::SolOut => SwapRouteMode::SolOut,
            WrapperRouteKind::SolThrough => return Err(WrapperCompileError::InvalidRouteKind),
        },
        direction,
        settlement,
        fee_mode,
        wsol_lane: 0,
        fee_bps: request.fee_bps,
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
            accounts_len,
            input_source: if matches!(request.route_kind, WrapperRouteKind::SolIn) {
                SwapLegInputSource::GrossSolNetOfFee
            } else {
                SwapLegInputSource::Fixed
            },
            input_amount: 0,
            input_patch_offset: SWAP_ROUTE_NO_PATCH_OFFSET,
            output_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
            ix_data: request.inner_ix_data.clone(),
        }],
    };

    build_execute_swap_route_instruction(&accounts, &execute_request, &route_accounts)
        .map_err(WrapperCompileError::MessageCompile)
}

/// Compile, sign, and serialize a wrapper-wrapped v0 transaction.
pub fn compile_wrapper_transaction(
    request: WrapperCompileRequest<'_>,
) -> Result<CompiledTransaction, WrapperCompileError> {
    if request.lookup_tables.is_empty() {
        return Err(WrapperCompileError::MissingLookupTables);
    }

    let execute_ix = build_wrapper_execute_instruction(&request)?;

    let mut instructions: Vec<Instruction> =
        Vec::with_capacity(request.preamble.len() + 1 + request.postamble.len());
    instructions.extend(request.preamble.iter().cloned());
    instructions.push(execute_ix);
    instructions.extend(request.postamble.iter().cloned());

    let mut signers: Vec<&Keypair> = Vec::with_capacity(1 + request.additional_signers.len());
    signers.push(request.payer);
    signers.extend_from_slice(&request.additional_signers);

    let message = v0::Message::try_compile(
        &request.payer.pubkey(),
        &instructions,
        &request.lookup_tables,
        request.blockhash,
    )
    .map_err(|error| WrapperCompileError::MessageCompile(error.to_string()))?;
    let required_signer_count = usize::from(message.header.num_required_signatures);
    let required_signers = message
        .account_keys
        .iter()
        .take(required_signer_count)
        .copied()
        .collect::<Vec<_>>();
    if required_signers.as_slice() != [request.payer.pubkey()] {
        return Err(WrapperCompileError::SigningFailed(format!(
            "v3 wrapper transactions must require exactly one signer (user); got [{}]",
            required_signers
                .iter()
                .map(Pubkey::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    let lookup_tables_used: Vec<String> = message
        .address_table_lookups
        .iter()
        .map(|lookup| lookup.account_key.to_string())
        .collect();

    // At least one ALT must be used or the wrapped tx is unlikely to fit.
    if lookup_tables_used.is_empty() {
        return Err(WrapperCompileError::WrongLookupTablesUsed {
            used: Vec::new(),
            expected: request
                .lookup_tables
                .first()
                .map(|t| t.key.to_string())
                .unwrap_or_else(|| "<unknown>".to_string()),
        });
    }

    let message_for_diagnostics = message.clone();
    let transaction = VersionedTransaction::try_new(VersionedMessage::V0(message), &signers)
        .map_err(|error| WrapperCompileError::SigningFailed(error.to_string()))?;

    let signature = transaction
        .signatures
        .first()
        .map(|value| value.to_string());

    let serialized = bincode::serialize(&transaction)
        .map_err(|error| WrapperCompileError::Serialize(error.to_string()))?;
    crate::alt_diagnostics::emit_alt_coverage_diagnostics(
        "execution-engine",
        &request.label,
        &instructions,
        &request.lookup_tables,
        &message_for_diagnostics,
        Some(serialized.len()),
        &[],
    );

    eprintln!(
        "[execution-engine][wrapper-compile] label={} route={:?} fee_bps={} gross_in={} min_net_out={} alts=[{}] inner_program={} bytes={}",
        request.label,
        request.route_kind,
        request.fee_bps,
        request.gross_sol_in_lamports,
        request.min_net_output,
        lookup_tables_used.join(","),
        request.inner_program,
        serialized.len()
    );

    Ok(CompiledTransaction {
        label: request.label,
        format: "v0-alt-wrapper".to_string(),
        serialized_base64: BASE64.encode(serialized),
        signature,
        lookup_tables_used,
        compute_unit_limit: request.compute_unit_limit.map(u64::from),
        compute_unit_price_micro_lamports: request.compute_unit_price_micro_lamports,
        inline_tip_lamports: request.inline_tip_lamports,
        inline_tip_account: request.inline_tip_account,
    })
}

/// Returns true if the given program id is the deployed wrapper id.
pub fn is_wrapper_program(program_id: &Pubkey) -> bool {
    *program_id == PROGRAM_ID
}

/// Inputs needed to translate a native transaction into a wrapped one.
#[derive(Debug, Clone)]
pub struct WrapCompiledTransactionRequest {
    pub label: String,
    pub route_kind: WrapperRouteKind,
    pub fee_bps: u16,
    pub fee_vault: Pubkey,
    pub gross_sol_in_lamports: u64,
    pub min_net_output: u64,
    pub select_first_allowlisted_venue_instruction: bool,
    pub select_last_allowlisted_venue_instruction: bool,
}

/// Errors that can be reported by `wrap_compiled_transaction`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapCompiledTransactionError {
    InvalidSerializedTx(String),
    UnsupportedMessageVersion,
    AltAccountMissing(String),
    AltIndexOutOfBounds { alt: String, index: u8 },
    NoVenueInstruction,
    AlreadyWrapped,
    MultipleVenueInstructions { count: usize },
    VenueAccountOutOfBounds,
    CompileFailed(String),
    SigningFailed(String),
    SerializeFailed(String),
    LookupTablesMissing,
    InvalidRouteKind,
}

impl std::fmt::Display for WrapCompiledTransactionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSerializedTx(error) => {
                write!(f, "failed to decode wrapper candidate tx bytes: {error}")
            }
            Self::UnsupportedMessageVersion => {
                write!(f, "wrapper wrap only supports v0 messages")
            }
            Self::AltAccountMissing(key) => {
                write!(f, "tx referenced ALT {key} but that ALT was not supplied")
            }
            Self::AltIndexOutOfBounds { alt, index } => {
                write!(f, "ALT {alt} index {index} is out of bounds")
            }
            Self::NoVenueInstruction => write!(
                f,
                "no allowlisted venue instruction found inside the tx — cannot wrap"
            ),
            Self::AlreadyWrapped => write!(f, "transaction is already wrapped"),
            Self::MultipleVenueInstructions { count } => write!(
                f,
                "found {count} allowlisted venue instructions; wrapper only supports exactly one per tx"
            ),
            Self::VenueAccountOutOfBounds => {
                write!(
                    f,
                    "venue instruction referenced an account index out of bounds"
                )
            }
            Self::CompileFailed(error) => write!(f, "wrapper wrap v0 compile failed: {error}"),
            Self::SigningFailed(error) => write!(f, "wrapper wrap signing failed: {error}"),
            Self::SerializeFailed(error) => write!(f, "wrapper wrap serialize failed: {error}"),
            Self::LookupTablesMissing => write!(
                f,
                "wrapper wrap requires the same ALTs that produced the source tx"
            ),
            Self::InvalidRouteKind => write!(f, "wrapper wrap route kind is not supported yet"),
        }
    }
}

impl std::error::Error for WrapCompiledTransactionError {}

/// Decompile a v0 transaction back to instructions with full metas.
fn decompile_v0_transaction(
    transaction: &VersionedTransaction,
    lookup_tables: &[AddressLookupTableAccount],
) -> Result<(Vec<Instruction>, Vec<Pubkey>), WrapCompiledTransactionError> {
    let mut account_keys = transaction.message.static_account_keys().to_vec();
    if let Some(lookups) = transaction.message.address_table_lookups() {
        let mut writable = Vec::new();
        let mut readonly = Vec::new();
        for lookup in lookups {
            let table = lookup_tables
                .iter()
                .find(|table| table.key == lookup.account_key)
                .ok_or_else(|| {
                    WrapCompiledTransactionError::AltAccountMissing(lookup.account_key.to_string())
                })?;
            for index in &lookup.writable_indexes {
                let address = table.addresses.get(usize::from(*index)).ok_or_else(|| {
                    WrapCompiledTransactionError::AltIndexOutOfBounds {
                        alt: table.key.to_string(),
                        index: *index,
                    }
                })?;
                writable.push(*address);
            }
            for index in &lookup.readonly_indexes {
                let address = table.addresses.get(usize::from(*index)).ok_or_else(|| {
                    WrapCompiledTransactionError::AltIndexOutOfBounds {
                        alt: table.key.to_string(),
                        index: *index,
                    }
                })?;
                readonly.push(*address);
            }
        }
        account_keys.extend(writable);
        account_keys.extend(readonly);
    }

    let mut instructions = Vec::new();
    for compiled in transaction.message.instructions() {
        let program_id = account_keys
            .get(usize::from(compiled.program_id_index))
            .copied()
            .ok_or(WrapCompiledTransactionError::VenueAccountOutOfBounds)?;
        let mut accounts = Vec::with_capacity(compiled.accounts.len());
        for account_index in &compiled.accounts {
            let index = usize::from(*account_index);
            let pubkey = account_keys
                .get(index)
                .copied()
                .ok_or(WrapCompiledTransactionError::VenueAccountOutOfBounds)?;
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

fn is_wrapper_execute_instruction(instruction: &Instruction) -> bool {
    if instruction.program_id != PROGRAM_ID {
        return false;
    }
    matches!(
        instruction.data.first().copied(),
        Some(
            RETIRED_EXECUTE_DISCRIMINATOR
                | RETIRED_EXECUTE_AMM_WSOL_DISCRIMINATOR
                | EXECUTE_SWAP_ROUTE_DISCRIMINATOR
                | EXECUTE_PUMP_BONDING_V2_DISCRIMINATOR
        )
    )
}

fn decode_wrapper_swap_route_instruction(
    instruction: &Instruction,
) -> Option<ExecuteSwapRouteRequest> {
    if instruction.program_id != PROGRAM_ID
        || instruction.data.first().copied() != Some(EXECUTE_SWAP_ROUTE_DISCRIMINATOR)
    {
        return None;
    }
    ExecuteSwapRouteRequest::try_from_slice(&instruction.data[1..])
        .ok()
        .filter(|request| request.version == ABI_VERSION)
}

fn decode_wrapper_pump_bonding_v2_instruction(
    instruction: &Instruction,
) -> Option<ExecutePumpBondingV2Request> {
    if instruction.program_id != PROGRAM_ID
        || instruction.data.first().copied() != Some(EXECUTE_PUMP_BONDING_V2_DISCRIMINATOR)
    {
        return None;
    }
    ExecutePumpBondingV2Request::try_from_slice(&instruction.data[1..])
        .ok()
        .filter(|request| request.version == ABI_VERSION)
}

fn swap_route_uses_wsol(request: &ExecuteSwapRouteRequest) -> bool {
    matches!(
        (request.route_mode, request.fee_mode),
        (SwapRouteMode::SolOut, SwapRouteFeeMode::WsolPost)
            | (SwapRouteMode::Mixed, SwapRouteFeeMode::SolPre)
            | (SwapRouteMode::Mixed, SwapRouteFeeMode::WsolPost)
    )
}

fn validate_already_wrapped_route_accounts(
    instruction: &Instruction,
    payer: &Pubkey,
    request: &WrapCompiledTransactionRequest,
    route: &ExecuteSwapRouteRequest,
) -> Result<(), WrapCompiledTransactionError> {
    let account = |index: usize| {
        instruction
            .accounts
            .get(index)
            .ok_or_else(|| {
                WrapCompiledTransactionError::CompileFailed(format!(
                    "already-wrapped v3 route was missing fixed account {index}"
                ))
            })
            .map(|meta| meta.pubkey)
    };
    if account(0)? != *payer || !instruction.accounts[0].is_signer {
        return Err(WrapCompiledTransactionError::CompileFailed(
            "already-wrapped v3 route user account did not match payer signer".to_string(),
        ));
    }
    if account(1)? != config_pda().0
        || account(2)? != request.fee_vault
        || account(5)? != instructions_sysvar_id()
        || account(7)? != TOKEN_PROGRAM_ID
    {
        return Err(WrapCompiledTransactionError::CompileFailed(
            "already-wrapped v3 route fixed accounts did not match wrapper configuration"
                .to_string(),
        ));
    }
    let route_offset = usize::from(route.route_accounts_offset);
    if route_offset >= instruction.accounts.len() {
        return Err(WrapCompiledTransactionError::CompileFailed(
            "already-wrapped v3 route_accounts_offset was out of bounds".to_string(),
        ));
    }
    if account(6)? != instruction.accounts[route_offset].pubkey {
        return Err(WrapCompiledTransactionError::CompileFailed(
            "already-wrapped v3 route inner program did not match first route account".to_string(),
        ));
    }
    if swap_route_uses_wsol(route) {
        if route.wsol_lane != 0 {
            return Err(WrapCompiledTransactionError::CompileFailed(
                "already-wrapped passthrough only supports WSOL lane 0".to_string(),
            ));
        }
        if account(4)? != route_wsol_pda(payer, route.wsol_lane).0
            || account(8)? != WSOL_MINT
            || account(9)? != system_program_id()
            || account(10)? != rent_sysvar_id()
        {
            return Err(WrapCompiledTransactionError::CompileFailed(
                "already-wrapped v3 WSOL route accounts did not match expected PDAs/programs"
                    .to_string(),
            ));
        }
    }
    Ok(())
}

fn validate_already_wrapped_pump_bonding_v2_accounts(
    instruction: &Instruction,
    payer: &Pubkey,
    request: &WrapCompiledTransactionRequest,
    route: &ExecutePumpBondingV2Request,
) -> Result<(), WrapCompiledTransactionError> {
    let account = |index: usize| {
        instruction
            .accounts
            .get(index)
            .ok_or_else(|| {
                WrapCompiledTransactionError::CompileFailed(format!(
                    "already-wrapped Pump v2 route was missing fixed account {index}"
                ))
            })
            .map(|meta| meta.pubkey)
    };
    if account(0)? != *payer || !instruction.accounts[0].is_signer {
        return Err(WrapCompiledTransactionError::CompileFailed(
            "already-wrapped Pump v2 route user account did not match payer signer".to_string(),
        ));
    }
    let expected_fee_vault_quote_ata = get_associated_token_address_with_program_id(
        &request.fee_vault,
        &WSOL_MINT,
        &TOKEN_PROGRAM_ID,
    );
    let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).map_err(|error| {
        WrapCompiledTransactionError::CompileFailed(format!("invalid Pump program id: {error}"))
    })?;
    if account(1)? != config_pda().0
        || account(2)? != request.fee_vault
        || account(3)? != expected_fee_vault_quote_ata
        || account(4)? != instructions_sysvar_id()
        || account(5)? != pump_program
    {
        return Err(WrapCompiledTransactionError::CompileFailed(
            "already-wrapped Pump v2 fixed accounts did not match wrapper configuration"
                .to_string(),
        ));
    }
    let fixed_count = usize::from(EXECUTE_PUMP_BONDING_V2_FIXED_ACCOUNT_COUNT);
    let pump_accounts = instruction.accounts.get(fixed_count..).ok_or_else(|| {
        WrapCompiledTransactionError::CompileFailed(
            "already-wrapped Pump v2 route was missing inner Pump accounts".to_string(),
        )
    })?;
    if route.fee_bps != request.fee_bps {
        return Err(WrapCompiledTransactionError::CompileFailed(format!(
            "already-wrapped Pump v2 fee_bps {} did not match expected {}",
            route.fee_bps, request.fee_bps
        )));
    }
    if route.quote_fee_mode != PumpBondingV2QuoteFeeMode::Wsol {
        return Err(WrapCompiledTransactionError::CompileFailed(
            "already-wrapped Pump v2 quote fee mode was not WSOL".to_string(),
        ));
    }
    match request.route_kind {
        WrapperRouteKind::SolIn => {
            if route.gross_quote_in_amount != request.gross_sol_in_lamports {
                return Err(WrapCompiledTransactionError::CompileFailed(format!(
                    "already-wrapped Pump v2 gross quote input {} did not match expected {}",
                    route.gross_quote_in_amount, request.gross_sol_in_lamports
                )));
            }
            if route.min_base_out_amount < request.min_net_output {
                return Err(WrapCompiledTransactionError::CompileFailed(format!(
                    "already-wrapped Pump v2 min base output {} was below expected {}",
                    route.min_base_out_amount, request.min_net_output
                )));
            }
            if route.base_amount_in != 0
                || route.gross_min_quote_out_amount != 0
                || route.net_min_quote_out_amount != 0
            {
                return Err(WrapCompiledTransactionError::CompileFailed(
                    "already-wrapped Pump v2 buy route carried sell-only amounts".to_string(),
                ));
            }
            validate_pump_bonding_v2_inner_accounts(
                pump_accounts,
                &PUMP_BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR,
                payer,
                &pump_program,
            )
            .map_err(WrapCompiledTransactionError::CompileFailed)?;
        }
        WrapperRouteKind::SolOut => {
            if route.gross_quote_in_amount != 0 || route.min_base_out_amount != 0 {
                return Err(WrapCompiledTransactionError::CompileFailed(
                    "already-wrapped Pump v2 sell route carried buy-only amounts".to_string(),
                ));
            }
            if route.base_amount_in == 0 {
                return Err(WrapCompiledTransactionError::CompileFailed(
                    "already-wrapped Pump v2 sell route had zero base input".to_string(),
                ));
            }
            if route.net_min_quote_out_amount < request.min_net_output {
                return Err(WrapCompiledTransactionError::CompileFailed(format!(
                    "already-wrapped Pump v2 net quote output {} was below expected {}",
                    route.net_min_quote_out_amount, request.min_net_output
                )));
            }
            validate_pump_bonding_v2_inner_accounts(
                pump_accounts,
                &PUMP_SELL_V2_DISCRIMINATOR,
                payer,
                &pump_program,
            )
            .map_err(WrapCompiledTransactionError::CompileFailed)?;
        }
        WrapperRouteKind::SolThrough => return Err(WrapCompiledTransactionError::InvalidRouteKind),
    }
    Ok(())
}

pub fn validate_already_wrapped_transaction(
    source: &CompiledTransaction,
    payer: &Pubkey,
    lookup_tables: &[AddressLookupTableAccount],
    request: &WrapCompiledTransactionRequest,
) -> Result<(), WrapCompiledTransactionError> {
    let bytes = BASE64
        .decode(source.serialized_base64.as_bytes())
        .map_err(|error| WrapCompiledTransactionError::InvalidSerializedTx(error.to_string()))?;
    let transaction: VersionedTransaction = bincode::deserialize(&bytes)
        .map_err(|error| WrapCompiledTransactionError::InvalidSerializedTx(error.to_string()))?;
    let required_signer_count = usize::from(transaction.message.header().num_required_signatures);
    let required_signers = transaction
        .message
        .static_account_keys()
        .iter()
        .take(required_signer_count)
        .copied()
        .collect::<Vec<_>>();
    if required_signers.as_slice() != [*payer] {
        return Err(WrapCompiledTransactionError::CompileFailed(format!(
            "already-wrapped v3 transaction must require exactly one signer (user); got [{}]",
            required_signers
                .iter()
                .map(Pubkey::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    let (instructions, _account_keys) = decompile_v0_transaction(&transaction, lookup_tables)?;
    if instructions.iter().any(|instruction| {
        instruction.program_id == PROGRAM_ID
            && matches!(
                instruction.data.first().copied(),
                Some(RETIRED_EXECUTE_DISCRIMINATOR | RETIRED_EXECUTE_AMM_WSOL_DISCRIMINATOR)
            )
    }) {
        return Err(WrapCompiledTransactionError::CompileFailed(
            "already-wrapped passthrough requires current wrapper instructions, not retired legacy wrapper instructions"
                .to_string(),
        ));
    }
    let wrapper_count = instructions
        .iter()
        .filter(|instruction| is_wrapper_execute_instruction(instruction))
        .count();
    let routes = instructions
        .iter()
        .filter_map(|instruction| {
            decode_wrapper_swap_route_instruction(instruction).map(|request| (instruction, request))
        })
        .collect::<Vec<_>>();
    let pump_routes = instructions
        .iter()
        .filter_map(|instruction| {
            decode_wrapper_pump_bonding_v2_instruction(instruction)
                .map(|request| (instruction, request))
        })
        .collect::<Vec<_>>();
    if routes.len() + pump_routes.len() != 1 {
        return Err(WrapCompiledTransactionError::CompileFailed(format!(
            "already-wrapped passthrough expected exactly one current wrapper instruction, found {}",
            routes.len() + pump_routes.len()
        )));
    }
    if wrapper_count != routes.len() + pump_routes.len() {
        return Err(WrapCompiledTransactionError::CompileFailed(
            "already-wrapped passthrough found a malformed or non-current wrapper instruction"
                .to_string(),
        ));
    }
    if let [(pump_instruction, pump_route)] = pump_routes.as_slice() {
        match request.route_kind {
            WrapperRouteKind::SolIn if pump_route.side != PumpBondingV2Side::Buy => {
                return Err(WrapCompiledTransactionError::CompileFailed(
                    "already-wrapped Pump v2 route metadata did not match expected buy shape"
                        .to_string(),
                ));
            }
            WrapperRouteKind::SolOut if pump_route.side != PumpBondingV2Side::Sell => {
                return Err(WrapCompiledTransactionError::CompileFailed(
                    "already-wrapped Pump v2 route metadata did not match expected sell shape"
                        .to_string(),
                ));
            }
            WrapperRouteKind::SolThrough => {
                return Err(WrapCompiledTransactionError::InvalidRouteKind);
            }
            _ => {}
        }
        validate_already_wrapped_pump_bonding_v2_accounts(
            pump_instruction,
            payer,
            request,
            pump_route,
        )?;
        return Ok(());
    }
    let [(route_instruction, route)] = routes.as_slice() else {
        unreachable!("exactly one current wrapper route was checked above");
    };
    if route.fee_bps != request.fee_bps {
        return Err(WrapCompiledTransactionError::CompileFailed(format!(
            "already-wrapped fee_bps {} did not match expected {}",
            route.fee_bps, request.fee_bps
        )));
    }
    match request.route_kind {
        WrapperRouteKind::SolIn => {
            if route.direction != SwapRouteDirection::Buy
                || route.settlement != SwapRouteSettlement::Token
                || route.fee_mode != SwapRouteFeeMode::SolPre
                || !matches!(
                    route.route_mode,
                    SwapRouteMode::SolIn | SwapRouteMode::Mixed
                )
            {
                return Err(WrapCompiledTransactionError::CompileFailed(
                    "already-wrapped SolIn route metadata did not match expected buy/SolPre shape"
                        .to_string(),
                ));
            }
            if route.gross_sol_in_lamports != request.gross_sol_in_lamports {
                return Err(WrapCompiledTransactionError::CompileFailed(format!(
                    "already-wrapped gross SOL input {} did not match expected {}",
                    route.gross_sol_in_lamports, request.gross_sol_in_lamports
                )));
            }
        }
        WrapperRouteKind::SolOut => {
            if route.direction != SwapRouteDirection::Sell
                || !matches!(
                    route.settlement,
                    SwapRouteSettlement::NativeSol | SwapRouteSettlement::Wsol
                )
                || !matches!(
                    route.fee_mode,
                    SwapRouteFeeMode::NativeSolPost | SwapRouteFeeMode::WsolPost
                )
                || !matches!(
                    route.route_mode,
                    SwapRouteMode::SolOut | SwapRouteMode::Mixed
                )
            {
                return Err(WrapCompiledTransactionError::CompileFailed(
                    "already-wrapped SolOut route metadata did not match expected sell/post-fee shape"
                        .to_string(),
                ));
            }
            if route.gross_sol_in_lamports != 0 {
                return Err(WrapCompiledTransactionError::CompileFailed(
                    "already-wrapped SolOut route unexpectedly set gross SOL input".to_string(),
                ));
            }
        }
        WrapperRouteKind::SolThrough => return Err(WrapCompiledTransactionError::InvalidRouteKind),
    }
    if request.min_net_output > 0 && route.min_net_output < request.min_net_output {
        return Err(WrapCompiledTransactionError::CompileFailed(format!(
            "already-wrapped min output {} was below expected {}",
            route.min_net_output, request.min_net_output
        )));
    }
    validate_already_wrapped_route_accounts(route_instruction, payer, request, route)?;
    Ok(())
}

/// Derive the user WSOL settlement account and fee-vault WSOL ATA for
/// WSOL-settled routes. The user-side account can be either the
/// payer's WSOL ATA or a temporary wrapped-SOL account created around
/// the venue leg.
fn derive_wsol_route_accounts(
    payer: &Pubkey,
    fee_vault: &Pubkey,
    route_kind: WrapperRouteKind,
    inner_program: &Pubkey,
    venue_ix_data: &[u8],
    venue_accounts: &[AccountMeta],
    preamble: &[Instruction],
    postamble: &[Instruction],
) -> Result<(Option<Pubkey>, Option<Pubkey>), WrapCompiledTransactionError> {
    let payer_wsol_ata =
        get_associated_token_address_with_program_id(payer, &WSOL_MINT, &TOKEN_PROGRAM_ID);
    let has_payer_wsol_ata = venue_accounts
        .iter()
        .any(|meta| meta.pubkey == payer_wsol_ata);
    let user_wsol_account = if has_payer_wsol_ata
        && !is_pump_bonding_v2_wsol_quote_instruction(inner_program, venue_ix_data, venue_accounts)
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
        )
        .map_err(WrapCompiledTransactionError::CompileFailed)?
    };
    let fee_vault_wsol_ata = user_wsol_account.map(|_| {
        get_associated_token_address_with_program_id(fee_vault, &WSOL_MINT, &TOKEN_PROGRAM_ID)
    });
    Ok((user_wsol_account, fee_vault_wsol_ata))
}

fn derive_temp_wsol_output_account(
    inner_program: &Pubkey,
    venue_accounts: &[AccountMeta],
) -> Option<Pubkey> {
    if *inner_program == raydium_clmm_program_id() {
        return derive_raydium_clmm_wsol_output_account(venue_accounts);
    }
    if *inner_program == raydium_cpmm_program_id() {
        return derive_raydium_cpmm_wsol_output_account(venue_accounts);
    }
    if *inner_program == raydium_amm_v4_program_id() {
        return derive_raydium_amm_v4_wsol_output_account(venue_accounts);
    }
    None
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
        if instruction.program_id != TOKEN_PROGRAM_ID
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
        instruction.program_id == TOKEN_PROGRAM_ID
            && instruction.data.first().copied()
                == Some(SPL_TOKEN_INITIALIZE_ACCOUNT3_DISCRIMINATOR)
            && instruction.accounts.first().map(|meta| meta.pubkey) == Some(*account)
            && instruction.accounts.get(1).map(|meta| meta.pubkey) == Some(WSOL_MINT)
    })
}

fn derive_amm_wsol_input_account_index(
    inner_program: &Pubkey,
    venue_accounts: &[AccountMeta],
    preamble: &[Instruction],
) -> Option<usize> {
    if *inner_program == raydium_clmm_program_id() {
        return derive_raydium_clmm_wsol_input_account_index(venue_accounts);
    }
    if *inner_program == raydium_cpmm_program_id() {
        return derive_raydium_cpmm_wsol_input_account_index(venue_accounts);
    }
    if *inner_program == raydium_amm_v4_program_id() {
        return derive_raydium_amm_v4_wsol_input_account_index(venue_accounts);
    }
    if *inner_program == bonk_launchpad_program_id() {
        return derive_bonk_launchpad_wsol_input_account_index(venue_accounts);
    }
    derive_generic_wsol_input_account_index(venue_accounts, preamble)
}

fn is_pump_bonding_v2_wsol_quote_instruction(
    inner_program: &Pubkey,
    venue_ix_data: &[u8],
    venue_accounts: &[AccountMeta],
) -> bool {
    let Ok(pump) = Pubkey::from_str(PUMP_PROGRAM_ID) else {
        return false;
    };
    if *inner_program != pump {
        return false;
    }
    let Some(discriminator) = venue_ix_data.get(0..8) else {
        return false;
    };
    if discriminator != PUMP_BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR
        && discriminator != PUMP_SELL_V2_DISCRIMINATOR
    {
        return false;
    }
    venue_accounts
        .get(PUMP_V2_QUOTE_MINT_INDEX)
        .map(|meta| meta.pubkey == WSOL_MINT)
        .unwrap_or(false)
}

fn parse_pump_bonding_v2_amounts(data: &[u8]) -> Option<([u8; 8], u64, u64)> {
    let discriminator = data.get(0..8)?.try_into().ok()?;
    let amount = read_u64_le(data, 8..16)?;
    let limit = read_u64_le(data, 16..24)?;
    Some((discriminator, amount, limit))
}

fn pump_bonding_v2_expected_account_layout(discriminator: &[u8; 8]) -> Option<(usize, usize)> {
    match discriminator {
        &PUMP_BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR => {
            Some((PUMP_V2_BUY_ACCOUNT_COUNT, PUMP_V2_BUY_PROGRAM_INDEX))
        }
        &PUMP_SELL_V2_DISCRIMINATOR => {
            Some((PUMP_V2_SELL_ACCOUNT_COUNT, PUMP_V2_SELL_PROGRAM_INDEX))
        }
        _ => None,
    }
}

fn validate_pump_bonding_v2_inner_accounts(
    accounts: &[AccountMeta],
    discriminator: &[u8; 8],
    payer: &Pubkey,
    pump_program: &Pubkey,
) -> Result<(), String> {
    let (expected_account_count, program_index) =
        pump_bonding_v2_expected_account_layout(discriminator).ok_or_else(|| {
            "Pump bonding v2 instruction discriminator was not supported".to_string()
        })?;
    if accounts.len() != expected_account_count {
        return Err(format!(
            "Pump bonding v2 wrapper expected {expected_account_count} accounts, got {}",
            accounts.len()
        ));
    }
    if accounts.get(PUMP_V2_USER_INDEX).map(|meta| meta.pubkey) != Some(*payer)
        || !accounts
            .get(PUMP_V2_USER_INDEX)
            .map(|meta| meta.is_signer)
            .unwrap_or(false)
    {
        return Err("Pump bonding v2 wrapper user account did not match payer signer".to_string());
    }
    if accounts.get(program_index).map(|meta| meta.pubkey) != Some(*pump_program) {
        return Err(
            "Pump bonding v2 wrapper program account did not match venue program".to_string(),
        );
    }
    let quote_ata =
        get_associated_token_address_with_program_id(payer, &WSOL_MINT, &TOKEN_PROGRAM_ID);
    if accounts
        .get(PUMP_V2_ASSOCIATED_QUOTE_USER_INDEX)
        .map(|meta| meta.pubkey)
        != Some(quote_ata)
    {
        return Err(
            "Pump bonding v2 wrapper quote user account did not match payer WSOL ATA".to_string(),
        );
    }
    if accounts
        .get(PUMP_V2_QUOTE_MINT_INDEX)
        .map(|meta| meta.pubkey)
        != Some(WSOL_MINT)
    {
        return Err("Pump bonding v2 wrapper quote mint was not WSOL".to_string());
    }
    Ok(())
}

fn try_build_pump_bonding_v2_wrapper_instruction(
    payer: &Pubkey,
    request: &WrapCompiledTransactionRequest,
    venue_ix: &Instruction,
) -> Result<Option<Instruction>, WrapCompiledTransactionError> {
    if !is_pump_bonding_v2_wsol_quote_instruction(
        &venue_ix.program_id,
        &venue_ix.data,
        &venue_ix.accounts,
    ) {
        return Ok(None);
    }
    let (discriminator, amount, limit) =
        parse_pump_bonding_v2_amounts(&venue_ix.data).ok_or_else(|| {
            WrapCompiledTransactionError::CompileFailed(
                "Pump bonding v2 instruction data was malformed".to_string(),
            )
        })?;
    validate_pump_bonding_v2_inner_accounts(
        &venue_ix.accounts,
        &discriminator,
        payer,
        &venue_ix.program_id,
    )
    .map_err(WrapCompiledTransactionError::CompileFailed)?;
    let wrapper_request = match (request.route_kind, discriminator) {
        (WrapperRouteKind::SolIn, PUMP_BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR) => {
            let expected_amount = request
                .gross_sol_in_lamports
                .checked_sub(estimate_sol_in_fee_lamports(
                    request.gross_sol_in_lamports,
                    request.fee_bps,
                ))
                .ok_or_else(|| {
                    WrapCompiledTransactionError::CompileFailed(
                        "Pump bonding v2 wrapper fee exceeds gross quote input".to_string(),
                    )
                })?;
            if amount != expected_amount {
                return Err(WrapCompiledTransactionError::CompileFailed(format!(
                    "Pump bonding v2 wrapper buy amount {amount} did not match net quote input {expected_amount}"
                )));
            }
            ExecutePumpBondingV2Request {
                version: ABI_VERSION,
                side: PumpBondingV2Side::Buy,
                quote_fee_mode: PumpBondingV2QuoteFeeMode::Wsol,
                fee_bps: request.fee_bps,
                gross_quote_in_amount: request.gross_sol_in_lamports,
                min_base_out_amount: limit,
                base_amount_in: 0,
                gross_min_quote_out_amount: 0,
                net_min_quote_out_amount: 0,
            }
        }
        (WrapperRouteKind::SolOut, PUMP_SELL_V2_DISCRIMINATOR) => ExecutePumpBondingV2Request {
            version: ABI_VERSION,
            side: PumpBondingV2Side::Sell,
            quote_fee_mode: PumpBondingV2QuoteFeeMode::Wsol,
            fee_bps: request.fee_bps,
            gross_quote_in_amount: 0,
            min_base_out_amount: 0,
            base_amount_in: amount,
            gross_min_quote_out_amount: limit,
            net_min_quote_out_amount: request.min_net_output,
        },
        _ => {
            return Err(WrapCompiledTransactionError::CompileFailed(
                "Pump bonding v2 wrapper route kind did not match instruction side".to_string(),
            ));
        }
    };
    let (config_pda_pubkey, _bump) = config_pda();
    let fee_vault_quote_ata = get_associated_token_address_with_program_id(
        &request.fee_vault,
        &WSOL_MINT,
        &TOKEN_PROGRAM_ID,
    );
    let instructions_sysvar = instructions_sysvar_id();
    let accounts = ExecutePumpBondingV2Accounts {
        user: payer,
        config_pda: &config_pda_pubkey,
        fee_vault: &request.fee_vault,
        fee_vault_quote_ata: &fee_vault_quote_ata,
        instructions_sysvar: &instructions_sysvar,
        pump_program: &venue_ix.program_id,
    };
    build_execute_pump_bonding_v2_instruction(&accounts, &wrapper_request, &venue_ix.accounts)
        .map(Some)
        .map_err(WrapCompiledTransactionError::CompileFailed)
}

fn build_create_wsol_ata_instruction(payer: &Pubkey, owner: &Pubkey) -> Instruction {
    spl_associated_token_account::instruction::create_associated_token_account_idempotent(
        payer,
        owner,
        &WSOL_MINT,
        &TOKEN_PROGRAM_ID,
    )
}

fn build_create_pump_v2_base_ata_instruction(
    payer: &Pubkey,
    venue_ix: &Instruction,
) -> Result<Instruction, WrapCompiledTransactionError> {
    let base_mint = venue_ix
        .accounts
        .get(PUMP_V2_BASE_MINT_INDEX)
        .map(|meta| meta.pubkey)
        .ok_or(WrapCompiledTransactionError::VenueAccountOutOfBounds)?;
    let base_token_program = venue_ix
        .accounts
        .get(PUMP_V2_BASE_TOKEN_PROGRAM_INDEX)
        .map(|meta| meta.pubkey)
        .ok_or(WrapCompiledTransactionError::VenueAccountOutOfBounds)?;
    let expected_base_ata =
        get_associated_token_address_with_program_id(payer, &base_mint, &base_token_program);
    if venue_ix
        .accounts
        .get(PUMP_V2_ASSOCIATED_BASE_USER_INDEX)
        .map(|meta| meta.pubkey)
        != Some(expected_base_ata)
    {
        return Err(WrapCompiledTransactionError::CompileFailed(
            "Pump bonding v2 wrapper base user account did not match payer base ATA".to_string(),
        ));
    }
    Ok(
        spl_associated_token_account::instruction::create_associated_token_account_idempotent(
            payer,
            payer,
            &base_mint,
            &base_token_program,
        ),
    )
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

fn derive_raydium_clmm_wsol_input_account_index(venue_accounts: &[AccountMeta]) -> Option<usize> {
    let user_input_account = venue_accounts.get(3)?;
    let input_mint = venue_accounts.get(11)?;
    (user_input_account.is_writable && input_mint.pubkey == WSOL_MINT).then_some(3)
}

fn derive_raydium_cpmm_wsol_input_account_index(venue_accounts: &[AccountMeta]) -> Option<usize> {
    let user_input_account = venue_accounts.get(4)?;
    let input_mint = venue_accounts.get(10)?;
    (user_input_account.is_writable && input_mint.pubkey == WSOL_MINT).then_some(4)
}

fn derive_raydium_clmm_wsol_output_account(venue_accounts: &[AccountMeta]) -> Option<Pubkey> {
    let user_output_account = venue_accounts.get(4)?;
    let output_mint = venue_accounts.get(12)?;
    (user_output_account.is_writable && output_mint.pubkey == WSOL_MINT)
        .then_some(user_output_account.pubkey)
}

fn derive_raydium_cpmm_wsol_output_account(venue_accounts: &[AccountMeta]) -> Option<Pubkey> {
    let user_output_account = venue_accounts.get(5)?;
    let output_mint = venue_accounts.get(11)?;
    (user_output_account.is_writable && output_mint.pubkey == WSOL_MINT)
        .then_some(user_output_account.pubkey)
}

fn derive_raydium_amm_v4_wsol_input_account_index(venue_accounts: &[AccountMeta]) -> Option<usize> {
    let user_input_account = venue_accounts.get(15)?;
    let user_owner = venue_accounts.get(17)?;
    (user_input_account.is_writable && user_owner.is_signer).then_some(15)
}

fn derive_bonk_launchpad_wsol_input_account_index(venue_accounts: &[AccountMeta]) -> Option<usize> {
    let user_input_account = venue_accounts.get(6)?;
    let input_mint = venue_accounts.get(10)?;
    (user_input_account.is_writable && input_mint.pubkey == WSOL_MINT).then_some(6)
}

fn derive_raydium_amm_v4_wsol_output_account(venue_accounts: &[AccountMeta]) -> Option<Pubkey> {
    let user_output_account = venue_accounts.get(16)?;
    let user_owner = venue_accounts.get(17)?;
    (user_output_account.is_writable && user_owner.is_signer).then_some(user_output_account.pubkey)
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
    if program_id == Pubkey::from_str(PUMP_PROGRAM_ID).ok()?
        && discriminator == PUMP_SELL_V2_DISCRIMINATOR
    {
        return read_u64_le(data, 16..24);
    }
    if program_id == pump_amm_program_id() && discriminator == PUMP_AMM_SELL_DISCRIMINATOR {
        return read_u64_le(data, 16..24);
    }
    if program_id == bonk_launchpad_program_id()
        && discriminator == BONK_SELL_EXACT_IN_DISCRIMINATOR
    {
        return read_u64_le(data, 16..24);
    }
    if (program_id == raydium_cpmm_program_id()
        && discriminator == RAYDIUM_CPMM_SWAP_BASE_INPUT_DISCRIMINATOR)
        || (program_id == raydium_clmm_program_id()
            && discriminator == RAYDIUM_CLMM_SWAP_V2_DISCRIMINATOR)
        || (program_id == orca_whirlpool_program_id()
            && discriminator == ORCA_WHIRLPOOL_SWAP_DISCRIMINATOR)
        || ((program_id == bags_dbc_program_id() || program_id == bags_damm_v2_program_id())
            && discriminator == BAGS_SWAP_DISCRIMINATOR)
    {
        return read_u64_le(data, 16..24);
    }
    if program_id == raydium_amm_v4_program_id()
        && data.first().copied() == Some(RAYDIUM_AMM_V4_SWAP_BASE_IN_DISCRIMINATOR)
    {
        return read_u64_le(data, 9..17);
    }
    None
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

fn should_append_system_program_inner_account(inner_program: &Pubkey) -> bool {
    *inner_program != raydium_amm_v4_program_id()
}

fn patch_amm_wsol_input_amount(
    inner_program: Pubkey,
    inner_ix_data: &[u8],
    net_input_lamports: u64,
) -> Vec<u8> {
    let mut patched = inner_ix_data.to_vec();
    let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).ok();
    let supports_amount_patch = (inner_program == raydium_clmm_program_id()
        && patched.get(0..8) == Some(RAYDIUM_CLMM_SWAP_V2_DISCRIMINATOR.as_slice()))
        || (inner_program == raydium_cpmm_program_id()
            && patched.get(0..8) == Some(RAYDIUM_CPMM_SWAP_BASE_INPUT_DISCRIMINATOR.as_slice()))
        || (inner_program == bonk_launchpad_program_id()
            && patched.get(0..8) == Some(BONK_BUY_EXACT_IN_DISCRIMINATOR.as_slice()))
        || (inner_program == pump_amm_program_id()
            && patched.get(0..8) == Some(PUMP_AMM_BUY_EXACT_QUOTE_IN_DISCRIMINATOR.as_slice()))
        || (pump_program == Some(inner_program)
            && patched.get(0..8) == Some(PUMP_BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR.as_slice()))
        || ((inner_program == bags_dbc_program_id() || inner_program == bags_damm_v2_program_id())
            && patched.get(0..8) == Some(BAGS_SWAP_DISCRIMINATOR.as_slice()));
    if supports_amount_patch && patched.len() >= 16 {
        patched[8..16].copy_from_slice(&net_input_lamports.to_le_bytes());
    }
    patched
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
    request: &WrapCompiledTransactionRequest,
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

    let (amm_wsol_account, _) = route_wsol_pda(payer, 0);
    let mut inner_accounts = venue_ix.accounts.clone();
    let original_meta = inner_accounts.get_mut(inner_wsol_account_index)?;
    original_meta.pubkey = amm_wsol_account;
    original_meta.is_signer = false;
    original_meta.is_writable = true;
    if should_append_system_program_inner_account(&venue_ix.program_id) {
        append_system_program_inner_account(&mut inner_accounts);
    }

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
    request: &WrapCompiledTransactionRequest,
    venue_ix: &Instruction,
    preamble: &[Instruction],
    postamble: &[Instruction],
    user_wsol_account: Option<Pubkey>,
) -> Option<AmmWsolV2Plan> {
    if !matches!(request.route_kind, WrapperRouteKind::SolOut) {
        return None;
    }
    let original_wsol_account = user_wsol_account?;
    let route_wsol_account = route_wsol_pda(payer, 0).0;
    let mut inner_accounts = venue_ix.accounts.clone();
    let output_index = inner_accounts
        .iter()
        .position(|meta| meta.pubkey == original_wsol_account && meta.is_writable)?;
    let output_meta = inner_accounts.get_mut(output_index)?;
    output_meta.pubkey = route_wsol_account;
    output_meta.is_signer = false;
    output_meta.is_writable = true;

    let preamble = strip_temp_wsol_lifecycle_instructions(preamble, &original_wsol_account)?;
    let postamble = strip_temp_wsol_lifecycle_instructions(postamble, &original_wsol_account)?;
    if should_append_system_program_inner_account(&venue_ix.program_id) {
        append_system_program_inner_account(&mut inner_accounts);
    }
    Some(AmmWsolV2Plan {
        inner_wsol_account_index: output_index,
        amm_wsol_account: route_wsol_account,
        pda_wsol_lamports: 0,
        inner_accounts,
        preamble,
        postamble,
    })
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
            (instruction.program_id == TOKEN_PROGRAM_ID
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
    Pubkey::new_from_array(owner) == TOKEN_PROGRAM_ID
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
        && instruction.accounts.get(3).map(|meta| meta.pubkey) == Some(WSOL_MINT)
}

fn is_spl_token_wsol_lifecycle_instruction(
    instruction: &Instruction,
    temp_wsol_account: &Pubkey,
) -> bool {
    if instruction.program_id != TOKEN_PROGRAM_ID
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

fn memo_program_id() -> Pubkey {
    Pubkey::from_str(MEMO_PROGRAM_ID).expect("memo program id must be valid")
}

fn build_wrapper_uniqueness_memo_instruction() -> Instruction {
    Instruction {
        program_id: memo_program_id(),
        accounts: vec![],
        data: format!("tt:{}", Uuid::new_v4().simple()).into_bytes(),
    }
}

fn raydium_clmm_program_id() -> Pubkey {
    Pubkey::from_str(RAYDIUM_CLMM_PROGRAM_ID).expect("Raydium CLMM program id must be valid")
}

fn raydium_cpmm_program_id() -> Pubkey {
    Pubkey::from_str(RAYDIUM_CPMM_PROGRAM_ID).expect("Raydium CPMM program id must be valid")
}

fn raydium_amm_v4_program_id() -> Pubkey {
    Pubkey::from_str(RAYDIUM_AMM_V4_PROGRAM_ID).expect("Raydium AMM v4 program id must be valid")
}

fn bonk_launchpad_program_id() -> Pubkey {
    Pubkey::from_str(BONK_LAUNCHPAD_PROGRAM_ID).expect("Bonk Launchpad program id must be valid")
}

fn pump_amm_program_id() -> Pubkey {
    Pubkey::from_str(PUMP_AMM_PROGRAM_ID).expect("Pump AMM program id must be valid")
}

fn orca_whirlpool_program_id() -> Pubkey {
    Pubkey::from_str(ORCA_WHIRLPOOL_PROGRAM_ID).expect("Orca Whirlpool program id must be valid")
}

fn bags_dbc_program_id() -> Pubkey {
    Pubkey::from_str(BAGS_DBC_PROGRAM_ID).expect("Bags DBC program id must be valid")
}

fn bags_damm_v2_program_id() -> Pubkey {
    Pubkey::from_str(BAGS_DAMM_V2_PROGRAM_ID).expect("Bags DAMM v2 program id must be valid")
}

fn is_memo_instruction(instruction: &Instruction) -> bool {
    instruction.program_id == memo_program_id()
}

/// Wrap an already-signed native transaction.
pub fn wrap_compiled_transaction(
    source: &CompiledTransaction,
    payer: &Keypair,
    lookup_tables: &[AddressLookupTableAccount],
    allowed_inner_programs: &[Pubkey],
    request: &WrapCompiledTransactionRequest,
) -> Result<CompiledTransaction, WrapCompiledTransactionError> {
    if matches!(request.route_kind, WrapperRouteKind::SolThrough) {
        return Err(WrapCompiledTransactionError::InvalidRouteKind);
    }
    if request.fee_bps > MAX_FEE_BPS {
        return Err(WrapCompiledTransactionError::CompileFailed(format!(
            "fee_bps {} exceeds hardcoded cap {}",
            request.fee_bps, MAX_FEE_BPS
        )));
    }
    if lookup_tables.is_empty() {
        return Err(WrapCompiledTransactionError::LookupTablesMissing);
    }

    let bytes = BASE64
        .decode(source.serialized_base64.as_bytes())
        .map_err(|error| WrapCompiledTransactionError::InvalidSerializedTx(error.to_string()))?;
    let transaction: VersionedTransaction = bincode::deserialize(&bytes)
        .map_err(|error| WrapCompiledTransactionError::InvalidSerializedTx(error.to_string()))?;

    let blockhash = match &transaction.message {
        VersionedMessage::V0(message) => message.recent_blockhash,
        _ => return Err(WrapCompiledTransactionError::UnsupportedMessageVersion),
    };

    let (instructions, _account_keys) = decompile_v0_transaction(&transaction, lookup_tables)?;
    if instructions.iter().any(is_wrapper_execute_instruction) {
        return Err(WrapCompiledTransactionError::AlreadyWrapped);
    }

    // Find the ONE venue instruction. More than one means the adapter
    // produced a multi-CPI venue (not currently used by any family) and
    // the wrapper can't split the fee across them cleanly.
    let mut venue_positions: Vec<usize> = Vec::new();
    for (idx, ix) in instructions.iter().enumerate() {
        if allowed_inner_programs.contains(&ix.program_id) {
            venue_positions.push(idx);
        }
    }
    let venue_idx = match venue_positions.as_slice() {
        [] => return Err(WrapCompiledTransactionError::NoVenueInstruction),
        [only] => *only,
        multiple => {
            if request.select_first_allowlisted_venue_instruction {
                multiple[0]
            } else if request.select_last_allowlisted_venue_instruction {
                *multiple
                    .last()
                    .expect("multiple venue instructions should not be empty")
            } else {
                return Err(WrapCompiledTransactionError::MultipleVenueInstructions {
                    count: multiple.len(),
                });
            }
        }
    };
    let mut request = request.clone();

    // Assemble the preamble (everything before venue) and postamble
    // (everything after venue). The wrapper Execute replaces the venue
    // ix in-place.
    let (preamble, rest) = instructions.split_at(venue_idx);
    let preamble: Vec<Instruction> = preamble
        .iter()
        .filter(|instruction| !is_memo_instruction(instruction))
        .cloned()
        .collect();

    let postamble: Vec<Instruction> = rest[1..]
        .iter()
        .filter(|instruction| !is_memo_instruction(instruction))
        .cloned()
        .collect();

    let venue_ix = instructions[venue_idx].clone();
    if matches!(request.route_kind, WrapperRouteKind::SolOut) && request.min_net_output == 0 {
        let gross_min_output = infer_sol_out_min_lamports_from_venue_instruction(&venue_ix)
            .ok_or_else(|| {
                WrapCompiledTransactionError::CompileFailed(format!(
                    "could not infer SOL output minimum for {}",
                    venue_ix.program_id
                ))
            })?;
        request.min_net_output = gross_min_output
            .checked_sub(estimate_sol_in_fee_lamports(
                gross_min_output,
                request.fee_bps,
            ))
            .ok_or_else(|| {
                WrapCompiledTransactionError::CompileFailed(
                    "wrapper fee exceeds minimum SOL output".to_string(),
                )
            })?;
    }
    let (user_wsol_ata, fee_vault_wsol_ata) = derive_wsol_route_accounts(
        &payer.pubkey(),
        &request.fee_vault,
        request.route_kind,
        &venue_ix.program_id,
        &venue_ix.data,
        &venue_ix.accounts,
        &preamble,
        &postamble,
    )?;
    let mut inner_accounts = venue_ix.accounts.clone();
    if should_append_system_program_inner_account(&venue_ix.program_id) {
        append_system_program_inner_account(&mut inner_accounts);
    }

    let pump_v2_wrapper =
        try_build_pump_bonding_v2_wrapper_instruction(&payer.pubkey(), &request, &venue_ix)?;
    let v3_wsol_plan =
        try_build_amm_wsol_v2_plan(&payer.pubkey(), &request, &venue_ix, &preamble, &postamble)
            .or_else(|| {
                try_build_wsol_out_v3_plan(
                    &payer.pubkey(),
                    &request,
                    &venue_ix,
                    &preamble,
                    &postamble,
                    user_wsol_ata,
                )
            });
    let (wrapper_execute, preamble, postamble, wrapper_mode) = if let Some(instruction) =
        pump_v2_wrapper
    {
        let user_pubkey = payer.pubkey();
        let mut preamble = preamble;
        if matches!(request.route_kind, WrapperRouteKind::SolIn) {
            preamble.push(build_create_pump_v2_base_ata_instruction(
                &user_pubkey,
                &venue_ix,
            )?);
        }
        preamble.push(build_create_wsol_ata_instruction(
            &user_pubkey,
            &user_pubkey,
        ));
        preamble.push(build_create_wsol_ata_instruction(
            &user_pubkey,
            &request.fee_vault,
        ));
        (instruction, preamble, postamble, "pump-v2-wrapper")
    } else if let Some(plan) = v3_wsol_plan {
        let user_pubkey = payer.pubkey();
        let (config_pda_pubkey, _bump) = config_pda();
        let fee_vault_wsol = fee_vault_wsol_ata.unwrap_or(ZEROED_WSOL_ATA_SENTINEL);
        let instructions_sysvar = instructions_sysvar_id();
        let token_program = TOKEN_PROGRAM_ID;
        let execute_accounts = ExecuteAccounts {
            user: &user_pubkey,
            config_pda: &config_pda_pubkey,
            fee_vault: &request.fee_vault,
            fee_vault_wsol_ata: &fee_vault_wsol,
            user_wsol_ata: &plan.amm_wsol_account,
            instructions_sysvar: &instructions_sysvar,
            inner_program: &venue_ix.program_id,
            token_program: &token_program,
        };
        let accounts = ExecuteSwapRouteAccounts {
            execute: execute_accounts,
            token_fee_vault_ata: None,
        };
        let route_mode = match request.route_kind {
            WrapperRouteKind::SolIn => SwapRouteMode::Mixed,
            WrapperRouteKind::SolOut => SwapRouteMode::SolOut,
            WrapperRouteKind::SolThrough => {
                return Err(WrapCompiledTransactionError::InvalidRouteKind);
            }
        };
        let direction = match request.route_kind {
            WrapperRouteKind::SolIn => SwapRouteDirection::Buy,
            WrapperRouteKind::SolOut => SwapRouteDirection::Sell,
            WrapperRouteKind::SolThrough => {
                return Err(WrapCompiledTransactionError::InvalidRouteKind);
            }
        };
        let fee_mode = match request.route_kind {
            WrapperRouteKind::SolIn => SwapRouteFeeMode::SolPre,
            WrapperRouteKind::SolOut => SwapRouteFeeMode::WsolPost,
            WrapperRouteKind::SolThrough => {
                return Err(WrapCompiledTransactionError::InvalidRouteKind);
            }
        };
        let settlement = match request.route_kind {
            WrapperRouteKind::SolIn => SwapRouteSettlement::Token,
            WrapperRouteKind::SolOut => SwapRouteSettlement::Wsol,
            WrapperRouteKind::SolThrough => {
                return Err(WrapCompiledTransactionError::InvalidRouteKind);
            }
        };
        let mut route_accounts = Vec::with_capacity(1 + plan.inner_accounts.len());
        route_accounts.push(AccountMeta::new_readonly(venue_ix.program_id, false));
        route_accounts.extend(plan.inner_accounts.iter().cloned());
        let accounts_len = u16::try_from(plan.inner_accounts.len())
            .map_err(|_| WrapCompiledTransactionError::VenueAccountOutOfBounds)?;
        let output_account_index = if matches!(request.route_kind, WrapperRouteKind::SolOut) {
            u16::try_from(1usize.saturating_add(plan.inner_wsol_account_index))
                .map_err(|_| WrapCompiledTransactionError::VenueAccountOutOfBounds)?
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
            fee_bps: request.fee_bps,
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
                accounts_len,
                input_source: if matches!(request.route_kind, WrapperRouteKind::SolIn) {
                    SwapLegInputSource::GrossSolNetOfFee
                } else {
                    SwapLegInputSource::Fixed
                },
                input_amount: plan.pda_wsol_lamports,
                input_patch_offset: SWAP_ROUTE_NO_PATCH_OFFSET,
                output_account_index,
                ix_data: if matches!(request.route_kind, WrapperRouteKind::SolIn) {
                    patch_amm_wsol_input_amount(
                        venue_ix.program_id,
                        &venue_ix.data,
                        plan.pda_wsol_lamports,
                    )
                } else {
                    venue_ix.data.clone()
                },
            }],
        };
        let instruction =
            build_execute_swap_route_instruction(&accounts, &execute_request, &route_accounts)
                .map_err(|error| WrapCompiledTransactionError::CompileFailed(error.to_string()))?;
        (instruction, plan.preamble, plan.postamble, "v3-route-wsol")
    } else {
        let instruction = build_wrapper_execute_instruction(&WrapperCompileRequest {
            label: source.label.clone(),
            payer,
            blockhash,
            lookup_tables: lookup_tables.to_vec(),
            additional_signers: Vec::new(),
            preamble: Vec::new(),
            postamble: Vec::new(),
            inner_program: venue_ix.program_id,
            inner_ix_data: venue_ix.data.clone(),
            inner_accounts,
            fee_vault: request.fee_vault,
            fee_vault_wsol_ata,
            user_wsol_ata,
            route_kind: request.route_kind,
            fee_bps: request.fee_bps,
            gross_sol_in_lamports: request.gross_sol_in_lamports,
            min_net_output: request.min_net_output,
            compute_unit_limit: source
                .compute_unit_limit
                .and_then(|v| u32::try_from(v).ok()),
            compute_unit_price_micro_lamports: source.compute_unit_price_micro_lamports,
            inline_tip_lamports: source.inline_tip_lamports,
            inline_tip_account: source.inline_tip_account.clone(),
        })
        .map_err(|error| WrapCompiledTransactionError::CompileFailed(error.to_string()))?;
        (instruction, preamble, postamble, "v3-native")
    };

    let mut new_instructions: Vec<Instruction> =
        Vec::with_capacity(preamble.len() + 2 + postamble.len());
    new_instructions.extend_from_slice(&preamble);
    new_instructions.push(wrapper_execute);
    new_instructions.extend_from_slice(&postamble);
    new_instructions.push(build_wrapper_uniqueness_memo_instruction());

    let message =
        v0::Message::try_compile(&payer.pubkey(), &new_instructions, lookup_tables, blockhash)
            .map_err(|error| WrapCompiledTransactionError::CompileFailed(error.to_string()))?;

    let lookup_tables_used: Vec<String> = message
        .address_table_lookups
        .iter()
        .map(|lookup| lookup.account_key.to_string())
        .collect();

    let restored_signers = compiled_transaction_signers::restore_compiled_transaction_signers(
        &source.serialized_base64,
    );
    let required_signer_count = usize::from(message.header.num_required_signatures);
    let required_signers = message
        .account_keys
        .iter()
        .take(required_signer_count)
        .copied()
        .collect::<Vec<_>>();
    if required_signers.as_slice() != [payer.pubkey()] {
        return Err(WrapCompiledTransactionError::CompileFailed(format!(
            "v3 wrapper transactions must require exactly one signer (user); got [{}]",
            required_signers
                .iter()
                .map(Pubkey::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let mut signers: Vec<&Keypair> = Vec::with_capacity(1 + restored_signers.len());
    signers.push(payer);
    signers.extend(
        restored_signers
            .iter()
            .filter(|signer| required_signers.contains(&signer.pubkey())),
    );
    let message_for_diagnostics = message.clone();
    let wrapped_tx = VersionedTransaction::try_new(VersionedMessage::V0(message), &signers)
        .map_err(|error| WrapCompiledTransactionError::SigningFailed(error.to_string()))?;

    let signature = wrapped_tx.signatures.first().map(|value| value.to_string());

    let serialized = bincode::serialize(&wrapped_tx)
        .map_err(|error| WrapCompiledTransactionError::SerializeFailed(error.to_string()))?;

    if serialized.len() > PACKET_LIMIT_BYTES {
        return Err(WrapCompiledTransactionError::CompileFailed(format!(
            "wrapped transaction exceeded packet limit: raw {} > {} bytes",
            serialized.len(),
            PACKET_LIMIT_BYTES
        )));
    }
    crate::alt_diagnostics::emit_alt_coverage_diagnostics(
        "execution-engine",
        &request.label,
        &new_instructions,
        lookup_tables,
        &message_for_diagnostics,
        Some(serialized.len()),
        &[],
    );

    eprintln!(
        "[execution-engine][wrapper-wrap] label={} mode={} route={:?} fee_bps={} venue_program={} user_wsol_ata_present={} bytes={}",
        request.label,
        wrapper_mode,
        request.route_kind,
        request.fee_bps,
        venue_ix.program_id,
        user_wsol_ata.is_some(),
        serialized.len()
    );

    Ok(CompiledTransaction {
        label: source.label.clone(),
        format: "v0-alt-wrapper".to_string(),
        serialized_base64: BASE64.encode(serialized),
        signature,
        lookup_tables_used,
        compute_unit_limit: source.compute_unit_limit,
        compute_unit_price_micro_lamports: source.compute_unit_price_micro_lamports,
        inline_tip_lamports: source.inline_tip_lamports,
        inline_tip_account: source.inline_tip_account.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrapper_abi::rent_sysvar_id;
    use borsh::BorshDeserialize;
    use solana_sdk::instruction::AccountMeta;

    fn stub_payer() -> Keypair {
        Keypair::new()
    }

    fn stub_request<'a>(
        payer: &'a Keypair,
        route: WrapperRouteKind,
        gross_in: u64,
    ) -> WrapperCompileRequest<'a> {
        WrapperCompileRequest {
            label: "test".to_string(),
            payer,
            blockhash: Hash::new_unique(),
            lookup_tables: vec![AddressLookupTableAccount {
                key: Pubkey::new_unique(),
                addresses: vec![Pubkey::new_unique(); 16],
            }],
            additional_signers: vec![],
            preamble: vec![],
            postamble: vec![],
            inner_program: Pubkey::new_unique(),
            inner_ix_data: vec![0xAA, 0xBB],
            inner_accounts: vec![
                AccountMeta::new_readonly(Pubkey::new_unique(), false),
                AccountMeta::new(Pubkey::new_unique(), false),
            ],
            fee_vault: Pubkey::new_unique(),
            fee_vault_wsol_ata: None,
            user_wsol_ata: None,
            route_kind: route,
            fee_bps: 10,
            gross_sol_in_lamports: gross_in,
            min_net_output: 1_000,
            compute_unit_limit: Some(400_000),
            compute_unit_price_micro_lamports: Some(2_000),
            inline_tip_lamports: None,
            inline_tip_account: None,
        }
    }

    #[test]
    fn estimate_sol_in_fee_is_floor_rounded() {
        assert_eq!(estimate_sol_in_fee_lamports(1_000_000_000, 10), 1_000_000);
        assert_eq!(estimate_sol_in_fee_lamports(999, 10), 0);
        assert_eq!(estimate_sol_in_fee_lamports(10_000_000, 0), 0);
    }

    #[test]
    fn build_execute_rejects_fee_bps_above_cap() {
        let payer = stub_payer();
        let mut request = stub_request(&payer, WrapperRouteKind::SolIn, 10_000_000);
        request.fee_bps = MAX_FEE_BPS + 1;
        let err = build_wrapper_execute_instruction(&request).unwrap_err();
        assert!(matches!(err, WrapperCompileError::FeeBpsOverCap { .. }));
    }

    #[test]
    fn build_execute_rejects_zero_sol_in_for_sol_in_route() {
        let payer = stub_payer();
        let request = stub_request(&payer, WrapperRouteKind::SolIn, 0);
        let err = build_wrapper_execute_instruction(&request).unwrap_err();
        assert!(matches!(err, WrapperCompileError::SolInRequiresGrossInput));
    }

    #[test]
    fn build_execute_rejects_nonzero_sol_in_for_sol_out_route() {
        let payer = stub_payer();
        let request = stub_request(&payer, WrapperRouteKind::SolOut, 500_000);
        let err = build_wrapper_execute_instruction(&request).unwrap_err();
        assert!(matches!(err, WrapperCompileError::SolOutRejectsGrossInput));
    }

    #[test]
    fn build_execute_rejects_zero_inner_program() {
        let payer = stub_payer();
        let mut request = stub_request(&payer, WrapperRouteKind::SolIn, 10_000);
        request.inner_program = Pubkey::default();
        let err = build_wrapper_execute_instruction(&request).unwrap_err();
        assert!(matches!(err, WrapperCompileError::InnerProgramZero));
    }

    #[test]
    fn build_execute_targets_wrapper_program_id() {
        let payer = stub_payer();
        let request = stub_request(&payer, WrapperRouteKind::SolIn, 10_000);
        let ix = build_wrapper_execute_instruction(&request).expect("build");
        assert_eq!(ix.program_id, PROGRAM_ID);
        assert_eq!(
            ix.accounts.len(),
            EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT as usize + 3
        );
        assert_eq!(ix.data[0], EXECUTE_SWAP_ROUTE_DISCRIMINATOR);
        assert_eq!(ix.accounts[7].pubkey, TOKEN_PROGRAM_ID);
        assert!(!ix.accounts[7].is_writable);
    }

    #[test]
    fn build_execute_rejects_half_wsol_slots_for_sol_out() {
        let payer = stub_payer();
        let mut request = stub_request(&payer, WrapperRouteKind::SolOut, 0);
        request.user_wsol_ata = Some(Pubkey::new_unique());
        request.fee_vault_wsol_ata = None;
        let err = build_wrapper_execute_instruction(&request).unwrap_err();
        assert!(matches!(err, WrapperCompileError::InvalidRouteKind));
    }

    #[test]
    fn build_execute_wsol_sell_is_not_a_direct_legacy_path() {
        let payer = stub_payer();
        let user_wsol = Pubkey::new_unique();
        let vault_wsol = Pubkey::new_unique();
        let mut request = stub_request(&payer, WrapperRouteKind::SolOut, 0);
        request.user_wsol_ata = Some(user_wsol);
        request.fee_vault_wsol_ata = Some(vault_wsol);
        let err = build_wrapper_execute_instruction(&request).unwrap_err();
        assert!(matches!(err, WrapperCompileError::InvalidRouteKind));
    }

    #[test]
    fn infers_sol_out_minimum_from_known_sell_instructions() {
        let mut pump_sell_data = PUMP_SELL_V2_DISCRIMINATOR.to_vec();
        pump_sell_data.extend_from_slice(&250_000u64.to_le_bytes());
        pump_sell_data.extend_from_slice(&1_000_000u64.to_le_bytes());
        let pump_sell = Instruction {
            program_id: Pubkey::from_str(PUMP_PROGRAM_ID).unwrap(),
            accounts: vec![],
            data: pump_sell_data,
        };

        assert_eq!(
            infer_sol_out_min_lamports_from_venue_instruction(&pump_sell),
            Some(1_000_000)
        );

        let mut pump_amm_sell_data = PUMP_AMM_SELL_DISCRIMINATOR.to_vec();
        pump_amm_sell_data.extend_from_slice(&300_000u64.to_le_bytes());
        pump_amm_sell_data.extend_from_slice(&2_000_000u64.to_le_bytes());
        let pump_amm_sell = Instruction {
            program_id: pump_amm_program_id(),
            accounts: vec![],
            data: pump_amm_sell_data,
        };
        assert_eq!(
            infer_sol_out_min_lamports_from_venue_instruction(&pump_amm_sell),
            Some(2_000_000)
        );

        let mut raydium_v4_sell_data = vec![RAYDIUM_AMM_V4_SWAP_BASE_IN_DISCRIMINATOR];
        raydium_v4_sell_data.extend_from_slice(&400_000u64.to_le_bytes());
        raydium_v4_sell_data.extend_from_slice(&3_000_000u64.to_le_bytes());
        let raydium_v4_sell = Instruction {
            program_id: raydium_amm_v4_program_id(),
            accounts: vec![],
            data: raydium_v4_sell_data,
        };
        assert_eq!(
            infer_sol_out_min_lamports_from_venue_instruction(&raydium_v4_sell),
            Some(3_000_000)
        );

        let mut orca_sell_data = ORCA_WHIRLPOOL_SWAP_DISCRIMINATOR.to_vec();
        orca_sell_data.extend_from_slice(&500_000u64.to_le_bytes());
        orca_sell_data.extend_from_slice(&4_000_000u64.to_le_bytes());
        orca_sell_data.extend_from_slice(&1u128.to_le_bytes());
        orca_sell_data.push(1);
        orca_sell_data.push(0);
        let orca_sell = Instruction {
            program_id: orca_whirlpool_program_id(),
            accounts: vec![],
            data: orca_sell_data,
        };
        assert_eq!(
            infer_sol_out_min_lamports_from_venue_instruction(&orca_sell),
            Some(4_000_000)
        );
    }

    #[test]
    fn already_wrapped_detection_includes_current_and_retired_discriminators() {
        for discriminator in [
            RETIRED_EXECUTE_DISCRIMINATOR,
            RETIRED_EXECUTE_AMM_WSOL_DISCRIMINATOR,
            EXECUTE_SWAP_ROUTE_DISCRIMINATOR,
            EXECUTE_PUMP_BONDING_V2_DISCRIMINATOR,
        ] {
            let instruction = Instruction {
                program_id: PROGRAM_ID,
                accounts: vec![],
                data: vec![discriminator],
            };
            assert!(is_wrapper_execute_instruction(&instruction));
        }
    }

    #[test]
    fn already_wrapped_account_validation_rejects_wrong_fee_vault() {
        let payer = Pubkey::new_unique();
        let fee_vault = Pubkey::new_unique();
        let inner_program = Pubkey::new_unique();
        let route_wsol = route_wsol_pda(&payer, 0).0;
        let route = ExecuteSwapRouteRequest {
            version: ABI_VERSION,
            route_mode: SwapRouteMode::Mixed,
            direction: SwapRouteDirection::Buy,
            settlement: SwapRouteSettlement::Token,
            fee_mode: SwapRouteFeeMode::SolPre,
            wsol_lane: 0,
            fee_bps: 10,
            gross_sol_in_lamports: 1_000_000,
            gross_token_in_amount: 0,
            min_net_output: 0,
            route_accounts_offset: EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT
                + EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT,
            intermediate_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
            token_fee_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
            legs: vec![],
        };
        let mut instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new_readonly(config_pda().0, false),
                AccountMeta::new(fee_vault, false),
                AccountMeta::new(ZEROED_WSOL_ATA_SENTINEL, false),
                AccountMeta::new(route_wsol, false),
                AccountMeta::new_readonly(instructions_sysvar_id(), false),
                AccountMeta::new_readonly(inner_program, false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
                AccountMeta::new_readonly(WSOL_MINT, false),
                AccountMeta::new_readonly(system_program_id(), false),
                AccountMeta::new_readonly(rent_sysvar_id(), false),
                AccountMeta::new_readonly(inner_program, false),
            ],
            data: vec![EXECUTE_SWAP_ROUTE_DISCRIMINATOR],
        };
        let request = WrapCompiledTransactionRequest {
            label: "follow-buy-atomic".to_string(),
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            fee_vault,
            gross_sol_in_lamports: 1_000_000,
            min_net_output: 0,
            select_first_allowlisted_venue_instruction: true,
            select_last_allowlisted_venue_instruction: false,
        };

        validate_already_wrapped_route_accounts(&instruction, &payer, &request, &route)
            .expect("valid account metas");
        instruction.accounts[2].pubkey = Pubkey::new_unique();
        let error = validate_already_wrapped_route_accounts(&instruction, &payer, &request, &route)
            .expect_err("wrong fee vault must fail");
        assert!(error.to_string().contains("fixed accounts"));
    }

    fn already_wrapped_pump_v2_instruction(payer: Pubkey, fee_vault: Pubkey) -> Instruction {
        let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();
        let fee_vault_quote_ata =
            get_associated_token_address_with_program_id(&fee_vault, &WSOL_MINT, &TOKEN_PROGRAM_ID);
        let user_wsol_ata =
            get_associated_token_address_with_program_id(&payer, &WSOL_MINT, &TOKEN_PROGRAM_ID);
        let mut pump_accounts: Vec<AccountMeta> = (0..PUMP_V2_BUY_ACCOUNT_COUNT)
            .map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false))
            .collect();
        pump_accounts[PUMP_V2_QUOTE_MINT_INDEX] = AccountMeta::new_readonly(WSOL_MINT, false);
        pump_accounts[PUMP_V2_USER_INDEX] = AccountMeta::new(payer, true);
        pump_accounts[PUMP_V2_ASSOCIATED_QUOTE_USER_INDEX] = AccountMeta::new(user_wsol_ata, false);
        pump_accounts[PUMP_V2_BUY_PROGRAM_INDEX] = AccountMeta::new_readonly(pump_program, false);
        let mut accounts = vec![
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(config_pda().0, false),
            AccountMeta::new(fee_vault, false),
            AccountMeta::new(fee_vault_quote_ata, false),
            AccountMeta::new_readonly(instructions_sysvar_id(), false),
            AccountMeta::new_readonly(pump_program, false),
        ];
        accounts.extend(pump_accounts);
        Instruction {
            program_id: PROGRAM_ID,
            accounts,
            data: vec![EXECUTE_PUMP_BONDING_V2_DISCRIMINATOR],
        }
    }

    #[test]
    fn already_wrapped_pump_v2_rejects_wrong_fee_vault_quote_ata() {
        let payer = Pubkey::new_unique();
        let fee_vault = Pubkey::new_unique();
        let route = ExecutePumpBondingV2Request {
            version: ABI_VERSION,
            side: PumpBondingV2Side::Buy,
            quote_fee_mode: PumpBondingV2QuoteFeeMode::Wsol,
            fee_bps: 10,
            gross_quote_in_amount: 1_000_000,
            min_base_out_amount: 1,
            base_amount_in: 0,
            gross_min_quote_out_amount: 0,
            net_min_quote_out_amount: 0,
        };
        let mut instruction = already_wrapped_pump_v2_instruction(payer, fee_vault);
        let request = WrapCompiledTransactionRequest {
            label: "pump-v2-already-wrapped".to_string(),
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            fee_vault,
            gross_sol_in_lamports: 1_000_000,
            min_net_output: 0,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };

        validate_already_wrapped_pump_bonding_v2_accounts(&instruction, &payer, &request, &route)
            .expect("valid Pump v2 fixed accounts");
        instruction.accounts[3].pubkey = Pubkey::new_unique();
        let error = validate_already_wrapped_pump_bonding_v2_accounts(
            &instruction,
            &payer,
            &request,
            &route,
        )
        .expect_err("wrong fee-vault quote ATA must fail");
        assert!(error.to_string().contains("fixed accounts"));
    }

    #[test]
    fn already_wrapped_pump_v2_rejects_wrong_inner_quote_ata() {
        let payer = Pubkey::new_unique();
        let fee_vault = Pubkey::new_unique();
        let route = ExecutePumpBondingV2Request {
            version: ABI_VERSION,
            side: PumpBondingV2Side::Buy,
            quote_fee_mode: PumpBondingV2QuoteFeeMode::Wsol,
            fee_bps: 10,
            gross_quote_in_amount: 1_000_000,
            min_base_out_amount: 1,
            base_amount_in: 0,
            gross_min_quote_out_amount: 0,
            net_min_quote_out_amount: 0,
        };
        let mut instruction = already_wrapped_pump_v2_instruction(payer, fee_vault);
        let request = WrapCompiledTransactionRequest {
            label: "pump-v2-already-wrapped".to_string(),
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            fee_vault,
            gross_sol_in_lamports: 1_000_000,
            min_net_output: 0,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };
        let inner_quote_index = usize::from(EXECUTE_PUMP_BONDING_V2_FIXED_ACCOUNT_COUNT)
            + PUMP_V2_ASSOCIATED_QUOTE_USER_INDEX;
        instruction.accounts[inner_quote_index].pubkey = Pubkey::new_unique();

        let error = validate_already_wrapped_pump_bonding_v2_accounts(
            &instruction,
            &payer,
            &request,
            &route,
        )
        .expect_err("wrong inner quote ATA must fail");
        assert!(error.to_string().contains("quote user account"));
    }

    #[test]
    fn already_wrapped_pump_v2_rejects_stale_buy_amounts() {
        let payer = Pubkey::new_unique();
        let fee_vault = Pubkey::new_unique();
        let route = ExecutePumpBondingV2Request {
            version: ABI_VERSION,
            side: PumpBondingV2Side::Buy,
            quote_fee_mode: PumpBondingV2QuoteFeeMode::Wsol,
            fee_bps: 10,
            gross_quote_in_amount: 900_000,
            min_base_out_amount: 1,
            base_amount_in: 0,
            gross_min_quote_out_amount: 0,
            net_min_quote_out_amount: 0,
        };
        let request = WrapCompiledTransactionRequest {
            label: "pump-v2-already-wrapped".to_string(),
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            fee_vault,
            gross_sol_in_lamports: 1_000_000,
            min_net_output: 1,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };
        let instruction = already_wrapped_pump_v2_instruction(payer, fee_vault);

        let error = validate_already_wrapped_pump_bonding_v2_accounts(
            &instruction,
            &payer,
            &request,
            &route,
        )
        .expect_err("stale buy input must fail");
        assert!(error.to_string().contains("gross quote input"));
    }

    #[test]
    fn already_wrapped_pump_v2_rejects_stale_sell_min_output() {
        let payer = Pubkey::new_unique();
        let fee_vault = Pubkey::new_unique();
        let route = ExecutePumpBondingV2Request {
            version: ABI_VERSION,
            side: PumpBondingV2Side::Sell,
            quote_fee_mode: PumpBondingV2QuoteFeeMode::Wsol,
            fee_bps: 10,
            gross_quote_in_amount: 0,
            min_base_out_amount: 0,
            base_amount_in: 500_000,
            gross_min_quote_out_amount: 1_000_000,
            net_min_quote_out_amount: 900_000,
        };
        let request = WrapCompiledTransactionRequest {
            label: "pump-v2-already-wrapped".to_string(),
            route_kind: WrapperRouteKind::SolOut,
            fee_bps: 10,
            fee_vault,
            gross_sol_in_lamports: 0,
            min_net_output: 950_000,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };
        let instruction = already_wrapped_pump_v2_instruction(payer, fee_vault);

        let error = validate_already_wrapped_pump_bonding_v2_accounts(
            &instruction,
            &payer,
            &request,
            &route,
        )
        .expect_err("stale sell min output must fail");
        assert!(error.to_string().contains("net quote output"));
    }

    #[test]
    fn derives_temp_wsol_output_account_from_postamble_close() {
        let payer = stub_payer();
        let fee_vault = Pubkey::new_unique();
        let temp_wsol = Pubkey::new_unique();
        let venue_accounts = vec![
            AccountMeta::new(Pubkey::new_unique(), false),
            AccountMeta::new(temp_wsol, false),
        ];
        let postamble = vec![Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(temp_wsol, false),
                AccountMeta::new(payer.pubkey(), false),
                AccountMeta::new_readonly(payer.pubkey(), true),
            ],
            data: vec![SPL_TOKEN_CLOSE_ACCOUNT_DISCRIMINATOR],
        }];
        let preamble = vec![
            spl_token::instruction::initialize_account3(
                &TOKEN_PROGRAM_ID,
                &temp_wsol,
                &WSOL_MINT,
                &payer.pubkey(),
            )
            .expect("initialize temp wsol"),
        ];

        let (user_wsol, fee_vault_wsol) = derive_wsol_route_accounts(
            &payer.pubkey(),
            &fee_vault,
            WrapperRouteKind::SolOut,
            &Pubkey::new_unique(),
            &[],
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
                &WSOL_MINT,
                &TOKEN_PROGRAM_ID,
            ))
        );
    }

    #[test]
    fn rejects_unproven_temp_wsol_output_close() {
        let payer = stub_payer();
        let fee_vault = Pubkey::new_unique();
        let temp_account = Pubkey::new_unique();
        let venue_accounts = vec![AccountMeta::new(temp_account, false)];
        let postamble = vec![Instruction {
            program_id: TOKEN_PROGRAM_ID,
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
            &[],
            &venue_accounts,
            &[],
            &postamble,
        )
        .expect_err("unproven close must fail");

        assert!(error.to_string().contains("could not be proven to be WSOL"));
    }

    #[test]
    fn compile_wrapper_tx_requires_lookup_tables() {
        let payer = stub_payer();
        let mut request = stub_request(&payer, WrapperRouteKind::SolIn, 10_000);
        request.lookup_tables.clear();
        let err = compile_wrapper_transaction(request).unwrap_err();
        assert_eq!(err, WrapperCompileError::MissingLookupTables);
    }

    #[test]
    fn signed_wrapper_tx_bytes_are_stable_across_calls() {
        let payer = stub_payer();
        let blockhash = Hash::new_unique();
        let inner_program = Pubkey::new_unique();
        let fee_vault = Pubkey::new_unique();
        let inner_account_a = Pubkey::new_from_array([9u8; 32]);
        let inner_account_b = Pubkey::new_from_array([8u8; 32]);
        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: vec![
                inner_program,
                fee_vault,
                inner_account_a,
                inner_account_b,
                config_pda().0,
                instructions_sysvar_id(),
                ZEROED_WSOL_ATA_SENTINEL,
                PROGRAM_ID,
                TOKEN_PROGRAM_ID,
                system_program_id(),
            ],
        };
        fn make<'a>(
            payer: &'a Keypair,
            blockhash: Hash,
            alt: AddressLookupTableAccount,
            inner_program: Pubkey,
            fee_vault: Pubkey,
            inner_a: Pubkey,
            inner_b: Pubkey,
        ) -> WrapperCompileRequest<'a> {
            WrapperCompileRequest {
                label: "stable".to_string(),
                payer,
                blockhash,
                lookup_tables: vec![alt],
                additional_signers: vec![],
                preamble: vec![],
                postamble: vec![],
                inner_program,
                inner_ix_data: vec![1, 2, 3, 4],
                inner_accounts: vec![
                    AccountMeta::new_readonly(inner_a, false),
                    AccountMeta::new(inner_b, false),
                ],
                fee_vault,
                fee_vault_wsol_ata: None,
                user_wsol_ata: None,
                route_kind: WrapperRouteKind::SolIn,
                fee_bps: 10,
                gross_sol_in_lamports: 100_000_000,
                min_net_output: 42,
                compute_unit_limit: Some(250_000),
                compute_unit_price_micro_lamports: Some(500),
                inline_tip_lamports: None,
                inline_tip_account: None,
            }
        }

        let first = compile_wrapper_transaction(make(
            &payer,
            blockhash,
            alt.clone(),
            inner_program,
            fee_vault,
            inner_account_a,
            inner_account_b,
        ))
        .expect("first compile");
        let second = compile_wrapper_transaction(make(
            &payer,
            blockhash,
            alt,
            inner_program,
            fee_vault,
            inner_account_a,
            inner_account_b,
        ))
        .expect("second compile");
        assert_eq!(first.serialized_base64, second.serialized_base64);
        assert_eq!(first.signature, second.signature);
    }

    /// Build a stand-alone CU-limit instruction without pulling in
    /// `solana-sdk::compute_budget` (the workspace SDK version doesn't
    /// expose it). Only the program_id matters for these tests — we
    /// just need an instruction that isn't in the allowlist.
    fn fake_cu_ix() -> Instruction {
        let cu_program: Pubkey = "ComputeBudget111111111111111111111111111111"
            .parse()
            .expect("compute budget pubkey");
        Instruction {
            program_id: cu_program,
            accounts: vec![],
            data: vec![0x02, 0x00, 0x00, 0x00, 0x00],
        }
    }

    fn fake_memo_ix() -> Instruction {
        Instruction {
            program_id: memo_program_id(),
            accounts: vec![],
            data: b"memo".to_vec(),
        }
    }

    /// End-to-end test for the post-compile wrap path: build a
    /// native-looking signed v0 tx that contains a "venue" ix (stubbed
    /// by an allowlisted program), then wrap it and confirm the
    /// resulting tx contains a single wrapper `Execute` instruction in
    /// place of the original venue ix.
    #[test]
    fn wrap_compiled_transaction_swaps_the_venue_instruction() {
        let payer = stub_payer();
        let venue_program = Pubkey::new_unique();
        let venue_account_a = Pubkey::new_from_array([7u8; 32]);
        let venue_account_b = Pubkey::new_from_array([5u8; 32]);
        let fee_vault = Pubkey::new_unique();
        let fee_vault_wsol_ata =
            get_associated_token_address_with_program_id(&fee_vault, &WSOL_MINT, &TOKEN_PROGRAM_ID);
        let user_wsol_ata = get_associated_token_address_with_program_id(
            &payer.pubkey(),
            &WSOL_MINT,
            &TOKEN_PROGRAM_ID,
        );
        let blockhash = Hash::new_unique();

        // The ALT must contain every pubkey the compiled v0 message
        // would try to deref through it. For a native tx we stage the
        // venue program + its two accounts. After wrapping the message
        // also references the wrapper program, the config PDA, the
        // instructions sysvar, the fee vault, the token program, the
        // system program, and the two WSOL ATAs — pre-stage them all
        // so the compiler can pack the wrapper prefix through the ALT.
        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: vec![
                venue_program,
                venue_account_a,
                venue_account_b,
                PROGRAM_ID,
                config_pda().0,
                instructions_sysvar_id(),
                fee_vault,
                fee_vault_wsol_ata,
                user_wsol_ata,
                TOKEN_PROGRAM_ID,
                system_program_id(),
            ],
        };

        // Build a native-looking tx: CU limit, CU price, venue ix, memo.
        let venue_ix = Instruction {
            program_id: venue_program,
            accounts: vec![
                AccountMeta::new(venue_account_a, false),
                AccountMeta::new(user_wsol_ata, false),
                AccountMeta::new_readonly(venue_account_b, false),
            ],
            data: vec![0xDE, 0xAD, 0xBE, 0xEF],
        };

        let instructions = vec![
            fake_cu_ix(),
            fake_memo_ix(),
            fake_cu_ix(),
            venue_ix.clone(),
            fake_memo_ix(),
        ];

        let message = v0::Message::try_compile(
            &payer.pubkey(),
            &instructions,
            std::slice::from_ref(&alt),
            blockhash,
        )
        .expect("compile native");
        let signed = VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer])
            .expect("sign native");
        let native_bytes = bincode::serialize(&signed).expect("serialize native");
        let native_compiled = CompiledTransaction {
            label: "native-test".to_string(),
            format: "v0-alt".to_string(),
            serialized_base64: BASE64.encode(native_bytes),
            signature: signed.signatures.first().map(|s| s.to_string()),
            lookup_tables_used: vec![alt.key.to_string()],
            compute_unit_limit: Some(250_000),
            compute_unit_price_micro_lamports: Some(2_000),
            inline_tip_lamports: None,
            inline_tip_account: None,
        };

        let wrapped = wrap_compiled_transaction(
            &native_compiled,
            &payer,
            std::slice::from_ref(&alt),
            &[venue_program],
            &WrapCompiledTransactionRequest {
                label: "wrapped-test".to_string(),
                route_kind: WrapperRouteKind::SolOut,
                fee_bps: 10,
                fee_vault,
                gross_sol_in_lamports: 0,
                min_net_output: 1,
                select_first_allowlisted_venue_instruction: false,
                select_last_allowlisted_venue_instruction: false,
            },
        )
        .expect("wrap succeeds");

        // The wrapped tx must re-decode cleanly, must contain one
        // wrapper `Execute` instruction (program_id == PROGRAM_ID) at
        // the slot the venue ix used to occupy, and must NOT contain
        // the venue program as a direct target anymore.
        let wrapped_bytes = BASE64.decode(wrapped.serialized_base64.as_bytes()).unwrap();
        let wrapped_tx: VersionedTransaction = bincode::deserialize(&wrapped_bytes).unwrap();
        let (decoded, _keys) =
            decompile_v0_transaction(&wrapped_tx, std::slice::from_ref(&alt)).unwrap();
        let wrapper_ix_count = decoded
            .iter()
            .filter(|ix| ix.program_id == PROGRAM_ID)
            .count();
        assert_eq!(wrapper_ix_count, 1, "expected exactly one wrapper Execute");
        let venue_ix_count = decoded
            .iter()
            .filter(|ix| ix.program_id == venue_program)
            .count();
        assert_eq!(
            venue_ix_count, 0,
            "venue program should no longer be a top-level target"
        );
        let memo_ix_count = decoded
            .iter()
            .filter(|ix| ix.program_id == memo_program_id())
            .count();
        assert_eq!(
            memo_ix_count, 1,
            "wrapper should strip venue memos and add one final uniqueness memo"
        );
        assert_eq!(wrapped.format, "v0-alt-wrapper");
    }

    #[test]
    fn wrap_compiled_transaction_adds_final_signature_uniqueness() {
        let payer = stub_payer();
        let venue_program = Pubkey::new_unique();
        let venue_account = Pubkey::new_unique();
        let fee_vault = Pubkey::new_unique();
        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: vec![
                PROGRAM_ID,
                config_pda().0,
                instructions_sysvar_id(),
                fee_vault,
                ZEROED_WSOL_ATA_SENTINEL,
                TOKEN_PROGRAM_ID,
                system_program_id(),
                venue_program,
                venue_account,
                memo_program_id(),
            ],
        };
        let blockhash = Hash::new_unique();
        let native_ix = Instruction {
            program_id: venue_program,
            accounts: vec![AccountMeta::new(venue_account, false)],
            data: vec![1, 2, 3],
        };
        let message = v0::Message::try_compile(
            &payer.pubkey(),
            &[native_ix],
            std::slice::from_ref(&alt),
            blockhash,
        )
        .expect("compile native");
        let signed = VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer])
            .expect("sign native");
        let native_compiled = CompiledTransaction {
            label: "native-test".to_string(),
            format: "v0-alt".to_string(),
            serialized_base64: BASE64.encode(bincode::serialize(&signed).expect("serialize")),
            signature: signed.signatures.first().map(|s| s.to_string()),
            lookup_tables_used: vec![alt.key.to_string()],
            compute_unit_limit: Some(250_000),
            compute_unit_price_micro_lamports: Some(2_000),
            inline_tip_lamports: None,
            inline_tip_account: None,
        };
        let request = WrapCompiledTransactionRequest {
            label: "wrapped-test".to_string(),
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            fee_vault,
            gross_sol_in_lamports: 100_000,
            min_net_output: 1,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };

        let first = wrap_compiled_transaction(
            &native_compiled,
            &payer,
            std::slice::from_ref(&alt),
            &[venue_program],
            &request,
        )
        .expect("first wrap");
        let second = wrap_compiled_transaction(
            &native_compiled,
            &payer,
            std::slice::from_ref(&alt),
            &[venue_program],
            &request,
        )
        .expect("second wrap");

        assert_ne!(first.signature, second.signature);
        assert_ne!(first.serialized_base64, second.serialized_base64);
    }

    #[test]
    fn raydium_v4_wsol_derivation_uses_full_swap_account_layout() {
        let user_source = Pubkey::new_unique();
        let user_destination = Pubkey::new_unique();
        let user_owner = Pubkey::new_unique();
        let mut accounts = (0..18)
            .map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false))
            .collect::<Vec<_>>();
        accounts[15] = AccountMeta::new(user_source, false);
        accounts[16] = AccountMeta::new(user_destination, false);
        accounts[17] = AccountMeta::new_readonly(user_owner, true);

        assert_eq!(
            derive_raydium_amm_v4_wsol_input_account_index(&accounts),
            Some(15)
        );
        assert_eq!(
            derive_raydium_amm_v4_wsol_output_account(&accounts),
            Some(user_destination)
        );

        accounts[17].is_signer = false;
        assert_eq!(
            derive_raydium_amm_v4_wsol_input_account_index(&accounts),
            None
        );
        assert_eq!(derive_raydium_amm_v4_wsol_output_account(&accounts), None);
    }

    #[test]
    fn raydium_v4_v3_wsol_plan_preserves_exact_inner_account_count() {
        let payer = Pubkey::new_unique();
        let temp_wsol = Pubkey::new_unique();
        let (route_wsol, _) = route_wsol_pda(&payer, 0);
        let rent_lamports = Rent::default().minimum_balance(SPL_TOKEN_ACCOUNT_LEN);
        let swap_lamports = 100_000_000;
        let mut venue_accounts = (0..18)
            .map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false))
            .collect::<Vec<_>>();
        venue_accounts[15] = AccountMeta::new(temp_wsol, false);
        venue_accounts[17] = AccountMeta::new_readonly(payer, true);
        let venue_ix = Instruction {
            program_id: raydium_amm_v4_program_id(),
            accounts: venue_accounts,
            data: vec![9],
        };
        let preamble = vec![solana_system_interface::instruction::create_account(
            &payer,
            &temp_wsol,
            rent_lamports + swap_lamports,
            SPL_TOKEN_ACCOUNT_LEN as u64,
            &TOKEN_PROGRAM_ID,
        )];
        let request = WrapCompiledTransactionRequest {
            label: "raydium-v4-v2-plan".to_string(),
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            fee_vault: Pubkey::new_unique(),
            gross_sol_in_lamports: swap_lamports,
            min_net_output: 1,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };

        let plan = try_build_amm_wsol_v2_plan(&payer, &request, &venue_ix, &preamble, &[]).unwrap();

        assert_eq!(plan.inner_accounts.len(), 18);
        assert_eq!(plan.inner_accounts[15].pubkey, route_wsol);
        assert!(!plan.inner_accounts[15].is_signer);
        assert!(plan.inner_accounts[15].is_writable);
        assert!(
            !plan
                .inner_accounts
                .iter()
                .any(|meta| meta.pubkey == system_program_id())
        );
    }

    #[test]
    fn generic_temp_wsol_input_uses_route_pda() {
        let payer = Pubkey::new_unique();
        let temp_wsol = Pubkey::new_unique();
        let (route_wsol, _) = route_wsol_pda(&payer, 0);
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
                &TOKEN_PROGRAM_ID,
            ),
            spl_token::instruction::initialize_account3(
                &TOKEN_PROGRAM_ID,
                &temp_wsol,
                &WSOL_MINT,
                &payer,
            )
            .expect("initialize temp wsol"),
            spl_token::instruction::sync_native(&TOKEN_PROGRAM_ID, &temp_wsol)
                .expect("sync temp wsol"),
        ];
        let postamble = vec![
            spl_token::instruction::close_account(
                &TOKEN_PROGRAM_ID,
                &temp_wsol,
                &payer,
                &payer,
                &[],
            )
            .expect("close temp wsol"),
        ];
        let request = WrapCompiledTransactionRequest {
            label: "pump-amm-v3-plan".to_string(),
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            fee_vault: Pubkey::new_unique(),
            gross_sol_in_lamports: swap_lamports,
            min_net_output: 1,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };

        let plan =
            try_build_amm_wsol_v2_plan(&payer, &request, &venue_ix, &preamble, &postamble).unwrap();

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
            get_associated_token_address_with_program_id(&payer, &WSOL_MINT, &TOKEN_PROGRAM_ID);
        let (route_wsol, _) = route_wsol_pda(&payer, 0);
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
                &WSOL_MINT,
                &TOKEN_PROGRAM_ID,
            ),
            solana_system_interface::instruction::transfer(&payer, &input_wsol_ata, net_lamports),
            spl_token::instruction::sync_native(&TOKEN_PROGRAM_ID, &input_wsol_ata)
                .expect("sync wsol ata"),
        ];
        let postamble = vec![
            spl_token::instruction::close_account(
                &TOKEN_PROGRAM_ID,
                &input_wsol_ata,
                &payer,
                &payer,
                &[],
            )
            .expect("close wsol ata"),
        ];
        let request = WrapCompiledTransactionRequest {
            label: "bags-v3-plan".to_string(),
            route_kind: WrapperRouteKind::SolIn,
            fee_bps,
            fee_vault: Pubkey::new_unique(),
            gross_sol_in_lamports: gross_lamports,
            min_net_output: 1,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };

        let plan =
            try_build_amm_wsol_v2_plan(&payer, &request, &venue_ix, &preamble, &postamble).unwrap();

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

        let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();
        let mut pump_bonding_data = PUMP_BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR.to_vec();
        pump_bonding_data.extend_from_slice(&123u64.to_le_bytes());
        pump_bonding_data.extend_from_slice(&456u64.to_le_bytes());
        let patched_pump_bonding =
            patch_amm_wsol_input_amount(pump_program, &pump_bonding_data, net_lamports);
        assert_eq!(&patched_pump_bonding[8..16], &net_lamports.to_le_bytes());
    }

    #[test]
    fn pump_bonding_v2_buy_preserves_user_quote_ata() {
        let payer = Pubkey::new_unique();
        let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();
        let user_wsol_ata =
            get_associated_token_address_with_program_id(&payer, &WSOL_MINT, &TOKEN_PROGRAM_ID);
        let mut venue_accounts: Vec<AccountMeta> = (0..PUMP_V2_BUY_ACCOUNT_COUNT)
            .map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false))
            .collect();
        venue_accounts[PUMP_V2_QUOTE_MINT_INDEX] = AccountMeta::new_readonly(WSOL_MINT, false);
        venue_accounts[PUMP_V2_USER_INDEX] = AccountMeta::new(payer, true);
        venue_accounts[PUMP_V2_ASSOCIATED_QUOTE_USER_INDEX] =
            AccountMeta::new(user_wsol_ata, false);
        venue_accounts[PUMP_V2_BUY_PROGRAM_INDEX] = AccountMeta::new_readonly(pump_program, false);
        let mut data = PUMP_BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&99_900_000u64.to_le_bytes());
        data.extend_from_slice(&1u64.to_le_bytes());
        let venue_ix = Instruction {
            program_id: pump_program,
            accounts: venue_accounts,
            data,
        };
        let request = WrapCompiledTransactionRequest {
            label: "pump-bonding-v2-buy".to_string(),
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            fee_vault: Pubkey::new_unique(),
            gross_sol_in_lamports: 100_000_000,
            min_net_output: 1,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };
        let pump_wrapper =
            try_build_pump_bonding_v2_wrapper_instruction(&payer, &request, &venue_ix)
                .expect("pump wrapper candidate")
                .expect("pump wrapper");
        assert_eq!(pump_wrapper.data[0], EXECUTE_PUMP_BONDING_V2_DISCRIMINATOR);
        assert!(try_build_amm_wsol_v2_plan(&payer, &request, &venue_ix, &[], &[]).is_none());
        let (user_wsol_account, fee_vault_wsol) = derive_wsol_route_accounts(
            &payer,
            &request.fee_vault,
            WrapperRouteKind::SolIn,
            &venue_ix.program_id,
            &venue_ix.data,
            &venue_ix.accounts,
            &[],
            &[],
        )
        .expect("derive wsol route accounts");
        assert_eq!(user_wsol_account, None);
        assert_eq!(fee_vault_wsol, None);
    }

    #[test]
    fn pump_bonding_v2_buy_rejects_stale_quote_input() {
        let payer = Pubkey::new_unique();
        let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();
        let user_wsol_ata =
            get_associated_token_address_with_program_id(&payer, &WSOL_MINT, &TOKEN_PROGRAM_ID);
        let mut venue_accounts: Vec<AccountMeta> = (0..PUMP_V2_BUY_ACCOUNT_COUNT)
            .map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false))
            .collect();
        venue_accounts[PUMP_V2_QUOTE_MINT_INDEX] = AccountMeta::new_readonly(WSOL_MINT, false);
        venue_accounts[PUMP_V2_USER_INDEX] = AccountMeta::new(payer, true);
        venue_accounts[PUMP_V2_ASSOCIATED_QUOTE_USER_INDEX] =
            AccountMeta::new(user_wsol_ata, false);
        venue_accounts[PUMP_V2_BUY_PROGRAM_INDEX] = AccountMeta::new_readonly(pump_program, false);
        let mut data = PUMP_BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&99_900_000u64.to_le_bytes());
        data.extend_from_slice(&1u64.to_le_bytes());
        let venue_ix = Instruction {
            program_id: pump_program,
            accounts: venue_accounts,
            data,
        };
        let request = WrapCompiledTransactionRequest {
            label: "pump-bonding-v2-buy".to_string(),
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            fee_vault: Pubkey::new_unique(),
            gross_sol_in_lamports: 100_000_000,
            min_net_output: 1,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };

        let error = try_build_pump_bonding_v2_wrapper_instruction(&payer, &request, &venue_ix)
            .expect_err("stale Pump v2 buy amount must fail closed");
        assert!(error.to_string().contains("net quote input"));
    }

    #[test]
    fn pump_bonding_v2_buy_plan_rejects_usdc_quote_curve() {
        let payer = Pubkey::new_unique();
        let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();
        let user_quote_ata = Pubkey::new_unique();
        let mut venue_accounts: Vec<AccountMeta> = (0..27)
            .map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false))
            .collect();
        venue_accounts[2] = AccountMeta::new_readonly(Pubkey::new_unique(), false);
        venue_accounts[13] = AccountMeta::new(payer, true);
        venue_accounts[15] = AccountMeta::new(user_quote_ata, false);
        venue_accounts[26] = AccountMeta::new_readonly(pump_program, false);
        let mut data = PUMP_BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&100_000_000u64.to_le_bytes());
        data.extend_from_slice(&1u64.to_le_bytes());
        let venue_ix = Instruction {
            program_id: pump_program,
            accounts: venue_accounts,
            data,
        };
        let request = WrapCompiledTransactionRequest {
            label: "pump-bonding-v2-buy-usdc".to_string(),
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            fee_vault: Pubkey::new_unique(),
            gross_sol_in_lamports: 100_000_000,
            min_net_output: 1,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };
        assert!(
            try_build_pump_bonding_v2_wrapper_instruction(&payer, &request, &venue_ix)
                .expect("pump wrapper candidate")
                .is_none()
        );
        assert!(try_build_amm_wsol_v2_plan(&payer, &request, &venue_ix, &[], &[]).is_none());
    }

    #[test]
    fn pump_bonding_v2_wsol_shape_mismatch_fails_closed() {
        let payer = Pubkey::new_unique();
        let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();
        let wrong_quote_ata = Pubkey::new_unique();
        let mut venue_accounts: Vec<AccountMeta> = (0..27)
            .map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false))
            .collect();
        venue_accounts[2] = AccountMeta::new_readonly(WSOL_MINT, false);
        venue_accounts[13] = AccountMeta::new(payer, true);
        venue_accounts[15] = AccountMeta::new(wrong_quote_ata, false);
        venue_accounts[26] = AccountMeta::new_readonly(pump_program, false);
        let mut data = PUMP_BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&100_000_000u64.to_le_bytes());
        data.extend_from_slice(&1u64.to_le_bytes());
        let venue_ix = Instruction {
            program_id: pump_program,
            accounts: venue_accounts,
            data,
        };
        let request = WrapCompiledTransactionRequest {
            label: "pump-bonding-v2-buy-wrong-wsol".to_string(),
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            fee_vault: Pubkey::new_unique(),
            gross_sol_in_lamports: 100_000_000,
            min_net_output: 1,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };

        let error = try_build_pump_bonding_v2_wrapper_instruction(&payer, &request, &venue_ix)
            .expect_err("recognized Pump v2 WSOL route must not fall back");
        assert!(error.to_string().contains("quote user account"));
    }

    #[test]
    fn pump_bonding_v2_wsol_side_mismatch_fails_closed() {
        let payer = Pubkey::new_unique();
        let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();
        let user_wsol_ata =
            get_associated_token_address_with_program_id(&payer, &WSOL_MINT, &TOKEN_PROGRAM_ID);
        let mut venue_accounts: Vec<AccountMeta> = (0..PUMP_V2_BUY_ACCOUNT_COUNT)
            .map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false))
            .collect();
        venue_accounts[PUMP_V2_QUOTE_MINT_INDEX] = AccountMeta::new_readonly(WSOL_MINT, false);
        venue_accounts[PUMP_V2_USER_INDEX] = AccountMeta::new(payer, true);
        venue_accounts[PUMP_V2_ASSOCIATED_QUOTE_USER_INDEX] =
            AccountMeta::new(user_wsol_ata, false);
        venue_accounts[PUMP_V2_BUY_PROGRAM_INDEX] = AccountMeta::new_readonly(pump_program, false);
        let mut data = PUMP_BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&99_900_000u64.to_le_bytes());
        data.extend_from_slice(&1u64.to_le_bytes());
        let venue_ix = Instruction {
            program_id: pump_program,
            accounts: venue_accounts,
            data,
        };
        let request = WrapCompiledTransactionRequest {
            label: "pump-bonding-v2-buy-as-sell".to_string(),
            route_kind: WrapperRouteKind::SolOut,
            fee_bps: 10,
            fee_vault: Pubkey::new_unique(),
            gross_sol_in_lamports: 0,
            min_net_output: 1,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };

        let error = try_build_pump_bonding_v2_wrapper_instruction(&payer, &request, &venue_ix)
            .expect_err("recognized Pump v2 WSOL side mismatch must not fall back");
        assert!(error.to_string().contains("route kind"));
    }

    #[test]
    fn pump_bonding_v2_sell_preserves_user_quote_ata() {
        let payer_kp = stub_payer();
        let payer = payer_kp.pubkey();
        let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();
        let user_wsol_ata =
            get_associated_token_address_with_program_id(&payer, &WSOL_MINT, &TOKEN_PROGRAM_ID);
        let mut venue_accounts: Vec<AccountMeta> = (0..PUMP_V2_SELL_ACCOUNT_COUNT)
            .map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false))
            .collect();
        venue_accounts[PUMP_V2_QUOTE_MINT_INDEX] = AccountMeta::new_readonly(WSOL_MINT, false);
        venue_accounts[PUMP_V2_USER_INDEX] = AccountMeta::new(payer, true);
        venue_accounts[PUMP_V2_ASSOCIATED_QUOTE_USER_INDEX] =
            AccountMeta::new(user_wsol_ata, false);
        venue_accounts[PUMP_V2_SELL_PROGRAM_INDEX] = AccountMeta::new_readonly(pump_program, false);
        let mut data = PUMP_SELL_V2_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&500_000u64.to_le_bytes());
        data.extend_from_slice(&1_000_000u64.to_le_bytes());
        let venue_ix = Instruction {
            program_id: pump_program,
            accounts: venue_accounts,
            data,
        };
        let (user_wsol_account, _fee_vault_wsol) = derive_wsol_route_accounts(
            &payer,
            &Pubkey::new_unique(),
            WrapperRouteKind::SolOut,
            &venue_ix.program_id,
            &venue_ix.data,
            &venue_ix.accounts,
            &[],
            &[],
        )
        .expect("derive wsol route accounts");
        assert_eq!(user_wsol_account, None);
        let request = WrapCompiledTransactionRequest {
            label: "pump-bonding-v2-sell".to_string(),
            route_kind: WrapperRouteKind::SolOut,
            fee_bps: 10,
            fee_vault: Pubkey::new_unique(),
            gross_sol_in_lamports: 0,
            min_net_output: 1,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        };
        let pump_wrapper =
            try_build_pump_bonding_v2_wrapper_instruction(&payer, &request, &venue_ix)
                .expect("pump wrapper candidate")
                .expect("pump wrapper");
        assert_eq!(pump_wrapper.data[0], EXECUTE_PUMP_BONDING_V2_DISCRIMINATOR);
        assert!(
            try_build_wsol_out_v3_plan(&payer, &request, &venue_ix, &[], &[], user_wsol_account)
                .is_none()
        );
    }

    #[test]
    fn wrap_compiled_pump_bonding_v2_adds_wsol_ata_creates() {
        let payer = stub_payer();
        let payer_pubkey = payer.pubkey();
        let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();
        let fee_vault = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let user_base_ata = get_associated_token_address_with_program_id(
            &payer_pubkey,
            &base_mint,
            &TOKEN_PROGRAM_ID,
        );
        let user_wsol_ata = get_associated_token_address_with_program_id(
            &payer_pubkey,
            &WSOL_MINT,
            &TOKEN_PROGRAM_ID,
        );
        let fee_vault_wsol_ata =
            get_associated_token_address_with_program_id(&fee_vault, &WSOL_MINT, &TOKEN_PROGRAM_ID);
        let mut venue_accounts: Vec<AccountMeta> = (0..PUMP_V2_BUY_ACCOUNT_COUNT)
            .map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false))
            .collect();
        venue_accounts[PUMP_V2_BASE_MINT_INDEX] = AccountMeta::new_readonly(base_mint, false);
        venue_accounts[PUMP_V2_QUOTE_MINT_INDEX] = AccountMeta::new_readonly(WSOL_MINT, false);
        venue_accounts[PUMP_V2_BASE_TOKEN_PROGRAM_INDEX] =
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false);
        venue_accounts[PUMP_V2_USER_INDEX] = AccountMeta::new(payer_pubkey, true);
        venue_accounts[PUMP_V2_ASSOCIATED_BASE_USER_INDEX] = AccountMeta::new(user_base_ata, false);
        venue_accounts[PUMP_V2_ASSOCIATED_QUOTE_USER_INDEX] =
            AccountMeta::new(user_wsol_ata, false);
        venue_accounts[PUMP_V2_BUY_PROGRAM_INDEX] = AccountMeta::new_readonly(pump_program, false);
        let mut data = PUMP_BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&99_900_000u64.to_le_bytes());
        data.extend_from_slice(&1u64.to_le_bytes());
        let venue_ix = Instruction {
            program_id: pump_program,
            accounts: venue_accounts,
            data,
        };
        let mut alt_addresses = vec![
            PROGRAM_ID,
            config_pda().0,
            instructions_sysvar_id(),
            fee_vault,
            fee_vault_wsol_ata,
            user_base_ata,
            base_mint,
            user_wsol_ata,
            WSOL_MINT,
            TOKEN_PROGRAM_ID,
            system_program_id(),
            spl_associated_token_account::id(),
            pump_program,
            memo_program_id(),
        ];
        alt_addresses.extend(venue_ix.accounts.iter().map(|meta| meta.pubkey));
        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: alt_addresses,
        };
        let message = v0::Message::try_compile(
            &payer_pubkey,
            std::slice::from_ref(&venue_ix),
            std::slice::from_ref(&alt),
            Hash::new_unique(),
        )
        .expect("compile native Pump v2");
        let signed = VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer])
            .expect("sign native");
        let native_compiled = CompiledTransaction {
            label: "pump-v2-buy-native".to_string(),
            format: "v0-alt".to_string(),
            serialized_base64: BASE64.encode(bincode::serialize(&signed).expect("serialize")),
            signature: signed.signatures.first().map(|s| s.to_string()),
            lookup_tables_used: vec![alt.key.to_string()],
            compute_unit_limit: Some(300_000),
            compute_unit_price_micro_lamports: Some(2_000),
            inline_tip_lamports: None,
            inline_tip_account: None,
        };

        let wrapped = wrap_compiled_transaction(
            &native_compiled,
            &payer,
            std::slice::from_ref(&alt),
            &[pump_program],
            &WrapCompiledTransactionRequest {
                label: "pump-v2-buy-wrapper".to_string(),
                route_kind: WrapperRouteKind::SolIn,
                fee_bps: 10,
                fee_vault,
                gross_sol_in_lamports: 100_000_000,
                min_net_output: 1,
                select_first_allowlisted_venue_instruction: false,
                select_last_allowlisted_venue_instruction: false,
            },
        )
        .expect("wrap succeeds");

        let wrapped_bytes = BASE64.decode(wrapped.serialized_base64.as_bytes()).unwrap();
        let wrapped_tx: VersionedTransaction = bincode::deserialize(&wrapped_bytes).unwrap();
        let (decoded, _) =
            decompile_v0_transaction(&wrapped_tx, std::slice::from_ref(&alt)).unwrap();
        let ata_creates = decoded
            .iter()
            .filter(|ix| ix.program_id == spl_associated_token_account::id())
            .collect::<Vec<_>>();
        assert_eq!(ata_creates.len(), 3);
        assert_eq!(ata_creates[0].accounts[1].pubkey, user_base_ata);
        assert_eq!(ata_creates[0].accounts[2].pubkey, payer_pubkey);
        assert_eq!(ata_creates[0].accounts[3].pubkey, base_mint);
        assert_eq!(ata_creates[1].accounts[1].pubkey, user_wsol_ata);
        assert_eq!(ata_creates[1].accounts[2].pubkey, payer_pubkey);
        assert_eq!(ata_creates[1].accounts[3].pubkey, WSOL_MINT);
        assert_eq!(ata_creates[2].accounts[1].pubkey, fee_vault_wsol_ata);
        assert_eq!(ata_creates[2].accounts[2].pubkey, fee_vault);
        assert_eq!(ata_creates[2].accounts[3].pubkey, WSOL_MINT);
        let wrapper_index = decoded
            .iter()
            .position(|ix| ix.program_id == PROGRAM_ID)
            .expect("wrapper ix");
        assert!(wrapper_index > 1);
        assert_eq!(
            decoded[wrapper_index].data[0],
            EXECUTE_PUMP_BONDING_V2_DISCRIMINATOR
        );
        assert!(
            decoded[..wrapper_index]
                .iter()
                .filter(|ix| ix.program_id == spl_associated_token_account::id())
                .count()
                == 3
        );
    }

    #[test]
    fn raydium_v4_v3_sell_preserves_exact_inner_account_count() {
        let payer = stub_payer();
        let fee_vault = Pubkey::new_unique();
        let venue_program = raydium_amm_v4_program_id();
        let user_wsol_account = Pubkey::new_unique();
        let mut venue_accounts = (0..18)
            .map(|_| AccountMeta::new(Pubkey::new_unique(), false))
            .collect::<Vec<_>>();
        venue_accounts[16] = AccountMeta::new(user_wsol_account, false);
        venue_accounts[17] = AccountMeta::new_readonly(payer.pubkey(), true);
        let mut alt_addresses = vec![
            PROGRAM_ID,
            config_pda().0,
            instructions_sysvar_id(),
            fee_vault,
            get_associated_token_address_with_program_id(&fee_vault, &WSOL_MINT, &TOKEN_PROGRAM_ID),
            user_wsol_account,
            route_wsol_pda(&payer.pubkey(), 0).0,
            WSOL_MINT,
            TOKEN_PROGRAM_ID,
            system_program_id(),
            venue_program,
            memo_program_id(),
        ];
        alt_addresses.extend(venue_accounts.iter().map(|meta| meta.pubkey));
        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: alt_addresses,
        };
        let venue_ix = Instruction {
            program_id: venue_program,
            accounts: venue_accounts,
            data: vec![9],
        };
        let message = v0::Message::try_compile(
            &payer.pubkey(),
            &[venue_ix],
            std::slice::from_ref(&alt),
            Hash::new_unique(),
        )
        .expect("compile native");
        let signed = VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer])
            .expect("sign native");
        let native_compiled = CompiledTransaction {
            label: "raydium-v4-sell-native".to_string(),
            format: "v0-alt".to_string(),
            serialized_base64: BASE64.encode(bincode::serialize(&signed).expect("serialize")),
            signature: signed.signatures.first().map(|s| s.to_string()),
            lookup_tables_used: vec![alt.key.to_string()],
            compute_unit_limit: Some(340_000),
            compute_unit_price_micro_lamports: Some(2_000),
            inline_tip_lamports: None,
            inline_tip_account: None,
        };

        let wrapped = wrap_compiled_transaction(
            &native_compiled,
            &payer,
            std::slice::from_ref(&alt),
            &[venue_program],
            &WrapCompiledTransactionRequest {
                label: "raydium-v4-sell-wrapper".to_string(),
                route_kind: WrapperRouteKind::SolOut,
                fee_bps: 10,
                fee_vault,
                gross_sol_in_lamports: 0,
                min_net_output: 1,
                select_first_allowlisted_venue_instruction: false,
                select_last_allowlisted_venue_instruction: false,
            },
        )
        .expect("wrap succeeds");
        let wrapped_bytes = BASE64.decode(wrapped.serialized_base64.as_bytes()).unwrap();
        let wrapped_tx: VersionedTransaction = bincode::deserialize(&wrapped_bytes).unwrap();
        let (decoded, _keys) =
            decompile_v0_transaction(&wrapped_tx, std::slice::from_ref(&alt)).unwrap();
        let wrapper_ix = decoded
            .iter()
            .find(|ix| ix.program_id == PROGRAM_ID)
            .expect("wrapper ix");

        assert_eq!(
            wrapper_ix.accounts.len(),
            (EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT + EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT)
                as usize
                + 1
                + 18
        );
        assert!(
            !wrapper_ix
                .accounts
                .iter()
                .skip(
                    (EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT + EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT)
                        as usize
                )
                .any(|meta| meta.pubkey == system_program_id())
        );
    }

    #[test]
    fn wrap_compiled_transaction_uses_v3_route_wsol_for_raydium_wsol_input() {
        let payer = stub_payer();
        let temp_wsol = Keypair::new();
        let temp_wsol_pubkey = temp_wsol.pubkey();
        let venue_program = raydium_clmm_program_id();
        let fee_vault = Pubkey::new_unique();
        let blockhash = Hash::new_unique();
        let rent_lamports = Rent::default().minimum_balance(SPL_TOKEN_ACCOUNT_LEN);
        let swap_lamports = 100_000_000;
        let (route_wsol, _) = route_wsol_pda(&payer.pubkey(), 0);
        let token_program = TOKEN_PROGRAM_ID;
        let setup_ixs = vec![
            solana_system_interface::instruction::create_account(
                &payer.pubkey(),
                &temp_wsol_pubkey,
                rent_lamports + swap_lamports,
                SPL_TOKEN_ACCOUNT_LEN as u64,
                &TOKEN_PROGRAM_ID,
            ),
            spl_token::instruction::initialize_account3(
                &token_program,
                &temp_wsol_pubkey,
                &WSOL_MINT,
                &payer.pubkey(),
            )
            .expect("initialize account"),
            spl_token::instruction::sync_native(&token_program, &temp_wsol_pubkey)
                .expect("sync native"),
        ];
        let close_ix = spl_token::instruction::close_account(
            &token_program,
            &temp_wsol_pubkey,
            &payer.pubkey(),
            &payer.pubkey(),
            &[],
        )
        .expect("close account");

        let mut venue_accounts = Vec::new();
        for index in 0..13 {
            let pubkey = match index {
                0 => payer.pubkey(),
                3 => temp_wsol_pubkey,
                11 => WSOL_MINT,
                _ => Pubkey::new_unique(),
            };
            venue_accounts.push(if index == 0 {
                AccountMeta::new_readonly(pubkey, true)
            } else if index == 3 {
                AccountMeta::new(pubkey, false)
            } else {
                AccountMeta::new_readonly(pubkey, false)
            });
        }
        let venue_ix = Instruction {
            program_id: venue_program,
            accounts: venue_accounts,
            data: vec![0xCA, 0xFE],
        };

        let mut instructions = vec![fake_cu_ix()];
        instructions.extend(setup_ixs);
        instructions.push(venue_ix);
        instructions.push(close_ix);
        instructions.push(fake_memo_ix());

        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: vec![
                venue_program,
                PROGRAM_ID,
                config_pda().0,
                instructions_sysvar_id(),
                fee_vault,
                TOKEN_PROGRAM_ID,
                system_program_id(),
                rent_sysvar_id(),
                WSOL_MINT,
                route_wsol,
                temp_wsol_pubkey,
            ],
        };
        let message = v0::Message::try_compile(
            &payer.pubkey(),
            &instructions,
            std::slice::from_ref(&alt),
            blockhash,
        )
        .expect("compile native");
        let signed =
            VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer, &temp_wsol])
                .expect("sign native");
        let native_compiled = CompiledTransaction {
            label: "raydium-v2".to_string(),
            format: "v0-alt".to_string(),
            serialized_base64: BASE64
                .encode(bincode::serialize(&signed).expect("serialize native")),
            signature: signed.signatures.first().map(|s| s.to_string()),
            lookup_tables_used: vec![alt.key.to_string()],
            compute_unit_limit: None,
            compute_unit_price_micro_lamports: None,
            inline_tip_lamports: None,
            inline_tip_account: None,
        };
        compiled_transaction_signers::remember_compiled_transaction_signers(
            &native_compiled.serialized_base64,
            &[&temp_wsol],
        );

        let wrapped = wrap_compiled_transaction(
            &native_compiled,
            &payer,
            std::slice::from_ref(&alt),
            &[venue_program],
            &WrapCompiledTransactionRequest {
                label: "raydium-v2".to_string(),
                route_kind: WrapperRouteKind::SolIn,
                fee_bps: 10,
                fee_vault,
                gross_sol_in_lamports: swap_lamports + 42,
                min_net_output: 1,
                select_first_allowlisted_venue_instruction: false,
                select_last_allowlisted_venue_instruction: false,
            },
        )
        .expect("wrap succeeds");

        let wrapped_bytes = BASE64.decode(wrapped.serialized_base64.as_bytes()).unwrap();
        let wrapped_tx: VersionedTransaction = bincode::deserialize(&wrapped_bytes).unwrap();
        let (decoded, _keys) =
            decompile_v0_transaction(&wrapped_tx, std::slice::from_ref(&alt)).unwrap();
        let wrapper_ix = decoded
            .iter()
            .find(|ix| ix.program_id == PROGRAM_ID)
            .expect("wrapper ix");
        assert_eq!(
            wrapper_ix.data[0],
            crate::wrapper_abi::EXECUTE_SWAP_ROUTE_DISCRIMINATOR
        );
        assert!(
            wrapper_ix
                .accounts
                .iter()
                .any(|meta| meta.pubkey == route_wsol)
        );
        let execute_request =
            ExecuteSwapRouteRequest::try_from_slice(&wrapper_ix.data[1..]).expect("decode v3");
        assert_eq!(execute_request.gross_sol_in_lamports, swap_lamports + 42);
        assert_eq!(execute_request.route_mode, SwapRouteMode::Mixed);
        assert_eq!(execute_request.fee_mode, SwapRouteFeeMode::SolPre);
        assert_eq!(
            execute_request.legs[0].input_amount,
            (swap_lamports + 42)
                - estimate_sol_in_fee_lamports(swap_lamports + 42, execute_request.fee_bps)
        );
        assert!(
            decoded
                .iter()
                .all(|ix| !instruction_references_pubkey(ix, &temp_wsol_pubkey)),
            "temp WSOL account should be removed from the wrapped v2 tx"
        );
    }

    #[test]
    fn wrap_compiled_transaction_reports_no_venue_when_none_match() {
        let payer = stub_payer();
        let blockhash = Hash::new_unique();
        // Intentionally build a tx that contains only compute-budget
        // instructions — no venue program anywhere — then assert the
        // wrap layer surfaces `NoVenueInstruction` so the caller can
        // decide whether to skip (preamble-only txs in a multi-tx
        // plan) or fail (single-tx plan that should have had a venue).
        let instructions = vec![fake_cu_ix()];
        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: vec![Pubkey::new_unique()],
        };
        let message =
            v0::Message::try_compile(&payer.pubkey(), &instructions, &[], blockhash).unwrap();
        let signed = VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer])
            .expect("sign native");
        let native_bytes = bincode::serialize(&signed).unwrap();
        let native_compiled = CompiledTransaction {
            label: "cu-only".to_string(),
            format: "v0".to_string(),
            serialized_base64: BASE64.encode(native_bytes),
            signature: signed.signatures.first().map(|s| s.to_string()),
            lookup_tables_used: vec![],
            compute_unit_limit: Some(100_000),
            compute_unit_price_micro_lamports: None,
            inline_tip_lamports: None,
            inline_tip_account: None,
        };

        let err = wrap_compiled_transaction(
            &native_compiled,
            &payer,
            std::slice::from_ref(&alt),
            &[Pubkey::new_unique()],
            &WrapCompiledTransactionRequest {
                label: "wrapper".to_string(),
                route_kind: WrapperRouteKind::SolIn,
                fee_bps: 10,
                fee_vault: Pubkey::new_unique(),
                gross_sol_in_lamports: 1_000_000,
                min_net_output: 1,
                select_first_allowlisted_venue_instruction: false,
                select_last_allowlisted_venue_instruction: false,
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            WrapCompiledTransactionError::NoVenueInstruction
        ));
    }

    #[test]
    fn wrap_compiled_transaction_rejects_extra_signers_in_v3() {
        let payer = stub_payer();
        let temp_signer = Keypair::new();
        let venue_program = Pubkey::new_unique();
        let fee_vault = Pubkey::new_unique();
        let blockhash = Hash::new_unique();
        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: vec![
                venue_program,
                PROGRAM_ID,
                config_pda().0,
                instructions_sysvar_id(),
                fee_vault,
                TOKEN_PROGRAM_ID,
                system_program_id(),
            ],
        };
        let instructions = vec![
            fake_cu_ix(),
            Instruction {
                program_id: venue_program,
                accounts: vec![AccountMeta::new(temp_signer.pubkey(), true)],
                data: vec![0xAB],
            },
        ];
        let message = v0::Message::try_compile(
            &payer.pubkey(),
            &instructions,
            std::slice::from_ref(&alt),
            blockhash,
        )
        .expect("compile native");
        let signed =
            VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer, &temp_signer])
                .expect("sign native");
        let native_compiled = CompiledTransaction {
            label: "wrapped-temp-signer".to_string(),
            format: "v0-alt".to_string(),
            serialized_base64: BASE64
                .encode(bincode::serialize(&signed).expect("serialize native")),
            signature: signed.signatures.first().map(|s| s.to_string()),
            lookup_tables_used: vec![alt.key.to_string()],
            compute_unit_limit: None,
            compute_unit_price_micro_lamports: None,
            inline_tip_lamports: None,
            inline_tip_account: None,
        };
        compiled_transaction_signers::remember_compiled_transaction_signers(
            &native_compiled.serialized_base64,
            &[&temp_signer],
        );

        let err = wrap_compiled_transaction(
            &native_compiled,
            &payer,
            std::slice::from_ref(&alt),
            &[venue_program],
            &WrapCompiledTransactionRequest {
                label: "wrapped-temp-signer".to_string(),
                route_kind: WrapperRouteKind::SolIn,
                fee_bps: 10,
                fee_vault,
                gross_sol_in_lamports: 1_000_000,
                min_net_output: 1,
                select_first_allowlisted_venue_instruction: false,
                select_last_allowlisted_venue_instruction: false,
            },
        )
        .expect_err("v3 wrapper must reject extra signer routes");
        assert!(
            err.to_string().contains("must require exactly one signer"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn wrap_compiled_transaction_can_select_first_allowlisted_venue() {
        let payer = stub_payer();
        let venue_program = Pubkey::new_unique();
        let fee_vault = Pubkey::new_unique();
        let blockhash = Hash::new_unique();
        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: vec![
                venue_program,
                PROGRAM_ID,
                config_pda().0,
                instructions_sysvar_id(),
                fee_vault,
                TOKEN_PROGRAM_ID,
                system_program_id(),
            ],
        };
        let first_account = Pubkey::new_unique();
        let second_account = Pubkey::new_unique();
        let instructions = vec![
            fake_cu_ix(),
            Instruction {
                program_id: venue_program,
                accounts: vec![AccountMeta::new(first_account, false)],
                data: vec![0x01],
            },
            Instruction {
                program_id: venue_program,
                accounts: vec![AccountMeta::new(second_account, false)],
                data: vec![0x02],
            },
        ];
        let message = v0::Message::try_compile(
            &payer.pubkey(),
            &instructions,
            std::slice::from_ref(&alt),
            blockhash,
        )
        .expect("compile native");
        let signed = VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer])
            .expect("sign native");
        let native_compiled = CompiledTransaction {
            label: "follow-buy-atomic".to_string(),
            format: "v0-alt".to_string(),
            serialized_base64: BASE64
                .encode(bincode::serialize(&signed).expect("serialize native")),
            signature: signed.signatures.first().map(|s| s.to_string()),
            lookup_tables_used: vec![alt.key.to_string()],
            compute_unit_limit: None,
            compute_unit_price_micro_lamports: None,
            inline_tip_lamports: None,
            inline_tip_account: None,
        };

        let wrapped = wrap_compiled_transaction(
            &native_compiled,
            &payer,
            std::slice::from_ref(&alt),
            &[venue_program],
            &WrapCompiledTransactionRequest {
                label: "follow-buy-atomic".to_string(),
                route_kind: WrapperRouteKind::SolIn,
                fee_bps: 10,
                fee_vault,
                gross_sol_in_lamports: 1_000_000,
                min_net_output: 1,
                select_first_allowlisted_venue_instruction: true,
                select_last_allowlisted_venue_instruction: false,
            },
        )
        .expect("wrap succeeds");

        let wrapped_bytes = BASE64.decode(wrapped.serialized_base64.as_bytes()).unwrap();
        let wrapped_tx: VersionedTransaction = bincode::deserialize(&wrapped_bytes).unwrap();
        let (decoded, _) =
            decompile_v0_transaction(&wrapped_tx, std::slice::from_ref(&alt)).unwrap();
        assert_eq!(decoded[1].program_id, PROGRAM_ID);
        assert_eq!(decoded[2].program_id, venue_program);
    }

    #[test]
    fn wrap_compiled_transaction_can_select_last_allowlisted_venue() {
        let payer = stub_payer();
        let venue_program = Pubkey::new_unique();
        let fee_vault = Pubkey::new_unique();
        let blockhash = Hash::new_unique();
        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: vec![
                venue_program,
                PROGRAM_ID,
                config_pda().0,
                instructions_sysvar_id(),
                fee_vault,
                TOKEN_PROGRAM_ID,
                system_program_id(),
            ],
        };
        let first_account = Pubkey::new_unique();
        let second_account = Pubkey::new_unique();
        let instructions = vec![
            fake_cu_ix(),
            Instruction {
                program_id: venue_program,
                accounts: vec![AccountMeta::new(first_account, false)],
                data: vec![0x01],
            },
            Instruction {
                program_id: venue_program,
                accounts: vec![AccountMeta::new(second_account, false)],
                data: vec![0x02],
            },
        ];
        let message = v0::Message::try_compile(
            &payer.pubkey(),
            &instructions,
            std::slice::from_ref(&alt),
            blockhash,
        )
        .expect("compile native");
        let signed = VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer])
            .expect("sign native");
        let native_compiled = CompiledTransaction {
            label: "follow-sell".to_string(),
            format: "v0-alt".to_string(),
            serialized_base64: BASE64
                .encode(bincode::serialize(&signed).expect("serialize native")),
            signature: signed.signatures.first().map(|s| s.to_string()),
            lookup_tables_used: vec![alt.key.to_string()],
            compute_unit_limit: None,
            compute_unit_price_micro_lamports: None,
            inline_tip_lamports: None,
            inline_tip_account: None,
        };

        let wrapped = wrap_compiled_transaction(
            &native_compiled,
            &payer,
            std::slice::from_ref(&alt),
            &[venue_program],
            &WrapCompiledTransactionRequest {
                label: "follow-sell".to_string(),
                route_kind: WrapperRouteKind::SolOut,
                fee_bps: 10,
                fee_vault,
                gross_sol_in_lamports: 0,
                min_net_output: 1,
                select_first_allowlisted_venue_instruction: false,
                select_last_allowlisted_venue_instruction: true,
            },
        )
        .expect("wrap succeeds");

        let wrapped_bytes = BASE64.decode(wrapped.serialized_base64.as_bytes()).unwrap();
        let wrapped_tx: VersionedTransaction = bincode::deserialize(&wrapped_bytes).unwrap();
        let (decoded, _) =
            decompile_v0_transaction(&wrapped_tx, std::slice::from_ref(&alt)).unwrap();
        assert_eq!(decoded[1].program_id, venue_program);
        assert_eq!(decoded[2].program_id, PROGRAM_ID);
    }

    #[test]
    fn derive_wsol_route_accounts_recovers_temp_raydium_clmm_output_account() {
        let payer = stub_payer();
        let fee_vault = Pubkey::new_unique();
        let temp_wsol_account = Pubkey::new_unique();
        let fee_vault_wsol_ata =
            get_associated_token_address_with_program_id(&fee_vault, &WSOL_MINT, &TOKEN_PROGRAM_ID);
        let venue_accounts = vec![
            AccountMeta::new_readonly(payer.pubkey(), true),
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
            AccountMeta::new(Pubkey::new_unique(), false),
            AccountMeta::new(Pubkey::new_unique(), false),
            AccountMeta::new(temp_wsol_account, false),
            AccountMeta::new(Pubkey::new_unique(), false),
            AccountMeta::new(Pubkey::new_unique(), false),
            AccountMeta::new(Pubkey::new_unique(), false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
            AccountMeta::new_readonly(memo_program_id(), false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
            AccountMeta::new_readonly(WSOL_MINT, false),
        ];

        let (user_wsol_account, fee_vault_route_account) = derive_wsol_route_accounts(
            &payer.pubkey(),
            &fee_vault,
            WrapperRouteKind::SolOut,
            &raydium_clmm_program_id(),
            &[],
            &venue_accounts,
            &[],
            &[],
        )
        .expect("derive raydium wsol route accounts");

        assert_eq!(user_wsol_account, Some(temp_wsol_account));
        assert_eq!(fee_vault_route_account, Some(fee_vault_wsol_ata));
    }
}
