use openmineros_asic_backend::{BackendError, SimulatedBackend};
use openmineros_common::status::HealthStatusResponse;
use openmineros_common::{
    BoardFamily, ChainStatus, ContributionConfig, ContributionStatus, EventBuilder, EventSeverity,
    EventsResponse, HealthStatus, MinerStatus, Model, PoolConfig, PoolSummary, ProfilesResponse,
    RuntimeConfig, Severity, SupportBundle, SupportBundlePrivacy, SystemInfo, TuningConfig,
    UpdateStatus, summarize_pools,
};
use serde_json::json;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct Supervisor {
    backend: SimulatedBackend,
    booted_at: Instant,
    active_slot: String,
    contribution: ContributionConfig,
    tuning: TuningConfig,
    pools: Vec<PoolConfig>,
}

impl Supervisor {
    pub fn new(model: Model, board: BoardFamily) -> Result<Self, BackendError> {
        Self::with_config(model, board, RuntimeConfig::default())
    }

    pub fn with_config(
        model: Model,
        board: BoardFamily,
        config: RuntimeConfig,
    ) -> Result<Self, BackendError> {
        Ok(Self {
            backend: SimulatedBackend::new(model, board)?,
            booted_at: Instant::now(),
            active_slot: "slot_a".to_string(),
            contribution: config.contribution,
            tuning: config.tuning,
            pools: config.pools,
        })
    }

    pub fn system_info(&self) -> SystemInfo {
        SystemInfo {
            model: self.backend.model(),
            board_family: self.backend.profile().family,
            firmware_version: env!("CARGO_PKG_VERSION").to_string(),
            active_slot: self.active_slot.clone(),
            uptime_seconds: self.uptime_seconds(),
            serial: None,
            capabilities: self.backend.profile().capabilities.clone(),
        }
    }

    fn uptime_seconds(&self) -> u64 {
        self.booted_at.elapsed().as_secs()
    }

    pub fn health(&self) -> HealthStatusResponse {
        let support = self.backend.support();
        let mut issues = Vec::new();
        let severity = match support {
            openmineros_common::SupportLevel::MvpStable => Severity::Ok,
            openmineros_common::SupportLevel::Experimental => {
                issues.push("target is experimental in build 0.1.0".to_string());
                Severity::Warn
            }
            openmineros_common::SupportLevel::Unsupported => {
                issues.push("target is unsupported in build 0.1.0".to_string());
                Severity::Error
            }
        };

        HealthStatusResponse {
            state: HealthStatus::Mining,
            severity,
            issues,
            active_slot: self.active_slot.clone(),
            rollback_available: true,
        }
    }

    pub fn miner_status(&self) -> MinerStatus {
        let mut status = self.backend.miner_status();
        status.mode = self.tuning.mode.into();
        status
    }

    pub fn chains(&self) -> Vec<ChainStatus> {
        self.backend.chain_statuses()
    }

    pub fn contribution_status(&self) -> ContributionStatus {
        ContributionStatus::from(self.contribution)
    }

    pub fn pools(&self) -> PoolSummary {
        summarize_pools(&self.pools)
    }

    pub fn profiles(&self) -> ProfilesResponse {
        ProfilesResponse::from(self.tuning)
    }

    pub fn events(&self) -> EventsResponse {
        let mut events = EventBuilder::new(self.uptime_seconds());
        let system_info = self.system_info();
        let health = self.health();
        let pools = self.pools();
        let contribution = self.contribution_status();
        let profiles = self.profiles();

        events.push(
            EventSeverity::Info,
            "boot.completed",
            "supervisor",
            "supervisor initialized",
            json!({
                "model": system_info.model,
                "board_family": system_info.board_family,
                "firmware_version": system_info.firmware_version,
                "active_slot": system_info.active_slot,
            }),
        );

        events.push(
            EventSeverity::Info,
            "config.loaded",
            "config",
            "runtime config loaded",
            json!({
                "pool_count": pools.configured,
                "enabled_pool_count": pools.enabled,
                "tuning_mode": profiles.active,
                "contribution_enabled": contribution.enabled,
                "contribution_rate_percent": contribution.rate_percent,
            }),
        );

        events.push(
            EventSeverity::Info,
            "contribution.target_locked",
            "contribution",
            "official contribution target is locked",
            json!({
                "target_locked": contribution.target_locked,
                "mutable_fields": contribution.mutable_fields,
                "beneficiary": contribution.beneficiary,
            }),
        );

        if pools.enabled == 0 {
            events.push(
                EventSeverity::Warn,
                "pool.unconfigured",
                "pool",
                "no enabled user pool is configured",
                json!({
                    "configured": pools.configured,
                    "enabled": pools.enabled,
                }),
            );
        } else {
            events.push(
                EventSeverity::Info,
                "pool.active_selected",
                "pool",
                "active pool selected by priority",
                json!({
                    "active_priority": pools.active_priority,
                    "enabled": pools.enabled,
                }),
            );
        }

        for issue in health.issues {
            events.push(
                EventSeverity::Warn,
                "system.health_issue",
                "supervisor",
                issue,
                json!({
                    "state": health.state,
                    "severity": health.severity,
                }),
            );
        }

        events.finish()
    }

    pub fn support_bundle(&self) -> SupportBundle {
        SupportBundle {
            schema_version: SupportBundle::SCHEMA_VERSION,
            generated_uptime_seconds: self.uptime_seconds(),
            privacy: SupportBundlePrivacy::default(),
            system: self.system_info(),
            health: self.health(),
            miner: self.miner_status(),
            chains: self.chains(),
            pools: self.pools(),
            profiles: self.profiles(),
            contribution: self.contribution_status(),
            events: self.events(),
        }
    }

    pub fn update_status(&self) -> UpdateStatus {
        UpdateStatus::development_default(self.active_slot.clone(), env!("CARGO_PKG_VERSION"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openmineros_common::{TuningMode, TuningTargetType};

    #[test]
    fn emits_pool_unconfigured_event_without_pools() {
        let supervisor = Supervisor::with_config(
            Model::S19jPro,
            BoardFamily::Xilinx,
            RuntimeConfig::default(),
        )
        .unwrap();
        let events = supervisor.events();

        assert!(
            events
                .events
                .iter()
                .any(|event| event.event_type == "pool.unconfigured")
        );
        assert!(
            events
                .events
                .iter()
                .any(|event| event.event_type == "contribution.target_locked")
        );
    }

    #[test]
    fn emits_active_pool_event_when_pool_is_enabled() {
        let config = RuntimeConfig::from_toml_str(
            r#"
            [tuning]
            mode = "eco"
            target_type = "watts"
            target_value = 2800

            [[pools]]
            priority = 0
            url = "stratum+tcp://pool.example:3333"
            user = "acct.worker"
            password = "x"
            enabled = true
            "#,
        )
        .unwrap();
        assert_eq!(config.tuning.mode, TuningMode::Eco);
        assert_eq!(config.tuning.target_type, TuningTargetType::Watts);

        let supervisor =
            Supervisor::with_config(Model::S19jPro, BoardFamily::Xilinx, config).unwrap();
        let events = supervisor.events();

        assert!(
            events
                .events
                .iter()
                .any(|event| event.event_type == "pool.active_selected")
        );
        assert!(
            !events
                .events
                .iter()
                .any(|event| event.event_type == "pool.unconfigured")
        );
    }

    #[test]
    fn support_bundle_redacts_pool_passwords() {
        let config = RuntimeConfig::from_toml_str(
            r#"
            [[pools]]
            priority = 0
            url = "stratum+tcp://pool.example:3333"
            user = "acct.worker"
            password = "super-secret"
            enabled = true
            "#,
        )
        .unwrap();

        let supervisor =
            Supervisor::with_config(Model::S19jPro, BoardFamily::Xilinx, config).unwrap();
        let bundle = supervisor.support_bundle();
        let serialized = serde_json::to_string(&bundle).unwrap();

        assert!(bundle.privacy.pool_passwords_redacted);
        assert!(serialized.contains("\"password_set\":true"));
        assert!(!serialized.contains("super-secret"));
    }

    #[test]
    fn update_status_exposes_ab_slots_without_mutation() {
        let supervisor = Supervisor::with_config(
            Model::S19jPro,
            BoardFamily::Xilinx,
            RuntimeConfig::default(),
        )
        .unwrap();
        let status = supervisor.update_status();

        assert_eq!(status.update_model, "a_b");
        assert_eq!(status.active_slot, "slot_a");
        assert_eq!(status.inactive_slot, "slot_b");
        assert!(!status.boot_once_pending);
    }
}
