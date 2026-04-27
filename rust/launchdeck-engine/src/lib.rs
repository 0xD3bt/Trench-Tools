// The crate is a large workspace member with several modules whose
// public types intentionally use camelCase field names to match
// on-the-wire JSON payloads (e.g. launchpad import contexts that
// serialize 1:1 with external API responses). We keep the
// `non_snake_case` allow global because touching those names in every
// serde derive would be churny for zero runtime gain.
//
// `dead_code` is intentionally NOT suppressed globally — individual
// items that are wired for future use should opt in with
// `#[allow(dead_code)]` at the item level so fresh dead code still
// surfaces in `cargo check`.
#![allow(non_snake_case)]

pub(crate) mod alt_diagnostics;
pub mod app_logs;
pub mod bags_native;
pub mod balance_stream;
pub mod bonk_native;
pub mod compiled_transaction_signers;
pub mod config;
pub mod crypto;
pub mod endpoint_profile;
pub mod execution_engine_bridge;
pub mod follow;
pub mod fs_utils;
pub mod image_library;
pub mod launchpad_dispatch;
pub mod launchpad_runtime;
pub mod launchpad_warm;
pub mod launchpads;
pub mod observability;
pub mod paths;
pub mod provider_tip;
pub mod providers;
pub mod pump_native;
pub mod report;
pub mod reports_browser;
pub mod rpc;
pub mod runtime;
pub mod strategies;
pub mod transport;
pub mod ui_bridge;
pub mod ui_config;
pub mod vamp;
pub mod wallet;
pub mod warm_manager;
pub mod wrapper_compile;
