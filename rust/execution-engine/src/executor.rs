use crate::{
    extension_api::{BuyFundingPolicy, MevMode, TradeSettlementAsset, TradeSide},
    trade_dispatch::TradeDispatchPlan,
    trade_planner::LifecycleAndCanonicalMarket,
    trade_runtime::{
        CompiledTradePlan, RuntimeExecutionPolicy, RuntimeSellIntent, TradeRuntimeRequest,
        execute_wallet_trade as execute_wallet_trade_with_adapter,
        execute_wallet_trade_with_pre_submit_check,
    },
};

#[derive(Debug, Clone)]
pub struct ExecutionPolicy {
    pub slippage_percent: String,
    pub mev_mode: MevMode,
    pub auto_tip_enabled: bool,
    pub fee_sol: String,
    pub tip_sol: String,
    pub provider: String,
    pub endpoint_profile: String,
    pub commitment: String,
    pub skip_preflight: bool,
    pub track_send_block_height: bool,
    pub buy_funding_policy: BuyFundingPolicy,
    pub sell_settlement_policy: crate::extension_api::SellSettlementPolicy,
    pub sell_settlement_asset: TradeSettlementAsset,
}

#[derive(Debug, Clone)]
pub enum SellIntent {
    Percent(String),
    SolOutput(String),
}

#[derive(Debug, Clone)]
pub struct WalletTradeRequest {
    pub side: TradeSide,
    pub mint: String,
    pub platform_label: Option<String>,
    pub buy_amount_sol: Option<String>,
    pub sell_intent: Option<SellIntent>,
    pub policy: ExecutionPolicy,
    pub planned_route: Option<TradeDispatchPlan>,
    pub planned_trade: Option<LifecycleAndCanonicalMarket>,
    pub pinned_pool: Option<String>,
    pub warm_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExecutedTrade {
    pub tx_signature: String,
    pub entry_preference_asset: Option<crate::extension_api::TradeSettlementAsset>,
}

#[derive(Debug, Clone)]
pub struct ExecutionExecutor;

impl Default for ExecutionExecutor {
    fn default() -> Self {
        Self
    }
}

impl ExecutionExecutor {
    pub fn route_name(&self) -> &'static str {
        "engine_native_dispatch"
    }

    pub async fn execute_wallet_trade(
        &self,
        request: WalletTradeRequest,
        wallet_key: String,
    ) -> Result<ExecutedTrade, String> {
        self.execute_wallet_trade_inner(
            request,
            wallet_key,
            Option::<fn(&str, &CompiledTradePlan) -> Result<(), String>>::None,
        )
        .await
    }

    pub async fn execute_wallet_trade_checked<F>(
        &self,
        request: WalletTradeRequest,
        wallet_key: String,
        pre_submit_check: F,
    ) -> Result<ExecutedTrade, String>
    where
        F: Fn(&str, &CompiledTradePlan) -> Result<(), String> + Send + Sync,
    {
        self.execute_wallet_trade_inner(request, wallet_key, Some(pre_submit_check))
            .await
    }

    async fn execute_wallet_trade_inner<F>(
        &self,
        request: WalletTradeRequest,
        wallet_key: String,
        pre_submit_check: Option<F>,
    ) -> Result<ExecutedTrade, String>
    where
        F: Fn(&str, &CompiledTradePlan) -> Result<(), String> + Send + Sync,
    {
        let runtime_request = TradeRuntimeRequest {
            side: request.side,
            mint: request.mint,
            buy_amount_sol: request.buy_amount_sol,
            sell_intent: request.sell_intent.map(|intent| match intent {
                SellIntent::Percent(value) => RuntimeSellIntent::Percent(value),
                SellIntent::SolOutput(value) => RuntimeSellIntent::SolOutput(value),
            }),
            policy: RuntimeExecutionPolicy {
                slippage_percent: request.policy.slippage_percent,
                mev_mode: request.policy.mev_mode,
                auto_tip_enabled: request.policy.auto_tip_enabled,
                fee_sol: request.policy.fee_sol,
                tip_sol: request.policy.tip_sol,
                provider: request.policy.provider,
                endpoint_profile: request.policy.endpoint_profile,
                commitment: request.policy.commitment,
                skip_preflight: request.policy.skip_preflight,
                track_send_block_height: request.policy.track_send_block_height,
                buy_funding_policy: request.policy.buy_funding_policy,
                sell_settlement_policy: request.policy.sell_settlement_policy,
                sell_settlement_asset: request.policy.sell_settlement_asset,
            },
            platform_label: request.platform_label,
            planned_route: request.planned_route,
            planned_trade: request.planned_trade,
            pinned_pool: request.pinned_pool,
            warm_key: request.warm_key,
        };
        let result = match pre_submit_check {
            Some(check) => {
                execute_wallet_trade_with_pre_submit_check(runtime_request, wallet_key, check)
                    .await?
            }
            None => execute_wallet_trade_with_adapter(runtime_request, wallet_key).await?,
        };
        Ok(ExecutedTrade {
            tx_signature: result.tx_signature,
            entry_preference_asset: result.entry_preference_asset,
        })
    }
}
