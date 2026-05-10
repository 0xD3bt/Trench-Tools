use std::{
    collections::HashMap,
    str::FromStr,
    sync::OnceLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::json;
use shared_transaction_submit::{compiled_transaction_signers, fetch_multiple_account_data};
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
use spl_token::instruction::{
    close_account as close_spl_account, initialize_account3, sync_native,
};

use crate::{
    extension_api::{MevMode, TradeSide},
    provider_tip::pick_tip_account_for_provider,
    rpc_client::{
        CompiledTransaction, fetch_account_data, fetch_account_owner_and_data,
        fetch_minimum_balance_for_rent_exemption,
    },
    trade_dispatch::{CompiledAdapterTrade, TransactionDependencyMode},
    trade_planner::{
        LifecycleAndCanonicalMarket, PlannerQuoteAsset, PlannerRuntimeBundle,
        PlannerVerificationSource, RaydiumAmmV4RuntimeBundle, TradeLifecycle, TradeVenueFamily,
        WrapperAction,
    },
    trade_runtime::{RuntimeSellIntent, TradeRuntimeRequest},
    wallet_store::load_solana_wallet_by_env_key,
    warming_service::shared_warming_service,
};

const RAYDIUM_AMM_V4_PROGRAM_ID: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
const RAYDIUM_AMM_V4_AUTHORITY: &str = "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1";
const RAYDIUM_AMM_V4_OPENBOOK_MARKET_PROGRAM_ID: &str =
    "srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX";
const RAYDIUM_AMM_V4_ACCOUNT_LEN: usize = 752;
const RAYDIUM_AMM_V4_BASE_VAULT_OFFSET: usize = 336;
const RAYDIUM_AMM_V4_QUOTE_VAULT_OFFSET: usize = 368;
const RAYDIUM_AMM_V4_BASE_MINT_OFFSET: usize = 400;
const RAYDIUM_AMM_V4_QUOTE_MINT_OFFSET: usize = 432;
const RAYDIUM_AMM_V4_OPEN_ORDERS_OFFSET: usize = 496;
const RAYDIUM_AMM_V4_MARKET_OFFSET: usize = 528;
const RAYDIUM_AMM_V4_MARKET_PROGRAM_OFFSET: usize = 560;
const RAYDIUM_AMM_V4_TARGET_ORDERS_OFFSET: usize = 592;
const SERUM_MARKET_VAULT_SIGNER_NONCE_OFFSET: usize = 45;
const SERUM_MARKET_BASE_MINT_OFFSET: usize = 53;
const SERUM_MARKET_QUOTE_MINT_OFFSET: usize = 85;
const SERUM_MARKET_BASE_VAULT_OFFSET: usize = 117;
const SERUM_MARKET_QUOTE_VAULT_OFFSET: usize = 165;
const SERUM_MARKET_EVENT_QUEUE_OFFSET: usize = 253;
const SERUM_MARKET_BIDS_OFFSET: usize = 285;
const SERUM_MARKET_ASKS_OFFSET: usize = 317;
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
const JITODONTFRONT_ACCOUNT: &str = "jitodontfront111111111111111111111111111111";
const SPL_TOKEN_ACCOUNT_LEN: u64 = 165;
const PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT: u64 = 1_000_000;
const RAYDIUM_AMM_V4_BUY_COMPUTE_UNIT_LIMIT: u32 = 340_000;
const RAYDIUM_AMM_V4_SELL_COMPUTE_UNIT_LIMIT: u32 = 340_000;
const HELIUS_SENDER_MIN_TIP_LAMPORTS: u64 = 200_000;
const HELLO_MOON_MIN_TIP_LAMPORTS: u64 = 1_000_000;

fn raydium_amm_v4_http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

#[derive(Debug, Clone)]
pub(crate) struct RaydiumAmmV4PoolState {
    pubkey: Pubkey,
    authority: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    base_vault: Pubkey,
    quote_vault: Pubkey,
    open_orders: Pubkey,
    target_orders: Pubkey,
    market_program: Pubkey,
    market: Pubkey,
    market_bids: Pubkey,
    market_asks: Pubkey,
    market_event_queue: Pubkey,
    market_base_vault: Pubkey,
    market_quote_vault: Pubkey,
    market_vault_signer: Pubkey,
    mint_token_program: Pubkey,
    trade_fee_numerator: u64,
    trade_fee_denominator: u64,
}

pub(crate) fn raydium_amm_v4_program_id() -> Result<Pubkey, String> {
    parse_pubkey(RAYDIUM_AMM_V4_PROGRAM_ID, "Raydium AMM v4 program")
}

pub(crate) async fn classify_raydium_amm_v4_pool_address(
    rpc_url: &str,
    address: &str,
    data: &[u8],
    commitment: &str,
) -> Result<Option<(String, String)>, String> {
    let pool_id = parse_pubkey(address, "Raydium AMM v4 pool id")?;
    let pool = match decode_raydium_amm_v4_pool_state(pool_id, data) {
        Ok(pool) => pool,
        Err(error) if error.contains("shorter") => return Ok(None),
        Err(error) => return Err(error),
    };
    let mint = raydium_amm_v4_pool_token_mint(&pool)?;
    let _ = enrich_raydium_amm_v4_pool_state(rpc_url, pool, &mint, commitment).await?;
    Ok(Some((mint.to_string(), pool_id.to_string())))
}

pub(crate) async fn plan_raydium_amm_v4_trade(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    let pinned_pool = request
        .pinned_pool
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Raydium AMM v4 routing requires an explicit pool address.".to_string())?;
    let pool_id = parse_pubkey(pinned_pool, "Raydium AMM v4 pool")?;
    let (owner, data) =
        fetch_account_owner_and_data(rpc_url, pinned_pool, &request.policy.commitment)
            .await?
            .ok_or_else(|| format!("Raydium AMM v4 pool {pinned_pool} was not found."))?;
    if owner != raydium_amm_v4_program_id()? {
        return Ok(None);
    }
    let pool = decode_raydium_amm_v4_pool_state(pool_id, &data)?;
    let mint = parse_pubkey(&request.mint, "Raydium AMM v4 mint")?;
    let pool =
        enrich_raydium_amm_v4_pool_state(rpc_url, pool, &mint, &request.policy.commitment).await?;
    Ok(Some(build_raydium_amm_v4_selector(request, &pool)))
}

pub(crate) async fn plan_raydium_amm_v4_trade_for_pool_id(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    pool_id: &Pubkey,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    let (owner, data) =
        fetch_account_owner_and_data(rpc_url, &pool_id.to_string(), &request.policy.commitment)
            .await?
            .ok_or_else(|| format!("Raydium AMM v4 pool {pool_id} was not found."))?;
    if owner != raydium_amm_v4_program_id()? {
        return Ok(None);
    }
    let mint = parse_pubkey(&request.mint, "Raydium AMM v4 mint")?;
    let pool = decode_raydium_amm_v4_pool_state(*pool_id, &data)?;
    let pool =
        enrich_raydium_amm_v4_pool_state(rpc_url, pool, &mint, &request.policy.commitment).await?;
    Ok(Some(build_raydium_amm_v4_selector(request, &pool)))
}

pub(crate) async fn find_raydium_amm_v4_pool_for_pair(
    rpc_url: &str,
    mint: &Pubkey,
    quote_mint: &Pubkey,
    commitment: &str,
) -> Result<Option<Pubkey>, String> {
    let mut candidates = Vec::new();
    for (left, right) in [(mint, quote_mint), (quote_mint, mint)] {
        candidates.extend(
            fetch_raydium_amm_v4_pools_for_ordered_pair(rpc_url, left, right, commitment).await?,
        );
    }
    candidates.sort_by_key(|pool_id| pool_id.to_string());
    candidates.dedup();
    match candidates.as_slice() {
        [] => Ok(None),
        [pool_id] => Ok(Some(*pool_id)),
        _ => Err(format!(
            "Multiple Raydium AMM v4 pools matched mint {} and quote {}; refusing ambiguous migrated LaunchLab route.",
            mint, quote_mint
        )),
    }
}

pub(crate) async fn compile_raydium_amm_v4_trade(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    wallet_key: &str,
) -> Result<CompiledAdapterTrade, String> {
    let rpc_url = crate::rpc_client::configured_rpc_url();
    let owner = load_solana_wallet_by_env_key(wallet_key)?;
    let owner_pubkey = owner.pubkey();
    let mint = parse_pubkey(&request.mint, "Raydium AMM v4 mint")?;
    let pool = pool_from_runtime_bundle(selector)?;
    validate_raydium_amm_v4_pool_for_mint(&pool, &mint)?;
    validate_raydium_amm_v4_token_program(&pool)?;
    if pool.trade_fee_denominator == 0 || pool.trade_fee_numerator >= pool.trade_fee_denominator {
        return Err("Raydium AMM v4 fee configuration is invalid.".to_string());
    }

    let base_reserve = read_token_account_amount(
        &fetch_account_data(
            &rpc_url,
            &pool.base_vault.to_string(),
            &request.policy.commitment,
        )
        .await?,
    )?;
    let quote_reserve = read_token_account_amount(
        &fetch_account_data(
            &rpc_url,
            &pool.quote_vault.to_string(),
            &request.policy.commitment,
        )
        .await?,
    )?;
    if base_reserve == 0 || quote_reserve == 0 {
        return Err("Raydium AMM v4 pool reserves are empty.".to_string());
    }

    let slippage_bps = parse_slippage_bps(Some(request.policy.slippage_percent.as_str()))?;
    let compute_unit_limit = match request.side {
        TradeSide::Buy => RAYDIUM_AMM_V4_BUY_COMPUTE_UNIT_LIMIT,
        TradeSide::Sell => RAYDIUM_AMM_V4_SELL_COMPUTE_UNIT_LIMIT,
    };
    let compute_unit_price_micro_lamports =
        priority_fee_sol_to_micro_lamports(&request.policy.fee_sol)?;
    let user_token_account = get_associated_token_address_with_program_id(
        &owner_pubkey,
        &mint,
        &pool.mint_token_program,
    );
    let temp_wsol_account = Keypair::new();
    let temp_wsol_account_pubkey = temp_wsol_account.pubkey();
    let wsol_account_rent_lamports = shared_warming_service()
        .minimum_balance_for_rent_exemption(SPL_TOKEN_ACCOUNT_LEN, || async {
            fetch_minimum_balance_for_rent_exemption(
                &rpc_url,
                &request.policy.commitment,
                SPL_TOKEN_ACCOUNT_LEN,
            )
            .await
        })
        .await?;

    let mut instructions = vec![build_compute_unit_limit_instruction(compute_unit_limit)?];
    if compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            compute_unit_price_micro_lamports,
        )?);
    }

    let wsol = wsol_mint()?;
    if matches!(request.side, TradeSide::Buy) {
        instructions.push(create_associated_token_account_idempotent(
            &owner_pubkey,
            &owner_pubkey,
            &mint,
            &pool.mint_token_program,
        ));
        let amount_in = parse_decimal_units(
            request
                .buy_amount_sol
                .as_deref()
                .ok_or_else(|| "Missing buyAmountSol for Raydium AMM v4 buy.".to_string())?,
            9,
            "buyAmountSol",
        )?;
        if amount_in == 0 {
            return Err("Buy amount must be greater than zero.".to_string());
        }
        let (input_vault, output_vault, input_reserve, output_reserve) =
            raydium_amm_v4_swap_sides(&pool, &wsol, &mint, base_reserve, quote_reserve)?;
        let quoted_out = raydium_amm_v4_quote_exact_in(
            amount_in,
            input_reserve,
            output_reserve,
            pool.trade_fee_numerator,
            pool.trade_fee_denominator,
        )?;
        if quoted_out == 0 {
            return Err("Raydium AMM v4 buy quote resolved to zero tokens.".to_string());
        }
        let min_out = apply_sell_side_slippage(quoted_out, slippage_bps);
        instructions.extend(build_wrapped_sol_open_instructions(
            &owner_pubkey,
            &temp_wsol_account_pubkey,
            wsol_account_rent_lamports
                .checked_add(amount_in)
                .ok_or_else(|| "Wrapped SOL funding overflowed.".to_string())?,
            true,
        )?);
        instructions.push(build_raydium_amm_v4_swap_base_in_instruction(
            &pool,
            &input_vault,
            &output_vault,
            &temp_wsol_account_pubkey,
            &user_token_account,
            &owner_pubkey,
            amount_in,
            min_out,
        )?);
    } else {
        let sell_intent = request
            .sell_intent
            .as_ref()
            .ok_or_else(|| "Missing sell intent for Raydium AMM v4 sell.".to_string())?;
        instructions.extend(build_wrapped_sol_open_instructions(
            &owner_pubkey,
            &temp_wsol_account_pubkey,
            wsol_account_rent_lamports,
            false,
        )?);
        let (input_vault, output_vault, input_reserve, output_reserve) =
            raydium_amm_v4_swap_sides(&pool, &mint, &wsol, base_reserve, quote_reserve)?;
        let amount_in = resolve_raydium_amm_v4_sell_input_amount(
            sell_intent,
            wallet_key,
            &owner_pubkey.to_string(),
            &request.mint,
            read_mint_decimals(
                &fetch_account_data(&rpc_url, &mint.to_string(), &request.policy.commitment)
                    .await?,
            )?,
            input_reserve,
            output_reserve,
            pool.trade_fee_numerator,
            pool.trade_fee_denominator,
        )
        .await?;
        let quoted_out = raydium_amm_v4_quote_exact_in(
            amount_in,
            input_reserve,
            output_reserve,
            pool.trade_fee_numerator,
            pool.trade_fee_denominator,
        )?;
        let min_out = apply_sell_side_slippage(quoted_out, slippage_bps);
        instructions.push(build_raydium_amm_v4_swap_base_in_instruction(
            &pool,
            &input_vault,
            &output_vault,
            &user_token_account,
            &temp_wsol_account_pubkey,
            &owner_pubkey,
            amount_in,
            min_out,
        )?);
    }

    instructions.push(build_wrapped_sol_close_instruction(
        &owner_pubkey,
        &temp_wsol_account_pubkey,
    )?);
    let resolved_tip = resolve_inline_tip(
        &owner_pubkey,
        &request.policy.provider,
        &request.policy.tip_sol,
    )?;
    let (inline_tip_lamports, inline_tip_account) =
        if let Some((tip_instruction, tip_lamports, tip_account_str)) = resolved_tip {
            instructions.push(tip_instruction);
            (Some(tip_lamports), Some(tip_account_str))
        } else {
            (None, None)
        };
    if matches!(request.policy.mev_mode, MevMode::Reduced | MevMode::Secure) {
        apply_jitodontfront(&mut instructions, &owner_pubkey)?;
    }

    let label = match request.side {
        TradeSide::Buy => "raydium-amm-v4-buy",
        TradeSide::Sell => "raydium-amm-v4-sell",
    };
    instructions.push(build_uniqueness_memo_instruction(label)?);
    let blockhash = shared_warming_service()
        .latest_blockhash(&rpc_url, &request.policy.commitment)
        .await?
        .blockhash;
    let lookup_tables = crate::pump_native::load_shared_super_lookup_tables(&rpc_url).await?;
    let message = v0::Message::try_compile(&owner_pubkey, &instructions, &lookup_tables, blockhash)
        .map_err(|error| format!("Failed to compile Raydium AMM v4 transaction: {error}"))?;
    let lookup_tables_used = message
        .address_table_lookups
        .iter()
        .map(|lookup| lookup.account_key.to_string())
        .collect::<Vec<_>>();
    let transaction =
        VersionedTransaction::try_new(VersionedMessage::V0(message), &[&owner, &temp_wsol_account])
            .map_err(|error| format!("Failed to sign Raydium AMM v4 transaction: {error}"))?;
    let signature = transaction
        .signatures
        .first()
        .map(|value| value.to_string())
        .ok_or_else(|| "Raydium AMM v4 transaction did not include a signature.".to_string())?;
    let serialized = bincode::serialize(&transaction)
        .map_err(|error| format!("Failed to serialize Raydium AMM v4 transaction: {error}"))?;
    let serialized_base64 = BASE64.encode(serialized);
    compiled_transaction_signers::remember_compiled_transaction_signers(
        &serialized_base64,
        &[&temp_wsol_account],
    );

    Ok(CompiledAdapterTrade {
        transactions: vec![CompiledTransaction {
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
        }],
        primary_tx_index: 0,
        dependency_mode: TransactionDependencyMode::Independent,
        entry_preference_asset: None,
    })
}

fn build_raydium_amm_v4_selector(
    request: &TradeRuntimeRequest,
    pool: &RaydiumAmmV4PoolState,
) -> LifecycleAndCanonicalMarket {
    LifecycleAndCanonicalMarket {
        lifecycle: TradeLifecycle::PostMigration,
        family: TradeVenueFamily::RaydiumAmmV4,
        canonical_market_key: pool.pubkey.to_string(),
        quote_asset: PlannerQuoteAsset::Wsol,
        verification_source: PlannerVerificationSource::OnchainDerived,
        wrapper_action: match request.side {
            TradeSide::Buy => WrapperAction::RaydiumAmmV4WsolBuy,
            TradeSide::Sell => WrapperAction::RaydiumAmmV4WsolSell,
        },
        wrapper_accounts: vec![
            pool.pubkey.to_string(),
            pool.base_vault.to_string(),
            pool.quote_vault.to_string(),
            pool.open_orders.to_string(),
            pool.market.to_string(),
        ],
        market_subtype: Some("amm-v4".to_string()),
        direct_protocol_target: Some("raydium-amm-v4".to_string()),
        input_amount_hint: None,
        minimum_output_hint: None,
        runtime_bundle: Some(PlannerRuntimeBundle::RaydiumAmmV4(
            RaydiumAmmV4RuntimeBundle {
                pool: pool.pubkey.to_string(),
                authority: pool.authority.to_string(),
                base_mint: pool.base_mint.to_string(),
                quote_mint: pool.quote_mint.to_string(),
                base_vault: pool.base_vault.to_string(),
                quote_vault: pool.quote_vault.to_string(),
                open_orders: pool.open_orders.to_string(),
                target_orders: pool.target_orders.to_string(),
                market_program: pool.market_program.to_string(),
                market: pool.market.to_string(),
                market_bids: pool.market_bids.to_string(),
                market_asks: pool.market_asks.to_string(),
                market_event_queue: pool.market_event_queue.to_string(),
                market_base_vault: pool.market_base_vault.to_string(),
                market_quote_vault: pool.market_quote_vault.to_string(),
                market_vault_signer: pool.market_vault_signer.to_string(),
                mint_token_program: pool.mint_token_program.to_string(),
                trade_fee_numerator: pool.trade_fee_numerator,
                trade_fee_denominator: pool.trade_fee_denominator,
            },
        )),
    }
}

async fn fetch_raydium_amm_v4_pools_for_ordered_pair(
    rpc_url: &str,
    left_mint: &Pubkey,
    right_mint: &Pubkey,
    commitment: &str,
) -> Result<Vec<Pubkey>, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": "execution-engine-raydium-amm-v4-pools",
        "method": "getProgramAccounts",
        "params": [
            RAYDIUM_AMM_V4_PROGRAM_ID,
            {
                "commitment": commitment,
                "encoding": "base64",
                "filters": [
                    {
                        "dataSize": RAYDIUM_AMM_V4_ACCOUNT_LEN
                    },
                    {
                        "memcmp": {
                            "offset": RAYDIUM_AMM_V4_BASE_MINT_OFFSET,
                            "bytes": left_mint.to_string()
                        }
                    },
                    {
                        "memcmp": {
                            "offset": RAYDIUM_AMM_V4_QUOTE_MINT_OFFSET,
                            "bytes": right_mint.to_string()
                        }
                    }
                ]
            }
        ]
    });
    let response = raydium_amm_v4_http_client()
        .post(rpc_url)
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("Failed to fetch Raydium AMM v4 pools from RPC: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch Raydium AMM v4 pools from RPC: status {}.",
            response.status()
        ));
    }
    let parsed: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("Failed to parse Raydium AMM v4 pool RPC response: {error}"))?;
    if let Some(error) = parsed.get("error") {
        let message = error
            .get("message")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| error.to_string());
        return Err(format!("Raydium AMM v4 pool RPC query failed: {message}"));
    }
    let accounts = parsed
        .get("result")
        .and_then(|value| value.as_array())
        .ok_or_else(|| {
            "Raydium AMM v4 pool RPC response did not include result accounts.".to_string()
        })?;
    let mut pools = Vec::with_capacity(accounts.len());
    for account in accounts {
        let Some(pubkey) = account.get("pubkey").and_then(|value| value.as_str()) else {
            continue;
        };
        let Ok(pool_id) = Pubkey::from_str(pubkey) else {
            continue;
        };
        let Some(encoded_data) = account
            .get("account")
            .and_then(|value| value.get("data"))
            .and_then(encoded_account_data)
        else {
            continue;
        };
        let Ok(data) = BASE64.decode(encoded_data.trim()) else {
            continue;
        };
        if decode_raydium_amm_v4_pool_state(pool_id, &data).is_ok() {
            pools.push(pool_id);
        }
    }
    Ok(pools)
}

fn encoded_account_data(value: &serde_json::Value) -> Option<&str> {
    value.as_str().or_else(|| {
        value
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.as_str())
    })
}

fn decode_raydium_amm_v4_pool_state(
    pubkey: Pubkey,
    data: &[u8],
) -> Result<RaydiumAmmV4PoolState, String> {
    if data.len() < RAYDIUM_AMM_V4_ACCOUNT_LEN {
        return Err("Raydium AMM v4 pool account data was shorter than expected.".to_string());
    }
    let status = read_u64_at(data, 0, "raydium status")?;
    if status != 6 {
        return Err(format!(
            "Raydium AMM v4 pool account status {status} is not supported for trading."
        ));
    }
    Ok(RaydiumAmmV4PoolState {
        pubkey,
        authority: parse_pubkey(RAYDIUM_AMM_V4_AUTHORITY, "Raydium AMM v4 authority")?,
        base_mint: read_pubkey_at(data, RAYDIUM_AMM_V4_BASE_MINT_OFFSET, "raydium base mint")?,
        quote_mint: read_pubkey_at(data, RAYDIUM_AMM_V4_QUOTE_MINT_OFFSET, "raydium quote mint")?,
        base_vault: read_pubkey_at(data, RAYDIUM_AMM_V4_BASE_VAULT_OFFSET, "raydium base vault")?,
        quote_vault: read_pubkey_at(
            data,
            RAYDIUM_AMM_V4_QUOTE_VAULT_OFFSET,
            "raydium quote vault",
        )?,
        open_orders: read_pubkey_at(
            data,
            RAYDIUM_AMM_V4_OPEN_ORDERS_OFFSET,
            "raydium open orders",
        )?,
        target_orders: read_pubkey_at(
            data,
            RAYDIUM_AMM_V4_TARGET_ORDERS_OFFSET,
            "raydium target orders",
        )?,
        market_program: read_pubkey_at(
            data,
            RAYDIUM_AMM_V4_MARKET_PROGRAM_OFFSET,
            "raydium market program",
        )?,
        market: read_pubkey_at(data, RAYDIUM_AMM_V4_MARKET_OFFSET, "raydium market")?,
        market_bids: Pubkey::default(),
        market_asks: Pubkey::default(),
        market_event_queue: Pubkey::default(),
        market_base_vault: Pubkey::default(),
        market_quote_vault: Pubkey::default(),
        market_vault_signer: Pubkey::default(),
        mint_token_program: Pubkey::default(),
        trade_fee_numerator: read_u64_at(data, 18 * 8, "raydium trade fee numerator")?,
        trade_fee_denominator: read_u64_at(data, 19 * 8, "raydium trade fee denominator")?,
    })
}

async fn enrich_raydium_amm_v4_pool_state(
    rpc_url: &str,
    mut pool: RaydiumAmmV4PoolState,
    mint: &Pubkey,
    commitment: &str,
) -> Result<RaydiumAmmV4PoolState, String> {
    validate_raydium_amm_v4_pool_for_mint(&pool, mint)?;
    validate_raydium_amm_v4_market_program(&pool)?;
    let market_address = pool.market.to_string();
    let (market_result, mint_token_program_result) = tokio::join!(
        fetch_account_owner_and_data(rpc_url, &market_address, commitment),
        fetch_mint_token_program(rpc_url, mint, commitment)
    );
    let (market_owner, market_data) = market_result?
        .ok_or_else(|| format!("Raydium AMM v4 market {} was not found.", pool.market))?;
    if market_owner != pool.market_program {
        return Err(format!(
            "Raydium AMM v4 market {} is owned by {} rather than declared market program {}.",
            pool.market, market_owner, pool.market_program
        ));
    }
    validate_raydium_amm_v4_market_mints(&pool, &market_data)?;
    pool.market_bids = read_pubkey_at(
        &market_data,
        SERUM_MARKET_BIDS_OFFSET,
        "raydium market bids",
    )?;
    pool.market_asks = read_pubkey_at(
        &market_data,
        SERUM_MARKET_ASKS_OFFSET,
        "raydium market asks",
    )?;
    pool.market_event_queue = read_pubkey_at(
        &market_data,
        SERUM_MARKET_EVENT_QUEUE_OFFSET,
        "raydium market event queue",
    )?;
    pool.market_base_vault = read_pubkey_at(
        &market_data,
        SERUM_MARKET_BASE_VAULT_OFFSET,
        "raydium market base vault",
    )?;
    pool.market_quote_vault = read_pubkey_at(
        &market_data,
        SERUM_MARKET_QUOTE_VAULT_OFFSET,
        "raydium market quote vault",
    )?;
    let vault_nonce = read_u64_at(
        &market_data,
        SERUM_MARKET_VAULT_SIGNER_NONCE_OFFSET,
        "raydium market vault signer nonce",
    )?;
    pool.market_vault_signer = Pubkey::create_program_address(
        &[pool.market.as_ref(), &vault_nonce.to_le_bytes()],
        &pool.market_program,
    )
    .map_err(|error| format!("Failed to derive Raydium AMM v4 market vault signer: {error}"))?;
    pool.mint_token_program = mint_token_program_result?;
    validate_raydium_amm_v4_token_program(&pool)?;
    validate_raydium_amm_v4_market_accounts(&pool)?;
    Ok(pool)
}

fn raydium_amm_v4_pool_token_mint(pool: &RaydiumAmmV4PoolState) -> Result<Pubkey, String> {
    let wsol = wsol_mint()?;
    match (pool.base_mint == wsol, pool.quote_mint == wsol) {
        (false, true) => Ok(pool.base_mint),
        (true, false) => Ok(pool.quote_mint),
        _ => Err(format!(
            "Raydium AMM v4 pool {} is not an exact WSOL pair ({}/{}).",
            pool.pubkey, pool.base_mint, pool.quote_mint
        )),
    }
}

fn validate_raydium_amm_v4_pool_for_mint(
    pool: &RaydiumAmmV4PoolState,
    mint: &Pubkey,
) -> Result<(), String> {
    let resolved_mint = raydium_amm_v4_pool_token_mint(pool)?;
    if resolved_mint != *mint {
        return Err(format!(
            "Raydium AMM v4 pool {} trades mint {} rather than requested mint {}.",
            pool.pubkey, resolved_mint, mint
        ));
    }
    if pool.base_vault == Pubkey::default() || pool.quote_vault == Pubkey::default() {
        return Err(format!(
            "Raydium AMM v4 pool {} has an invalid vault address.",
            pool.pubkey
        ));
    }
    Ok(())
}

fn validate_raydium_amm_v4_market_program(pool: &RaydiumAmmV4PoolState) -> Result<(), String> {
    let openbook_program = parse_pubkey(
        RAYDIUM_AMM_V4_OPENBOOK_MARKET_PROGRAM_ID,
        "Raydium AMM v4 OpenBook market program",
    )?;
    if pool.market_program != openbook_program {
        return Err(format!(
            "Raydium AMM v4 pool {} uses unsupported market program {}; expected {}.",
            pool.pubkey, pool.market_program, openbook_program
        ));
    }
    Ok(())
}

fn validate_raydium_amm_v4_market_mints(
    pool: &RaydiumAmmV4PoolState,
    market_data: &[u8],
) -> Result<(), String> {
    let market_base_mint = read_pubkey_at(
        market_data,
        SERUM_MARKET_BASE_MINT_OFFSET,
        "raydium market base mint",
    )?;
    let market_quote_mint = read_pubkey_at(
        market_data,
        SERUM_MARKET_QUOTE_MINT_OFFSET,
        "raydium market quote mint",
    )?;
    if market_base_mint != pool.base_mint || market_quote_mint != pool.quote_mint {
        return Err(format!(
            "Raydium AMM v4 market {} mints {}/{} do not match pool {} mints {}/{}.",
            pool.market,
            market_base_mint,
            market_quote_mint,
            pool.pubkey,
            pool.base_mint,
            pool.quote_mint
        ));
    }
    Ok(())
}

fn validate_raydium_amm_v4_market_accounts(pool: &RaydiumAmmV4PoolState) -> Result<(), String> {
    let accounts = [
        ("open_orders", pool.open_orders),
        ("target_orders", pool.target_orders),
        ("market", pool.market),
        ("market_bids", pool.market_bids),
        ("market_asks", pool.market_asks),
        ("market_event_queue", pool.market_event_queue),
        ("market_base_vault", pool.market_base_vault),
        ("market_quote_vault", pool.market_quote_vault),
        ("market_vault_signer", pool.market_vault_signer),
    ];
    if let Some((label, _)) = accounts
        .iter()
        .find(|(_, account)| *account == Pubkey::default())
    {
        return Err(format!(
            "Raydium AMM v4 pool {} has an invalid {label} account.",
            pool.pubkey
        ));
    }
    Ok(())
}

fn validate_raydium_amm_v4_token_program(pool: &RaydiumAmmV4PoolState) -> Result<(), String> {
    let token_program = token_program_id()?;
    if pool.mint_token_program != token_program {
        return Err(format!(
            "Raydium AMM v4 pool {} uses unsupported mint token program {}; only SPL Token is supported.",
            pool.pubkey, pool.mint_token_program
        ));
    }
    Ok(())
}

fn pool_from_runtime_bundle(
    selector: &LifecycleAndCanonicalMarket,
) -> Result<RaydiumAmmV4PoolState, String> {
    let Some(PlannerRuntimeBundle::RaydiumAmmV4(bundle)) = selector.runtime_bundle.as_ref() else {
        return Err("Raydium AMM v4 selector was missing its runtime bundle.".to_string());
    };
    Ok(RaydiumAmmV4PoolState {
        pubkey: parse_pubkey(&bundle.pool, "Raydium AMM v4 pool")?,
        authority: parse_pubkey(&bundle.authority, "Raydium AMM v4 authority")?,
        base_mint: parse_pubkey(&bundle.base_mint, "Raydium AMM v4 base mint")?,
        quote_mint: parse_pubkey(&bundle.quote_mint, "Raydium AMM v4 quote mint")?,
        base_vault: parse_pubkey(&bundle.base_vault, "Raydium AMM v4 base vault")?,
        quote_vault: parse_pubkey(&bundle.quote_vault, "Raydium AMM v4 quote vault")?,
        open_orders: parse_pubkey(&bundle.open_orders, "Raydium AMM v4 open orders")?,
        target_orders: parse_pubkey(&bundle.target_orders, "Raydium AMM v4 target orders")?,
        market_program: parse_pubkey(&bundle.market_program, "Raydium AMM v4 market program")?,
        market: parse_pubkey(&bundle.market, "Raydium AMM v4 market")?,
        market_bids: parse_pubkey(&bundle.market_bids, "Raydium AMM v4 market bids")?,
        market_asks: parse_pubkey(&bundle.market_asks, "Raydium AMM v4 market asks")?,
        market_event_queue: parse_pubkey(
            &bundle.market_event_queue,
            "Raydium AMM v4 market event queue",
        )?,
        market_base_vault: parse_pubkey(
            &bundle.market_base_vault,
            "Raydium AMM v4 market base vault",
        )?,
        market_quote_vault: parse_pubkey(
            &bundle.market_quote_vault,
            "Raydium AMM v4 market quote vault",
        )?,
        market_vault_signer: parse_pubkey(
            &bundle.market_vault_signer,
            "Raydium AMM v4 market vault signer",
        )?,
        mint_token_program: parse_pubkey(
            &bundle.mint_token_program,
            "Raydium AMM v4 mint token program",
        )?,
        trade_fee_numerator: bundle.trade_fee_numerator,
        trade_fee_denominator: bundle.trade_fee_denominator,
    })
}

#[derive(Debug, Clone)]
struct CachedRaydiumAmmV4QuoteSnapshot {
    fetched_at: Instant,
    pool: RaydiumAmmV4PoolState,
    base_reserve: u64,
    quote_reserve: u64,
}

fn raydium_amm_v4_quote_snapshot_cache()
-> &'static tokio::sync::Mutex<HashMap<String, CachedRaydiumAmmV4QuoteSnapshot>> {
    static CACHE: OnceLock<tokio::sync::Mutex<HashMap<String, CachedRaydiumAmmV4QuoteSnapshot>>> =
        OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

fn raydium_amm_v4_quote_snapshot_ttl(selector: &LifecycleAndCanonicalMarket) -> Duration {
    match selector.lifecycle {
        TradeLifecycle::PreMigration => Duration::from_millis(1_500),
        TradeLifecycle::PostMigration => Duration::from_millis(3_000),
    }
}

fn raydium_amm_v4_quote_snapshot_key(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    commitment: &str,
) -> String {
    format!(
        "rpc={}|cmt={}|family={}|market={}|quote={}|mint={}",
        rpc_url,
        commitment,
        selector.family.label(),
        selector.canonical_market_key,
        selector.quote_asset.label(),
        mint
    )
}

fn quote_raydium_amm_v4_snapshot(
    snapshot: &CachedRaydiumAmmV4QuoteSnapshot,
    mint_pubkey: &Pubkey,
    token_amount_raw: u64,
) -> Result<u64, String> {
    validate_raydium_amm_v4_pool_for_mint(&snapshot.pool, mint_pubkey)?;
    let wsol = wsol_mint()?;
    let (input_reserve, output_reserve) =
        if *mint_pubkey == snapshot.pool.base_mint && wsol == snapshot.pool.quote_mint {
            (snapshot.base_reserve, snapshot.quote_reserve)
        } else if *mint_pubkey == snapshot.pool.quote_mint && wsol == snapshot.pool.base_mint {
            (snapshot.quote_reserve, snapshot.base_reserve)
        } else {
            return Err(format!(
                "Raydium AMM v4 pool {} is not a WSOL pair for mint {}.",
                snapshot.pool.pubkey, mint_pubkey
            ));
        };
    raydium_amm_v4_quote_exact_in(
        token_amount_raw,
        input_reserve,
        output_reserve,
        snapshot.pool.trade_fee_numerator,
        snapshot.pool.trade_fee_denominator,
    )
}

pub(crate) async fn quote_raydium_amm_v4_holding_value_sol(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    token_amount_raw: u64,
    commitment: &str,
) -> Result<u64, String> {
    quote_raydium_amm_v4_holding_value_sol_with_cache(
        rpc_url,
        selector,
        mint,
        token_amount_raw,
        commitment,
        true,
    )
    .await
}

pub(crate) async fn quote_raydium_amm_v4_holding_value_sol_fresh(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    token_amount_raw: u64,
    commitment: &str,
) -> Result<u64, String> {
    quote_raydium_amm_v4_holding_value_sol_with_cache(
        rpc_url,
        selector,
        mint,
        token_amount_raw,
        commitment,
        false,
    )
    .await
}

async fn quote_raydium_amm_v4_holding_value_sol_with_cache(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    token_amount_raw: u64,
    commitment: &str,
    use_cache: bool,
) -> Result<u64, String> {
    if token_amount_raw == 0 {
        return Ok(0);
    }
    let mint_pubkey = parse_pubkey(mint, "Raydium AMM v4 quote mint")?;
    let cache_key = raydium_amm_v4_quote_snapshot_key(rpc_url, selector, mint, commitment);
    if use_cache {
        let cache_ttl = raydium_amm_v4_quote_snapshot_ttl(selector);
        let cache = raydium_amm_v4_quote_snapshot_cache().lock().await;
        if let Some(snapshot) = cache.get(&cache_key)
            && snapshot.fetched_at.elapsed() <= cache_ttl
        {
            return quote_raydium_amm_v4_snapshot(snapshot, &mint_pubkey, token_amount_raw);
        }
    }
    let pool = if selector.runtime_bundle.is_some() {
        pool_from_runtime_bundle(selector)?
    } else {
        let pool_id = parse_pubkey(
            &selector.canonical_market_key,
            "Raydium AMM v4 canonical market",
        )?;
        let data = fetch_account_data(rpc_url, &pool_id.to_string(), commitment).await?;
        let decoded = decode_raydium_amm_v4_pool_state(pool_id, &data)?;
        enrich_raydium_amm_v4_pool_state(rpc_url, decoded, &mint_pubkey, commitment).await?
    };
    validate_raydium_amm_v4_pool_for_mint(&pool, &mint_pubkey)?;
    let reserve_accounts = vec![pool.base_vault.to_string(), pool.quote_vault.to_string()];
    let reserve_datas = fetch_multiple_account_data(rpc_url, &reserve_accounts, commitment).await?;
    if reserve_datas.len() != reserve_accounts.len() {
        return Err(format!(
            "Raydium AMM v4 reserve fetch returned {} accounts for {} requested accounts.",
            reserve_datas.len(),
            reserve_accounts.len()
        ));
    }
    let snapshot = CachedRaydiumAmmV4QuoteSnapshot {
        fetched_at: Instant::now(),
        pool,
        base_reserve: read_token_account_amount(reserve_datas[0].as_deref().ok_or_else(|| {
            format!(
                "Raydium AMM v4 base reserve account {} was not found.",
                reserve_accounts[0]
            )
        })?)?,
        quote_reserve: read_token_account_amount(reserve_datas[1].as_deref().ok_or_else(
            || {
                format!(
                    "Raydium AMM v4 quote reserve account {} was not found.",
                    reserve_accounts[1]
                )
            },
        )?)?,
    };
    if use_cache {
        let mut cache = raydium_amm_v4_quote_snapshot_cache().lock().await;
        cache.insert(cache_key, snapshot.clone());
        if cache.len() > 256 {
            cache.retain(|_, entry| entry.fetched_at.elapsed() <= Duration::from_secs(30));
        }
    }
    quote_raydium_amm_v4_snapshot(&snapshot, &mint_pubkey, token_amount_raw)
}

fn raydium_amm_v4_swap_sides(
    pool: &RaydiumAmmV4PoolState,
    input_mint: &Pubkey,
    output_mint: &Pubkey,
    base_reserve: u64,
    quote_reserve: u64,
) -> Result<(Pubkey, Pubkey, u64, u64), String> {
    if *input_mint == pool.base_mint && *output_mint == pool.quote_mint {
        Ok((
            pool.base_vault,
            pool.quote_vault,
            base_reserve,
            quote_reserve,
        ))
    } else if *input_mint == pool.quote_mint && *output_mint == pool.base_mint {
        Ok((
            pool.quote_vault,
            pool.base_vault,
            quote_reserve,
            base_reserve,
        ))
    } else {
        Err("Raydium AMM v4 swap mints do not match the selected pool.".to_string())
    }
}

fn raydium_amm_v4_quote_exact_in(
    amount_in: u64,
    input_reserve: u64,
    output_reserve: u64,
    fee_numerator: u64,
    fee_denominator: u64,
) -> Result<u64, String> {
    if amount_in == 0 {
        return Ok(0);
    }
    if input_reserve == 0 || output_reserve == 0 {
        return Err("Raydium AMM v4 reserves are empty.".to_string());
    }
    if fee_denominator == 0 || fee_numerator >= fee_denominator {
        return Err("Raydium AMM v4 fee configuration is invalid.".to_string());
    }
    let amount_after_fee = (u128::from(amount_in)
        * u128::from(fee_denominator.saturating_sub(fee_numerator)))
        / u128::from(fee_denominator);
    let numerator = amount_after_fee.saturating_mul(u128::from(output_reserve));
    let denominator = u128::from(input_reserve).saturating_add(amount_after_fee);
    if denominator == 0 {
        return Ok(0);
    }
    Ok((numerator / denominator).min(u128::from(u64::MAX)) as u64)
}

async fn resolve_raydium_amm_v4_sell_input_amount(
    sell_intent: &RuntimeSellIntent,
    wallet_key: &str,
    owner: &str,
    mint: &str,
    mint_decimals: u8,
    input_reserve: u64,
    output_reserve: u64,
    fee_numerator: u64,
    fee_denominator: u64,
) -> Result<u64, String> {
    let balance = crate::wallet_token_cache::fetch_token_balance_with_cache(
        Some(wallet_key),
        owner,
        mint,
        mint_decimals,
    )
    .await?;
    if balance.amount_raw == 0 {
        return Err("You have 0 tokens.".to_string());
    }
    let amount = match sell_intent {
        RuntimeSellIntent::Percent(value) => {
            let percent_bps = u128::from(parse_percent_to_bps(value)?);
            ((u128::from(balance.amount_raw) * percent_bps) / 10_000u128).min(u128::from(u64::MAX))
                as u64
        }
        RuntimeSellIntent::SolOutput(value) => {
            let desired_output = parse_decimal_units(value, 9, "sellOutputSol")?;
            crate::sell_target_sizing::choose_target_sized_token_amount(
                balance.amount_raw,
                desired_output,
                |amount| {
                    raydium_amm_v4_quote_exact_in(
                        amount,
                        input_reserve,
                        output_reserve,
                        fee_numerator,
                        fee_denominator,
                    )
                    .and_then(crate::sell_target_sizing::net_sol_after_wrapper_fee)
                },
            )?
        }
    };
    if amount == 0 {
        return Err("Sell amount resolves to zero tokens.".to_string());
    }
    if amount > balance.amount_raw {
        return Err(format!(
            "Wallet balance is too small for the requested Raydium AMM v4 sell. Need {amount}, have {}.",
            balance.amount_raw
        ));
    }
    Ok(amount)
}

fn build_raydium_amm_v4_swap_base_in_instruction(
    pool: &RaydiumAmmV4PoolState,
    _input_vault: &Pubkey,
    _output_vault: &Pubkey,
    user_source: &Pubkey,
    user_destination: &Pubkey,
    user_owner: &Pubkey,
    amount_in: u64,
    min_out: u64,
) -> Result<Instruction, String> {
    let mut data = Vec::with_capacity(17);
    data.push(9u8);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_out.to_le_bytes());
    Ok(Instruction {
        program_id: raydium_amm_v4_program_id()?,
        accounts: vec![
            AccountMeta::new_readonly(token_program_id()?, false),
            AccountMeta::new(pool.pubkey, false),
            AccountMeta::new_readonly(pool.authority, false),
            AccountMeta::new(pool.open_orders, false),
            AccountMeta::new(pool.target_orders, false),
            AccountMeta::new(pool.base_vault, false),
            AccountMeta::new(pool.quote_vault, false),
            AccountMeta::new_readonly(pool.market_program, false),
            AccountMeta::new(pool.market, false),
            AccountMeta::new(pool.market_bids, false),
            AccountMeta::new(pool.market_asks, false),
            AccountMeta::new(pool.market_event_queue, false),
            AccountMeta::new(pool.market_base_vault, false),
            AccountMeta::new(pool.market_quote_vault, false),
            AccountMeta::new_readonly(pool.market_vault_signer, false),
            AccountMeta::new(*user_source, false),
            AccountMeta::new(*user_destination, false),
            AccountMeta::new_readonly(*user_owner, true),
        ],
        data,
    })
}

fn build_wrapped_sol_open_instructions(
    owner: &Pubkey,
    wrapped_account: &Pubkey,
    lamports: u64,
    sync_after_initialize: bool,
) -> Result<Vec<Instruction>, String> {
    let token_program = token_program_id()?;
    let mut instructions = vec![
        create_account(
            owner,
            wrapped_account,
            lamports,
            SPL_TOKEN_ACCOUNT_LEN,
            &token_program,
        ),
        initialize_account3(&token_program, wrapped_account, &wsol_mint()?, owner).map_err(
            |error| format!("Failed to build wrapped SOL initialize instruction: {error}"),
        )?,
    ];
    if sync_after_initialize {
        instructions.push(
            sync_native(&token_program, wrapped_account)
                .map_err(|error| format!("Failed to build syncNative instruction: {error}"))?,
        );
    }
    Ok(instructions)
}

fn build_wrapped_sol_close_instruction(
    owner: &Pubkey,
    wrapped_account: &Pubkey,
) -> Result<Instruction, String> {
    close_spl_account(&token_program_id()?, wrapped_account, owner, owner, &[])
        .map_err(|error| format!("Failed to build WSOL close instruction: {error}"))
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

async fn fetch_mint_token_program(
    rpc_url: &str,
    mint: &Pubkey,
    commitment: &str,
) -> Result<Pubkey, String> {
    let owner = fetch_account_owner_and_data(rpc_url, &mint.to_string(), commitment)
        .await?
        .map(|(owner, _)| owner)
        .ok_or_else(|| format!("Mint account {mint} was not found."))?;
    let token_program = token_program_id()?;
    if owner != token_program {
        return Err(format!(
            "Raydium AMM v4 mint {mint} is owned by unsupported token program {owner}; only SPL Token is supported."
        ));
    }
    Ok(owner)
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

fn read_mint_decimals(data: &[u8]) -> Result<u8, String> {
    if data.len() < 45 {
        return Err("Mint account data was shorter than expected (decimals).".to_string());
    }
    Ok(data[44])
}

fn read_pubkey_at(data: &[u8], offset: usize, label: &str) -> Result<Pubkey, String> {
    let end = offset
        .checked_add(32)
        .ok_or_else(|| format!("{label} offset overflowed"))?;
    let bytes: [u8; 32] = data
        .get(offset..end)
        .ok_or_else(|| format!("{label} bytes were missing"))?
        .try_into()
        .map_err(|_| format!("{label} bytes were invalid"))?;
    Ok(Pubkey::new_from_array(bytes))
}

fn read_u64_at(data: &[u8], offset: usize, label: &str) -> Result<u64, String> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| format!("{label} offset overflowed"))?;
    let bytes: [u8; 8] = data
        .get(offset..end)
        .ok_or_else(|| format!("{label} bytes were missing"))?
        .try_into()
        .map_err(|_| format!("{label} bytes were invalid"))?;
    Ok(u64::from_le_bytes(bytes))
}

fn token_program_id() -> Result<Pubkey, String> {
    parse_pubkey(TOKEN_PROGRAM_ID, "token program")
}

fn wsol_mint() -> Result<Pubkey, String> {
    parse_pubkey(WSOL_MINT, "WSOL mint")
}

fn parse_pubkey(value: &str, label: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value.trim()).map_err(|error| format!("Invalid {label}: {error}"))
}

fn apply_sell_side_slippage(value: u64, slippage_bps: u16) -> u64 {
    let minimum = ((u128::from(value)
        * u128::from(10_000u64.saturating_sub(u64::from(slippage_bps))))
        / 10_000u128)
        .min(u128::from(u64::MAX)) as u64;
    if value > 0 && minimum == 0 {
        1
    } else {
        minimum
    }
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

fn parse_percent_to_bps(value: &str) -> Result<u16, String> {
    let hundredths = parse_decimal_scaled(value, 2, "sellPercent")?;
    if hundredths > 10_000 {
        return Err("Sell percent must be between 0 and 100.".to_string());
    }
    u16::try_from(hundredths).map_err(|_| "Sell percent is out of range.".to_string())
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

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pool() -> RaydiumAmmV4PoolState {
        RaydiumAmmV4PoolState {
            pubkey: Pubkey::new_unique(),
            authority: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: wsol_mint().expect("wsol"),
            base_vault: Pubkey::new_unique(),
            quote_vault: Pubkey::new_unique(),
            open_orders: Pubkey::new_unique(),
            target_orders: Pubkey::new_unique(),
            market_program: parse_pubkey(
                RAYDIUM_AMM_V4_OPENBOOK_MARKET_PROGRAM_ID,
                "Raydium AMM v4 OpenBook market program",
            )
            .expect("openbook market program"),
            market: Pubkey::new_unique(),
            market_bids: Pubkey::new_unique(),
            market_asks: Pubkey::new_unique(),
            market_event_queue: Pubkey::new_unique(),
            market_base_vault: Pubkey::new_unique(),
            market_quote_vault: Pubkey::new_unique(),
            market_vault_signer: Pubkey::new_unique(),
            mint_token_program: token_program_id().expect("token program"),
            trade_fee_numerator: 25,
            trade_fee_denominator: 10_000,
        }
    }

    fn write_pubkey(data: &mut [u8], offset: usize, pubkey: Pubkey) {
        data[offset..offset + 32].copy_from_slice(pubkey.as_ref());
    }

    #[test]
    fn market_mint_validation_rejects_mismatched_market() {
        let pool = sample_pool();
        let mut market_data = vec![0u8; SERUM_MARKET_ASKS_OFFSET + 32];
        write_pubkey(
            &mut market_data,
            SERUM_MARKET_BASE_MINT_OFFSET,
            Pubkey::new_unique(),
        );
        write_pubkey(
            &mut market_data,
            SERUM_MARKET_QUOTE_MINT_OFFSET,
            pool.quote_mint,
        );

        let error = validate_raydium_amm_v4_market_mints(&pool, &market_data).unwrap_err();
        assert!(error.contains("do not match pool"));
    }

    #[test]
    fn token_program_validation_rejects_token_2022_like_mints() {
        let mut pool = sample_pool();
        pool.mint_token_program = Pubkey::new_unique();

        let error = validate_raydium_amm_v4_token_program(&pool).unwrap_err();
        assert!(error.contains("unsupported mint token program"));
    }

    #[test]
    fn market_program_validation_rejects_unexpected_programs() {
        let mut pool = sample_pool();
        pool.market_program = Pubkey::new_unique();

        let error = validate_raydium_amm_v4_market_program(&pool).unwrap_err();
        assert!(error.contains("unsupported market program"));
    }

    #[test]
    fn swap_instruction_uses_full_raydium_v4_account_layout() {
        let pool = sample_pool();
        let user_source = Pubkey::new_unique();
        let user_destination = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let instruction = build_raydium_amm_v4_swap_base_in_instruction(
            &pool,
            &pool.base_vault,
            &pool.quote_vault,
            &user_source,
            &user_destination,
            &owner,
            990_000_000,
            1,
        )
        .expect("raydium amm v4 instruction");

        assert_eq!(instruction.program_id, raydium_amm_v4_program_id().unwrap());
        assert_eq!(instruction.accounts.len(), 18);
        assert_eq!(instruction.accounts[15].pubkey, user_source);
        assert_eq!(instruction.accounts[16].pubkey, user_destination);
        assert_eq!(instruction.accounts[17].pubkey, owner);
        assert!(instruction.accounts[17].is_signer);
        assert_eq!(instruction.data[0], 9);
    }

    #[test]
    fn jitodontfront_does_not_mutate_raydium_swap_accounts() {
        let pool = sample_pool();
        let user_source = Pubkey::new_unique();
        let user_destination = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let swap = build_raydium_amm_v4_swap_base_in_instruction(
            &pool,
            &pool.base_vault,
            &pool.quote_vault,
            &user_source,
            &user_destination,
            &owner,
            990_000_000,
            1,
        )
        .expect("raydium amm v4 instruction");
        let original_accounts = swap.accounts.clone();
        let mut instructions = vec![swap];

        apply_jitodontfront(&mut instructions, &owner).expect("jitodontfront");

        assert_eq!(instructions.len(), 2);
        assert_eq!(instructions[1].accounts, original_accounts);
        assert_eq!(instructions[1].accounts.len(), 18);
        assert_eq!(
            instructions[0].program_id,
            solana_system_interface::program::ID
        );
        assert_eq!(instructions[0].accounts.len(), 3);
        assert_eq!(
            instructions[0].accounts[2].pubkey,
            parse_pubkey(JITODONTFRONT_ACCOUNT, "jitodontfront").unwrap()
        );
    }
}
