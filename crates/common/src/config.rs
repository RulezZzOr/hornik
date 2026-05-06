use crate::{ContributionConfig, ContributionConfigError};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use thiserror::Error;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub contribution: ContributionConfig,
}

impl RuntimeConfig {
    pub fn from_toml_str(input: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(input).map_err(ConfigError::Parse)?;
        config.validate()?;
        Ok(config)
    }

    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let input = fs::read_to_string(path).map_err(ConfigError::Read)?;
        let config: Self = toml::from_str(&input).map_err(ConfigError::Parse)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        self.contribution
            .validate()
            .map_err(ConfigError::Contribution)
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config: {0}")]
    Read(#[source] std::io::Error),
    #[error("failed to parse config: {0}")]
    Parse(#[source] toml::de::Error),
    #[error("invalid contribution config: {0}")]
    Contribution(#[source] ContributionConfigError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DEFAULT_CONTRIBUTION_BENEFICIARY, default_contribution_endpoints};

    #[test]
    fn default_config_has_zero_contribution() {
        let config = RuntimeConfig::from_toml_str("").unwrap();

        assert!(!config.contribution.enabled);
        assert_eq!(config.contribution.rate_percent, 0.0);
    }

    #[test]
    fn parses_allowed_contribution_fields() {
        let config = RuntimeConfig::from_toml_str(
            r#"
            [contribution]
            enabled = true
            rate_percent = 2.5
            "#,
        )
        .unwrap();

        assert!(config.contribution.enabled);
        assert_eq!(config.contribution.rate_percent, 2.5);
    }

    #[test]
    fn rejects_beneficiary_override() {
        let err = RuntimeConfig::from_toml_str(
            r#"
            [contribution]
            enabled = true
            rate_percent = 1.0
            beneficiary = "bc1qattacker"
            "#,
        )
        .unwrap_err();

        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn rejects_endpoint_override() {
        let err = RuntimeConfig::from_toml_str(
            r#"
            [contribution]
            enabled = true
            rate_percent = 1.0

            [[contribution.endpoints]]
            url = "stratum+tcp://attacker.example:3333"
            user = "bad.worker"
            password = "x"
            "#,
        )
        .unwrap_err();

        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn rejects_contribution_rate_above_three_percent() {
        let err = RuntimeConfig::from_toml_str(
            r#"
            [contribution]
            enabled = true
            rate_percent = 3.01
            "#,
        )
        .unwrap_err();

        assert!(matches!(err, ConfigError::Contribution(_)));
    }

    #[test]
    fn contribution_target_is_not_loaded_from_config() {
        let config = RuntimeConfig::from_toml_str(
            r#"
            [contribution]
            enabled = true
            rate_percent = 1.0
            "#,
        )
        .unwrap();
        let status = crate::ContributionStatus::from(config.contribution);

        assert_eq!(status.beneficiary, DEFAULT_CONTRIBUTION_BENEFICIARY);
        assert_eq!(status.endpoints, default_contribution_endpoints());
    }
}
