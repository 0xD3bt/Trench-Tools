//! Minimal runtime ABI for the on-chain fee-routing program.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    sysvar,
};
use solana_system_interface::program as system_program;

/// Deployed fee-routing program id.
pub const PROGRAM_ID: Pubkey = Pubkey::new_from_array([
    6, 196, 127, 156, 96, 207, 120, 50, 171, 67, 238, 180, 137, 200, 26, 173, 65, 208, 186, 99, 39,
    237, 181, 214, 193, 37, 222, 188, 224, 190, 61, 36,
]);

/// Byte value stamped on every wrapper instruction.
pub const ABI_VERSION: u8 = 3;

/// Hard upper bound on the voluntary fee.
pub const MAX_FEE_BPS: u16 = 20;

/// Canonical SPL Token program id.
pub const TOKEN_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
    6, 221, 246, 225, 215, 101, 161, 147, 217, 203, 225, 70, 206, 235, 121, 172, 28, 180, 133, 237,
    95, 91, 55, 145, 58, 140, 245, 133, 126, 255, 0, 169,
]);

/// Canonical native-mint (WSOL) pubkey.
pub const WSOL_MINT: Pubkey = Pubkey::new_from_array([
    6, 155, 136, 87, 254, 171, 129, 132, 251, 104, 127, 99, 70, 24, 192, 53, 218, 196, 57, 220, 26,
    235, 59, 85, 152, 160, 240, 0, 0, 0, 0, 1,
]);

pub const EXECUTE_SWAP_ROUTE_DISCRIMINATOR: u8 = 8;
pub const EXECUTE_PUMP_BONDING_V2_DISCRIMINATOR: u8 = 9;

/// Seeds used to derive the program's singleton Config PDA.
pub const CONFIG_SEED: &[u8] = b"config";
pub const ROUTE_WSOL_SEED: &[u8] = b"route-wsol";
pub const ROUTE_WSOL_LANE_COUNT: u8 = 8;

/// Derive the singleton Config PDA. Returns `(pda, bump)`.
pub fn config_pda() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CONFIG_SEED], &PROGRAM_ID)
}

/// Classifies how SOL flows through a wrapper `Execute` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum WrapperRouteKind {
    /// SOL flows into the swap. Fee is taken from the gross SOL input
    /// before any WSOL wrap or inner CPI. Covers native-SOL buys and
    /// SOL-funded USD1 top-ups.
    SolIn = 0,
    /// SOL flows out of the swap. Fee is taken from gross SOL output
    /// after the inner CPI but before handing the proceeds back to the
    /// user. Covers native-SOL sells and SOL-settled USD1 unwinds.
    SolOut = 1,
    /// Reserved for future compatibility.
    SolThrough = 2,
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

pub const SWAP_ROUTE_NO_PATCH_OFFSET: u16 = u16::MAX;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum PumpBondingV2Side {
    Buy = 0,
    Sell = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum PumpBondingV2QuoteFeeMode {
    Wsol = 0,
    Token = 1,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ExecutePumpBondingV2Request {
    pub version: u8,
    pub side: PumpBondingV2Side,
    pub quote_fee_mode: PumpBondingV2QuoteFeeMode,
    pub fee_bps: u16,
    pub gross_quote_in_amount: u64,
    pub min_base_out_amount: u64,
    pub base_amount_in: u64,
    pub gross_min_quote_out_amount: u64,
    pub net_min_quote_out_amount: u64,
}

/// Fixed order of the accounts that prefix every `Execute` call.
#[derive(Debug, Clone, Copy)]
pub struct ExecuteAccounts<'a> {
    /// User wallet that signs the transaction. Writable because the
    /// program debits the SOL fee from this account on SolIn buys and
    /// receives the SOL proceeds on native-SOL SolOut sells.
    pub user: &'a Pubkey,
    /// Singleton config PDA. Read-only during Execute.
    pub config_pda: &'a Pubkey,
    /// Operator-owned fee vault wallet. Receives the SOL fee on SolIn
    /// buys and native-SOL SolOut sells.
    pub fee_vault: &'a Pubkey,
    /// Fee-vault WSOL ATA. Receives the WSOL fee on WSOL SolOut sells.
    /// May be zeroed when not used.
    pub fee_vault_wsol_ata: &'a Pubkey,
    /// User's WSOL ATA (or zeroed when the route is fully native).
    pub user_wsol_ata: &'a Pubkey,
    /// Instruction sysvar. Used for re-entry introspection.
    pub instructions_sysvar: &'a Pubkey,
    /// Inner venue program id. Must be in the allowlist.
    pub inner_program: &'a Pubkey,
    /// SPL Token program. Needed so the wrapper can CPI a `Transfer` of
    /// the WSOL fee from the user's WSOL ATA to the fee-vault WSOL ATA
    /// on WSOL-settling sells. Callers MUST pass
    /// [`TOKEN_PROGRAM_ID`]; the program rejects any other pubkey here.
    pub token_program: &'a Pubkey,
}

pub const EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT: u16 = 8;
pub const EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT: u16 = 3;
pub const EXECUTE_SWAP_ROUTE_TOKEN_FEE_ACCOUNT_COUNT: u16 = 1;
pub const EXECUTE_PUMP_BONDING_V2_FIXED_ACCOUNT_COUNT: u16 = 6;

impl<'a> ExecuteAccounts<'a> {
    pub fn to_account_metas(&self) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(*self.user, true),
            AccountMeta::new_readonly(*self.config_pda, false),
            AccountMeta::new(*self.fee_vault, false),
            AccountMeta::new(*self.fee_vault_wsol_ata, false),
            AccountMeta::new(*self.user_wsol_ata, false),
            AccountMeta::new_readonly(*self.instructions_sysvar, false),
            AccountMeta::new_readonly(*self.inner_program, false),
            AccountMeta::new_readonly(*self.token_program, false),
        ]
    }
}

pub fn route_wsol_pda(user: &Pubkey, lane: u8) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[ROUTE_WSOL_SEED, user.as_ref(), &[lane]], &PROGRAM_ID)
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

#[derive(Debug, Clone, Copy)]
pub struct ExecuteSwapRouteAccounts<'a> {
    pub execute: ExecuteAccounts<'a>,
    pub token_fee_vault_ata: Option<&'a Pubkey>,
}

#[derive(Debug, Clone, Copy)]
pub struct ExecutePumpBondingV2Accounts<'a> {
    pub user: &'a Pubkey,
    pub config_pda: &'a Pubkey,
    pub fee_vault: &'a Pubkey,
    pub fee_vault_quote_ata: &'a Pubkey,
    pub instructions_sysvar: &'a Pubkey,
    pub pump_program: &'a Pubkey,
}

impl<'a> ExecutePumpBondingV2Accounts<'a> {
    pub fn to_account_metas(&self) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(*self.user, true),
            AccountMeta::new_readonly(*self.config_pda, false),
            AccountMeta::new(*self.fee_vault, false),
            AccountMeta::new(*self.fee_vault_quote_ata, false),
            AccountMeta::new_readonly(*self.instructions_sysvar, false),
            AccountMeta::new_readonly(*self.pump_program, false),
        ]
    }
}

impl<'a> ExecuteSwapRouteAccounts<'a> {
    pub fn to_account_metas(
        &self,
        request: &ExecuteSwapRouteRequest,
    ) -> Result<Vec<AccountMeta>, String> {
        if swap_route_uses_wsol(request) && request.wsol_lane >= ROUTE_WSOL_LANE_COUNT {
            return Err(format!(
                "wrapper route WSOL lane {} outside supported range 0..{}",
                request.wsol_lane, ROUTE_WSOL_LANE_COUNT
            ));
        }
        let mut metas = self.execute.to_account_metas();
        if swap_route_uses_wsol(request) {
            metas.extend([
                AccountMeta::new_readonly(WSOL_MINT, false),
                AccountMeta::new_readonly(system_program_id(), false),
                AccountMeta::new_readonly(rent_sysvar_id(), false),
            ]);
        }
        if swap_route_uses_token_fee(request) {
            let token_fee_vault_ata = self
                .token_fee_vault_ata
                .ok_or_else(|| "token fee route requires token_fee_vault_ata".to_string())?;
            metas.push(AccountMeta::new(*token_fee_vault_ata, false));
        }
        Ok(metas)
    }
}

pub fn encode_execute_swap_route_data(
    request: &ExecuteSwapRouteRequest,
) -> Result<Vec<u8>, String> {
    if request.version != ABI_VERSION {
        return Err(format!(
            "wrapper ABI version mismatch: got {}, expected {}",
            request.version, ABI_VERSION
        ));
    }
    if request.fee_bps > MAX_FEE_BPS {
        return Err(format!(
            "wrapper fee_bps {} exceeds hardcoded cap {}",
            request.fee_bps, MAX_FEE_BPS
        ));
    }
    let mut buf = Vec::with_capacity(
        96 + request
            .legs
            .iter()
            .map(|leg| leg.ix_data.len())
            .sum::<usize>(),
    );
    buf.push(EXECUTE_SWAP_ROUTE_DISCRIMINATOR);
    borsh::to_writer(&mut buf, request)
        .map_err(|error| format!("Failed to serialize ExecuteSwapRoute request: {error}"))?;
    Ok(buf)
}

pub fn build_execute_swap_route_instruction(
    accounts: &ExecuteSwapRouteAccounts<'_>,
    request: &ExecuteSwapRouteRequest,
    route_accounts: &[AccountMeta],
) -> Result<Instruction, String> {
    let mut expected_offset = EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT;
    if swap_route_uses_wsol(request) {
        expected_offset = expected_offset
            .checked_add(EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT)
            .ok_or_else(|| "route_accounts_offset overflowed".to_string())?;
    }
    if swap_route_uses_token_fee(request) {
        expected_offset = expected_offset
            .checked_add(EXECUTE_SWAP_ROUTE_TOKEN_FEE_ACCOUNT_COUNT)
            .ok_or_else(|| "route_accounts_offset overflowed".to_string())?;
    }
    if request.route_accounts_offset != expected_offset {
        return Err(format!(
            "route_accounts_offset must equal expected v3 swap-route prefix ({}), got {}",
            expected_offset, request.route_accounts_offset
        ));
    }
    let mut metas = accounts.to_account_metas(request)?;
    metas.extend_from_slice(route_accounts);
    let data = encode_execute_swap_route_data(request)?;
    Ok(Instruction {
        program_id: PROGRAM_ID,
        accounts: metas,
        data,
    })
}

pub fn encode_execute_pump_bonding_v2_data(
    request: &ExecutePumpBondingV2Request,
) -> Result<Vec<u8>, String> {
    if request.version != ABI_VERSION {
        return Err(format!(
            "wrapper ABI version mismatch: got {}, expected {}",
            request.version, ABI_VERSION
        ));
    }
    if request.fee_bps > MAX_FEE_BPS {
        return Err(format!(
            "wrapper fee_bps {} exceeds hardcoded cap {}",
            request.fee_bps, MAX_FEE_BPS
        ));
    }
    let mut buf = Vec::with_capacity(48);
    buf.push(EXECUTE_PUMP_BONDING_V2_DISCRIMINATOR);
    borsh::to_writer(&mut buf, request)
        .map_err(|error| format!("Failed to serialize ExecutePumpBondingV2 request: {error}"))?;
    Ok(buf)
}

pub fn build_execute_pump_bonding_v2_instruction(
    accounts: &ExecutePumpBondingV2Accounts<'_>,
    request: &ExecutePumpBondingV2Request,
    pump_accounts: &[AccountMeta],
) -> Result<Instruction, String> {
    let mut metas = accounts.to_account_metas();
    metas.extend_from_slice(pump_accounts);
    let data = encode_execute_pump_bonding_v2_data(request)?;
    Ok(Instruction {
        program_id: PROGRAM_ID,
        accounts: metas,
        data,
    })
}

/// Helper for the instructions-sysvar pubkey.
pub const fn instructions_sysvar_id() -> Pubkey {
    sysvar::instructions::ID
}

pub const fn rent_sysvar_id() -> Pubkey {
    sysvar::rent::ID
}

/// Helper for the system-program pubkey.
pub const fn system_program_id() -> Pubkey {
    system_program::ID
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_id_is_expected_deployed_program() {
        assert_eq!(
            PROGRAM_ID.to_string(),
            "TRENCHCfkCTud86C8ZC9kk2CFWJErYz4oZFaYttoxJF",
            "engine PROGRAM_ID bytes drifted from the deployed program"
        );
    }

    #[test]
    fn token_program_id_is_spl_token() {
        assert_eq!(
            TOKEN_PROGRAM_ID.to_string(),
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "SPL Token program pubkey drifted"
        );
    }

    #[test]
    fn wsol_mint_is_canonical_native_mint() {
        assert_eq!(
            WSOL_MINT.to_string(),
            "So11111111111111111111111111111111111111112",
            "WSOL native mint pubkey drifted"
        );
    }

    #[test]
    fn execute_swap_route_round_trips() {
        let request = ExecuteSwapRouteRequest {
            version: ABI_VERSION,
            route_mode: SwapRouteMode::Mixed,
            direction: SwapRouteDirection::Buy,
            settlement: SwapRouteSettlement::Token,
            fee_mode: SwapRouteFeeMode::SolPre,
            wsol_lane: 0,
            fee_bps: 10,
            gross_sol_in_lamports: 500_000_000,
            gross_token_in_amount: 0,
            min_net_output: 1_000,
            route_accounts_offset: EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT
                + EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT,
            intermediate_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
            token_fee_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
            legs: vec![SwapRouteLeg {
                program_account_index: 0,
                accounts_start: 1,
                accounts_len: 4,
                input_source: SwapLegInputSource::GrossSolNetOfFee,
                input_amount: 0,
                input_patch_offset: 8,
                output_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
                ix_data: vec![0; 16],
            }],
        };
        let bytes = encode_execute_swap_route_data(&request).expect("encode");
        assert_eq!(bytes[0], EXECUTE_SWAP_ROUTE_DISCRIMINATOR);
        let decoded = ExecuteSwapRouteRequest::try_from_slice(&bytes[1..]).expect("roundtrip");
        assert_eq!(decoded, request);
    }

    #[test]
    fn execute_pump_bonding_v2_round_trips() {
        let request = ExecutePumpBondingV2Request {
            version: ABI_VERSION,
            side: PumpBondingV2Side::Buy,
            quote_fee_mode: PumpBondingV2QuoteFeeMode::Wsol,
            fee_bps: 10,
            gross_quote_in_amount: 500_000_000,
            min_base_out_amount: 1_000,
            base_amount_in: 0,
            gross_min_quote_out_amount: 0,
            net_min_quote_out_amount: 0,
        };
        let bytes = encode_execute_pump_bonding_v2_data(&request).expect("encode");
        assert_eq!(bytes[0], EXECUTE_PUMP_BONDING_V2_DISCRIMINATOR);
        let decoded = ExecutePumpBondingV2Request::try_from_slice(&bytes[1..]).expect("roundtrip");
        assert_eq!(decoded, request);
    }

    #[test]
    fn build_execute_pump_bonding_v2_instruction_uses_fixed_prefix() {
        let user = Pubkey::new_unique();
        let config = config_pda().0;
        let fee_vault = Pubkey::new_unique();
        let fee_vault_quote_ata = Pubkey::new_unique();
        let instructions = instructions_sysvar_id();
        let pump_program = Pubkey::new_unique();
        let accounts = ExecutePumpBondingV2Accounts {
            user: &user,
            config_pda: &config,
            fee_vault: &fee_vault,
            fee_vault_quote_ata: &fee_vault_quote_ata,
            instructions_sysvar: &instructions,
            pump_program: &pump_program,
        };
        let request = ExecutePumpBondingV2Request {
            version: ABI_VERSION,
            side: PumpBondingV2Side::Sell,
            quote_fee_mode: PumpBondingV2QuoteFeeMode::Wsol,
            fee_bps: 10,
            gross_quote_in_amount: 0,
            min_base_out_amount: 0,
            base_amount_in: 1_000,
            gross_min_quote_out_amount: 500_000_000,
            net_min_quote_out_amount: 499_000_000,
        };
        let pump_account = AccountMeta::new_readonly(pump_program, false);
        let instruction =
            build_execute_pump_bonding_v2_instruction(&accounts, &request, &[pump_account])
                .expect("build");
        assert_eq!(
            instruction.accounts.len(),
            usize::from(EXECUTE_PUMP_BONDING_V2_FIXED_ACCOUNT_COUNT) + 1
        );
        assert_eq!(instruction.accounts[5].pubkey, pump_program);
        assert_eq!(instruction.data[0], EXECUTE_PUMP_BONDING_V2_DISCRIMINATOR);
    }

    #[test]
    fn encode_rejects_fee_bps_above_cap() {
        let request = ExecutePumpBondingV2Request {
            version: ABI_VERSION,
            side: PumpBondingV2Side::Buy,
            quote_fee_mode: PumpBondingV2QuoteFeeMode::Wsol,
            fee_bps: MAX_FEE_BPS + 1,
            gross_quote_in_amount: 0,
            min_base_out_amount: 0,
            base_amount_in: 0,
            gross_min_quote_out_amount: 0,
            net_min_quote_out_amount: 0,
        };
        let err = encode_execute_pump_bonding_v2_data(&request).unwrap_err();
        assert!(err.contains("exceeds hardcoded cap"));
    }

    #[test]
    fn encode_rejects_stale_version() {
        let request = ExecutePumpBondingV2Request {
            version: ABI_VERSION.wrapping_add(1),
            side: PumpBondingV2Side::Buy,
            quote_fee_mode: PumpBondingV2QuoteFeeMode::Wsol,
            fee_bps: 0,
            gross_quote_in_amount: 0,
            min_base_out_amount: 0,
            base_amount_in: 0,
            gross_min_quote_out_amount: 0,
            net_min_quote_out_amount: 0,
        };
        let err = encode_execute_pump_bonding_v2_data(&request).unwrap_err();
        assert!(err.contains("version mismatch"));
    }

    #[test]
    fn config_pda_is_deterministic() {
        let (pda1, bump1) = config_pda();
        let (pda2, bump2) = config_pda();
        assert_eq!(pda1, pda2);
        assert_eq!(bump1, bump2);
    }

    #[test]
    fn abi_version_and_cap_are_frozen_until_lockstep_release() {
        assert_eq!(ABI_VERSION, 3, "engine ABI_VERSION drifted from program");
        assert_eq!(MAX_FEE_BPS, 20, "engine fee cap drifted from program");
    }

    #[test]
    fn pump_bonding_v2_fixed_account_count_is_six() {
        assert_eq!(EXECUTE_PUMP_BONDING_V2_FIXED_ACCOUNT_COUNT, 6);
    }

    #[test]
    fn route_wsol_lanes_are_distinct() {
        let user = Pubkey::new_unique();
        let (lane0, _) = route_wsol_pda(&user, 0);
        let (lane1, _) = route_wsol_pda(&user, 1);
        assert_ne!(lane0, lane1);
        assert_eq!(route_wsol_pda(&user, 0).0, lane0);
    }
}
