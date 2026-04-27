use shared_execution_routing::{
    execution::NormalizedExecution, transport::TransportPlan as SharedTransportPlan,
};
use shared_transaction_submit::{
    CompiledTransaction as SharedCompiledTransaction, SentResult as SharedSentResult,
};

use crate::{
    extension_api::MevMode,
    rpc_client::{CompiledTransaction, SentResult},
    trade_runtime::RuntimeExecutionPolicy,
    transport::TransportPlan,
};

pub fn to_launchdeck_execution(policy: &RuntimeExecutionPolicy) -> NormalizedExecution {
    let jitodontfront = matches!(policy.mev_mode, MevMode::Reduced | MevMode::Secure);
    NormalizedExecution {
        simulate: false,
        send: true,
        txFormat: "v0-alt".to_string(),
        commitment: policy.commitment.clone(),
        skipPreflight: policy.skip_preflight,
        trackSendBlockHeight: policy.track_send_block_height,
        provider: policy.provider.clone(),
        endpointProfile: policy.endpoint_profile.clone(),
        mevProtect: !matches!(policy.mev_mode, MevMode::Off),
        mevMode: mev_mode_label(&policy.mev_mode).to_string(),
        jitodontfront,
        autoGas: policy.auto_tip_enabled,
        autoMode: auto_mode_label(policy.auto_tip_enabled).to_string(),
        priorityFeeSol: policy.fee_sol.clone(),
        tipSol: policy.tip_sol.clone(),
        maxPriorityFeeSol: policy.fee_sol.clone(),
        maxTipSol: policy.tip_sol.clone(),
        buyProvider: policy.provider.clone(),
        buyEndpointProfile: policy.endpoint_profile.clone(),
        buyMevProtect: !matches!(policy.mev_mode, MevMode::Off),
        buyMevMode: mev_mode_label(&policy.mev_mode).to_string(),
        buyJitodontfront: jitodontfront,
        buyAutoGas: policy.auto_tip_enabled,
        buyAutoMode: auto_mode_label(policy.auto_tip_enabled).to_string(),
        buyPriorityFeeSol: policy.fee_sol.clone(),
        buyTipSol: policy.tip_sol.clone(),
        buySlippagePercent: policy.slippage_percent.clone(),
        buyMaxPriorityFeeSol: policy.fee_sol.clone(),
        buyMaxTipSol: policy.tip_sol.clone(),
        buyFundingPolicy: buy_funding_policy_label(policy.buy_funding_policy).to_string(),
        sellAutoGas: policy.auto_tip_enabled,
        sellAutoMode: auto_mode_label(policy.auto_tip_enabled).to_string(),
        sellProvider: policy.provider.clone(),
        sellEndpointProfile: policy.endpoint_profile.clone(),
        sellMevProtect: !matches!(policy.mev_mode, MevMode::Off),
        sellMevMode: mev_mode_label(&policy.mev_mode).to_string(),
        sellJitodontfront: jitodontfront,
        sellPriorityFeeSol: policy.fee_sol.clone(),
        sellTipSol: policy.tip_sol.clone(),
        sellSlippagePercent: policy.slippage_percent.clone(),
        sellMaxPriorityFeeSol: policy.fee_sol.clone(),
        sellMaxTipSol: policy.tip_sol.clone(),
        sellSettlementPolicy: sell_settlement_policy_label(policy.sell_settlement_policy)
            .to_string(),
        sellSettlementAsset: trade_settlement_asset_label(policy.sell_settlement_asset).to_string(),
    }
}

pub fn map_compiled_transaction(compiled: SharedCompiledTransaction) -> CompiledTransaction {
    CompiledTransaction {
        label: compiled.label,
        format: compiled.format,
        serialized_base64: compiled.serializedBase64,
        signature: compiled.signature,
        lookup_tables_used: compiled.lookupTablesUsed,
        compute_unit_limit: compiled.computeUnitLimit,
        compute_unit_price_micro_lamports: compiled.computeUnitPriceMicroLamports,
        inline_tip_lamports: compiled.inlineTipLamports,
        inline_tip_account: compiled.inlineTipAccount,
    }
}

pub fn map_transport_plan(plan: &TransportPlan) -> SharedTransportPlan {
    SharedTransportPlan {
        requestedProvider: plan.requested_provider.clone(),
        resolvedProvider: plan.resolved_provider.clone(),
        requestedEndpointProfile: plan.requested_endpoint_profile.clone(),
        resolvedEndpointProfile: plan.resolved_endpoint_profile.clone(),
        executionClass: plan.execution_class.clone(),
        transportType: plan.transport_type.clone(),
        ordering: plan.ordering.clone(),
        verified: plan.verified,
        supportsBundle: plan.supports_bundle,
        requiresInlineTip: plan.requires_inline_tip,
        requiresPriorityFee: plan.requires_priority_fee,
        separateTipTransaction: plan.separate_tip_transaction,
        skipPreflight: plan.skip_preflight,
        maxRetries: plan.max_retries,
        standardRpcSubmitEndpoints: plan.standard_rpc_submit_endpoints.clone(),
        helloMoonApiKeyConfigured: plan.hello_moon_api_key_configured,
        helloMoonMevProtect: plan.hello_moon_mev_protect,
        helloMoonQuicEndpoint: plan.hello_moon_quic_endpoint.clone(),
        helloMoonQuicEndpoints: plan.hello_moon_quic_endpoints.clone(),
        helloMoonBundleEndpoint: plan.hello_moon_bundle_endpoint.clone(),
        helloMoonBundleEndpoints: plan.hello_moon_bundle_endpoints.clone(),
        heliusSenderEndpoint: plan.helius_sender_endpoint.clone(),
        heliusSenderEndpoints: plan.helius_sender_endpoints.clone(),
        watchEndpoint: plan.watch_endpoint.clone(),
        watchEndpoints: plan.watch_endpoints.clone(),
        jitoBundleEndpoints: plan.jito_bundle_endpoints.clone(),
        warnings: plan.warnings.clone(),
    }
}

pub fn map_compiled_transaction_to_shared(
    compiled: &CompiledTransaction,
) -> SharedCompiledTransaction {
    SharedCompiledTransaction {
        label: compiled.label.clone(),
        format: compiled.format.clone(),
        blockhash: String::new(),
        lastValidBlockHeight: 0,
        serializedBase64: compiled.serialized_base64.clone(),
        signature: compiled.signature.clone(),
        lookupTablesUsed: compiled.lookup_tables_used.clone(),
        computeUnitLimit: compiled.compute_unit_limit,
        computeUnitPriceMicroLamports: compiled.compute_unit_price_micro_lamports,
        inlineTipLamports: compiled.inline_tip_lamports,
        inlineTipAccount: compiled.inline_tip_account.clone(),
    }
}

pub fn map_sent_result(result: SharedSentResult) -> SentResult {
    SentResult {
        label: result.label,
        format: result.format,
        signature: result.signature,
        transport_type: result.transportType,
        endpoint: result.endpoint,
        attempted_endpoints: result.attemptedEndpoints,
        skip_preflight: result.skipPreflight,
        max_retries: result.maxRetries,
        confirmation_status: result.confirmationStatus,
        error: None,
        bundle_id: result.bundleId,
        attempted_bundle_ids: result.attemptedBundleIds,
        transaction_subscribe_account_required: result.transactionSubscribeAccountRequired,
    }
}

pub fn map_sent_result_to_shared(result: SentResult) -> SharedSentResult {
    SharedSentResult {
        label: result.label,
        format: result.format,
        signature: result.signature,
        explorerUrl: None,
        transportType: result.transport_type,
        endpoint: result.endpoint,
        attemptedEndpoints: result.attempted_endpoints,
        skipPreflight: result.skip_preflight,
        maxRetries: result.max_retries,
        confirmationStatus: result.confirmation_status,
        confirmationSource: None,
        submittedAtMs: None,
        firstObservedStatus: None,
        firstObservedSlot: None,
        firstObservedAtMs: None,
        confirmedAtMs: None,
        sendObservedSlot: None,
        confirmedObservedSlot: None,
        confirmedSlot: None,
        computeUnitLimit: None,
        computeUnitPriceMicroLamports: None,
        inlineTipLamports: None,
        inlineTipAccount: None,
        bundleId: result.bundle_id,
        attemptedBundleIds: result.attempted_bundle_ids,
        transactionSubscribeAccountRequired: result.transaction_subscribe_account_required,
        postTokenBalances: vec![],
        confirmedTokenBalanceRaw: None,
        balanceWatchAccount: None,
        capturePostTokenBalances: false,
        requestFullTransactionDetails: false,
    }
}

fn mev_mode_label(mode: &MevMode) -> &'static str {
    match mode {
        MevMode::Off => "off",
        MevMode::Reduced => "reduced",
        MevMode::Secure => "secure",
    }
}

fn auto_mode_label(enabled: bool) -> &'static str {
    if enabled { "auto" } else { "manual" }
}

fn buy_funding_policy_label(policy: crate::extension_api::BuyFundingPolicy) -> &'static str {
    match policy {
        crate::extension_api::BuyFundingPolicy::SolOnly => "sol_only",
        crate::extension_api::BuyFundingPolicy::PreferUsd1ElseTopUp => "prefer_usd1_else_topup",
        crate::extension_api::BuyFundingPolicy::Usd1Only => "usd1_only",
    }
}

fn sell_settlement_policy_label(
    policy: crate::extension_api::SellSettlementPolicy,
) -> &'static str {
    match policy {
        crate::extension_api::SellSettlementPolicy::AlwaysToSol => "always_to_sol",
        crate::extension_api::SellSettlementPolicy::AlwaysToUsd1 => "always_to_usd1",
        crate::extension_api::SellSettlementPolicy::MatchStoredEntryPreference => {
            "match_stored_entry_preference"
        }
    }
}

fn trade_settlement_asset_label(asset: crate::extension_api::TradeSettlementAsset) -> &'static str {
    match asset {
        crate::extension_api::TradeSettlementAsset::Sol => "sol",
        crate::extension_api::TradeSettlementAsset::Usd1 => "usd1",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_api::MevMode;
    use crate::trade_runtime::RuntimeExecutionPolicy;

    fn sample_policy(mev_mode: MevMode) -> RuntimeExecutionPolicy {
        RuntimeExecutionPolicy {
            slippage_percent: "90".to_string(),
            mev_mode,
            auto_tip_enabled: false,
            fee_sol: "0.0001".to_string(),
            tip_sol: "0.001".to_string(),
            provider: "hellomoon".to_string(),
            endpoint_profile: "fra".to_string(),
            commitment: "confirmed".to_string(),
            skip_preflight: true,
            track_send_block_height: true,
            buy_funding_policy: crate::extension_api::BuyFundingPolicy::SolOnly,
            sell_settlement_policy: crate::extension_api::SellSettlementPolicy::AlwaysToSol,
            sell_settlement_asset: crate::extension_api::TradeSettlementAsset::Sol,
        }
    }

    #[test]
    fn secure_mev_maps_to_secure_label_for_launchdeck() {
        assert_eq!(mev_mode_label(&MevMode::Secure), "secure");
    }

    #[test]
    fn reduced_mev_enables_jitodontfront_for_launchdeck_execution() {
        let execution = to_launchdeck_execution(&sample_policy(MevMode::Reduced));
        assert!(execution.jitodontfront);
        assert!(execution.buyJitodontfront);
        assert!(execution.sellJitodontfront);
    }

    #[test]
    fn map_sent_result_preserves_bundle_metadata() {
        let sent = map_sent_result(SharedSentResult {
            label: "buy".to_string(),
            format: "v0".to_string(),
            signature: Some("sig-1".to_string()),
            explorerUrl: None,
            transportType: "jito-bundle".to_string(),
            endpoint: Some("https://bundle.example".to_string()),
            attemptedEndpoints: vec!["https://bundle.example".to_string()],
            skipPreflight: false,
            maxRetries: 0,
            confirmationStatus: Some("confirmed".to_string()),
            confirmationSource: None,
            submittedAtMs: None,
            firstObservedStatus: None,
            firstObservedSlot: None,
            firstObservedAtMs: None,
            confirmedAtMs: None,
            sendObservedSlot: None,
            confirmedObservedSlot: None,
            confirmedSlot: None,
            computeUnitLimit: None,
            computeUnitPriceMicroLamports: None,
            inlineTipLamports: None,
            inlineTipAccount: None,
            bundleId: Some("bundle-1".to_string()),
            attemptedBundleIds: vec!["bundle-1".to_string(), "bundle-2".to_string()],
            transactionSubscribeAccountRequired: vec![],
            postTokenBalances: vec![],
            confirmedTokenBalanceRaw: None,
            balanceWatchAccount: None,
            capturePostTokenBalances: false,
            requestFullTransactionDetails: false,
        });

        assert_eq!(sent.bundle_id.as_deref(), Some("bundle-1"));
        assert_eq!(
            sent.attempted_bundle_ids,
            vec!["bundle-1".to_string(), "bundle-2".to_string()]
        );
    }
}
