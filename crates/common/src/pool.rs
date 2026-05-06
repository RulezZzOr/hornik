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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolConnectionPolicy {
    #[serde(default = "default_latency_warning_ms")]
    pub latency_warning_ms: u32,
    #[serde(default = "default_job_processing_budget_ms")]
    pub job_processing_budget_ms: u32,
    #[serde(default = "default_reconnect_min_interval_seconds")]
    pub reconnect_min_interval_seconds: u32,
    #[serde(default = "default_failover_cooldown_seconds")]
    pub failover_cooldown_seconds: u32,
    #[serde(default = "default_keepalive_interval_seconds")]
    pub keepalive_interval_seconds: u32,
}

impl Default for PoolConnectionPolicy {
    fn default() -> Self {
        Self {
            latency_warning_ms: 500,
            job_processing_budget_ms: 50,
            reconnect_min_interval_seconds: 15,
            failover_cooldown_seconds: 60,
            keepalive_interval_seconds: 30,
        }
    }
}

impl PoolConnectionPolicy {
    pub fn validate(self) -> Result<(), PoolConfigError> {
        if self.latency_warning_ms == 0 {
            return Err(PoolConfigError::InvalidConnectionPolicy(
                "latency_warning_ms must be greater than zero",
            ));
        }
        if self.job_processing_budget_ms == 0 {
            return Err(PoolConfigError::InvalidConnectionPolicy(
                "job_processing_budget_ms must be greater than zero",
            ));
        }
        if self.job_processing_budget_ms > self.latency_warning_ms {
            return Err(PoolConfigError::InvalidConnectionPolicy(
                "job_processing_budget_ms must not exceed latency_warning_ms",
            ));
        }
        if self.reconnect_min_interval_seconds == 0 {
            return Err(PoolConfigError::InvalidConnectionPolicy(
                "reconnect_min_interval_seconds must be greater than zero",
            ));
        }
        if self.failover_cooldown_seconds < self.reconnect_min_interval_seconds {
            return Err(PoolConfigError::InvalidConnectionPolicy(
                "failover_cooldown_seconds must be at least reconnect_min_interval_seconds",
            ));
        }
        if self.keepalive_interval_seconds == 0 {
            return Err(PoolConfigError::InvalidConnectionPolicy(
                "keepalive_interval_seconds must be greater than zero",
            ));
        }

        Ok(())
    }
}

fn default_latency_warning_ms() -> u32 {
    PoolConnectionPolicy::default().latency_warning_ms
}

fn default_job_processing_budget_ms() -> u32 {
    PoolConnectionPolicy::default().job_processing_budget_ms
}

fn default_reconnect_min_interval_seconds() -> u32 {
    PoolConnectionPolicy::default().reconnect_min_interval_seconds
}

fn default_failover_cooldown_seconds() -> u32 {
    PoolConnectionPolicy::default().failover_cooldown_seconds
}

fn default_keepalive_interval_seconds() -> u32 {
    PoolConnectionPolicy::default().keepalive_interval_seconds
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolRuntimeState {
    Unconfigured,
    ReadyNoConnection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolRuntimeSummary {
    pub state: PoolRuntimeState,
    pub active_priority: Option<u8>,
    pub active_latency_ms: Option<f64>,
    pub job_processing_p50_ms: Option<f64>,
    pub job_processing_p99_ms: Option<f64>,
    pub reconnects_total: u64,
    pub reconnect_suppressed_total: u64,
    pub stale_jobs_total: u64,
    pub policy: PoolConnectionPolicy,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolStrategyState {
    Unconfigured,
    PlannedNoConnection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolConnectionRole {
    Active,
    Failover,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolConnectionPlan {
    pub priority: u8,
    pub role: PoolConnectionRole,
    pub url: String,
    pub user: String,
    pub keepalive_interval_seconds: u32,
    pub reconnect_min_interval_seconds: u32,
    pub failover_cooldown_seconds: u32,
    pub latency_warning_ms: u32,
    pub job_processing_budget_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolStrategyResponse {
    pub state: PoolStrategyState,
    pub active_priority: Option<u8>,
    pub failover_priority_order: Vec<u8>,
    pub persistent_connection_required: bool,
    pub reconnect_jitter_allowed: bool,
    pub plans: Vec<PoolConnectionPlan>,
    pub notes: Vec<String>,
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

pub fn plan_pool_strategy(
    pools: &[PoolConfig],
    policy: PoolConnectionPolicy,
) -> PoolStrategyResponse {
    let mut enabled = pools.iter().filter(|pool| pool.enabled).collect::<Vec<_>>();
    enabled.sort_by_key(|pool| pool.priority);

    let active_priority = enabled.first().map(|pool| pool.priority);
    let state = if enabled.is_empty() {
        PoolStrategyState::Unconfigured
    } else {
        PoolStrategyState::PlannedNoConnection
    };

    PoolStrategyResponse {
        state,
        active_priority,
        failover_priority_order: enabled.iter().map(|pool| pool.priority).collect(),
        persistent_connection_required: active_priority.is_some(),
        reconnect_jitter_allowed: false,
        plans: enabled
            .into_iter()
            .enumerate()
            .map(|(index, pool)| PoolConnectionPlan {
                priority: pool.priority,
                role: if index == 0 {
                    PoolConnectionRole::Active
                } else {
                    PoolConnectionRole::Failover
                },
                url: pool.url.clone(),
                user: pool.user.clone(),
                keepalive_interval_seconds: policy.keepalive_interval_seconds,
                reconnect_min_interval_seconds: policy.reconnect_min_interval_seconds,
                failover_cooldown_seconds: policy.failover_cooldown_seconds,
                latency_warning_ms: policy.latency_warning_ms,
                job_processing_budget_ms: policy.job_processing_budget_ms,
            })
            .collect(),
        notes: vec![
            "build 0.1.0 does not open stratum sockets; this is the deterministic connection plan for the future engine".to_string(),
            "the active pool is held persistently and reconnect attempts are suppressed inside the minimum reconnect interval".to_string(),
            "failover follows enabled pool priority order and waits for the configured cooldown before changing target".to_string(),
        ],
    }
}

pub fn summarize_pool_runtime(
    pools: &[PoolConfig],
    policy: PoolConnectionPolicy,
) -> PoolRuntimeSummary {
    let summary = summarize_pools(pools);
    let state = if summary.enabled == 0 {
        PoolRuntimeState::Unconfigured
    } else {
        PoolRuntimeState::ReadyNoConnection
    };

    PoolRuntimeSummary {
        state,
        active_priority: summary.active_priority,
        active_latency_ms: None,
        job_processing_p50_ms: None,
        job_processing_p99_ms: None,
        reconnects_total: 0,
        reconnect_suppressed_total: 0,
        stale_jobs_total: 0,
        policy,
        notes: vec![
            "build 0.1.0 exposes pool latency and reconnect policy before stratum networking is enabled".to_string(),
            "future stratum engine must prefer low latency, fast job processing, and stable persistent connections".to_string(),
        ],
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
    #[error("invalid pool connection policy: {0}")]
    InvalidConnectionPolicy(&'static str),
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

    #[test]
    fn default_connection_policy_prioritizes_latency_and_reconnect_hygiene() {
        let policy = PoolConnectionPolicy::default();

        assert_eq!(policy.latency_warning_ms, 500);
        assert_eq!(policy.job_processing_budget_ms, 50);
        assert_eq!(policy.reconnect_min_interval_seconds, 15);
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn rejects_job_processing_budget_above_latency_warning() {
        let err = PoolConnectionPolicy {
            latency_warning_ms: 25,
            job_processing_budget_ms: 50,
            ..PoolConnectionPolicy::default()
        }
        .validate()
        .unwrap_err();

        assert!(matches!(err, PoolConfigError::InvalidConnectionPolicy(_)));
    }

    #[test]
    fn summarizes_pool_runtime_without_fake_latency() {
        let runtime = summarize_pool_runtime(&[pool(0, true)], PoolConnectionPolicy::default());

        assert_eq!(runtime.state, PoolRuntimeState::ReadyNoConnection);
        assert_eq!(runtime.active_priority, Some(0));
        assert_eq!(runtime.active_latency_ms, None);
        assert_eq!(runtime.reconnects_total, 0);
    }

    #[test]
    fn plans_pool_strategy_by_enabled_priority_without_jitter() {
        let strategy = plan_pool_strategy(
            &[pool(2, true), pool(0, false), pool(1, true)],
            PoolConnectionPolicy::default(),
        );

        assert_eq!(strategy.state, PoolStrategyState::PlannedNoConnection);
        assert_eq!(strategy.active_priority, Some(1));
        assert_eq!(strategy.failover_priority_order, vec![1, 2]);
        assert!(strategy.persistent_connection_required);
        assert!(!strategy.reconnect_jitter_allowed);
        assert_eq!(strategy.plans[0].role, PoolConnectionRole::Active);
        assert_eq!(strategy.plans[1].role, PoolConnectionRole::Failover);
        assert_eq!(strategy.plans[0].keepalive_interval_seconds, 30);
    }

    #[test]
    fn pool_strategy_is_unconfigured_without_enabled_pools() {
        let strategy = plan_pool_strategy(&[pool(0, false)], PoolConnectionPolicy::default());

        assert_eq!(strategy.state, PoolStrategyState::Unconfigured);
        assert_eq!(strategy.active_priority, None);
        assert!(strategy.plans.is_empty());
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
