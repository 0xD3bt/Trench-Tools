use std::str::FromStr;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use num_bigint::BigUint;
use shared_raydium_launchlab::{
    CurveQuoteConfig, DecodedLaunchLabPool, LaunchLabPoolContext, LaunchLabPoolStatus,
    build_buy_exact_in_instruction, build_min_amount_from_bps, build_sell_exact_in_instruction,
    canonical_pool_id, decode_launchlab_config, decode_launchlab_pool, decode_platform_config,
    launchlab_program_id, quote_buy_exact_in_amount_a, quote_sell_exact_in_amount_b, wsol_mint,
};
use shared_transaction_submit::compiled_transaction_signers;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::{VersionedMessage, v0},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use solana_system_interface::instruction::{create_account, transfer};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use spl_token::instruction::{close_account as close_spl_account, initialize_account3};

use crate::{
    extension_api::{TradeSettlementAsset, TradeSide},
    provider_tip::pick_tip_account_for_provider,
    raydium_amm_v4_native::{
        find_raydium_amm_v4_pool_for_pair, plan_raydium_amm_v4_trade_for_pool_id,
    },
    raydium_cpmm_native::{find_raydium_cpmm_pool_for_pair, plan_raydium_cpmm_trade_for_pool_id},
    rpc_client::{
        CompiledTransaction, fetch_account_data, fetch_account_owner_and_data,
        fetch_multiple_account_owner_and_data,
    },
    trade_dispatch::{CompiledAdapterTrade, TransactionDependencyMode},
    trade_planner::{
        LifecycleAndCanonicalMarket, PlannerQuoteAsset, PlannerVerificationSource, TradeLifecycle,
        TradeVenueFamily, WrapperAction,
    },
    trade_runtime::{RuntimeSellIntent, TradeRuntimeRequest},
    wallet_store::load_solana_wallet_by_env_key,
};

const BONK_LETSBONK_PLATFORM_ID: &str = "FfYek5vEz23cMkWsdJwG2oa6EphsvXSHrGpdALN4g6W1";
const BONK_BONKERS_PLATFORM_ID: &str = "82NMHVCKwehXgbXMyzL41mvv3sdkypaMCtTxvJ4CtTzm";
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
const JITODONTFRONT_ACCOUNT: &str = "jitodontfront111111111111111111111111111111";
const SPL_TOKEN_ACCOUNT_LEN: u64 = 165;
const LAUNCHLAB_BUY_COMPUTE_UNIT_LIMIT: u32 = 280_000;
const LAUNCHLAB_SELL_COMPUTE_UNIT_LIMIT: u32 = 280_000;
const PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT: u64 = 1_000_000;
const HELIUS_SENDER_MIN_TIP_LAMPORTS: u64 = 200_000;
const HELLO_MOON_MIN_TIP_LAMPORTS: u64 = 1_000_000;
const LAUNCHLAB_MIGRATE_TO_AMM_V4: u8 = 0;
const LAUNCHLAB_MIGRATE_TO_CPMM: u8 = 1;

#[derive(Debug, Clone)]
pub(crate) struct RaydiumLaunchLabClassification {
    pub mint: String,
    pub pool_id: String,
    pub status: LaunchLabPoolStatus,
}

pub(crate) fn raydium_launchlab_program_id() -> Result<Pubkey, String> {
    launchlab_program_id()
}

pub(crate) fn classify_raydium_launchlab_pool_address(
    address: &str,
    owner: &Pubkey,
    data: &[u8],
) -> Result<Option<RaydiumLaunchLabClassification>, String> {
    if *owner != launchlab_program_id()? {
        return Ok(None);
    }
    let pool = match decode_launchlab_pool(data) {
        Ok(pool) => pool,
        Err(_) => return Ok(None),
    };
    if is_bonk_platform(&pool.platform_id) {
        return Ok(None);
    }
    let Ok(quote_mint) = wsol_mint() else {
        return Ok(None);
    };
    if pool.mint_b != quote_mint {
        return Ok(None);
    }
    let address_pubkey = match Pubkey::from_str(address.trim()) {
        Ok(pubkey) => pubkey,
        Err(_) => return Ok(None),
    };
    if canonical_pool_id(&pool.mint_a, &quote_mint)? != address_pubkey {
        return Ok(None);
    }
    Ok(Some(RaydiumLaunchLabClassification {
        mint: pool.mint_a.to_string(),
        pool_id: address.trim().to_string(),
        status: pool.lifecycle_status(),
    }))
}

pub(crate) async fn plan_raydium_launchlab_trade(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    validate_launchlab_policy_for_side(request)?;

    let mint = parse_pubkey(&request.mint, "Raydium LaunchLab mint")?;
    let quote_mint = wsol_mint()?;
    let canonical_pool = canonical_pool_id(&mint, &quote_mint)?;
    if let Some(pinned_pool) = request
        .pinned_pool
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && pinned_pool != canonical_pool.to_string()
    {
        return Err(format!(
            "Selected pair {pinned_pool} is not the canonical Raydium LaunchLab pool {} for mint {}.",
            canonical_pool, request.mint
        ));
    }
    let (owner, pool_data) = match fetch_account_owner_and_data(
        rpc_url,
        &canonical_pool.to_string(),
        &request.policy.commitment,
    )
    .await?
    {
        Some(account) => account,
        None => return Ok(None),
    };
    if owner != launchlab_program_id()? {
        return Ok(None);
    }
    let pool = decode_launchlab_pool(&pool_data)?;
    validate_launchlab_pool_for_request(&pool, &mint, &canonical_pool)?;
    if is_bonk_platform(&pool.platform_id) {
        return Err(
            "Raydium LaunchLab route resolved to a LetsBonk/Bonkers platform pool; use the Bonk route."
                .to_string(),
        );
    }
    selector_for_pool(rpc_url, request, canonical_pool, &pool).await
}

fn validate_launchlab_policy_for_side(request: &TradeRuntimeRequest) -> Result<(), String> {
    match request.side {
        TradeSide::Buy => {
            if !matches!(
                request.policy.buy_funding_policy,
                crate::extension_api::BuyFundingPolicy::SolOnly
            ) {
                return Err(
                    "Raydium LaunchLab routing currently supports SOL-funded buys only."
                        .to_string(),
                );
            }
        }
        TradeSide::Sell => {
            if !matches!(
                request.policy.sell_settlement_asset,
                TradeSettlementAsset::Sol
            ) {
                return Err(
                    "Raydium LaunchLab routing currently supports SOL settlement only.".to_string(),
                );
            }
        }
    }
    Ok(())
}

pub(crate) async fn compile_raydium_launchlab_trade(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    wallet_key: &str,
) -> Result<CompiledAdapterTrade, String> {
    if !matches!(selector.family, TradeVenueFamily::RaydiumLaunchLab) {
        return Err(format!(
            "Raydium LaunchLab compiler received unsupported family {}.",
            selector.family.label()
        ));
    }
    let rpc_url = crate::rpc_client::configured_rpc_url();
    let owner = load_solana_wallet_by_env_key(wallet_key)?;
    let context = load_live_launchlab_context(&rpc_url, selector, request).await?;

    match request.side {
        TradeSide::Buy => {
            let buy_amount_sol = request
                .buy_amount_sol
                .as_deref()
                .ok_or_else(|| "Missing buyAmountSol for Raydium LaunchLab buy.".to_string())?;
            let transaction =
                compile_buy_transaction(&rpc_url, request, &owner, &context, buy_amount_sol)
                    .await?;
            Ok(CompiledAdapterTrade {
                transactions: vec![transaction],
                primary_tx_index: 0,
                dependency_mode: TransactionDependencyMode::Independent,
                entry_preference_asset: Some(TradeSettlementAsset::Sol),
            })
        }
        TradeSide::Sell => {
            let compiled = compile_sell_transaction(&rpc_url, request, &owner, &context).await?;
            Ok(CompiledAdapterTrade {
                transactions: vec![compiled],
                primary_tx_index: 0,
                dependency_mode: TransactionDependencyMode::Independent,
                entry_preference_asset: None,
            })
        }
    }
}

pub(crate) async fn quote_raydium_launchlab_token_value_lamports(
    rpc_url: &str,
    commitment: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    token_amount_raw: u64,
) -> Result<u64, String> {
    if !matches!(selector.family, TradeVenueFamily::RaydiumLaunchLab) {
        return Err(format!(
            "Raydium LaunchLab quote received unsupported family {}.",
            selector.family.label()
        ));
    }
    if token_amount_raw == 0 {
        return Ok(0);
    }
    let mint = parse_pubkey(mint, "Raydium LaunchLab quote mint")?;
    let quote_mint = wsol_mint()?;
    let canonical_pool = canonical_pool_id(&mint, &quote_mint)?;
    if selector.canonical_market_key != canonical_pool.to_string() {
        return Err(format!(
            "Raydium LaunchLab quote selector market {} does not match canonical pool {} for mint {}.",
            selector.canonical_market_key, canonical_pool, mint
        ));
    }
    let pool_data = fetch_account_data(rpc_url, &canonical_pool.to_string(), commitment).await?;
    let pool = decode_launchlab_pool(&pool_data)?;
    validate_launchlab_pool_for_request(&pool, &mint, &canonical_pool)?;
    if !matches!(pool.lifecycle_status(), LaunchLabPoolStatus::Trading) {
        return Err(
            "Raydium LaunchLab quote only supports active pre-migration pools.".to_string(),
        );
    }
    let config_accounts = vec![pool.config_id.to_string(), pool.platform_id.to_string()];
    let config_results =
        fetch_multiple_account_owner_and_data(rpc_url, &config_accounts, commitment).await?;
    let config_data = config_results
        .first()
        .and_then(|entry| entry.as_ref())
        .map(|(_, data)| data.clone())
        .ok_or_else(|| format!("Raydium LaunchLab config {} was not found.", pool.config_id))?;
    let platform_data = config_results
        .get(1)
        .and_then(|entry| entry.as_ref())
        .map(|(_, data)| data.clone())
        .ok_or_else(|| {
            format!(
                "Raydium LaunchLab platform {} was not found.",
                pool.platform_id
            )
        })?;
    let context = LaunchLabPoolContext {
        pool_id: canonical_pool,
        pool,
        config: decode_launchlab_config(&config_data)?,
        platform: decode_platform_config(&platform_data)?,
        quote_mint,
        token_program: spl_token::id(),
        quote_token_program: spl_token::id(),
    };
    let expected_amount_b = quote_sell_exact_in_amount_b(
        &quote_config_from_context(&context),
        &BigUint::from(token_amount_raw),
    )?;
    biguint_to_u64(&expected_amount_b, "Raydium LaunchLab token value")
}

async fn load_live_launchlab_context(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
) -> Result<LaunchLabPoolContext, String> {
    let mint = parse_pubkey(&request.mint, "Raydium LaunchLab mint")?;
    let quote_mint = wsol_mint()?;
    let canonical_pool = canonical_pool_id(&mint, &quote_mint)?;
    if selector.canonical_market_key != canonical_pool.to_string() {
        return Err(format!(
            "Raydium LaunchLab stale route: selector market {} no longer matches canonical pool {} for mint {}.",
            selector.canonical_market_key, canonical_pool, request.mint
        ));
    }
    let pool_data = fetch_account_data(
        rpc_url,
        &canonical_pool.to_string(),
        &request.policy.commitment,
    )
    .await?;
    let pool = decode_launchlab_pool(&pool_data)?;
    validate_launchlab_pool_for_request(&pool, &mint, &canonical_pool)?;
    match pool.lifecycle_status() {
        LaunchLabPoolStatus::Trading => {}
        LaunchLabPoolStatus::Migrating => Err(
            "[stale_route_reclassified] Raydium LaunchLab pool is migrating; refusing to compile stale pre-migration route."
                .to_string(),
        )?,
        LaunchLabPoolStatus::Migrated => Err(
            "[stale_route_reclassified] Raydium LaunchLab pool has migrated; re-plan before trading the migrated Raydium pool."
                .to_string(),
        )?,
        LaunchLabPoolStatus::Unknown(status) => Err(format!(
            "Raydium LaunchLab pool status {status} is unsupported for trading."
        ))?,
    };
    let config_accounts = vec![pool.config_id.to_string(), pool.platform_id.to_string()];
    let (config_results, token_program_result) = tokio::join!(
        fetch_multiple_account_owner_and_data(
            rpc_url,
            &config_accounts,
            &request.policy.commitment,
        ),
        fetch_supported_spl_mint_program(rpc_url, &mint, &request.policy.commitment)
    );
    let config_results = config_results?;
    let config_data = config_results
        .first()
        .and_then(|entry| entry.as_ref())
        .map(|(_, data)| data.clone())
        .ok_or_else(|| format!("Raydium LaunchLab config {} was not found.", pool.config_id))?;
    let platform_data = config_results
        .get(1)
        .and_then(|entry| entry.as_ref())
        .map(|(_, data)| data.clone())
        .ok_or_else(|| {
            format!(
                "Raydium LaunchLab platform {} was not found.",
                pool.platform_id
            )
        })?;
    let token_program = token_program_result?;
    Ok(LaunchLabPoolContext {
        pool_id: canonical_pool,
        pool,
        config: decode_launchlab_config(&config_data)?,
        platform: decode_platform_config(&platform_data)?,
        quote_mint,
        token_program,
        quote_token_program: spl_token::id(),
    })
}

async fn compile_buy_transaction(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    owner: &Keypair,
    context: &LaunchLabPoolContext,
    buy_amount_sol: &str,
) -> Result<CompiledTransaction, String> {
    let owner_pubkey = owner.pubkey();
    let amount_b = parse_decimal_units(buy_amount_sol, 9, "buyAmountSol")?;
    if amount_b == 0 {
        return Err("Raydium LaunchLab buy amount must be greater than zero.".to_string());
    }
    let slippage_bps = parse_slippage_bps(Some(request.policy.slippage_percent.as_str()))?;
    let quote_config = quote_config_from_context(context);
    let expected_amount_a = quote_buy_exact_in_amount_a(&quote_config, &BigUint::from(amount_b))?;
    if expected_amount_a == BigUint::from(0u8) {
        return Err("Raydium LaunchLab buy quote resolved to zero tokens.".to_string());
    }
    let min_amount_a = biguint_to_u64(
        &build_min_amount_from_bps(&expected_amount_a, u64::from(slippage_bps)),
        "Raydium LaunchLab buy min output",
    )?;

    let user_token_account_a = get_associated_token_address_with_program_id(
        &owner_pubkey,
        &context.pool.mint_a,
        &context.token_program,
    );
    let wrapped_signer = Keypair::new();
    let user_token_account_b = wrapped_signer.pubkey();
    let rent_lamports = crate::warming_service::shared_warming_service()
        .minimum_balance_for_rent_exemption(SPL_TOKEN_ACCOUNT_LEN, || async {
            crate::rpc_client::fetch_minimum_balance_for_rent_exemption(
                rpc_url,
                &request.policy.commitment,
                SPL_TOKEN_ACCOUNT_LEN,
            )
            .await
        })
        .await?;

    let mut instructions =
        launchlab_prefix_instructions(request, LAUNCHLAB_BUY_COMPUTE_UNIT_LIMIT)?;
    instructions.push(create_associated_token_account_idempotent(
        &owner_pubkey,
        &owner_pubkey,
        &context.pool.mint_a,
        &context.token_program,
    ));
    instructions.extend(build_wrapped_sol_open_instructions(
        &owner_pubkey,
        &user_token_account_b,
        rent_lamports
            .checked_add(amount_b)
            .ok_or_else(|| "Raydium LaunchLab WSOL funding overflowed.".to_string())?,
    )?);
    instructions.push(build_buy_exact_in_instruction(
        &owner_pubkey,
        context,
        &user_token_account_a,
        &user_token_account_b,
        amount_b,
        min_amount_a,
    )?);
    instructions.push(build_wrapped_sol_close_instruction(
        &owner_pubkey,
        &user_token_account_b,
    )?);
    finalize_launchlab_transaction(
        rpc_url,
        request,
        owner,
        &[&wrapped_signer],
        instructions,
        "raydium-launchlab-buy",
        LAUNCHLAB_BUY_COMPUTE_UNIT_LIMIT,
    )
    .await
}

async fn compile_sell_transaction(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    owner: &Keypair,
    context: &LaunchLabPoolContext,
) -> Result<CompiledTransaction, String> {
    let owner_pubkey = owner.pubkey();
    let user_token_account_a = get_associated_token_address_with_program_id(
        &owner_pubkey,
        &context.pool.mint_a,
        &context.token_program,
    );
    let token_account_data = fetch_account_data(
        rpc_url,
        &user_token_account_a.to_string(),
        &request.policy.commitment,
    )
    .await?;
    let balance = read_token_account_amount(&token_account_data)?;
    let sell_intent = request
        .sell_intent
        .as_ref()
        .ok_or_else(|| "Missing sell intent for Raydium LaunchLab sell.".to_string())?;
    let quote_config = quote_config_from_context(context);
    let amount_a = match sell_intent {
        RuntimeSellIntent::Percent(_) => {
            let sell_percent = parse_sell_percent(sell_intent)?;
            ((u128::from(balance) * u128::from(sell_percent)) / 100u128).min(u128::from(u64::MAX))
                as u64
        }
        RuntimeSellIntent::SolOutput(value) => {
            let target_lamports = parse_decimal_units(value, 9, "sellOutputSol")?;
            crate::sell_target_sizing::choose_target_sized_token_amount(
                balance,
                target_lamports,
                |amount| {
                    quote_sell_exact_in_amount_b(&quote_config, &BigUint::from(amount))
                        .and_then(|quote| biguint_to_u64(&quote, "Raydium LaunchLab sell quote"))
                        .and_then(crate::sell_target_sizing::net_sol_after_wrapper_fee)
                },
            )?
        }
    };
    if amount_a == 0 {
        return Err("Raydium LaunchLab sell amount resolved to zero.".to_string());
    }
    let slippage_bps = parse_slippage_bps(Some(request.policy.slippage_percent.as_str()))?;
    let expected_amount_b = quote_sell_exact_in_amount_b(&quote_config, &BigUint::from(amount_a))?;
    if expected_amount_b == BigUint::from(0u8) {
        return Err("Raydium LaunchLab sell quote resolved to zero SOL.".to_string());
    }
    let min_amount_b = biguint_to_u64(
        &build_min_amount_from_bps(&expected_amount_b, u64::from(slippage_bps)),
        "Raydium LaunchLab sell min output",
    )?;

    let wrapped_signer = Keypair::new();
    let user_token_account_b = wrapped_signer.pubkey();
    let rent_lamports = crate::warming_service::shared_warming_service()
        .minimum_balance_for_rent_exemption(SPL_TOKEN_ACCOUNT_LEN, || async {
            crate::rpc_client::fetch_minimum_balance_for_rent_exemption(
                rpc_url,
                &request.policy.commitment,
                SPL_TOKEN_ACCOUNT_LEN,
            )
            .await
        })
        .await?;
    let mut instructions =
        launchlab_prefix_instructions(request, LAUNCHLAB_SELL_COMPUTE_UNIT_LIMIT)?;
    instructions.extend(build_wrapped_sol_open_instructions(
        &owner_pubkey,
        &user_token_account_b,
        rent_lamports,
    )?);
    instructions.push(build_sell_exact_in_instruction(
        &owner_pubkey,
        context,
        &user_token_account_a,
        &user_token_account_b,
        amount_a,
        min_amount_b,
    )?);
    instructions.push(build_wrapped_sol_close_instruction(
        &owner_pubkey,
        &user_token_account_b,
    )?);
    finalize_launchlab_transaction(
        rpc_url,
        request,
        owner,
        &[&wrapped_signer],
        instructions,
        "raydium-launchlab-sell",
        LAUNCHLAB_SELL_COMPUTE_UNIT_LIMIT,
    )
    .await
}

fn quote_config_from_context(context: &LaunchLabPoolContext) -> CurveQuoteConfig {
    CurveQuoteConfig {
        pool: context.pool.curve_state(),
        curve_type: context.config.curve_type,
        trade_fee_rate: BigUint::from(context.config.trade_fee_rate),
        platform_fee_rate: BigUint::from(context.platform.fee_rate),
        creator_fee_rate: BigUint::from(context.platform.creator_fee_rate),
    }
}

fn launchlab_prefix_instructions(
    request: &TradeRuntimeRequest,
    compute_unit_limit: u32,
) -> Result<Vec<Instruction>, String> {
    let mut instructions = vec![build_compute_unit_limit_instruction(compute_unit_limit)?];
    let compute_unit_price_micro_lamports =
        priority_fee_sol_to_micro_lamports(&request.policy.fee_sol)?;
    if compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            compute_unit_price_micro_lamports,
        )?);
    }
    Ok(instructions)
}

async fn finalize_launchlab_transaction(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    owner: &Keypair,
    extra_signers: &[&Keypair],
    mut instructions: Vec<Instruction>,
    label: &str,
    compute_unit_limit: u32,
) -> Result<CompiledTransaction, String> {
    let owner_pubkey = owner.pubkey();
    let compute_unit_price_micro_lamports =
        priority_fee_sol_to_micro_lamports(&request.policy.fee_sol)?;
    let (inline_tip_lamports, inline_tip_account) =
        if let Some((tip_instruction, tip_lamports, tip_account)) = resolve_inline_tip(
            &owner_pubkey,
            &request.policy.provider,
            &request.policy.tip_sol,
        )? {
            instructions.push(tip_instruction);
            (Some(tip_lamports), Some(tip_account))
        } else {
            (None, None)
        };
    if matches!(
        request.policy.mev_mode,
        crate::extension_api::MevMode::Reduced | crate::extension_api::MevMode::Secure
    ) {
        apply_jitodontfront(&mut instructions, &owner_pubkey)?;
    }
    instructions.push(build_uniqueness_memo_instruction(label)?);
    let blockhash = crate::warming_service::shared_warming_service()
        .latest_blockhash(rpc_url, &request.policy.commitment)
        .await?
        .blockhash;
    let lookup_tables = crate::pump_native::load_shared_super_lookup_tables(rpc_url).await?;
    let message = v0::Message::try_compile(&owner_pubkey, &instructions, &lookup_tables, blockhash)
        .map_err(|error| format!("Failed to compile Raydium LaunchLab transaction: {error}"))?;
    let lookup_tables_used = message
        .address_table_lookups
        .iter()
        .map(|lookup| lookup.account_key.to_string())
        .collect::<Vec<_>>();
    let mut signers = Vec::with_capacity(1 + extra_signers.len());
    signers.push(owner);
    signers.extend(extra_signers.iter().copied());
    let transaction = VersionedTransaction::try_new(VersionedMessage::V0(message), &signers)
        .map_err(|error| format!("Failed to sign Raydium LaunchLab transaction: {error}"))?;
    let signature = transaction
        .signatures
        .first()
        .map(|value| value.to_string())
        .ok_or_else(|| "Raydium LaunchLab transaction did not include a signature.".to_string())?;
    let serialized = bincode::serialize(&transaction)
        .map_err(|error| format!("Failed to serialize Raydium LaunchLab transaction: {error}"))?;
    let serialized_base64 = BASE64.encode(serialized);
    compiled_transaction_signers::remember_compiled_transaction_signers(
        &serialized_base64,
        extra_signers,
    );
    Ok(CompiledTransaction {
        label: label.to_string(),
        format: "v0-alt".to_string(),
        serialized_base64,
        signature: Some(signature),
        lookup_tables_used,
        compute_unit_limit: Some(u64::from(compute_unit_limit)),
        compute_unit_price_micro_lamports: if compute_unit_price_micro_lamports > 0 {
            Some(compute_unit_price_micro_lamports)
        } else {
            None
        },
        inline_tip_lamports,
        inline_tip_account,
    })
}

async fn selector_for_pool(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    pool_id: Pubkey,
    pool: &DecodedLaunchLabPool,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    let lifecycle_status = pool.lifecycle_status();
    match lifecycle_status {
        LaunchLabPoolStatus::Trading => Ok(Some(LifecycleAndCanonicalMarket {
            lifecycle: TradeLifecycle::PreMigration,
            family: TradeVenueFamily::RaydiumLaunchLab,
            canonical_market_key: pool_id.to_string(),
            quote_asset: PlannerQuoteAsset::Sol,
            verification_source: PlannerVerificationSource::OnchainDerived,
            wrapper_action: match request.side {
                TradeSide::Buy => WrapperAction::RaydiumLaunchLabSolBuy,
                TradeSide::Sell => WrapperAction::RaydiumLaunchLabSolSell,
            },
            wrapper_accounts: vec![pool_id.to_string()],
            market_subtype: Some("launchlab-active".to_string()),
            direct_protocol_target: Some("raydium-launchlab".to_string()),
            input_amount_hint: request.buy_amount_sol.clone(),
            minimum_output_hint: match &request.sell_intent {
                Some(RuntimeSellIntent::SolOutput(value)) => Some(value.clone()),
                _ => None,
            },
            runtime_bundle: None,
        })),
        LaunchLabPoolStatus::Migrating => Err(
            "Raydium LaunchLab pool is migrating; trading is disabled until migration completes."
                .to_string(),
        ),
        LaunchLabPoolStatus::Migrated => {
            selector_for_migrated_pool(rpc_url, request, pool_id, pool).await
        }
        LaunchLabPoolStatus::Unknown(status) => Err(format!(
            "Raydium LaunchLab pool status {status} is unsupported for trading."
        )),
    }
}

async fn selector_for_migrated_pool(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    launchlab_pool_id: Pubkey,
    pool: &DecodedLaunchLabPool,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    let quote_mint = wsol_mint()?;
    if pool.migrate_type == LAUNCHLAB_MIGRATE_TO_CPMM {
        fetch_supported_spl_mint_program(rpc_url, &pool.mint_a, &request.policy.commitment).await?;
        let Some(raydium_pool) = find_raydium_cpmm_pool_for_pair(
            rpc_url,
            &pool.mint_a,
            &quote_mint,
            &request.policy.commitment,
        )
        .await?
        else {
            return Err(format!(
                "Raydium LaunchLab pool {launchlab_pool_id} has migrated to CPMM, but no canonical supported Raydium CPMM SOL pool was proven for mint {}.",
                pool.mint_a
            ));
        };
        return match plan_raydium_cpmm_trade_for_pool_id(rpc_url, request, &raydium_pool).await? {
            Some(selector) => Ok(Some(selector)),
            None => Err(format!(
                "Raydium LaunchLab pool {launchlab_pool_id} migrated to Raydium CPMM pool {raydium_pool}, but the destination is not a supported CPMM pool."
            )),
        };
    }
    if pool.migrate_type != LAUNCHLAB_MIGRATE_TO_AMM_V4 {
        return Err(format!(
            "Raydium LaunchLab pool {launchlab_pool_id} has unsupported migration type {} for mint {}.",
            pool.migrate_type, pool.mint_a
        ));
    }
    let Some(raydium_pool) = find_raydium_amm_v4_pool_for_pair(
        rpc_url,
        &pool.mint_a,
        &quote_mint,
        &request.policy.commitment,
    )
    .await?
    else {
        return Err(format!(
            "Raydium LaunchLab pool {launchlab_pool_id} has migrated, but no canonical supported Raydium AMM v4 SOL pool was proven for mint {}.",
            pool.mint_a
        ));
    };
    match plan_raydium_amm_v4_trade_for_pool_id(rpc_url, request, &raydium_pool).await? {
        Some(selector) => Ok(Some(selector)),
        None => Err(format!(
            "Raydium LaunchLab pool {launchlab_pool_id} migrated to Raydium pool {raydium_pool}, but the destination is not a supported AMM v4 pool."
        )),
    }
}

fn validate_launchlab_pool_for_request(
    pool: &DecodedLaunchLabPool,
    mint: &Pubkey,
    pool_id: &Pubkey,
) -> Result<(), String> {
    if pool.mint_a != *mint {
        return Err(format!(
            "Raydium LaunchLab pool {} is for mint {}, not requested mint {}.",
            pool_id, pool.mint_a, mint
        ));
    }
    let quote_mint = wsol_mint()?;
    if pool.mint_b != quote_mint {
        return Err(format!(
            "Raydium LaunchLab pool {} is quote mint {}, not WSOL.",
            pool_id, pool.mint_b
        ));
    }
    let canonical_pool = canonical_pool_id(mint, &quote_mint)?;
    if canonical_pool != *pool_id {
        return Err(format!(
            "Raydium LaunchLab pool {} is not the canonical SOL pool {} for mint {}.",
            pool_id, canonical_pool, mint
        ));
    }
    if is_bonk_platform(&pool.platform_id) {
        return Err(
            "Raydium LaunchLab route resolved to a LetsBonk/Bonkers platform pool; use the Bonk route."
                .to_string(),
        );
    }
    Ok(())
}

fn is_bonk_platform(platform_id: &Pubkey) -> bool {
    let letsbonk = Pubkey::from_str(BONK_LETSBONK_PLATFORM_ID).unwrap_or_default();
    let bonkers = Pubkey::from_str(BONK_BONKERS_PLATFORM_ID).unwrap_or_default();
    *platform_id == letsbonk || *platform_id == bonkers
}

fn parse_pubkey(value: &str, label: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|error| format!("Invalid {label}: {error}"))
}

async fn fetch_supported_spl_mint_program(
    rpc_url: &str,
    mint: &Pubkey,
    commitment: &str,
) -> Result<Pubkey, String> {
    let owner = fetch_account_owner_and_data(rpc_url, &mint.to_string(), commitment)
        .await?
        .map(|(owner, _)| owner)
        .ok_or_else(|| format!("Raydium LaunchLab mint {mint} was not found."))?;
    if owner != spl_token::id() {
        return Err(format!(
            "Raydium LaunchLab mint {mint} is owned by unsupported token program {owner}; only SPL Token is enabled for this route."
        ));
    }
    Ok(owner)
}

fn parse_sell_percent(intent: &RuntimeSellIntent) -> Result<u8, String> {
    match intent {
        RuntimeSellIntent::Percent(value) => {
            let parsed = value
                .trim()
                .parse::<u8>()
                .map_err(|error| format!("Invalid Raydium LaunchLab sell percent: {error}"))?;
            if parsed == 0 || parsed > 100 {
                return Err("Raydium LaunchLab sell percent must be between 1 and 100.".to_string());
            }
            Ok(parsed)
        }
        RuntimeSellIntent::SolOutput(_) => {
            Err("Raydium LaunchLab exact-output sells are not enabled for this route.".to_string())
        }
    }
}

fn build_wrapped_sol_open_instructions(
    owner: &Pubkey,
    wrapped_account: &Pubkey,
    lamports: u64,
) -> Result<Vec<Instruction>, String> {
    Ok(vec![
        create_account(
            owner,
            wrapped_account,
            lamports,
            SPL_TOKEN_ACCOUNT_LEN,
            &spl_token::id(),
        ),
        initialize_account3(&spl_token::id(), wrapped_account, &wsol_mint()?, owner).map_err(
            |error| format!("Failed to initialize Raydium LaunchLab WSOL account: {error}"),
        )?,
    ])
}

fn build_wrapped_sol_close_instruction(
    owner: &Pubkey,
    wrapped_account: &Pubkey,
) -> Result<Instruction, String> {
    close_spl_account(&spl_token::id(), wrapped_account, owner, owner, &[])
        .map_err(|error| format!("Failed to build Raydium LaunchLab WSOL close: {error}"))
}

fn build_compute_unit_limit_instruction(compute_unit_limit: u32) -> Result<Instruction, String> {
    let mut data = vec![2];
    data.extend_from_slice(&compute_unit_limit.to_le_bytes());
    Ok(Instruction {
        program_id: parse_pubkey(COMPUTE_BUDGET_PROGRAM_ID, "compute budget program")?,
        accounts: vec![],
        data,
    })
}

fn build_compute_unit_price_instruction(micro_lamports: u64) -> Result<Instruction, String> {
    let mut data = vec![3];
    data.extend_from_slice(&micro_lamports.to_le_bytes());
    Ok(Instruction {
        program_id: parse_pubkey(COMPUTE_BUDGET_PROGRAM_ID, "compute budget program")?,
        accounts: vec![],
        data,
    })
}

fn build_uniqueness_memo_instruction(label: &str) -> Result<Instruction, String> {
    Ok(Instruction {
        program_id: parse_pubkey(MEMO_PROGRAM_ID, "memo program")?,
        accounts: vec![],
        data: format!("tt:{label}:{}", now_unix_ms()).into_bytes(),
    })
}

fn resolve_inline_tip(
    payer: &Pubkey,
    provider: &str,
    tip_sol: &str,
) -> Result<Option<(Instruction, u64, String)>, String> {
    let provider_tip_account_raw = pick_tip_account_for_provider(provider);
    let (tip_account_str, resolved_from_provider) = if provider_tip_account_raw.is_empty() {
        match configured_tip_account()? {
            Some(account) => (account.to_string(), false),
            None => return Ok(None),
        }
    } else {
        (provider_tip_account_raw, true)
    };
    let min_lamports = provider_min_tip_lamports(provider);
    let requested_lamports = parse_decimal_units(tip_sol, 9, "tipSol").unwrap_or(0);
    let lamports = if resolved_from_provider {
        requested_lamports.max(min_lamports)
    } else {
        requested_lamports
    };
    if lamports == 0 {
        return Ok(None);
    }
    let tip_pubkey = parse_pubkey(&tip_account_str, "tip account")?;
    Ok(Some((
        transfer(payer, &tip_pubkey, lamports),
        lamports,
        tip_account_str,
    )))
}

fn provider_min_tip_lamports(provider: &str) -> u64 {
    match provider.trim() {
        "helius-sender" => HELIUS_SENDER_MIN_TIP_LAMPORTS,
        "hellomoon" => HELLO_MOON_MIN_TIP_LAMPORTS,
        _ => 0,
    }
}

fn configured_tip_account() -> Result<Option<Pubkey>, String> {
    let raw = std::env::var("EXECUTION_ENGINE_JITO_TIP_ACCOUNT")
        .ok()
        .or_else(|| std::env::var("JITO_TIP_ACCOUNT").ok())
        .unwrap_or_default();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        parse_pubkey(trimmed, "tip account").map(Some)
    }
}

fn apply_jitodontfront(instructions: &mut Vec<Instruction>, payer: &Pubkey) -> Result<(), String> {
    if instructions.iter().any(|instruction| {
        instruction
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == JITODONTFRONT_ACCOUNT)
    }) {
        return Ok(());
    }
    let dontfront = parse_pubkey(JITODONTFRONT_ACCOUNT, "jitodontfront")?;
    let mut instruction = transfer(payer, payer, 0);
    instruction
        .accounts
        .push(AccountMeta::new_readonly(dontfront, false));
    instructions.insert(0, instruction);
    Ok(())
}

fn read_token_account_amount(data: &[u8]) -> Result<u64, String> {
    if data.len() < 72 {
        return Err("Token account data was shorter than expected.".to_string());
    }
    let bytes: [u8; 8] = data[64..72]
        .try_into()
        .map_err(|_| "Token account amount bytes were invalid.".to_string())?;
    Ok(u64::from_le_bytes(bytes))
}

fn priority_fee_sol_to_micro_lamports(priority_fee_sol: &str) -> Result<u64, String> {
    let lamports = parse_decimal_units(priority_fee_sol, 9, "feeSol")?;
    if lamports == 0 {
        Ok(0)
    } else {
        Ok((lamports.saturating_mul(1_000_000)) / PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT)
    }
}

fn parse_slippage_bps(value: Option<&str>) -> Result<u16, String> {
    let raw = value.unwrap_or("20").trim();
    if raw.is_empty() {
        return Ok(2_000);
    }
    let hundredths = parse_decimal_scaled(raw, 2, "slippagePercent")?;
    if hundredths > 1_000_000 {
        return Err("slippagePercent is out of range.".to_string());
    }
    let bps =
        u16::try_from(hundredths).map_err(|_| "slippagePercent is out of range.".to_string())?;
    Ok(if bps == 0 { 1 } else { bps })
}

fn parse_decimal_units(value: &str, decimals: usize, label: &str) -> Result<u64, String> {
    let scaled = parse_decimal_scaled(value, decimals, label)?;
    u64::try_from(scaled).map_err(|_| format!("{label} is too large."))
}

fn parse_decimal_scaled(value: &str, decimals: usize, label: &str) -> Result<u128, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    let mut parts = trimmed.split('.');
    let whole = parts.next().unwrap_or_default();
    let fractional = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        return Err(format!("{label} is not a valid number."));
    }
    if !whole.chars().all(|ch| ch.is_ascii_digit())
        || !fractional.chars().all(|ch| ch.is_ascii_digit())
    {
        return Err(format!("{label} is not a valid number."));
    }
    if fractional.len() > decimals {
        return Err(format!("{label} has too many decimal places."));
    }
    let scale = 10_u128
        .checked_pow(u32::try_from(decimals).map_err(|_| format!("{label} scale overflowed."))?)
        .ok_or_else(|| format!("{label} scale overflowed."))?;
    let whole_value = if whole.is_empty() {
        0
    } else {
        whole
            .parse::<u128>()
            .map_err(|error| format!("{label} parse failed: {error}"))?
    };
    let fractional_value = if fractional.is_empty() {
        0
    } else {
        let padded = format!("{fractional:0<width$}", width = decimals);
        padded
            .parse::<u128>()
            .map_err(|error| format!("{label} parse failed: {error}"))?
    };
    whole_value
        .checked_mul(scale)
        .and_then(|value| value.checked_add(fractional_value))
        .ok_or_else(|| format!("{label} is too large."))
}

fn biguint_to_u64(value: &BigUint, label: &str) -> Result<u64, String> {
    let digits = value.to_u64_digits();
    match digits.as_slice() {
        [] => Ok(0),
        [value] => Ok(*value),
        _ => Err(format!("{label} exceeds u64.")),
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        extension_api::{BuyFundingPolicy, MevMode, SellSettlementPolicy},
        trade_runtime::RuntimeExecutionPolicy,
    };

    fn request(platform_label: Option<&str>) -> TradeRuntimeRequest {
        TradeRuntimeRequest {
            side: TradeSide::Buy,
            mint: Pubkey::new_unique().to_string(),
            buy_amount_sol: Some("0.1".to_string()),
            sell_intent: None,
            policy: RuntimeExecutionPolicy {
                slippage_percent: "5".to_string(),
                mev_mode: MevMode::Off,
                auto_tip_enabled: false,
                fee_sol: "0".to_string(),
                tip_sol: "0".to_string(),
                provider: "standard-rpc".to_string(),
                endpoint_profile: "global".to_string(),
                commitment: "confirmed".to_string(),
                skip_preflight: false,
                track_send_block_height: false,
                buy_funding_policy: BuyFundingPolicy::SolOnly,
                sell_settlement_policy: SellSettlementPolicy::AlwaysToSol,
                sell_settlement_asset: TradeSettlementAsset::Sol,
            },
            platform_label: platform_label.map(str::to_string),
            planned_route: None,
            planned_trade: None,
            pinned_pool: None,
            warm_key: None,
        }
    }

    fn active_pool(mint: Pubkey) -> DecodedLaunchLabPool {
        DecodedLaunchLabPool {
            creator: Pubkey::new_unique(),
            status: 0,
            migrate_type: 0,
            supply: 1_000_000,
            config_id: Pubkey::new_unique(),
            total_sell_a: 0,
            virtual_a: 1_000_000,
            virtual_b: 1_000_000,
            real_a: 1_000_000,
            real_b: 1_000_000,
            platform_id: Pubkey::new_unique(),
            mint_a: mint,
            mint_b: wsol_mint().expect("wsol mint"),
        }
    }

    #[tokio::test]
    async fn platform_label_does_not_gate_active_selector() {
        let mint = Pubkey::new_unique();
        let pool_id = canonical_pool_id(&mint, &wsol_mint().expect("wsol mint")).expect("pool");
        let pool = active_pool(mint);
        let mut axiom = request(Some("axiom"));
        axiom.mint = mint.to_string();
        let mut j7 = request(Some("j7"));
        j7.mint = mint.to_string();

        let axiom_selector = selector_for_pool("unused", &axiom, pool_id, &pool)
            .await
            .expect("selector")
            .expect("active selector");
        let j7_selector = selector_for_pool("unused", &j7, pool_id, &pool)
            .await
            .expect("selector")
            .expect("active selector");
        assert_eq!(
            axiom_selector.canonical_market_key,
            j7_selector.canonical_market_key
        );
        assert!(matches!(
            axiom_selector.family,
            TradeVenueFamily::RaydiumLaunchLab
        ));
    }

    #[test]
    fn sell_percent_rejects_exact_output() {
        assert_eq!(
            parse_sell_percent(&RuntimeSellIntent::Percent("100".to_string())).expect("percent"),
            100
        );
        assert!(parse_sell_percent(&RuntimeSellIntent::SolOutput("1".to_string())).is_err());
    }

    #[test]
    fn policy_validation_is_side_specific() {
        let mut buy = request(Some("launchlab"));
        buy.policy.sell_settlement_policy = SellSettlementPolicy::AlwaysToUsd1;
        buy.policy.sell_settlement_asset = TradeSettlementAsset::Usd1;
        validate_launchlab_policy_for_side(&buy).expect("buy ignores sell settlement defaults");

        buy.policy.buy_funding_policy = BuyFundingPolicy::Usd1Only;
        assert!(validate_launchlab_policy_for_side(&buy).is_err());

        let mut sell = request(Some("launchlab"));
        sell.side = TradeSide::Sell;
        sell.buy_amount_sol = None;
        sell.sell_intent = Some(RuntimeSellIntent::Percent("100".to_string()));
        sell.policy.buy_funding_policy = BuyFundingPolicy::Usd1Only;
        validate_launchlab_policy_for_side(&sell).expect("sell ignores buy funding defaults");

        sell.policy.sell_settlement_asset = TradeSettlementAsset::Usd1;
        assert!(validate_launchlab_policy_for_side(&sell).is_err());
    }
}
