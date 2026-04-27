use std::str::FromStr;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use num_traits::ToPrimitive;
use sha2::{Digest, Sha256};
use shared_transaction_submit::compiled_transaction_signers;
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_sdk::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{AddressLookupTableAccount, VersionedMessage, v0},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use solana_system_interface::instruction::create_account;
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use spl_token::instruction::{
    close_account as close_spl_account, initialize_account3, sync_native,
};

use crate::{
    bonk_execution_support::build_trusted_raydium_clmm_swap_exact_in,
    extension_api::{TradeSettlementAsset, TradeSide},
    rpc_client::{CompiledTransaction, configured_rpc_url, fetch_account_owner_and_data},
    trade_dispatch::{CompiledAdapterTrade, TransactionDependencyMode},
    trade_planner::{
        LifecycleAndCanonicalMarket, PlannerQuoteAsset, PlannerRuntimeBundle,
        PlannerVerificationSource, TradeLifecycle, TradeVenueFamily, TrustedStableRuntimeBundle,
        WrapperAction,
    },
    trade_runtime::TradeRuntimeRequest,
    wallet_store::load_solana_wallet_by_env_key,
    warming_service::shared_warming_service,
};

const SHARED_SUPER_LOOKUP_TABLE: &str = "7CaMLcAuSskoeN7HoRwZjsSthU8sMwKqxtXkyMiMjuc";
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const RAYDIUM_CLMM_PROGRAM_ID: &str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";
const ORCA_WHIRLPOOL_PROGRAM_ID: &str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const USDT_MINT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY8hd7YTPLW1m";
const USD1_MINT: &str = "USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB";
const SPL_TOKEN_ACCOUNT_LEN: u64 = 165;
const TEMP_WSOL_RENT_LAMPORTS: u64 = 2_039_280;
const STABLE_SWAP_COMPUTE_UNITS: u32 = 360_000;
const TRUSTED_STABLE_DEFAULT_SLIPPAGE_CAP_BPS: u64 = 100;
const TRUSTED_STABLE_MAX_SLIPPAGE_CAP_BPS: u64 = 500;
const ORCA_WHIRLPOOL_TICK_ARRAY_SIZE: i32 = 88;
const ORCA_MIN_SQRT_PRICE_X64: u128 = 4_295_048_016;
const ORCA_MAX_SQRT_PRICE_X64: u128 = 79_226_673_515_401_279_992_447_579_055;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustedStableVenue {
    RaydiumClmm,
    OrcaWhirlpool,
}

impl TrustedStableVenue {
    pub fn label(&self) -> &'static str {
        match self {
            Self::RaydiumClmm => "raydium-clmm",
            Self::OrcaWhirlpool => "orca-whirlpool",
        }
    }

    fn program_id(&self) -> &'static str {
        match self {
            Self::RaydiumClmm => RAYDIUM_CLMM_PROGRAM_ID,
            Self::OrcaWhirlpool => ORCA_WHIRLPOOL_PROGRAM_ID,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrustedStableRoute {
    pub label: &'static str,
    pub pool: &'static str,
    pub venue: TrustedStableVenue,
    pub buy_input_mint: &'static str,
    pub buy_output_mint: &'static str,
    pub sell_input_mint: &'static str,
    pub sell_output_mint: &'static str,
    pub buy_input_decimals: u8,
    pub buy_output_decimals: u8,
    pub sell_input_decimals: u8,
    pub sell_output_decimals: u8,
    pub quote_asset: PlannerQuoteAsset,
}

const TRUSTED_STABLE_ROUTES: &[TrustedStableRoute] = &[
    TrustedStableRoute {
        label: "orca-sol-usdc",
        pool: "Czfq3xZZDmsdGdUyrNLtRhGc47cXcZtLG4crryfu44zE",
        venue: TrustedStableVenue::OrcaWhirlpool,
        buy_input_mint: WSOL_MINT,
        buy_output_mint: USDC_MINT,
        sell_input_mint: USDC_MINT,
        sell_output_mint: WSOL_MINT,
        buy_input_decimals: 9,
        buy_output_decimals: 6,
        sell_input_decimals: 6,
        sell_output_decimals: 9,
        quote_asset: PlannerQuoteAsset::Wsol,
    },
    TrustedStableRoute {
        label: "raydium-wsol-usdc",
        pool: "3ucNos4NbumPLZNWztqGHNFFgkHeRMBQAVemeeomsUxv",
        venue: TrustedStableVenue::RaydiumClmm,
        buy_input_mint: WSOL_MINT,
        buy_output_mint: USDC_MINT,
        sell_input_mint: USDC_MINT,
        sell_output_mint: WSOL_MINT,
        buy_input_decimals: 9,
        buy_output_decimals: 6,
        sell_input_decimals: 6,
        sell_output_decimals: 9,
        quote_asset: PlannerQuoteAsset::Wsol,
    },
    TrustedStableRoute {
        label: "raydium-wsol-usd1",
        pool: "AQAGYQsdU853WAKhXM79CgNdoyhrRwXvYHX6qrDyC1FS",
        venue: TrustedStableVenue::RaydiumClmm,
        buy_input_mint: WSOL_MINT,
        buy_output_mint: USD1_MINT,
        sell_input_mint: USD1_MINT,
        sell_output_mint: WSOL_MINT,
        buy_input_decimals: 9,
        buy_output_decimals: 6,
        sell_input_decimals: 6,
        sell_output_decimals: 9,
        quote_asset: PlannerQuoteAsset::Wsol,
    },
    TrustedStableRoute {
        label: "raydium-usd1-usdc",
        pool: "BCDdHonby65iduz3Ev3c9v5XjNkzyu5e56KRFHpBM4T9",
        venue: TrustedStableVenue::RaydiumClmm,
        buy_input_mint: USDC_MINT,
        buy_output_mint: USD1_MINT,
        sell_input_mint: USD1_MINT,
        sell_output_mint: USDC_MINT,
        buy_input_decimals: 6,
        buy_output_decimals: 6,
        sell_input_decimals: 6,
        sell_output_decimals: 6,
        quote_asset: PlannerQuoteAsset::Usdc,
    },
    TrustedStableRoute {
        label: "raydium-wsol-usdt",
        pool: "3nMFwZXwY1s1M5s8vYAHqd4wGs4iSxXE4LRoUMMYqEgF",
        venue: TrustedStableVenue::RaydiumClmm,
        buy_input_mint: WSOL_MINT,
        buy_output_mint: USDT_MINT,
        sell_input_mint: USDT_MINT,
        sell_output_mint: WSOL_MINT,
        buy_input_decimals: 9,
        buy_output_decimals: 6,
        sell_input_decimals: 6,
        sell_output_decimals: 9,
        quote_asset: PlannerQuoteAsset::Wsol,
    },
    TrustedStableRoute {
        label: "raydium-usdc-usdt",
        pool: "BZtgQEyS6eXUXicYPHecYQ7PybqodXQMvkjUbP4R8mUU",
        venue: TrustedStableVenue::RaydiumClmm,
        buy_input_mint: USDT_MINT,
        buy_output_mint: USDC_MINT,
        sell_input_mint: USDC_MINT,
        sell_output_mint: USDT_MINT,
        buy_input_decimals: 6,
        buy_output_decimals: 6,
        sell_input_decimals: 6,
        sell_output_decimals: 6,
        quote_asset: PlannerQuoteAsset::Usdt,
    },
];

pub fn trusted_stable_route_for_pool(address: &str) -> Option<&'static TrustedStableRoute> {
    let normalized = address.trim();
    TRUSTED_STABLE_ROUTES
        .iter()
        .find(|route| route.pool == normalized)
}

pub fn trusted_stable_routes() -> &'static [TrustedStableRoute] {
    TRUSTED_STABLE_ROUTES
}

pub fn platform_allows_trusted_stable(platform: Option<&str>) -> bool {
    matches!(
        platform
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "axiom" | "axiom.trade"
    )
}

pub fn trusted_stable_route_descriptor(
    raw_address: &str,
    platform: Option<&str>,
) -> Option<crate::trade_dispatch::RouteDescriptor> {
    if !platform_allows_trusted_stable(platform) {
        return None;
    }
    let route = trusted_stable_route_for_pool(raw_address)?;
    Some(crate::trade_dispatch::RouteDescriptor {
        raw_address: raw_address.trim().to_string(),
        resolved_input_kind: crate::trade_dispatch::TradeInputKind::Pair,
        resolved_mint: route.buy_output_mint.to_string(),
        resolved_pair: Some(route.pool.to_string()),
        route_locked_pair: Some(route.pool.to_string()),
        family: Some(TradeVenueFamily::TrustedStableSwap),
        lifecycle: Some(TradeLifecycle::PostMigration),
        quote_asset: Some(route.quote_asset.clone()),
        canonical_market_key: Some(route.pool.to_string()),
        non_canonical: false,
    })
}

pub fn build_trusted_stable_selector(
    route: &TrustedStableRoute,
    side: TradeSide,
) -> LifecycleAndCanonicalMarket {
    LifecycleAndCanonicalMarket {
        lifecycle: TradeLifecycle::PostMigration,
        family: TradeVenueFamily::TrustedStableSwap,
        canonical_market_key: route.pool.to_string(),
        quote_asset: route.quote_asset.clone(),
        verification_source: PlannerVerificationSource::OnchainDerived,
        wrapper_action: match side {
            TradeSide::Buy => WrapperAction::TrustedStableSwapBuy,
            TradeSide::Sell => WrapperAction::TrustedStableSwapSell,
        },
        wrapper_accounts: vec![route.pool.to_string()],
        market_subtype: Some(route.venue.label().to_string()),
        direct_protocol_target: Some(route.venue.program_id().to_string()),
        input_amount_hint: None,
        minimum_output_hint: None,
        runtime_bundle: Some(PlannerRuntimeBundle::TrustedStable(
            TrustedStableRuntimeBundle {
                pool: route.pool.to_string(),
                venue: route.venue.label().to_string(),
                buy_input_mint: route.buy_input_mint.to_string(),
                buy_output_mint: route.buy_output_mint.to_string(),
                sell_input_mint: route.sell_input_mint.to_string(),
                sell_output_mint: route.sell_output_mint.to_string(),
            },
        )),
    }
}

pub async fn plan_trusted_stable_trade(
    request: &TradeRuntimeRequest,
) -> Result<crate::trade_dispatch::TradeDispatchPlan, String> {
    if !platform_allows_trusted_stable(request.platform_label.as_deref()) {
        return Err(
            "Trusted stable swaps are currently only enabled on Axiom pool pages.".to_string(),
        );
    }
    let route = trusted_stable_route_for_pool(&request.mint)
        .or_else(|| {
            request
                .pinned_pool
                .as_deref()
                .and_then(trusted_stable_route_for_pool)
        })
        .ok_or_else(|| "Requested address is not an approved trusted stable pool.".to_string())?;
    Ok(crate::trade_dispatch::TradeDispatchPlan {
        adapter: crate::trade_dispatch::TradeAdapter::StableNative,
        selector: build_trusted_stable_selector(route, request.side.clone()),
        execution_backend: crate::rollout::preferred_execution_backend(),
        raw_address: request.mint.clone(),
        resolved_input_kind: crate::trade_dispatch::TradeInputKind::Pair,
        resolved_mint: route.buy_output_mint.to_string(),
        resolved_pinned_pool: Some(route.pool.to_string()),
        non_canonical: false,
    })
}

pub async fn compile_trusted_stable_trade(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    wallet_key: &str,
) -> Result<CompiledAdapterTrade, String> {
    if selector.family != TradeVenueFamily::TrustedStableSwap {
        return Err("Stable compiler received a non-stable selector.".to_string());
    }
    let route = trusted_stable_route_for_pool(&selector.canonical_market_key)
        .ok_or_else(|| "Trusted stable selector referenced an unknown pool.".to_string())?;
    if !platform_allows_trusted_stable(request.platform_label.as_deref()) {
        return Err(
            "Trusted stable swaps are currently only enabled on Axiom pool pages.".to_string(),
        );
    }
    let owner_keypair = load_solana_wallet_by_env_key(wallet_key)?;
    let owner = owner_keypair.pubkey();
    let rpc_url = configured_rpc_url();
    let slippage_bps = trusted_stable_effective_slippage_bps(&request.policy.slippage_percent)?;
    let compute_price = priority_fee_sol_to_micro_lamports(&request.policy.fee_sol)?;
    let mut instructions = vec![
        compute_unit_limit_instruction(STABLE_SWAP_COMPUTE_UNITS)?,
        compute_unit_price_instruction(compute_price)?,
    ];
    let token_program = token_program_id()?;
    let (input_mint, output_mint, input_decimals, output_decimals, amount_in) = match request.side {
        TradeSide::Buy => {
            if route.buy_input_mint != WSOL_MINT {
                return Err(format!(
                    "Trusted stable route {} is direct-only and cannot use a SOL buy amount.",
                    route.label
                ));
            }
            let amount = request
                .buy_amount_sol
                .as_deref()
                .ok_or_else(|| "Trusted stable buy requires a buy amount.".to_string())
                .and_then(|value| {
                    parse_decimal_u64(value, route.buy_input_decimals, "stable buy amount")
                })?;
            (
                parse_pubkey(route.buy_input_mint, "stable buy input mint")?,
                parse_pubkey(route.buy_output_mint, "stable buy output mint")?,
                route.buy_input_decimals,
                route.buy_output_decimals,
                amount,
            )
        }
        TradeSide::Sell => {
            let sell_mint = route.sell_input_mint;
            let balance = crate::wallet_token_cache::fetch_token_balance_with_cache(
                Some(wallet_key),
                &owner.to_string(),
                sell_mint,
                route.sell_input_decimals,
            )
            .await?;
            if balance.amount_raw == 0 {
                return Err("You have 0 tokens.".to_string());
            }
            let percent = request
                .sell_intent
                .as_ref()
                .map(|intent| match intent {
                    crate::trade_runtime::RuntimeSellIntent::Percent(value) => value.as_str(),
                    crate::trade_runtime::RuntimeSellIntent::SolOutput(_) => "",
                })
                .filter(|value| !value.is_empty())
                .unwrap_or("100");
            let amount = percent_of_amount(balance.amount_raw, percent)?;
            (
                parse_pubkey(route.sell_input_mint, "stable sell input mint")?,
                parse_pubkey(route.sell_output_mint, "stable sell output mint")?,
                route.sell_input_decimals,
                route.sell_output_decimals,
                amount,
            )
        }
    };
    if amount_in == 0 {
        return Err("Trusted stable input amount resolved to zero.".to_string());
    }

    let mut extra_signers: Vec<Keypair> = Vec::new();
    let user_input_account = if input_mint.to_string() == WSOL_MINT {
        let temp = Keypair::new();
        instructions.extend(open_temp_wsol_instructions(
            &owner,
            &temp.pubkey(),
            amount_in.saturating_add(TEMP_WSOL_RENT_LAMPORTS),
            true,
        )?);
        let address = temp.pubkey();
        extra_signers.push(temp);
        address
    } else {
        get_associated_token_address_with_program_id(&owner, &input_mint, &token_program)
    };
    let user_output_account = if output_mint.to_string() == WSOL_MINT {
        let temp = Keypair::new();
        instructions.extend(open_temp_wsol_instructions(
            &owner,
            &temp.pubkey(),
            TEMP_WSOL_RENT_LAMPORTS,
            false,
        )?);
        let address = temp.pubkey();
        extra_signers.push(temp);
        address
    } else {
        let ata =
            get_associated_token_address_with_program_id(&owner, &output_mint, &token_program);
        instructions.push(create_associated_token_account_idempotent(
            &owner,
            &owner,
            &output_mint,
            &token_program,
        ));
        ata
    };

    let swap_instruction = match route.venue {
        TrustedStableVenue::RaydiumClmm => {
            build_trusted_raydium_clmm_swap_exact_in(
                &rpc_url,
                route.pool,
                &request.policy.commitment,
                &owner,
                &user_input_account,
                &user_output_account,
                &input_mint,
                &output_mint,
                amount_in,
                slippage_bps,
            )
            .await?
            .instruction
        }
        TrustedStableVenue::OrcaWhirlpool => {
            build_orca_whirlpool_swap_exact_in(
                &rpc_url,
                route,
                &request.policy.commitment,
                &owner,
                &user_input_account,
                &user_output_account,
                &input_mint,
                &output_mint,
                input_decimals,
                output_decimals,
                amount_in,
                slippage_bps,
            )
            .await?
        }
    };
    instructions.push(swap_instruction);
    if output_mint.to_string() == WSOL_MINT {
        instructions.push(close_temp_wsol_instruction(&owner, &user_output_account)?);
    }
    if input_mint.to_string() == WSOL_MINT {
        instructions.push(close_temp_wsol_instruction(&owner, &user_input_account)?);
    }

    let blockhash = shared_warming_service()
        .latest_blockhash(&rpc_url, &request.policy.commitment)
        .await?
        .blockhash;
    let lookup_tables = load_shared_lookup_tables(&rpc_url, &request.policy.commitment).await?;
    let compiled = compile_stable_transaction(
        route.label,
        blockhash,
        &owner_keypair,
        &extra_signers,
        &instructions,
        &lookup_tables,
        STABLE_SWAP_COMPUTE_UNITS,
        compute_price,
    )?;
    let entry_preference_asset = match request.side {
        TradeSide::Buy => stable_asset_for_mint(route.buy_output_mint),
        TradeSide::Sell => stable_asset_for_mint(route.sell_output_mint),
    };
    Ok(CompiledAdapterTrade {
        transactions: vec![compiled],
        primary_tx_index: 0,
        dependency_mode: TransactionDependencyMode::Independent,
        entry_preference_asset,
    })
}

pub fn trusted_stable_effective_slippage_bps(user_slippage_percent: &str) -> Result<u64, String> {
    let cap = std::env::var("TRUSTED_STABLE_SWAP_MAX_SLIPPAGE_BPS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(TRUSTED_STABLE_DEFAULT_SLIPPAGE_CAP_BPS)
        .min(TRUSTED_STABLE_MAX_SLIPPAGE_CAP_BPS);
    let user_bps = slippage_percent_to_bps(user_slippage_percent).unwrap_or(cap);
    Ok(user_bps.min(cap))
}

fn stable_asset_for_mint(mint: &str) -> Option<TradeSettlementAsset> {
    match mint {
        WSOL_MINT => Some(TradeSettlementAsset::Sol),
        USD1_MINT => Some(TradeSettlementAsset::Usd1),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct DecodedWhirlpool {
    tick_spacing: u16,
    liquidity: u128,
    sqrt_price_x64: u128,
    tick_current_index: i32,
    mint_a: Pubkey,
    vault_a: Pubkey,
    mint_b: Pubkey,
    vault_b: Pubkey,
}

async fn build_orca_whirlpool_swap_exact_in(
    rpc_url: &str,
    route: &TrustedStableRoute,
    commitment: &str,
    owner: &Pubkey,
    user_input_account: &Pubkey,
    user_output_account: &Pubkey,
    input_mint: &Pubkey,
    output_mint: &Pubkey,
    input_decimals: u8,
    output_decimals: u8,
    amount_in: u64,
    slippage_bps: u64,
) -> Result<Instruction, String> {
    let pool_id = parse_pubkey(route.pool, "trusted Orca pool")?;
    let (owner_pubkey, pool_data) = fetch_account_owner_and_data(rpc_url, route.pool, commitment)
        .await?
        .ok_or_else(|| format!("Trusted Orca pool {} was not found.", route.pool))?;
    if owner_pubkey != orca_whirlpool_program_id()? {
        return Err(format!(
            "Trusted Orca pool {} owner mismatch: {owner_pubkey}",
            route.pool
        ));
    }
    let pool = decode_orca_whirlpool(&pool_data)?;
    if pool.liquidity == 0 {
        return Err("Trusted Orca pool liquidity is zero.".to_string());
    }
    if !((*input_mint == pool.mint_a && *output_mint == pool.mint_b)
        || (*input_mint == pool.mint_b && *output_mint == pool.mint_a))
    {
        return Err(format!(
            "Trusted Orca pool {} mint pair mismatch.",
            route.pool
        ));
    }
    let a_to_b = *input_mint == pool.mint_a;
    let min_out = quote_orca_spot_min_out(
        &pool,
        a_to_b,
        amount_in,
        input_decimals,
        output_decimals,
        slippage_bps,
    )?;
    if min_out == 0 {
        return Err("Trusted Orca quote returned zero output.".to_string());
    }
    let tick_arrays = derive_orca_tick_arrays(&pool_id, &pool, a_to_b)?;
    let oracle = derive_orca_pda(&[b"oracle", pool_id.as_ref()])?;
    let sqrt_price_limit = if a_to_b {
        ORCA_MIN_SQRT_PRICE_X64
    } else {
        ORCA_MAX_SQRT_PRICE_X64
    };
    let mut data = Vec::with_capacity(42);
    data.extend_from_slice(&anchor_discriminator("global:swap"));
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_out.to_le_bytes());
    data.extend_from_slice(&sqrt_price_limit.to_le_bytes());
    data.push(1);
    data.push(u8::from(a_to_b));
    let (owner_account_a, owner_account_b) = if a_to_b {
        (*user_input_account, *user_output_account)
    } else {
        (*user_output_account, *user_input_account)
    };
    Ok(Instruction {
        program_id: orca_whirlpool_program_id()?,
        accounts: vec![
            AccountMeta::new_readonly(token_program_id()?, false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(pool_id, false),
            AccountMeta::new(owner_account_a, false),
            AccountMeta::new(pool.vault_a, false),
            AccountMeta::new(owner_account_b, false),
            AccountMeta::new(pool.vault_b, false),
            AccountMeta::new(tick_arrays[0], false),
            AccountMeta::new(tick_arrays[1], false),
            AccountMeta::new(tick_arrays[2], false),
            AccountMeta::new_readonly(oracle, false),
        ],
        data,
    })
}

fn quote_orca_spot_min_out(
    pool: &DecodedWhirlpool,
    a_to_b: bool,
    amount_in: u64,
    input_decimals: u8,
    output_decimals: u8,
    slippage_bps: u64,
) -> Result<u64, String> {
    let sqrt = pool
        .sqrt_price_x64
        .to_f64()
        .ok_or_else(|| "Trusted Orca sqrt price overflowed f64.".to_string())?
        / 18_446_744_073_709_551_616.0;
    let price_b_per_a = sqrt * sqrt;
    let input_human = amount_in as f64 / 10f64.powi(i32::from(input_decimals));
    let output_human = if a_to_b {
        input_human
            * price_b_per_a
            * 10f64.powi(i32::from(input_decimals) - i32::from(output_decimals))
    } else {
        input_human / price_b_per_a
            * 10f64.powi(i32::from(input_decimals) - i32::from(output_decimals))
    };
    let gross = output_human * 10f64.powi(i32::from(output_decimals));
    let min = gross * (10_000u64.saturating_sub(slippage_bps) as f64 / 10_000.0);
    if !min.is_finite() || min <= 0.0 {
        return Err("Trusted Orca spot quote was invalid.".to_string());
    }
    Ok(min.floor() as u64)
}

fn decode_orca_whirlpool(data: &[u8]) -> Result<DecodedWhirlpool, String> {
    let mut offset = 8usize;
    offset += 32; // whirlpools_config
    offset += 1; // bump
    let tick_spacing = read_u16(data, &mut offset)?;
    offset += 2; // tick_spacing_seed
    offset += 2; // fee_rate
    offset += 2; // protocol_fee_rate
    let liquidity = read_u128(data, &mut offset)?;
    let sqrt_price_x64 = read_u128(data, &mut offset)?;
    let tick_current_index = read_i32(data, &mut offset)?;
    offset += 8 + 8; // protocol fees owed
    let mint_a = read_pubkey(data, &mut offset)?;
    let vault_a = read_pubkey(data, &mut offset)?;
    offset += 16; // fee growth global A
    let mint_b = read_pubkey(data, &mut offset)?;
    let vault_b = read_pubkey(data, &mut offset)?;
    Ok(DecodedWhirlpool {
        tick_spacing,
        liquidity,
        sqrt_price_x64,
        tick_current_index,
        mint_a,
        vault_a,
        mint_b,
        vault_b,
    })
}

fn derive_orca_tick_arrays(
    pool_id: &Pubkey,
    pool: &DecodedWhirlpool,
    a_to_b: bool,
) -> Result<[Pubkey; 3], String> {
    let tick_count = i32::from(pool.tick_spacing) * ORCA_WHIRLPOOL_TICK_ARRAY_SIZE;
    let current_start = pool.tick_current_index.div_euclid(tick_count) * tick_count;
    let starts = if a_to_b {
        [
            current_start,
            current_start.saturating_sub(tick_count),
            current_start.saturating_sub(tick_count.saturating_mul(2)),
        ]
    } else {
        [
            current_start,
            current_start.saturating_add(tick_count),
            current_start.saturating_add(tick_count.saturating_mul(2)),
        ]
    };
    Ok([
        derive_orca_tick_array(pool_id, starts[0])?,
        derive_orca_tick_array(pool_id, starts[1])?,
        derive_orca_tick_array(pool_id, starts[2])?,
    ])
}

fn derive_orca_tick_array(pool_id: &Pubkey, start_tick_index: i32) -> Result<Pubkey, String> {
    derive_orca_pda(&[
        b"tick_array",
        pool_id.as_ref(),
        start_tick_index.to_string().as_bytes(),
    ])
}

fn derive_orca_pda(seeds: &[&[u8]]) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(seeds, &orca_whirlpool_program_id()?).0)
}

fn anchor_discriminator(name: &str) -> [u8; 8] {
    let digest = Sha256::digest(name.as_bytes());
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest[..8]);
    out
}

fn compile_stable_transaction(
    label: &str,
    blockhash: Hash,
    payer: &Keypair,
    extra_signers: &[Keypair],
    instructions: &[Instruction],
    lookup_tables: &[AddressLookupTableAccount],
    compute_unit_limit: u32,
    compute_unit_price_micro_lamports: u64,
) -> Result<CompiledTransaction, String> {
    let message = v0::Message::try_compile(&payer.pubkey(), instructions, lookup_tables, blockhash)
        .map_err(|error| format!("Failed to compile trusted stable v0 message: {error}"))?;
    let lookup_tables_used = message
        .address_table_lookups
        .iter()
        .map(|lookup| lookup.account_key.to_string())
        .collect::<Vec<_>>();
    if !lookup_tables_used
        .iter()
        .any(|table| table == SHARED_SUPER_LOOKUP_TABLE)
    {
        return Err(format!(
            "Trusted stable v0 compilation must use shared ALT {SHARED_SUPER_LOOKUP_TABLE}; used [{}].",
            lookup_tables_used.join(", ")
        ));
    }
    let mut signers: Vec<&Keypair> = Vec::with_capacity(1 + extra_signers.len());
    signers.push(payer);
    signers.extend(extra_signers.iter());
    let transaction = VersionedTransaction::try_new(VersionedMessage::V0(message), &signers)
        .map_err(|error| format!("Failed to sign trusted stable v0 transaction: {error}"))?;
    let signature = transaction
        .signatures
        .first()
        .map(|value| value.to_string());
    let serialized = bincode::serialize(&transaction)
        .map_err(|error| format!("Failed to serialize trusted stable v0 transaction: {error}"))?;
    let serialized_base64 = BASE64.encode(serialized);
    let extra_refs = extra_signers.iter().collect::<Vec<_>>();
    compiled_transaction_signers::remember_compiled_transaction_signers(
        &serialized_base64,
        &extra_refs,
    );
    Ok(CompiledTransaction {
        label: format!("trusted-stable-{label}"),
        format: "v0-alt".to_string(),
        serialized_base64,
        signature,
        lookup_tables_used,
        compute_unit_limit: Some(u64::from(compute_unit_limit)),
        compute_unit_price_micro_lamports: if compute_unit_price_micro_lamports > 0 {
            Some(compute_unit_price_micro_lamports)
        } else {
            None
        },
        inline_tip_lamports: None,
        inline_tip_account: None,
    })
}

async fn load_shared_lookup_tables(
    rpc_url: &str,
    commitment: &str,
) -> Result<Vec<AddressLookupTableAccount>, String> {
    let (_owner, data) =
        fetch_account_owner_and_data(rpc_url, SHARED_SUPER_LOOKUP_TABLE, commitment)
            .await?
            .ok_or_else(|| format!("Shared ALT {SHARED_SUPER_LOOKUP_TABLE} was not found."))?;
    let table = AddressLookupTable::deserialize(&data)
        .map_err(|error| format!("Failed to decode shared ALT: {error}"))?;
    Ok(vec![AddressLookupTableAccount {
        key: parse_pubkey(SHARED_SUPER_LOOKUP_TABLE, "shared ALT")?,
        addresses: table.addresses.to_vec(),
    }])
}

fn open_temp_wsol_instructions(
    owner: &Pubkey,
    wrapped_account: &Pubkey,
    lamports: u64,
    sync_after_initialize: bool,
) -> Result<Vec<Instruction>, String> {
    let token_program = token_program_id()?;
    let mut out = vec![
        create_account(
            owner,
            wrapped_account,
            lamports,
            SPL_TOKEN_ACCOUNT_LEN,
            &token_program,
        ),
        initialize_account3(&token_program, wrapped_account, &wsol_mint()?, owner)
            .map_err(|error| format!("Failed to initialize stable WSOL account: {error}"))?,
    ];
    if sync_after_initialize {
        out.push(
            sync_native(&token_program, wrapped_account)
                .map_err(|error| format!("Failed to sync stable WSOL account: {error}"))?,
        );
    }
    Ok(out)
}

fn close_temp_wsol_instruction(
    owner: &Pubkey,
    wrapped_account: &Pubkey,
) -> Result<Instruction, String> {
    close_spl_account(&token_program_id()?, wrapped_account, owner, owner, &[])
        .map_err(|error| format!("Failed to close stable WSOL account: {error}"))
}

fn compute_unit_limit_instruction(compute_unit_limit: u32) -> Result<Instruction, String> {
    let mut data = vec![2];
    data.extend_from_slice(&compute_unit_limit.to_le_bytes());
    Ok(Instruction {
        program_id: compute_budget_program_id()?,
        accounts: vec![],
        data,
    })
}

fn compute_unit_price_instruction(micro_lamports: u64) -> Result<Instruction, String> {
    let mut data = vec![3];
    data.extend_from_slice(&micro_lamports.to_le_bytes());
    Ok(Instruction {
        program_id: compute_budget_program_id()?,
        accounts: vec![],
        data,
    })
}

fn percent_of_amount(amount: u64, percent: &str) -> Result<u64, String> {
    let percent_raw = parse_decimal_u64(percent, 2, "sell percent")?;
    let numerator = u128::from(amount) * u128::from(percent_raw);
    let value = numerator / 10_000;
    u64::try_from(value).map_err(|error| format!("Stable sell amount overflowed: {error}"))
}

fn slippage_percent_to_bps(value: &str) -> Result<u64, String> {
    parse_decimal_u64(value, 2, "slippage percent")
}

fn priority_fee_sol_to_micro_lamports(priority_fee_sol: &str) -> Result<u64, String> {
    let lamports = parse_decimal_u64(priority_fee_sol, 9, "feeSol")?;
    Ok(lamports)
}

fn parse_decimal_u64(value: &str, decimals: u8, label: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} is empty."));
    }
    if trimmed.starts_with('-') {
        return Err(format!("{label} cannot be negative."));
    }
    let mut parts = trimmed.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        return Err(format!("{label} has too many decimal points."));
    }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("{label} must be numeric."));
    }
    let scale = 10u128.pow(u32::from(decimals));
    let whole_value = if whole.is_empty() {
        0u128
    } else {
        whole
            .parse::<u128>()
            .map_err(|error| format!("Invalid {label}: {error}"))?
    };
    let mut fraction_string = fraction.to_string();
    if fraction_string.len() > usize::from(decimals) {
        fraction_string.truncate(usize::from(decimals));
    }
    while fraction_string.len() < usize::from(decimals) {
        fraction_string.push('0');
    }
    let fraction_value = if fraction_string.is_empty() {
        0u128
    } else {
        fraction_string
            .parse::<u128>()
            .map_err(|error| format!("Invalid {label}: {error}"))?
    };
    let value = whole_value
        .checked_mul(scale)
        .and_then(|whole| whole.checked_add(fraction_value))
        .ok_or_else(|| format!("{label} overflowed."))?;
    u64::try_from(value).map_err(|error| format!("{label} overflowed u64: {error}"))
}

fn read_u16(data: &[u8], offset: &mut usize) -> Result<u16, String> {
    let bytes = data
        .get(*offset..(*offset + 2))
        .ok_or_else(|| "Trusted Orca account was too short.".to_string())?;
    *offset += 2;
    Ok(u16::from_le_bytes(bytes.try_into().map_err(|_| {
        "Trusted Orca account returned invalid u16.".to_string()
    })?))
}

fn read_i32(data: &[u8], offset: &mut usize) -> Result<i32, String> {
    let bytes = data
        .get(*offset..(*offset + 4))
        .ok_or_else(|| "Trusted Orca account was too short.".to_string())?;
    *offset += 4;
    Ok(i32::from_le_bytes(bytes.try_into().map_err(|_| {
        "Trusted Orca account returned invalid i32.".to_string()
    })?))
}

fn read_u128(data: &[u8], offset: &mut usize) -> Result<u128, String> {
    let bytes = data
        .get(*offset..(*offset + 16))
        .ok_or_else(|| "Trusted Orca account was too short.".to_string())?;
    *offset += 16;
    Ok(u128::from_le_bytes(bytes.try_into().map_err(|_| {
        "Trusted Orca account returned invalid u128.".to_string()
    })?))
}

fn read_pubkey(data: &[u8], offset: &mut usize) -> Result<Pubkey, String> {
    let bytes = data
        .get(*offset..(*offset + 32))
        .ok_or_else(|| "Trusted Orca account was too short.".to_string())?;
    *offset += 32;
    Ok(Pubkey::new_from_array(bytes.try_into().map_err(|_| {
        "Trusted Orca account returned invalid pubkey.".to_string()
    })?))
}

fn parse_pubkey(value: &str, label: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|error| format!("Invalid {label}: {error}"))
}

fn compute_budget_program_id() -> Result<Pubkey, String> {
    parse_pubkey(COMPUTE_BUDGET_PROGRAM_ID, "Compute Budget program id")
}

fn token_program_id() -> Result<Pubkey, String> {
    parse_pubkey(TOKEN_PROGRAM_ID, "SPL Token program id")
}

fn wsol_mint() -> Result<Pubkey, String> {
    parse_pubkey(WSOL_MINT, "WSOL mint")
}

fn orca_whirlpool_program_id() -> Result<Pubkey, String> {
    parse_pubkey(ORCA_WHIRLPOOL_PROGRAM_ID, "Orca Whirlpool program id")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_route_table_contains_exact_pool_ids() {
        assert!(
            trusted_stable_route_for_pool("Czfq3xZZDmsdGdUyrNLtRhGc47cXcZtLG4crryfu44zE").is_some()
        );
        assert!(
            trusted_stable_route_for_pool("3ucNos4NbumPLZNWztqGHNFFgkHeRMBQAVemeeomsUxv").is_some()
        );
        assert!(trusted_stable_route_for_pool(USDC_MINT).is_none());
    }

    #[test]
    fn stable_slippage_cap_clamps_user_preset() {
        unsafe {
            std::env::set_var("TRUSTED_STABLE_SWAP_MAX_SLIPPAGE_BPS", "100");
        }
        assert_eq!(trusted_stable_effective_slippage_bps("0.50").unwrap(), 50);
        assert_eq!(trusted_stable_effective_slippage_bps("0.25").unwrap(), 25);
        assert_eq!(trusted_stable_effective_slippage_bps("0.10").unwrap(), 10);
        assert_eq!(trusted_stable_effective_slippage_bps("1").unwrap(), 100);
        assert_eq!(trusted_stable_effective_slippage_bps("5").unwrap(), 100);
        assert_eq!(trusted_stable_effective_slippage_bps("50.0").unwrap(), 100);
        unsafe {
            std::env::set_var("TRUSTED_STABLE_SWAP_MAX_SLIPPAGE_BPS", "10000");
        }
        assert_eq!(trusted_stable_effective_slippage_bps("50").unwrap(), 500);
    }

    #[test]
    fn stable_activation_is_axiom_only() {
        assert!(platform_allows_trusted_stable(Some("axiom")));
        assert!(!platform_allows_trusted_stable(Some("j7")));
        assert!(
            trusted_stable_route_descriptor(
                "AQAGYQsdU853WAKhXM79CgNdoyhrRwXvYHX6qrDyC1FS",
                Some("axiom"),
            )
            .is_some()
        );
        assert!(
            trusted_stable_route_descriptor(
                "AQAGYQsdU853WAKhXM79CgNdoyhrRwXvYHX6qrDyC1FS",
                Some("j7"),
            )
            .is_none()
        );
    }

    #[test]
    fn stable_selector_is_pool_specific_and_marks_venue() {
        let route = trusted_stable_route_for_pool("Czfq3xZZDmsdGdUyrNLtRhGc47cXcZtLG4crryfu44zE")
            .expect("orca stable route");
        let selector = build_trusted_stable_selector(route, TradeSide::Buy);
        assert_eq!(selector.family, TradeVenueFamily::TrustedStableSwap);
        assert_eq!(selector.canonical_market_key, route.pool);
        assert_eq!(selector.market_subtype.as_deref(), Some("orca-whirlpool"));
        assert_eq!(
            selector.direct_protocol_target.as_deref(),
            Some(ORCA_WHIRLPOOL_PROGRAM_ID)
        );
        assert_eq!(selector.wrapper_action, WrapperAction::TrustedStableSwapBuy);
    }

    #[test]
    fn orca_spot_quote_applies_decimals_and_stable_slippage() {
        let sqrt_price_x64 = (0.1f64.sqrt() * 18_446_744_073_709_551_616.0) as u128;
        let pool = DecodedWhirlpool {
            tick_spacing: 4,
            liquidity: 1,
            sqrt_price_x64,
            tick_current_index: 0,
            mint_a: Pubkey::new_unique(),
            vault_a: Pubkey::new_unique(),
            mint_b: Pubkey::new_unique(),
            vault_b: Pubkey::new_unique(),
        };
        let min_out =
            quote_orca_spot_min_out(&pool, true, 1_000_000_000, 9, 6, 100).expect("quote");
        assert_eq!(min_out, 99_000_000);
    }
}
