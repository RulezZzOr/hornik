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

/// A concrete operating point the executor applies and may lock or roll back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuningProfile {
    pub frequency_mhz: u16,
    pub voltage_mv: u16,
}

/// Measured chain behaviour after holding a tuning step for its dwell time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StepMeasurement {
    pub hashrate_ths: f64,
    pub hw_error_rate_percent: f64,
    pub chip_temp_max_c: f64,
    /// Whether the pool rejected shares produced at this step.
    pub rejected_shares: bool,
}

/// What recording a measurement did to the executor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum TuningOutcome {
    /// Step accepted; hold the chain at `next` for the next dwell.
    Advance { next: TuningProfile },
    /// All steps accepted; `profile` is the locked best operating point.
    Locked { profile: TuningProfile },
    /// A guardrail tripped; the chain was rolled back to the last safe profile.
    RolledBack { safe: TuningProfile, reason: String },
}

/// Lifecycle state of a [`TuningExecutor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TuningExecutorState {
    Running,
    Locked,
    RolledBack,
}

/// Discover-then-lock tuning executor with always-available rollback.
///
/// It walks a sequence of [`TuningExecutionStep`]s (baseline -> downclock ->
/// upclock -> voltage trim). After each step the operator records a
/// [`StepMeasurement`]; a step that breaches the [`TuningGuardrails`] (thermal,
/// HW error rate, rejected shares) or regresses hashrate during the upclock
/// phase causes an immediate rollback to the last known-good profile. Only after
/// every step passes does the executor lock the final profile, so the runtime
/// can always fall back to a safe operating point.
#[derive(Debug, Clone)]
pub struct TuningExecutor {
    steps: Vec<TuningExecutionStep>,
    guardrails: TuningGuardrails,
    index: usize,
    baseline_hashrate_ths: f64,
    last_known_good: TuningProfile,
    state: TuningExecutorState,
}

impl TuningExecutor {
    /// Start from a known-good `baseline` profile and its measured hashrate.
    pub fn new(
        baseline: TuningProfile,
        baseline_hashrate_ths: f64,
        steps: Vec<TuningExecutionStep>,
        guardrails: TuningGuardrails,
    ) -> Self {
        Self {
            steps,
            guardrails,
            index: 0,
            baseline_hashrate_ths,
            last_known_good: baseline,
            state: TuningExecutorState::Running,
        }
    }

    pub fn state(&self) -> TuningExecutorState {
        self.state
    }

    /// The last profile proven safe (the baseline until a step is accepted).
    pub fn last_known_good(&self) -> TuningProfile {
        self.last_known_good
    }

    /// The profile the current step asks the chain to hold, if still running.
    pub fn current_target(&self) -> Option<TuningProfile> {
        if self.state != TuningExecutorState::Running {
            return None;
        }
        self.steps.get(self.index).map(|step| TuningProfile {
            frequency_mhz: step.target_frequency_mhz,
            voltage_mv: step.target_voltage_mv,
        })
    }

    /// Record the measurement for the current step and advance, lock, or roll back.
    pub fn record(&mut self, measurement: StepMeasurement) -> TuningOutcome {
        let Some(step) = self.steps.get(self.index).cloned() else {
            // Nothing left to run: already locked at the last known-good profile.
            return TuningOutcome::Locked {
                profile: self.last_known_good,
            };
        };
        let target = TuningProfile {
            frequency_mhz: step.target_frequency_mhz,
            voltage_mv: step.target_voltage_mv,
        };

        if let Some(reason) = self.guardrail_breach(step.phase, &measurement) {
            self.state = TuningExecutorState::RolledBack;
            return TuningOutcome::RolledBack {
                safe: self.last_known_good,
                reason,
            };
        }

        // Step passed: it becomes the new known-good point.
        self.last_known_good = target;
        self.index += 1;

        match self.current_target() {
            Some(next) => TuningOutcome::Advance { next },
            None => {
                self.state = TuningExecutorState::Locked;
                TuningOutcome::Locked {
                    profile: self.last_known_good,
                }
            }
        }
    }

    /// Return the first guardrail a measurement breaches, if any.
    fn guardrail_breach(&self, phase: TuningPhase, m: &StepMeasurement) -> Option<String> {
        if m.chip_temp_max_c > self.guardrails.max_chip_temp_c {
            return Some(format!(
                "chip temp {:.1}C exceeds tuning limit {:.1}C",
                m.chip_temp_max_c, self.guardrails.max_chip_temp_c
            ));
        }
        if m.hw_error_rate_percent > self.guardrails.max_hw_error_rate_percent {
            return Some(format!(
                "hw error rate {:.3}% exceeds limit {:.3}%",
                m.hw_error_rate_percent, self.guardrails.max_hw_error_rate_percent
            ));
        }
        if self.guardrails.rollback_on_rejected_shares && m.rejected_shares {
            return Some("pool rejected shares at this step".to_string());
        }
        // The upclock phase must not regress hashrate below the baseline.
        if phase == TuningPhase::UpclockStability && m.hashrate_ths < self.baseline_hashrate_ths {
            return Some(format!(
                "upclock hashrate {:.2} TH/s regressed below baseline {:.2} TH/s",
                m.hashrate_ths, self.baseline_hashrate_ths
            ));
        }
        None
    }
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

    fn step(order: u8, phase: TuningPhase, freq: u16, volt: u16) -> TuningExecutionStep {
        TuningExecutionStep {
            order,
            phase,
            command: "set".to_string(),
            target_frequency_mhz: freq,
            target_voltage_mv: volt,
            min_duration_seconds: 300,
        }
    }

    fn sweep_steps() -> Vec<TuningExecutionStep> {
        vec![
            step(1, TuningPhase::DownclockEfficiency, 520, 1420),
            step(2, TuningPhase::UpclockStability, 540, 1420),
            step(3, TuningPhase::VoltageTrim, 540, 1410),
        ]
    }

    fn good() -> StepMeasurement {
        StepMeasurement {
            hashrate_ths: 110.0,
            hw_error_rate_percent: 0.0,
            chip_temp_max_c: 70.0,
            rejected_shares: false,
        }
    }

    fn executor() -> TuningExecutor {
        TuningExecutor::new(
            TuningProfile {
                frequency_mhz: 525,
                voltage_mv: 1420,
            },
            104.0,
            sweep_steps(),
            TuningGuardrails::default(),
        )
    }

    #[test]
    fn all_passing_steps_lock_the_final_profile() {
        let mut exec = executor();
        assert_eq!(
            exec.current_target().unwrap(),
            TuningProfile { frequency_mhz: 520, voltage_mv: 1420 }
        );
        assert!(matches!(exec.record(good()), TuningOutcome::Advance { .. }));
        assert!(matches!(exec.record(good()), TuningOutcome::Advance { .. }));
        let outcome = exec.record(good());
        assert_eq!(
            outcome,
            TuningOutcome::Locked {
                profile: TuningProfile { frequency_mhz: 540, voltage_mv: 1410 }
            }
        );
        assert_eq!(exec.state(), TuningExecutorState::Locked);
        assert_eq!(exec.current_target(), None);
    }

    #[test]
    fn thermal_breach_rolls_back_to_last_known_good() {
        let mut exec = executor();
        // First step passes -> known-good becomes 520/1420.
        assert!(matches!(exec.record(good()), TuningOutcome::Advance { .. }));
        let hot = StepMeasurement {
            chip_temp_max_c: 95.0,
            ..good()
        };
        let outcome = exec.record(hot);
        assert_eq!(
            outcome,
            TuningOutcome::RolledBack {
                safe: TuningProfile { frequency_mhz: 520, voltage_mv: 1420 },
                reason: "chip temp 95.0C exceeds tuning limit 85.0C".to_string(),
            }
        );
        assert_eq!(exec.state(), TuningExecutorState::RolledBack);
        assert_eq!(exec.current_target(), None);
    }

    #[test]
    fn upclock_hashrate_regression_rolls_back() {
        let mut exec = executor();
        exec.record(good()); // downclock ok
        let regressed = StepMeasurement {
            hashrate_ths: 100.0, // below baseline 104
            ..good()
        };
        assert!(matches!(
            exec.record(regressed),
            TuningOutcome::RolledBack { .. }
        ));
    }

    #[test]
    fn rejected_shares_roll_back_when_guardrail_enabled() {
        let mut exec = executor();
        let rejected = StepMeasurement {
            rejected_shares: true,
            ..good()
        };
        match exec.record(rejected) {
            TuningOutcome::RolledBack { safe, .. } => {
                assert_eq!(safe, TuningProfile { frequency_mhz: 525, voltage_mv: 1420 });
            }
            other => panic!("expected rollback, got {other:?}"),
        }
    }

    #[test]
    fn hw_error_rate_breach_rolls_back() {
        let mut exec = executor();
        let errors = StepMeasurement {
            hw_error_rate_percent: 1.0,
            ..good()
        };
        assert!(matches!(
            exec.record(errors),
            TuningOutcome::RolledBack { .. }
        ));
    }
}
