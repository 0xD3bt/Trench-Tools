use std::str::FromStr;

use num_bigint::BigUint;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

pub const LAUNCHLAB_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";
pub const RAYDIUM_CPMM_PROGRAM_ID: &str = "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C";
pub const RAYDIUM_AMM_V4_PROGRAM_ID: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
pub const FEE_RATE_DENOMINATOR: u64 = 1_000_000;

pub const BUY_EXACT_IN_DISCRIMINATOR: [u8; 8] = [250, 234, 13, 123, 213, 156, 19, 236];
pub const SELL_EXACT_IN_DISCRIMINATOR: [u8; 8] = [149, 39, 222, 155, 211, 124, 152, 26];
pub const CPMM_SWAP_BASE_INPUT_DISCRIMINATOR: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];
pub const CPMM_SWAP_BASE_OUTPUT_DISCRIMINATOR: [u8; 8] = [55, 217, 98, 86, 163, 74, 180, 173];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchLabPoolStatus {
    Trading,
    Migrating,
    Migrated,
    Unknown(u8),
}

impl LaunchLabPoolStatus {
    pub fn from_raw(value: u8) -> Self {
        match value {
            0 => Self::Trading,
            1 => Self::Migrating,
            2 => Self::Migrated,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedLaunchLabPool {
    pub creator: Pubkey,
    pub status: u8,
    pub migrate_type: u8,
    pub supply: u64,
    pub config_id: Pubkey,
    pub total_sell_a: u64,
    pub virtual_a: u64,
    pub virtual_b: u64,
    pub real_a: u64,
    pub real_b: u64,
    pub platform_id: Pubkey,
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
}

impl DecodedLaunchLabPool {
    pub fn lifecycle_status(&self) -> LaunchLabPoolStatus {
        LaunchLabPoolStatus::from_raw(self.status)
    }

    pub fn curve_state(&self) -> CurvePoolState {
        CurvePoolState {
            total_sell_a: BigUint::from(self.total_sell_a),
            virtual_a: BigUint::from(self.virtual_a),
            virtual_b: BigUint::from(self.virtual_b),
            real_a: BigUint::from(self.real_a),
            real_b: BigUint::from(self.real_b),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedLaunchLabConfig {
    pub curve_type: u8,
    pub migrate_fee: u64,
    pub trade_fee_rate: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedPlatformConfig {
    pub fee_rate: u64,
    pub creator_fee_rate: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchLabPoolContext {
    pub pool_id: Pubkey,
    pub pool: DecodedLaunchLabPool,
    pub config: DecodedLaunchLabConfig,
    pub platform: DecodedPlatformConfig,
    pub quote_mint: Pubkey,
    pub token_program: Pubkey,
    pub quote_token_program: Pubkey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurvePoolState {
    pub total_sell_a: BigUint,
    pub virtual_a: BigUint,
    pub virtual_b: BigUint,
    pub real_a: BigUint,
    pub real_b: BigUint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveQuoteConfig {
    pub pool: CurvePoolState,
    pub curve_type: u8,
    pub trade_fee_rate: BigUint,
    pub platform_fee_rate: BigUint,
    pub creator_fee_rate: BigUint,
}

pub fn launchlab_program_id() -> Result<Pubkey, String> {
    parse_pubkey(LAUNCHLAB_PROGRAM_ID, "LaunchLab program id")
}

pub fn raydium_cpmm_program_id() -> Result<Pubkey, String> {
    parse_pubkey(RAYDIUM_CPMM_PROGRAM_ID, "Raydium CPMM program id")
}

pub fn raydium_amm_v4_program_id() -> Result<Pubkey, String> {
    parse_pubkey(RAYDIUM_AMM_V4_PROGRAM_ID, "Raydium AMM v4 program id")
}

pub fn wsol_mint() -> Result<Pubkey, String> {
    parse_pubkey(WSOL_MINT, "WSOL mint")
}

pub fn token_2022_program_id() -> Result<Pubkey, String> {
    parse_pubkey(TOKEN_2022_PROGRAM_ID, "Token-2022 program id")
}

pub fn canonical_pool_id(mint_a: &Pubkey, quote_mint: &Pubkey) -> Result<Pubkey, String> {
    let program = launchlab_program_id()?;
    Ok(Pubkey::find_program_address(&[b"pool", mint_a.as_ref(), quote_mint.as_ref()], &program).0)
}

pub fn launchpad_auth_pda() -> Result<Pubkey, String> {
    let program = launchlab_program_id()?;
    Ok(Pubkey::find_program_address(&[b"vault_auth_seed"], &program).0)
}

pub fn launchpad_cpi_event_pda() -> Result<Pubkey, String> {
    let program = launchlab_program_id()?;
    Ok(Pubkey::find_program_address(&[b"__event_authority"], &program).0)
}

pub fn launchpad_pool_vault_pda(pool_id: &Pubkey, mint: &Pubkey) -> Result<Pubkey, String> {
    let program = launchlab_program_id()?;
    Ok(Pubkey::find_program_address(&[b"pool_vault", pool_id.as_ref(), mint.as_ref()], &program).0)
}

pub fn platform_fee_vault_pda(platform_id: &Pubkey, mint: &Pubkey) -> Result<Pubkey, String> {
    let program = launchlab_program_id()?;
    Ok(Pubkey::find_program_address(&[platform_id.as_ref(), mint.as_ref()], &program).0)
}

pub fn creator_fee_vault_pda(creator: &Pubkey, mint: &Pubkey) -> Result<Pubkey, String> {
    let program = launchlab_program_id()?;
    Ok(Pubkey::find_program_address(&[creator.as_ref(), mint.as_ref()], &program).0)
}

pub fn decode_launchlab_config(data: &[u8]) -> Result<DecodedLaunchLabConfig, String> {
    let mut offset = 0usize;
    let _discriminator = read_u64(data, &mut offset)?;
    let _epoch = read_u64(data, &mut offset)?;
    let curve_type = read_u8(data, &mut offset)?;
    skip(data, &mut offset, 2)?;
    let migrate_fee = read_u64(data, &mut offset)?;
    let trade_fee_rate = read_u64(data, &mut offset)?;
    Ok(DecodedLaunchLabConfig {
        curve_type,
        migrate_fee,
        trade_fee_rate,
    })
}

pub fn decode_platform_config(data: &[u8]) -> Result<DecodedPlatformConfig, String> {
    let mut offset = 0usize;
    let _discriminator = read_u64(data, &mut offset)?;
    let _epoch = read_u64(data, &mut offset)?;
    let _platform_claim_fee_wallet = read_pubkey(data, &mut offset)?;
    let _platform_lock_nft_wallet = read_pubkey(data, &mut offset)?;
    let _platform_scale = read_u64(data, &mut offset)?;
    let _creator_scale = read_u64(data, &mut offset)?;
    let _burn_scale = read_u64(data, &mut offset)?;
    let fee_rate = read_u64(data, &mut offset)?;
    skip(data, &mut offset, 64 + 256 + 256)?;
    let _cp_config_id = read_pubkey(data, &mut offset)?;
    let creator_fee_rate = read_u64(data, &mut offset)?;
    Ok(DecodedPlatformConfig {
        fee_rate,
        creator_fee_rate,
    })
}

pub fn decode_launchlab_pool(data: &[u8]) -> Result<DecodedLaunchLabPool, String> {
    let mut offset = 0usize;
    let _discriminator = read_u64(data, &mut offset)?;
    let _epoch = read_u64(data, &mut offset)?;
    let _bump = read_u8(data, &mut offset)?;
    let status = read_u8(data, &mut offset)?;
    let _mint_decimals_a = read_u8(data, &mut offset)?;
    let _mint_decimals_b = read_u8(data, &mut offset)?;
    let migrate_type = read_u8(data, &mut offset)?;
    let supply = read_u64(data, &mut offset)?;
    let total_sell_a = read_u64(data, &mut offset)?;
    let virtual_a = read_u64(data, &mut offset)?;
    let virtual_b = read_u64(data, &mut offset)?;
    let real_a = read_u64(data, &mut offset)?;
    let real_b = read_u64(data, &mut offset)?;
    let _total_fund_raising_b = read_u64(data, &mut offset)?;
    let _protocol_fee = read_u64(data, &mut offset)?;
    let _platform_fee = read_u64(data, &mut offset)?;
    let _migrate_fee = read_u64(data, &mut offset)?;
    for _ in 0..5 {
        let _ = read_u64(data, &mut offset)?;
    }
    let config_id = read_pubkey(data, &mut offset)?;
    let platform_id = read_pubkey(data, &mut offset)?;
    let mint_a = read_pubkey(data, &mut offset)?;
    let mint_b = read_pubkey(data, &mut offset)?;
    let _vault_a = read_pubkey(data, &mut offset)?;
    let _vault_b = read_pubkey(data, &mut offset)?;
    let creator = read_pubkey(data, &mut offset)?;
    Ok(DecodedLaunchLabPool {
        creator,
        status,
        migrate_type,
        supply,
        config_id,
        total_sell_a,
        virtual_a,
        virtual_b,
        real_a,
        real_b,
        platform_id,
        mint_a,
        mint_b,
    })
}

pub fn quote_buy_exact_in_amount_a(
    config: &CurveQuoteConfig,
    amount_b: &BigUint,
) -> Result<BigUint, String> {
    let fee_rate = total_fee_rate(config)?;
    let total_fee = calculate_fee(amount_b, &fee_rate);
    let amount_less_fee_b = big_sub(amount_b, &total_fee, "buy input after fee")?;
    let quoted_amount_a = curve_buy_exact_in(&config.pool, config.curve_type, &amount_less_fee_b)?;
    let remaining_amount_a = big_sub(
        &config.pool.total_sell_a,
        &config.pool.real_a,
        "remaining sell amount",
    )?;
    Ok(quoted_amount_a.min(remaining_amount_a))
}

pub fn quote_buy_exact_out_amount_b(
    config: &CurveQuoteConfig,
    requested_amount_a: &BigUint,
) -> Result<BigUint, String> {
    let remaining_amount_a = big_sub(
        &config.pool.total_sell_a,
        &config.pool.real_a,
        "remaining sell amount",
    )?;
    if requested_amount_a > &remaining_amount_a {
        return Err(
            "Requested exact output exceeds remaining LaunchLab token liquidity.".to_string(),
        );
    }
    let real_amount_a = requested_amount_a.clone();
    let amount_in_less_fee_b =
        curve_buy_exact_out(&config.pool, config.curve_type, &real_amount_a)?;
    let fee_rate = total_fee_rate(config)?;
    calculate_pre_fee(&amount_in_less_fee_b, &fee_rate)
}

pub fn quote_sell_exact_in_amount_b(
    config: &CurveQuoteConfig,
    amount_a: &BigUint,
) -> Result<BigUint, String> {
    let quoted_amount_b = curve_sell_exact_in(&config.pool, config.curve_type, amount_a)?;
    let fee_rate = total_fee_rate(config)?;
    let total_fee = calculate_fee(&quoted_amount_b, &fee_rate);
    big_sub(&quoted_amount_b, &total_fee, "sell output after fee")
}

pub fn quote_sell_exact_out_amount_a(
    config: &CurveQuoteConfig,
    amount_b: &BigUint,
) -> Result<BigUint, String> {
    let fee_rate = total_fee_rate(config)?;
    let amount_out_with_fee_b = calculate_pre_fee(amount_b, &fee_rate)?;
    if config.pool.real_b < amount_out_with_fee_b {
        return Err("Insufficient liquidity".to_string());
    }
    let amount_a = curve_sell_exact_out(&config.pool, config.curve_type, &amount_out_with_fee_b)?;
    if amount_a > config.pool.real_a {
        return Err("Insufficient launch token liquidity".to_string());
    }
    Ok(amount_a)
}

pub fn build_min_amount_from_bps(amount: &BigUint, slippage_bps: u64) -> BigUint {
    let safe_bps = slippage_bps.min(10_000);
    let minimum = (amount * BigUint::from(10_000u64 - safe_bps)) / BigUint::from(10_000u64);
    if amount > &BigUint::ZERO && minimum == BigUint::ZERO {
        BigUint::from(1u8)
    } else {
        minimum
    }
}

pub fn build_buy_exact_in_instruction(
    owner: &Pubkey,
    context: &LaunchLabPoolContext,
    user_token_account_a: &Pubkey,
    user_token_account_b: &Pubkey,
    amount_b: u64,
    min_amount_a: u64,
) -> Result<Instruction, String> {
    let launchpad_program = launchlab_program_id()?;
    let auth = launchpad_auth_pda()?;
    let vault_a = launchpad_pool_vault_pda(&context.pool_id, &context.pool.mint_a)?;
    let vault_b = launchpad_pool_vault_pda(&context.pool_id, &context.quote_mint)?;
    let platform_claim_fee_vault =
        platform_fee_vault_pda(&context.pool.platform_id, &context.quote_mint)?;
    let creator_claim_fee_vault =
        creator_fee_vault_pda(&context.pool.creator, &context.quote_mint)?;
    let cpi_event = launchpad_cpi_event_pda()?;
    let mut data = Vec::with_capacity(32);
    data.extend_from_slice(&BUY_EXACT_IN_DISCRIMINATOR);
    data.extend_from_slice(&amount_b.to_le_bytes());
    data.extend_from_slice(&min_amount_a.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    Ok(Instruction {
        program_id: launchpad_program,
        accounts: vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(auth, false),
            AccountMeta::new_readonly(context.pool.config_id, false),
            AccountMeta::new_readonly(context.pool.platform_id, false),
            AccountMeta::new(context.pool_id, false),
            AccountMeta::new(*user_token_account_a, false),
            AccountMeta::new(*user_token_account_b, false),
            AccountMeta::new(vault_a, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(context.pool.mint_a, false),
            AccountMeta::new_readonly(context.quote_mint, false),
            AccountMeta::new_readonly(context.token_program, false),
            AccountMeta::new_readonly(context.quote_token_program, false),
            AccountMeta::new_readonly(cpi_event, false),
            AccountMeta::new_readonly(launchpad_program, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new(platform_claim_fee_vault, false),
            AccountMeta::new(creator_claim_fee_vault, false),
        ],
        data,
    })
}

pub fn build_sell_exact_in_instruction(
    owner: &Pubkey,
    context: &LaunchLabPoolContext,
    user_token_account_a: &Pubkey,
    user_token_account_b: &Pubkey,
    amount_a: u64,
    min_amount_b: u64,
) -> Result<Instruction, String> {
    let launchpad_program = launchlab_program_id()?;
    let auth = launchpad_auth_pda()?;
    let vault_a = launchpad_pool_vault_pda(&context.pool_id, &context.pool.mint_a)?;
    let vault_b = launchpad_pool_vault_pda(&context.pool_id, &context.quote_mint)?;
    let platform_claim_fee_vault =
        platform_fee_vault_pda(&context.pool.platform_id, &context.quote_mint)?;
    let creator_claim_fee_vault =
        creator_fee_vault_pda(&context.pool.creator, &context.quote_mint)?;
    let cpi_event = launchpad_cpi_event_pda()?;
    let mut data = Vec::with_capacity(32);
    data.extend_from_slice(&SELL_EXACT_IN_DISCRIMINATOR);
    data.extend_from_slice(&amount_a.to_le_bytes());
    data.extend_from_slice(&min_amount_b.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    Ok(Instruction {
        program_id: launchpad_program,
        accounts: vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(auth, false),
            AccountMeta::new_readonly(context.pool.config_id, false),
            AccountMeta::new_readonly(context.pool.platform_id, false),
            AccountMeta::new(context.pool_id, false),
            AccountMeta::new(*user_token_account_a, false),
            AccountMeta::new(*user_token_account_b, false),
            AccountMeta::new(vault_a, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(context.pool.mint_a, false),
            AccountMeta::new_readonly(context.quote_mint, false),
            AccountMeta::new_readonly(context.token_program, false),
            AccountMeta::new_readonly(context.quote_token_program, false),
            AccountMeta::new_readonly(cpi_event, false),
            AccountMeta::new_readonly(launchpad_program, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new(platform_claim_fee_vault, false),
            AccountMeta::new(creator_claim_fee_vault, false),
        ],
        data,
    })
}

pub fn build_cpmm_base_output_data(max_amount_in: u64, amount_out: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(24);
    data.extend_from_slice(&CPMM_SWAP_BASE_OUTPUT_DISCRIMINATOR);
    data.extend_from_slice(&max_amount_in.to_le_bytes());
    data.extend_from_slice(&amount_out.to_le_bytes());
    data
}

fn parse_pubkey(value: &str, label: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|error| format!("Invalid {label}: {error}"))
}

fn read_u8(data: &[u8], offset: &mut usize) -> Result<u8, String> {
    let byte = *data
        .get(*offset)
        .ok_or_else(|| "LaunchLab account data was shorter than expected.".to_string())?;
    *offset += 1;
    Ok(byte)
}

fn read_u64(data: &[u8], offset: &mut usize) -> Result<u64, String> {
    let bytes = data
        .get(*offset..offset.saturating_add(8))
        .ok_or_else(|| "LaunchLab account data was shorter than expected.".to_string())?;
    let mut raw = [0u8; 8];
    raw.copy_from_slice(bytes);
    *offset += 8;
    Ok(u64::from_le_bytes(raw))
}

fn read_pubkey(data: &[u8], offset: &mut usize) -> Result<Pubkey, String> {
    let bytes = data
        .get(*offset..offset.saturating_add(32))
        .ok_or_else(|| "LaunchLab account data was shorter than expected.".to_string())?;
    let mut raw = [0u8; 32];
    raw.copy_from_slice(bytes);
    *offset += 32;
    Ok(Pubkey::new_from_array(raw))
}

fn skip(data: &[u8], offset: &mut usize, len: usize) -> Result<(), String> {
    data.get(*offset..offset.saturating_add(len))
        .ok_or_else(|| "LaunchLab account data was shorter than expected.".to_string())?;
    *offset += len;
    Ok(())
}

fn q64() -> BigUint {
    BigUint::from(1u8) << 64usize
}

fn big_sub(left: &BigUint, right: &BigUint, label: &str) -> Result<BigUint, String> {
    if left < right {
        return Err(format!("LaunchLab {label} underflow."));
    }
    Ok(left - right)
}

fn ceil_div(amount_a: &BigUint, amount_b: &BigUint) -> BigUint {
    if amount_a == &BigUint::ZERO {
        BigUint::ZERO
    } else {
        (amount_a + amount_b - BigUint::from(1u8)) / amount_b
    }
}

fn sqrt_floor(value: &BigUint) -> BigUint {
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

fn sqrt_round(value: &BigUint) -> BigUint {
    let floor = sqrt_floor(value);
    let floor_squared = &floor * &floor;
    let remainder = value - &floor_squared;
    if remainder > floor {
        floor + BigUint::from(1u8)
    } else {
        floor
    }
}

fn total_fee_rate(config: &CurveQuoteConfig) -> Result<BigUint, String> {
    let total = &config.trade_fee_rate + &config.platform_fee_rate + &config.creator_fee_rate;
    if total > BigUint::from(FEE_RATE_DENOMINATOR) {
        return Err("total fee rate gt 1_000_000".to_string());
    }
    Ok(total)
}

fn calculate_fee(amount: &BigUint, fee_rate: &BigUint) -> BigUint {
    let numerator = amount * fee_rate;
    ceil_div(&numerator, &BigUint::from(FEE_RATE_DENOMINATOR))
}

fn calculate_pre_fee(post_fee_amount: &BigUint, fee_rate: &BigUint) -> Result<BigUint, String> {
    if fee_rate == &BigUint::ZERO {
        return Ok(post_fee_amount.clone());
    }
    let denominator = big_sub(
        &BigUint::from(FEE_RATE_DENOMINATOR),
        fee_rate,
        "fee denominator",
    )?;
    if denominator == BigUint::ZERO {
        return Err("LaunchLab fee denominator was zero.".to_string());
    }
    let numerator = post_fee_amount * BigUint::from(FEE_RATE_DENOMINATOR);
    Ok((numerator + &denominator - BigUint::from(1u8)) / denominator)
}

fn curve_buy_exact_in(
    pool: &CurvePoolState,
    curve_type: u8,
    amount: &BigUint,
) -> Result<BigUint, String> {
    match curve_type {
        0 => {
            let input_reserve = &pool.virtual_b + &pool.real_b;
            let output_reserve = big_sub(&pool.virtual_a, &pool.real_a, "launch output reserve")?;
            Ok((amount * output_reserve) / (input_reserve + amount))
        }
        1 => {
            if pool.virtual_b == BigUint::ZERO {
                return Err("LaunchLab fixed-price virtual quote reserve was zero.".to_string());
            }
            Ok((&pool.virtual_a * amount) / &pool.virtual_b)
        }
        2 => {
            if pool.virtual_a == BigUint::ZERO {
                return Err("LaunchLab linear-price virtual coefficient was zero.".to_string());
            }
            let new_quote = &pool.real_b + amount;
            let term_inside_sqrt = (BigUint::from(2u8) * new_quote * q64()) / &pool.virtual_a;
            let sqrt_term = sqrt_round(&term_inside_sqrt);
            big_sub(&sqrt_term, &pool.real_a, "linear-price amount out")
        }
        _ => Err("find curve error".to_string()),
    }
}

fn curve_buy_exact_out(
    pool: &CurvePoolState,
    curve_type: u8,
    amount: &BigUint,
) -> Result<BigUint, String> {
    match curve_type {
        0 => {
            let input_reserve = &pool.virtual_b + &pool.real_b;
            let output_reserve = big_sub(&pool.virtual_a, &pool.real_a, "launch output reserve")?;
            let denominator = big_sub(&output_reserve, amount, "launch remaining output reserve")?;
            if denominator == BigUint::ZERO {
                return Err(
                    "LaunchLab constant-product buyExactOut denominator was zero.".to_string(),
                );
            }
            Ok(ceil_div(&(input_reserve * amount), &denominator))
        }
        1 => {
            if pool.virtual_a == BigUint::ZERO {
                return Err("LaunchLab fixed-price virtual token reserve was zero.".to_string());
            }
            Ok(ceil_div(&(&pool.virtual_b * amount), &pool.virtual_a))
        }
        2 => {
            let new_base = &pool.real_a + amount;
            let new_base_squared = &new_base * &new_base;
            let denominator = BigUint::from(2u8) * q64();
            let new_quote = ceil_div(&(&pool.virtual_a * new_base_squared), &denominator);
            big_sub(&new_quote, &pool.real_b, "linear-price amount in")
        }
        _ => Err("find curve error".to_string()),
    }
}

fn curve_sell_exact_in(
    pool: &CurvePoolState,
    curve_type: u8,
    amount: &BigUint,
) -> Result<BigUint, String> {
    match curve_type {
        0 => {
            let input_reserve = big_sub(&pool.virtual_a, &pool.real_a, "launch input reserve")?;
            let output_reserve = &pool.virtual_b + &pool.real_b;
            Ok((amount * output_reserve) / (input_reserve + amount))
        }
        1 => {
            if pool.virtual_a == BigUint::ZERO {
                return Err("LaunchLab fixed-price virtual token reserve was zero.".to_string());
            }
            Ok((&pool.virtual_b * amount) / &pool.virtual_a)
        }
        2 => {
            let new_base = big_sub(&pool.real_a, amount, "linear-price new base")?;
            let new_base_squared = &new_base * &new_base;
            let denominator = BigUint::from(2u8) * q64();
            let new_quote = ceil_div(&(&pool.virtual_a * new_base_squared), &denominator);
            big_sub(&pool.real_b, &new_quote, "linear-price sell output")
        }
        _ => Err("find curve error".to_string()),
    }
}

fn curve_sell_exact_out(
    pool: &CurvePoolState,
    curve_type: u8,
    amount: &BigUint,
) -> Result<BigUint, String> {
    match curve_type {
        0 => {
            let input_reserve = big_sub(&pool.virtual_a, &pool.real_a, "launch input reserve")?;
            let output_reserve = &pool.virtual_b + &pool.real_b;
            let denominator = big_sub(&output_reserve, amount, "launch remaining output reserve")?;
            if denominator == BigUint::ZERO {
                return Err(
                    "LaunchLab constant-product sellExactOut denominator was zero.".to_string(),
                );
            }
            Ok(ceil_div(&(input_reserve * amount), &denominator))
        }
        1 => {
            if pool.virtual_b == BigUint::ZERO {
                return Err("LaunchLab fixed-price virtual quote reserve was zero.".to_string());
            }
            Ok(ceil_div(&(&pool.virtual_a * amount), &pool.virtual_b))
        }
        2 => {
            let new_quote = big_sub(&pool.real_b, amount, "linear-price new quote")?;
            if pool.virtual_a == BigUint::ZERO {
                return Err("LaunchLab linear-price virtual coefficient was zero.".to_string());
            }
            let term_inside_sqrt = (BigUint::from(2u8) * new_quote * q64()) / &pool.virtual_a;
            let sqrt_term = sqrt_round(&term_inside_sqrt);
            big_sub(&pool.real_a, &sqrt_term, "linear-price sell input")
        }
        _ => Err("find curve error".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_pool_derivation_is_stable() {
        let mint = Pubkey::new_unique();
        let quote = wsol_mint().expect("wsol");
        let first = canonical_pool_id(&mint, &quote).expect("pool");
        let second = Pubkey::find_program_address(
            &[b"pool", mint.as_ref(), quote.as_ref()],
            &launchlab_program_id().expect("program"),
        )
        .0;
        assert_eq!(first, second);
    }

    #[test]
    fn status_mapping_matches_raydium_docs() {
        assert_eq!(
            LaunchLabPoolStatus::from_raw(0),
            LaunchLabPoolStatus::Trading
        );
        assert_eq!(
            LaunchLabPoolStatus::from_raw(1),
            LaunchLabPoolStatus::Migrating
        );
        assert_eq!(
            LaunchLabPoolStatus::from_raw(2),
            LaunchLabPoolStatus::Migrated
        );
        assert_eq!(
            LaunchLabPoolStatus::from_raw(9),
            LaunchLabPoolStatus::Unknown(9)
        );
    }

    #[test]
    fn exact_in_builders_use_expected_discriminators_and_layouts() {
        let owner = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let quote = wsol_mint().expect("wsol");
        let pool_id = canonical_pool_id(&mint, &quote).expect("pool");
        let config_id = Pubkey::new_unique();
        let platform_id = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let context = LaunchLabPoolContext {
            pool_id,
            pool: DecodedLaunchLabPool {
                creator,
                status: 0,
                migrate_type: 0,
                supply: 1_000_000,
                config_id,
                total_sell_a: 900_000,
                virtual_a: 1_000_000,
                virtual_b: 100_000,
                real_a: 0,
                real_b: 0,
                platform_id,
                mint_a: mint,
                mint_b: quote,
            },
            config: DecodedLaunchLabConfig {
                curve_type: 0,
                migrate_fee: 0,
                trade_fee_rate: 2_500,
            },
            platform: DecodedPlatformConfig {
                fee_rate: 0,
                creator_fee_rate: 0,
            },
            quote_mint: quote,
            token_program: spl_token::id(),
            quote_token_program: spl_token::id(),
        };
        let user_a = Pubkey::new_unique();
        let user_b = Pubkey::new_unique();

        let buy = build_buy_exact_in_instruction(&owner, &context, &user_a, &user_b, 123, 45)
            .expect("buy");
        assert_eq!(buy.program_id, launchlab_program_id().expect("program"));
        assert_eq!(buy.accounts.len(), 18);
        assert_eq!(buy.data[0..8], BUY_EXACT_IN_DISCRIMINATOR);
        assert_eq!(u64::from_le_bytes(buy.data[8..16].try_into().unwrap()), 123);
        assert_eq!(u64::from_le_bytes(buy.data[16..24].try_into().unwrap()), 45);

        let sell = build_sell_exact_in_instruction(&owner, &context, &user_a, &user_b, 77, 66)
            .expect("sell");
        assert_eq!(sell.accounts.len(), 18);
        assert_eq!(sell.data[0..8], SELL_EXACT_IN_DISCRIMINATOR);
        assert_eq!(u64::from_le_bytes(sell.data[8..16].try_into().unwrap()), 77);
        assert_eq!(
            u64::from_le_bytes(sell.data[16..24].try_into().unwrap()),
            66
        );
    }

    #[test]
    fn exact_out_quotes_are_inverse_for_constant_product_curve() {
        let config = CurveQuoteConfig {
            pool: CurvePoolState {
                total_sell_a: BigUint::from(900_000u64),
                virtual_a: BigUint::from(1_000_000u64),
                virtual_b: BigUint::from(100_000u64),
                real_a: BigUint::from(0u64),
                real_b: BigUint::from(0u64),
            },
            curve_type: 0,
            trade_fee_rate: BigUint::from(2_500u64),
            platform_fee_rate: BigUint::from(1_000u64),
            creator_fee_rate: BigUint::from(500u64),
        };
        let wanted_tokens = BigUint::from(1_000u64);
        let required_quote =
            quote_buy_exact_out_amount_b(&config, &wanted_tokens).expect("buy out");
        let received_tokens =
            quote_buy_exact_in_amount_a(&config, &required_quote).expect("buy in");
        assert!(received_tokens >= wanted_tokens);

        let sell_config = CurveQuoteConfig {
            pool: CurvePoolState {
                total_sell_a: BigUint::from(900_000u64),
                virtual_a: BigUint::from(1_000_000u64),
                virtual_b: BigUint::from(100_000u64),
                real_a: BigUint::from(10_000u64),
                real_b: BigUint::from(1_000u64),
            },
            ..config
        };
        let wanted_quote = BigUint::from(500u64);
        let required_tokens =
            quote_sell_exact_out_amount_a(&sell_config, &wanted_quote).expect("sell out");
        let received_quote =
            quote_sell_exact_in_amount_b(&sell_config, &required_tokens).expect("sell in");
        assert!(received_quote >= wanted_quote);
    }

    #[test]
    fn cpmm_base_output_data_layout_is_pinned() {
        let data = build_cpmm_base_output_data(123, 45);
        assert_eq!(data[0..8], CPMM_SWAP_BASE_OUTPUT_DISCRIMINATOR);
        assert_eq!(u64::from_le_bytes(data[8..16].try_into().unwrap()), 123);
        assert_eq!(u64::from_le_bytes(data[16..24].try_into().unwrap()), 45);
    }
}
