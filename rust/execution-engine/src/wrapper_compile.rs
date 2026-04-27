//! Compile and wrap SOL-touching transactions for the fee-routing
//! program.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
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
        ABI_VERSION, EXECUTE_AMM_WSOL_FIXED_ACCOUNT_COUNT, EXECUTE_FIXED_ACCOUNT_COUNT,
        ExecuteAccounts, ExecuteAmmWsolAccounts, ExecuteAmmWsolRequest, ExecuteRequest,
        MAX_FEE_BPS, PROGRAM_ID, TOKEN_PROGRAM_ID, WSOL_MINT, WrapperRouteKind, WsolAccountMode,
        amm_wsol_pda, build_execute_amm_wsol_instruction, build_execute_instruction, config_pda,
        instructions_sysvar_id, rent_sysvar_id, system_program_id,
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
const SPL_TOKEN_ACCOUNT_LEN: usize = 165;
const SYSTEM_CREATE_ACCOUNT_DISCRIMINATOR: u32 = 0;
const SPL_TOKEN_CLOSE_ACCOUNT_DISCRIMINATOR: u8 = 9;
const SPL_TOKEN_SYNC_NATIVE_DISCRIMINATOR: u8 = 17;
const SPL_TOKEN_INITIALIZE_ACCOUNT3_DISCRIMINATOR: u8 = 18;

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

/// Build the wrapper `Execute` instruction from a compile request.
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

    // WSOL-settling SolOut routes need both WSOL ATA slots.
    let (fee_vault_wsol_provided, user_wsol_provided) = (
        request.fee_vault_wsol_ata.is_some(),
        request.user_wsol_ata.is_some(),
    );
    if matches!(request.route_kind, WrapperRouteKind::SolOut)
        && fee_vault_wsol_provided != user_wsol_provided
    {
        return Err(WrapperCompileError::InvalidRouteKind);
    }

    let user_pubkey = request.payer.pubkey();
    let (config_pda_pubkey, _bump) = config_pda();
    let fee_vault_wsol_ata = request
        .fee_vault_wsol_ata
        .unwrap_or(ZEROED_WSOL_ATA_SENTINEL);
    let user_wsol_ata = request.user_wsol_ata.unwrap_or(ZEROED_WSOL_ATA_SENTINEL);
    let instructions_sysvar = instructions_sysvar_id();
    let token_program = TOKEN_PROGRAM_ID;

    let accounts = ExecuteAccounts {
        user: &user_pubkey,
        config_pda: &config_pda_pubkey,
        fee_vault: &request.fee_vault,
        fee_vault_wsol_ata: &fee_vault_wsol_ata,
        user_wsol_ata: &user_wsol_ata,
        instructions_sysvar: &instructions_sysvar,
        inner_program: &request.inner_program,
        token_program: &token_program,
    };

    let execute_request = ExecuteRequest {
        version: ABI_VERSION,
        route_kind: request.route_kind,
        fee_bps: request.fee_bps,
        gross_sol_in_lamports: request.gross_sol_in_lamports,
        min_net_output: request.min_net_output,
        inner_accounts_offset: EXECUTE_FIXED_ACCOUNT_COUNT,
        inner_ix_data: request.inner_ix_data.clone(),
    };

    build_execute_instruction(&accounts, &execute_request, &request.inner_accounts)
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

/// Derive the user WSOL settlement account and fee-vault WSOL ATA for
/// WSOL-settled routes. The user-side account can be either the
/// payer's WSOL ATA or a temporary wrapped-SOL account created around
/// the venue leg.
fn derive_wsol_route_accounts(
    payer: &Pubkey,
    fee_vault: &Pubkey,
    inner_program: &Pubkey,
    venue_accounts: &[AccountMeta],
) -> (Option<Pubkey>, Option<Pubkey>) {
    let payer_wsol_ata =
        get_associated_token_address_with_program_id(payer, &WSOL_MINT, &TOKEN_PROGRAM_ID);
    let user_wsol_account = if venue_accounts
        .iter()
        .any(|meta| meta.pubkey == payer_wsol_ata)
    {
        Some(payer_wsol_ata)
    } else {
        derive_temp_wsol_output_account(inner_program, venue_accounts)
    };
    let fee_vault_wsol_ata = user_wsol_account.map(|_| {
        get_associated_token_address_with_program_id(fee_vault, &WSOL_MINT, &TOKEN_PROGRAM_ID)
    });
    (user_wsol_account, fee_vault_wsol_ata)
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

fn derive_amm_wsol_input_account_index(
    inner_program: &Pubkey,
    venue_accounts: &[AccountMeta],
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
    None
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

fn derive_raydium_amm_v4_wsol_output_account(venue_accounts: &[AccountMeta]) -> Option<Pubkey> {
    let user_output_account = venue_accounts.get(16)?;
    let user_owner = venue_accounts.get(17)?;
    (user_output_account.is_writable && user_owner.is_signer).then_some(user_output_account.pubkey)
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
        derive_amm_wsol_input_account_index(&venue_ix.program_id, &venue_ix.accounts)?;
    let original_wsol_account = venue_ix.accounts.get(inner_wsol_account_index)?.pubkey;
    let create_lamports = find_temp_wsol_create_lamports(preamble, &original_wsol_account)?;
    let rent_lamports = Rent::default().minimum_balance(SPL_TOKEN_ACCOUNT_LEN);
    let pda_wsol_lamports = create_lamports.checked_sub(rent_lamports)?;
    if pda_wsol_lamports == 0 {
        return None;
    }

    let preamble = strip_temp_wsol_lifecycle_instructions(preamble, &original_wsol_account)?;
    let postamble = strip_temp_wsol_lifecycle_instructions(postamble, &original_wsol_account)?;

    let (amm_wsol_account, _) = amm_wsol_pda(payer);
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

    let venue_ix = instructions[venue_idx].clone();
    let (user_wsol_ata, fee_vault_wsol_ata) = derive_wsol_route_accounts(
        &payer.pubkey(),
        &request.fee_vault,
        &venue_ix.program_id,
        &venue_ix.accounts,
    );
    let mut inner_accounts = venue_ix.accounts.clone();
    if should_append_system_program_inner_account(&venue_ix.program_id) {
        append_system_program_inner_account(&mut inner_accounts);
    }

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

    let v2_plan =
        try_build_amm_wsol_v2_plan(&payer.pubkey(), request, &venue_ix, &preamble, &postamble);
    let (wrapper_execute, preamble, postamble, wrapper_mode) = if let Some(plan) = v2_plan {
        let user_pubkey = payer.pubkey();
        let (config_pda_pubkey, _bump) = config_pda();
        let fee_vault_wsol_sentinel = ZEROED_WSOL_ATA_SENTINEL;
        let user_wsol_sentinel = ZEROED_WSOL_ATA_SENTINEL;
        let instructions_sysvar = instructions_sysvar_id();
        let token_program = TOKEN_PROGRAM_ID;
        let system_program = system_program_id();
        let rent_sysvar = rent_sysvar_id();
        let execute_accounts = ExecuteAccounts {
            user: &user_pubkey,
            config_pda: &config_pda_pubkey,
            fee_vault: &request.fee_vault,
            fee_vault_wsol_ata: &fee_vault_wsol_sentinel,
            user_wsol_ata: &user_wsol_sentinel,
            instructions_sysvar: &instructions_sysvar,
            inner_program: &venue_ix.program_id,
            token_program: &token_program,
        };
        let accounts = ExecuteAmmWsolAccounts {
            execute: execute_accounts,
            amm_wsol_account: &plan.amm_wsol_account,
            wsol_mint: &WSOL_MINT,
            system_program: &system_program,
            rent_sysvar: &rent_sysvar,
        };
        let execute_request = ExecuteAmmWsolRequest {
            version: ABI_VERSION,
            route_kind: request.route_kind,
            fee_bps: request.fee_bps,
            gross_sol_in_lamports: plan.pda_wsol_lamports,
            min_net_output: request.min_net_output,
            inner_accounts_offset: EXECUTE_AMM_WSOL_FIXED_ACCOUNT_COUNT,
            wsol_account_mode: WsolAccountMode::CreateOrReuse,
            pda_wsol_lamports: plan.pda_wsol_lamports,
            inner_wsol_account_index: u16::try_from(plan.inner_wsol_account_index)
                .map_err(|_| WrapCompiledTransactionError::VenueAccountOutOfBounds)?,
            inner_ix_data: venue_ix.data.clone(),
        };
        let instruction =
            build_execute_amm_wsol_instruction(&accounts, &execute_request, &plan.inner_accounts)
                .map_err(|error| WrapCompiledTransactionError::CompileFailed(error.to_string()))?;
        (instruction, plan.preamble, plan.postamble, "v2-amm-wsol")
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
        (instruction, preamble, postamble, "legacy")
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
        assert_eq!(ix.accounts.len(), EXECUTE_FIXED_ACCOUNT_COUNT as usize + 2);
        assert_eq!(ix.data[0], 1);
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
    fn build_execute_wsol_sell_populates_both_wsol_slots() {
        let payer = stub_payer();
        let user_wsol = Pubkey::new_unique();
        let vault_wsol = Pubkey::new_unique();
        let mut request = stub_request(&payer, WrapperRouteKind::SolOut, 0);
        request.user_wsol_ata = Some(user_wsol);
        request.fee_vault_wsol_ata = Some(vault_wsol);
        let ix = build_wrapper_execute_instruction(&request).expect("build");
        assert_eq!(ix.accounts[3].pubkey, vault_wsol);
        assert_eq!(ix.accounts[4].pubkey, user_wsol);
        assert!(ix.accounts[3].is_writable);
        assert!(ix.accounts[4].is_writable);
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
    fn raydium_v4_v2_wsol_plan_preserves_exact_inner_account_count() {
        let payer = Pubkey::new_unique();
        let temp_wsol = Pubkey::new_unique();
        let (amm_wsol, _) = amm_wsol_pda(&payer);
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
        assert_eq!(plan.inner_accounts[15].pubkey, amm_wsol);
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
    fn raydium_v4_legacy_sell_preserves_exact_inner_account_count() {
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
            EXECUTE_FIXED_ACCOUNT_COUNT as usize + 18
        );
        assert!(
            !wrapper_ix
                .accounts
                .iter()
                .skip(EXECUTE_FIXED_ACCOUNT_COUNT as usize)
                .any(|meta| meta.pubkey == system_program_id())
        );
    }

    #[test]
    fn wrap_compiled_transaction_uses_v2_for_raydium_wsol_input() {
        let payer = stub_payer();
        let temp_wsol = Keypair::new();
        let temp_wsol_pubkey = temp_wsol.pubkey();
        let venue_program = raydium_clmm_program_id();
        let fee_vault = Pubkey::new_unique();
        let blockhash = Hash::new_unique();
        let rent_lamports = Rent::default().minimum_balance(SPL_TOKEN_ACCOUNT_LEN);
        let swap_lamports = 100_000_000;
        let (amm_wsol, _) = amm_wsol_pda(&payer.pubkey());
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
                amm_wsol,
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
            crate::wrapper_abi::EXECUTE_AMM_WSOL_DISCRIMINATOR
        );
        assert!(
            wrapper_ix
                .accounts
                .iter()
                .any(|meta| meta.pubkey == amm_wsol)
        );
        let execute_request =
            ExecuteAmmWsolRequest::try_from_slice(&wrapper_ix.data[1..]).expect("decode v2");
        assert_eq!(execute_request.gross_sol_in_lamports, swap_lamports);
        assert_eq!(execute_request.pda_wsol_lamports, swap_lamports);
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
    fn wrap_compiled_transaction_reuses_registered_extra_signers() {
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

        let wrapped = wrap_compiled_transaction(
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
        .expect("wrap succeeds with restored signer");
        assert_eq!(wrapped.format, "v0-alt-wrapper");
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
            &raydium_clmm_program_id(),
            &venue_accounts,
        );

        assert_eq!(user_wsol_account, Some(temp_wsol_account));
        assert_eq!(fee_vault_route_account, Some(fee_vault_wsol_ata));
    }
}
