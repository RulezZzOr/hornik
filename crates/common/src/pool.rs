use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolConfig {
    pub priority: u8,
    pub url: String,
    pub user: String,
    #[serde(default = "default_pool_password")]
    pub password: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

impl PoolConfig {
    pub fn validate(&self) -> Result<(), PoolConfigError> {
        if !self.url.starts_with("stratum+tcp://") && !self.url.starts_with("stratum+ssl://") {
            return Err(PoolConfigError::UnsupportedUrlScheme {
                priority: self.priority,
                url: self.url.clone(),
            });
        }

        if self.enabled && self.user.trim().is_empty() {
            return Err(PoolConfigError::MissingUser {
                priority: self.priority,
            });
        }

        Ok(())
    }

    pub fn redacted(&self, active: bool) -> PoolInfo {
        PoolInfo {
            priority: self.priority,
            url: self.url.clone(),
            user: self.user.clone(),
            enabled: self.enabled,
            active,
            password_set: !self.password.is_empty(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolInfo {
    pub priority: u8,
    pub url: String,
    pub user: String,
    pub enabled: bool,
    pub active: bool,
    pub password_set: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSummary {
    pub configured: usize,
    pub enabled: usize,
    pub active_priority: Option<u8>,
    pub pools: Vec<PoolInfo>,
}

pub fn summarize_pools(pools: &[PoolConfig]) -> PoolSummary {
    let active_priority = pools
        .iter()
        .filter(|pool| pool.enabled)
        .min_by_key(|pool| pool.priority)
        .map(|pool| pool.priority);

    PoolSummary {
        configured: pools.len(),
        enabled: pools.iter().filter(|pool| pool.enabled).count(),
        active_priority,
        pools: pools
            .iter()
            .map(|pool| pool.redacted(Some(pool.priority) == active_priority))
            .collect(),
    }
}

pub fn validate_pools(pools: &[PoolConfig]) -> Result<(), PoolConfigError> {
    let mut priorities = HashSet::new();

    for pool in pools {
        if !priorities.insert(pool.priority) {
            return Err(PoolConfigError::DuplicatePriority(pool.priority));
        }
        pool.validate()?;
    }

    Ok(())
}

fn default_pool_password() -> String {
    "x".to_string()
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Error)]
pub enum PoolConfigError {
    #[error("duplicate pool priority: {0}")]
    DuplicatePriority(u8),
    #[error("pool {priority} uses unsupported URL scheme: {url}")]
    UnsupportedUrlScheme { priority: u8, url: String },
    #[error("enabled pool {priority} is missing user")]
    MissingUser { priority: u8 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_lowest_enabled_priority_as_active() {
        let summary = summarize_pools(&[
            pool(1, true),
            PoolConfig {
                priority: 0,
                enabled: false,
                ..pool(0, false)
            },
            pool(2, true),
        ]);

        assert_eq!(summary.configured, 3);
        assert_eq!(summary.enabled, 2);
        assert_eq!(summary.active_priority, Some(1));
        assert!(summary.pools[0].active);
    }

    #[test]
    fn rejects_duplicate_priorities() {
        let err = validate_pools(&[pool(0, true), pool(0, true)]).unwrap_err();
        assert!(matches!(err, PoolConfigError::DuplicatePriority(0)));
    }

    #[test]
    fn rejects_non_stratum_urls() {
        let err = PoolConfig {
            url: "https://example.com".to_string(),
            ..pool(0, true)
        }
        .validate()
        .unwrap_err();

        assert!(matches!(err, PoolConfigError::UnsupportedUrlScheme { .. }));
    }

    #[test]
    fn redacted_pool_info_does_not_expose_password() {
        let info = PoolConfig {
            password: "secret".to_string(),
            ..pool(0, true)
        }
        .redacted(true);

        assert_eq!(info.priority, 0);
        assert!(info.password_set);
    }

    fn pool(priority: u8, enabled: bool) -> PoolConfig {
        PoolConfig {
            priority,
            url: "stratum+tcp://pool.example:3333".to_string(),
            user: format!("user.worker{priority}"),
            password: "x".to_string(),
            enabled,
        }
    }
}
