#![allow(non_snake_case, dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BagsLaunchMetadata {
    #[serde(default)]
    pub configKey: String,
    #[serde(default)]
    pub migrationFeeOption: Option<i64>,
    #[serde(default)]
    pub expectedMigrationFamily: String,
    #[serde(default)]
    pub expectedDammConfigKey: String,
    #[serde(default)]
    pub expectedDammDerivationMode: String,
    #[serde(default)]
    pub preMigrationDbcPoolAddress: String,
    #[serde(default)]
    pub postMigrationDammPoolAddress: String,
}
