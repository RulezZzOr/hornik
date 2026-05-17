use crate::{BoardFamily, Model, RuntimeBackendMode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirmwareGapState {
    Ready,
    Partial,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirmwareGap {
    pub key: String,
    pub title: String,
    pub state: FirmwareGapState,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FirmwareDeploymentReport {
    pub schema_version: u8,
    pub backend: RuntimeBackendMode,
    pub board_family: BoardFamily,
    pub model: Model,
    pub probe_root: Option<String>,
    pub deployable: bool,
    pub gaps: Vec<FirmwareGap>,
    pub notes: Vec<String>,
}

impl FirmwareDeploymentReport {
    pub const SCHEMA_VERSION: u8 = 1;
}
