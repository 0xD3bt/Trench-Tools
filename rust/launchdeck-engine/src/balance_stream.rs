#![allow(dead_code)]

pub use shared_extension_runtime::balance_stream::{
    BalanceEventPayload, BalanceSnapshot, BalanceStreamHandle, ConnectionState, StreamConfig,
    StreamEvent, TradeEventPayload,
};

pub fn spawn(config: StreamConfig) -> BalanceStreamHandle {
    crate::wallet::ensure_wallet_runtime_configured();
    shared_extension_runtime::balance_stream::spawn(config)
}
