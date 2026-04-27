#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedExecution {
    pub simulate: bool,
    pub send: bool,
    pub txFormat: String,
    pub commitment: String,
    pub skipPreflight: bool,
    pub trackSendBlockHeight: bool,
    pub provider: String,
    pub endpointProfile: String,
    #[serde(default)]
    pub mevProtect: bool,
    #[serde(default)]
    pub mevMode: String,
    #[serde(default)]
    pub jitodontfront: bool,
    pub autoGas: bool,
    pub autoMode: String,
    pub priorityFeeSol: String,
    pub tipSol: String,
    pub maxPriorityFeeSol: String,
    pub maxTipSol: String,
    pub buyProvider: String,
    pub buyEndpointProfile: String,
    #[serde(default)]
    pub buyMevProtect: bool,
    #[serde(default)]
    pub buyMevMode: String,
    #[serde(default)]
    pub buyJitodontfront: bool,
    pub buyAutoGas: bool,
    pub buyAutoMode: String,
    pub buyPriorityFeeSol: String,
    pub buyTipSol: String,
    pub buySlippagePercent: String,
    pub buyMaxPriorityFeeSol: String,
    pub buyMaxTipSol: String,
    #[serde(default)]
    pub buyFundingPolicy: String,
    pub sellAutoGas: bool,
    pub sellAutoMode: String,
    pub sellProvider: String,
    pub sellEndpointProfile: String,
    #[serde(default)]
    pub sellMevProtect: bool,
    #[serde(default)]
    pub sellMevMode: String,
    #[serde(default)]
    pub sellJitodontfront: bool,
    pub sellPriorityFeeSol: String,
    pub sellTipSol: String,
    pub sellSlippagePercent: String,
    pub sellMaxPriorityFeeSol: String,
    pub sellMaxTipSol: String,
    #[serde(default)]
    pub sellSettlementPolicy: String,
    #[serde(default)]
    pub sellSettlementAsset: String,
}
