use openmineros_asic_backend::{BackendError, BackendHandle};
use openmineros_common::status::HealthStatusResponse;
use openmineros_common::{
    BoardFamily, ChainStatus, ContributionConfig, ContributionStatus, EventBuilder, EventSeverity,
    EventsResponse, HardwareProbeReport, HealthStatus, MinerStatus, Model, PoolConfig, PoolSummary,
    ProfilesResponse, RuntimeBackendMode, RuntimeConfig, Severity, SupportBundle,
    SupportBundlePrivacy, SystemInfo, TuningConfig, UpdateStatus, summarize_pools,
};
use serde_json::json;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct Supervisor {
    backend: BackendHandle,
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
        Self::with_backend_mode(model, board, config, RuntimeBackendMode::Simulated)
    }

    pub fn with_backend_mode(
        model: Model,
        board: BoardFamily,
        config: RuntimeConfig,
        backend_mode: RuntimeBackendMode,
    ) -> Result<Self, BackendError> {
        Ok(Self {
            backend: BackendHandle::new(backend_mode, model, board)?,
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
            backend: self.backend.mode(),
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
        let runtime = self.backend.runtime_status();
        let mut issues = Vec::new();
        let support_severity = match support {
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
        issues.extend(runtime.issues);
        let severity = max_severity(support_severity, runtime.severity);

        HealthStatusResponse {
            state: runtime.state,
            severity,
            issues,
            active_slot: self.active_slot.clone(),
            rollback_available: true,
        }
    }

    pub fn miner_status(&self) -> MinerStatus {
        let mut status = self.backend.miner_status();
        if self.backend.mode() == RuntimeBackendMode::Simulated {
            status.mode = self.tuning.mode.into();
        }
        status
    }

    pub fn chains(&self) -> Vec<ChainStatus> {
        self.backend.chain_statuses()
    }

    pub fn hardware_probe_report(&self) -> HardwareProbeReport {
        self.backend.probe_report()
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
                "backend": system_info.backend,
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
                "backend": system_info.backend,
                "contribution_enabled": contribution.enabled,
                "contribution_rate_percent": contribution.rate_percent,
            }),
        );

        if self.backend.mode() == RuntimeBackendMode::HardwareProbe {
            events.push(
                EventSeverity::Warn,
                "backend.hardware_probe_scaffold",
                "asic-backend",
                "hardware probe backend is read-only in build 0.1.0",
                json!({
                    "backend": self.backend.mode(),
                    "model": system_info.model,
                    "board_family": system_info.board_family,
                }),
            );
        }

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

    pub fn dashboard_overview(&self) -> openmineros_common::DashboardOverview {
        openmineros_common::DashboardOverview {
            schema_version: openmineros_common::DashboardOverview::SCHEMA_VERSION,
            system: self.system_info(),
            health: self.health(),
            miner: self.miner_status(),
            chains: self.chains(),
            pools: self.pools(),
            profiles: self.profiles(),
            contribution: self.contribution_status(),
            update: self.update_status(),
            events: self.events(),
        }
    }

    pub fn prometheus_metrics(&self) -> String {
        let system = self.system_info();
        let health = self.health();
        let miner = self.miner_status();
        let contribution = self.contribution_status();
        let pools = self.pools();
        let profiles = self.profiles();
        let update = self.update_status();
        let mut output = String::new();

        metric(&mut output, "omo_miner_hashrate_ths", miner.hashrate_ths);
        metric(&mut output, "omo_miner_power_watts", miner.power_w);
        metric(
            &mut output,
            "omo_miner_efficiency_j_th",
            miner.efficiency_j_th,
        );
        metric(
            &mut output,
            "omo_miner_uptime_seconds",
            system.uptime_seconds,
        );
        metric(
            &mut output,
            "omo_shares_accepted_total",
            miner.accepted_shares,
        );
        metric(
            &mut output,
            "omo_shares_rejected_total",
            miner.rejected_shares,
        );
        labeled_metric(
            &mut output,
            "omo_miner_mode_active",
            &[("mode", miner_mode_label(miner.mode))],
            1,
        );

        labeled_metric(
            &mut output,
            "omo_system_health_state",
            &[("state", health_state_label(health.state))],
            1,
        );
        metric(
            &mut output,
            "omo_system_health_severity",
            severity_value(health.severity),
        );

        metric(
            &mut output,
            "omo_contribution_enabled",
            bool_value(contribution.enabled),
        );
        metric(
            &mut output,
            "omo_contribution_rate_percent",
            contribution.rate_percent,
        );
        metric(
            &mut output,
            "omo_contribution_target_locked",
            bool_value(contribution.target_locked),
        );

        metric(&mut output, "omo_pool_configured_total", pools.configured);
        metric(&mut output, "omo_pool_enabled_total", pools.enabled);
        metric(
            &mut output,
            "omo_pool_active_priority",
            pools.active_priority.map_or(-1_i32, i32::from),
        );

        labeled_metric(
            &mut output,
            "omo_tuning_profile_active",
            &[("profile", tuning_mode_label(profiles.active))],
            1,
        );
        for profile in profiles.profiles {
            labeled_metric(
                &mut output,
                "omo_tuning_profile_available",
                &[("profile", tuning_mode_label(profile.name))],
                bool_value(profile.available),
            );
        }

        metric(
            &mut output,
            "omo_update_rollback_available",
            bool_value(update.rollback_available),
        );
        metric(
            &mut output,
            "omo_update_boot_once_pending",
            bool_value(update.boot_once_pending),
        );
        for slot in update.slots {
            labeled_metric(
                &mut output,
                "omo_update_slot_bootable",
                &[
                    ("slot", &slot.name),
                    ("state", slot_state_label(slot.state)),
                ],
                bool_value(slot.bootable),
            );
        }

        for chain in self.chains() {
            let chain_id = chain.id.to_string();
            labeled_metric(
                &mut output,
                "omo_chain_up",
                &[("chain", &chain_id)],
                bool_value(chain.present && chain.enabled),
            );
            labeled_metric(
                &mut output,
                "omo_chain_asic_detected",
                &[("chain", &chain_id)],
                chain.asic_detected,
            );
            labeled_metric(
                &mut output,
                "omo_temp_board_celsius",
                &[("chain", &chain_id)],
                chain.temp_board_c,
            );
            labeled_metric(
                &mut output,
                "omo_temp_chip_max_celsius",
                &[("chain", &chain_id)],
                chain.temp_chip_max_c,
            );
        }

        output
    }
}

fn metric(output: &mut String, name: &str, value: impl std::fmt::Display) {
    output.push_str(&format!("{name} {value}\n"));
}

fn labeled_metric(
    output: &mut String,
    name: &str,
    labels: &[(&str, &str)],
    value: impl std::fmt::Display,
) {
    let labels = labels
        .iter()
        .map(|(key, value)| format!(r#"{key}="{}""#, escape_label_value(value)))
        .collect::<Vec<_>>()
        .join(",");
    output.push_str(&format!("{name}{{{labels}}} {value}\n"));
}

fn escape_label_value(value: &str) -> String {
    value.replace('\\', r"\\").replace('"', "\\\"")
}

fn bool_value(value: bool) -> u8 {
    u8::from(value)
}

fn max_severity(left: Severity, right: Severity) -> Severity {
    if severity_value(left) >= severity_value(right) {
        left
    } else {
        right
    }
}

fn severity_value(severity: Severity) -> u8 {
    match severity {
        Severity::Ok => 0,
        Severity::Warn => 1,
        Severity::Error => 2,
        Severity::Critical => 3,
    }
}

fn health_state_label(state: HealthStatus) -> &'static str {
    match state {
        HealthStatus::Booting => "booting",
        HealthStatus::Recovering => "recovering",
        HealthStatus::Idle => "idle",
        HealthStatus::Starting => "starting",
        HealthStatus::Mining => "mining",
        HealthStatus::Degraded => "degraded",
        HealthStatus::SafeMode => "safe_mode",
        HealthStatus::Updating => "updating",
        HealthStatus::RollbackPending => "rollback_pending",
        HealthStatus::Fault => "fault",
    }
}

fn miner_mode_label(mode: openmineros_common::MinerMode) -> &'static str {
    match mode {
        openmineros_common::MinerMode::StockLike => "stock_like",
        openmineros_common::MinerMode::Eco => "eco",
        openmineros_common::MinerMode::Balanced => "balanced",
        openmineros_common::MinerMode::Performance => "performance",
        openmineros_common::MinerMode::Manual => "manual",
        openmineros_common::MinerMode::SafeMode => "safe_mode",
    }
}

fn tuning_mode_label(mode: openmineros_common::TuningMode) -> &'static str {
    match mode {
        openmineros_common::TuningMode::StockLike => "stock_like",
        openmineros_common::TuningMode::Eco => "eco",
        openmineros_common::TuningMode::Balanced => "balanced",
        openmineros_common::TuningMode::Performance => "performance",
        openmineros_common::TuningMode::Manual => "manual",
        openmineros_common::TuningMode::SafeMode => "safe_mode",
    }
}

fn slot_state_label(state: openmineros_common::SlotState) -> &'static str {
    match state {
        openmineros_common::SlotState::Active => "active",
        openmineros_common::SlotState::Inactive => "inactive",
        openmineros_common::SlotState::Unknown => "unknown",
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

    #[test]
    fn hardware_probe_mode_reports_safe_read_only_state() {
        let supervisor = Supervisor::with_backend_mode(
            Model::S19jPro,
            BoardFamily::Xilinx,
            RuntimeConfig::default(),
            RuntimeBackendMode::HardwareProbe,
        )
        .unwrap();
        let info = supervisor.system_info();
        let health = supervisor.health();
        let miner = supervisor.miner_status();
        let chains = supervisor.chains();
        let events = supervisor.events();

        assert_eq!(info.backend, RuntimeBackendMode::HardwareProbe);
        assert_eq!(health.state, HealthStatus::Recovering);
        assert_eq!(health.severity, Severity::Warn);
        assert!(
            health
                .issues
                .iter()
                .any(|issue| issue.contains("read-only"))
        );
        assert_eq!(miner.hashrate_ths, 0.0);
        assert_eq!(miner.mode, openmineros_common::MinerMode::SafeMode);
        assert!(chains.iter().all(|chain| !chain.present));
        assert!(
            events
                .events
                .iter()
                .any(|event| event.event_type == "backend.hardware_probe_scaffold")
        );
    }

    #[test]
    fn hardware_probe_report_is_exposed_from_supervisor() {
        let supervisor = Supervisor::with_backend_mode(
            Model::S19jPro,
            BoardFamily::Xilinx,
            RuntimeConfig::default(),
            RuntimeBackendMode::HardwareProbe,
        )
        .unwrap();
        let report = supervisor.hardware_probe_report();

        assert_eq!(report.backend, RuntimeBackendMode::HardwareProbe);
        assert_eq!(report.model, Model::S19jPro);
        assert_eq!(report.board_family, BoardFamily::Xilinx);
        assert!(report.safe_read_only);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.interface == "uart" && check.path == "/dev/ttyPS0")
        );
    }

    #[test]
    fn dashboard_overview_redacts_pool_passwords() {
        let config = RuntimeConfig::from_toml_str(
            r#"
            [contribution]
            enabled = true
            rate_percent = 1.5

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
        let overview = supervisor.dashboard_overview();
        let serialized = serde_json::to_string(&overview).unwrap();

        assert_eq!(
            overview.schema_version,
            openmineros_common::DashboardOverview::SCHEMA_VERSION
        );
        assert_eq!(overview.pools.enabled, 1);
        assert!(overview.contribution.target_locked);
        assert!(overview.update.rollback_available);
        assert!(
            overview
                .events
                .events
                .iter()
                .any(|event| event.event_type == "pool.active_selected")
        );
        assert!(serialized.contains("\"password_set\":true"));
        assert!(!serialized.contains("super-secret"));
    }

    #[test]
    fn prometheus_metrics_do_not_expose_pool_secrets_or_addresses() {
        let config = RuntimeConfig::from_toml_str(
            r#"
            [contribution]
            enabled = true
            rate_percent = 1.0

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
        let metrics = supervisor.prometheus_metrics();

        assert!(metrics.contains("omo_miner_hashrate_ths"));
        assert!(metrics.contains("omo_pool_configured_total 1"));
        assert!(metrics.contains("omo_contribution_target_locked 1"));
        assert!(!metrics.contains("super-secret"));
        assert!(!metrics.contains("pool.example"));
        assert!(!metrics.contains("acct.worker"));
        assert!(!metrics.contains("bc1qp6d4vxmenug97ghcy027vsn3902yadcj77ka6j"));
    }
}
