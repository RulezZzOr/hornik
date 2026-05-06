use openmineros_asic_backend::{BackendError, SimulatedBackend};
use openmineros_common::status::HealthStatusResponse;
use openmineros_common::{
    BoardFamily, ChainStatus, ContributionConfig, ContributionStatus, HealthStatus, MinerStatus,
    Model, RuntimeConfig, Severity, SystemInfo,
};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct Supervisor {
    backend: SimulatedBackend,
    booted_at: Instant,
    active_slot: String,
    contribution: ContributionConfig,
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
        })
    }

    pub fn system_info(&self) -> SystemInfo {
        SystemInfo {
            model: self.backend.model(),
            board_family: self.backend.profile().family,
            firmware_version: env!("CARGO_PKG_VERSION").to_string(),
            active_slot: self.active_slot.clone(),
            uptime_seconds: self.booted_at.elapsed().as_secs(),
            serial: None,
            capabilities: self.backend.profile().capabilities.clone(),
        }
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
        self.backend.miner_status()
    }

    pub fn chains(&self) -> Vec<ChainStatus> {
        self.backend.chain_statuses()
    }

    pub fn contribution_status(&self) -> ContributionStatus {
        ContributionStatus::from(self.contribution)
    }
}
