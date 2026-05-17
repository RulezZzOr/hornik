use crate::{BoardFamily, Model, RuntimeBackendMode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStatus {
    Detected,
    Missing,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeCheck {
    pub name: String,
    pub interface: String,
    pub path: String,
    pub required: bool,
    pub status: ProbeStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareProbeSummary {
    pub total: usize,
    pub detected: usize,
    pub missing_required: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareProbeReport {
    pub schema_version: u8,
    pub backend: RuntimeBackendMode,
    pub model: Model,
    pub board_family: BoardFamily,
    pub safe_read_only: bool,
    pub probe_root: Option<String>,
    pub summary: HardwareProbeSummary,
    pub checks: Vec<ProbeCheck>,
    pub notes: Vec<String>,
}

impl HardwareProbeReport {
    pub const SCHEMA_VERSION: u8 = 1;

    pub fn new(
        backend: RuntimeBackendMode,
        model: Model,
        board_family: BoardFamily,
        probe_root: Option<String>,
        checks: Vec<ProbeCheck>,
        notes: Vec<String>,
    ) -> Self {
        let summary = HardwareProbeSummary {
            total: checks.len(),
            detected: checks
                .iter()
                .filter(|check| check.status == ProbeStatus::Detected)
                .count(),
            missing_required: checks
                .iter()
                .filter(|check| check.required && check.status == ProbeStatus::Missing)
                .count(),
            skipped: checks
                .iter()
                .filter(|check| check.status == ProbeStatus::Skipped)
                .count(),
        };

        Self {
            schema_version: Self::SCHEMA_VERSION,
            backend,
            model,
            board_family,
            safe_read_only: true,
            probe_root,
            summary,
            checks,
            notes,
        }
    }
}
