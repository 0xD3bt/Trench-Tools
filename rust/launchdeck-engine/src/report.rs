#![allow(non_snake_case, dead_code)]

use serde::Serialize;

use crate::{
    config::{NormalizedConfig, NormalizedRecipient},
    transport::{execution_class, resolved_provider},
};

const DEFAULT_LOOKUP_TABLES: [&str; 1] = ["AXVvmhWaaPtV52jqYuTNqp1xRrkbxhfJfeHQKxq5cbvZ"];

#[derive(Debug, Clone, Serialize)]
pub struct InstructionSummary {
    pub index: usize,
    pub programId: String,
    pub keyCount: usize,
    pub writableKeys: usize,
    pub signerKeys: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeeSettings {
    pub computeUnitLimit: Option<i64>,
    pub computeUnitPriceMicroLamports: Option<i64>,
    pub jitoTipLamports: i64,
    pub jitoTipAccount: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionSummary {
    pub label: String,
    pub instructionSummary: Vec<InstructionSummary>,
    pub legacyLength: Option<usize>,
    pub v0Length: Option<usize>,
    pub v0AltLength: Option<usize>,
    pub legacyError: Option<String>,
    pub v0Error: Option<String>,
    pub v0AltError: Option<String>,
    pub lookupTablesUsed: Vec<String>,
    pub fitsWithAlts: bool,
    pub exceedsPacketLimit: bool,
    pub feeSettings: FeeSettings,
    pub base64: Option<serde_json::Value>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionItem {
    pub label: String,
    pub format: String,
    pub err: Option<String>,
    pub unitsConsumed: Option<i64>,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentItem {
    pub label: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionSummary {
    pub provider: String,
    pub resolvedProvider: String,
    pub policy: String,
    pub executionClass: String,
    pub autoGas: bool,
    pub launchAutoMode: String,
    pub activePresetId: String,
    pub buyProvider: String,
    pub buyPolicy: String,
    pub sellProvider: String,
    pub sellPolicy: String,
    pub buyAutoGas: bool,
    pub buyAutoMode: String,
    pub requestedSummary: String,
    pub txFormat: String,
    pub commitment: String,
    pub skipPreflight: bool,
    pub simulation: Vec<ExecutionItem>,
    pub sent: Vec<SentItem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LaunchReport {
    pub builtAt: String,
    pub configPath: Option<String>,
    pub mode: String,
    pub launchpad: String,
    pub rpcUrl: String,
    pub creator: String,
    pub agentAuthority: Option<String>,
    pub mint: String,
    pub cashbackEnabled: bool,
    pub agentEnabled: bool,
    pub feeSharingStatus: String,
    pub creatorFeeReceiver: String,
    pub buybackBps: Option<i64>,
    pub devBuyDescription: String,
    pub requestedLookupTables: Vec<String>,
    pub loadedLookupTables: Vec<String>,
    pub bundleJitoTip: bool,
    pub transactions: Vec<TransactionSummary>,
    pub execution: ExecutionSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outPath: Option<String>,
}

fn summarize_recipients(
    entries: &[NormalizedRecipient],
    fallback_wallet: &str,
    buyback_bps: Option<i64>,
) -> String {
    if !entries.is_empty() {
        return entries
            .iter()
            .map(|entry| {
                let share = entry.shareBps as f64 / 100.0;
                match entry.r#type.as_deref() {
                    Some("agent") => format!("agent buyback ({share}%)"),
                    _ if !entry.githubUsername.is_empty() => {
                        format!("@{} ({share}%)", entry.githubUsername)
                    }
                    _ if !entry.address.is_empty() => {
                        format!("{} ({share}%)", short_address(&entry.address, 4, 4))
                    }
                    _ => format!("unknown ({share}%)"),
                }
            })
            .collect::<Vec<_>>()
            .join(" + ");
    }
    let buyback = buyback_bps.unwrap_or_default();
    let wallet_share = 10_000 - buyback;
    let mut parts = Vec::new();
    if buyback > 0 {
        parts.push(format!("agent buyback ({}%)", buyback as f64 / 100.0));
    }
    if wallet_share > 0 {
        parts.push(format!(
            "{} ({}%)",
            short_address(fallback_wallet, 4, 4),
            wallet_share as f64 / 100.0
        ));
    }
    parts.join(" + ")
}

fn short_address(value: &str, left: usize, right: usize) -> String {
    if value.is_empty() {
        return "(unknown)".to_string();
    }
    if value.len() <= left + right {
        return value.to_string();
    }
    format!("{}...{}", &value[..left], &value[value.len() - right..])
}

fn parse_lookup_table_addresses(config: &NormalizedConfig) -> Vec<String> {
    let mut values = Vec::new();
    if config.tx.useDefaultLookupTables {
        values.extend(DEFAULT_LOOKUP_TABLES.iter().map(|entry| entry.to_string()));
    }
    values.extend(config.tx.lookupTables.clone());
    let mut deduped = Vec::new();
    for value in values {
        if !deduped.iter().any(|entry| entry == &value) {
            deduped.push(value);
        }
    }
    deduped
}

fn planned_transactions(
    config: &NormalizedConfig,
    bundle_jito_tip: bool,
    lookup_tables: &[String],
) -> Vec<TransactionSummary> {
    let mut transactions = vec![TransactionSummary {
        label: "launch".to_string(),
        instructionSummary: vec![],
        legacyLength: None,
        v0Length: None,
        v0AltLength: None,
        legacyError: None,
        v0Error: None,
        v0AltError: None,
        lookupTablesUsed: lookup_tables.to_vec(),
        fitsWithAlts: false,
        exceedsPacketLimit: false,
        feeSettings: FeeSettings {
            computeUnitLimit: config.tx.computeUnitLimit,
            computeUnitPriceMicroLamports: config.tx.computeUnitPriceMicroLamports,
            jitoTipLamports: if bundle_jito_tip {
                0
            } else {
                config.tx.jitoTipLamports
            },
            jitoTipAccount: if bundle_jito_tip || config.tx.jitoTipAccount.is_empty() {
                None
            } else {
                Some(config.tx.jitoTipAccount.clone())
            },
        },
        base64: None,
        warnings: vec![],
    }];

    let follow_up = match config.mode.as_str() {
        "regular" | "cashback" if config.feeSharing.generateLaterSetup => Some("follow-up"),
        "agent-custom" | "agent-locked" => Some("agent-setup"),
        _ => None,
    };
    if let Some(label) = follow_up {
        transactions.push(TransactionSummary {
            label: label.to_string(),
            instructionSummary: vec![],
            legacyLength: None,
            v0Length: None,
            v0AltLength: None,
            legacyError: None,
            v0Error: None,
            v0AltError: None,
            lookupTablesUsed: lookup_tables.to_vec(),
            fitsWithAlts: false,
            exceedsPacketLimit: false,
            feeSettings: FeeSettings {
                computeUnitLimit: config.tx.computeUnitLimit,
                computeUnitPriceMicroLamports: config.tx.computeUnitPriceMicroLamports,
                jitoTipLamports: 0,
                jitoTipAccount: None,
            },
            base64: None,
            warnings: vec![],
        });
    }
    if bundle_jito_tip {
        transactions.push(TransactionSummary {
            label: "jito-tip".to_string(),
            instructionSummary: vec![],
            legacyLength: None,
            v0Length: None,
            v0AltLength: None,
            legacyError: None,
            v0Error: None,
            v0AltError: None,
            lookupTablesUsed: lookup_tables.to_vec(),
            fitsWithAlts: false,
            exceedsPacketLimit: false,
            feeSettings: FeeSettings {
                computeUnitLimit: None,
                computeUnitPriceMicroLamports: None,
                jitoTipLamports: config.tx.jitoTipLamports,
                jitoTipAccount: if config.tx.jitoTipAccount.is_empty() {
                    None
                } else {
                    Some(config.tx.jitoTipAccount.clone())
                },
            },
            base64: None,
            warnings: vec![],
        });
    }
    transactions
}

fn requested_summary(
    config: &NormalizedConfig,
    resolved_provider: &str,
    execution_class: &str,
) -> String {
    if config.execution.send {
        if config.execution.simulate {
            return format!(
                "simulate + send ({}, provider={}, class={})",
                config.execution.txFormat, resolved_provider, execution_class
            );
        }
        return format!(
            "send ({}, provider={}, class={})",
            config.execution.txFormat, resolved_provider, execution_class
        );
    }
    if config.execution.simulate {
        return format!(
            "simulate ({}, provider={}, class={})",
            config.execution.txFormat, resolved_provider, execution_class
        );
    }
    "build only".to_string()
}

pub fn build_report(
    config: &NormalizedConfig,
    built_at: String,
    rpc_url: String,
    creator: String,
    mint: String,
    agent_authority: Option<String>,
    config_path: Option<String>,
) -> LaunchReport {
    let bundle_jito_tip = config.tx.jitoTipLamports > 0 && config.mode != "agent-unlocked";
    let requested_lookup_tables = parse_lookup_table_addresses(config);
    let transactions = planned_transactions(config, bundle_jito_tip, &requested_lookup_tables);
    let resolved_provider = resolved_provider(&config.execution, transactions.len());
    let execution_class = execution_class(&config.execution, transactions.len());
    let creator_fee_receiver = match config.mode.as_str() {
        "cashback" => "cashback to traders".to_string(),
        "agent-locked" => "agent buyback escrow (locked after launch)".to_string(),
        "agent-custom" => summarize_recipients(
            &config.agent.feeRecipients,
            &creator,
            config.agent.buybackBps,
        ),
        _ if config.creatorFee.mode == "github" && !config.creatorFee.githubUsername.is_empty() => {
            format!("GitHub @{}", config.creatorFee.githubUsername)
        }
        _ if config.creatorFee.mode == "github" && !config.creatorFee.githubUserId.is_empty() => {
            format!("GitHub user {}", config.creatorFee.githubUserId)
        }
        _ if config.creatorFee.mode == "wallet" && !config.creatorFee.address.is_empty() => {
            config.creatorFee.address.clone()
        }
        _ => creator.clone(),
    };
    let fee_sharing_status = match config.mode.as_str() {
        "agent-custom" => "agent custom split bundled post-launch (final)".to_string(),
        "agent-unlocked" => "untouched on launch (configure later once)".to_string(),
        "agent-locked" => "locked to agent escrow".to_string(),
        _ if config.feeSharing.generateLaterSetup => "deferred one-time setup artifact".to_string(),
        _ => "untouched".to_string(),
    };
    let mut warnings = vec![
        "Rust engine owns validation, runtime state, and API contracts. Native Pump assembly covers LaunchDeck's Pump launch modes end-to-end."
            .to_string(),
    ];
    let has_unverified_provider = !crate::providers::get_provider_meta(&resolved_provider).verified;
    if has_unverified_provider {
        warnings.push(format!(
            "Provider {} is currently marked unverified in this environment.",
            resolved_provider
        ));
    }

    LaunchReport {
        builtAt: built_at,
        configPath: config_path,
        mode: config.mode.clone(),
        launchpad: config.launchpad.clone(),
        rpcUrl: rpc_url,
        creator: creator.clone(),
        agentAuthority: agent_authority,
        mint,
        cashbackEnabled: config.mode == "cashback",
        agentEnabled: config.mode != "regular" && config.mode != "cashback",
        feeSharingStatus: fee_sharing_status,
        creatorFeeReceiver: creator_fee_receiver,
        buybackBps: if config.mode == "regular" || config.mode == "cashback" {
            None
        } else {
            config.agent.buybackBps
        },
        devBuyDescription: config
            .devBuy
            .as_ref()
            .map(|entry| format!("{}:{}", entry.mode, entry.amount))
            .unwrap_or_else(|| "none".to_string()),
        requestedLookupTables: requested_lookup_tables.clone(),
        loadedLookupTables: requested_lookup_tables,
        bundleJitoTip: bundle_jito_tip,
        transactions,
        execution: ExecutionSummary {
            provider: config.execution.provider.clone(),
            resolvedProvider: resolved_provider.clone(),
            policy: config.execution.policy.clone(),
            executionClass: execution_class.clone(),
            autoGas: config.execution.autoGas,
            launchAutoMode: config.execution.autoMode.clone(),
            activePresetId: config.presets.activePresetId.clone(),
            buyProvider: config.execution.buyProvider.clone(),
            buyPolicy: config.execution.buyPolicy.clone(),
            sellProvider: config.execution.sellProvider.clone(),
            sellPolicy: config.execution.sellPolicy.clone(),
            buyAutoGas: config.execution.buyAutoGas,
            buyAutoMode: config.execution.buyAutoMode.clone(),
            requestedSummary: requested_summary(config, &resolved_provider, &execution_class),
            txFormat: config.execution.txFormat.clone(),
            commitment: config.execution.commitment.clone(),
            skipPreflight: config.execution.skipPreflight,
            simulation: vec![],
            sent: vec![],
            warnings,
        },
        outPath: None,
    }
}

pub fn render_report(report: &LaunchReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Mode: {}", report.mode));
    lines.push(format!(
        "Config: {}",
        report.configPath.as_deref().unwrap_or("(cli only)")
    ));
    lines.push(format!("RPC: {}", report.rpcUrl));
    lines.push(format!("Creator: {}", report.creator));
    lines.push(format!(
        "Agent authority: {}",
        report.agentAuthority.as_deref().unwrap_or("(not used)")
    ));
    lines.push(format!("Mint: {}", report.mint));
    lines.push(format!("Cashback enabled: {}", report.cashbackEnabled));
    lines.push(format!("Tokenized Agent enabled: {}", report.agentEnabled));
    lines.push(format!(
        "Creator fee receiver: {}",
        report.creatorFeeReceiver
    ));
    lines.push(format!("Fee sharing: {}", report.feeSharingStatus));
    lines.push(format!(
        "Buyback BPS: {}",
        report
            .buybackBps
            .map(|value| value.to_string())
            .unwrap_or_else(|| "(not used)".to_string())
    ));
    lines.push(format!("Dev buy: {}", report.devBuyDescription));
    lines.push(format!("Launchpad: {}", report.launchpad));
    lines.push(format!(
        "Provider: {} ({})",
        report.execution.resolvedProvider, report.execution.policy
    ));
    lines.push(format!(
        "Execution class: {}",
        report.execution.executionClass
    ));
    if report.bundleJitoTip {
        lines.push("Jito tip handling: separate bundled tip transaction".to_string());
    }
    lines.push(format!("Execution: {}", report.execution.requestedSummary));
    lines.push(String::new());
    lines.push("Transactions:".to_string());
    for tx in &report.transactions {
        lines.push(format!(
            "- {}: {} instructions | legacy={} | v0={} | v0+alt={}",
            tx.label,
            tx.instructionSummary.len(),
            tx.legacyLength
                .map(|value| format!("{value} bytes"))
                .unwrap_or_else(|| "n/a".to_string()),
            tx.v0Length
                .map(|value| format!("{value} bytes"))
                .unwrap_or_else(|| "n/a".to_string()),
            tx.v0AltLength
                .map(|value| format!("{value} bytes"))
                .unwrap_or_else(|| "n/a".to_string())
        ));
        if !tx.lookupTablesUsed.is_empty() {
            lines.push(format!(
                "  lookup tables: {}",
                tx.lookupTablesUsed.join(", ")
            ));
        }
    }
    if !report.execution.warnings.is_empty() {
        lines.push(String::new());
        lines.push("Warnings:".to_string());
        for warning in &report.execution.warnings {
            lines.push(format!("- {}", warning));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RawConfig, normalize_raw_config};

    fn regular_config() -> crate::config::NormalizedConfig {
        let mut raw = RawConfig {
            mode: "regular".to_string(),
            launchpad: "pump".to_string(),
            ..RawConfig::default()
        };
        raw.token.name = "LaunchDeck".to_string();
        raw.token.symbol = "LDECK".to_string();
        raw.token.uri = "ipfs://fixture".to_string();
        normalize_raw_config(raw).expect("normalized config")
    }

    #[test]
    fn agent_unlocked_plans_single_launch_transaction() {
        let mut config = regular_config();
        config.mode = "agent-unlocked".to_string();
        config.agent.buybackBps = Some(2_500);

        let report = build_report(
            &config,
            "2026-03-27T00:00:00Z".to_string(),
            "https://rpc.example".to_string(),
            "creator".to_string(),
            "mint".to_string(),
            Some("creator".to_string()),
            None,
        );

        assert_eq!(report.transactions.len(), 1);
        assert_eq!(report.transactions[0].label, "launch");
    }
}
