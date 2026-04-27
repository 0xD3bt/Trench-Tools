#![allow(non_snake_case, dead_code)]

use std::{collections::BTreeMap, env, fs};

use shared_extension_runtime::catalog::{self, LaunchpadAvailabilityInputs};
#[allow(unused_imports)]
pub use shared_extension_runtime::catalog::{
    LaunchpadAvailability, StrategySupport, TokenMetadataLimits,
};

fn bags_configured() -> bool {
    env::var("BAGS_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || fs::read_to_string(crate::paths::bags_credentials_path())
            .ok()
            .map(|value| value.contains("\"apiKey\""))
            .unwrap_or(false)
        || fs::read_to_string(crate::paths::bags_session_path())
            .ok()
            .map(|value| value.contains("\"apiKey\""))
            .unwrap_or(false)
}

pub fn launchpad_registry() -> BTreeMap<String, LaunchpadAvailability> {
    catalog::launchpad_registry(LaunchpadAvailabilityInputs {
        bags_configured: bags_configured(),
    })
}
