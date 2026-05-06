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
}
