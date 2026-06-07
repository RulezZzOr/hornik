use crate::{
    ContributionConfig, ContributionConfigError, PoolConfig, PoolConfigError, PoolConnectionPolicy,
    TuningConfig, TuningConfigError, validate_pools,
};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use thiserror::Error;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub contribution: ContributionConfig,
    #[serde(default)]
    pub tuning: TuningConfig,
    #[serde(default)]
    pub pools: Vec<PoolConfig>,
    #[serde(default)]
    pub pool_policy: PoolConnectionPolicy,
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
            .map_err(ConfigError::Contribution)?;
        self.tuning.validate().map_err(ConfigError::Tuning)?;
        self.pool_policy
            .validate()
            .map_err(ConfigError::PoolPolicy)?;
        validate_pools(&self.pools).map_err(ConfigError::Pools)
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
    #[error("invalid tuning config: {0}")]
    Tuning(#[source] TuningConfigError),
    #[error("invalid pool config: {0}")]
    Pools(#[source] PoolConfigError),
    #[error("invalid pool policy: {0}")]
    PoolPolicy(#[source] PoolConfigError),
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
        assert_eq!(config.tuning.mode, crate::TuningMode::StockLike);
        assert_eq!(config.pool_policy.latency_warning_ms, 500);
        assert!(config.pools.is_empty());
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

    #[test]
    fn parses_pool_config() {
        let config = RuntimeConfig::from_toml_str(
            r#"
            [[pools]]
            priority = 0
            url = "stratum+tcp://pool.example:3333"
            user = "acct.worker"
            password = "x"
            enabled = true
            "#,
        )
        .unwrap();

        assert_eq!(config.pools.len(), 1);
        assert_eq!(config.pools[0].priority, 0);
    }

    #[test]
    fn parses_tuning_config() {
        let config = RuntimeConfig::from_toml_str(
            r#"
            [tuning]
            mode = "eco"
            target_type = "watts"
            target_value = 2800
            autotune = false
            "#,
        )
        .unwrap();

        assert_eq!(config.tuning.mode, crate::TuningMode::Eco);
        assert_eq!(config.tuning.target_value, Some(2800.0));
    }

    #[test]
    fn rejects_manual_tuning_in_mvp() {
        let err = RuntimeConfig::from_toml_str(
            r#"
            [tuning]
            mode = "manual"
            "#,
        )
        .unwrap_err();

        assert!(matches!(err, ConfigError::Tuning(_)));
    }

    #[test]
    fn rejects_duplicate_pool_priorities() {
        let err = RuntimeConfig::from_toml_str(
            r#"
            [[pools]]
            priority = 0
            url = "stratum+tcp://pool-a.example:3333"
            user = "acct.worker"

            [[pools]]
            priority = 0
            url = "stratum+tcp://pool-b.example:3333"
            user = "acct.worker"
            "#,
        )
        .unwrap_err();

        assert!(matches!(err, ConfigError::Pools(_)));
    }

    #[test]
    fn rejects_pool_password_leak_fields() {
        let err = RuntimeConfig::from_toml_str(
            r#"
            [[pools]]
            priority = 0
            url = "stratum+tcp://pool.example:3333"
            user = "acct.worker"
            secret = "bad"
            "#,
        )
        .unwrap_err();

        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn parses_pool_connection_policy() {
        let config = RuntimeConfig::from_toml_str(
            r#"
            [pool_policy]
            latency_warning_ms = 250
            job_processing_budget_ms = 25
            reconnect_min_interval_seconds = 20
            failover_cooldown_seconds = 90
            keepalive_interval_seconds = 15
            "#,
        )
        .unwrap();

        assert_eq!(config.pool_policy.latency_warning_ms, 250);
        assert_eq!(config.pool_policy.job_processing_budget_ms, 25);
        assert_eq!(config.pool_policy.reconnect_min_interval_seconds, 20);
    }

    #[test]
    fn rejects_invalid_pool_connection_policy() {
        let err = RuntimeConfig::from_toml_str(
            r#"
            [pool_policy]
            latency_warning_ms = 25
            job_processing_budget_ms = 50
            "#,
        )
        .unwrap_err();

        assert!(matches!(err, ConfigError::PoolPolicy(_)));
    }
}
