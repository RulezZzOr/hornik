use crate::BoardFamily;
use crate::MinerMode;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TuningMode {
    #[default]
    StockLike,
    Eco,
    Balanced,
    Performance,
    Manual,
    SafeMode,
}

impl From<TuningMode> for MinerMode {
    fn from(value: TuningMode) -> Self {
        match value {
            TuningMode::StockLike => Self::StockLike,
            TuningMode::Eco => Self::Eco,
            TuningMode::Balanced => Self::Balanced,
            TuningMode::Performance => Self::Performance,
            TuningMode::Manual => Self::Manual,
            TuningMode::SafeMode => Self::SafeMode,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TuningTargetType {
    #[default]
    Watts,
    Efficiency,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuningConfig {
    #[serde(default)]
    pub mode: TuningMode,
    #[serde(default)]
    pub target_type: TuningTargetType,
    #[serde(default)]
    pub target_value: Option<f64>,
    #[serde(default)]
    pub autotune: bool,
}

impl Default for TuningConfig {
    fn default() -> Self {
        Self {
            mode: TuningMode::StockLike,
            target_type: TuningTargetType::Watts,
            target_value: None,
            autotune: false,
        }
    }
}

impl TuningConfig {
    pub fn validate(self) -> Result<(), TuningConfigError> {
        if self.mode == TuningMode::Manual {
            return Err(TuningConfigError::ManualDisabledInMvp);
        }

        if let Some(target_value) = self.target_value {
            if !target_value.is_finite() || target_value <= 0.0 {
                return Err(TuningConfigError::InvalidTargetValue(target_value));
            }
        }

        if self.autotune && matches!(self.mode, TuningMode::SafeMode) {
            return Err(TuningConfigError::AutotuneDisabledInSafeMode);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileInfo {
    pub name: TuningMode,
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfilesResponse {
    pub active: TuningMode,
    pub target_type: TuningTargetType,
    pub target_value: Option<f64>,
    pub autotune: bool,
    pub profiles: Vec<ProfileInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TuningPlanState {
    Disabled,
    PlannedReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TuningPhase {
    Baseline,
    DownclockEfficiency,
    UpclockStability,
    VoltageTrim,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuningStep {
    pub order: u8,
    pub phase: TuningPhase,
    pub scope: String,
    pub action: String,
    pub min_duration_seconds: u32,
    pub enabled_in_build: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TuningGuardrails {
    pub chip_frequency_step_mhz: u16,
    pub voltage_step_mv: u16,
    pub min_step_duration_seconds: u32,
    pub max_chip_temp_c: f64,
    pub max_hw_error_rate_percent: f64,
    pub rollback_on_rejected_shares: bool,
    pub voltage_trim_requires_stable_upclock: bool,
}

impl Default for TuningGuardrails {
    fn default() -> Self {
        Self {
            chip_frequency_step_mhz: 5,
            voltage_step_mv: 5,
            min_step_duration_seconds: 300,
            max_chip_temp_c: 85.0,
            max_hw_error_rate_percent: 0.03,
            rollback_on_rejected_shares: true,
            voltage_trim_requires_stable_upclock: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TuningPlanResponse {
    pub state: TuningPlanState,
    pub active_phase: Option<TuningPhase>,
    pub writable: bool,
    pub guardrails: TuningGuardrails,
    pub steps: Vec<TuningStep>,
    pub notes: Vec<String>,
}

impl From<TuningConfig> for TuningPlanResponse {
    fn from(config: TuningConfig) -> Self {
        let state = if config.autotune {
            TuningPlanState::PlannedReadOnly
        } else {
            TuningPlanState::Disabled
        };

        Self {
            state,
            active_phase: config.autotune.then_some(TuningPhase::Baseline),
            writable: false,
            guardrails: TuningGuardrails::default(),
            steps: tuning_plan_steps(),
            notes: vec![
                "build 0.1.0 exposes the autotune plan only; it does not write clocks or voltages"
                    .to_string(),
                "voltage trim is last and requires stable upclock results".to_string(),
                "future autotune must roll back on thermal, HW error, rejected-share, or chain instability signals".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuningProtocolFrame {
    pub order: u8,
    pub phase: TuningPhase,
    pub command: String,
    pub target_frequency_mhz: u16,
    pub target_voltage_mv: u16,
    pub min_duration_seconds: u32,
    pub frame: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuningProtocolTranscript {
    pub schema_version: u8,
    pub board_family: BoardFamily,
    pub chip_id: u16,
    pub base_frequency_mhz: u16,
    pub base_voltage_mv: u16,
    pub frequency_step_mhz: u16,
    pub voltage_step_mv: u16,
    pub frames: Vec<TuningProtocolFrame>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuningProtocolSequenceSpec {
    pub board_family: BoardFamily,
    pub chip_id: u16,
    pub phases: Vec<TuningPhase>,
    pub base_frequency_mhz: u16,
    pub base_voltage_mv: u16,
    pub frequency_step_mhz: u16,
    pub voltage_step_mv: u16,
    pub min_step_duration_seconds: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TuningExecutionState {
    Disabled,
    PlannedReadOnly,
    ReadyToArm,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuningExecutionStep {
    pub order: u8,
    pub phase: TuningPhase,
    pub command: String,
    pub target_frequency_mhz: u16,
    pub target_voltage_mv: u16,
    pub min_duration_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TuningExecutionStatus {
    pub schema_version: u8,
    pub state: TuningExecutionState,
    pub autotune: bool,
    pub active_phase: Option<TuningPhase>,
    pub current_step: Option<TuningExecutionStep>,
    pub queued_steps: Vec<TuningExecutionStep>,
    pub write_allowed: bool,
    pub blocked_reason: Option<String>,
    pub notes: Vec<String>,
}

impl TuningExecutionStatus {
    pub const SCHEMA_VERSION: u8 = 1;
}

impl From<TuningConfig> for ProfilesResponse {
    fn from(config: TuningConfig) -> Self {
        Self {
            active: config.mode,
            target_type: config.target_type,
            target_value: config.target_value,
            autotune: config.autotune,
            profiles: profile_catalog(),
        }
    }
}

pub fn tuning_plan_steps() -> Vec<TuningStep> {
    [
        (
            1,
            TuningPhase::Baseline,
            "chain".to_string(),
            "measure stock-like stability before any OC change".to_string(),
            900,
        ),
        (
            2,
            TuningPhase::DownclockEfficiency,
            "chip".to_string(),
            "step frequency down slowly to find efficient per-chip baseline".to_string(),
            600,
        ),
        (
            3,
            TuningPhase::UpclockStability,
            "chip".to_string(),
            "step frequency up slowly after downclock baseline is stable".to_string(),
            600,
        ),
        (
            4,
            TuningPhase::VoltageTrim,
            "chip".to_string(),
            "trim voltage last, only after stable frequency results".to_string(),
            900,
        ),
    ]
    .into_iter()
    .map(
        |(order, phase, scope, action, min_duration_seconds)| TuningStep {
            order,
            phase,
            scope,
            action,
            min_duration_seconds,
            enabled_in_build: false,
        },
    )
    .collect()
}

pub fn profile_catalog() -> Vec<ProfileInfo> {
    [
        (TuningMode::StockLike, true, None),
        (TuningMode::Eco, true, None),
        (TuningMode::Balanced, true, None),
        (TuningMode::Performance, true, None),
        (
            TuningMode::Manual,
            false,
            Some("manual tuning is disabled in build 0.1.0".to_string()),
        ),
        (TuningMode::SafeMode, true, None),
    ]
    .into_iter()
    .map(|(name, available, reason)| ProfileInfo {
        name,
        available,
        reason,
    })
    .collect()
}

#[derive(Debug, Error)]
pub enum TuningConfigError {
    #[error("manual tuning is disabled in build 0.1.0")]
    ManualDisabledInMvp,
    #[error("tuning target value must be positive and finite: {0}")]
    InvalidTargetValue(f64),
    #[error("autotune is disabled in safe_mode")]
    AutotuneDisabledInSafeMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_like_is_default() {
        let config = TuningConfig::default();

        assert_eq!(config.mode, TuningMode::StockLike);
        assert!(!config.autotune);
        assert_eq!(MinerMode::from(config.mode), MinerMode::StockLike);
    }

    #[test]
    fn rejects_manual_in_mvp() {
        let config = TuningConfig {
            mode: TuningMode::Manual,
            ..TuningConfig::default()
        };

        assert!(matches!(
            config.validate(),
            Err(TuningConfigError::ManualDisabledInMvp)
        ));
    }

    #[test]
    fn rejects_negative_target_value() {
        let config = TuningConfig {
            target_value: Some(-1.0),
            ..TuningConfig::default()
        };

        assert!(matches!(
            config.validate(),
            Err(TuningConfigError::InvalidTargetValue(_))
        ));
    }

    #[test]
    fn profile_catalog_marks_manual_unavailable() {
        let manual = profile_catalog()
            .into_iter()
            .find(|profile| profile.name == TuningMode::Manual)
            .unwrap();

        assert!(!manual.available);
    }

    #[test]
    fn tuning_plan_keeps_voltage_trim_last() {
        let plan = TuningPlanResponse::from(TuningConfig {
            autotune: true,
            ..TuningConfig::default()
        });

        assert_eq!(plan.state, TuningPlanState::PlannedReadOnly);
        assert_eq!(plan.active_phase, Some(TuningPhase::Baseline));
        assert!(!plan.writable);
        assert_eq!(plan.steps.last().unwrap().phase, TuningPhase::VoltageTrim);
        assert!(plan.guardrails.voltage_trim_requires_stable_upclock);
        assert!(plan.guardrails.rollback_on_rejected_shares);
        assert!(
            plan.steps
                .windows(2)
                .all(|window| window[0].order < window[1].order)
        );
    }
}
