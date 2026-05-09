#![allow(non_snake_case, dead_code)]

use serde::Serialize;
use serde_json::Value;
#[allow(unused_imports)]
pub use shared_extension_runtime::catalog::{
    LaunchpadRuntimeCapabilities, launchpad_runtime_capabilities,
};
use solana_sdk::{pubkey::Pubkey, signature::Keypair};
use std::str::FromStr;

use crate::{
    bags_native::{
        BagsFeeEstimateSnapshot, BagsFeeRecipientLookupResponse, BagsFollowBuyContext,
        BagsImportContext, BagsMarketSnapshot, NativeBagsArtifacts,
        compile_atomic_follow_buy_transaction as compile_atomic_bags_follow_buy,
        compile_follow_buy_transaction as compile_bags_follow_buy,
        compile_follow_sell_transaction as compile_bags_follow_sell,
        derive_follow_owner_token_account as derive_bags_follow_owner_token_account,
        detect_bags_import_context, fetch_bags_market_snapshot, lookup_bags_fee_recipient,
        quote_launch as quote_bags_launch, try_compile_native_bags, warm_bags_helper_ping,
    },
    bonk_native::{
        BonkImportContext, BonkMarketSnapshot, BonkUsd1RouteSetup, NativeBonkArtifacts,
        NativeBonkPoolContext,
        compile_atomic_follow_buy_transaction_with_metadata as compile_atomic_bonk_follow_buy_with_metadata,
        compile_follow_buy_transaction_with_metadata as compile_bonk_follow_buy_with_metadata,
        compile_follow_sell_transaction_with_token_amount_and_settlement as compile_bonk_follow_sell_with_token_amount_and_settlement,
        derive_canonical_pool_id as derive_bonk_canonical_pool_id,
        derive_follow_owner_token_account as derive_bonk_follow_owner_token_account,
        detect_bonk_import_context, detect_bonk_import_context_with_quote_asset,
        fetch_bonk_market_snapshot,
        predict_dev_buy_token_amount as predict_bonk_dev_buy_token_amount,
        quote_launch as quote_bonk_launch, try_compile_native_bonk, warm_bonk_state,
    },
    config::{
        LaunchpadActionBackendMode, NormalizedConfig, NormalizedExecution,
        configured_launchpad_action_backend_mode,
    },
    follow::BagsLaunchMetadata,
    pump_native::{
        LaunchQuote, NativeCompileTimings, NativePumpArtifacts, PumpMarketSnapshot,
        PumpPreviewBasis, compile_atomic_follow_buy_transaction as compile_atomic_pump_follow_buy,
        compile_follow_buy_transaction as compile_pump_follow_buy,
        compile_follow_sell_transaction_with_token_amount as compile_pump_follow_sell_with_token_amount,
        derive_follow_owner_token_account as derive_pump_follow_owner_token_account,
        fetch_pump_market_snapshot,
        predict_dev_buy_token_amount as predict_pump_dev_buy_token_amount,
        quote_launch as quote_pump_launch, try_compile_native_pump, warm_default_lookup_tables,
        warm_pump_global_state,
    },
    rpc::CompiledTransaction,
    transport::TransportPlan,
    vanity_pool::VanityReservation,
    wrapper_compile::{
        LaunchdeckWrapRequest, WrapperRouteKind, estimate_sol_in_fee_lamports,
        load_shared_lookup_tables, parse_sol_amount_to_lamports, refresh_shared_lookup_tables,
        wrap_compiled_transaction,
    },
};

#[derive(Debug, Clone)]
pub struct NativeLaunchArtifacts {
    pub compiled_transactions: Vec<CompiledTransaction>,
    pub creation_transactions: Vec<CompiledTransaction>,
    pub deferred_setup_transactions: Vec<CompiledTransaction>,
    /// Jito-style setup bundles (e.g. Bags fee-share) executed before `setup_transactions`.
    pub setup_bundles: Vec<Vec<CompiledTransaction>>,
    /// Sequential setup transactions after bundles (Bags fee-share direct txs).
    pub setup_transactions: Vec<CompiledTransaction>,
    /// Follow daemon metadata when Bags uses a prelaunch setup send path.
    pub bags_launch_follow: Option<BagsLaunchMetadata>,
    pub bags_config_key: String,
    pub bags_metadata_uri: String,
    pub bags_fee_estimate: Option<BagsFeeEstimateSnapshot>,
    pub bags_prepare_launch_ms: Option<u128>,
    pub bags_metadata_upload_ms: Option<u128>,
    pub bags_fee_recipient_resolve_ms: Option<u128>,
    pub report: Value,
    pub text: String,
    pub compile_timings: NativeCompileTimings,
    pub mint: String,
    pub launch_creator: String,
    pub vanity_reservation: Option<VanityReservation>,
}

impl NativeLaunchArtifacts {
    pub fn bags_needs_prelaunch_setup(&self) -> bool {
        !self.setup_bundles.is_empty() || !self.setup_transactions.is_empty()
    }
}

pub fn launchpad_action_backend(launchpad: &str, action: &str) -> Option<&'static str> {
    if launchpad.trim().eq_ignore_ascii_case("bonk")
        || launchpad.trim().eq_ignore_ascii_case("bagsapp")
    {
        return Some(match action {
            "startup-warm" | "quote" | "market-snapshot" | "import-context" | "follow-buy"
            | "follow-sell" | "build-launch" => "rust-native",
            _ => "rust-native",
        });
    }
    let launchpad_key = launchpad.trim().to_ascii_lowercase();
    let default_backend = match (launchpad_key.as_str(), action) {
        ("pump", _) => Some("rust-native"),
        _ => None,
    }?;
    match configured_launchpad_action_backend_mode(launchpad, action) {
        LaunchpadActionBackendMode::Helper => Some("helper-bridge"),
        LaunchpadActionBackendMode::Rust => Some("rust-native"),
        LaunchpadActionBackendMode::Auto => Some(default_backend),
    }
}

pub fn launchpad_action_rollout_state(launchpad: &str, action: &str) -> Option<&'static str> {
    match launchpad_action_backend(launchpad, action) {
        Some("rust-native") => Some("rust-only"),
        Some("helper-bridge") => Some("helper-backed"),
        Some("rust-primary-fallback") => Some("rust-primary-fallback"),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct FollowBuyCompileRequest<'a> {
    pub launchpad: &'a str,
    pub launch_mode: &'a str,
    pub quote_asset: &'a str,
    pub rpc_url: &'a str,
    pub execution: &'a NormalizedExecution,
    pub token_mayhem_mode: bool,
    pub jito_tip_account: &'a str,
    pub wallet_secret: &'a [u8],
    pub mint: &'a str,
    pub launch_creator: &'a str,
    pub buy_amount_sol: &'a str,
    pub allow_ata_creation: bool,
    pub prefer_post_setup_creator_vault: bool,
    pub bonk_pool_context: Option<&'a NativeBonkPoolContext>,
    pub bonk_pool_id: Option<&'a str>,
    pub bonk_usd1_route_setup: Option<&'a BonkUsd1RouteSetup>,
    pub bags_follow_buy_context: Option<&'a BagsFollowBuyContext>,
    pub bags_launch: Option<&'a BagsLaunchMetadata>,
    pub wrapper_fee_bps: u16,
}

#[derive(Debug, Clone)]
pub struct FollowBuyCompiledTransactions {
    pub transactions: Vec<CompiledTransaction>,
    pub primary_tx_index: usize,
    pub requires_ordered_execution: bool,
}

#[derive(Debug, Clone)]
pub struct FollowSellCompileRequest<'a> {
    pub launchpad: &'a str,
    pub quote_asset: &'a str,
    pub rpc_url: &'a str,
    pub execution: &'a NormalizedExecution,
    pub token_mayhem_mode: bool,
    pub jito_tip_account: &'a str,
    pub wallet_secret: &'a [u8],
    pub mint: &'a str,
    pub launch_creator: &'a str,
    pub sell_percent: u8,
    pub prefer_post_setup_creator_vault: bool,
    pub token_amount_override: Option<u64>,
    pub bonk_pool_id: Option<&'a str>,
    pub bonk_launch_mode: Option<&'a str>,
    pub bonk_launch_creator: Option<&'a str>,
    pub pump_cashback_enabled_override: Option<bool>,
    pub bags_launch: Option<&'a BagsLaunchMetadata>,
    pub wrapper_fee_bps: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "launchpad", rename_all = "kebab-case")]
pub enum LaunchpadStartupWarmResult {
    Pump {
        #[serde(skip_serializing_if = "Option::is_none")]
        lookupTablesLoaded: Option<usize>,
        previewBasis: PumpPreviewBasis,
    },
    Bonk {
        payload: Value,
    },
    Bagsapp {
        payload: Value,
    },
}

#[derive(Debug, Clone)]
pub enum LaunchpadMarketSnapshot {
    Pump(PumpMarketSnapshot),
    Bonk(BonkMarketSnapshot),
    Bags(BagsMarketSnapshot),
}

impl LaunchpadMarketSnapshot {
    pub fn quote_asset(&self) -> &str {
        match self {
            Self::Pump(snapshot) => snapshot.quoteAsset.as_str(),
            Self::Bonk(snapshot) => snapshot.quoteAsset.as_str(),
            Self::Bags(snapshot) => snapshot.quoteAsset.as_str(),
        }
    }

    pub fn market_cap_lamports_u64(&self) -> Result<u64, String> {
        match self {
            Self::Pump(snapshot) => Ok(snapshot.marketCapLamports),
            Self::Bonk(snapshot) => snapshot
                .marketCapLamports
                .parse::<u64>()
                .map_err(|error| format!("Invalid Bonk market cap payload: {error}")),
            Self::Bags(snapshot) => snapshot
                .marketCapLamports
                .parse::<u64>()
                .map_err(|error| format!("Invalid Bags market cap payload: {error}")),
        }
    }
}

#[derive(Debug, Clone)]
pub enum LaunchpadImportContext {
    Bonk(BonkImportContext),
    Bags(BagsImportContext),
}

pub async fn warm_launchpad_for_startup(
    launchpad: &str,
    rpc_url: &str,
) -> Result<Option<LaunchpadStartupWarmResult>, String> {
    match launchpad.trim().to_ascii_lowercase().as_str() {
        "pump" => {
            let lookup_tables_loaded = warm_default_lookup_tables(rpc_url).await?;
            let preview_basis = warm_pump_global_state(rpc_url).await?;
            Ok(Some(LaunchpadStartupWarmResult::Pump {
                lookupTablesLoaded: Some(lookup_tables_loaded),
                previewBasis: preview_basis,
            }))
        }
        "bonk" => Ok(Some(LaunchpadStartupWarmResult::Bonk {
            payload: warm_bonk_state(rpc_url).await?,
        })),
        "bagsapp" => Ok(Some(LaunchpadStartupWarmResult::Bagsapp {
            payload: warm_bags_helper_ping().await?,
        })),
        _ => Ok(None),
    }
}

impl From<NativePumpArtifacts> for NativeLaunchArtifacts {
    fn from(value: NativePumpArtifacts) -> Self {
        Self {
            compiled_transactions: value.compiled_transactions,
            creation_transactions: value.creation_transactions,
            deferred_setup_transactions: value.deferred_setup_transactions,
            setup_bundles: Vec::new(),
            setup_transactions: Vec::new(),
            bags_launch_follow: None,
            bags_config_key: String::new(),
            bags_metadata_uri: String::new(),
            bags_fee_estimate: None,
            bags_prepare_launch_ms: None,
            bags_metadata_upload_ms: None,
            bags_fee_recipient_resolve_ms: None,
            report: value.report,
            text: value.text,
            compile_timings: value.compile_timings,
            mint: value.mint,
            launch_creator: value.launch_creator,
            vanity_reservation: value.vanity_reservation,
        }
    }
}

impl From<NativeBonkArtifacts> for NativeLaunchArtifacts {
    fn from(value: NativeBonkArtifacts) -> Self {
        Self {
            compiled_transactions: value.compiled_transactions,
            creation_transactions: value.creation_transactions,
            deferred_setup_transactions: value.deferred_setup_transactions,
            setup_bundles: Vec::new(),
            setup_transactions: Vec::new(),
            bags_launch_follow: None,
            bags_config_key: String::new(),
            bags_metadata_uri: String::new(),
            bags_fee_estimate: None,
            bags_prepare_launch_ms: None,
            bags_metadata_upload_ms: None,
            bags_fee_recipient_resolve_ms: None,
            report: value.report,
            text: value.text,
            compile_timings: value.compile_timings,
            mint: value.mint,
            launch_creator: value.launch_creator,
            vanity_reservation: value.vanity_reservation,
        }
    }
}

impl From<NativeBagsArtifacts> for NativeLaunchArtifacts {
    fn from(value: NativeBagsArtifacts) -> Self {
        let compiled_transactions = value.compiled_transactions.clone();
        let bags_launch_follow = Some(BagsLaunchMetadata {
            configKey: value.config_key.clone(),
            migrationFeeOption: value.migration_fee_option,
            expectedMigrationFamily: value.expected_migration_family.clone(),
            expectedDammConfigKey: value.expected_damm_config_key.clone(),
            expectedDammDerivationMode: value.expected_damm_derivation_mode.clone(),
            preMigrationDbcPoolAddress: value.pre_migration_dbc_pool_address.clone(),
            postMigrationDammPoolAddress: String::new(),
        });
        Self {
            creation_transactions: value.compiled_transactions,
            compiled_transactions,
            deferred_setup_transactions: vec![],
            setup_bundles: value.setup_bundles,
            setup_transactions: value.setup_transactions,
            bags_launch_follow,
            bags_config_key: value.config_key,
            bags_metadata_uri: value.metadata_uri,
            bags_fee_estimate: Some(value.fee_estimate),
            bags_prepare_launch_ms: value.prepare_launch_ms,
            bags_metadata_upload_ms: value.metadata_upload_ms,
            bags_fee_recipient_resolve_ms: value.fee_recipient_resolve_ms,
            report: value.report,
            text: value.text,
            compile_timings: value.compile_timings,
            mint: value.mint,
            launch_creator: value.launch_creator,
            vanity_reservation: None,
        }
    }
}

/// `launch_blockhash_prime`: optional `(blockhash, last_valid_block_height)` from
/// [`crate::launchpad_warm::build_launchpad_warm_context`] for the same `rpc_url` and `config.execution.commitment`.
pub async fn try_compile_native_launchpad(
    rpc_url: &str,
    config: &NormalizedConfig,
    transport_plan: &TransportPlan,
    wallet_secret: &[u8],
    built_at: String,
    creator_public_key: String,
    config_path: Option<String>,
    allow_ata_creation: bool,
    // Blockhash from request-scoped warm prime (`LaunchpadWarmContext`); same `rpc_url` + commitment as compile.
    launch_blockhash_prime: Option<(String, u64)>,
) -> Result<Option<NativeLaunchArtifacts>, String> {
    let artifacts = match config.launchpad.as_str() {
        "pump" => try_compile_native_pump(
            rpc_url,
            config,
            transport_plan,
            wallet_secret,
            built_at,
            creator_public_key,
            config_path,
            allow_ata_creation,
            launch_blockhash_prime,
        )
        .await
        .map(|result| result.map(Into::into)),
        "bonk" => try_compile_native_bonk(
            rpc_url,
            config,
            transport_plan,
            wallet_secret,
            built_at,
            creator_public_key,
            config_path,
            allow_ata_creation,
        )
        .await
        .map(|result| result.map(Into::into)),
        "bagsapp" => try_compile_native_bags(
            rpc_url,
            config,
            transport_plan,
            wallet_secret,
            built_at,
            creator_public_key,
            config_path,
            allow_ata_creation,
            launch_blockhash_prime,
        )
        .await
        .map(|result| result.map(Into::into)),
        _ => Ok(None),
    }?;
    maybe_wrap_launch_dev_buy_artifacts(rpc_url, config, wallet_secret, artifacts).await
}

pub async fn quote_launch_for_launchpad(
    rpc_url: &str,
    launchpad: &str,
    quote_asset: &str,
    launch_mode: &str,
    mode: &str,
    amount: &str,
) -> Result<Option<LaunchQuote>, String> {
    match launchpad {
        "pump" => quote_pump_launch(rpc_url, mode, amount).await,
        "bonk" => quote_bonk_launch(rpc_url, quote_asset, launch_mode, mode, amount).await,
        "bagsapp" => quote_bags_launch(rpc_url, launch_mode, mode, amount).await,
        _ => Ok(None),
    }
}

async fn maybe_wrap_launch_dev_buy_artifacts(
    rpc_url: &str,
    config: &NormalizedConfig,
    wallet_secret: &[u8],
    artifacts: Option<NativeLaunchArtifacts>,
) -> Result<Option<NativeLaunchArtifacts>, String> {
    let Some(artifacts) = artifacts else {
        return Ok(None);
    };
    let Some(dev_buy) = config.devBuy.as_ref() else {
        return Ok(Some(artifacts));
    };
    let _ = (rpc_url, wallet_secret, dev_buy);
    // Launch dev-buy transactions intentionally stay native so creation
    // flows keep the venue's canonical transaction shape.
    Ok(Some(artifacts))
}

pub async fn maybe_wrap_launch_dev_buy_transaction(
    rpc_url: &str,
    config: &NormalizedConfig,
    wallet_secret: &[u8],
    source: CompiledTransaction,
) -> Result<CompiledTransaction, String> {
    let Some(dev_buy) = config.devBuy.as_ref() else {
        return Ok(source);
    };
    let _ = (rpc_url, config, wallet_secret, dev_buy);
    // Launch dev-buy transactions intentionally stay native so creation
    // flows keep the venue's canonical transaction shape.
    Ok(source)
}

fn launch_dev_buy_wrap_request(
    config: &NormalizedConfig,
    dev_buy: &crate::config::NormalizedDevBuy,
) -> Result<LaunchdeckWrapRequest, String> {
    let gross_sol_in_lamports = match dev_buy.mode.trim().to_ascii_lowercase().as_str() {
        "sol" => match parse_sol_amount_to_lamports(&dev_buy.amount) {
            Ok(value) if value > 0 => value,
            Ok(_) => {
                return Err(
                    "LaunchDeck wrapper dev-buy amount must be greater than zero".to_string(),
                );
            }
            Err(error) => {
                return Err(format!(
                    "LaunchDeck wrapper dev-buy amount parse failed for launchpad={}: {}",
                    config.launchpad, error
                ));
            }
        },
        "tokens" if config.launchpad != "bagsapp" => 0,
        "tokens" => {
            return Err(
                "Bags creation dev-buy token mode is not atomic-safe in the native API path; Bags currently requires initialBuyLamports."
                    .to_string(),
            );
        }
        other => {
            return Err(format!(
                "LaunchDeck wrapper dev-buy mode {other} is not supported for launchpad={}",
                config.launchpad
            ));
        }
    };
    Ok(LaunchdeckWrapRequest {
        route_kind: WrapperRouteKind::SolIn,
        fee_bps: config.wrapperDefaultFeeBps,
        gross_sol_in_lamports,
        infer_gross_sol_in_from_inner: dev_buy.mode.trim().eq_ignore_ascii_case("tokens"),
        min_net_output: 0,
        select_first_allowlisted_venue_instruction: false,
        select_last_allowlisted_venue_instruction: true,
    })
}

async fn maybe_wrap_follow_transaction(
    rpc_url: &str,
    wallet_secret: &[u8],
    launchpad: &str,
    source: CompiledTransaction,
    request: LaunchdeckWrapRequest,
    allow_native_passthrough: bool,
) -> Result<CompiledTransaction, String> {
    let payer = match Keypair::try_from(wallet_secret) {
        Ok(payer) => payer,
        Err(error) => {
            return Err(format!(
                "LaunchDeck wrapper wallet decode failed for launchpad={} label={}: {}",
                launchpad, source.label, error
            ));
        }
    };
    let lookup_tables = match load_shared_lookup_tables(rpc_url).await {
        Ok(tables) => tables,
        Err(error) => {
            return Err(format!(
                "LaunchDeck wrapper ALT load failed for launchpad={} label={}: {}",
                launchpad, source.label, error
            ));
        }
    };
    let wrap_result = wrap_compiled_transaction(&source, &payer, &lookup_tables, request);
    let wrap_result = match wrap_result {
        Err(error) if wrapper_alt_cache_miss_error(&error) => {
            let refreshed_lookup_tables = refresh_shared_lookup_tables(rpc_url).await?;
            wrap_compiled_transaction(&source, &payer, &refreshed_lookup_tables, request)
        }
        other => other,
    };
    match wrap_result {
        Ok(wrapped) => {
            let gross_label = if request.infer_gross_sol_in_from_inner {
                "inferred".to_string()
            } else {
                request.gross_sol_in_lamports.to_string()
            };
            let fee_estimate_label = if request.infer_gross_sol_in_from_inner {
                "inferred".to_string()
            } else {
                estimate_sol_in_fee_lamports(request.gross_sol_in_lamports, request.fee_bps)
                    .to_string()
            };
            eprintln!(
                "[launchdeck-engine][wrapper-wrap] launchpad={} label={} route={:?} gross_sol_in={} fee_bps={} fee_estimate={} format={}",
                launchpad,
                wrapped.label,
                request.route_kind,
                gross_label,
                request.fee_bps,
                fee_estimate_label,
                wrapped.format
            );
            Ok(wrapped)
        }
        Err(error) => {
            if error.contains("transaction is already wrapped")
                && bonk_allows_already_wrapped_passthrough(launchpad, &source.label)
            {
                eprintln!(
                    "[launchdeck-engine][wrapper-wrap] passthrough=already_wrapped launchpad={} label={}",
                    launchpad, source.label
                );
                return Ok(source);
            }
            if allow_native_passthrough
                && let Some(reason) = wrapper_passthrough_reason(launchpad, &error)
            {
                eprintln!(
                    "[launchdeck-engine][wrapper-wrap] setup-passthrough=native launchpad={} label={} reason={}",
                    launchpad, source.label, reason
                );
                return Ok(source);
            }
            eprintln!(
                "[launchdeck-engine][wrapper-wrap] fail_closed launchpad={} label={} route={:?} reason=wrap_failed error={}",
                launchpad, source.label, request.route_kind, error
            );
            Err(format!(
                "LaunchDeck wrapper failed for launchpad={} label={}: {}",
                launchpad, source.label, error
            ))
        }
    }
}

fn wrapper_alt_cache_miss_error(error: &str) -> bool {
    error.contains("ALT ") && error.contains(" index ") && error.contains(" missing")
}

fn bonk_allows_already_wrapped_passthrough(launchpad: &str, label: &str) -> bool {
    launchpad == "bonk" && matches!(label, "follow-buy" | "follow-buy-atomic" | "follow-sell")
}

fn wrapper_passthrough_reason<'a>(launchpad: &str, error: &str) -> Option<&'a str> {
    if error.contains("no allowlisted venue instruction found") {
        return Some("no_venue_instruction");
    }
    let _ = launchpad;
    None
}

fn wrapper_feeable_buy_execution(
    launchpad: &str,
    quote_asset: &str,
    execution: &NormalizedExecution,
) -> NormalizedExecution {
    let mut normalized = execution.clone();
    let original_policy = execution
        .buyFundingPolicy
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .replace(' ', "_");
    normalized.buyFundingPolicy = if launchpad == "bonk"
        && quote_asset.eq_ignore_ascii_case("usd1")
        && original_policy == "usd1_only"
    {
        // Preserve USD1 amount semantics, but source that USD1 via a SOL top-up
        // so the wrapper can collect the SOL/WSOL fee on the conversion leg.
        "usd1_via_sol".to_string()
    } else {
        "sol_only".to_string()
    };
    normalized
}

fn wrapper_feeable_sell_execution(execution: &NormalizedExecution) -> NormalizedExecution {
    let mut normalized = execution.clone();
    normalized.sellSettlementPolicy = "always_to_sol".to_string();
    normalized.sellSettlementAsset = "sol".to_string();
    normalized
}

async fn maybe_wrap_follow_transactions(
    rpc_url: &str,
    wallet_secret: &[u8],
    launchpad: &str,
    sources: Vec<CompiledTransaction>,
    request: LaunchdeckWrapRequest,
    primary_tx_index: Option<usize>,
    wrapper_tx_index: Option<usize>,
) -> Result<Vec<CompiledTransaction>, String> {
    let mut wrapped = Vec::with_capacity(sources.len());
    let wrapper_tx_index = wrapper_tx_index.or(primary_tx_index);
    let mut wrapped_any = false;
    for (index, source) in sources.into_iter().enumerate() {
        if wrapper_tx_index != Some(index) {
            wrapped.push(source);
            continue;
        }
        let wrapped_source = maybe_wrap_follow_transaction(
            rpc_url,
            wallet_secret,
            launchpad,
            source,
            request,
            false,
        )
        .await?;
        wrapped_any = true;
        wrapped.push(wrapped_source);
    }
    if wrapper_tx_index.is_some() && !wrapped_any {
        return Err(format!(
            "LaunchDeck wrapper selected missing transaction index launchpad={launchpad}"
        ));
    }
    Ok(wrapped)
}

pub async fn wrap_follow_buy_transaction_for_launchpad(
    rpc_url: &str,
    wallet_secret: &[u8],
    launchpad: &str,
    source: CompiledTransaction,
    buy_amount_sol: &str,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let buy_gross_lamports = parse_sol_amount_to_lamports(buy_amount_sol)?;
    maybe_wrap_follow_transaction(
        rpc_url,
        wallet_secret,
        launchpad,
        source,
        LaunchdeckWrapRequest {
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: wrapper_fee_bps,
            gross_sol_in_lamports: buy_gross_lamports,
            infer_gross_sol_in_from_inner: false,
            min_net_output: 0,
            select_first_allowlisted_venue_instruction: false,
            select_last_allowlisted_venue_instruction: false,
        },
        false,
    )
    .await
}

pub async fn compile_follow_buy_for_launchpad(
    request: FollowBuyCompileRequest<'_>,
) -> Result<FollowBuyCompiledTransactions, String> {
    let buy_gross_lamports = parse_sol_amount_to_lamports(request.buy_amount_sol)?;
    let execution =
        wrapper_feeable_buy_execution(request.launchpad, request.quote_asset, request.execution);
    let wrap_request = LaunchdeckWrapRequest {
        route_kind: WrapperRouteKind::SolIn,
        fee_bps: request.wrapper_fee_bps,
        gross_sol_in_lamports: buy_gross_lamports,
        infer_gross_sol_in_from_inner: false,
        min_net_output: 0,
        select_first_allowlisted_venue_instruction: false,
        select_last_allowlisted_venue_instruction: false,
    };
    match request.launchpad {
        "pump" => {
            let tx = compile_pump_follow_buy(
                request.rpc_url,
                &execution,
                request.token_mayhem_mode,
                request.jito_tip_account,
                request.wallet_secret,
                request.mint,
                request.launch_creator,
                request.buy_amount_sol,
                request.prefer_post_setup_creator_vault,
                request.wrapper_fee_bps,
            )
            .await?;
            Ok(FollowBuyCompiledTransactions {
                transactions: vec![
                    maybe_wrap_follow_transaction(
                        request.rpc_url,
                        request.wallet_secret,
                        request.launchpad,
                        tx,
                        wrap_request,
                        false,
                    )
                    .await?,
                ],
                primary_tx_index: 0,
                requires_ordered_execution: false,
            })
        }
        "bonk" => {
            let compiled = compile_bonk_follow_buy_with_metadata(
                request.rpc_url,
                request.quote_asset,
                &execution,
                request.jito_tip_account,
                request.wallet_secret,
                request.mint,
                request.buy_amount_sol,
                request.allow_ata_creation,
                request.bonk_pool_context,
                request.bonk_pool_id,
                request.bonk_usd1_route_setup,
                request.wrapper_fee_bps,
            )
            .await?;
            let mut wrap_request = wrap_request;
            if let Some(gross_sol_in_lamports) = compiled.wrapper_gross_sol_in_lamports {
                wrap_request.gross_sol_in_lamports = gross_sol_in_lamports;
            }
            Ok(FollowBuyCompiledTransactions {
                transactions: maybe_wrap_follow_transactions(
                    request.rpc_url,
                    request.wallet_secret,
                    request.launchpad,
                    compiled.transactions,
                    wrap_request,
                    Some(compiled.primary_tx_index),
                    compiled.wrapper_tx_index,
                )
                .await?,
                primary_tx_index: compiled.primary_tx_index,
                requires_ordered_execution: compiled.requires_ordered_execution,
            })
        }
        "bagsapp" => {
            let tx = compile_bags_follow_buy(
                request.rpc_url,
                &execution,
                request.token_mayhem_mode,
                request.jito_tip_account,
                request.wallet_secret,
                request.mint,
                request.launch_creator,
                request.buy_amount_sol,
                request.bags_launch,
                request.bags_follow_buy_context,
            )
            .await?;
            Ok(FollowBuyCompiledTransactions {
                transactions: vec![
                    maybe_wrap_follow_transaction(
                        request.rpc_url,
                        request.wallet_secret,
                        request.launchpad,
                        tx,
                        wrap_request,
                        false,
                    )
                    .await?,
                ],
                primary_tx_index: 0,
                requires_ordered_execution: false,
            })
        }
        other => Err(format!(
            "Unsupported launchpad for follow buy compilation: {other}"
        )),
    }
}

pub async fn compile_atomic_follow_buy_for_launchpad(
    launchpad: &str,
    launch_mode: &str,
    quote_asset: &str,
    rpc_url: &str,
    execution: &NormalizedExecution,
    token_mayhem_mode: bool,
    jito_tip_account: &str,
    wallet_secret: &[u8],
    mint: &str,
    launch_creator: &str,
    buy_amount_sol: &str,
    allow_ata_creation: bool,
    predicted_creator_dev_buy_token_amount: Option<u64>,
    predicted_creator_dev_buy_quote_amount: Option<u64>,
    pump_cashback_enabled_override: Option<bool>,
    bags_launch: Option<&BagsLaunchMetadata>,
    wrapper_fee_bps: u16,
) -> Result<CompiledTransaction, String> {
    let execution = wrapper_feeable_buy_execution(launchpad, quote_asset, execution);
    let (compiled, wrapper_gross_sol_in_lamports) = match launchpad {
        "pump" => {
            let tx = compile_atomic_pump_follow_buy(
                rpc_url,
                &execution,
                token_mayhem_mode,
                jito_tip_account,
                wallet_secret,
                mint,
                launch_creator,
                buy_amount_sol,
                predicted_creator_dev_buy_token_amount,
                pump_cashback_enabled_override,
                wrapper_fee_bps,
            )
            .await?;
            (tx, parse_sol_amount_to_lamports(buy_amount_sol)?)
        }
        "bonk" => {
            let result = compile_atomic_bonk_follow_buy_with_metadata(
                rpc_url,
                launch_mode,
                quote_asset,
                &execution,
                token_mayhem_mode,
                jito_tip_account,
                wallet_secret,
                mint,
                launch_creator,
                buy_amount_sol,
                allow_ata_creation,
                predicted_creator_dev_buy_quote_amount,
                wrapper_fee_bps,
            )
            .await?;
            (
                result.transaction,
                result
                    .wrapper_gross_sol_in_lamports
                    .unwrap_or(parse_sol_amount_to_lamports(buy_amount_sol)?),
            )
        }
        "bagsapp" => {
            let tx = compile_atomic_bags_follow_buy(
                rpc_url,
                launch_mode,
                quote_asset,
                &execution,
                token_mayhem_mode,
                jito_tip_account,
                wallet_secret,
                mint,
                launch_creator,
                buy_amount_sol,
                bags_launch,
            )
            .await?;
            (tx, parse_sol_amount_to_lamports(buy_amount_sol)?)
        }
        other => {
            return Err(format!(
                "Unsupported launchpad for same-time sniper buys: {other}"
            ));
        }
    };
    Ok(maybe_wrap_follow_transaction(
        rpc_url,
        wallet_secret,
        launchpad,
        compiled,
        LaunchdeckWrapRequest {
            route_kind: WrapperRouteKind::SolIn,
            fee_bps: wrapper_fee_bps,
            gross_sol_in_lamports: wrapper_gross_sol_in_lamports,
            infer_gross_sol_in_from_inner: false,
            min_net_output: 0,
            select_first_allowlisted_venue_instruction: matches!(launchpad, "bonk" | "bagsapp"),
            select_last_allowlisted_venue_instruction: false,
        },
        false,
    )
    .await?)
}

pub async fn compile_follow_sell_for_launchpad(
    request: FollowSellCompileRequest<'_>,
) -> Result<Option<CompiledTransaction>, String> {
    let execution = wrapper_feeable_sell_execution(request.execution);
    let compiled = match request.launchpad {
        "pump" => {
            compile_pump_follow_sell_with_token_amount(
                request.rpc_url,
                &execution,
                request.token_mayhem_mode,
                request.jito_tip_account,
                request.wallet_secret,
                request.mint,
                request.launch_creator,
                request.sell_percent,
                request.prefer_post_setup_creator_vault,
                request.token_amount_override,
                request.pump_cashback_enabled_override,
            )
            .await
        }
        "bonk" => {
            compile_bonk_follow_sell_with_token_amount_and_settlement(
                request.rpc_url,
                request.quote_asset,
                &execution,
                request.jito_tip_account,
                request.wallet_secret,
                request.mint,
                request.sell_percent,
                request.token_amount_override,
                request.bonk_pool_id,
                request.bonk_launch_mode,
                request.bonk_launch_creator,
                &execution.sellSettlementAsset,
                request.wrapper_fee_bps,
            )
            .await
        }
        "bagsapp" => {
            compile_bags_follow_sell(
                request.rpc_url,
                &execution,
                request.token_mayhem_mode,
                request.jito_tip_account,
                request.wallet_secret,
                request.mint,
                request.launch_creator,
                request.sell_percent,
                request.prefer_post_setup_creator_vault,
                request.bags_launch,
            )
            .await
        }
        other => Err(format!(
            "Unsupported launchpad for follow sell compilation: {other}"
        )),
    }?;
    Ok(match compiled {
        Some(tx) => Some(
            maybe_wrap_follow_transaction(
                request.rpc_url,
                request.wallet_secret,
                request.launchpad,
                tx,
                LaunchdeckWrapRequest {
                    route_kind: WrapperRouteKind::SolOut,
                    fee_bps: request.wrapper_fee_bps,
                    gross_sol_in_lamports: 0,
                    infer_gross_sol_in_from_inner: false,
                    min_net_output: 0,
                    select_first_allowlisted_venue_instruction: false,
                    select_last_allowlisted_venue_instruction: matches!(request.launchpad, "bonk"),
                },
                false,
            )
            .await?,
        ),
        None => None,
    })
}

pub fn derive_follow_owner_token_account_for_launchpad(
    launchpad: &str,
    owner: &str,
    mint: &str,
) -> Result<String, String> {
    let owner_pubkey =
        Pubkey::from_str(owner).map_err(|error| format!("Invalid owner public key: {error}"))?;
    let mint_pubkey =
        Pubkey::from_str(mint).map_err(|error| format!("Invalid mint public key: {error}"))?;
    match launchpad {
        "pump" => {
            Ok(derive_pump_follow_owner_token_account(&owner_pubkey, &mint_pubkey)?.to_string())
        }
        "bonk" => {
            Ok(derive_bonk_follow_owner_token_account(&owner_pubkey, &mint_pubkey).to_string())
        }
        "bagsapp" => {
            Ok(derive_bags_follow_owner_token_account(&owner_pubkey, &mint_pubkey)?.to_string())
        }
        other => Err(format!(
            "Unsupported launchpad for follow owner token account derivation: {other}"
        )),
    }
}

pub async fn fetch_market_snapshot_for_launchpad(
    launchpad: &str,
    rpc_url: &str,
    mint: &str,
    quote_asset: &str,
    bags_launch: Option<&BagsLaunchMetadata>,
) -> Result<Option<LaunchpadMarketSnapshot>, String> {
    match launchpad {
        "pump" => fetch_pump_market_snapshot(rpc_url, mint)
            .await
            .map(|snapshot| Some(LaunchpadMarketSnapshot::Pump(snapshot))),
        "bonk" => fetch_bonk_market_snapshot(rpc_url, mint, quote_asset)
            .await
            .map(|snapshot| Some(LaunchpadMarketSnapshot::Bonk(snapshot))),
        "bagsapp" => fetch_bags_market_snapshot(rpc_url, mint, bags_launch)
            .await
            .map(|snapshot| Some(LaunchpadMarketSnapshot::Bags(snapshot))),
        _ => Ok(None),
    }
}

pub async fn detect_import_context_for_launchpad(
    launchpad: &str,
    rpc_url: &str,
    mint: &str,
    quote_asset: Option<&str>,
) -> Result<Option<LaunchpadImportContext>, String> {
    match launchpad {
        "bonk" => {
            let context =
                if let Some(quote_asset) = quote_asset.filter(|value| !value.trim().is_empty()) {
                    detect_bonk_import_context_with_quote_asset(rpc_url, mint, quote_asset).await?
                } else {
                    detect_bonk_import_context(rpc_url, mint).await?
                };
            Ok(context.map(LaunchpadImportContext::Bonk))
        }
        "bagsapp" => detect_bags_import_context(rpc_url, mint)
            .await
            .map(|context| context.map(LaunchpadImportContext::Bags)),
        "pump" => Ok(None),
        other => Err(format!(
            "Unsupported launchpad for import context detection: {other}"
        )),
    }
}

pub async fn lookup_fee_recipient_for_launchpad(
    launchpad: &str,
    rpc_url: &str,
    provider: &str,
    username: &str,
    github_user_id: &str,
) -> Result<Option<BagsFeeRecipientLookupResponse>, String> {
    match launchpad {
        "bagsapp" => lookup_bags_fee_recipient(rpc_url, provider, username, github_user_id)
            .await
            .map(Some),
        "pump" | "bonk" => Ok(None),
        other => Err(format!(
            "Unsupported launchpad for fee recipient lookup: {other}"
        )),
    }
}

pub async fn predict_dev_buy_token_amount_for_launchpad(
    rpc_url: &str,
    config: &NormalizedConfig,
) -> Result<Option<u64>, String> {
    match config.launchpad.as_str() {
        "pump" => predict_pump_dev_buy_token_amount(rpc_url, config).await,
        "bonk" => predict_bonk_dev_buy_token_amount(rpc_url, config).await,
        "bagsapp" => Ok(None),
        other => Err(format!(
            "Unsupported launchpad for predicted dev buy tokens: {other}"
        )),
    }
}

pub async fn derive_canonical_pool_id_for_launchpad(
    _rpc_url: &str,
    launchpad: &str,
    quote_asset: &str,
    mint: &str,
) -> Result<Option<String>, String> {
    match launchpad {
        "bonk" => derive_bonk_canonical_pool_id(quote_asset, mint)
            .await
            .map(Some),
        "pump" | "bagsapp" => Ok(None),
        other => Err(format!(
            "Unsupported launchpad for canonical pool derivation: {other}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RawConfig, normalize_raw_config};
    use serde_json::json;

    fn dummy_compiled_transaction(label: &str, format: &str) -> CompiledTransaction {
        CompiledTransaction {
            label: label.to_string(),
            format: format.to_string(),
            blockhash: "11111111111111111111111111111111".to_string(),
            lastValidBlockHeight: 0,
            serializedBase64: String::new(),
            signature: None,
            lookupTablesUsed: vec![],
            computeUnitLimit: None,
            computeUnitPriceMicroLamports: None,
            inlineTipLamports: None,
            inlineTipAccount: None,
        }
    }

    fn dummy_native_launch_artifacts() -> NativeLaunchArtifacts {
        NativeLaunchArtifacts {
            compiled_transactions: vec![dummy_compiled_transaction("launch", "v0-alt")],
            creation_transactions: vec![dummy_compiled_transaction("launch", "v0-alt")],
            deferred_setup_transactions: vec![dummy_compiled_transaction("setup", "v0-alt")],
            setup_bundles: vec![],
            setup_transactions: vec![],
            bags_launch_follow: None,
            bags_config_key: String::new(),
            bags_metadata_uri: String::new(),
            bags_fee_estimate: None,
            bags_prepare_launch_ms: None,
            bags_metadata_upload_ms: None,
            bags_fee_recipient_resolve_ms: None,
            report: json!({}),
            text: String::new(),
            compile_timings: NativeCompileTimings::default(),
            mint: "mint".to_string(),
            launch_creator: "creator".to_string(),
            vanity_reservation: None,
        }
    }

    fn pump_config_with_dev_buy() -> crate::config::NormalizedConfig {
        let mut raw = RawConfig {
            mode: "regular".to_string(),
            launchpad: "pump".to_string(),
            ..RawConfig::default()
        };
        raw.token.name = "LaunchDeck".to_string();
        raw.token.symbol = "LDECK".to_string();
        raw.token.uri = "ipfs://fixture".to_string();
        raw.tx.computeUnitPriceMicroLamports = Some(json!(1));
        raw.tx.jitoTipLamports = Some(json!(200_000));
        raw.tx.jitoTipAccount = "4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE".to_string();
        raw.devBuy = Some(crate::config::RawDevBuy {
            mode: "sol".to_string(),
            amount: "0.1".to_string(),
        });
        normalize_raw_config(raw).expect("normalized pump config")
    }

    fn bags_config_with_dev_buy() -> crate::config::NormalizedConfig {
        let mut raw = RawConfig {
            mode: "bags-2-2".to_string(),
            launchpad: "bagsapp".to_string(),
            ..RawConfig::default()
        };
        raw.token.name = "LaunchDeck".to_string();
        raw.token.symbol = "LDECK".to_string();
        raw.token.uri = "ipfs://fixture".to_string();
        raw.tx.computeUnitPriceMicroLamports = Some(json!(1));
        raw.tx.jitoTipLamports = Some(json!(200_000));
        raw.tx.jitoTipAccount = "4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE".to_string();
        raw.devBuy = Some(crate::config::RawDevBuy {
            mode: "sol".to_string(),
            amount: "0.1".to_string(),
        });
        normalize_raw_config(raw).expect("normalized bags config")
    }

    #[tokio::test]
    async fn pump_launch_dev_buy_artifacts_are_not_wrapper_wrapped() {
        let artifacts = dummy_native_launch_artifacts();
        let normalized = pump_config_with_dev_buy();

        let wrapped = maybe_wrap_launch_dev_buy_artifacts(
            "http://127.0.0.1:1",
            &normalized,
            &[],
            Some(artifacts),
        )
        .await
        .expect("pump passthrough")
        .expect("artifacts");

        assert_eq!(wrapped.compiled_transactions[0].format, "v0-alt");
        assert_eq!(wrapped.creation_transactions[0].format, "v0-alt");
        assert_eq!(wrapped.deferred_setup_transactions[0].format, "v0-alt");
    }

    #[tokio::test]
    async fn bags_prelaunch_setup_artifacts_are_not_wrapper_wrapped() {
        let mut artifacts = dummy_native_launch_artifacts();
        artifacts.compiled_transactions =
            vec![dummy_compiled_transaction("bags-config-direct-1", "v0-alt")];
        artifacts.creation_transactions = artifacts.compiled_transactions.clone();
        artifacts.setup_transactions = artifacts.compiled_transactions.clone();
        let normalized = bags_config_with_dev_buy();

        let wrapped = maybe_wrap_launch_dev_buy_artifacts(
            "http://127.0.0.1:1",
            &normalized,
            &[],
            Some(artifacts),
        )
        .await
        .expect("bags setup passthrough")
        .expect("artifacts");

        assert_eq!(
            wrapped.compiled_transactions[0].label,
            "bags-config-direct-1"
        );
        assert_eq!(wrapped.compiled_transactions[0].format, "v0-alt");
        assert_eq!(wrapped.setup_transactions[0].format, "v0-alt");
    }

    #[tokio::test]
    async fn pump_launch_dev_buy_single_transaction_is_not_wrapper_wrapped() {
        let normalized = pump_config_with_dev_buy();
        let source = dummy_compiled_transaction("launch", "v0-alt");

        let wrapped =
            maybe_wrap_launch_dev_buy_transaction("http://127.0.0.1:1", &normalized, &[], source)
                .await
                .expect("pump passthrough");

        assert_eq!(wrapped.format, "v0-alt");
    }

    #[test]
    fn wrapper_passthrough_is_limited_to_setup_without_venue_instruction() {
        assert_eq!(
            wrapper_passthrough_reason(
                "bagsapp",
                "Failed to sign LaunchDeck wrapper tx: not enough signers"
            ),
            None
        );
        assert_eq!(
            wrapper_passthrough_reason(
                "bonk",
                "no allowlisted venue instruction found inside the tx"
            ),
            Some("no_venue_instruction")
        );
    }

    #[test]
    fn bonk_already_wrapped_passthrough_accepts_usd1_follow_routes() {
        assert!(bonk_allows_already_wrapped_passthrough(
            "bonk",
            "follow-buy"
        ));
        assert!(bonk_allows_already_wrapped_passthrough(
            "bonk",
            "follow-buy-atomic"
        ));
        assert!(bonk_allows_already_wrapped_passthrough(
            "bonk",
            "follow-sell"
        ));
        assert!(!bonk_allows_already_wrapped_passthrough(
            "pump",
            "follow-buy"
        ));
        assert!(!bonk_allows_already_wrapped_passthrough("bonk", "launch"));
    }

    #[test]
    fn bonk_usd1_only_wrapper_buy_preserves_quote_amount_semantics() {
        let mut execution = pump_config_with_dev_buy().execution;
        execution.buyFundingPolicy = "usd1_only".to_string();

        let normalized = wrapper_feeable_buy_execution("bonk", "usd1", &execution);

        assert_eq!(normalized.buyFundingPolicy, "usd1_via_sol");
    }

    #[tokio::test]
    async fn bonk_canonical_pool_helper_ignores_live_import_context() {
        let canonical = derive_canonical_pool_id_for_launchpad(
            "http://127.0.0.1:1",
            "bonk",
            "sol",
            "So11111111111111111111111111111111111111112",
        )
        .await
        .expect("canonical pool id");
        let direct =
            derive_bonk_canonical_pool_id("sol", "So11111111111111111111111111111111111111112")
                .await
                .expect("direct canonical pool id");

        assert_eq!(canonical, Some(direct));
    }

    #[test]
    fn capabilities_report_bags_helper_reachability() {
        let caps = launchpad_runtime_capabilities("bagsapp").expect("bagsapp runtime capabilities");
        assert!(caps.startupWarm);
        assert!(!caps.helperBackedWarm);
        assert!(!caps.helperBackedFollow);
        assert!(!caps.helperBackedCompile);
        assert!(caps.prelaunchSetup);
    }

    #[test]
    fn action_backend_reports_expected_owners() {
        assert_eq!(
            launchpad_action_backend("pump", "quote"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bonk", "quote"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bonk", "market-snapshot"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bonk", "import-context"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bonk", "follow-buy"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bonk", "follow-sell"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bonk", "build-launch"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bagsapp", "startup-warm"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bagsapp", "quote"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bagsapp", "market-snapshot"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bagsapp", "import-context"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bagsapp", "fee-recipient-lookup"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bagsapp", "follow-buy"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bagsapp", "follow-sell"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bagsapp", "prepare-launch"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_backend("bagsapp", "build-launch"),
            Some("rust-native")
        );
        assert_eq!(
            launchpad_action_rollout_state("bonk", "quote"),
            Some("rust-only")
        );
        assert_eq!(
            launchpad_action_rollout_state("bonk", "follow-buy"),
            Some("rust-only")
        );
        assert_eq!(
            launchpad_action_rollout_state("bonk", "build-launch"),
            Some("rust-only")
        );
        assert_eq!(
            launchpad_action_rollout_state("bagsapp", "fee-recipient-lookup"),
            Some("rust-only")
        );
    }

    #[test]
    fn bags_launch_metadata_is_preserved_without_deferred_setup() {
        let artifacts = NativeBagsArtifacts {
            compiled_transactions: vec![],
            report: json!({}),
            text: String::new(),
            compile_timings: NativeCompileTimings::default(),
            mint: "mint".to_string(),
            launch_creator: "creator".to_string(),
            config_key: "config".to_string(),
            metadata_uri: "uri".to_string(),
            migration_fee_option: Some(7),
            expected_migration_family: "damm-v2".to_string(),
            expected_damm_config_key: "damm-config".to_string(),
            expected_damm_derivation_mode: "canonical".to_string(),
            pre_migration_dbc_pool_address: "dbc-pool".to_string(),
            setup_bundles: vec![],
            setup_transactions: vec![],
            fee_estimate: BagsFeeEstimateSnapshot {
                helius: json!({}),
                jito: json!({}),
                setupJitoTipLamports: 0,
                setupJitoTipSource: String::new(),
                setupJitoTipPercentile: String::new(),
                setupJitoTipCapLamports: 0,
                setupJitoTipMinLamports: 0,
                warnings: vec![],
            },
            prepare_launch_ms: None,
            metadata_upload_ms: None,
            fee_recipient_resolve_ms: None,
        };
        let native: NativeLaunchArtifacts = artifacts.into();
        let bags_launch = native
            .bags_launch_follow
            .expect("bags launch metadata should always be present");
        assert_eq!(bags_launch.configKey, "config");
        assert_eq!(bags_launch.expectedMigrationFamily, "damm-v2");
        assert_eq!(bags_launch.preMigrationDbcPoolAddress, "dbc-pool");
    }

    #[test]
    fn launchpad_market_snapshot_parses_helper_string_market_caps() {
        let bonk = LaunchpadMarketSnapshot::Bonk(BonkMarketSnapshot {
            mint: "mint".to_string(),
            creator: "creator".to_string(),
            virtualTokenReserves: "0".to_string(),
            virtualSolReserves: "0".to_string(),
            realTokenReserves: "0".to_string(),
            realSolReserves: "0".to_string(),
            tokenTotalSupply: "0".to_string(),
            complete: false,
            marketCapLamports: "42".to_string(),
            marketCapSol: "0.000000042".to_string(),
            quoteAsset: "sol".to_string(),
            quoteAssetLabel: "SOL".to_string(),
        });
        assert_eq!(bonk.market_cap_lamports_u64().expect("bonk market cap"), 42);

        let bags = LaunchpadMarketSnapshot::Bags(BagsMarketSnapshot {
            mint: "mint".to_string(),
            creator: "creator".to_string(),
            virtualTokenReserves: "0".to_string(),
            virtualSolReserves: "0".to_string(),
            realTokenReserves: "0".to_string(),
            realSolReserves: "0".to_string(),
            tokenTotalSupply: "0".to_string(),
            complete: false,
            quoteAsset: "sol".to_string(),
            quoteAssetLabel: "SOL".to_string(),
            marketCapLamports: "84".to_string(),
            marketCapSol: "0.000000084".to_string(),
        });
        assert_eq!(bags.market_cap_lamports_u64().expect("bags market cap"), 84);
    }
}
