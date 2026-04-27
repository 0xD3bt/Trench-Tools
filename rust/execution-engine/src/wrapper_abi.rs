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
pub const ABI_VERSION: u8 = 1;

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

/// Discriminator prefixing the runtime `Execute` instruction.
pub const EXECUTE_DISCRIMINATOR: u8 = 1;
pub const EXECUTE_AMM_WSOL_DISCRIMINATOR: u8 = 7;

/// Seeds used to derive the program's singleton Config PDA.
pub const CONFIG_SEED: &[u8] = b"config";
pub const AMM_WSOL_SEED: &[u8] = b"amm-wsol";

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

/// Byte layout of the `Execute` instruction data.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ExecuteRequest {
    pub version: u8,
    pub route_kind: WrapperRouteKind,
    pub fee_bps: u16,
    /// Gross SOL input in lamports. Required for `SolIn`, ignored for
    /// `SolOut` (zero-initialized on the wire).
    pub gross_sol_in_lamports: u64,
    /// Net floor the user must receive. Units depend on `route_kind`:
    /// - `SolIn` buy: minimum token base-units delivered.
    /// - `SolOut` sell: minimum net SOL lamports delivered after fee.
    /// - `SolThrough`: reserved.
    pub min_net_output: u64,
    /// Number of fixed-prefix wrapper accounts that precede the inner
    /// CPI account list. The program uses this to split the `accounts`
    /// slice into `[wrapper_prefix..][inner_accounts..]`.
    pub inner_accounts_offset: u16,
    /// Opaque instruction data forwarded verbatim to the inner venue
    /// program. The wrapper does NOT parse this payload; the inner
    /// program's own decoder handles it.
    pub inner_ix_data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum WsolAccountMode {
    CreateOrReuse = 0,
    ReuseOnly = 1,
}

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

pub const EXECUTE_FIXED_ACCOUNT_COUNT: u16 = 8;
pub const EXECUTE_AMM_WSOL_FIXED_ACCOUNT_COUNT: u16 = 12;

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

#[derive(Debug, Clone, Copy)]
pub struct ExecuteAmmWsolAccounts<'a> {
    pub execute: ExecuteAccounts<'a>,
    pub amm_wsol_account: &'a Pubkey,
    pub wsol_mint: &'a Pubkey,
    pub system_program: &'a Pubkey,
    pub rent_sysvar: &'a Pubkey,
}

impl<'a> ExecuteAmmWsolAccounts<'a> {
    pub fn to_account_metas(&self) -> Vec<AccountMeta> {
        let mut metas = self.execute.to_account_metas();
        metas.extend([
            AccountMeta::new(*self.amm_wsol_account, false),
            AccountMeta::new_readonly(*self.wsol_mint, false),
            AccountMeta::new_readonly(*self.system_program, false),
            AccountMeta::new_readonly(*self.rent_sysvar, false),
        ]);
        metas
    }
}

pub fn amm_wsol_pda(user: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[AMM_WSOL_SEED, user.as_ref()], &PROGRAM_ID)
}

/// Serialize an `Execute` instruction payload.
pub fn encode_execute_data(request: &ExecuteRequest) -> Result<Vec<u8>, String> {
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
    let mut buf = Vec::with_capacity(64 + request.inner_ix_data.len());
    buf.push(EXECUTE_DISCRIMINATOR);
    borsh::to_writer(&mut buf, request)
        .map_err(|error| format!("Failed to serialize Execute request: {error}"))?;
    Ok(buf)
}

pub fn encode_execute_amm_wsol_data(request: &ExecuteAmmWsolRequest) -> Result<Vec<u8>, String> {
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
    let mut buf = Vec::with_capacity(80 + request.inner_ix_data.len());
    buf.push(EXECUTE_AMM_WSOL_DISCRIMINATOR);
    borsh::to_writer(&mut buf, request)
        .map_err(|error| format!("Failed to serialize ExecuteAmmWsol request: {error}"))?;
    Ok(buf)
}

/// Build a wrapper `Execute` instruction.
pub fn build_execute_instruction(
    accounts: &ExecuteAccounts<'_>,
    request: &ExecuteRequest,
    inner_accounts: &[AccountMeta],
) -> Result<Instruction, String> {
    if request.inner_accounts_offset != EXECUTE_FIXED_ACCOUNT_COUNT {
        return Err(format!(
            "inner_accounts_offset must equal EXECUTE_FIXED_ACCOUNT_COUNT ({}), got {}",
            EXECUTE_FIXED_ACCOUNT_COUNT, request.inner_accounts_offset
        ));
    }
    let mut metas = accounts.to_account_metas();
    metas.extend_from_slice(inner_accounts);
    let data = encode_execute_data(request)?;
    Ok(Instruction {
        program_id: PROGRAM_ID,
        accounts: metas,
        data,
    })
}

pub fn build_execute_amm_wsol_instruction(
    accounts: &ExecuteAmmWsolAccounts<'_>,
    request: &ExecuteAmmWsolRequest,
    inner_accounts: &[AccountMeta],
) -> Result<Instruction, String> {
    if request.inner_accounts_offset != EXECUTE_AMM_WSOL_FIXED_ACCOUNT_COUNT {
        return Err(format!(
            "inner_accounts_offset must equal EXECUTE_AMM_WSOL_FIXED_ACCOUNT_COUNT ({}), got {}",
            EXECUTE_AMM_WSOL_FIXED_ACCOUNT_COUNT, request.inner_accounts_offset
        ));
    }
    let mut metas = accounts.to_account_metas();
    metas.extend_from_slice(inner_accounts);
    let data = encode_execute_amm_wsol_data(request)?;
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
    fn execute_round_trips() {
        let request = ExecuteRequest {
            version: ABI_VERSION,
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            gross_sol_in_lamports: 500_000_000,
            min_net_output: 1_000,
            inner_accounts_offset: EXECUTE_FIXED_ACCOUNT_COUNT,
            inner_ix_data: vec![1, 2, 3],
        };
        let bytes = encode_execute_data(&request).expect("encode");
        assert_eq!(bytes[0], EXECUTE_DISCRIMINATOR);
        let decoded = ExecuteRequest::try_from_slice(&bytes[1..]).expect("roundtrip");
        assert_eq!(decoded, request);
    }

    #[test]
    fn execute_amm_wsol_round_trips() {
        let request = ExecuteAmmWsolRequest {
            version: ABI_VERSION,
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            gross_sol_in_lamports: 500_000_000,
            min_net_output: 1_000,
            inner_accounts_offset: EXECUTE_FIXED_ACCOUNT_COUNT + 4,
            wsol_account_mode: WsolAccountMode::CreateOrReuse,
            pda_wsol_lamports: 499_000_000,
            inner_wsol_account_index: 3,
            inner_ix_data: vec![1, 2, 3],
        };
        let bytes = encode_execute_amm_wsol_data(&request).expect("encode");
        assert_eq!(bytes[0], EXECUTE_AMM_WSOL_DISCRIMINATOR);
        let decoded = ExecuteAmmWsolRequest::try_from_slice(&bytes[1..]).expect("roundtrip");
        assert_eq!(decoded, request);
    }

    #[test]
    fn build_execute_amm_wsol_instruction_uses_v2_fixed_prefix() {
        let user = Pubkey::new_unique();
        let config = config_pda().0;
        let fee_vault = Pubkey::new_unique();
        let zero = Pubkey::default();
        let instructions = instructions_sysvar_id();
        let inner_program = Pubkey::new_unique();
        let token_program = TOKEN_PROGRAM_ID;
        let (amm_wsol, _) = amm_wsol_pda(&user);
        let system = system_program_id();
        let rent = rent_sysvar_id();
        let accounts = ExecuteAmmWsolAccounts {
            execute: ExecuteAccounts {
                user: &user,
                config_pda: &config,
                fee_vault: &fee_vault,
                fee_vault_wsol_ata: &zero,
                user_wsol_ata: &zero,
                instructions_sysvar: &instructions,
                inner_program: &inner_program,
                token_program: &token_program,
            },
            amm_wsol_account: &amm_wsol,
            wsol_mint: &WSOL_MINT,
            system_program: &system,
            rent_sysvar: &rent,
        };
        let request = ExecuteAmmWsolRequest {
            version: ABI_VERSION,
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 10,
            gross_sol_in_lamports: 500_000_000,
            min_net_output: 1_000,
            inner_accounts_offset: EXECUTE_AMM_WSOL_FIXED_ACCOUNT_COUNT,
            wsol_account_mode: WsolAccountMode::CreateOrReuse,
            pda_wsol_lamports: 499_000_000,
            inner_wsol_account_index: 3,
            inner_ix_data: vec![1, 2, 3],
        };
        let instruction =
            build_execute_amm_wsol_instruction(&accounts, &request, &[]).expect("build");
        assert_eq!(instruction.accounts.len(), 12);
        assert_eq!(instruction.accounts[8].pubkey, amm_wsol);
        assert_eq!(instruction.data[0], EXECUTE_AMM_WSOL_DISCRIMINATOR);
    }

    #[test]
    fn encode_rejects_fee_bps_above_cap() {
        let request = ExecuteRequest {
            version: ABI_VERSION,
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: MAX_FEE_BPS + 1,
            gross_sol_in_lamports: 0,
            min_net_output: 0,
            inner_accounts_offset: EXECUTE_FIXED_ACCOUNT_COUNT,
            inner_ix_data: vec![],
        };
        let err = encode_execute_data(&request).unwrap_err();
        assert!(err.contains("exceeds hardcoded cap"));
    }

    #[test]
    fn encode_rejects_stale_version() {
        let request = ExecuteRequest {
            version: ABI_VERSION.wrapping_add(1),
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: 0,
            gross_sol_in_lamports: 0,
            min_net_output: 0,
            inner_accounts_offset: EXECUTE_FIXED_ACCOUNT_COUNT,
            inner_ix_data: vec![],
        };
        let err = encode_execute_data(&request).unwrap_err();
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
        assert_eq!(ABI_VERSION, 1, "engine ABI_VERSION drifted from program");
        assert_eq!(MAX_FEE_BPS, 20, "engine fee cap drifted from program");
    }

    #[test]
    fn execute_fixed_account_count_is_eight() {
        assert_eq!(EXECUTE_FIXED_ACCOUNT_COUNT, 8);
    }
}
