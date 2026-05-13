#![allow(dead_code, non_snake_case)]

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shared_execution_routing::alt_manifest::{
    PUMP_APR28_FEE_RECIPIENTS, lookup_table_address_content_hash, shared_alt_manifest_entries,
};
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_sdk::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{AddressLookupTableAccount, VersionedMessage, v0},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use solana_system_interface::{instruction::transfer, program as system_program};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::{
    collections::HashMap,
    fs,
    str::FromStr,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::{join, task::JoinSet, time::sleep};

use crate::{
    bonk_native::{TrustedRaydiumClmmSwap, build_trusted_raydium_clmm_swap_exact_in},
    compiled_transaction_signers,
    config::{
        NormalizedConfig, NormalizedExecution, NormalizedRecipient,
        configured_default_agent_setup_compute_unit_limit,
        configured_default_dev_auto_sell_compute_unit_limit,
        configured_default_follow_up_compute_unit_limit,
        configured_default_launch_compute_unit_limit,
        configured_default_sniper_buy_compute_unit_limit, has_launch_follow_up,
        launch_follow_up_label,
    },
    paths,
    report::{LaunchReport, build_report, render_report},
    rpc::{
        COMPILE_BLOCKHASH_MIN_REMAINING_BLOCKS, CompiledTransaction,
        SOLANA_TRANSACTION_PACKET_LIMIT, fetch_account_data, fetch_account_data_with_owner,
        fetch_account_exists, fetch_latest_blockhash_cached,
        fetch_latest_blockhash_cached_with_prime, fetch_multiple_account_data,
        fetch_multiple_account_exists,
    },
    transport::TransportPlan,
    vanity_pool::{
        VanityLaunchpad, VanityReservation, append_vanity_report_note, reserve_vanity_mint,
    },
    wallet::read_keypair_bytes,
    wrapper_compile::{
        ABI_VERSION as WRAPPER_ABI_VERSION, EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT,
        EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT, ExecutePumpBondingV2Request,
        ExecuteSwapRouteRequest, PumpBondingV2QuoteFeeMode, PumpBondingV2Side,
        SWAP_ROUTE_NO_PATCH_OFFSET, SwapLegInputSource, SwapRouteDirection, SwapRouteFeeMode,
        SwapRouteLeg, SwapRouteMode, SwapRouteSettlement,
        build_execute_pump_bonding_v2_instruction, build_execute_swap_route_instruction,
        estimate_sol_in_fee_lamports, route_wsol_pda, wrapper_fee_vault, wrapper_token_program_id,
        wrapper_wsol_mint,
    },
};

const JITODONTFRONT_ACCOUNT: &str = "jitodontfront111111111111111111111111111111";
const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMP_AMM_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const MAYHEM_PROGRAM_ID: &str = "MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e";
const PUMP_FEE_PROGRAM_ID: &str = "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ";
const PUMP_AGENT_PAYMENTS_PROGRAM_ID: &str = "AgenTMiC2hvxGebTsgmsD4HHBa8WEcqGFf87iwRRxLo7";
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT: u64 = 1_000_000;
const TOKEN_DECIMALS: u32 = 6;
const GLOBAL_ACCOUNT_DISCRIMINATOR_BYTES: usize = 8;
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const USDT_MINT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
const USD1_MINT: &str = "USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB";
const RAYDIUM_SOL_USDC_POOL: &str = "3ucNos4NbumPLZNWztqGHNFFgkHeRMBQAVemeeomsUxv";
const SHARED_SUPER_LOOKUP_TABLE: &str = "7CaMLcAuSskoeN7HoRwZjsSthU8sMwKqxtXkyMiMjuc";
const PLATFORM_GITHUB: u8 = 2;
const DEFAULT_LOOKUP_TABLES: [&str; 1] = [SHARED_SUPER_LOOKUP_TABLE];
const DEFAULT_LAUNCH_LOOKUP_TABLE_PROFILES: [[&str; 1]; 1] = [[SHARED_SUPER_LOOKUP_TABLE]];
const DEFAULT_FOLLOW_UP_LOOKUP_TABLE_PROFILES: [[&str; 1]; 1] = [[SHARED_SUPER_LOOKUP_TABLE]];
const LOOKUP_TABLE_CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct NativePumpArtifacts {
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

#[derive(Debug, Clone, Default)]
pub struct NativeCompileTimings {
    pub launch_creator_prep_ms: u128,
    pub alt_load_ms: u128,
    pub blockhash_fetch_ms: u128,
    pub global_fetch_ms: Option<u128>,
    pub follow_up_prep_ms: Option<u128>,
    pub tx_serialize_ms: u128,
    pub launch_serialize_ms: Option<u128>,
    pub follow_up_serialize_ms: Option<u128>,
    pub tip_serialize_ms: Option<u128>,
}

#[allow(dead_code, non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchQuote {
    pub mode: String,
    pub input: String,
    pub estimatedTokens: String,
    pub estimatedSol: String,
    #[serde(default)]
    pub estimatedQuoteAmount: String,
    #[serde(default)]
    pub quoteAsset: String,
    #[serde(default)]
    pub quoteAssetLabel: String,
    pub estimatedSupplyPercent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PumpMarketSnapshot {
    pub mint: String,
    pub creator: String,
    pub virtualTokenReserves: u64,
    pub virtualSolReserves: u64,
    pub realTokenReserves: u64,
    pub realSolReserves: u64,
    pub tokenTotalSupply: u64,
    pub complete: bool,
    pub marketCapLamports: u64,
    pub marketCapSol: String,
    #[serde(default)]
    pub quoteAsset: String,
    #[serde(default)]
    pub quoteAssetLabel: String,
}

pub fn supports_native_pump_compile(config: &NormalizedConfig) -> bool {
    if config.launchpad != "pump" {
        return false;
    }
    if config.mode != "regular"
        && config.mode != "cashback"
        && config.mode != "agent-unlocked"
        && config.mode != "agent-custom"
        && config.mode != "agent-locked"
    {
        return false;
    }
    if config.feeSharing.generateLaterSetup
        && config.feeSharing.recipients.iter().any(|entry| {
            (!entry.githubUserId.is_empty() || !entry.githubUsername.is_empty())
                && entry.githubUserId.is_empty()
        })
    {
        return false;
    }
    if config.creatorFee.mode == "github" && config.creatorFee.githubUserId.is_empty() {
        return false;
    }
    if config.mode == "agent-custom"
        && config.agent.feeRecipients.iter().any(|entry| {
            (!entry.githubUserId.is_empty() || !entry.githubUsername.is_empty())
                && entry.githubUserId.is_empty()
        })
    {
        return false;
    }
    true
}

pub async fn try_compile_native_pump(
    rpc_url: &str,
    config: &NormalizedConfig,
    transport_plan: &TransportPlan,
    wallet_secret: &[u8],
    built_at: String,
    creator_public_key: String,
    config_path: Option<String>,
    allow_queued_vanity: bool,
    launch_blockhash_prime: Option<(String, u64)>,
) -> Result<Option<NativePumpArtifacts>, String> {
    if !supports_native_pump_compile(config) {
        return Ok(None);
    }

    let creator_keypair = keypair_from_secret_bytes(wallet_secret)?;
    let creator = creator_keypair.pubkey();
    let agent_authority = resolve_agent_authority(config, &creator)?;
    let resolved_mint =
        resolve_mint_keypair_for_launch(rpc_url, config, allow_queued_vanity).await?;
    let mint_keypair = resolved_mint.keypair;
    let mint = mint_keypair.pubkey();
    ensure_vanity_mint_unused(rpc_url, resolved_mint.requires_unused_check, &mint).await?;
    let vanity_reservation = resolved_mint.vanity_reservation;
    let separate_tip_transaction =
        transport_plan.separateTipTransaction && config.tx.jitoTipLamports > 0;
    let has_follow_up_transaction = has_launch_follow_up(config);
    let launch_creator_future = async {
        let started = Instant::now();
        let resolved =
            resolve_launch_creator_and_pre_instructions(rpc_url, config, &creator).await?;
        Ok::<_, String>((started.elapsed().as_millis(), resolved))
    };
    let lookup_tables_future = async {
        let started = Instant::now();
        let tables = load_lookup_table_accounts(rpc_url, config).await?;
        Ok::<_, String>((started.elapsed().as_millis(), tables))
    };
    let rpc_for_blockhash = rpc_url.to_string();
    let blockhash_commitment = config.execution.commitment.clone();
    let blockhash_prime = launch_blockhash_prime;
    let blockhash_future = async move {
        let started = Instant::now();
        let blockhash = fetch_latest_blockhash_cached_with_prime(
            &rpc_for_blockhash,
            &blockhash_commitment,
            blockhash_prime,
            COMPILE_BLOCKHASH_MIN_REMAINING_BLOCKS,
        )
        .await?;
        Ok::<_, String>((started.elapsed().as_millis(), blockhash))
    };
    let global_future = async {
        if config.devBuy.is_some() {
            let started = Instant::now();
            let global = fetch_global_state_cached(rpc_url).await?;
            Ok::<_, String>(Some((started.elapsed().as_millis(), global)))
        } else {
            Ok::<_, String>(None)
        }
    };
    let (launch_creator_result, lookup_tables_result, blockhash_result, global_result) = join!(
        launch_creator_future,
        lookup_tables_future,
        blockhash_future,
        global_future
    );
    let (launch_creator_prep_ms, (launch_creator, launch_pre_instructions)) =
        launch_creator_result?;
    let (alt_load_ms, lookup_tables) = lookup_tables_result?;
    let (blockhash_fetch_ms, (blockhash, last_valid_block_height)) = blockhash_result?;
    let global_result = global_result?;
    let global_fetch_ms = global_result.as_ref().map(|(elapsed_ms, _)| *elapsed_ms);
    let global = global_result.map(|(_, global)| global);
    let mut compile_timings = NativeCompileTimings {
        launch_creator_prep_ms,
        alt_load_ms,
        blockhash_fetch_ms,
        global_fetch_ms,
        follow_up_prep_ms: None,
        tx_serialize_ms: 0,
        launch_serialize_ms: None,
        follow_up_serialize_ms: None,
        tip_serialize_ms: None,
    };
    let tx_format = select_native_format(&config.execution.txFormat, !lookup_tables.is_empty())?;
    let creation_compute_unit_price_micro_lamports =
        effective_creation_compute_unit_price_micro_lamports(
            config,
            transport_plan,
            has_follow_up_transaction,
        );
    let single_bundle_tip_last_tx = transport_plan.transportType == "hellomoon-bundle";
    let launch_tip_lamports = if transport_plan.requiresInlineTip || !separate_tip_transaction {
        if single_bundle_tip_last_tx && has_follow_up_transaction {
            0
        } else {
            config.tx.jitoTipLamports
        }
    } else {
        0
    };
    let launch_tip_account = if launch_tip_lamports > 0 && !config.tx.jitoTipAccount.is_empty() {
        config.tx.jitoTipAccount.clone()
    } else {
        String::new()
    };
    let follow_up_tip_lamports = if transport_plan.requiresInlineTip {
        config.tx.jitoTipLamports
    } else {
        0
    };
    let follow_up_tip_account =
        if follow_up_tip_lamports > 0 && !config.tx.jitoTipAccount.is_empty() {
            config.tx.jitoTipAccount.clone()
        } else {
            String::new()
        };

    let launch_tx_config = NativeTxConfig {
        compute_unit_limit: configured_launch_compute_unit_limit(config)?,
        compute_unit_price_micro_lamports: creation_compute_unit_price_micro_lamports,
        jito_tip_lamports: launch_tip_lamports,
        jito_tip_account: launch_tip_account,
    };

    let launch_lookup_table_variants =
        lookup_table_variants_for_transaction("launch", config, &lookup_tables);
    let mut launch_instructions = launch_pre_instructions;
    launch_instructions.extend(build_launch_instructions(
        config,
        mint,
        creator,
        launch_creator,
        agent_authority.as_ref(),
        global.as_ref(),
    )?);
    let launch_tx_instructions = with_tx_settings(
        launch_instructions,
        &launch_tx_config,
        &creator,
        config.execution.jitodontfront,
    )?;
    let launch_serialize_started = Instant::now();
    let (launch_compiled, launch_metrics) = compile_transaction_with_metrics(
        "launch",
        tx_format,
        &blockhash,
        last_valid_block_height,
        &creator_keypair,
        Some(&mint_keypair),
        launch_tx_instructions.clone(),
        &launch_tx_config,
        &launch_lookup_table_variants,
    )?;
    compile_timings.tx_serialize_ms += launch_serialize_started.elapsed().as_millis();
    compile_timings.launch_serialize_ms = Some(launch_serialize_started.elapsed().as_millis());
    let mut launch_metrics = launch_metrics;
    launch_metrics.warnings.extend(transaction_size_diagnostics(
        &launch_tx_instructions,
        &launch_tx_config,
    ));
    let mut compiled_transactions = vec![launch_compiled.clone()];
    let mut creation_transactions = vec![launch_compiled];
    let mut deferred_setup_transactions = vec![];
    let mut compile_metrics = vec![launch_metrics];
    let mut instruction_summaries = vec![summarize_instructions(&launch_tx_instructions)];

    if let Some(follow_up_label) = native_follow_up_label(config) {
        let follow_up_lookup_table_variants =
            lookup_table_variants_for_transaction(follow_up_label, config, &lookup_tables);
        let follow_up_prep_started = Instant::now();
        let follow_up_instructions = build_native_follow_up_instructions(
            rpc_url,
            config,
            mint,
            creator,
            agent_authority.as_ref(),
        )
        .await?;
        compile_timings.follow_up_prep_ms = Some(follow_up_prep_started.elapsed().as_millis());
        let follow_up_tx_instructions = with_tx_settings(
            follow_up_instructions,
            &NativeTxConfig {
                compute_unit_limit: configured_follow_up_compute_unit_limit(
                    config,
                    follow_up_label,
                )?,
                compute_unit_price_micro_lamports:
                    effective_follow_up_compute_unit_price_micro_lamports(config, transport_plan),
                jito_tip_lamports: follow_up_tip_lamports,
                jito_tip_account: follow_up_tip_account.clone(),
            },
            &creator,
            config.execution.jitodontfront,
        )?;
        let follow_up_serialize_started = Instant::now();
        let (follow_up_compiled, follow_up_metrics) = compile_transaction_with_metrics(
            follow_up_label,
            tx_format,
            &blockhash,
            last_valid_block_height,
            &creator_keypair,
            None,
            follow_up_tx_instructions.clone(),
            &NativeTxConfig {
                compute_unit_limit: configured_follow_up_compute_unit_limit(
                    config,
                    follow_up_label,
                )?,
                compute_unit_price_micro_lamports:
                    effective_follow_up_compute_unit_price_micro_lamports(config, transport_plan),
                jito_tip_lamports: follow_up_tip_lamports,
                jito_tip_account: follow_up_tip_account.clone(),
            },
            &follow_up_lookup_table_variants,
        )?;
        let follow_up_serialized_len = BASE64
            .decode(follow_up_compiled.serializedBase64.as_bytes())
            .map_err(|error| {
                format!("Failed to decode compiled {follow_up_label} transaction: {error}")
            })?
            .len();
        if follow_up_serialized_len > SOLANA_TRANSACTION_PACKET_LIMIT {
            return Err(format!(
                "Pump {follow_up_label} transaction is {follow_up_serialized_len} bytes, exceeding Solana's {SOLANA_TRANSACTION_PACKET_LIMIT}-byte packet limit. Reduce fee-sharing recipients or disable split agent setup."
            ));
        }
        compile_timings.tx_serialize_ms += follow_up_serialize_started.elapsed().as_millis();
        compile_timings.follow_up_serialize_ms =
            Some(follow_up_serialize_started.elapsed().as_millis());
        let mut follow_up_metrics = follow_up_metrics;
        follow_up_metrics
            .warnings
            .extend(transaction_size_diagnostics(
                &follow_up_tx_instructions,
                &NativeTxConfig {
                    compute_unit_limit: configured_follow_up_compute_unit_limit(
                        config,
                        follow_up_label,
                    )?,
                    compute_unit_price_micro_lamports:
                        effective_follow_up_compute_unit_price_micro_lamports(
                            config,
                            transport_plan,
                        ),
                    jito_tip_lamports: follow_up_tip_lamports,
                    jito_tip_account: follow_up_tip_account.clone(),
                },
            ));
        deferred_setup_transactions.push(follow_up_compiled.clone());
        compiled_transactions.push(follow_up_compiled);
        compile_metrics.push(follow_up_metrics);
        instruction_summaries.push(summarize_instructions(&follow_up_tx_instructions));
    }

    if separate_tip_transaction {
        let tip_instruction = build_jito_tip_instruction(config, creator)?;
        let tip_tx_instructions = vec![tip_instruction];
        let tip_lookup_table_variants =
            lookup_table_variants_for_transaction("jito-tip", config, &lookup_tables);
        let tip_serialize_started = Instant::now();
        let (tip_compiled, tip_metrics) = compile_transaction_with_metrics(
            "jito-tip",
            tx_format,
            &blockhash,
            last_valid_block_height,
            &creator_keypair,
            None,
            tip_tx_instructions.clone(),
            &NativeTxConfig {
                compute_unit_limit: configured_launch_compute_unit_limit(config)?,
                compute_unit_price_micro_lamports: 0,
                jito_tip_lamports: config.tx.jitoTipLamports,
                jito_tip_account: config.tx.jitoTipAccount.clone(),
            },
            &tip_lookup_table_variants,
        )?;
        compile_timings.tx_serialize_ms += tip_serialize_started.elapsed().as_millis();
        compile_timings.tip_serialize_ms = Some(tip_serialize_started.elapsed().as_millis());
        let mut tip_metrics = tip_metrics;
        tip_metrics.warnings.extend(transaction_size_diagnostics(
            &tip_tx_instructions,
            &NativeTxConfig {
                compute_unit_limit: configured_launch_compute_unit_limit(config)?,
                compute_unit_price_micro_lamports: 0,
                jito_tip_lamports: config.tx.jitoTipLamports,
                jito_tip_account: config.tx.jitoTipAccount.clone(),
            },
        ));
        creation_transactions.push(tip_compiled.clone());
        compiled_transactions.push(tip_compiled);
        compile_metrics.push(tip_metrics);
        instruction_summaries.push(summarize_instructions(&tip_tx_instructions));
    }

    let mut report = build_report(
        config,
        transport_plan,
        built_at,
        rpc_url.to_string(),
        creator_public_key,
        mint.to_string(),
        agent_authority.map(|authority| authority.to_string()),
        config_path,
        lookup_tables
            .iter()
            .map(|table| table.key.to_string())
            .collect(),
    );
    if let Some(first_note) = report.execution.notes.first_mut() {
        *first_note =
            "Rust engine owns validation, runtime state, and API contracts. Native Pump assembly now covers LaunchDeck's Pump launch modes end-to-end; non-Pump flows still fall back to the JS compile bridge."
                .to_string();
    }
    report.execution.notes.push(
        "Native Pump assembly now includes compute-budget and priority-fee instructions for supported launch shapes."
            .to_string(),
    );
    apply_transaction_details(
        &mut report,
        &compiled_transactions,
        &instruction_summaries,
        &compile_metrics,
    )?;
    let text = render_report(&report);
    let mut report = serde_json::to_value(report).map_err(|error| error.to_string())?;
    let bonding_curve_address = bonding_curve_pda(&mint)?.to_string();
    report["pairAddress"] = serde_json::Value::String(bonding_curve_address.clone());
    report["routeAddress"] = serde_json::Value::String(bonding_curve_address.clone());
    report["bondingCurveAddress"] = serde_json::Value::String(bonding_curve_address);
    append_vanity_report_note(&mut report, vanity_reservation.as_ref());

    Ok(Some(NativePumpArtifacts {
        compiled_transactions,
        creation_transactions,
        deferred_setup_transactions,
        report,
        text,
        compile_timings,
        mint: mint.to_string(),
        launch_creator: launch_creator.to_string(),
        vanity_reservation,
    }))
}

#[derive(Debug, Clone)]
struct NativeTxConfig {
    compute_unit_limit: u32,
    compute_unit_price_micro_lamports: i64,
    jito_tip_lamports: i64,
    jito_tip_account: String,
}

#[derive(Debug, Clone, Default)]
struct TransactionCompileMetrics {
    legacy_length: Option<usize>,
    v0_length: Option<usize>,
    v0_alt_length: Option<usize>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct CompiledTxCandidate {
    compiled: CompiledTransaction,
    serialized_len: usize,
}

fn ensure_compiled_candidate_fits_packet(
    label: &str,
    candidate: &CompiledTxCandidate,
) -> Result<(), String> {
    if candidate.serialized_len > SOLANA_TRANSACTION_PACKET_LIMIT {
        return Err(format!(
            "Pump {label} transaction exceeded packet limit: raw {} > {} bytes",
            candidate.serialized_len, SOLANA_TRANSACTION_PACKET_LIMIT
        ));
    }
    Ok(())
}

fn create_v2_metadata_payload_bytes(instructions: &[Instruction]) -> Option<usize> {
    instructions.iter().find_map(|instruction| {
        if instruction.program_id.to_string() != PUMP_PROGRAM_ID {
            return None;
        }
        if instruction.data.len() < 8
            || instruction.data[..8] != [214, 144, 76, 236, 95, 139, 49, 180]
        {
            return None;
        }
        Some(instruction.data.len().saturating_sub(42))
    })
}

fn transaction_size_diagnostics(
    instructions: &[Instruction],
    tx_config: &NativeTxConfig,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(metadata_payload_bytes) = create_v2_metadata_payload_bytes(instructions) {
        warnings.push(format!(
            "CreateV2 metadata payload contributes {metadata_payload_bytes} bytes from name/symbol/uri."
        ));
    }
    if tx_config.compute_unit_price_micro_lamports > 0 {
        warnings.push(
            "Priority fee adds a ComputeBudget price instruction to this transaction.".to_string(),
        );
    }
    if tx_config.jito_tip_lamports > 0 {
        warnings.push("Inline tip adds a SystemProgram transfer instruction.".to_string());
    }
    warnings
}

fn effective_creation_compute_unit_price_micro_lamports(
    config: &NormalizedConfig,
    transport_plan: &TransportPlan,
    has_follow_up_transaction: bool,
) -> i64 {
    if transport_plan.transportType == "jito-bundle" && has_follow_up_transaction {
        return 0;
    }
    config.tx.computeUnitPriceMicroLamports.unwrap_or(0)
}

fn effective_follow_up_compute_unit_price_micro_lamports(
    config: &NormalizedConfig,
    transport_plan: &TransportPlan,
) -> i64 {
    if transport_plan.transportType == "jito-bundle" {
        return 0;
    }
    config.tx.computeUnitPriceMicroLamports.unwrap_or(0)
}

fn u64_to_u32_limit(value: u64, label: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("{label} is too large for a u32 compute unit limit."))
}

fn configured_launch_compute_unit_limit(config: &NormalizedConfig) -> Result<u32, String> {
    let limit = config
        .tx
        .computeUnitLimit
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or_else(configured_default_launch_compute_unit_limit);
    u64_to_u32_limit(limit, "launch compute unit limit")
}

fn configured_follow_up_compute_unit_limit(
    config: &NormalizedConfig,
    follow_up_label: &str,
) -> Result<u32, String> {
    let limit = config
        .tx
        .computeUnitLimit
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or_else(|| match follow_up_label {
            "agent-setup" => configured_default_agent_setup_compute_unit_limit(),
            _ => configured_default_follow_up_compute_unit_limit(),
        });
    u64_to_u32_limit(limit, "follow-up compute unit limit")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeTxFormat {
    Legacy,
    Auto,
    V0,
    V0Alt,
}

#[derive(Debug, Clone)]
struct PumpGlobalState {
    fee_recipient: Pubkey,
    initial_virtual_token_reserves: u64,
    initial_virtual_sol_reserves: u64,
    initial_real_token_reserves: u64,
    fee_basis_points: u64,
    creator_fee_basis_points: u64,
    fee_recipients: [Pubkey; 7],
    reserved_fee_recipient: Pubkey,
    reserved_fee_recipients: [Pubkey; 7],
    buyback_fee_recipients: [Pubkey; 8],
    buyback_basis_points: u64,
    initial_virtual_quote_reserves: u64,
    whitelisted_quote_mints: [Pubkey; 1],
}

#[derive(Debug, Clone, Serialize)]
pub struct PumpPreviewBasis {
    pub initialVirtualTokenReserves: String,
    pub initialVirtualSolReserves: String,
    pub initialRealTokenReserves: String,
    pub feeBasisPoints: String,
    pub creatorFeeBasisPoints: String,
}

#[derive(Debug, Clone)]
struct PumpBondingCurveState {
    virtual_token_reserves: u64,
    virtual_sol_reserves: u64,
    real_token_reserves: u64,
    real_sol_reserves: u64,
    token_total_supply: u64,
    complete: bool,
    creator: Pubkey,
    is_mayhem_mode: bool,
    cashback_enabled: bool,
    quote_mint: Pubkey,
}

#[derive(Debug, Clone)]
struct PumpAmmPoolState {
    pubkey: String,
    creator: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    pool_base_token_account: Pubkey,
    pool_quote_token_account: Pubkey,
}

const PUMP_AMM_POOL_DISCRIMINATOR_BYTES: usize = 8;
const PUMP_AMM_POOL_ACCOUNT_SIZE: usize = PUMP_AMM_POOL_DISCRIMINATOR_BYTES + 237;

#[derive(Debug, Clone)]
pub struct PreparedFollowBuyStatic {
    user: Pubkey,
    mint: Pubkey,
    launch_creator: Pubkey,
    token_program: Pubkey,
    sol_amount: u64,
    tx_config: NativeTxConfig,
}

#[derive(Debug, Clone)]
pub struct PreparedFollowBuyRuntime {
    global: PumpGlobalState,
    curve: PumpBondingCurveState,
    creator_vault_authority: Pubkey,
}

fn global_state_cache() -> &'static Mutex<Option<PumpGlobalState>> {
    static CACHE: OnceLock<Mutex<Option<PumpGlobalState>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn get_cached_global_state() -> Option<PumpGlobalState> {
    global_state_cache().lock().ok()?.clone()
}

fn cache_global_state(state: &PumpGlobalState) {
    if let Ok(mut cache) = global_state_cache().lock() {
        *cache = Some(state.clone());
    }
}

fn pump_preview_basis_from_global(global: &PumpGlobalState) -> PumpPreviewBasis {
    PumpPreviewBasis {
        initialVirtualTokenReserves: global.initial_virtual_token_reserves.to_string(),
        initialVirtualSolReserves: global.initial_virtual_sol_reserves.to_string(),
        initialRealTokenReserves: global.initial_real_token_reserves.to_string(),
        feeBasisPoints: global.fee_basis_points.to_string(),
        creatorFeeBasisPoints: global.creator_fee_basis_points.to_string(),
    }
}

fn keypair_from_secret_bytes(bytes: &[u8]) -> Result<Keypair, String> {
    Keypair::try_from(bytes).map_err(|error| error.to_string())
}

struct ResolvedMintKeypair {
    keypair: Keypair,
    vanity_reservation: Option<VanityReservation>,
    requires_unused_check: bool,
}

fn resolve_mint_keypair(config: &NormalizedConfig) -> Result<Keypair, String> {
    let vanity_private_key = config.vanityPrivateKey.trim();
    if vanity_private_key.is_empty() {
        return Ok(Keypair::new());
    }
    let bytes = read_keypair_bytes(vanity_private_key)
        .map_err(|error| format!("Invalid vanity private key: {error}"))?;
    keypair_from_secret_bytes(&bytes)
        .map_err(|error| format!("Invalid vanity private key: {error}"))
}

async fn resolve_mint_keypair_for_launch(
    rpc_url: &str,
    config: &NormalizedConfig,
    allow_queued_vanity: bool,
) -> Result<ResolvedMintKeypair, String> {
    let vanity_private_key = config.vanityPrivateKey.trim();
    if !vanity_private_key.is_empty() {
        return Ok(ResolvedMintKeypair {
            keypair: resolve_mint_keypair(config)?,
            vanity_reservation: None,
            requires_unused_check: true,
        });
    }
    if allow_queued_vanity
        && let Some(reserved) = reserve_vanity_mint(VanityLaunchpad::Pump, rpc_url).await?
    {
        return Ok(ResolvedMintKeypair {
            keypair: reserved.keypair,
            vanity_reservation: Some(reserved.reservation),
            requires_unused_check: false,
        });
    }
    Ok(ResolvedMintKeypair {
        keypair: Keypair::new(),
        vanity_reservation: None,
        requires_unused_check: false,
    })
}

async fn ensure_vanity_mint_unused(
    rpc_url: &str,
    requires_unused_check: bool,
    mint: &Pubkey,
) -> Result<(), String> {
    if !requires_unused_check {
        return Ok(());
    }
    match fetch_account_data(rpc_url, &mint.to_string(), "confirmed").await {
        Ok(_) => Err(format!(
            "This vanity address has already been used on-chain. Generate a fresh one. ({})",
            mint
        )),
        Err(error) if error.contains("was not found.") => Ok(()),
        Err(error) => Err(format!(
            "Failed to verify vanity private key availability: {error}"
        )),
    }
}

fn parse_pubkey(value: &str, label: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|error| format!("Invalid {label}: {error}"))
}

fn pump_program_id() -> Result<Pubkey, String> {
    parse_pubkey(PUMP_PROGRAM_ID, "PUMP program id")
}

fn mayhem_program_id() -> Result<Pubkey, String> {
    parse_pubkey(MAYHEM_PROGRAM_ID, "Mayhem program id")
}

fn pump_amm_program_id() -> Result<Pubkey, String> {
    parse_pubkey(PUMP_AMM_PROGRAM_ID, "PUMP AMM program id")
}

fn token_2022_program_id() -> Result<Pubkey, String> {
    parse_pubkey(TOKEN_2022_PROGRAM_ID, "Token 2022 program id")
}

pub fn derive_follow_owner_token_account_with_token_program(
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Result<Pubkey, String> {
    ensure_supported_pump_bonding_token_program(token_program)?;
    Ok(get_associated_token_address_with_program_id(
        owner,
        mint,
        token_program,
    ))
}

pub fn derive_follow_owner_token_account(owner: &Pubkey, mint: &Pubkey) -> Result<Pubkey, String> {
    derive_follow_owner_token_account_with_token_program(owner, mint, &token_2022_program_id()?)
}

fn token_program_id() -> Result<Pubkey, String> {
    parse_pubkey(TOKEN_PROGRAM_ID, "Token program id")
}

fn ensure_supported_pump_bonding_token_program(token_program: &Pubkey) -> Result<(), String> {
    if *token_program == token_program_id()? || *token_program == token_2022_program_id()? {
        Ok(())
    } else {
        Err(format!(
            "Pump bonding mint is owned by unsupported token program {token_program}."
        ))
    }
}

pub(crate) async fn resolve_pump_bonding_mint_token_program(
    rpc_url: &str,
    mint: &Pubkey,
    commitment: &str,
) -> Result<Pubkey, String> {
    let (_, owner) = fetch_account_data_with_owner(rpc_url, &mint.to_string(), commitment).await?;
    let owner = parse_pubkey(&owner, "Pump bonding mint owner")?;
    ensure_supported_pump_bonding_token_program(&owner)?;
    Ok(owner)
}

fn pump_fee_program_id() -> Result<Pubkey, String> {
    parse_pubkey(PUMP_FEE_PROGRAM_ID, "PUMP fee program id")
}

fn compute_budget_program_id() -> Result<Pubkey, String> {
    parse_pubkey(COMPUTE_BUDGET_PROGRAM_ID, "Compute Budget program id")
}

fn pump_agent_payments_program_id() -> Result<Pubkey, String> {
    parse_pubkey(
        PUMP_AGENT_PAYMENTS_PROGRAM_ID,
        "Pump Agent Payments program id",
    )
}

fn wsol_mint() -> Result<Pubkey, String> {
    parse_pubkey(WSOL_MINT, "WSOL mint")
}

fn usdc_mint() -> Result<Pubkey, String> {
    parse_pubkey(USDC_MINT, "USDC mint")
}

fn event_authority_pda(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"__event_authority"], program_id).0
}

fn selected_pump_apr28_fee_recipient() -> Result<Pubkey, String> {
    parse_pubkey(PUMP_APR28_FEE_RECIPIENTS[0], "Pump April 28 fee recipient")
}

fn bonding_curve_pda(mint: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &pump_program_id()?).0)
}

pub fn pump_bonding_curve_address(mint: &str) -> Result<String, String> {
    let mint = parse_pubkey(mint, "mint")?;
    Ok(bonding_curve_pda(&mint)?.to_string())
}

fn mint_authority_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"mint-authority"], &pump_program_id()?).0)
}

fn global_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"global"], &pump_program_id()?).0)
}

fn global_params_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"global-params"], &mayhem_program_id()?).0)
}

fn global_volume_accumulator_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"global_volume_accumulator"], &pump_program_id()?).0)
}

fn user_volume_accumulator_pda(user: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(
        &[b"user_volume_accumulator", user.as_ref()],
        &pump_program_id()?,
    )
    .0)
}

fn creator_vault_pda(creator: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"creator-vault", creator.as_ref()], &pump_program_id()?).0)
}

fn bonding_curve_v2_pda(mint: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"bonding-curve-v2", mint.as_ref()], &pump_program_id()?).0)
}

fn fee_config_pda() -> Result<Pubkey, String> {
    let pump_program = pump_program_id()?;
    let fee_program = pump_fee_program_id()?;
    Ok(Pubkey::find_program_address(&[b"fee_config", pump_program.as_ref()], &fee_program).0)
}

fn fee_sharing_config_pda(mint: &Pubkey) -> Result<Pubkey, String> {
    Ok(
        Pubkey::find_program_address(&[b"sharing-config", mint.as_ref()], &pump_fee_program_id()?)
            .0,
    )
}

async fn resolve_follow_creator_vault_authority(
    rpc_url: &str,
    mint: &Pubkey,
    launch_creator: &Pubkey,
    prefer_post_setup_creator_vault: bool,
) -> Result<Pubkey, String> {
    let sharing_config = fee_sharing_config_pda(mint)?;
    Ok(select_follow_creator_vault_authority(
        launch_creator,
        &sharing_config,
        prefer_post_setup_creator_vault,
        fetch_account_exists(rpc_url, &sharing_config.to_string(), "confirmed").await?,
    ))
}

fn select_follow_creator_vault_authority(
    launch_creator: &Pubkey,
    sharing_config: &Pubkey,
    prefer_post_setup_creator_vault: bool,
    sharing_config_exists: bool,
) -> Pubkey {
    if prefer_post_setup_creator_vault || sharing_config_exists {
        return *sharing_config;
    }
    *launch_creator
}

fn fee_program_global_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"fee-program-global"], &pump_fee_program_id()?).0)
}

fn agent_global_config_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"global-config"], &pump_agent_payments_program_id()?).0)
}

fn token_agent_payments_pda(mint: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(
        &[b"token-agent-payments", mint.as_ref()],
        &pump_agent_payments_program_id()?,
    )
    .0)
}

fn coin_creator_vault_authority_pda(coin_creator: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(
        &[b"creator_vault", coin_creator.as_ref()],
        &pump_amm_program_id()?,
    )
    .0)
}

fn coin_creator_vault_ata_pda(coin_creator_vault_authority: &Pubkey) -> Result<Pubkey, String> {
    Ok(get_associated_token_address_with_program_id(
        coin_creator_vault_authority,
        &wsol_mint()?,
        &token_program_id()?,
    ))
}

fn social_fee_pda(user_id: &str, platform: u8) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(
        &[b"social-fee-pda", user_id.as_bytes(), &[platform]],
        &pump_fee_program_id()?,
    )
    .0)
}

async fn fetch_global_state(rpc_url: &str) -> Result<PumpGlobalState, String> {
    let account_data = fetch_account_data(rpc_url, &global_pda()?.to_string(), "confirmed").await?;
    decode_global_state(&account_data)
}

async fn fetch_global_state_cached(rpc_url: &str) -> Result<PumpGlobalState, String> {
    if let Some(global) = get_cached_global_state() {
        return Ok(global);
    }
    let global = fetch_global_state(rpc_url).await?;
    cache_global_state(&global);
    Ok(global)
}

#[allow(dead_code)]
pub async fn warm_pump_global_state(rpc_url: &str) -> Result<PumpPreviewBasis, String> {
    let global = fetch_global_state(rpc_url).await?;
    cache_global_state(&global);
    Ok(pump_preview_basis_from_global(&global))
}

fn read_bool(data: &[u8], offset: &mut usize) -> Result<bool, String> {
    let Some(byte) = data.get(*offset) else {
        return Err("Unexpected end of global account while reading bool.".to_string());
    };
    *offset += 1;
    Ok(*byte != 0)
}

fn read_u64(data: &[u8], offset: &mut usize) -> Result<u64, String> {
    let end = offset.saturating_add(8);
    let bytes: [u8; 8] = data
        .get(*offset..end)
        .ok_or_else(|| "Unexpected end of global account while reading u64.".to_string())?
        .try_into()
        .map_err(|_| "Failed to read u64 bytes from global account.".to_string())?;
    *offset = end;
    Ok(u64::from_le_bytes(bytes))
}

fn read_pubkey(data: &[u8], offset: &mut usize) -> Result<Pubkey, String> {
    let end = offset.saturating_add(32);
    let bytes: [u8; 32] = data
        .get(*offset..end)
        .ok_or_else(|| "Unexpected end of global account while reading pubkey.".to_string())?
        .try_into()
        .map_err(|_| "Failed to read pubkey bytes from global account.".to_string())?;
    *offset = end;
    Ok(Pubkey::new_from_array(bytes))
}

fn read_pubkey_array<const N: usize>(
    data: &[u8],
    offset: &mut usize,
) -> Result<[Pubkey; N], String> {
    let mut values = Vec::with_capacity(N);
    for _ in 0..N {
        values.push(read_pubkey(data, offset)?);
    }
    values
        .try_into()
        .map_err(|_| "Failed to decode pubkey array from global account.".to_string())
}

fn decode_global_state(data: &[u8]) -> Result<PumpGlobalState, String> {
    if data.len() < GLOBAL_ACCOUNT_DISCRIMINATOR_BYTES {
        return Err("Global account data was too short.".to_string());
    }
    let mut offset = GLOBAL_ACCOUNT_DISCRIMINATOR_BYTES;
    let _initialized = read_bool(data, &mut offset)?;
    let _authority = read_pubkey(data, &mut offset)?;
    let fee_recipient = read_pubkey(data, &mut offset)?;
    let initial_virtual_token_reserves = read_u64(data, &mut offset)?;
    let initial_virtual_sol_reserves = read_u64(data, &mut offset)?;
    let initial_real_token_reserves = read_u64(data, &mut offset)?;
    let _token_total_supply = read_u64(data, &mut offset)?;
    let fee_basis_points = read_u64(data, &mut offset)?;
    let _withdraw_authority = read_pubkey(data, &mut offset)?;
    let _enable_migrate = read_bool(data, &mut offset)?;
    let _pool_migration_fee = read_u64(data, &mut offset)?;
    let creator_fee_basis_points = read_u64(data, &mut offset)?;
    let fee_recipients = read_pubkey_array::<7>(data, &mut offset)?;
    let _set_creator_authority = read_pubkey(data, &mut offset)?;
    let _admin_set_creator_authority = read_pubkey(data, &mut offset)?;
    let _create_v2_enabled = read_bool(data, &mut offset)?;
    let _whitelist_pda = read_pubkey(data, &mut offset)?;
    let reserved_fee_recipient = read_pubkey(data, &mut offset)?;
    let _mayhem_mode_enabled = read_bool(data, &mut offset)?;
    let reserved_fee_recipients = read_pubkey_array::<7>(data, &mut offset)?;
    let _is_cashback_enabled = if offset < data.len() {
        read_bool(data, &mut offset)?
    } else {
        false
    };
    let buyback_fee_recipients = if offset < data.len() {
        read_pubkey_array::<8>(data, &mut offset)?
    } else {
        [Pubkey::default(); 8]
    };
    let buyback_basis_points = if offset < data.len() {
        read_u64(data, &mut offset)?
    } else {
        0
    };
    let initial_virtual_quote_reserves = if offset < data.len() {
        read_u64(data, &mut offset)?
    } else {
        0
    };
    let whitelisted_quote_mints = if offset < data.len() {
        read_pubkey_array::<1>(data, &mut offset)?
    } else {
        [Pubkey::default(); 1]
    };

    Ok(PumpGlobalState {
        fee_recipient,
        initial_virtual_token_reserves,
        initial_virtual_sol_reserves,
        initial_real_token_reserves,
        fee_basis_points,
        creator_fee_basis_points,
        fee_recipients,
        reserved_fee_recipient,
        reserved_fee_recipients,
        buyback_fee_recipients,
        buyback_basis_points,
        initial_virtual_quote_reserves,
        whitelisted_quote_mints,
    })
}

fn parse_bonding_curve_state(data: &[u8]) -> Result<PumpBondingCurveState, String> {
    let mut offset = 8usize;
    let virtual_token_reserves = read_u64(data, &mut offset)?;
    let virtual_sol_reserves = read_u64(data, &mut offset)?;
    let real_token_reserves = read_u64(data, &mut offset)?;
    let real_sol_reserves = read_u64(data, &mut offset)?;
    let token_total_supply = read_u64(data, &mut offset)?;
    let complete = read_bool(data, &mut offset)?;
    let creator = read_pubkey(data, &mut offset)?;
    let is_mayhem_mode = if offset < data.len() {
        read_bool(data, &mut offset)?
    } else {
        false
    };
    let cashback_enabled = if offset < data.len() {
        read_bool(data, &mut offset)?
    } else {
        false
    };
    let quote_mint = if offset < data.len() {
        read_pubkey(data, &mut offset)?
    } else {
        Pubkey::default()
    };
    Ok(PumpBondingCurveState {
        virtual_token_reserves,
        virtual_sol_reserves,
        real_token_reserves,
        real_sol_reserves,
        token_total_supply,
        complete,
        creator,
        is_mayhem_mode,
        cashback_enabled,
        quote_mint,
    })
}

fn parse_pump_amm_pool_state(
    account_pubkey: &str,
    data: &[u8],
) -> Result<PumpAmmPoolState, String> {
    if data.len() < PUMP_AMM_POOL_ACCOUNT_SIZE {
        return Err("Pump AMM pool account data was too short.".to_string());
    }
    let mut offset = PUMP_AMM_POOL_DISCRIMINATOR_BYTES;
    let _pool_bump = data.get(offset).copied().ok_or_else(|| {
        "Pump AMM pool account was too short while reading pool bump.".to_string()
    })?;
    offset += 1;
    offset += 2; // index
    let creator = read_pubkey(data, &mut offset)?;
    let base_mint = read_pubkey(data, &mut offset)?;
    let quote_mint = read_pubkey(data, &mut offset)?;
    let _lp_mint = read_pubkey(data, &mut offset)?;
    let pool_base_token_account = read_pubkey(data, &mut offset)?;
    let pool_quote_token_account = read_pubkey(data, &mut offset)?;
    Ok(PumpAmmPoolState {
        pubkey: account_pubkey.to_string(),
        creator,
        base_mint,
        quote_mint,
        pool_base_token_account,
        pool_quote_token_account,
    })
}

fn decode_spl_token_account_amount(data: &[u8], label: &str) -> Result<u64, String> {
    let bytes: [u8; 8] = data
        .get(64..72)
        .ok_or_else(|| format!("{label} token account data was too short."))?
        .try_into()
        .map_err(|_| format!("Failed to read token amount bytes from {label} account."))?;
    Ok(u64::from_le_bytes(bytes))
}

fn decode_spl_mint_supply(data: &[u8], label: &str) -> Result<u64, String> {
    let bytes: [u8; 8] = data
        .get(36..44)
        .ok_or_else(|| format!("{label} mint account data was too short."))?
        .try_into()
        .map_err(|_| format!("Failed to read mint supply bytes from {label} mint account."))?;
    Ok(u64::from_le_bytes(bytes))
}

fn current_market_cap_quote_units(total_supply: u64, quote_reserve: u64, base_reserve: u64) -> u64 {
    if base_reserve == 0 {
        return 0;
    }
    ((u128::from(total_supply) * u128::from(quote_reserve)) / u128::from(base_reserve))
        .min(u128::from(u64::MAX))
        .try_into()
        .unwrap_or(u64::MAX)
}

fn quote_asset_meta_for_mint(quote_mint: &Pubkey) -> Option<(&'static str, &'static str, u32)> {
    match quote_mint.to_string().as_str() {
        WSOL_MINT => Some(("sol", "SOL", 9)),
        USDC_MINT => Some(("usdc", "USDC", 6)),
        USDT_MINT => Some(("usdt", "USDT", 6)),
        USD1_MINT => Some(("usd1", "USD1", 6)),
        _ => None,
    }
}

fn pump_amm_quote_priority(quote_mint: &Pubkey) -> u8 {
    match quote_mint.to_string().as_str() {
        WSOL_MINT => 0,
        USDC_MINT => 1,
        USDT_MINT => 2,
        USD1_MINT => 3,
        _ => 255,
    }
}

fn pump_pool_authority_pda(base_mint: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(
        &[b"pool-authority", base_mint.as_ref()],
        &pump_program_id()?,
    )
    .0)
}

fn derive_pump_amm_pool_address(
    creator: &Pubkey,
    mint: &Pubkey,
    quote_mint: &Pubkey,
    index: u16,
) -> Result<Pubkey, String> {
    let program_id = parse_pubkey(PUMP_AMM_PROGRAM_ID, "pump amm program")?;
    let index_bytes = index.to_le_bytes();
    let (pubkey, _) = Pubkey::find_program_address(
        &[
            b"pool",
            &index_bytes,
            creator.as_ref(),
            mint.as_ref(),
            quote_mint.as_ref(),
        ],
        &program_id,
    );
    Ok(pubkey)
}

async fn find_pump_amm_pool_state(
    rpc_url: &str,
    mint: &Pubkey,
    creator: &Pubkey,
) -> Result<Option<PumpAmmPoolState>, String> {
    let quote_candidates = [WSOL_MINT, USDC_MINT, USDT_MINT, USD1_MINT];
    let mut requests = Vec::new();
    let canonical_creator = pump_pool_authority_pda(mint)?;

    let mut push_candidate = |pool_pubkey: Pubkey| {
        let value = pool_pubkey.to_string();
        if !requests.iter().any(|existing| existing == &value) {
            requests.push(value);
        }
    };

    for quote_mint in quote_candidates {
        let quote_pubkey = parse_pubkey(quote_mint, "pump amm quote mint")?;
        push_candidate(derive_pump_amm_pool_address(
            &canonical_creator,
            mint,
            &quote_pubkey,
            0,
        )?);
        for index in 0u16..=3 {
            push_candidate(derive_pump_amm_pool_address(
                creator,
                mint,
                &quote_pubkey,
                index,
            )?);
        }
    }
    let accounts = fetch_multiple_account_data(rpc_url, &requests, "confirmed").await?;
    let mut pools = requests
        .into_iter()
        .zip(accounts.into_iter())
        .filter_map(|(pubkey, data)| {
            data.and_then(|bytes| parse_pump_amm_pool_state(&pubkey, &bytes).ok())
        })
        .filter(|pool| {
            pool.base_mint == *mint && quote_asset_meta_for_mint(&pool.quote_mint).is_some()
        })
        .collect::<Vec<_>>();
    pools.sort_by_key(|pool| pump_amm_quote_priority(&pool.quote_mint));
    Ok(pools.into_iter().next())
}

async fn fetch_pump_amm_market_snapshot_for_mint(
    rpc_url: &str,
    mint: &Pubkey,
    creator: &Pubkey,
) -> Result<PumpMarketSnapshot, String> {
    let Some(pool) = find_pump_amm_pool_state(rpc_url, mint, creator).await? else {
        return Err(format!(
            "No Pump AMM pool found for mint {} and creator {} with a supported quote asset.",
            mint, creator
        ));
    };
    let Some((quote_asset, quote_asset_label, quote_decimals)) =
        quote_asset_meta_for_mint(&pool.quote_mint)
    else {
        return Err(format!(
            "Pump AMM pool {} uses unsupported quote mint {}.",
            pool.pubkey, pool.quote_mint
        ));
    };
    let account_keys = vec![
        pool.pool_base_token_account.to_string(),
        pool.pool_quote_token_account.to_string(),
        mint.to_string(),
    ];
    let accounts = fetch_multiple_account_data(rpc_url, &account_keys, "confirmed").await?;
    let base_vault = accounts
        .first()
        .and_then(|entry| entry.as_ref())
        .ok_or_else(|| {
            format!(
                "Pump AMM base vault {} was not found.",
                pool.pool_base_token_account
            )
        })?;
    let quote_vault = accounts
        .get(1)
        .and_then(|entry| entry.as_ref())
        .ok_or_else(|| {
            format!(
                "Pump AMM quote vault {} was not found.",
                pool.pool_quote_token_account
            )
        })?;
    let mint_account = accounts
        .get(2)
        .and_then(|entry| entry.as_ref())
        .ok_or_else(|| format!("Mint account {} was not found.", mint))?;
    let base_reserve = decode_spl_token_account_amount(base_vault, "Pump AMM base vault")?;
    let quote_reserve = decode_spl_token_account_amount(quote_vault, "Pump AMM quote vault")?;
    let total_supply = decode_spl_mint_supply(mint_account, "Pump token")?;
    let market_cap_quote_units =
        current_market_cap_quote_units(total_supply, quote_reserve, base_reserve);
    Ok(PumpMarketSnapshot {
        mint: mint.to_string(),
        creator: pool.creator.to_string(),
        virtualTokenReserves: base_reserve,
        virtualSolReserves: quote_reserve,
        realTokenReserves: base_reserve,
        realSolReserves: quote_reserve,
        tokenTotalSupply: total_supply,
        complete: true,
        marketCapLamports: market_cap_quote_units,
        marketCapSol: format_decimal_u128(u128::from(market_cap_quote_units), quote_decimals, 6),
        quoteAsset: quote_asset.to_string(),
        quoteAssetLabel: quote_asset_label.to_string(),
    })
}

async fn fetch_bonding_curve_state(
    rpc_url: &str,
    mint: &Pubkey,
) -> Result<PumpBondingCurveState, String> {
    let data =
        fetch_account_data(rpc_url, &bonding_curve_pda(mint)?.to_string(), "confirmed").await?;
    parse_bonding_curve_state(&data)
}

fn parse_decimal_u64(value: &str, decimals: u32, label: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} cannot be blank."));
    }
    if !trimmed
        .chars()
        .all(|char| char.is_ascii_digit() || char == '.')
    {
        return Err(format!(
            "{label} must be a positive decimal string. Got: {value}"
        ));
    }
    let mut parts = trimmed.split('.');
    let whole = parts.next().unwrap_or_default();
    let fractional = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        return Err(format!(
            "{label} must be a positive decimal string. Got: {value}"
        ));
    }
    if fractional.len() > decimals as usize {
        return Err(format!(
            "{label} supports at most {decimals} decimal places. Got: {value}"
        ));
    }
    let mut normalized = String::new();
    normalized.push_str(whole);
    normalized.push_str(&format!("{fractional:0<width$}", width = decimals as usize));
    let digits = normalized.trim_start_matches('0');
    if digits.is_empty() {
        return Ok(0);
    }
    digits.parse::<u64>().map_err(|error| error.to_string())
}

fn ceil_div(numerator: u128, denominator: u128) -> u128 {
    numerator.div_ceil(denominator)
}

fn compute_total_fee_basis_points(global: &PumpGlobalState, include_creator_fee: bool) -> u128 {
    u128::from(global.fee_basis_points)
        + if include_creator_fee {
            u128::from(global.creator_fee_basis_points)
        } else {
            0
        }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
fn format_supply_percent(raw_token_amount: u64) -> String {
    if raw_token_amount == 0 {
        return "0".to_string();
    }
    const TOTAL_SUPPLY_RAW: u128 = 1_000_000_000u128 * 1_000_000u128;
    let scaled = (u128::from(raw_token_amount) * 1_000_000u128) / TOTAL_SUPPLY_RAW;
    format_decimal_u128(scaled, 4, 4)
}

fn quote_buy_tokens_from_sol(global: &PumpGlobalState, spendable_sol: u64) -> u64 {
    if spendable_sol == 0 {
        return 0;
    }
    let total_fee_basis_points = compute_total_fee_basis_points(global, true);
    let input_amount = ((u128::from(spendable_sol).saturating_sub(1)) * 10_000)
        / (10_000 + total_fee_basis_points);
    if input_amount == 0 {
        return 0;
    }
    let virtual_token_reserves = u128::from(global.initial_virtual_token_reserves);
    let virtual_sol_reserves = u128::from(global.initial_virtual_sol_reserves);
    let tokens = (input_amount * virtual_token_reserves) / (virtual_sol_reserves + input_amount);
    tokens
        .min(u128::from(global.initial_real_token_reserves))
        .try_into()
        .unwrap_or(u64::MAX)
}

fn quote_buy_tokens_from_curve(
    curve: &PumpBondingCurveState,
    global: &PumpGlobalState,
    spendable_sol: u64,
) -> u64 {
    if spendable_sol == 0 {
        return 0;
    }
    let total_fee_basis_points = compute_total_fee_basis_points(global, true);
    let input_amount = ((u128::from(spendable_sol).saturating_sub(1)) * 10_000)
        / (10_000 + total_fee_basis_points);
    if input_amount == 0 {
        return 0;
    }
    let virtual_token_reserves = u128::from(curve.virtual_token_reserves);
    let virtual_sol_reserves = u128::from(curve.virtual_sol_reserves);
    let tokens = (input_amount * virtual_token_reserves) / (virtual_sol_reserves + input_amount);
    tokens
        .min(u128::from(curve.real_token_reserves))
        .try_into()
        .unwrap_or(u64::MAX)
}

fn quote_buy_sol_from_tokens(global: &PumpGlobalState, token_amount: u64) -> u64 {
    if token_amount == 0 {
        return 0;
    }
    let amount = u128::from(token_amount).min(u128::from(global.initial_real_token_reserves));
    let virtual_token_reserves = u128::from(global.initial_virtual_token_reserves);
    let virtual_sol_reserves = u128::from(global.initial_virtual_sol_reserves);
    let sol_cost = ((amount * virtual_sol_reserves) / (virtual_token_reserves - amount)) + 1;
    let protocol_fee = ceil_div(sol_cost * u128::from(global.fee_basis_points), 10_000);
    let creator_fee = ceil_div(
        sol_cost * u128::from(global.creator_fee_basis_points),
        10_000,
    );
    // The exact-SOL-in path derives effective curve input from spendable SOL minus one lamport.
    (sol_cost + protocol_fee + creator_fee + 1)
        .min(u128::from(u64::MAX))
        .try_into()
        .unwrap_or(u64::MAX)
}

fn quote_sell_sol_from_curve(
    curve: &PumpBondingCurveState,
    global: &PumpGlobalState,
    token_amount: u64,
) -> u64 {
    if token_amount == 0 {
        return 0;
    }
    let gross_output = (u128::from(token_amount) * u128::from(curve.virtual_sol_reserves))
        / (u128::from(curve.virtual_token_reserves) + u128::from(token_amount));
    let protocol_fee = ceil_div(gross_output * u128::from(global.fee_basis_points), 10_000);
    let creator_fee = ceil_div(
        gross_output * u128::from(global.creator_fee_basis_points),
        10_000,
    );
    gross_output
        .saturating_sub(protocol_fee)
        .saturating_sub(creator_fee)
        .min(u128::from(curve.real_sol_reserves))
        .try_into()
        .unwrap_or_default()
}

fn quote_sell_quote_from_curve(
    curve: &PumpBondingCurveState,
    global: &PumpGlobalState,
    token_amount: u64,
) -> u64 {
    quote_sell_sol_from_curve(curve, global, token_amount)
}

fn split_two_leg_slippage_bps(slippage_bps: u64) -> (u64, u64) {
    let capped = slippage_bps.min(10_000);
    (capped.saturating_sub(capped / 2), capped / 2)
}

fn quote_buy_curve_input_from_tokens(global: &PumpGlobalState, token_amount: u64) -> u64 {
    if token_amount == 0 {
        return 0;
    }
    let amount = u128::from(token_amount).min(u128::from(global.initial_real_token_reserves));
    let virtual_token_reserves = u128::from(global.initial_virtual_token_reserves);
    let virtual_sol_reserves = u128::from(global.initial_virtual_sol_reserves);
    (((amount * virtual_sol_reserves) / (virtual_token_reserves.saturating_sub(amount))) + 1)
        .min(u128::from(u64::MAX))
        .try_into()
        .unwrap_or(u64::MAX)
}

fn synthetic_curve_after_buy_tokens(
    global: &PumpGlobalState,
    launch_creator: &Pubkey,
    token_amount: u64,
    cashback_enabled: bool,
) -> PumpBondingCurveState {
    let curve_input = quote_buy_curve_input_from_tokens(global, token_amount);
    PumpBondingCurveState {
        virtual_token_reserves: global
            .initial_virtual_token_reserves
            .saturating_sub(token_amount),
        virtual_sol_reserves: global
            .initial_virtual_sol_reserves
            .saturating_add(curve_input),
        real_token_reserves: global
            .initial_real_token_reserves
            .saturating_sub(token_amount),
        real_sol_reserves: curve_input,
        token_total_supply: global.initial_real_token_reserves,
        complete: false,
        creator: *launch_creator,
        is_mayhem_mode: false,
        cashback_enabled,
        quote_mint: Pubkey::default(),
    }
}

fn current_market_cap_lamports(curve: &PumpBondingCurveState) -> u64 {
    if curve.virtual_token_reserves == 0 {
        return 0;
    }
    ((u128::from(curve.token_total_supply) * u128::from(curve.virtual_sol_reserves))
        / u128::from(curve.virtual_token_reserves))
    .min(u128::from(u64::MAX))
    .try_into()
    .unwrap_or(u64::MAX)
}

fn resolve_dev_buy_quote(
    config: &NormalizedConfig,
    global: &PumpGlobalState,
) -> Result<Option<(u64, u64)>, String> {
    let Some(dev_buy) = &config.devBuy else {
        return Ok(None);
    };
    if dev_buy.mode == "sol" {
        let sol_amount = parse_decimal_u64(&dev_buy.amount, 9, "devBuy.amount")?;
        return Ok(Some((
            sol_amount,
            quote_buy_tokens_from_sol(global, sol_amount),
        )));
    }
    if dev_buy.mode == "tokens" {
        let token_amount = parse_decimal_u64(&dev_buy.amount, TOKEN_DECIMALS, "devBuy.amount")?;
        return Ok(Some((
            quote_buy_sol_from_tokens(global, token_amount),
            token_amount,
        )));
    }
    Err(format!(
        "Unsupported devBuy.mode for native Pump compile: {}",
        dev_buy.mode
    ))
}

pub async fn predict_dev_buy_token_amount(
    rpc_url: &str,
    config: &NormalizedConfig,
) -> Result<Option<u64>, String> {
    let global = fetch_global_state_cached(rpc_url).await?;
    Ok(resolve_dev_buy_quote(config, &global)?.map(|(_, token_amount)| token_amount))
}

#[allow(dead_code)]
pub async fn quote_launch(
    rpc_url: &str,
    mode: &str,
    amount: &str,
) -> Result<Option<LaunchQuote>, String> {
    let trimmed_mode = mode.trim().to_lowercase();
    let trimmed_amount = amount.trim();
    if trimmed_mode.is_empty() || trimmed_amount.is_empty() {
        return Ok(None);
    }
    if trimmed_mode != "sol" && trimmed_mode != "tokens" {
        return Err(format!(
            "Unsupported dev buy quote mode: {mode}. Expected sol or tokens."
        ));
    }
    let global = fetch_global_state_cached(rpc_url).await?;
    if trimmed_mode == "sol" {
        let spendable_sol = parse_decimal_u64(trimmed_amount, 9, "buy amount")?;
        if spendable_sol == 0 {
            return Ok(None);
        }
        let tokens_out = quote_buy_tokens_from_sol(&global, spendable_sol);
        return Ok(Some(LaunchQuote {
            mode: trimmed_mode,
            input: trimmed_amount.to_string(),
            estimatedTokens: format_decimal_u128(u128::from(tokens_out), TOKEN_DECIMALS, 6),
            estimatedSol: format_decimal_u128(u128::from(spendable_sol), 9, 6),
            estimatedQuoteAmount: format_decimal_u128(u128::from(spendable_sol), 9, 6),
            quoteAsset: "sol".to_string(),
            quoteAssetLabel: "SOL".to_string(),
            estimatedSupplyPercent: format_supply_percent(tokens_out),
        }));
    }

    let token_amount = parse_decimal_u64(trimmed_amount, TOKEN_DECIMALS, "buy amount")?;
    if token_amount == 0 || token_amount >= global.initial_virtual_token_reserves {
        return Ok(None);
    }
    Ok(Some(LaunchQuote {
        mode: trimmed_mode,
        input: trimmed_amount.to_string(),
        estimatedTokens: format_decimal_u128(u128::from(token_amount), TOKEN_DECIMALS, 6),
        estimatedSol: format_decimal_u128(
            u128::from(quote_buy_sol_from_tokens(&global, token_amount)),
            9,
            6,
        ),
        estimatedQuoteAmount: format_decimal_u128(
            u128::from(quote_buy_sol_from_tokens(&global, token_amount)),
            9,
            6,
        ),
        quoteAsset: "sol".to_string(),
        quoteAssetLabel: "SOL".to_string(),
        estimatedSupplyPercent: format_supply_percent(token_amount),
    }))
}

pub async fn fetch_pump_market_snapshot(
    rpc_url: &str,
    mint: &str,
) -> Result<PumpMarketSnapshot, String> {
    let mint = parse_pubkey(mint, "mint")?;
    match fetch_bonding_curve_state(rpc_url, &mint).await {
        Ok(curve) => {
            let market_cap_lamports = current_market_cap_lamports(&curve);
            let curve_snapshot = PumpMarketSnapshot {
                mint: mint.to_string(),
                creator: curve.creator.to_string(),
                virtualTokenReserves: curve.virtual_token_reserves,
                virtualSolReserves: curve.virtual_sol_reserves,
                realTokenReserves: curve.real_token_reserves,
                realSolReserves: curve.real_sol_reserves,
                tokenTotalSupply: curve.token_total_supply,
                complete: curve.complete,
                marketCapLamports: market_cap_lamports,
                marketCapSol: format_decimal_u128(u128::from(market_cap_lamports), 9, 6),
                quoteAsset: "sol".to_string(),
                quoteAssetLabel: "SOL".to_string(),
            };
            if !curve.complete && curve.virtual_token_reserves > 0 && curve.virtual_sol_reserves > 0
            {
                return Ok(curve_snapshot);
            }
            fetch_pump_amm_market_snapshot_for_mint(rpc_url, &mint, &curve.creator)
                .await
                .or(Ok(curve_snapshot))
        }
        Err(curve_error) => Err(format!(
            "Failed to fetch Pump bonding-curve snapshot ({curve_error}). Pump AMM fallback requires the bonding-curve creator."
        )),
    }
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
    let percent = parse_decimal_u64(slippage_percent, 2, "slippage percent")?;
    Ok(percent.min(10_000))
}

fn wrapper_net_sol_input(gross_lamports: u64, wrapper_fee_bps: u16) -> Result<u64, String> {
    let fee_lamports = estimate_sol_in_fee_lamports(gross_lamports, wrapper_fee_bps);
    let net_lamports = gross_lamports
        .checked_sub(fee_lamports)
        .ok_or_else(|| "Pump wrapper fee exceeds gross SOL input.".to_string())?;
    if gross_lamports > 0 && net_lamports == 0 {
        return Err("Pump wrapper net SOL input resolves to zero.".to_string());
    }
    Ok(net_lamports)
}

fn route_account_index(
    route_accounts: &[AccountMeta],
    pubkey: &Pubkey,
    context: &str,
) -> Result<u16, String> {
    route_accounts
        .iter()
        .position(|meta| meta.pubkey == *pubkey)
        .ok_or_else(|| format!("{context} account was not present in wrapper route accounts"))?
        .try_into()
        .map_err(|_| format!("{context} account index does not fit in u16"))
}

fn route_len_u16(len: usize, context: &str) -> Result<u16, String> {
    len.try_into()
        .map_err(|_| format!("{context} route account count does not fit in u16"))
}

fn route_fee_lamports_floor(gross_lamports: u64, fee_bps: u16) -> Result<u64, String> {
    gross_lamports
        .checked_mul(u64::from(fee_bps))
        .ok_or_else(|| "Pump wrapper route fee calculation overflowed".to_string())
        .map(|value| value / 10_000)
}

pub async fn compile_follow_buy_transaction(
    rpc_url: &str,
    execution: &NormalizedExecution,
    token_mayhem_mode: bool,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    launch_creator: &str,
    buy_amount_sol: &str,
    prefer_post_setup_creator_vault: bool,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let prepared = prepare_follow_buy_static(
        rpc_url,
        execution,
        jito_tip_account,
        wallet_secret,
        mint,
        launch_creator,
        buy_amount_sol,
    )
    .await?;
    let runtime = prepare_follow_buy_runtime(
        rpc_url,
        mint,
        launch_creator,
        prefer_post_setup_creator_vault,
    )
    .await?;
    finalize_follow_buy_transaction(
        rpc_url,
        execution,
        token_mayhem_mode,
        wallet_secret,
        &prepared,
        &runtime,
        wrapper_fee_bps,
    )
    .await
}

fn provider_uses_follow_tip(provider: &str) -> bool {
    matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "helius-sender" | "hellomoon" | "jito-bundle"
    )
}

const HELLOMOON_MIN_INLINE_TIP_LAMPORTS: i64 = 1_000_000;

fn resolve_follow_tip_config(
    provider: &str,
    tip_sol: &str,
    jito_tip_account: &str,
    label: &str,
) -> Result<(i64, String), String> {
    if !provider_uses_follow_tip(provider) {
        return Ok((0, String::new()));
    }
    let tip_lamports = parse_decimal_u64(tip_sol, 9, label)? as i64;
    if provider.trim().eq_ignore_ascii_case("hellomoon")
        && tip_lamports < HELLOMOON_MIN_INLINE_TIP_LAMPORTS
    {
        return Err(format!(
            "{label} must be at least 0.001 SOL when using Hello Moon for follow / snipe / auto-sell."
        ));
    }
    let tip_account = if tip_sol.trim().is_empty() {
        String::new()
    } else {
        jito_tip_account.to_string()
    };
    Ok((tip_lamports, tip_account))
}

pub async fn prepare_follow_buy_static(
    rpc_url: &str,
    execution: &NormalizedExecution,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    launch_creator: &str,
    buy_amount_sol: &str,
) -> Result<PreparedFollowBuyStatic, String> {
    let user_keypair = keypair_from_secret_bytes(wallet_secret)?;
    let user = user_keypair.pubkey();
    let mint = parse_pubkey(mint, "mint")?;
    let launch_creator = parse_pubkey(launch_creator, "launch creator")?;
    let token_program =
        resolve_pump_bonding_mint_token_program(rpc_url, &mint, &execution.commitment).await?;
    let sol_amount = parse_decimal_u64(buy_amount_sol, 9, "followLaunch.snipes.buyAmountSol")?;
    let (jito_tip_lamports, jito_tip_account) = resolve_follow_tip_config(
        &execution.buyProvider,
        &execution.buyTipSol,
        jito_tip_account,
        "buy tip",
    )?;
    let tx_config = NativeTxConfig {
        compute_unit_limit: u64_to_u32_limit(
            configured_default_sniper_buy_compute_unit_limit(),
            "sniper buy compute unit limit",
        )?,
        compute_unit_price_micro_lamports: priority_fee_sol_to_micro_lamports(
            &execution.buyPriorityFeeSol,
        )? as i64,
        jito_tip_lamports,
        jito_tip_account,
    };
    let _ = rpc_url;
    Ok(PreparedFollowBuyStatic {
        user,
        mint,
        launch_creator,
        token_program,
        sol_amount,
        tx_config,
    })
}

pub async fn prepare_follow_buy_runtime(
    rpc_url: &str,
    mint: &str,
    launch_creator: &str,
    prefer_post_setup_creator_vault: bool,
) -> Result<PreparedFollowBuyRuntime, String> {
    let mint = parse_pubkey(mint, "mint")?;
    let launch_creator = parse_pubkey(launch_creator, "launch creator")?;
    let creator_vault_authority = resolve_follow_creator_vault_authority(
        rpc_url,
        &mint,
        &launch_creator,
        prefer_post_setup_creator_vault,
    )
    .await?;
    let global = fetch_global_state_cached(rpc_url).await?;
    let curve = fetch_bonding_curve_state(rpc_url, &mint).await?;
    Ok(PreparedFollowBuyRuntime {
        global,
        curve,
        creator_vault_authority,
    })
}

async fn finalize_usdc_follow_buy_transaction(
    rpc_url: &str,
    execution: &NormalizedExecution,
    token_mayhem_mode: bool,
    user_keypair: &Keypair,
    prepared: &PreparedFollowBuyStatic,
    runtime: &PreparedFollowBuyRuntime,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let user = user_keypair.pubkey();
    let usdc_mint = usdc_mint()?;
    let quote_token_program = token_program_id()?;
    let route_wsol_account = route_wsol_pda(&user, 0);
    let user_usdc_account =
        get_associated_token_address_with_program_id(&user, &usdc_mint, &quote_token_program);
    let net_sol_amount = wrapper_net_sol_input(prepared.sol_amount, wrapper_fee_bps)?;
    let slippage_bps = slippage_bps_from_percent(&execution.buySlippagePercent)?;
    let (conversion_slippage_bps, pump_slippage_bps) = split_two_leg_slippage_bps(slippage_bps);
    let conversion = build_trusted_raydium_clmm_swap_exact_in(
        rpc_url,
        RAYDIUM_SOL_USDC_POOL,
        &execution.commitment,
        &user,
        &route_wsol_account,
        &user_usdc_account,
        &wsol_mint()?,
        &usdc_mint,
        net_sol_amount,
        conversion_slippage_bps,
    )
    .await?;
    let token_amount =
        quote_buy_tokens_from_curve(&runtime.curve, &runtime.global, conversion.min_out);
    if token_amount == 0 {
        return Err("Pump USDC follow buy quote resolved to zero tokens.".to_string());
    }
    let min_tokens_out = apply_buy_token_slippage(token_amount, pump_slippage_bps);
    let pump_ix = build_buy_exact_quote_in_v2_instruction(
        &runtime.global,
        &prepared.mint,
        &runtime.creator_vault_authority,
        &user,
        conversion.min_out,
        min_tokens_out,
        &prepared.token_program,
        &usdc_mint,
        &quote_token_program,
        token_mayhem_mode,
    )?;
    let user_base_account = get_associated_token_address_with_program_id(
        &user,
        &prepared.mint,
        &prepared.token_program,
    );
    let wrapper_ix = build_pump_usdc_buy_from_sol_route_instruction(
        &user,
        prepared.sol_amount,
        net_sol_amount,
        conversion,
        pump_ix,
        &user_usdc_account,
        &user_base_account,
        min_tokens_out,
        wrapper_fee_bps,
    )?;
    let instructions = vec![
        build_create_token_ata_instruction(&user, &usdc_mint, &quote_token_program)?,
        build_create_token_ata_instruction(&user, &prepared.mint, &prepared.token_program)?,
        wrapper_ix,
    ];
    compile_usdc_follow_transaction(
        rpc_url,
        "follow-buy",
        execution,
        user_keypair,
        instructions,
        &prepared.tx_config,
        execution.buyJitodontfront,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
fn build_pump_usdc_buy_from_sol_route_instruction(
    user: &Pubkey,
    gross_sol_in_lamports: u64,
    net_sol_in_lamports: u64,
    conversion: TrustedRaydiumClmmSwap,
    pump_ix: Instruction,
    user_usdc_account: &Pubkey,
    user_base_account: &Pubkey,
    min_tokens_out: u64,
    fee_bps: u16,
) -> Result<Instruction, String> {
    let route_wsol_account = route_wsol_pda(user, 0);
    let mut route_accounts = vec![
        AccountMeta::new_readonly(conversion.instruction.program_id, false),
        AccountMeta::new_readonly(pump_ix.program_id, false),
    ];
    let conversion_program_index = 0u16;
    let pump_program_index = 1u16;
    let conversion_accounts_start =
        route_len_u16(route_accounts.len(), "Pump USDC conversion route leg")?;
    route_accounts.extend(conversion.instruction.accounts.iter().cloned());
    let conversion_accounts_len = route_len_u16(
        conversion.instruction.accounts.len(),
        "Pump USDC conversion route leg",
    )?;
    let conversion_output_index = route_account_index(
        &route_accounts,
        user_usdc_account,
        "Pump USDC conversion output",
    )?;
    let pump_accounts_start = route_len_u16(route_accounts.len(), "Pump USDC buy route leg")?;
    route_accounts.extend(pump_ix.accounts.iter().cloned());
    let pump_accounts_len = route_len_u16(pump_ix.accounts.len(), "Pump USDC buy route leg")?;
    let pump_output_index =
        route_account_index(&route_accounts, user_base_account, "Pump USDC buy output")?;
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
        min_net_output: min_tokens_out,
        route_accounts_offset: EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT
            + EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT,
        intermediate_account_index: conversion_output_index,
        token_fee_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
        legs: vec![
            SwapRouteLeg {
                program_account_index: conversion_program_index,
                accounts_start: conversion_accounts_start,
                accounts_len: conversion_accounts_len,
                input_source: SwapLegInputSource::GrossSolNetOfFee,
                input_amount: net_sol_in_lamports,
                input_patch_offset: 8,
                output_account_index: conversion_output_index,
                ix_data: conversion.instruction.data,
            },
            SwapRouteLeg {
                program_account_index: pump_program_index,
                accounts_start: pump_accounts_start,
                accounts_len: pump_accounts_len,
                input_source: SwapLegInputSource::PreviousTokenDelta,
                input_amount: conversion.min_out,
                input_patch_offset: 8,
                output_account_index: pump_output_index,
                ix_data: pump_ix.data,
            },
        ],
    };
    build_execute_swap_route_instruction(
        user,
        &zeroed_wsol,
        &route_wsol_account,
        &conversion.instruction.program_id,
        &request,
        &route_accounts,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_pump_usdc_sell_to_sol_route_instruction(
    user: &Pubkey,
    pump_ix: Instruction,
    token_amount: u64,
    quote_amount_in: u64,
    user_usdc_account: &Pubkey,
    unwind: TrustedRaydiumClmmSwap,
    route_wsol_account: &Pubkey,
    fee_bps: u16,
) -> Result<Instruction, String> {
    let mut route_accounts = vec![
        AccountMeta::new_readonly(pump_ix.program_id, false),
        AccountMeta::new_readonly(unwind.instruction.program_id, false),
    ];
    let pump_program_index = 0u16;
    let unwind_program_index = 1u16;
    let pump_accounts_start = route_len_u16(route_accounts.len(), "Pump USDC sell route leg")?;
    route_accounts.extend(pump_ix.accounts.iter().cloned());
    let pump_accounts_len = route_len_u16(pump_ix.accounts.len(), "Pump USDC sell route leg")?;
    let pump_output_index =
        route_account_index(&route_accounts, user_usdc_account, "Pump USDC sell output")?;
    let unwind_accounts_start =
        route_len_u16(route_accounts.len(), "Pump USDC sell unwind route leg")?;
    route_accounts.extend(unwind.instruction.accounts.iter().cloned());
    let unwind_accounts_len = route_len_u16(
        unwind.instruction.accounts.len(),
        "Pump USDC sell unwind route leg",
    )?;
    let unwind_output_index = route_account_index(
        &route_accounts,
        route_wsol_account,
        "Pump USDC sell unwind output",
    )?;
    let min_net_sol_out = unwind
        .min_out
        .checked_sub(route_fee_lamports_floor(unwind.min_out, fee_bps)?)
        .ok_or_else(|| "Pump USDC sell unwind minimum output fee underflowed".to_string())?;
    let fee_vault_wsol_ata = get_associated_token_address_with_program_id(
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
        intermediate_account_index: pump_output_index,
        token_fee_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
        legs: vec![
            SwapRouteLeg {
                program_account_index: pump_program_index,
                accounts_start: pump_accounts_start,
                accounts_len: pump_accounts_len,
                input_source: SwapLegInputSource::Fixed,
                input_amount: token_amount,
                input_patch_offset: SWAP_ROUTE_NO_PATCH_OFFSET,
                output_account_index: pump_output_index,
                ix_data: pump_ix.data,
            },
            SwapRouteLeg {
                program_account_index: unwind_program_index,
                accounts_start: unwind_accounts_start,
                accounts_len: unwind_accounts_len,
                input_source: SwapLegInputSource::PreviousTokenDelta,
                input_amount: quote_amount_in,
                input_patch_offset: 8,
                output_account_index: unwind_output_index,
                ix_data: unwind.instruction.data,
            },
        ],
    };
    build_execute_swap_route_instruction(
        user,
        &fee_vault_wsol_ata,
        route_wsol_account,
        &pump_ix.program_id,
        &request,
        &route_accounts,
        None,
    )
}

async fn compile_usdc_follow_transaction(
    rpc_url: &str,
    label: &str,
    execution: &NormalizedExecution,
    user_keypair: &Keypair,
    instructions: Vec<Instruction>,
    tx_config: &NativeTxConfig,
    jitodontfront_enabled: bool,
) -> Result<CompiledTransaction, String> {
    let tx_instructions = with_tx_settings(
        instructions,
        tx_config,
        &user_keypair.pubkey(),
        jitodontfront_enabled,
    )?;
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, &execution.commitment).await?;
    let lookup_tables =
        load_shared_lookup_tables_for_tx_format(rpc_url, &execution.txFormat).await?;
    let lookup_table_variants = if lookup_tables.is_empty() {
        vec![]
    } else {
        vec![lookup_tables]
    };
    let tx_format = select_native_format(&execution.txFormat, !lookup_table_variants.is_empty())?;
    let (compiled, _) = compile_transaction_with_metrics(
        label,
        tx_format,
        &blockhash,
        last_valid_block_height,
        user_keypair,
        None,
        tx_instructions,
        tx_config,
        &lookup_table_variants,
    )?;
    Ok(compiled)
}

pub async fn finalize_follow_buy_transaction(
    rpc_url: &str,
    execution: &NormalizedExecution,
    token_mayhem_mode: bool,
    wallet_secret: &[u8],
    prepared: &PreparedFollowBuyStatic,
    runtime: &PreparedFollowBuyRuntime,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let user_keypair = keypair_from_secret_bytes(wallet_secret)?;
    let user = user_keypair.pubkey();
    if user != prepared.user {
        return Err("Prepared follow buy no longer matches the active wallet secret.".to_string());
    }
    let quote_mint = resolved_curve_quote_mint(&runtime.curve.quote_mint)?;
    if quote_mint == usdc_mint()? {
        return finalize_usdc_follow_buy_transaction(
            rpc_url,
            execution,
            token_mayhem_mode,
            &user_keypair,
            prepared,
            runtime,
            wrapper_fee_bps,
        )
        .await;
    }
    ensure_wsol_bonding_curve_quote(&runtime.curve)?;
    let net_sol_amount = wrapper_net_sol_input(prepared.sol_amount, wrapper_fee_bps)?;
    let token_amount = quote_buy_tokens_from_curve(&runtime.curve, &runtime.global, net_sol_amount);
    let min_tokens_out = apply_buy_token_slippage(
        token_amount,
        slippage_bps_from_percent(&execution.buySlippagePercent)?,
    );
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, &execution.commitment).await?;
    let lookup_tables =
        load_shared_lookup_tables_for_tx_format(rpc_url, &execution.txFormat).await?;
    let lookup_table_variants = if lookup_tables.is_empty() {
        vec![]
    } else {
        vec![lookup_tables]
    };
    let tx_format = select_native_format(&execution.txFormat, !lookup_table_variants.is_empty())?;
    let instructions = vec![
        build_create_token_ata_instruction(&user, &prepared.mint, &prepared.token_program)?,
        build_buy_exact_sol_in_instruction(
            &runtime.global,
            &prepared.mint,
            &runtime.creator_vault_authority,
            &user,
            net_sol_amount,
            min_tokens_out,
            &prepared.token_program,
            token_mayhem_mode,
        )?,
    ];
    let tx_instructions = with_tx_settings(
        instructions,
        &prepared.tx_config,
        &user,
        execution.buyJitodontfront,
    )?;
    let (compiled, _) = compile_transaction_with_metrics(
        "follow-buy",
        tx_format,
        &blockhash,
        last_valid_block_height,
        &user_keypair,
        None,
        tx_instructions,
        &prepared.tx_config,
        &lookup_table_variants,
    )?;
    Ok(compiled)
}

pub async fn compile_atomic_follow_buy_transaction(
    rpc_url: &str,
    execution: &NormalizedExecution,
    token_mayhem_mode: bool,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    launch_creator: &str,
    buy_amount_sol: &str,
    predicted_prior_buy_token_amount: Option<u64>,
    cashback_enabled_override: Option<bool>,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let user_keypair = keypair_from_secret_bytes(wallet_secret)?;
    let user = user_keypair.pubkey();
    let mint = parse_pubkey(mint, "mint")?;
    let launch_creator = parse_pubkey(launch_creator, "launch creator")?;
    let token_program = token_2022_program_id()?;
    let global = fetch_global_state_cached(rpc_url).await?;
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, &execution.commitment).await?;
    let sol_amount = parse_decimal_u64(buy_amount_sol, 9, "followLaunch.snipes.buyAmountSol")?;
    let net_sol_amount = wrapper_net_sol_input(sol_amount, wrapper_fee_bps)?;
    let buy_slippage_bps = slippage_bps_from_percent(&execution.buySlippagePercent)?;
    let tokens_out = if let Some(token_amount) = predicted_prior_buy_token_amount {
        let curve = synthetic_curve_after_buy_tokens(
            &global,
            &launch_creator,
            token_amount,
            cashback_enabled_override.unwrap_or(false),
        );
        quote_buy_tokens_from_curve(&curve, &global, net_sol_amount)
    } else {
        quote_buy_tokens_from_sol(&global, net_sol_amount)
    };
    let instructions = vec![
        build_create_token_ata_instruction(&user, &mint, &token_program)?,
        build_buy_exact_sol_in_instruction(
            &global,
            &mint,
            &launch_creator,
            &user,
            net_sol_amount,
            apply_buy_token_slippage(tokens_out, buy_slippage_bps),
            &token_program,
            token_mayhem_mode,
        )?,
    ];
    let (jito_tip_lamports, jito_tip_account) = resolve_follow_tip_config(
        &execution.buyProvider,
        &execution.buyTipSol,
        jito_tip_account,
        "buy tip",
    )?;
    let tx_config = NativeTxConfig {
        compute_unit_limit: u64_to_u32_limit(
            configured_default_sniper_buy_compute_unit_limit(),
            "sniper buy compute unit limit",
        )?,
        compute_unit_price_micro_lamports: priority_fee_sol_to_micro_lamports(
            &execution.buyPriorityFeeSol,
        )? as i64,
        jito_tip_lamports,
        jito_tip_account,
    };
    let tx_instructions =
        with_tx_settings(instructions, &tx_config, &user, execution.buyJitodontfront)?;
    let lookup_tables =
        load_shared_lookup_tables_for_tx_format(rpc_url, &execution.txFormat).await?;
    let lookup_table_variants = if lookup_tables.is_empty() {
        vec![]
    } else {
        vec![lookup_tables]
    };
    let tx_format = select_native_format(&execution.txFormat, !lookup_table_variants.is_empty())?;
    let (compiled, _) = compile_transaction_with_metrics(
        "follow-buy-atomic",
        tx_format,
        &blockhash,
        last_valid_block_height,
        &user_keypair,
        None,
        tx_instructions,
        &tx_config,
        &lookup_table_variants,
    )?;
    Ok(compiled)
}

pub async fn compile_follow_sell_transaction(
    rpc_url: &str,
    execution: &NormalizedExecution,
    token_mayhem_mode: bool,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    launch_creator: &str,
    sell_percent: u8,
    prefer_post_setup_creator_vault: bool,
) -> Result<Option<CompiledTransaction>, String> {
    compile_follow_sell_transaction_with_token_amount(
        rpc_url,
        execution,
        token_mayhem_mode,
        jito_tip_account,
        wallet_secret,
        mint,
        launch_creator,
        sell_percent,
        prefer_post_setup_creator_vault,
        None,
        None,
        10,
    )
    .await
}

pub async fn compile_follow_sell_transaction_with_token_amount(
    rpc_url: &str,
    execution: &NormalizedExecution,
    token_mayhem_mode: bool,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    launch_creator: &str,
    sell_percent: u8,
    prefer_post_setup_creator_vault: bool,
    token_amount_override: Option<u64>,
    cashback_enabled_override: Option<bool>,
    wrapper_fee_bps: u16,
) -> Result<Option<CompiledTransaction>, String> {
    let user_keypair = keypair_from_secret_bytes(wallet_secret)?;
    let user = user_keypair.pubkey();
    let mint = parse_pubkey(mint, "mint")?;
    let launch_creator = parse_pubkey(launch_creator, "launch creator")?;
    let token_program =
        resolve_pump_bonding_mint_token_program(rpc_url, &mint, &execution.commitment).await?;
    let creator_vault_authority = resolve_follow_creator_vault_authority(
        rpc_url,
        &mint,
        &launch_creator,
        prefer_post_setup_creator_vault,
    )
    .await?;
    let global = fetch_global_state_cached(rpc_url).await?;
    let curve = if let Some(token_amount_override) = token_amount_override {
        synthetic_curve_after_buy_tokens(
            &global,
            &launch_creator,
            token_amount_override,
            cashback_enabled_override.unwrap_or(false),
        )
    } else {
        fetch_bonding_curve_state(rpc_url, &mint).await?
    };
    let token_amount = if let Some(token_amount_override) = token_amount_override {
        ((u128::from(token_amount_override) * u128::from(sell_percent)) / 100u128)
            .min(u128::from(u64::MAX)) as u64
    } else {
        let associated_user =
            get_associated_token_address_with_program_id(&user, &mint, &token_program);
        let account_key = associated_user.to_string();
        let mut account_data = None;
        let mut last_error = None;
        for attempt in 0..15 {
            match fetch_account_data(rpc_url, &account_key, &execution.commitment).await {
                Ok(data) => {
                    account_data = Some(data);
                    last_error = None;
                    break;
                }
                Err(error) if error.contains("was not found") && attempt < 14 => {
                    last_error = Some(error);
                    sleep(Duration::from_millis(200)).await;
                }
                Err(error) => return Err(error),
            }
        }
        let account_data = account_data.ok_or_else(|| {
            last_error.unwrap_or_else(|| format!("Account {account_key} was not found."))
        })?;
        let token_balance = read_token_account_amount(&account_data)?;
        ((u128::from(token_balance) * u128::from(sell_percent)) / 100u128).min(u128::from(u64::MAX))
            as u64
    };
    if token_amount == 0 {
        return Ok(None);
    }
    let (jito_tip_lamports, jito_tip_account) = resolve_follow_tip_config(
        &execution.sellProvider,
        &execution.sellTipSol,
        jito_tip_account,
        "sell tip",
    )?;
    let tx_config = NativeTxConfig {
        compute_unit_limit: u64_to_u32_limit(
            configured_default_dev_auto_sell_compute_unit_limit(),
            "dev auto sell compute unit limit",
        )?,
        compute_unit_price_micro_lamports: priority_fee_sol_to_micro_lamports(
            &execution.sellPriorityFeeSol,
        )? as i64,
        jito_tip_lamports,
        jito_tip_account,
    };
    let quote_mint = resolved_curve_quote_mint(&curve.quote_mint)?;
    if quote_mint == usdc_mint()? {
        return compile_usdc_follow_sell_transaction(
            rpc_url,
            execution,
            token_mayhem_mode,
            &user_keypair,
            &mint,
            &token_program,
            &global,
            &curve,
            &creator_vault_authority,
            token_amount,
            &tx_config,
            wrapper_fee_bps,
        )
        .await
        .map(Some);
    }
    ensure_wsol_bonding_curve_quote(&curve)?;
    let gross_quote = quote_sell_sol_from_curve(&curve, &global, token_amount);
    let slippage_bps = slippage_bps_from_percent(&execution.sellSlippagePercent)?;
    let min_sol_output = apply_sell_side_slippage(gross_quote, slippage_bps);
    let min_net_sol_output = min_sol_output
        .checked_sub(estimate_sol_in_fee_lamports(
            min_sol_output,
            wrapper_fee_bps,
        ))
        .ok_or_else(|| "Pump follow sell minimum output fee underflowed".to_string())?;
    let (blockhash, last_valid_block_height) =
        fetch_latest_blockhash_cached(rpc_url, &execution.commitment).await?;
    let pump_ix = build_sell_instruction(
        &global,
        &mint,
        &creator_vault_authority,
        &user,
        token_amount,
        min_sol_output,
        &token_program,
        curve.cashback_enabled,
        token_mayhem_mode,
    )?;
    let wrapper_ix = build_pump_bonding_v2_sell_wrapper_instruction(
        &user,
        pump_ix,
        token_amount,
        min_sol_output,
        min_net_sol_output,
        wrapper_fee_bps,
    )?;
    let instructions = vec![
        build_create_token_ata_instruction(&user, &wsol_mint()?, &token_program_id()?)?,
        build_create_token_ata_for_owner_instruction(
            &user,
            &wrapper_fee_vault(),
            &wsol_mint()?,
            &token_program_id()?,
        )?,
        wrapper_ix,
    ];
    let tx_instructions =
        with_tx_settings(instructions, &tx_config, &user, execution.sellJitodontfront)?;
    let lookup_tables =
        load_shared_lookup_tables_for_tx_format(rpc_url, &execution.txFormat).await?;
    let lookup_table_variants = if lookup_tables.is_empty() {
        vec![]
    } else {
        vec![lookup_tables]
    };
    let tx_format = select_native_format(&execution.txFormat, !lookup_table_variants.is_empty())?;
    let (compiled, _) = compile_transaction_with_metrics(
        "follow-sell",
        tx_format,
        &blockhash,
        last_valid_block_height,
        &user_keypair,
        None,
        tx_instructions,
        &tx_config,
        &lookup_table_variants,
    )?;
    Ok(Some(compiled))
}

#[allow(clippy::too_many_arguments)]
async fn compile_usdc_follow_sell_transaction(
    rpc_url: &str,
    execution: &NormalizedExecution,
    token_mayhem_mode: bool,
    user_keypair: &Keypair,
    mint: &Pubkey,
    token_program: &Pubkey,
    global: &PumpGlobalState,
    curve: &PumpBondingCurveState,
    creator_vault_authority: &Pubkey,
    token_amount: u64,
    tx_config: &NativeTxConfig,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let user = user_keypair.pubkey();
    let usdc_mint = usdc_mint()?;
    let wsol_mint = wsol_mint()?;
    let quote_token_program = token_program_id()?;
    let user_usdc_account =
        get_associated_token_address_with_program_id(&user, &usdc_mint, &quote_token_program);
    let route_wsol_account = route_wsol_pda(&user, 0);
    let expected_usdc_out = quote_sell_quote_from_curve(curve, global, token_amount);
    if expected_usdc_out == 0 {
        return Err("Pump USDC follow sell quote resolved to zero USDC.".to_string());
    }
    let slippage_bps = slippage_bps_from_percent(&execution.sellSlippagePercent)?;
    let (pump_slippage_bps, unwind_slippage_bps) = split_two_leg_slippage_bps(slippage_bps);
    let min_usdc_out = apply_sell_side_slippage(expected_usdc_out, pump_slippage_bps);
    let pump_ix = build_sell_v2_instruction(
        global,
        mint,
        creator_vault_authority,
        &user,
        token_amount,
        min_usdc_out,
        token_program,
        &usdc_mint,
        &quote_token_program,
        curve.cashback_enabled,
        token_mayhem_mode,
    )?;
    let unwind = build_trusted_raydium_clmm_swap_exact_in(
        rpc_url,
        RAYDIUM_SOL_USDC_POOL,
        &execution.commitment,
        &user,
        &user_usdc_account,
        &route_wsol_account,
        &usdc_mint,
        &wsol_mint,
        expected_usdc_out,
        unwind_slippage_bps,
    )
    .await?;
    let wrapper_ix = build_pump_usdc_sell_to_sol_route_instruction(
        &user,
        pump_ix,
        token_amount,
        min_usdc_out,
        &user_usdc_account,
        unwind,
        &route_wsol_account,
        wrapper_fee_bps,
    )?;
    let instructions = vec![
        build_create_token_ata_instruction(&user, &usdc_mint, &quote_token_program)?,
        build_create_token_ata_instruction(&user, mint, token_program)?,
        wrapper_ix,
    ];
    compile_usdc_follow_transaction(
        rpc_url,
        "follow-sell",
        execution,
        user_keypair,
        instructions,
        tx_config,
        execution.sellJitodontfront,
    )
    .await
}

fn apply_buy_token_slippage(token_amount: u64, slippage_bps: u64) -> u64 {
    let minimum = (u128::from(token_amount)
        * u128::from(10_000u64.saturating_sub(slippage_bps.min(10_000))))
        / 10_000u128;
    let minimum = minimum.min(u128::from(u64::MAX)) as u64;
    if token_amount > 0 && minimum == 0 {
        1
    } else {
        minimum
    }
}

fn apply_sell_side_slippage(value: u64, slippage_bps: u64) -> u64 {
    let minimum = (u128::from(value)
        * u128::from(10_000u64.saturating_sub(slippage_bps.min(10_000))))
        / 10_000u128;
    let minimum = minimum.min(u128::from(u64::MAX)) as u64;
    if value > 0 && minimum == 0 {
        1
    } else {
        minimum
    }
}

fn select_buy_fee_recipient(global: &PumpGlobalState, mayhem_mode: bool) -> Pubkey {
    if mayhem_mode {
        if global.reserved_fee_recipient != Pubkey::default() {
            return global.reserved_fee_recipient;
        }
        if let Some(entry) = global
            .reserved_fee_recipients
            .iter()
            .copied()
            .find(|entry| *entry != Pubkey::default())
        {
            return entry;
        }
    } else {
        if global.fee_recipient != Pubkey::default() {
            return global.fee_recipient;
        }
        if let Some(entry) = global
            .fee_recipients
            .iter()
            .copied()
            .find(|entry| *entry != Pubkey::default())
        {
            return entry;
        }
    }
    Pubkey::default()
}

fn select_buyback_fee_recipient(global: &PumpGlobalState) -> Pubkey {
    global
        .buyback_fee_recipients
        .iter()
        .copied()
        .find(|entry| *entry != Pubkey::default())
        .unwrap_or_default()
}

fn resolved_curve_quote_mint(quote_mint: &Pubkey) -> Result<Pubkey, String> {
    if *quote_mint == Pubkey::default() {
        wsol_mint()
    } else {
        Ok(*quote_mint)
    }
}

fn ensure_wsol_bonding_curve_quote(curve: &PumpBondingCurveState) -> Result<(), String> {
    let quote_mint = resolved_curve_quote_mint(&curve.quote_mint)?;
    if quote_mint == wsol_mint()? {
        Ok(())
    } else {
        Err(format!(
            "LaunchDeck native Pump bonding flows only support WSOL-quoted curves; curve uses quote mint {quote_mint}."
        ))
    }
}

fn sol_vault_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"sol-vault"], &mayhem_program_id()?).0)
}

fn mayhem_state_pda(mint: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"mayhem-state", mint.as_ref()], &mayhem_program_id()?).0)
}

async fn resolve_launch_creator_and_pre_instructions(
    rpc_url: &str,
    config: &NormalizedConfig,
    creator: &Pubkey,
) -> Result<(Pubkey, Vec<Instruction>), String> {
    if config.creatorFee.mode == "wallet" && !config.creatorFee.address.is_empty() {
        return Ok((
            parse_pubkey(&config.creatorFee.address, "creatorFee.address")?,
            vec![],
        ));
    }
    if config.creatorFee.mode == "github" && !config.creatorFee.githubUserId.is_empty() {
        let social_fee = social_fee_pda(&config.creatorFee.githubUserId, PLATFORM_GITHUB)?;
        let mut pre_instructions = Vec::new();
        if !fetch_account_exists(rpc_url, &social_fee.to_string(), "confirmed").await? {
            pre_instructions.push(build_create_social_fee_pda_instruction(
                creator,
                &config.creatorFee.githubUserId,
                PLATFORM_GITHUB,
            )?);
        }
        return Ok((social_fee, pre_instructions));
    }
    Ok((*creator, vec![]))
}

fn resolve_agent_authority(
    config: &NormalizedConfig,
    creator: &Pubkey,
) -> Result<Option<Pubkey>, String> {
    if config.mode == "regular" || config.mode == "cashback" {
        return Ok(None);
    }
    if config.agent.authority.trim().is_empty() {
        return Ok(Some(*creator));
    }
    Ok(Some(parse_pubkey(
        &config.agent.authority,
        "agent.authority",
    )?))
}

fn build_launch_instructions(
    config: &NormalizedConfig,
    mint: Pubkey,
    creator: Pubkey,
    launch_creator: Pubkey,
    agent_authority: Option<&Pubkey>,
    global: Option<&PumpGlobalState>,
) -> Result<Vec<Instruction>, String> {
    if config.quoteAsset.eq_ignore_ascii_case("usdc") {
        return Err(
            "Pump USDC quote-asset creation is not enabled yet; LaunchDeck keeps quoteAsset=usdc gated until Pump publishes the create-v2 USDC account/data shape."
                .to_string(),
        );
    }
    let mut instructions = vec![build_create_v2_instruction(
        &mint,
        &creator,
        &launch_creator,
        &config.token.name,
        &config.token.symbol,
        &config.token.uri,
        config.token.mayhemMode,
        config.mode == "cashback",
    )?];
    let mut launch_dev_buy_fee_transfer = None;
    if let Some(global) = global {
        if let Some((sol_amount, token_amount)) = resolve_dev_buy_quote(config, global)? {
            let buy_slippage_bps = slippage_bps_from_percent(&config.execution.buySlippagePercent)?;
            instructions.push(build_extend_account_instruction(
                &bonding_curve_pda(&mint)?,
                &creator,
            )?);
            let token_2022 = token_2022_program_id()?;
            instructions.push(build_create_token_ata_instruction(
                &creator,
                &mint,
                &token_2022,
            )?);
            instructions.push(build_buy_exact_sol_in_instruction(
                global,
                &mint,
                &launch_creator,
                &creator,
                sol_amount,
                apply_buy_token_slippage(token_amount, buy_slippage_bps),
                &token_2022,
                config.token.mayhemMode,
            )?);
            launch_dev_buy_fee_transfer =
                build_launch_dev_buy_fee_transfer_instruction(config, creator, sol_amount);
        }
    }
    if config.mode == "agent-custom" && !config.agent.splitAgentInit {
        let authority = agent_authority
            .ok_or_else(|| format!("agent authority is required for {} mode.", config.mode))?;
        instructions.push(build_agent_initialize_instruction(
            &mint,
            &creator,
            authority,
            config.agent.buybackBps.unwrap_or(0) as u16,
        )?);
    }
    if let Some(fee_transfer) = launch_dev_buy_fee_transfer {
        instructions.push(fee_transfer);
    }
    Ok(instructions)
}

fn encode_borsh_string(buffer: &mut Vec<u8>, value: &str) {
    buffer.extend_from_slice(&(value.len() as u32).to_le_bytes());
    buffer.extend_from_slice(value.as_bytes());
}

fn build_create_v2_instruction(
    mint: &Pubkey,
    user: &Pubkey,
    creator: &Pubkey,
    name: &str,
    symbol: &str,
    uri: &str,
    mayhem_mode: bool,
    cashback_enabled: bool,
) -> Result<Instruction, String> {
    let pump_program = pump_program_id()?;
    let token_2022 = token_2022_program_id()?;
    let associated_bonding_curve =
        get_associated_token_address_with_program_id(&bonding_curve_pda(mint)?, mint, &token_2022);
    let sol_vault = sol_vault_pda()?;
    let mayhem_state = mayhem_state_pda(mint)?;
    let mayhem_token_vault =
        get_associated_token_address_with_program_id(&sol_vault, mint, &token_2022);

    let mut data = vec![214, 144, 76, 236, 95, 139, 49, 180];
    encode_borsh_string(&mut data, name);
    encode_borsh_string(&mut data, symbol);
    encode_borsh_string(&mut data, uri);
    data.extend_from_slice(creator.as_ref());
    data.push(u8::from(mayhem_mode));
    data.push(u8::from(cashback_enabled));

    let instruction = Instruction {
        program_id: pump_program,
        accounts: vec![
            AccountMeta::new(*mint, true),
            AccountMeta::new_readonly(mint_authority_pda()?, false),
            AccountMeta::new(bonding_curve_pda(mint)?, false),
            AccountMeta::new(associated_bonding_curve, false),
            AccountMeta::new_readonly(global_pda()?, false),
            AccountMeta::new(*user, true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(token_2022, false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new(mayhem_program_id()?, false),
            AccountMeta::new_readonly(global_params_pda()?, false),
            AccountMeta::new(sol_vault, false),
            AccountMeta::new(mayhem_state, false),
            AccountMeta::new(mayhem_token_vault, false),
            AccountMeta::new_readonly(event_authority_pda(&pump_program), false),
            AccountMeta::new_readonly(pump_program, false),
        ],
        data,
    };
    Ok(instruction)
}

fn build_jito_tip_instruction(
    config: &NormalizedConfig,
    payer: Pubkey,
) -> Result<Instruction, String> {
    let tip_account = parse_pubkey(&config.tx.jitoTipAccount, "tx.jitoTipAccount")?;
    Ok(transfer(
        &payer,
        &tip_account,
        config.tx.jitoTipLamports as u64,
    ))
}

fn build_launch_dev_buy_fee_transfer_instruction(
    config: &NormalizedConfig,
    payer: Pubkey,
    dev_buy_lamports: u64,
) -> Option<Instruction> {
    let fee_lamports = estimate_sol_in_fee_lamports(dev_buy_lamports, config.wrapperDefaultFeeBps);
    if fee_lamports == 0 {
        return None;
    }
    Some(transfer(&payer, &wrapper_fee_vault(), fee_lamports))
}

fn build_agent_initialize_instruction(
    mint: &Pubkey,
    creator: &Pubkey,
    agent_authority: &Pubkey,
    buyback_bps: u16,
) -> Result<Instruction, String> {
    let program_id = pump_agent_payments_program_id()?;
    let mut data = vec![180, 248, 163, 8, 49, 94, 126, 96];
    data.extend_from_slice(agent_authority.as_ref());
    data.extend_from_slice(&buyback_bps.to_le_bytes());
    let instruction = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(*creator, true),
            AccountMeta::new_readonly(bonding_curve_pda(mint)?, false),
            AccountMeta::new(agent_global_config_pda()?, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(token_agent_payments_pda(mint)?, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(event_authority_pda(&program_id), false),
            AccountMeta::new_readonly(program_id, false),
        ],
        data,
    };
    Ok(instruction)
}

fn build_extend_account_instruction(
    account: &Pubkey,
    user: &Pubkey,
) -> Result<Instruction, String> {
    let program_id = pump_program_id()?;
    Ok(Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(*account, false),
            AccountMeta::new_readonly(*user, true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(event_authority_pda(&program_id), false),
            AccountMeta::new_readonly(program_id, false),
        ],
        data: vec![234, 102, 194, 203, 150, 72, 62, 229],
    })
}

fn build_create_token_ata_instruction(
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Result<Instruction, String> {
    build_create_token_ata_for_owner_instruction(owner, owner, mint, token_program)
}

fn build_create_token_ata_for_owner_instruction(
    payer: &Pubkey,
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Result<Instruction, String> {
    Ok(
        spl_associated_token_account::instruction::create_associated_token_account_idempotent(
            payer,
            owner,
            mint,
            token_program,
        ),
    )
}

fn build_buy_exact_sol_in_instruction(
    global: &PumpGlobalState,
    mint: &Pubkey,
    launch_creator: &Pubkey,
    user: &Pubkey,
    spendable_sol_in: u64,
    min_tokens_out: u64,
    base_token_program: &Pubkey,
    mayhem_mode: bool,
) -> Result<Instruction, String> {
    build_buy_exact_quote_in_v2_instruction(
        global,
        mint,
        launch_creator,
        user,
        spendable_sol_in,
        min_tokens_out,
        base_token_program,
        &wsol_mint()?,
        &token_program_id()?,
        mayhem_mode,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_buy_exact_quote_in_v2_instruction(
    global: &PumpGlobalState,
    mint: &Pubkey,
    creator_vault_authority: &Pubkey,
    user: &Pubkey,
    quote_amount_in: u64,
    min_tokens_out: u64,
    base_token_program: &Pubkey,
    quote_mint: &Pubkey,
    quote_token_program: &Pubkey,
    mayhem_mode: bool,
) -> Result<Instruction, String> {
    let pump_program = pump_program_id()?;
    let bonding_curve = bonding_curve_pda(mint)?;
    let user_volume_accumulator = user_volume_accumulator_pda(user)?;
    let associated_bonding_curve =
        get_associated_token_address_with_program_id(&bonding_curve, mint, base_token_program);
    let associated_user =
        get_associated_token_address_with_program_id(user, mint, base_token_program);
    let associated_quote_user =
        get_associated_token_address_with_program_id(user, quote_mint, quote_token_program);
    let associated_quote_bonding_curve = get_associated_token_address_with_program_id(
        &bonding_curve,
        quote_mint,
        quote_token_program,
    );
    let fee_recipient = select_buy_fee_recipient(global, mayhem_mode);
    let buyback_fee_recipient = select_buyback_fee_recipient(global);
    let associated_quote_fee_recipient = get_associated_token_address_with_program_id(
        &fee_recipient,
        quote_mint,
        quote_token_program,
    );
    let associated_quote_buyback_fee_recipient = get_associated_token_address_with_program_id(
        &buyback_fee_recipient,
        quote_mint,
        quote_token_program,
    );
    let creator_vault = creator_vault_pda(creator_vault_authority)?;
    let associated_creator_vault = get_associated_token_address_with_program_id(
        &creator_vault,
        quote_mint,
        quote_token_program,
    );
    let associated_user_volume_accumulator = get_associated_token_address_with_program_id(
        &user_volume_accumulator,
        quote_mint,
        quote_token_program,
    );
    let mut data = vec![194, 171, 28, 70, 104, 77, 91, 47];
    data.extend_from_slice(&quote_amount_in.to_le_bytes());
    data.extend_from_slice(&min_tokens_out.to_le_bytes());
    Ok(Instruction {
        program_id: pump_program,
        accounts: vec![
            AccountMeta::new_readonly(global_pda()?, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(*quote_mint, false),
            AccountMeta::new_readonly(*base_token_program, false),
            AccountMeta::new_readonly(*quote_token_program, false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new(fee_recipient, false),
            AccountMeta::new(associated_quote_fee_recipient, false),
            AccountMeta::new(buyback_fee_recipient, false),
            AccountMeta::new(associated_quote_buyback_fee_recipient, false),
            AccountMeta::new(bonding_curve, false),
            AccountMeta::new(associated_bonding_curve, false),
            AccountMeta::new(associated_quote_bonding_curve, false),
            AccountMeta::new(*user, true),
            AccountMeta::new(associated_user, false),
            AccountMeta::new(associated_quote_user, false),
            AccountMeta::new(creator_vault, false),
            AccountMeta::new(associated_creator_vault, false),
            AccountMeta::new(fee_sharing_config_pda(mint)?, false),
            AccountMeta::new_readonly(global_volume_accumulator_pda()?, false),
            AccountMeta::new(user_volume_accumulator, false),
            AccountMeta::new(associated_user_volume_accumulator, false),
            AccountMeta::new_readonly(fee_config_pda()?, false),
            AccountMeta::new_readonly(pump_fee_program_id()?, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(event_authority_pda(&pump_program), false),
            AccountMeta::new_readonly(pump_program, false),
        ],
        data,
    })
}

fn build_sell_instruction(
    global: &PumpGlobalState,
    mint: &Pubkey,
    launch_creator: &Pubkey,
    user: &Pubkey,
    token_amount: u64,
    min_sol_output: u64,
    base_token_program: &Pubkey,
    cashback_enabled: bool,
    mayhem_mode: bool,
) -> Result<Instruction, String> {
    build_sell_v2_instruction(
        global,
        mint,
        launch_creator,
        user,
        token_amount,
        min_sol_output,
        base_token_program,
        &wsol_mint()?,
        &token_program_id()?,
        cashback_enabled,
        mayhem_mode,
    )
}

fn build_pump_bonding_v2_sell_wrapper_instruction(
    user: &Pubkey,
    pump_ix: Instruction,
    base_amount_in: u64,
    gross_min_quote_out_amount: u64,
    net_min_quote_out_amount: u64,
    fee_bps: u16,
) -> Result<Instruction, String> {
    let fee_vault_quote_ata = get_associated_token_address_with_program_id(
        &wrapper_fee_vault(),
        &wrapper_wsol_mint(),
        &wrapper_token_program_id(),
    );
    let request = ExecutePumpBondingV2Request {
        version: WRAPPER_ABI_VERSION,
        side: PumpBondingV2Side::Sell,
        quote_fee_mode: PumpBondingV2QuoteFeeMode::Wsol,
        fee_bps,
        gross_quote_in_amount: 0,
        min_base_out_amount: 0,
        base_amount_in,
        gross_min_quote_out_amount,
        net_min_quote_out_amount,
    };
    build_execute_pump_bonding_v2_instruction(
        user,
        &fee_vault_quote_ata,
        &pump_ix.program_id,
        &request,
        &pump_ix.accounts,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_sell_v2_instruction(
    global: &PumpGlobalState,
    mint: &Pubkey,
    creator_vault_authority: &Pubkey,
    user: &Pubkey,
    token_amount: u64,
    min_quote_output: u64,
    base_token_program: &Pubkey,
    quote_mint: &Pubkey,
    quote_token_program: &Pubkey,
    _cashback_enabled: bool,
    mayhem_mode: bool,
) -> Result<Instruction, String> {
    let pump_program = pump_program_id()?;
    let bonding_curve = bonding_curve_pda(mint)?;
    let user_volume_accumulator = user_volume_accumulator_pda(user)?;
    let associated_bonding_curve =
        get_associated_token_address_with_program_id(&bonding_curve, mint, base_token_program);
    let associated_user =
        get_associated_token_address_with_program_id(user, mint, base_token_program);
    let associated_quote_user =
        get_associated_token_address_with_program_id(user, quote_mint, quote_token_program);
    let associated_quote_bonding_curve = get_associated_token_address_with_program_id(
        &bonding_curve,
        quote_mint,
        quote_token_program,
    );
    let fee_recipient = select_buy_fee_recipient(global, mayhem_mode);
    let associated_quote_fee_recipient = get_associated_token_address_with_program_id(
        &fee_recipient,
        quote_mint,
        quote_token_program,
    );
    let creator_vault = creator_vault_pda(creator_vault_authority)?;
    let associated_creator_vault = get_associated_token_address_with_program_id(
        &creator_vault,
        quote_mint,
        quote_token_program,
    );
    let buyback_fee_recipient = select_buyback_fee_recipient(global);
    let associated_quote_buyback_fee_recipient = get_associated_token_address_with_program_id(
        &buyback_fee_recipient,
        quote_mint,
        quote_token_program,
    );
    let associated_user_volume_accumulator = get_associated_token_address_with_program_id(
        &user_volume_accumulator,
        quote_mint,
        quote_token_program,
    );
    let mut data = vec![93, 246, 130, 60, 231, 233, 64, 178];
    data.extend_from_slice(&token_amount.to_le_bytes());
    data.extend_from_slice(&min_quote_output.to_le_bytes());
    Ok(Instruction {
        program_id: pump_program,
        accounts: vec![
            AccountMeta::new_readonly(global_pda()?, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(*quote_mint, false),
            AccountMeta::new_readonly(*base_token_program, false),
            AccountMeta::new_readonly(*quote_token_program, false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new(fee_recipient, false),
            AccountMeta::new(associated_quote_fee_recipient, false),
            AccountMeta::new(buyback_fee_recipient, false),
            AccountMeta::new(associated_quote_buyback_fee_recipient, false),
            AccountMeta::new(bonding_curve, false),
            AccountMeta::new(associated_bonding_curve, false),
            AccountMeta::new(associated_quote_bonding_curve, false),
            AccountMeta::new(*user, true),
            AccountMeta::new(associated_user, false),
            AccountMeta::new(associated_quote_user, false),
            AccountMeta::new(creator_vault, false),
            AccountMeta::new(associated_creator_vault, false),
            AccountMeta::new(fee_sharing_config_pda(mint)?, false),
            AccountMeta::new(user_volume_accumulator, false),
            AccountMeta::new(associated_user_volume_accumulator, false),
            AccountMeta::new_readonly(fee_config_pda()?, false),
            AccountMeta::new_readonly(pump_fee_program_id()?, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(event_authority_pda(&pump_program), false),
            AccountMeta::new_readonly(pump_program, false),
        ],
        data,
    })
}

fn read_token_account_amount(data: &[u8]) -> Result<u64, String> {
    if data.len() < 72 {
        return Err("Token account data was shorter than expected.".to_string());
    }
    let amount_bytes: [u8; 8] = data[64..72]
        .try_into()
        .map_err(|_| "Token account amount bytes were invalid.".to_string())?;
    Ok(u64::from_le_bytes(amount_bytes))
}

fn recipient_pubkey(recipient: &NormalizedRecipient) -> Result<Pubkey, String> {
    if !recipient.address.is_empty() {
        return parse_pubkey(&recipient.address, "feeSharing.recipient.address");
    }
    if !recipient.githubUserId.is_empty() {
        return social_fee_pda(&recipient.githubUserId, PLATFORM_GITHUB);
    }
    Err("Each fee-sharing recipient must provide either an address or githubUserId.".to_string())
}

fn encode_shareholders(recipients: &[NormalizedRecipient]) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    data.extend_from_slice(&(recipients.len() as u32).to_le_bytes());
    for recipient in recipients {
        let address = recipient_pubkey(recipient)?;
        data.extend_from_slice(address.as_ref());
        let share_bps = u16::try_from(recipient.shareBps).map_err(|_| {
            format!(
                "shareBps is out of range for recipient {}",
                recipient.address
            )
        })?;
        data.extend_from_slice(&share_bps.to_le_bytes());
    }
    Ok(data)
}

fn build_create_fee_sharing_config_instruction(
    mint: &Pubkey,
    payer: &Pubkey,
) -> Result<Instruction, String> {
    let program_id = pump_fee_program_id()?;
    Ok(Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(event_authority_pda(&program_id), false),
            AccountMeta::new_readonly(program_id, false),
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(global_pda()?, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(fee_sharing_config_pda(mint)?, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new(bonding_curve_pda(mint)?, false),
            AccountMeta::new_readonly(pump_program_id()?, false),
            AccountMeta::new_readonly(event_authority_pda(&pump_program_id()?), false),
            AccountMeta::new_readonly(program_id, false),
            AccountMeta::new_readonly(pump_amm_program_id()?, false),
            AccountMeta::new_readonly(program_id, false),
        ],
        data: vec![195, 78, 86, 76, 111, 52, 251, 213],
    })
}

fn build_update_fee_shares_instruction(
    mint: &Pubkey,
    authority: &Pubkey,
    current_shareholders: &[Pubkey],
    recipients: &[crate::config::NormalizedRecipient],
) -> Result<Instruction, String> {
    let program_id = pump_fee_program_id()?;
    let sharing_config = fee_sharing_config_pda(mint)?;
    let coin_creator_vault_authority = coin_creator_vault_authority_pda(&sharing_config)?;
    let mut data = vec![189, 13, 136, 99, 187, 164, 237, 35];
    data.extend_from_slice(&encode_shareholders(recipients)?);
    let mut accounts = vec![
        AccountMeta::new_readonly(event_authority_pda(&program_id), false),
        AccountMeta::new_readonly(program_id, false),
        AccountMeta::new_readonly(*authority, true),
        AccountMeta::new_readonly(global_pda()?, false),
        AccountMeta::new_readonly(*mint, false),
        AccountMeta::new(sharing_config, false),
        AccountMeta::new_readonly(bonding_curve_pda(mint)?, false),
        AccountMeta::new(creator_vault_pda(&sharing_config)?, false),
        AccountMeta::new_readonly(system_program::id(), false),
        AccountMeta::new_readonly(pump_program_id()?, false),
        AccountMeta::new_readonly(event_authority_pda(&pump_program_id()?), false),
        AccountMeta::new_readonly(pump_amm_program_id()?, false),
        AccountMeta::new_readonly(event_authority_pda(&pump_amm_program_id()?), false),
        AccountMeta::new_readonly(wsol_mint()?, false),
        AccountMeta::new_readonly(token_program_id()?, false),
        AccountMeta::new_readonly(spl_associated_token_account::id(), false),
        AccountMeta::new(coin_creator_vault_authority, false),
        AccountMeta::new(
            coin_creator_vault_ata_pda(&coin_creator_vault_authority)?,
            false,
        ),
    ];
    accounts.extend(
        current_shareholders
            .iter()
            .map(|shareholder| AccountMeta::new(*shareholder, false)),
    );
    Ok(Instruction {
        program_id,
        accounts,
        data,
    })
}

fn build_create_social_fee_pda_instruction(
    payer: &Pubkey,
    user_id: &str,
    platform: u8,
) -> Result<Instruction, String> {
    let program_id = pump_fee_program_id()?;
    let mut data = vec![144, 224, 59, 211, 78, 248, 202, 220];
    encode_borsh_string(&mut data, user_id);
    data.push(platform);
    Ok(Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(social_fee_pda(user_id, platform)?, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(fee_program_global_pda()?, false),
            AccountMeta::new_readonly(event_authority_pda(&program_id), false),
            AccountMeta::new_readonly(program_id, false),
        ],
        data,
    })
}

async fn build_social_fee_pda_create_instructions(
    rpc_url: &str,
    recipients: &[NormalizedRecipient],
    payer: &Pubkey,
) -> Result<Vec<Instruction>, String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut ordered_user_ids = Vec::new();
    let mut addresses = Vec::new();
    for (index, recipient) in recipients.iter().enumerate() {
        if recipient.githubUserId.is_empty() {
            continue;
        }
        if !seen.insert(recipient.githubUserId.clone()) {
            continue;
        }
        ordered_user_ids.push((index, recipient.githubUserId.clone()));
        addresses.push(social_fee_pda(&recipient.githubUserId, PLATFORM_GITHUB)?.to_string());
    }
    let exists = fetch_multiple_account_exists(rpc_url, &addresses, "confirmed").await?;
    ordered_user_ids.sort_by_key(|(index, _)| *index);
    let mut instructions = Vec::new();
    for ((_, user_id), exists) in ordered_user_ids.into_iter().zip(exists.into_iter()) {
        if !exists {
            instructions.push(build_create_social_fee_pda_instruction(
                payer,
                &user_id,
                PLATFORM_GITHUB,
            )?);
        }
    }
    Ok(instructions)
}

async fn build_fee_sharing_follow_up_instructions(
    rpc_url: &str,
    config: &NormalizedConfig,
    mint: Pubkey,
    creator: Pubkey,
) -> Result<Vec<Instruction>, String> {
    build_fee_sharing_setup_instructions(
        rpc_url,
        mint,
        creator,
        std::slice::from_ref(&creator),
        &config.feeSharing.recipients,
    )
    .await
}

async fn build_fee_sharing_setup_instructions(
    rpc_url: &str,
    mint: Pubkey,
    creator: Pubkey,
    current_shareholders: &[Pubkey],
    recipients: &[NormalizedRecipient],
) -> Result<Vec<Instruction>, String> {
    if recipients.is_empty() {
        return Err("fee sharing recipients are required for native follow-up setup.".to_string());
    }
    let mut instructions = vec![build_create_fee_sharing_config_instruction(
        &mint, &creator,
    )?];
    instructions
        .extend(build_social_fee_pda_create_instructions(rpc_url, recipients, &creator).await?);
    instructions.push(build_update_fee_shares_instruction(
        &mint,
        &creator,
        current_shareholders,
        recipients,
    )?);
    Ok(instructions)
}

fn resolve_agent_fee_recipients(
    config: &NormalizedConfig,
    mint: &Pubkey,
    creator: &Pubkey,
) -> Result<Vec<NormalizedRecipient>, String> {
    let configured = if config.agent.feeRecipients.is_empty() {
        let buyback_bps = config.agent.buybackBps.unwrap_or(0);
        let mut recipients = Vec::new();
        if buyback_bps > 0 {
            recipients.push(NormalizedRecipient {
                r#type: Some("agent".to_string()),
                address: String::new(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: buyback_bps,
            });
        }
        let wallet_share = 10_000 - buyback_bps;
        if wallet_share > 0 {
            recipients.push(NormalizedRecipient {
                r#type: Some("wallet".to_string()),
                address: if config.agent.feeReceiver.is_empty() {
                    creator.to_string()
                } else {
                    config.agent.feeReceiver.clone()
                },
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: wallet_share,
            });
        }
        recipients
    } else {
        config.agent.feeRecipients.clone()
    };

    let agent_receiver = token_agent_payments_pda(mint)?.to_string();
    let mut resolved = Vec::with_capacity(configured.len());
    for entry in configured {
        if entry.r#type.as_deref() == Some("agent") {
            resolved.push(NormalizedRecipient {
                r#type: Some("wallet".to_string()),
                address: agent_receiver.clone(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: entry.shareBps,
            });
        } else {
            resolved.push(entry);
        }
    }
    Ok(resolved)
}

fn native_follow_up_label(config: &NormalizedConfig) -> Option<&'static str> {
    launch_follow_up_label(config)
}

async fn build_native_follow_up_instructions(
    rpc_url: &str,
    config: &NormalizedConfig,
    mint: Pubkey,
    creator: Pubkey,
    agent_authority: Option<&Pubkey>,
) -> Result<Vec<Instruction>, String> {
    match config.mode.as_str() {
        "regular" | "cashback" => {
            build_fee_sharing_follow_up_instructions(rpc_url, config, mint, creator).await
        }
        "agent-custom" => {
            let authority = agent_authority
                .ok_or_else(|| "agent authority is required for agent-custom mode.".to_string())?;
            let recipients = resolve_agent_fee_recipients(config, &mint, &creator)?;
            let mut instructions = vec![build_agent_initialize_instruction(
                &mint,
                &creator,
                authority,
                config.agent.buybackBps.unwrap_or(0) as u16,
            )?];
            instructions.extend(
                build_fee_sharing_setup_instructions(
                    rpc_url,
                    mint,
                    creator,
                    std::slice::from_ref(&creator),
                    &recipients,
                )
                .await?,
            );
            Ok(instructions)
        }
        "agent-locked" => {
            let authority = agent_authority
                .ok_or_else(|| "agent authority is required for agent-locked mode.".to_string())?;
            let recipients = vec![NormalizedRecipient {
                r#type: Some("wallet".to_string()),
                address: token_agent_payments_pda(&mint)?.to_string(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 10_000,
            }];
            let mut instructions = vec![build_agent_initialize_instruction(
                &mint,
                &creator,
                authority,
                config.agent.buybackBps.unwrap_or(0) as u16,
            )?];
            instructions.extend(
                build_fee_sharing_setup_instructions(
                    rpc_url,
                    mint,
                    creator,
                    std::slice::from_ref(&creator),
                    &recipients,
                )
                .await?,
            );
            Ok(instructions)
        }
        unsupported => Err(format!(
            "Native Pump follow-up builder does not support mode={unsupported}."
        )),
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

fn with_tx_settings(
    core_instructions: Vec<Instruction>,
    tx_config: &NativeTxConfig,
    payer: &Pubkey,
    jitodontfront_enabled: bool,
) -> Result<Vec<Instruction>, String> {
    let mut instructions = vec![build_compute_unit_limit_instruction(
        tx_config.compute_unit_limit,
    )?];
    if tx_config.compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            tx_config.compute_unit_price_micro_lamports as u64,
        )?);
    }
    instructions.extend(apply_jitodontfront(
        core_instructions,
        jitodontfront_enabled,
        payer,
    )?);
    if tx_config.jito_tip_lamports > 0 {
        let tip_account = parse_pubkey(&tx_config.jito_tip_account, "tx.jitoTipAccount")?;
        instructions.push(transfer(
            payer,
            &tip_account,
            tx_config.jito_tip_lamports as u64,
        ));
    }
    Ok(instructions)
}

fn apply_jitodontfront(
    mut instructions: Vec<Instruction>,
    enabled: bool,
    payer: &Pubkey,
) -> Result<Vec<Instruction>, String> {
    if !enabled {
        return Ok(instructions);
    }
    let dontfront = parse_pubkey(JITODONTFRONT_ACCOUNT, "jitodontfront")?;
    if instructions.iter().any(|instruction| {
        instruction
            .accounts
            .iter()
            .any(|meta| meta.pubkey == dontfront)
    }) {
        return Ok(instructions);
    }
    let mut instruction = transfer(payer, payer, 0);
    instruction
        .accounts
        .push(AccountMeta::new_readonly(dontfront, false));
    instructions.insert(0, instruction);
    Ok(instructions)
}

fn parse_lookup_table_addresses(config: &NormalizedConfig) -> Vec<String> {
    let _ = config;
    DEFAULT_LOOKUP_TABLES
        .iter()
        .map(|entry| entry.to_string())
        .collect()
}

fn dedupe_lookup_table_addresses(values: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();
    for value in values {
        if !deduped.iter().any(|entry| entry == &value) {
            deduped.push(value);
        }
    }
    deduped
}

fn default_lookup_table_profiles_for_label(label: &str) -> Vec<Vec<String>> {
    match label {
        "launch" => DEFAULT_LAUNCH_LOOKUP_TABLE_PROFILES
            .iter()
            .map(|profile| profile.iter().map(|entry| entry.to_string()).collect())
            .collect(),
        "follow-up" | "agent-setup" => DEFAULT_FOLLOW_UP_LOOKUP_TABLE_PROFILES
            .iter()
            .map(|profile| profile.iter().map(|entry| entry.to_string()).collect())
            .collect(),
        _ => DEFAULT_LOOKUP_TABLES
            .iter()
            .map(|entry| vec![entry.to_string()])
            .collect(),
    }
}

fn lookup_table_variants_for_transaction(
    _label: &str,
    _config: &NormalizedConfig,
    loaded_lookup_tables: &[AddressLookupTableAccount],
) -> Vec<Vec<AddressLookupTableAccount>> {
    let shared_variant = loaded_lookup_tables
        .iter()
        .filter(|table| table.key.to_string() == SHARED_SUPER_LOOKUP_TABLE)
        .cloned()
        .collect::<Vec<_>>();
    if shared_variant.is_empty() {
        vec![]
    } else {
        vec![shared_variant]
    }
}

#[derive(Clone)]
struct CachedLookupTableAccount {
    loaded_at: Instant,
    table: AddressLookupTableAccount,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedLookupTableCache {
    tables: HashMap<String, PersistedLookupTableEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedLookupTableEntry {
    addresses: Vec<String>,
    #[serde(default)]
    address_count: Option<usize>,
    #[serde(default)]
    content_hash: Option<String>,
    #[serde(default)]
    manifest_hash: Option<String>,
}

fn shared_alt_manifest_hash() -> String {
    let mut required_addresses = shared_alt_manifest_entries()
        .into_iter()
        .filter(|entry| entry.required)
        .map(|entry| entry.address.to_string())
        .collect::<Vec<_>>();
    required_addresses.sort();
    lookup_table_address_content_hash(&required_addresses)
}

fn merge_persisted_lookup_table_caches(
    caches: impl IntoIterator<Item = PersistedLookupTableCache>,
) -> PersistedLookupTableCache {
    let mut merged = PersistedLookupTableCache::default();
    for cache in caches {
        for (address, entry) in cache.tables {
            merged.tables.entry(address).or_insert(entry);
        }
    }
    merged
}

fn lookup_table_cache() -> &'static Mutex<HashMap<String, CachedLookupTableAccount>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedLookupTableAccount>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn persisted_lookup_table_cache() -> &'static Mutex<PersistedLookupTableCache> {
    static CACHE: OnceLock<Mutex<PersistedLookupTableCache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let cache = merge_persisted_lookup_table_caches(
            [
                paths::shared_lookup_table_cache_path(),
                paths::legacy_bonk_lookup_table_cache_path(),
            ]
            .into_iter()
            .filter_map(|path| {
                fs::read_to_string(path)
                    .ok()
                    .and_then(|raw| serde_json::from_str::<PersistedLookupTableCache>(&raw).ok())
            }),
        );
        Mutex::new(cache)
    })
}

fn is_default_lookup_table_address(address: &str) -> bool {
    DEFAULT_LOOKUP_TABLES.contains(&address)
}

fn persist_lookup_table_account(
    address: &str,
    table: &AddressLookupTableAccount,
) -> Result<(), String> {
    if !is_default_lookup_table_address(address) {
        return Ok(());
    }
    let mut cache = persisted_lookup_table_cache()
        .lock()
        .map_err(|error| error.to_string())?;
    let addresses = table
        .addresses
        .iter()
        .map(|entry| entry.to_string())
        .collect::<Vec<_>>();
    let address_count = addresses.len();
    let content_hash = lookup_table_address_content_hash(&addresses);
    let manifest_hash = shared_alt_manifest_hash();
    cache.tables.insert(
        address.to_string(),
        PersistedLookupTableEntry {
            addresses,
            address_count: Some(address_count),
            content_hash: Some(content_hash),
            manifest_hash: Some(manifest_hash),
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

fn load_persisted_lookup_table_account(address: &str) -> Option<AddressLookupTableAccount> {
    if !is_default_lookup_table_address(address) {
        return None;
    }
    let cache = persisted_lookup_table_cache().lock().ok()?;
    let entry = cache.tables.get(address)?;
    if entry.address_count != Some(entry.addresses.len()) {
        eprintln!(
            "[launchdeck-engine][alt-cache] ignoring stale shared ALT snapshot {} due to missing/mismatched address count",
            address
        );
        return None;
    }
    let content_hash = lookup_table_address_content_hash(&entry.addresses);
    if entry.content_hash.as_deref() != Some(content_hash.as_str()) {
        eprintln!(
            "[launchdeck-engine][alt-cache] ignoring stale shared ALT snapshot {} due to content hash mismatch",
            address
        );
        return None;
    }
    let manifest_hash = shared_alt_manifest_hash();
    if entry.manifest_hash.as_deref() != Some(manifest_hash.as_str()) {
        eprintln!(
            "[launchdeck-engine][alt-cache] ignoring stale shared ALT snapshot {} due to manifest hash mismatch",
            address
        );
        return None;
    }
    let key = parse_pubkey(address, "lookup table address").ok()?;
    let addresses = entry
        .addresses
        .iter()
        .map(|entry| parse_pubkey(entry, "lookup table entry"))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some(AddressLookupTableAccount { key, addresses })
}

fn get_cached_lookup_table_account(address: &str) -> Option<AddressLookupTableAccount> {
    let mut cache = lookup_table_cache().lock().ok()?;
    match cache.get(address) {
        Some(entry) if entry.loaded_at.elapsed() <= LOOKUP_TABLE_CACHE_TTL => {
            Some(entry.table.clone())
        }
        Some(_) => {
            cache.remove(address);
            None
        }
        None => None,
    }
}

fn cache_lookup_table_account(address: &str, table: &AddressLookupTableAccount) {
    if let Ok(mut cache) = lookup_table_cache().lock() {
        cache.insert(
            address.to_string(),
            CachedLookupTableAccount {
                loaded_at: Instant::now(),
                table: table.clone(),
            },
        );
    }
}

async fn load_lookup_table_account(
    rpc_url: String,
    address: String,
) -> Result<Option<AddressLookupTableAccount>, String> {
    if let Some(table) = get_cached_lookup_table_account(&address) {
        return Ok(Some(table));
    }
    if let Some(table) = load_persisted_lookup_table_account(&address) {
        cache_lookup_table_account(&address, &table);
        return Ok(Some(table));
    }
    let data = match fetch_account_data(&rpc_url, &address, "confirmed").await {
        Ok(data) => data,
        Err(error) if error.contains("was not found") => return Ok(None),
        Err(error) => return Err(error),
    };
    let table = AddressLookupTable::deserialize(&data)
        .map_err(|error| format!("Failed to decode address lookup table {address}: {error}"))?;
    let account = AddressLookupTableAccount {
        key: parse_pubkey(&address, "lookup table address")?,
        addresses: table.addresses.to_vec(),
    };
    cache_lookup_table_account(&address, &account);
    persist_lookup_table_account(&address, &account)?;
    Ok(Some(account))
}

async fn load_lookup_table_accounts_for_addresses(
    rpc_url: &str,
    requested: &[String],
) -> Result<Vec<AddressLookupTableAccount>, String> {
    if requested.is_empty() {
        return Ok(vec![]);
    }
    let mut tasks = JoinSet::new();
    for (index, address) in requested.iter().cloned().enumerate() {
        let rpc_url = rpc_url.to_string();
        tasks.spawn(async move { (index, load_lookup_table_account(rpc_url, address).await) });
    }
    let mut resolved = vec![None; requested.len()];
    while let Some(joined) = tasks.join_next().await {
        let (index, table_result) = joined.map_err(|error| error.to_string())?;
        resolved[index] = Some(table_result?);
    }
    Ok(resolved.into_iter().flatten().flatten().collect::<Vec<_>>())
}

async fn load_lookup_table_accounts(
    rpc_url: &str,
    config: &NormalizedConfig,
) -> Result<Vec<AddressLookupTableAccount>, String> {
    let requested = parse_lookup_table_addresses(config);
    if requested.is_empty() {
        return Ok(vec![]);
    }
    load_lookup_table_accounts_for_addresses(rpc_url, &requested).await
}

#[allow(dead_code)]
pub async fn warm_default_lookup_tables(rpc_url: &str) -> Result<usize, String> {
    let requested = DEFAULT_LOOKUP_TABLES
        .iter()
        .map(|entry| entry.to_string())
        .collect::<Vec<_>>();
    let loaded = load_lookup_table_accounts_for_addresses(rpc_url, &requested).await?;
    Ok(loaded.len())
}

async fn load_shared_lookup_tables_for_tx_format(
    rpc_url: &str,
    requested_format: &str,
) -> Result<Vec<AddressLookupTableAccount>, String> {
    let _ = requested_format;
    let requested = DEFAULT_LOOKUP_TABLES
        .iter()
        .map(|entry| entry.to_string())
        .collect::<Vec<_>>();
    load_lookup_table_accounts_for_addresses(rpc_url, &requested).await
}

fn select_native_format(
    requested: &str,
    has_lookup_tables: bool,
) -> Result<NativeTxFormat, String> {
    match requested {
        "legacy" | "v0" | "v0-alt" | "auto" => {
            if has_lookup_tables {
                Ok(NativeTxFormat::V0Alt)
            } else {
                Err(format!(
                    "Native Pump compile requires the shared lookup table {SHARED_SUPER_LOOKUP_TABLE} for txFormat={requested}."
                ))
            }
        }
        unsupported => Err(format!(
            "Native Pump compile does not yet support txFormat={unsupported}."
        )),
    }
}

fn metrics_length_mut<'a>(
    metrics: &'a mut TransactionCompileMetrics,
    format: &str,
) -> Result<&'a mut Option<usize>, String> {
    match format {
        "legacy" => Ok(&mut metrics.legacy_length),
        "v0" => Ok(&mut metrics.v0_length),
        "v0-alt" => Ok(&mut metrics.v0_alt_length),
        unsupported => Err(format!(
            "Unsupported compiled transaction format in metrics: {unsupported}"
        )),
    }
}

fn choose_best_auto_candidate(candidates: &[CompiledTxCandidate]) -> usize {
    let packet_limit = 1232usize;
    candidates
        .iter()
        .enumerate()
        .min_by_key(|(_, candidate)| {
            (
                candidate.serialized_len > packet_limit,
                candidate.serialized_len,
                candidate.compiled.format == "v0-alt",
            )
        })
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn auto_selection_warning(
    selected: &CompiledTxCandidate,
    v0_length: usize,
    v0_alt_length: Option<usize>,
) -> String {
    let selected_suffix = if selected.serialized_len > 1232 {
        " but it still exceeds the 1232-byte packet limit"
    } else {
        ""
    };
    match v0_alt_length {
        Some(alt_length) if selected.compiled.format == "v0-alt" => format!(
            "Auto selected v0-alt at {} bytes over v0 at {} bytes{}.",
            alt_length, v0_length, selected_suffix
        ),
        Some(alt_length) => format!(
            "Auto kept v0 at {} bytes because v0-alt measured {} bytes{}.",
            v0_length, alt_length, selected_suffix
        ),
        None => format!(
            "Auto used v0 at {} bytes because no lookup-table candidate was available{}.",
            v0_length, selected_suffix
        ),
    }
}

fn compile_transaction_with_metrics(
    label: &str,
    tx_format: NativeTxFormat,
    blockhash: &str,
    last_valid_block_height: u64,
    payer: &Keypair,
    mint_signer: Option<&Keypair>,
    instructions: Vec<Instruction>,
    tx_config: &NativeTxConfig,
    lookup_table_variants: &[Vec<AddressLookupTableAccount>],
) -> Result<(CompiledTransaction, TransactionCompileMetrics), String> {
    let preferred_lookup_tables = lookup_table_variants
        .first()
        .map(|variant| variant.as_slice())
        .unwrap_or(&[]);
    if tx_format != NativeTxFormat::Auto {
        let candidate = compile_transaction_candidate(
            label,
            tx_format,
            blockhash,
            last_valid_block_height,
            payer,
            mint_signer,
            instructions,
            tx_config,
            preferred_lookup_tables,
        )?;
        let mut metrics = TransactionCompileMetrics::default();
        *metrics_length_mut(&mut metrics, &candidate.compiled.format)? =
            Some(candidate.serialized_len);
        ensure_compiled_candidate_fits_packet(label, &candidate)?;
        return Ok((candidate.compiled, metrics));
    }

    let v0_candidate = compile_transaction_candidate(
        label,
        NativeTxFormat::V0,
        blockhash,
        last_valid_block_height,
        payer,
        mint_signer,
        instructions.clone(),
        tx_config,
        &[],
    )?;

    if lookup_table_variants.is_empty() {
        let candidate = compile_transaction_candidate(
            label,
            NativeTxFormat::V0,
            blockhash,
            last_valid_block_height,
            payer,
            mint_signer,
            instructions,
            tx_config,
            &[],
        )?;
        let mut metrics = TransactionCompileMetrics::default();
        *metrics_length_mut(&mut metrics, &candidate.compiled.format)? =
            Some(candidate.serialized_len);
        metrics.warnings.push(auto_selection_warning(
            &candidate,
            candidate.serialized_len,
            None,
        ));
        ensure_compiled_candidate_fits_packet(label, &candidate)?;
        return Ok((candidate.compiled, metrics));
    }

    let mut metrics = TransactionCompileMetrics::default();
    *metrics_length_mut(&mut metrics, &v0_candidate.compiled.format)? =
        Some(v0_candidate.serialized_len);
    let mut candidates = vec![v0_candidate];
    let mut best_alt_length: Option<usize> = None;
    for variant in lookup_table_variants {
        if variant.is_empty() {
            continue;
        }
        let alt_candidate = compile_transaction_candidate(
            label,
            NativeTxFormat::V0Alt,
            blockhash,
            last_valid_block_height,
            payer,
            mint_signer,
            instructions.clone(),
            tx_config,
            variant,
        )?;
        if alt_candidate.compiled.format == "v0-alt" {
            best_alt_length = Some(match best_alt_length {
                Some(current_best) => current_best.min(alt_candidate.serialized_len),
                None => alt_candidate.serialized_len,
            });
        }
        candidates.push(alt_candidate);
    }
    *metrics_length_mut(&mut metrics, "v0-alt")? = best_alt_length;
    let selected_index = choose_best_auto_candidate(&candidates);
    let selected = candidates
        .get(selected_index)
        .cloned()
        .ok_or_else(|| "Auto transaction candidate selection failed.".to_string())?;
    metrics.warnings.push(auto_selection_warning(
        &selected,
        candidates[0].serialized_len,
        best_alt_length,
    ));
    ensure_compiled_candidate_fits_packet(label, &selected)?;
    Ok((selected.compiled, metrics))
}

fn compile_transaction_candidate(
    label: &str,
    tx_format: NativeTxFormat,
    blockhash: &str,
    last_valid_block_height: u64,
    payer: &Keypair,
    mint_signer: Option<&Keypair>,
    instructions: Vec<Instruction>,
    tx_config: &NativeTxConfig,
    lookup_tables: &[AddressLookupTableAccount],
) -> Result<CompiledTxCandidate, String> {
    if tx_format == NativeTxFormat::Auto {
        return Err("Auto transaction format must be resolved before compile.".to_string());
    }
    if tx_format == NativeTxFormat::Legacy {
        return Err(
            "Native Pump shared-ALT-only compilation no longer supports legacy transaction format."
                .to_string(),
        );
    }
    if tx_format == NativeTxFormat::V0Alt && lookup_tables.is_empty() {
        return Err(format!(
            "Native Pump v0-alt compile requires the shared lookup table {SHARED_SUPER_LOOKUP_TABLE}."
        ));
    }
    let hash = Hash::from_str(blockhash).map_err(|error| error.to_string())?;
    let message = v0::Message::try_compile(&payer.pubkey(), &instructions, lookup_tables, hash)
        .map_err(|error| error.to_string())?;
    let lookup_tables_used = message
        .address_table_lookups
        .iter()
        .map(|lookup| lookup.account_key.to_string())
        .collect::<Vec<_>>();
    if lookup_tables_used.len() != 1 || lookup_tables_used[0] != SHARED_SUPER_LOOKUP_TABLE {
        return Err(format!(
            "Native Pump v0-alt compilation must actually use the shared lookup table {SHARED_SUPER_LOOKUP_TABLE}; used [{}].",
            lookup_tables_used.join(", ")
        ));
    }
    let signers: Vec<&Keypair> = match mint_signer {
        Some(mint) => vec![payer, mint],
        None => vec![payer],
    };
    let message_for_diagnostics = message.clone();
    let transaction = VersionedTransaction::try_new(VersionedMessage::V0(message), &signers)
        .map_err(|error| error.to_string())?;
    let serialized = bincode::serialize(&transaction).map_err(|error| error.to_string())?;
    let serialized_len = serialized.len();
    crate::alt_diagnostics::emit_alt_coverage_diagnostics(
        "launchdeck-engine",
        label,
        &instructions,
        lookup_tables,
        &message_for_diagnostics,
        Some(serialized_len),
        &[],
    );
    let serialized_base64 = BASE64.encode(serialized);
    compiled_transaction_signers::remember_compiled_transaction_signers(
        &serialized_base64,
        &signers[1..],
    );
    let signature = crate::rpc::precompute_transaction_signature(&serialized_base64);
    Ok(CompiledTxCandidate {
        serialized_len,
        compiled: CompiledTransaction {
            label: label.to_string(),
            format: "v0-alt".to_string(),
            blockhash: blockhash.to_string(),
            lastValidBlockHeight: last_valid_block_height,
            serializedBase64: serialized_base64,
            signature,
            lookupTablesUsed: lookup_tables_used,
            computeUnitLimit: Some(u64::from(tx_config.compute_unit_limit)),
            computeUnitPriceMicroLamports: if tx_config.compute_unit_price_micro_lamports > 0 {
                Some(tx_config.compute_unit_price_micro_lamports as u64)
            } else {
                None
            },
            inlineTipLamports: if tx_config.jito_tip_lamports > 0 {
                Some(tx_config.jito_tip_lamports as u64)
            } else {
                None
            },
            inlineTipAccount: if tx_config.jito_tip_lamports > 0
                && !tx_config.jito_tip_account.is_empty()
            {
                Some(tx_config.jito_tip_account.clone())
            } else {
                None
            },
        },
    })
}

fn summarize_instructions(instructions: &[Instruction]) -> Vec<crate::report::InstructionSummary> {
    instructions
        .iter()
        .enumerate()
        .map(|(index, instruction)| crate::report::InstructionSummary {
            index,
            programId: instruction.program_id.to_string(),
            keyCount: instruction.accounts.len(),
            writableKeys: instruction
                .accounts
                .iter()
                .filter(|meta| meta.is_writable)
                .count(),
            signerKeys: instruction
                .accounts
                .iter()
                .filter(|meta| meta.is_signer)
                .count(),
        })
        .collect()
}

fn apply_transaction_details(
    report: &mut LaunchReport,
    compiled_transactions: &[CompiledTransaction],
    instruction_summaries: &[Vec<crate::report::InstructionSummary>],
    compile_metrics: &[TransactionCompileMetrics],
) -> Result<(), String> {
    for (index, summary) in report.transactions.iter_mut().enumerate() {
        let Some(compiled) = compiled_transactions.get(index) else {
            break;
        };
        let metrics = compile_metrics.get(index).cloned().unwrap_or_default();
        let raw = BASE64
            .decode(&compiled.serializedBase64)
            .map_err(|error| error.to_string())?;
        let encoded_len = compiled.serializedBase64.len();
        summary.legacyLength = metrics.legacy_length;
        summary.legacyBase64Length = metrics.legacy_length.map(|_| encoded_len);
        summary.v0Length = metrics.v0_length;
        summary.v0Base64Length = metrics.v0_length.map(|_| encoded_len);
        summary.v0AltLength = metrics.v0_alt_length;
        summary.v0AltBase64Length = metrics.v0_alt_length.map(|_| encoded_len);
        if compiled.format == "legacy" {
            if summary.legacyLength.is_none() {
                summary.legacyLength = Some(raw.len());
                summary.legacyBase64Length = Some(encoded_len);
            }
            summary.v0Error = Some("Native path compiled as legacy only.".to_string());
            summary.v0AltError = Some("Native path compiled as legacy only.".to_string());
        } else {
            if compiled.format == "v0-alt" {
                if summary.v0AltLength.is_none() {
                    summary.v0AltLength = Some(raw.len());
                    summary.v0AltBase64Length = Some(encoded_len);
                }
                summary.v0Error = Some("Native path compiled with lookup tables.".to_string());
            } else {
                if summary.v0Length.is_none() {
                    summary.v0Length = Some(raw.len());
                    summary.v0Base64Length = Some(encoded_len);
                }
                summary.v0AltError =
                    Some("Native path compiled as versioned without lookup tables.".to_string());
            }
            summary.legacyError = Some("Native path compiled as versioned only.".to_string());
        }
        summary.fitsWithAlts = compiled.format == "v0-alt";
        summary.lookupTablesUsed = if compiled.format == "v0-alt" {
            compiled.lookupTablesUsed.clone()
        } else {
            vec![]
        };
        summary.exceedsPacketLimit = raw.len() > 1232;
        summary.instructionSummary = instruction_summaries
            .get(index)
            .cloned()
            .unwrap_or_default();
        summary.warnings.extend(metrics.warnings);
        if raw.len() > 1232 {
            summary.warnings.push(format!(
                "Compiled packet exceeds the 1232-byte Solana limit by {} bytes.",
                raw.len() - 1232
            ));
        } else if raw.len() >= 1150 {
            summary.warnings.push(format!(
                "Compiled packet is within {} bytes of the 1232-byte Solana limit.",
                1232 - raw.len()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RawConfig, normalize_raw_config};
    use crate::transport::{build_transport_plan, estimate_transaction_count};
    use borsh::BorshDeserialize;

    fn assert_sol_transfer_instruction(
        instruction: &Instruction,
        from: &Pubkey,
        to: &Pubkey,
        lamports: u64,
    ) {
        assert_eq!(instruction.program_id, system_program::id());
        assert_eq!(instruction.accounts[0].pubkey, *from);
        assert!(instruction.accounts[0].is_signer);
        assert_eq!(instruction.accounts[1].pubkey, *to);
        let decoded =
            bincode::deserialize::<solana_system_interface::instruction::SystemInstruction>(
                &instruction.data,
            )
            .expect("system transfer instruction");
        assert_eq!(
            decoded,
            solana_system_interface::instruction::SystemInstruction::Transfer { lamports }
        );
    }

    fn sample_global() -> PumpGlobalState {
        let selected_fee_recipient =
            selected_pump_apr28_fee_recipient().expect("Pump April 28 fee recipient");
        PumpGlobalState {
            fee_recipient: selected_fee_recipient,
            initial_virtual_token_reserves: 1_073_000_000_000_000,
            initial_virtual_sol_reserves: 30_000_000_000,
            initial_real_token_reserves: 793_100_000_000_000,
            fee_basis_points: 100,
            creator_fee_basis_points: 50,
            fee_recipients: [Pubkey::default(); 7],
            reserved_fee_recipient: Pubkey::default(),
            reserved_fee_recipients: [Pubkey::default(); 7],
            buyback_fee_recipients: [selected_fee_recipient; 8],
            buyback_basis_points: 0,
            initial_virtual_quote_reserves: 30_000_000_000,
            whitelisted_quote_mints: [wsol_mint().expect("wsol mint")],
        }
    }

    fn sample_curve() -> PumpBondingCurveState {
        PumpBondingCurveState {
            virtual_token_reserves: 900_000_000_000_000,
            virtual_sol_reserves: 40_000_000_000,
            real_token_reserves: 600_000_000_000_000,
            real_sol_reserves: 35_000_000_000,
            token_total_supply: 1_000_000_000_000_000,
            complete: false,
            creator: Pubkey::new_unique(),
            is_mayhem_mode: false,
            cashback_enabled: false,
            quote_mint: Pubkey::default(),
        }
    }

    fn deployed_shared_alt_lookup_table_fixture() -> AddressLookupTableAccount {
        let addresses = [
            "ComputeBudget111111111111111111111111111111",
            "11111111111111111111111111111111",
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK",
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj",
            "FfYek5vEz23cMkWsdJwG2oa6EphsvXSHrGpdALN4g6W1",
            "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "DTgyhdzARWt8fiFrA5E2ECTdR5U3rpGho2AmaPWsieWg",
            "9bcapqGioeAGybVSbtYoSZR23qZESNbToTV5C2QUJMqZ",
            "5JXjm3xUYU7St1g7hptBF6bPqZAeTpWR8kzoeDK8uvPr",
            "67pirGqYiCT6j56DdQmAivWZSuZEtYbzSqMTWUNcHZAL",
            "So11111111111111111111111111111111111111112",
            "SysvarRent111111111111111111111111111111111",
            "E64NGkDLLCdQ2yFNPcavaKptrEgmiQaNykUuLC1Qgwyp",
            "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
            "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr",
            "USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB",
            "WLHv2UAZm6z4KyaaELi5pjdbJh6RESMva1Rnn8pJVVh",
            "EPiZbnrThjyLnoQ6QQzkxeFqyL5uyg9RzNHHAudUPxBz",
            "AQAGYQsdU853WAKhXM79CgNdoyhrRwXvYHX6qrDyC1FS",
            "5QpMZ6MuyKjg8Qa1X8gM5G3YMsd43rpHb2iQ6hdcRM7m",
            "DHY2efKhMcZyAgmPw82C2Gez1e98Ab7oWcXfxz9frUCr",
            "2s8VC2vpZcoUCbvEqwUagvdyHfUeWYQZuKV376acx6iF",
            "HKcNrRDTFuXVTyMCkp5h3eU4cyqbzteAW1UR7m2CWbVs",
            "2DPAtwB8L12vrMRExbLuyGnC7n2J5LNoZQSejeQGpwkr",
            "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s",
            "8m39KcS9MJb4QnLW7sXA8pHXMvKQFbyfeEBnsXJSxybu",
            "Dz7NXp8838CivsPYZvACQ23gtAuq2DVtVLMQLaUQR3JE",
            "6s1xP3hpbAfFoNtUNF8mfHsjr2Bd97JxFJRWLbL6aHuX",
            "13ec7XdrjF3h3YcqBTFDSReRcUFwbCnJaAQspM4j6DDJ",
            "2mFsfTTscbFcjE7spzaucYuGSj9kPj1oLaotemzNetST",
            "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf",
            "5FqUo9aBjsp7QeeyN6Vi2ZmF2fjS4H5EU7wnAQwPy17z",
            "62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV",
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
            "6sd8BkAkqsrxbfsxLsjBxA2yCbMppNCt6sJk8MHh85VP",
            "71YUh7AEj2MFWAtUX8anpr5Lig7n9NR2xReW7QDkJqpb",
            "7osZceLJJJ6jbk58XxyTwvcqQauQgSyRkym8R8UAwBqd",
            "8FoNgzmjuSmiy86EPCWxvv1q7oJSu2WGA7wPymwki2LJ",
            "8R4wThp45WHYQQuCbz5GqTm4pYUVk6eCTZe1NSa8EBnV",
            "8viRCH1uDz8BUdBQkT88ymgQrtdmYEvs3Efy31GuWntu",
            "8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt",
            "91tDWyGdurPekG8JCWCfzJn7aGfLgb9zb711fHpyGR2j",
            "94qWNrtmfn42h3ZjUZwWvK1MEo9uVmmrBPd2hpNjYDjb",
            "96bWWuSF6de5H3wFAMb2gQ2sV2NLuwFJpB4WZ7BZnFJD",
            "9VCdFMNuUddxpRichH5AVtZgtZXvLwto4VWYaMaKpW3J",
            "A1zfX5kBQWreVwDFUN22KjCcBBMt1RAuPCCoKqZczdrD",
            "ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw",
            "AgenTMiC2hvxGebTsgmsD4HHBa8WEcqGFf87iwRRxLo7",
            "ALeLWphFxNVNXpXFEC4Ssf2Jan1Wki72Us8tXMMrQuQZ",
            "ARoYcw8G5iKpx73FtZFUpDergSk6SFLDPDiqNxbrthj4",
            "AV7PjXHL5JXZ1YoYRoN9Dsstg1x2UciBupMCXcJP8gUz",
            "AXVvmhWaaPtV52jqYuTNqp1xRrkbxhfJfeHQKxq5cbvZ",
            "BckPpoRV4h329qAuhTCNoWdWAy2pZSJ89Qu3nuCU1zsj",
            "BwWK17cbHxwWBKZkUYvzxLcNQ1YVyaFezduWbtm2de6s",
            "C2aFPdENg4A2HQsmrd5rTw5TaYBX5Ku887cWjbFKtZpw",
            "C6rJRXMjCz4dRaNBWKHGdCFhU4BjsbkY4qkgnDfJ7SHy",
            "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1",
            "CHqnuTkj6sXDFknM652aEFPECZh9qVsBXWkhPohmV9dA",
            "CLxr3hPwBGZTYW28ybKe1upWU8HTSPD7HVUtExemA5gf",
            "D6QxXDt6hhcCpto4HiZKkN2YQ2iZRF5R7S3caCHpUsML",
            "D7Md4MyZFukPP6AErnhDDEYVrytS91sUaEnsjDj9msDR",
            "EKyrMGEiA6pUEC3TBUXeHgwQFkbWCuxinn72JMBNo7Zk",
            "EPbCowNs3NJxCogQjnguV38zXaSikkJMZcoBjU2Sobwc",
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
            "F1ppSHedBsGGwEKH78JVgoqr4xkQHswtsGGLpgM7bCP2",
            "F2nCzGoVTQp1x18X1Hy3M48JqCGD4xHYJxiX6fXu1SVG",
            "FzkaMusramphgC5TEs79f5RCfbdMy1PH7gDqQSP7xzDa",
            "GDGbYoY6gGyac26T8dXrct6N3k4jVrCmWPDor5AbsuK5",
            "GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS",
            "GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR",
            "GuGi1hEUEpUKpeVvFqvhoYQKibbpqJDUcuMr5iEvitoA",
            "HcAR1LpgSGFxeLyb1vkhsCuN6AtxQsww3E2pMMXkwHqx",
            "Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y",
            "HsC37rNFvJgpfH7y2Y6kqnwEQN4WfdM5FLArWnux5GUs",
            "Hu6v7CzuGmhMVVWoZCyr4e7gRPkdDB3qxG6AqG6fpSBu",
            "J7jjNCcg3qbansJjLWvq86q5qgUJNhve3vLSnGBEK4kB",
            "J7QDYTAuLoH27kZbnKMpPvW6wLUjDTH5Z2LbAfYqAq2q",
            "J9rxfQowAB45Xnwc5fZCRWCHMEuSDbTjzCFQdQrkGSyV",
            "jitodontfront111111111111111111111111111111",
            "MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e",
            "NDHxDztmnJCkSCvA2BRGCbn2X2iR3iFPktCdqY8niv3",
            "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA",
            "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ",
            "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM",
            "2ciX7vPQDYZe7YA8bgieADdJWeGRkWAKJsNHa8YTnPki",
            "3rmHSu74h1ZcmAisVcWerTCiRDQbUrBKmcwptYGjHfet",
            "65ZmLL5QPVeN2dW2TW2LTZzCK3vnjLpHpRj7AzuByvsY",
            "82NMHVCKwehXgbXMyzL41mvv3sdkypaMCtTxvJ4CtTzm",
            "8Ks12pbrD6PXxfty1hVQiE9sc289zgU1zHkvXhrSdriF",
            "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG",
            "dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN",
            "FhVo3mqL8PW5pH5U2CN4XE33DokiyZnUwuGpH2hmHLuM",
            "HLnpSz9h2S4hiLQ43rnSD9XkcUThA7B8hQMKmDaiTLcC",
            "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C",
            "TRENCHCfkCTud86C8ZC9kk2CFWJErYz4oZFaYttoxJF",
            "3y5uHX5whbkayE7SzZ945tsTdPAqUNN15jc3UBzFZ7b7",
            "Sysvar1nstructions1111111111111111111111111",
            "7HKc2NAi2Q2ZG3eSN7VJrtBgGi7dNFAz9DLnPNDUncM2",
            "5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD",
            "9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7",
            "GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL",
            "3BpXnfJaUTiwXnJNe7Ej1rcbzqTTQUvLShZaWazebsVR",
            "5cjcW9wExnJJiqgLjq7DEG75Pm6JBgE1hNv4B2vHXUW6",
            "EHAAiTxcdDwQ3U4bU6YcMsQGaekdzLS3B5SmYo46kJtL",
            "5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD",
            "A7hAgCzFw14fejgCp387JUJRMNyz4j89JKnhtKU8piqW",
            "HjQjngTDqoHE6aaGhUqfz9aQ7WZcBRjy5xB8PScLSr8i",
            "2HLoA8PQuxqUfNDVa6kCL8CZ1FkDMcqZZSE3HDEpKqSZ",
            "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",
            "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1",
            "srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX",
            shared_execution_routing::alt_manifest::SELECTED_PUMP_APR28_WSOL_FEE_RECIPIENT_ATA,
            shared_execution_routing::alt_manifest::SELECTED_PUMP_APR28_USDC_FEE_RECIPIENT_ATA,
        ]
        .into_iter()
        .map(|address| Pubkey::from_str(address).expect("deployed ALT fixture address"))
        .collect();
        AddressLookupTableAccount {
            key: parse_pubkey(SHARED_SUPER_LOOKUP_TABLE, "shared lookup table").unwrap(),
            addresses,
        }
    }

    fn regular_config() -> crate::config::NormalizedConfig {
        let mut raw = RawConfig {
            mode: "regular".to_string(),
            launchpad: "pump".to_string(),
            ..RawConfig::default()
        };
        raw.token.name = "LaunchDeck".to_string();
        raw.token.symbol = "LDECK".to_string();
        raw.token.uri = "ipfs://fixture".to_string();
        raw.tx.computeUnitPriceMicroLamports = Some(Value::from(1));
        raw.tx.jitoTipLamports = Some(Value::from(200_000));
        raw.tx.jitoTipAccount = "4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE".to_string();
        raw.execution.skipPreflight = Some(Value::Bool(true));
        raw.execution.provider = "helius-sender".to_string();
        raw.execution.buyProvider = "helius-sender".to_string();
        raw.execution.sellProvider = "helius-sender".to_string();
        normalize_raw_config(raw).expect("normalized config")
    }

    #[test]
    fn jito_single_creation_keeps_priority_fee() {
        let mut config = regular_config();
        config.execution.provider = "jito-bundle".to_string();
        let transport_plan =
            build_transport_plan(&config.execution, estimate_transaction_count(&config));
        assert_eq!(
            effective_creation_compute_unit_price_micro_lamports(&config, &transport_plan, false),
            config.tx.computeUnitPriceMicroLamports.unwrap_or(0)
        );
    }

    #[test]
    fn slippage_percent_maps_to_expected_bps() {
        assert_eq!(slippage_bps_from_percent("20").expect("20%"), 2_000);
        assert_eq!(slippage_bps_from_percent("0.5").expect("0.5%"), 50);
        assert_eq!(slippage_bps_from_percent("100").expect("100%"), 10_000);
    }

    #[test]
    fn pump_usdc_buy_route_uses_dynamic_intermediate_quote_amount() {
        let user = Pubkey::new_unique();
        let user_usdc = Pubkey::new_unique();
        let user_base = Pubkey::new_unique();
        let conversion_program = Pubkey::new_unique();
        let pump_program = pump_program_id().expect("pump program");
        let conversion_ix = Instruction {
            program_id: conversion_program,
            accounts: vec![
                AccountMeta::new_readonly(user, true),
                AccountMeta::new(route_wsol_pda(&user, 0), false),
                AccountMeta::new(user_usdc, false),
            ],
            data: vec![0; 24],
        };
        let pump_ix = Instruction {
            program_id: pump_program,
            accounts: vec![
                AccountMeta::new(user_usdc, false),
                AccountMeta::new(user_base, false),
            ],
            data: vec![1; 24],
        };
        let wrapper_ix = build_pump_usdc_buy_from_sol_route_instruction(
            &user,
            1_000_000,
            999_000,
            TrustedRaydiumClmmSwap {
                instruction: conversion_ix,
                expected_out: 2_000,
                min_out: 1_900,
            },
            pump_ix,
            &user_usdc,
            &user_base,
            10_000,
            10,
        )
        .expect("wrapper ix");
        let request = ExecuteSwapRouteRequest::try_from_slice(&wrapper_ix.data[1..])
            .expect("decode route request");

        assert_eq!(request.route_mode, SwapRouteMode::Mixed);
        assert_eq!(request.direction, SwapRouteDirection::Buy);
        assert_eq!(request.fee_mode, SwapRouteFeeMode::SolPre);
        assert_eq!(
            request.legs[0].input_source,
            SwapLegInputSource::GrossSolNetOfFee
        );
        assert_eq!(
            request.legs[1].input_source,
            SwapLegInputSource::PreviousTokenDelta
        );
        assert_eq!(request.legs[1].input_amount, 1_900);
        assert_eq!(request.legs[1].input_patch_offset, 8);
    }

    #[test]
    fn pump_usdc_sell_route_unwinds_dynamic_quote_amount_to_wsol() {
        let user = Pubkey::new_unique();
        let user_usdc = Pubkey::new_unique();
        let route_wsol = route_wsol_pda(&user, 0);
        let pump_program = pump_program_id().expect("pump program");
        let unwind_program = Pubkey::new_unique();
        let pump_ix = Instruction {
            program_id: pump_program,
            accounts: vec![
                AccountMeta::new_readonly(user, true),
                AccountMeta::new(user_usdc, false),
            ],
            data: vec![2; 24],
        };
        let unwind_ix = Instruction {
            program_id: unwind_program,
            accounts: vec![
                AccountMeta::new(user_usdc, false),
                AccountMeta::new(route_wsol, false),
            ],
            data: vec![3; 24],
        };
        let wrapper_ix = build_pump_usdc_sell_to_sol_route_instruction(
            &user,
            pump_ix,
            123_000,
            900_000,
            &user_usdc,
            TrustedRaydiumClmmSwap {
                instruction: unwind_ix,
                expected_out: 1_000_000,
                min_out: 990_000,
            },
            &route_wsol,
            10,
        )
        .expect("wrapper ix");
        let request = ExecuteSwapRouteRequest::try_from_slice(&wrapper_ix.data[1..])
            .expect("decode route request");

        assert_eq!(request.route_mode, SwapRouteMode::Mixed);
        assert_eq!(request.direction, SwapRouteDirection::Sell);
        assert_eq!(request.settlement, SwapRouteSettlement::Wsol);
        assert_eq!(request.fee_mode, SwapRouteFeeMode::WsolPost);
        assert_eq!(request.legs[0].input_source, SwapLegInputSource::Fixed);
        assert_eq!(
            request.legs[1].input_source,
            SwapLegInputSource::PreviousTokenDelta
        );
        assert_eq!(request.legs[1].input_amount, 900_000);
        assert_eq!(request.legs[1].input_patch_offset, 8);
    }

    #[test]
    fn jito_multi_tx_creation_drops_priority_fee() {
        let mut config = regular_config();
        config.execution.provider = "jito-bundle".to_string();
        config.mode = "agent-custom".to_string();
        config.agent.buybackBps = Some(2_500);
        config.agent.feeRecipients = vec![
            crate::config::NormalizedRecipient {
                r#type: Some("agent".to_string()),
                address: String::new(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 2_500,
            },
            crate::config::NormalizedRecipient {
                r#type: Some("wallet".to_string()),
                address: Pubkey::new_unique().to_string(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 7_500,
            },
        ];
        let transport_plan =
            build_transport_plan(&config.execution, estimate_transaction_count(&config));
        assert_eq!(
            effective_creation_compute_unit_price_micro_lamports(&config, &transport_plan, true),
            0
        );
        assert_eq!(
            effective_follow_up_compute_unit_price_micro_lamports(&config, &transport_plan),
            0
        );
    }

    #[test]
    fn supports_plain_regular_pump_launches() {
        let config = regular_config();
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn resolve_mint_keypair_uses_vanity_private_key_when_present() {
        let vanity_keypair = Keypair::new();
        let mut config = regular_config();
        config.vanityPrivateKey = bs58::encode(vanity_keypair.to_bytes()).into_string();

        let resolved = resolve_mint_keypair(&config).expect("vanity mint keypair");

        assert_eq!(resolved.pubkey(), vanity_keypair.pubkey());
    }

    #[test]
    fn resolve_mint_keypair_rejects_invalid_vanity_private_key() {
        let mut config = regular_config();
        config.vanityPrivateKey = "not-a-keypair".to_string();

        let error = resolve_mint_keypair(&config).expect_err("invalid vanity key should fail");

        assert!(error.contains("Invalid vanity private key"));
    }

    #[test]
    fn supports_sol_dev_buy_for_native_pump_path() {
        let mut config = regular_config();
        config.devBuy = Some(crate::config::NormalizedDevBuy {
            mode: "sol".to_string(),
            amount: "0.1".to_string(),
            source: "test".to_string(),
        });
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn supports_agent_unlocked_for_native_pump_path() {
        let mut config = regular_config();
        config.mode = "agent-unlocked".to_string();
        config.agent.buybackBps = Some(2_500);
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn supports_agent_custom_for_native_pump_path() {
        let mut config = regular_config();
        config.mode = "agent-custom".to_string();
        config.agent.buybackBps = Some(2_500);
        config.agent.feeRecipients = vec![
            crate::config::NormalizedRecipient {
                r#type: Some("agent".to_string()),
                address: String::new(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 2_500,
            },
            crate::config::NormalizedRecipient {
                r#type: Some("wallet".to_string()),
                address: Pubkey::new_unique().to_string(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 7_500,
            },
        ];
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn supports_agent_locked_for_native_pump_path() {
        let mut config = regular_config();
        config.mode = "agent-locked".to_string();
        config.agent.buybackBps = Some(2_500);
        config.creatorFee.mode = "agent-escrow".to_string();
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn auto_now_resolves_to_shared_alt_path() {
        assert_eq!(
            select_native_format("auto", true).expect("format"),
            NativeTxFormat::V0Alt
        );
        assert_eq!(
            select_native_format("legacy", true).expect("format"),
            NativeTxFormat::V0Alt
        );
        assert!(select_native_format("auto", false).is_err());
    }

    #[test]
    fn v0_alt_requires_loaded_lookup_tables() {
        assert_eq!(
            select_native_format("v0-alt", true).expect("format"),
            NativeTxFormat::V0Alt
        );
        assert!(select_native_format("v0-alt", false).is_err());
    }

    fn test_compiled_candidate(format: &str, serialized_len: usize) -> CompiledTxCandidate {
        CompiledTxCandidate {
            serialized_len,
            compiled: CompiledTransaction {
                label: "launch".to_string(),
                format: format.to_string(),
                blockhash: "blockhash".to_string(),
                lastValidBlockHeight: 123,
                serializedBase64: String::new(),
                signature: None,
                lookupTablesUsed: vec![],
                computeUnitLimit: Some(0),
                computeUnitPriceMicroLamports: None,
                inlineTipLamports: None,
                inlineTipAccount: None,
            },
        }
    }

    #[test]
    fn auto_selection_prefers_smaller_versioned_candidate() {
        let candidates = vec![
            test_compiled_candidate("v0", 1210),
            test_compiled_candidate("v0-alt", 1178),
        ];
        let selected = choose_best_auto_candidate(&candidates);
        assert_eq!(selected, 1);
    }

    #[test]
    fn auto_selection_prefers_fitting_candidate_when_only_one_fits() {
        let candidates = vec![
            test_compiled_candidate("v0", 1228),
            test_compiled_candidate("v0-alt", 1240),
        ];
        let selected = choose_best_auto_candidate(&candidates);
        assert_eq!(selected, 0);
    }

    #[test]
    fn launch_lookup_table_profiles_prioritize_shared_super_table() {
        let profiles = default_lookup_table_profiles_for_label("launch");
        assert_eq!(
            profiles[0],
            vec!["7CaMLcAuSskoeN7HoRwZjsSthU8sMwKqxtXkyMiMjuc".to_string()]
        );
        assert_eq!(profiles.len(), 1);
    }

    #[test]
    fn follow_up_lookup_table_profiles_prioritize_shared_super_table() {
        let profiles = default_lookup_table_profiles_for_label("agent-setup");
        assert_eq!(
            profiles[0],
            vec!["7CaMLcAuSskoeN7HoRwZjsSthU8sMwKqxtXkyMiMjuc".to_string()]
        );
        assert_eq!(profiles.len(), 1);
    }

    #[test]
    fn create_v2_metadata_payload_bytes_tracks_name_symbol_and_uri() {
        let mint = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let instruction = build_create_v2_instruction(
            &mint,
            &user,
            &creator,
            "LaunchDeck",
            "LDECK",
            "ipfs://fixture",
            false,
            false,
        )
        .expect("instruction");
        let metadata_payload_bytes =
            create_v2_metadata_payload_bytes(&[instruction]).expect("metadata payload");

        assert_eq!(
            metadata_payload_bytes,
            12 + "LaunchDeck".len() + "LDECK".len() + "ipfs://fixture".len()
        );
    }

    #[test]
    fn supports_wallet_only_deferred_fee_sharing() {
        let mut config = regular_config();
        config.feeSharing.generateLaterSetup = true;
        config.feeSharing.recipients = vec![crate::config::NormalizedRecipient {
            r#type: Some("wallet".to_string()),
            address: Pubkey::new_unique().to_string(),
            githubUserId: String::new(),
            githubUsername: String::new(),
            shareBps: 10_000,
        }];
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn supports_github_creator_fee_routing_with_user_id() {
        let mut config = regular_config();
        config.creatorFee.mode = "github".to_string();
        config.creatorFee.githubUserId = "12345".to_string();
        config.creatorFee.githubUsername = "launchdeck".to_string();
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn supports_social_fee_sharing_follow_up_when_user_ids_are_present() {
        let mut config = regular_config();
        config.feeSharing.generateLaterSetup = true;
        config.feeSharing.recipients = vec![crate::config::NormalizedRecipient {
            r#type: Some("github".to_string()),
            address: String::new(),
            githubUserId: "12345".to_string(),
            githubUsername: "launchdeck".to_string(),
            shareBps: 10_000,
        }];
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn still_rejects_social_fee_sharing_without_user_id() {
        let mut config = regular_config();
        config.feeSharing.generateLaterSetup = true;
        config.feeSharing.recipients = vec![crate::config::NormalizedRecipient {
            r#type: Some("github".to_string()),
            address: String::new(),
            githubUserId: String::new(),
            githubUsername: "launchdeck".to_string(),
            shareBps: 10_000,
        }];
        assert!(!supports_native_pump_compile(&config));
    }

    #[test]
    fn still_rejects_agent_custom_social_recipient_without_user_id() {
        let mut config = regular_config();
        config.mode = "agent-custom".to_string();
        config.agent.buybackBps = Some(2_500);
        config.agent.feeRecipients = vec![crate::config::NormalizedRecipient {
            r#type: Some("github".to_string()),
            address: String::new(),
            githubUserId: String::new(),
            githubUsername: "launchdeck".to_string(),
            shareBps: 10_000,
        }];
        assert!(!supports_native_pump_compile(&config));
    }

    #[test]
    fn create_v2_instruction_shape_matches_expected_accounts() {
        let mint = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let instruction = build_create_v2_instruction(
            &mint,
            &user,
            &creator,
            "LaunchDeck",
            "LDECK",
            "ipfs://fixture",
            false,
            false,
        )
        .expect("instruction");

        assert_eq!(instruction.program_id.to_string(), PUMP_PROGRAM_ID);
        assert_eq!(instruction.accounts.len(), 16);
        assert_eq!(
            &instruction.data[..8],
            &[214, 144, 76, 236, 95, 139, 49, 180]
        );
    }

    #[test]
    fn parse_bonding_curve_state_reads_mayhem_cashback_and_quote_mint() {
        let creator = Pubkey::new_unique();
        let quote_mint = wsol_mint().expect("wsol mint");
        let mut data = vec![0u8; 8];
        data.extend_from_slice(&1u64.to_le_bytes());
        data.extend_from_slice(&2u64.to_le_bytes());
        data.extend_from_slice(&3u64.to_le_bytes());
        data.extend_from_slice(&4u64.to_le_bytes());
        data.extend_from_slice(&5u64.to_le_bytes());
        data.push(0);
        data.extend_from_slice(creator.as_ref());
        data.push(1);
        data.push(1);
        data.extend_from_slice(quote_mint.as_ref());

        let decoded = parse_bonding_curve_state(&data).expect("decode bonding curve");
        assert!(!decoded.complete);
        assert_eq!(decoded.creator, creator);
        assert!(decoded.is_mayhem_mode);
        assert!(decoded.cashback_enabled);
        assert_eq!(decoded.quote_mint, quote_mint);
    }

    #[test]
    fn buy_exact_sol_instruction_shape_matches_expected_accounts() {
        let global = sample_global();
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let token_2022 = token_2022_program_id().expect("token 2022 program id");
        let instruction = build_buy_exact_sol_in_instruction(
            &global,
            &mint,
            &creator,
            &user,
            100_000_000,
            1_000_000,
            &token_2022,
            false,
        )
        .expect("buy instruction");

        assert_eq!(instruction.program_id.to_string(), PUMP_PROGRAM_ID);
        assert_eq!(instruction.accounts.len(), 27);
        assert_eq!(&instruction.data[..8], &[194, 171, 28, 70, 104, 77, 91, 47]);
        assert_eq!(&instruction.data[8..16], &100_000_000u64.to_le_bytes());
        assert_eq!(&instruction.data[16..24], &1_000_000u64.to_le_bytes());
        assert_eq!(
            instruction.accounts[2].pubkey,
            wsol_mint().expect("wsol mint")
        );
        assert_eq!(instruction.accounts[3].pubkey, token_2022);
        assert_eq!(
            instruction.accounts[4].pubkey,
            token_program_id().expect("token program id")
        );
    }

    #[test]
    fn buy_exact_sol_instruction_keeps_fee_accounts_before_bonding_curve_v2() {
        let global = sample_global();
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let token_2022 = token_2022_program_id().expect("token 2022 program id");
        let instruction = build_buy_exact_sol_in_instruction(
            &global,
            &mint,
            &creator,
            &user,
            101_000_000,
            1_000_000,
            &token_2022,
            false,
        )
        .expect("buy instruction");

        assert_eq!(
            instruction.accounts[18].pubkey,
            fee_sharing_config_pda(&mint).expect("fee sharing config pda")
        );
        assert_eq!(
            instruction.accounts[22].pubkey,
            fee_config_pda().expect("fee config pda")
        );
        assert_eq!(
            instruction.accounts[23].pubkey,
            pump_fee_program_id().expect("fee program id")
        );
        assert_eq!(
            instruction.accounts[10].pubkey,
            bonding_curve_pda(&mint).expect("bonding curve pda")
        );
        assert!(instruction.accounts[18].is_writable);
    }

    #[test]
    fn buy_exact_sol_instruction_uses_supplied_creator_vault_authority() {
        let global = sample_global();
        let mint = Pubkey::new_unique();
        let creator_vault_authority = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let token_2022 = token_2022_program_id().expect("token 2022 program id");
        let instruction = build_buy_exact_sol_in_instruction(
            &global,
            &mint,
            &creator_vault_authority,
            &user,
            101_000_000,
            1_000_000,
            &token_2022,
            false,
        )
        .expect("buy instruction");
        let creator_vault = creator_vault_pda(&creator_vault_authority).expect("creator vault pda");

        assert_eq!(instruction.accounts[16].pubkey, creator_vault);
        assert_eq!(
            instruction.accounts[17].pubkey,
            get_associated_token_address_with_program_id(
                &creator_vault,
                &wsol_mint().expect("wsol mint"),
                &token_program_id().expect("token program id"),
            )
        );
    }

    #[test]
    fn bonding_curve_buy_token_slippage_floors_positive_quote_to_one() {
        assert_eq!(apply_buy_token_slippage(1_000, 0), 1_000);
        assert_eq!(apply_buy_token_slippage(1_000, 5_000), 500);
        assert_eq!(apply_buy_token_slippage(1_000, 10_000), 1);
        assert_eq!(apply_buy_token_slippage(0, 10_000), 0);
    }

    #[test]
    fn split_two_leg_slippage_preserves_total_budget_shape() {
        assert_eq!(split_two_leg_slippage_bps(0), (0, 0));
        assert_eq!(split_two_leg_slippage_bps(501), (251, 250));
        assert_eq!(split_two_leg_slippage_bps(20_000), (5_000, 5_000));
    }

    #[test]
    fn usdc_sell_quote_uses_curve_quote_reserves() {
        let global = sample_global();
        let mut curve = sample_curve();
        curve.quote_mint = usdc_mint().expect("usdc mint");
        curve.real_sol_reserves = 35_000_000_000;
        curve.real_token_reserves = 600_000_000_000_000;
        curve.virtual_sol_reserves = 40_000_000_000;

        assert!(quote_sell_quote_from_curve(&curve, &global, 1_000_000) > 0);
    }

    #[test]
    fn bonding_curve_exact_sol_buy_quote_math_keeps_spend_fixed() {
        let global = sample_global();
        let spend_lamports = 500_000_000;
        let quoted_tokens = quote_buy_tokens_from_sol(&global, spend_lamports);

        assert!(quoted_tokens > 1);
        assert_eq!(apply_buy_token_slippage(quoted_tokens, 0), quoted_tokens);
        assert_eq!(
            apply_buy_token_slippage(quoted_tokens, 5_000),
            quoted_tokens / 2
        );
        assert_eq!(apply_buy_token_slippage(quoted_tokens, 10_000), 1);

        let instruction = build_buy_exact_sol_in_instruction(
            &global,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            spend_lamports,
            apply_buy_token_slippage(quoted_tokens, 10_000),
            &token_2022_program_id().expect("token 2022 program id"),
            false,
        )
        .expect("buy exact sol instruction");
        assert_eq!(&instruction.data[8..16], &spend_lamports.to_le_bytes());
    }

    #[test]
    fn pump_bonding_follow_sell_uses_v2_wrapper() {
        let global = sample_global();
        let mint = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let pump_ix = build_sell_instruction(
            &global,
            &mint,
            &Pubkey::new_unique(),
            &user,
            1_000_000,
            500_000_000,
            &token_2022_program_id().expect("token 2022 program id"),
            false,
            false,
        )
        .expect("pump sell ix");

        let wrapper_ix = build_pump_bonding_v2_sell_wrapper_instruction(
            &user,
            pump_ix,
            1_000_000,
            500_000_000,
            499_500_000,
            10,
        )
        .expect("wrapper sell ix");

        assert_eq!(
            wrapper_ix.program_id.to_string(),
            "TRENCHCfkCTud86C8ZC9kk2CFWJErYz4oZFaYttoxJF"
        );
        assert_eq!(wrapper_ix.data[0], 9);
        assert_eq!(wrapper_ix.accounts.len(), 6 + 26);
        assert_eq!(wrapper_ix.accounts[0].pubkey, user);
        assert_eq!(wrapper_ix.accounts[0].is_signer, true);
        assert_eq!(wrapper_ix.accounts[5].pubkey.to_string(), PUMP_PROGRAM_ID);
    }

    #[test]
    fn wrapper_net_sol_input_subtracts_fee_for_follow_quotes() {
        assert_eq!(
            wrapper_net_sol_input(1_000_000_000, 10).unwrap(),
            999_000_000
        );
        assert_eq!(
            wrapper_net_sol_input(1_000_000_000, 0).unwrap(),
            1_000_000_000
        );
    }

    #[test]
    fn follow_buy_pump_instruction_uses_net_wrapper_input() {
        let global = sample_global();
        let gross_lamports = 1_000_000;
        let net_lamports = wrapper_net_sol_input(gross_lamports, 10).expect("net lamports");
        let instruction = build_buy_exact_sol_in_instruction(
            &global,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            net_lamports,
            1,
            &token_2022_program_id().expect("token 2022 program id"),
            false,
        )
        .expect("buy instruction");

        assert_eq!(&instruction.data[8..16], &net_lamports.to_le_bytes());
        assert_ne!(&instruction.data[8..16], &gross_lamports.to_le_bytes());
    }

    #[test]
    fn token_mode_dev_buy_exact_sol_quote_preserves_first_buy_token_target() {
        let mut config = regular_config();
        config.execution.buySlippagePercent = "100".to_string();
        config.devBuy = Some(crate::config::NormalizedDevBuy {
            mode: "tokens".to_string(),
            amount: "1000".to_string(),
            source: "test".to_string(),
        });
        let global = sample_global();
        let requested_tokens = 1_000u64 * 10u64.pow(TOKEN_DECIMALS);
        let (quoted_sol, quoted_tokens) = resolve_dev_buy_quote(&config, &global)
            .expect("dev buy quote")
            .expect("token-mode quote");

        assert_eq!(quoted_tokens, requested_tokens);
        assert!(quote_buy_tokens_from_sol(&global, quoted_sol) >= requested_tokens);

        let instructions = build_launch_instructions(
            &config,
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            None,
            Some(&global),
        )
        .expect("launch instructions");
        assert_eq!(
            &instructions[3].data[..8],
            &[194, 171, 28, 70, 104, 77, 91, 47]
        );
        assert_eq!(&instructions[3].data[8..16], &quoted_sol.to_le_bytes());
        assert_eq!(&instructions[3].data[16..24], &1u64.to_le_bytes());
        assert_sol_transfer_instruction(
            &instructions[4],
            &instructions[0].accounts[5].pubkey,
            &wrapper_fee_vault(),
            estimate_sol_in_fee_lamports(quoted_sol, config.wrapperDefaultFeeBps),
        );
    }

    #[test]
    fn launch_instructions_match_sdk_create_and_buy_shape_for_sol_dev_buy() {
        let mut config = regular_config();
        config.execution.buySlippagePercent = "5".to_string();
        config.devBuy = Some(crate::config::NormalizedDevBuy {
            mode: "sol".to_string(),
            amount: "0.5".to_string(),
            source: "test".to_string(),
        });
        let global = sample_global();
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let launch_creator = Pubkey::new_unique();

        let instructions =
            build_launch_instructions(&config, mint, creator, launch_creator, None, Some(&global))
                .expect("launch instructions");

        assert_eq!(instructions.len(), 5);
        assert_eq!(instructions[0].program_id.to_string(), PUMP_PROGRAM_ID);
        assert_eq!(instructions[1].program_id.to_string(), PUMP_PROGRAM_ID);
        assert_eq!(
            instructions[2].program_id.to_string(),
            spl_associated_token_account::id().to_string()
        );
        assert_eq!(instructions[3].program_id.to_string(), PUMP_PROGRAM_ID);
        assert_eq!(
            &instructions[3].data[..8],
            &[194, 171, 28, 70, 104, 77, 91, 47]
        );

        let quoted_tokens = quote_buy_tokens_from_sol(&global, 500_000_000);
        let expected_min_tokens_out = apply_buy_token_slippage(quoted_tokens, 500);
        assert_eq!(&instructions[3].data[8..16], &500_000_000u64.to_le_bytes());
        assert_eq!(
            &instructions[3].data[16..24],
            &expected_min_tokens_out.to_le_bytes()
        );
        assert_sol_transfer_instruction(&instructions[4], &creator, &wrapper_fee_vault(), 500_000);
    }

    #[test]
    fn launch_dev_buy_fee_transfer_is_omitted_when_fee_bps_is_zero() {
        let mut config = regular_config();
        config.wrapperDefaultFeeBps = 0;
        config.execution.buySlippagePercent = "5".to_string();
        config.devBuy = Some(crate::config::NormalizedDevBuy {
            mode: "sol".to_string(),
            amount: "0.5".to_string(),
            source: "test".to_string(),
        });
        let global = sample_global();

        let instructions = build_launch_instructions(
            &config,
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            None,
            Some(&global),
        )
        .expect("launch instructions");

        assert_eq!(instructions.len(), 4);
        assert_eq!(instructions[3].program_id.to_string(), PUMP_PROGRAM_ID);
    }

    #[test]
    fn agent_locked_launch_with_dev_buy_defers_agent_initialize() {
        let mut config = regular_config();
        config.mode = "agent-locked".to_string();
        config.agent.buybackBps = Some(2_500);
        config.execution.buySlippagePercent = "5".to_string();
        config.devBuy = Some(crate::config::NormalizedDevBuy {
            mode: "sol".to_string(),
            amount: "0.1".to_string(),
            source: "test".to_string(),
        });
        let global = sample_global();
        let creator = Pubkey::new_unique();
        let agent_authority = Pubkey::new_unique();

        let instructions = build_launch_instructions(
            &config,
            Pubkey::new_unique(),
            creator,
            Pubkey::new_unique(),
            Some(&agent_authority),
            Some(&global),
        )
        .expect("agent launch instructions");

        assert_eq!(instructions.len(), 5);
        assert_eq!(instructions[3].program_id.to_string(), PUMP_PROGRAM_ID);
        assert_sol_transfer_instruction(&instructions[4], &creator, &wrapper_fee_vault(), 100_000);
    }

    #[test]
    fn launch_dev_buy_transactions_fit_with_deployed_shared_alt() {
        // Keep this tied to a fixture of the deployed shared ALT instead of
        // dynamically adding launch-specific accounts that cannot exist in one global table.
        let modes = [
            "regular",
            "cashback",
            "agent-unlocked",
            "agent-locked",
            "agent-custom",
        ];
        let dev_buy_modes = [("sol", "0.5"), ("tokens", "1000")];
        let global = sample_global();

        for mode in modes {
            for (dev_buy_mode, amount) in dev_buy_modes {
                let mut config = regular_config();
                config.mode = mode.to_string();
                config.execution.txFormat = "v0-alt".to_string();
                config.execution.buySlippagePercent = "5".to_string();
                config.devBuy = Some(crate::config::NormalizedDevBuy {
                    mode: dev_buy_mode.to_string(),
                    amount: amount.to_string(),
                    source: "test".to_string(),
                });
                if mode.starts_with("agent-") {
                    config.agent.buybackBps = Some(2_500);
                }
                if mode == "agent-custom" {
                    config.agent.feeRecipients = vec![
                        crate::config::NormalizedRecipient {
                            r#type: Some("agent".to_string()),
                            address: String::new(),
                            githubUserId: String::new(),
                            githubUsername: String::new(),
                            shareBps: 2_500,
                        },
                        crate::config::NormalizedRecipient {
                            r#type: Some("wallet".to_string()),
                            address: Pubkey::new_unique().to_string(),
                            githubUserId: String::new(),
                            githubUsername: String::new(),
                            shareBps: 7_500,
                        },
                    ];
                }

                let payer = Keypair::new();
                let mint = Keypair::new();
                let agent_authority = mode.starts_with("agent-").then(Pubkey::new_unique);
                let core_instructions = build_launch_instructions(
                    &config,
                    mint.pubkey(),
                    payer.pubkey(),
                    payer.pubkey(),
                    agent_authority.as_ref(),
                    Some(&global),
                )
                .expect("launch instructions");
                let tx_config = NativeTxConfig {
                    compute_unit_limit: configured_launch_compute_unit_limit(&config).unwrap(),
                    compute_unit_price_micro_lamports: config
                        .tx
                        .computeUnitPriceMicroLamports
                        .unwrap_or_default(),
                    jito_tip_lamports: config.tx.jitoTipLamports,
                    jito_tip_account: config.tx.jitoTipAccount.clone(),
                };
                let instructions = with_tx_settings(
                    core_instructions,
                    &tx_config,
                    &payer.pubkey(),
                    config.execution.jitodontfront,
                )
                .expect("tx settings");
                let lookup_table = deployed_shared_alt_lookup_table_fixture();
                let candidate = compile_transaction_candidate(
                    "launch-fit-test",
                    NativeTxFormat::V0Alt,
                    &Hash::new_unique().to_string(),
                    0,
                    &payer,
                    Some(&mint),
                    instructions,
                    &tx_config,
                    &[lookup_table],
                )
                .unwrap_or_else(|error| {
                    panic!("compile failed for mode={mode} dev_buy_mode={dev_buy_mode}: {error}")
                });

                assert!(
                    candidate.serialized_len <= 1232,
                    "mode={mode} dev_buy_mode={dev_buy_mode} deployed shared ALT serialized to {} bytes",
                    candidate.serialized_len
                );
            }
        }
    }

    #[test]
    fn synthetic_curve_after_buy_reduces_follow_buy_output() {
        let global = sample_global();
        let launch_creator = Pubkey::new_unique();
        let spend_lamports = 500_000_000;
        let creator_dev_buy_tokens = quote_buy_tokens_from_sol(&global, 250_000_000);
        let base_quote = quote_buy_tokens_from_sol(&global, spend_lamports);
        let advanced_curve = synthetic_curve_after_buy_tokens(
            &global,
            &launch_creator,
            creator_dev_buy_tokens,
            false,
        );
        let advanced_quote = quote_buy_tokens_from_curve(&advanced_curve, &global, spend_lamports);

        assert!(advanced_quote < base_quote);
        assert!(advanced_curve.real_sol_reserves > 0);
        assert!(advanced_curve.real_token_reserves < global.initial_real_token_reserves);
    }

    #[test]
    fn agent_locked_launch_defers_agent_initialize_to_setup_tx() {
        let mut config = regular_config();
        config.mode = "agent-locked".to_string();
        config.agent.buybackBps = Some(2_500);
        config.creatorFee.mode = "agent-escrow".to_string();
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let launch_creator = Pubkey::new_unique();
        let agent_authority = Pubkey::new_unique();

        let instructions = build_launch_instructions(
            &config,
            mint,
            creator,
            launch_creator,
            Some(&agent_authority),
            None,
        )
        .expect("agent locked launch instructions");

        assert_eq!(instructions.len(), 1);
        assert_eq!(instructions[0].program_id.to_string(), PUMP_PROGRAM_ID);
    }

    #[test]
    fn agent_custom_split_launch_defers_agent_initialize_to_setup_tx() {
        let mut config = regular_config();
        config.mode = "agent-custom".to_string();
        config.agent.buybackBps = Some(2_500);
        config.agent.splitAgentInit = true;
        config.agent.feeRecipients = vec![
            crate::config::NormalizedRecipient {
                r#type: Some("agent".to_string()),
                address: String::new(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 2_500,
            },
            crate::config::NormalizedRecipient {
                r#type: Some("wallet".to_string()),
                address: Pubkey::new_unique().to_string(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 7_500,
            },
        ];
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let launch_creator = Pubkey::new_unique();
        let agent_authority = Pubkey::new_unique();

        let instructions = build_launch_instructions(
            &config,
            mint,
            creator,
            launch_creator,
            Some(&agent_authority),
            None,
        )
        .expect("agent custom launch instructions");

        assert_eq!(instructions.len(), 1);
        assert_eq!(instructions[0].program_id.to_string(), PUMP_PROGRAM_ID);
    }

    #[test]
    fn agent_unlocked_launch_does_not_initialize_agent() {
        let mut config = regular_config();
        config.mode = "agent-unlocked".to_string();
        config.agent.buybackBps = Some(2_500);
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let launch_creator = Pubkey::new_unique();
        let agent_authority = Pubkey::new_unique();

        let instructions = build_launch_instructions(
            &config,
            mint,
            creator,
            launch_creator,
            Some(&agent_authority),
            None,
        )
        .expect("agent unlocked launch instructions");

        assert_eq!(instructions.len(), 1);
        assert_eq!(instructions[0].program_id.to_string(), PUMP_PROGRAM_ID);
        assert_eq!(native_follow_up_label(&config), None);
    }

    #[test]
    fn agent_custom_without_split_keeps_agent_initialize_in_creation_tx() {
        let mut config = regular_config();
        config.mode = "agent-custom".to_string();
        config.agent.buybackBps = Some(2_500);
        config.agent.splitAgentInit = false;
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let launch_creator = Pubkey::new_unique();
        let agent_authority = Pubkey::new_unique();

        let instructions = build_launch_instructions(
            &config,
            mint,
            creator,
            launch_creator,
            Some(&agent_authority),
            None,
        )
        .expect("agent custom launch instructions");

        assert_eq!(instructions.len(), 2);
        assert_eq!(instructions[0].program_id.to_string(), PUMP_PROGRAM_ID);
        assert_eq!(
            instructions[1].program_id.to_string(),
            PUMP_AGENT_PAYMENTS_PROGRAM_ID
        );
    }

    #[test]
    fn compute_budget_instruction_layout_matches_web3() {
        let compute_unit_limit = 340_000u32;
        let limit =
            build_compute_unit_limit_instruction(compute_unit_limit).expect("limit instruction");
        let price = build_compute_unit_price_instruction(123_456).expect("price instruction");

        assert_eq!(limit.program_id.to_string(), COMPUTE_BUDGET_PROGRAM_ID);
        assert_eq!(
            limit.data,
            [&[2u8][..], &compute_unit_limit.to_le_bytes()[..]].concat()
        );
        assert_eq!(price.program_id.to_string(), COMPUTE_BUDGET_PROGRAM_ID);
        assert_eq!(price.data, [3, 64, 226, 1, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn social_fee_pda_instruction_shape_matches_expected_accounts() {
        let payer = Pubkey::new_unique();
        let instruction = build_create_social_fee_pda_instruction(&payer, "12345", PLATFORM_GITHUB)
            .expect("social fee instruction");

        assert_eq!(instruction.program_id.to_string(), PUMP_FEE_PROGRAM_ID);
        assert_eq!(instruction.accounts.len(), 6);
        assert_eq!(
            &instruction.data[..8],
            &[144, 224, 59, 211, 78, 248, 202, 220]
        );
    }

    #[test]
    fn follow_sell_uses_sharing_config_when_post_setup_is_preferred() {
        let launch_creator = Pubkey::new_unique();
        let sharing_config = Pubkey::new_unique();

        let authority =
            select_follow_creator_vault_authority(&launch_creator, &sharing_config, true, false);

        assert_eq!(authority, sharing_config);
    }

    #[test]
    fn follow_sell_switches_to_sharing_config_once_live() {
        let launch_creator = Pubkey::new_unique();
        let sharing_config = Pubkey::new_unique();

        let authority =
            select_follow_creator_vault_authority(&launch_creator, &sharing_config, true, true);

        assert_eq!(authority, sharing_config);
    }

    #[test]
    fn follow_sell_uses_launch_creator_when_post_setup_is_not_preferred() {
        let launch_creator = Pubkey::new_unique();
        let sharing_config = Pubkey::new_unique();

        let authority =
            select_follow_creator_vault_authority(&launch_creator, &sharing_config, false, false);

        assert_eq!(authority, launch_creator);
    }

    #[test]
    fn create_fee_sharing_config_instruction_matches_live_account_shape() {
        let mint = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let instruction = build_create_fee_sharing_config_instruction(&mint, &payer)
            .expect("create fee sharing config instruction");

        assert_eq!(instruction.program_id.to_string(), PUMP_FEE_PROGRAM_ID);
        assert_eq!(instruction.accounts.len(), 13);
        assert_eq!(
            instruction.accounts[10].pubkey,
            pump_fee_program_id().expect("fee program id")
        );
        assert_eq!(
            instruction.accounts[11].pubkey,
            pump_amm_program_id().expect("pump amm program id")
        );
        assert_eq!(
            instruction.accounts[12].pubkey,
            pump_fee_program_id().expect("fee program id")
        );
        assert_eq!(
            &instruction.data[..8],
            &[195, 78, 86, 76, 111, 52, 251, 213]
        );
    }

    #[test]
    fn agent_initialize_instruction_shape_matches_sdk_layout() {
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let agent_authority = Pubkey::new_unique();
        let instruction =
            build_agent_initialize_instruction(&mint, &creator, &agent_authority, 2_500)
                .expect("agent initialize instruction");

        assert_eq!(
            instruction.program_id.to_string(),
            PUMP_AGENT_PAYMENTS_PROGRAM_ID
        );
        assert_eq!(instruction.accounts.len(), 8);
        assert_eq!(&instruction.data[..8], &[180, 248, 163, 8, 49, 94, 126, 96]);
        assert_eq!(&instruction.data[8..40], agent_authority.as_ref(),);
        assert_eq!(&instruction.data[40..42], &2_500u16.to_le_bytes());
    }

    #[tokio::test]
    async fn fee_sharing_follow_up_contains_two_instructions_without_revoke() {
        let mut config = regular_config();
        config.feeSharing.generateLaterSetup = true;
        config.feeSharing.recipients = vec![crate::config::NormalizedRecipient {
            r#type: Some("wallet".to_string()),
            address: Pubkey::new_unique().to_string(),
            githubUserId: String::new(),
            githubUsername: String::new(),
            shareBps: 10_000,
        }];
        let instructions = build_fee_sharing_follow_up_instructions(
            "http://127.0.0.1:8899",
            &config,
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        )
        .await
        .expect("follow-up instructions");

        assert_eq!(instructions.len(), 2);
        assert_eq!(instructions[0].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
        assert_eq!(instructions[1].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
    }

    #[tokio::test]
    async fn agent_locked_follow_up_contains_agent_initialize_and_fee_setup() {
        let mut config = regular_config();
        config.mode = "agent-locked".to_string();
        config.agent.buybackBps = Some(2_500);
        config.creatorFee.mode = "agent-escrow".to_string();
        let creator = Pubkey::new_unique();
        let agent_authority = Pubkey::new_unique();
        let instructions = build_native_follow_up_instructions(
            "http://127.0.0.1:8899",
            &config,
            Pubkey::new_unique(),
            creator,
            Some(&agent_authority),
        )
        .await
        .expect("agent locked follow-up instructions");

        assert_eq!(instructions.len(), 3);
        assert_eq!(
            instructions[0].program_id.to_string(),
            PUMP_AGENT_PAYMENTS_PROGRAM_ID
        );
        assert_eq!(instructions[1].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
        assert_eq!(instructions[2].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
    }

    #[tokio::test]
    async fn agent_custom_follow_up_contains_agent_initialize_and_fee_setup() {
        let mut config = regular_config();
        config.mode = "agent-custom".to_string();
        config.agent.buybackBps = Some(2_500);
        config.agent.splitAgentInit = true;
        config.agent.feeRecipients = vec![
            crate::config::NormalizedRecipient {
                r#type: Some("agent".to_string()),
                address: String::new(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 2_500,
            },
            crate::config::NormalizedRecipient {
                r#type: Some("wallet".to_string()),
                address: Pubkey::new_unique().to_string(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 7_500,
            },
        ];
        let creator = Pubkey::new_unique();
        let agent_authority = Pubkey::new_unique();
        let instructions = build_native_follow_up_instructions(
            "http://127.0.0.1:8899",
            &config,
            Pubkey::new_unique(),
            creator,
            Some(&agent_authority),
        )
        .await
        .expect("agent custom follow-up instructions");

        assert_eq!(instructions.len(), 3);
        assert_eq!(
            instructions[0].program_id.to_string(),
            PUMP_AGENT_PAYMENTS_PROGRAM_ID
        );
        assert_eq!(instructions[1].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
        assert_eq!(instructions[2].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
    }

    #[test]
    fn agent_unlocked_has_no_follow_up_transaction() {
        let mut config = regular_config();
        config.mode = "agent-unlocked".to_string();
        config.agent.buybackBps = Some(2_500);

        assert_eq!(native_follow_up_label(&config), None);
    }

    #[test]
    fn standard_rpc_follow_buy_tip_is_ignored_when_blank() {
        let (tip_lamports, tip_account) =
            resolve_follow_tip_config("standard-rpc", "", "tip-account", "buy tip")
                .expect("standard rpc buy tip");
        assert_eq!(tip_lamports, 0);
        assert!(tip_account.is_empty());
    }

    #[test]
    fn standard_rpc_follow_sell_tip_is_ignored_even_when_present() {
        let (tip_lamports, tip_account) =
            resolve_follow_tip_config("standard-rpc", "0.01", "tip-account", "sell tip")
                .expect("standard rpc sell tip");
        assert_eq!(tip_lamports, 0);
        assert!(tip_account.is_empty());
    }

    #[test]
    fn jito_follow_tip_still_requires_valid_tip_value() {
        let error = resolve_follow_tip_config("jito-bundle", "", "tip-account", "buy tip")
            .expect_err("blank jito tip should fail");
        assert!(error.contains("buy tip"));
    }

    #[test]
    fn hellomoon_follow_tip_requires_at_least_point_zero_zero_one_sol() {
        let (lamports, account) =
            resolve_follow_tip_config("hellomoon", "0.001", "tip-account", "buy tip")
                .expect("valid hellomoon tip");
        assert_eq!(lamports, 1_000_000);
        assert_eq!(account, "tip-account");
        let error = resolve_follow_tip_config("hellomoon", "0.0001", "tip-account", "buy tip")
            .expect_err("sub-minimum hellomoon tip should fail");
        assert!(error.contains("0.001 SOL"), "unexpected: {error}");
    }

    #[test]
    fn agent_custom_without_init_has_no_follow_up_transaction() {
        let mut config = regular_config();
        config.mode = "agent-custom".to_string();
        config.agent.buybackBps = Some(0);
        config.agent.splitAgentInit = false;
        config.agent.feeRecipients = vec![];

        assert_eq!(native_follow_up_label(&config), None);
    }

    #[test]
    fn agent_custom_split_with_implicit_buyback_has_follow_up_transaction() {
        let mut config = regular_config();
        config.mode = "agent-custom".to_string();
        config.agent.buybackBps = Some(2_500);
        config.agent.splitAgentInit = true;
        config.agent.feeRecipients = vec![];

        assert_eq!(native_follow_up_label(&config), Some("agent-setup"));
    }

    #[test]
    fn agent_custom_split_with_explicit_recipients_has_follow_up_transaction() {
        let mut config = regular_config();
        config.mode = "agent-custom".to_string();
        config.agent.buybackBps = Some(0);
        config.agent.splitAgentInit = true;
        config.agent.feeRecipients = vec![crate::config::NormalizedRecipient {
            r#type: Some("wallet".to_string()),
            address: Pubkey::new_unique().to_string(),
            githubUserId: String::new(),
            githubUsername: String::new(),
            shareBps: 10_000,
        }];

        assert_eq!(native_follow_up_label(&config), Some("agent-setup"));
    }

    #[test]
    fn update_fee_shares_uses_current_shareholders_as_remaining_accounts() {
        let mint = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let current_shareholder = Pubkey::new_unique();
        let next_shareholder = NormalizedRecipient {
            r#type: Some("wallet".to_string()),
            address: Pubkey::new_unique().to_string(),
            githubUserId: String::new(),
            githubUsername: String::new(),
            shareBps: 10_000,
        };

        let instruction = build_update_fee_shares_instruction(
            &mint,
            &authority,
            &[current_shareholder],
            &[next_shareholder],
        )
        .expect("update fee shares instruction");

        assert_eq!(instruction.program_id.to_string(), PUMP_FEE_PROGRAM_ID);
        assert_eq!(
            instruction.accounts.last().map(|account| account.pubkey),
            Some(current_shareholder)
        );
        assert!(
            instruction
                .accounts
                .last()
                .map(|account| account.is_writable)
                .unwrap_or(false)
        );
        assert!(
            !instruction
                .accounts
                .last()
                .map(|account| account.is_signer)
                .unwrap_or(true)
        );
    }

    #[test]
    fn parse_pump_amm_pool_state_reads_core_fields() {
        let creator = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        let lp_mint = Pubkey::new_unique();
        let base_vault = Pubkey::new_unique();
        let quote_vault = Pubkey::new_unique();
        let coin_creator = Pubkey::new_unique();
        let mut data = vec![0u8; 301];
        let mut offset = PUMP_AMM_POOL_DISCRIMINATOR_BYTES;
        data[offset] = 254;
        offset += 1;
        data[offset..offset + 2].copy_from_slice(&0u16.to_le_bytes());
        offset += 2;
        data[offset..offset + 32].copy_from_slice(creator.as_ref());
        offset += 32;
        data[offset..offset + 32].copy_from_slice(base_mint.as_ref());
        offset += 32;
        data[offset..offset + 32].copy_from_slice(quote_mint.as_ref());
        offset += 32;
        data[offset..offset + 32].copy_from_slice(lp_mint.as_ref());
        offset += 32;
        data[offset..offset + 32].copy_from_slice(base_vault.as_ref());
        offset += 32;
        data[offset..offset + 32].copy_from_slice(quote_vault.as_ref());
        offset += 32;
        data[offset..offset + 8].copy_from_slice(&123u64.to_le_bytes());
        offset += 8;
        data[offset..offset + 32].copy_from_slice(coin_creator.as_ref());

        let parsed = parse_pump_amm_pool_state("pool", &data).expect("pump amm pool should parse");
        assert_eq!(parsed.pubkey, "pool");
        assert_eq!(parsed.creator, creator);
        assert_eq!(parsed.base_mint, base_mint);
        assert_eq!(parsed.quote_mint, quote_mint);
        assert_eq!(parsed.pool_base_token_account, base_vault);
        assert_eq!(parsed.pool_quote_token_account, quote_vault);
    }

    #[test]
    fn current_market_cap_quote_units_scales_from_reserves() {
        assert_eq!(
            current_market_cap_quote_units(1_000_000_000_000, 500_000_000, 250_000_000_000),
            2_000_000_000
        );
    }

    #[test]
    fn merge_persisted_lookup_table_caches_keeps_entries_from_both_sources() {
        let merged = merge_persisted_lookup_table_caches([
            PersistedLookupTableCache {
                tables: HashMap::from([(
                    "shared".to_string(),
                    PersistedLookupTableEntry {
                        addresses: vec!["A".to_string()],
                        address_count: None,
                        content_hash: None,
                        manifest_hash: None,
                    },
                )]),
            },
            PersistedLookupTableCache {
                tables: HashMap::from([(
                    "legacy".to_string(),
                    PersistedLookupTableEntry {
                        addresses: vec!["B".to_string()],
                        address_count: None,
                        content_hash: None,
                        manifest_hash: None,
                    },
                )]),
            },
        ]);

        assert_eq!(merged.tables.len(), 2);
        assert_eq!(merged.tables["shared"].addresses, vec!["A".to_string()]);
        assert_eq!(merged.tables["legacy"].addresses, vec!["B".to_string()]);
    }
}
