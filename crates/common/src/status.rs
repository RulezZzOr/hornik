use crate::{BoardFamily, CapabilitySet, Model};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeBackendMode {
    #[default]
    Simulated,
    HardwareProbe,
}

impl RuntimeBackendMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Simulated => "simulated",
            Self::HardwareProbe => "hardware-probe",
        }
    }
}

impl fmt::Display for RuntimeBackendMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RuntimeBackendMode {
    type Err = RuntimeBackendModeParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "simulated" | "sim" => Ok(Self::Simulated),
            "hardware-probe" | "hardware_probe" | "probe" | "hw-probe" => Ok(Self::HardwareProbe),
            other => Err(RuntimeBackendModeParseError(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown runtime backend mode: {0}")]
pub struct RuntimeBackendModeParseError(String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemInfo {
    pub model: Model,
    pub board_family: BoardFamily,
    pub backend: RuntimeBackendMode,
    pub firmware_version: String,
    pub active_slot: String,
    pub uptime_seconds: u64,
    pub serial: Option<String>,
    pub capabilities: CapabilitySet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Ok,
    Warn,
    Error,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Booting,
    Recovering,
    Idle,
    Starting,
    Mining,
    Degraded,
    SafeMode,
    Updating,
    RollbackPending,
    Fault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthStatusResponse {
    pub state: HealthStatus,
    pub severity: Severity,
    pub issues: Vec<String>,
    pub active_slot: String,
    pub rollback_available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MinerMode {
    StockLike,
    Eco,
    Balanced,
    Performance,
    Manual,
    SafeMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MinerStatus {
    pub hashrate_ths: f64,
    pub power_w: u32,
    pub efficiency_j_th: f64,
    pub accepted_shares: u64,
    pub rejected_shares: u64,
    pub mode: MinerMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainStatus {
    pub id: u8,
    pub present: bool,
    pub enabled: bool,
    pub asic_detected: u16,
    pub temp_board_c: f64,
    pub temp_chip_max_c: f64,
    pub fault: Option<String>,
}
