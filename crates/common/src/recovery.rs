use crate::{BoardFamily, Model, RuntimeBackendMode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiBrickState {
    SafeFirstBoot,
    Guarded,
    Unsafe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AntiBrickCheck {
    pub key: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AntiBrickReport {
    pub schema_version: u8,
    pub board_family: BoardFamily,
    pub model: Model,
    pub backend: RuntimeBackendMode,
    pub state: AntiBrickState,
    pub safe_to_first_boot: bool,
    pub production_flashable: bool,
    pub nand_writes_allowed: bool,
    pub asic_writes_allowed: bool,
    pub tuning_writes_allowed: bool,
    pub flashing_allowed: bool,
    pub checks: Vec<AntiBrickCheck>,
    pub notes: Vec<String>,
}

impl AntiBrickReport {
    pub const SCHEMA_VERSION: u8 = 1;
}
