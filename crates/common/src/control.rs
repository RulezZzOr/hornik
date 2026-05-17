use crate::{BoardFamily, Model, TuningMode, TuningTargetType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardControlState {
    Running,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TuningLockState {
    Searching,
    Locked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockedTuningProfile {
    pub mode: TuningMode,
    pub target_type: TuningTargetType,
    pub target_value: Option<f64>,
    pub base_frequency_mhz: u16,
    pub base_voltage_mv: u16,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeControlReport {
    pub schema_version: u8,
    pub board_family: BoardFamily,
    pub model: Model,
    pub board_state: BoardControlState,
    pub board_paused: bool,
    pub pause_reason: Option<String>,
    pub tuning_lock_state: TuningLockState,
    pub tuning_locked: bool,
    pub locked_profile: Option<LockedTuningProfile>,
    pub notes: Vec<String>,
}

impl RuntimeControlReport {
    pub const SCHEMA_VERSION: u8 = 1;
}
