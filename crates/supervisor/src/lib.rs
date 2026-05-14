use openmineros_asic_backend::{
    AsicJobDispatcher, BackendError, BackendHandle, build_tuning_execution_steps,
    build_tuning_protocol_frames,
};
use openmineros_common::status::HealthStatusResponse;
use openmineros_common::{
    BoardFamily, ChainStatus, ContributionConfig, ContributionStatus, EventBuilder, EventSeverity,
    EventsResponse, HardwareIdentityReport, HardwareProbeReport, HardwareReadinessReport,
    HardwareReadinessState, HardwareSafetyGate, HealthStatus, JobPipelinePolicy, MinerStatus,
    Model, PoolConfig, PoolConnectionPolicy, PoolRuntimeState, PoolRuntimeSummary,
    PoolStrategyResponse, PoolSummary, ProfilesResponse, RuntimeBackendMode, RuntimeConfig,
    Severity, SharePrecheckResult, ShareValidationMode, StratumConnectionState, StratumEngineState,
    StratumEngineStatus, StratumMessageKind, StratumShareCandidate, StratumSubmitPolicy,
    SupportBundle, SupportBundlePrivacy, SystemInfo, TuningConfig, TuningExecutionState,
    TuningExecutionStatus, TuningPhase, TuningPlanResponse, TuningProtocolSequenceSpec,
    TuningProtocolTranscript, UpdateStatus, classify_stratum_message, evaluate_hardware_readiness,
    evaluate_hardware_safety, plan_pool_strategy, precheck_share_submit, summarize_pool_runtime,
    summarize_pools,
};
use serde_json::json;
use std::{
    io::{BufRead, BufReader, Write},
    net::{Shutdown, TcpStream},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct Supervisor {
    backend: BackendHandle,
    booted_at: Instant,
    active_slot: String,
    contribution: ContributionConfig,
    tuning: TuningConfig,
    pools: Vec<PoolConfig>,
    pool_policy: PoolConnectionPolicy,
    stratum_engine: Mutex<StratumEngine>,
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
        let pool_policy = config.pool_policy;
        let pools = config.pools;
        let job_pipeline = JobPipelinePolicy::from(pool_policy);
        let pool_strategy = plan_pool_strategy(&pools, pool_policy);
        let backend = BackendHandle::new(backend_mode, model, board)?;
        let dispatcher: Arc<dyn AsicJobDispatcher + Send + Sync> = Arc::new(backend.clone());
        let stratum_engine = StratumEngine::new(
            pools.clone(),
            job_pipeline,
            pool_policy,
            backend_mode,
            pool_strategy.active_priority,
            dispatcher.clone(),
        );
        Ok(Self {
            backend,
            booted_at: Instant::now(),
            active_slot: "slot_a".to_string(),
            contribution: config.contribution,
            tuning: config.tuning,
            pools,
            pool_policy,
            stratum_engine: Mutex::new(stratum_engine),
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

    pub fn hardware_identity_report(&self) -> HardwareIdentityReport {
        self.backend.identity_report()
    }

    pub fn hardware_safety_gate(&self) -> HardwareSafetyGate {
        evaluate_hardware_safety(
            self.backend.mode(),
            self.backend.support(),
            &self.hardware_identity_report(),
        )
    }

    pub fn hardware_readiness_report(&self) -> HardwareReadinessReport {
        evaluate_hardware_readiness(
            self.backend.mode(),
            self.backend.model(),
            self.backend.profile(),
            self.backend.support(),
            &self.hardware_safety_gate(),
        )
    }

    pub fn contribution_status(&self) -> ContributionStatus {
        ContributionStatus::from(self.contribution)
    }

    pub fn pools(&self) -> PoolSummary {
        summarize_pools(&self.pools)
    }

    pub fn pool_runtime(&self) -> PoolRuntimeSummary {
        let mut runtime = summarize_pool_runtime(&self.pools, self.pool_policy);
        let safety = self.hardware_safety_gate();
        let mut engine = self
            .stratum_engine
            .lock()
            .expect("stratum engine mutex poisoned");
        engine.poll(
            self.backend.mode() == RuntimeBackendMode::HardwareMining,
            safety.hardware_mining_allowed,
        );
        runtime.active_priority = engine.status.active_pool_priority;
        runtime.reconnects_total = engine.reconnects_total;
        runtime.reconnect_suppressed_total = engine.reconnect_suppressed_total;
        runtime.stale_jobs_total = engine.stale_jobs_total;
        runtime.active_latency_ms = engine.last_connect_latency_ms;
        if engine.status.socket_open {
            runtime.state = PoolRuntimeState::LiveConnection;
        }
        runtime
    }

    pub fn pool_strategy(&self) -> PoolStrategyResponse {
        plan_pool_strategy(&self.pools, self.pool_policy)
    }

    pub fn job_pipeline(&self) -> JobPipelinePolicy {
        JobPipelinePolicy::from(self.pool_policy)
    }

    pub fn stratum_status(&self) -> StratumEngineStatus {
        let safety = self.hardware_safety_gate();
        let mut engine = self
            .stratum_engine
            .lock()
            .expect("stratum engine mutex poisoned");
        engine.poll(
            self.backend.mode() == RuntimeBackendMode::HardwareMining,
            safety.hardware_mining_allowed,
        );
        engine.status.clone()
    }

    pub fn stratum_submit_policy(&self) -> StratumSubmitPolicy {
        self.stratum_status().submit_policy
    }

    pub fn submit_share(&self, candidate: StratumShareCandidate) -> SharePrecheckResult {
        let safety = self.hardware_safety_gate();
        let mut engine = self
            .stratum_engine
            .lock()
            .expect("stratum engine mutex poisoned");
        engine.poll(
            self.backend.mode() == RuntimeBackendMode::HardwareMining,
            safety.hardware_mining_allowed,
        );
        engine.submit_share(candidate)
    }

    pub fn profiles(&self) -> ProfilesResponse {
        ProfilesResponse::from(self.tuning)
    }

    pub fn tuning_plan(&self) -> TuningPlanResponse {
        TuningPlanResponse::from(self.tuning)
    }

    pub fn tuning_transcript(&self) -> TuningProtocolTranscript {
        let plan = self.tuning_plan();
        let board = self.backend.profile().family;
        let chip_id = 0;
        let base_frequency_mhz = 725;
        let base_voltage_mv = 800;
        let frequency_step_mhz = plan.guardrails.chip_frequency_step_mhz;
        let voltage_step_mv = plan.guardrails.voltage_step_mv;
        let phases: Vec<TuningPhase> = plan.steps.iter().map(|step| step.phase).collect();
        let frames = build_tuning_protocol_frames(&TuningProtocolSequenceSpec {
            board_family: board,
            chip_id,
            phases,
            base_frequency_mhz,
            base_voltage_mv,
            frequency_step_mhz,
            voltage_step_mv,
            min_step_duration_seconds: plan.guardrails.min_step_duration_seconds,
        });

        TuningProtocolTranscript {
            schema_version: 1,
            board_family: board,
            chip_id,
            base_frequency_mhz,
            base_voltage_mv,
            frequency_step_mhz,
            voltage_step_mv,
            frames,
            notes: vec![
                "read-only tuning transcript shows how chip-by-chip OC commands would be framed"
                    .to_string(),
                "baseline, downclock efficiency, upclock stability, then voltage trim".to_string(),
                "actual hardware writes remain gated until the tuning executor is implemented"
                    .to_string(),
            ],
        }
    }

    pub fn tuning_execution_status(&self) -> TuningExecutionStatus {
        let plan = self.tuning_plan();
        let safety = self.hardware_safety_gate();
        let transcript = self.tuning_transcript();
        let write_allowed = safety.tuning_writes_allowed;
        let state = if !self.tuning.autotune {
            TuningExecutionState::Disabled
        } else if !safety.configured_target_accepted
            || matches!(
                safety.state,
                openmineros_common::HardwareSafetyState::BlockedIdentityConflict
                    | openmineros_common::HardwareSafetyState::BlockedUnknownIdentity
                    | openmineros_common::HardwareSafetyState::BlockedUnsupportedTarget
            )
        {
            TuningExecutionState::Blocked
        } else if write_allowed {
            TuningExecutionState::ReadyToArm
        } else {
            TuningExecutionState::PlannedReadOnly
        };

        let steps = build_tuning_execution_steps(&TuningProtocolSequenceSpec {
            board_family: self.backend.profile().family,
            chip_id: 0,
            phases: plan.steps.iter().map(|step| step.phase).collect(),
            base_frequency_mhz: transcript.base_frequency_mhz,
            base_voltage_mv: transcript.base_voltage_mv,
            frequency_step_mhz: transcript.frequency_step_mhz,
            voltage_step_mv: transcript.voltage_step_mv,
            min_step_duration_seconds: plan.guardrails.min_step_duration_seconds,
        });
        let current_step = steps.first().cloned();
        let queued_steps = if steps.len() > 1 {
            steps.into_iter().skip(1).collect()
        } else {
            Vec::new()
        };

        TuningExecutionStatus {
            schema_version: TuningExecutionStatus::SCHEMA_VERSION,
            state,
            autotune: self.tuning.autotune,
            active_phase: plan.active_phase,
            current_step,
            queued_steps,
            write_allowed,
            blocked_reason: match state {
                TuningExecutionState::Disabled => {
                    Some("autotune is disabled in the active config".to_string())
                }
                TuningExecutionState::Blocked => {
                    Some(safety.reasons.first().cloned().unwrap_or_else(|| {
                        "tuning executor is blocked by safety policy".to_string()
                    }))
                }
                TuningExecutionState::PlannedReadOnly => Some(
                    "tuning executor is planned but write paths remain gated in build 0.1.0"
                        .to_string(),
                ),
                TuningExecutionState::ReadyToArm => None,
            },
            notes: vec![
                "execution status is derived from the tuning plan, transcript, and safety gate"
                    .to_string(),
                "build 0.1.0 exposes the sequence read-only until tuning writes are implemented"
                    .to_string(),
            ],
        }
    }

    pub fn events(&self) -> EventsResponse {
        let mut events = EventBuilder::new(self.uptime_seconds());
        let system_info = self.system_info();
        let identity = self.hardware_identity_report();
        let safety = self.hardware_safety_gate();
        let readiness = self.hardware_readiness_report();
        let health = self.health();
        let pools = self.pools();
        let pool_runtime = self.pool_runtime();
        let pool_strategy = self.pool_strategy();
        let job_pipeline = self.job_pipeline();
        let stratum = self.stratum_status();
        let contribution = self.contribution_status();
        let profiles = self.profiles();
        let tuning_plan = self.tuning_plan();
        let tuning_execution = self.tuning_execution_status();

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
            "hardware.identity_evaluated",
            "hardware",
            "hardware identity evaluated from configured target and read-only evidence",
            json!({
                "state": identity.state,
                "confidence": identity.confidence,
                "configured_board": identity.configured_board,
                "configured_model": identity.configured_model,
                "detected_board": identity.detected_board,
                "detected_model": identity.detected_model,
                "evidence_count": identity.evidence.len(),
                "safe_read_only": identity.safe_read_only,
            }),
        );

        events.push(
            if safety.hardware_mining_allowed || safety.simulated_mining_allowed {
                EventSeverity::Info
            } else {
                EventSeverity::Warn
            },
            "hardware.safety_gate_evaluated",
            "hardware",
            "hardware action safety gate evaluated",
            json!({
                "state": safety.state,
                "configured_target_accepted": safety.configured_target_accepted,
                "identity_confirmed": safety.identity_confirmed,
                "simulated_mining_allowed": safety.simulated_mining_allowed,
                "hardware_mining_allowed": safety.hardware_mining_allowed,
                "asic_bus_writes_allowed": safety.asic_bus_writes_allowed,
                "tuning_writes_allowed": safety.tuning_writes_allowed,
                "flashing_allowed": safety.flashing_allowed,
            }),
        );

        events.push(
            if readiness.state == HardwareReadinessState::Blocked {
                EventSeverity::Warn
            } else {
                EventSeverity::Info
            },
            "hardware.readiness_evaluated",
            "hardware",
            "hardware target readiness evaluated",
            json!({
                "state": readiness.state,
                "support": readiness.support,
                "recovery": readiness.recovery,
                "capabilities": readiness.capabilities.flags,
                "actions": readiness.actions,
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
                "pool_runtime_state": pool_runtime.state,
                "pool_strategy_state": pool_strategy.state,
                "job_pipeline_state": job_pipeline.state,
                "stratum_state": stratum.state,
                "stratum_socket_open": stratum.socket_open,
                "latency_warning_ms": pool_runtime.policy.latency_warning_ms,
                "job_processing_budget_ms": pool_runtime.policy.job_processing_budget_ms,
                "reconnect_min_interval_seconds": pool_runtime.policy.reconnect_min_interval_seconds,
                "tuning_mode": profiles.active,
                "tuning_plan_state": tuning_plan.state,
                "tuning_writable": tuning_plan.writable,
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
            "tuning.plan_loaded",
            "tuning",
            "slow chip-by-chip tuning plan loaded",
            json!({
                "state": tuning_plan.state,
                "active_phase": tuning_plan.active_phase,
                "writable": tuning_plan.writable,
                "voltage_trim_last": tuning_plan.steps.last().map(|step| step.phase),
                "frequency_step_mhz": tuning_plan.guardrails.chip_frequency_step_mhz,
                "voltage_step_mv": tuning_plan.guardrails.voltage_step_mv,
                "min_step_duration_seconds": tuning_plan.guardrails.min_step_duration_seconds,
            }),
        );

        events.push(
            if tuning_execution.state == TuningExecutionState::Blocked {
                EventSeverity::Warn
            } else {
                EventSeverity::Info
            },
            "tuning.execution_preview",
            "tuning",
            "tuning execution status derived from plan and safety gate",
            json!({
                "state": tuning_execution.state,
                "autotune": tuning_execution.autotune,
                "active_phase": tuning_execution.active_phase,
                "write_allowed": tuning_execution.write_allowed,
                "blocked_reason": tuning_execution.blocked_reason,
                "current_step": tuning_execution.current_step,
                "queued_steps": tuning_execution.queued_steps.len(),
            }),
        );

        events.push(
            EventSeverity::Info,
            "job.pipeline_loaded",
            "miner",
            "low-latency job pipeline policy loaded",
            json!({
                "state": job_pipeline.state,
                "notify_to_dispatch_budget_ms": job_pipeline.notify_to_dispatch_budget_ms,
                "stale_job_retirement_ms": job_pipeline.stale_job_retirement_ms,
                "max_pending_jobs": job_pipeline.max_pending_jobs,
                "prefer_newest_job": job_pipeline.prefer_newest_job,
                "drop_stale_jobs": job_pipeline.drop_stale_jobs,
                "reset_nonce_on_new_prev_hash": job_pipeline.reset_nonce_on_new_prev_hash,
            }),
        );

        events.push(
            EventSeverity::Info,
            if stratum.state == StratumEngineState::Live {
                "stratum.engine_live"
            } else {
                "stratum.engine_planned"
            },
            "stratum",
            if stratum.state == StratumEngineState::Live {
                if stratum.submit_policy.enabled_in_build {
                    "Stratum V1 socket engine is connected and dispatching work"
                } else {
                    "Stratum V1 socket engine is connected and receiving work while ASIC dispatch stays gated"
                }
            } else {
                "Stratum V1 engine contract loaded without opening sockets"
            },
            json!({
                "state": stratum.state,
                "protocol": stratum.protocol,
                "connection": stratum.connection,
                "socket_open": stratum.socket_open,
                "active_pool_priority": stratum.active_pool_priority,
                "subscribed": stratum.subscribed,
                "authorized": stratum.authorized,
                "share_validation": stratum.share_validation,
                "submit_enabled_in_build": stratum.submit_policy.enabled_in_build,
                "local_precheck_required": stratum.submit_policy.local_precheck_required,
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
                "pool.strategy_loaded",
                "pool",
                "low-latency pool connection strategy loaded",
                json!({
                    "active_priority": pool_strategy.active_priority,
                    "failover_priority_order": pool_strategy.failover_priority_order,
                    "persistent_connection_required": pool_strategy.persistent_connection_required,
                    "reconnect_jitter_allowed": pool_strategy.reconnect_jitter_allowed,
                    "keepalive_interval_seconds": self.pool_policy.keepalive_interval_seconds,
                    "reconnect_min_interval_seconds": self.pool_policy.reconnect_min_interval_seconds,
                    "failover_cooldown_seconds": self.pool_policy.failover_cooldown_seconds,
                }),
            );
            events.push(
                EventSeverity::Info,
                "pool.active_selected",
                "pool",
                "active pool selected by priority",
                json!({
                    "active_priority": pools.active_priority,
                    "enabled": pools.enabled,
                    "latency_warning_ms": pool_runtime.policy.latency_warning_ms,
                    "job_processing_budget_ms": pool_runtime.policy.job_processing_budget_ms,
                    "reconnect_min_interval_seconds": pool_runtime.policy.reconnect_min_interval_seconds,
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
            identity: self.hardware_identity_report(),
            safety: self.hardware_safety_gate(),
            readiness: self.hardware_readiness_report(),
            health: self.health(),
            miner: self.miner_status(),
            job_pipeline: self.job_pipeline(),
            stratum: self.stratum_status(),
            chains: self.chains(),
            pools: self.pools(),
            pool_runtime: self.pool_runtime(),
            pool_strategy: self.pool_strategy(),
            profiles: self.profiles(),
            tuning_plan: self.tuning_plan(),
            tuning_execution: self.tuning_execution_status(),
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
            identity: self.hardware_identity_report(),
            safety: self.hardware_safety_gate(),
            readiness: self.hardware_readiness_report(),
            health: self.health(),
            miner: self.miner_status(),
            job_pipeline: self.job_pipeline(),
            stratum: self.stratum_status(),
            chains: self.chains(),
            pools: self.pools(),
            pool_runtime: self.pool_runtime(),
            pool_strategy: self.pool_strategy(),
            profiles: self.profiles(),
            tuning_plan: self.tuning_plan(),
            tuning_execution: self.tuning_execution_status(),
            contribution: self.contribution_status(),
            update: self.update_status(),
            events: self.events(),
        }
    }

    pub fn prometheus_metrics(&self) -> String {
        let system = self.system_info();
        let identity = self.hardware_identity_report();
        let safety = self.hardware_safety_gate();
        let readiness = self.hardware_readiness_report();
        let health = self.health();
        let miner = self.miner_status();
        let job_pipeline = self.job_pipeline();
        let stratum = self.stratum_status();
        let contribution = self.contribution_status();
        let pools = self.pools();
        let pool_runtime = self.pool_runtime();
        let pool_strategy = self.pool_strategy();
        let profiles = self.profiles();
        let tuning_plan = self.tuning_plan();
        let tuning_execution = self.tuning_execution_status();
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
            "omo_hardware_identity_evidence_total",
            identity.evidence.len(),
        );
        metric(
            &mut output,
            "omo_hardware_identity_conflict",
            bool_value(identity.state == openmineros_common::HardwareIdentityState::Conflict),
        );
        metric(
            &mut output,
            "omo_hardware_safety_configured_target_accepted",
            bool_value(safety.configured_target_accepted),
        );
        metric(
            &mut output,
            "omo_hardware_safety_hardware_mining_allowed",
            bool_value(safety.hardware_mining_allowed),
        );
        metric(
            &mut output,
            "omo_hardware_safety_asic_bus_writes_allowed",
            bool_value(safety.asic_bus_writes_allowed),
        );
        metric(
            &mut output,
            "omo_hardware_safety_flashing_allowed",
            bool_value(safety.flashing_allowed),
        );
        labeled_metric(
            &mut output,
            "omo_hardware_readiness_state",
            &[("state", hardware_readiness_state_label(readiness.state))],
            1,
        );
        metric(
            &mut output,
            "omo_hardware_readiness_actions_allowed_total",
            readiness
                .actions
                .iter()
                .filter(|action| action.allowed)
                .count(),
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
        metric(
            &mut output,
            "omo_job_notify_to_dispatch_budget_ms",
            job_pipeline.notify_to_dispatch_budget_ms,
        );
        metric(
            &mut output,
            "omo_job_stale_retirement_ms",
            job_pipeline.stale_job_retirement_ms,
        );
        metric(
            &mut output,
            "omo_job_max_pending_jobs",
            job_pipeline.max_pending_jobs,
        );
        metric(
            &mut output,
            "omo_job_prefer_newest",
            bool_value(job_pipeline.prefer_newest_job),
        );
        metric(
            &mut output,
            "omo_job_drop_stale",
            bool_value(job_pipeline.drop_stale_jobs),
        );
        metric(
            &mut output,
            "omo_job_reset_nonce_on_new_prev_hash",
            bool_value(job_pipeline.reset_nonce_on_new_prev_hash),
        );
        metric(
            &mut output,
            "omo_stratum_socket_open",
            bool_value(stratum.socket_open),
        );
        metric(
            &mut output,
            "omo_stratum_subscribed",
            bool_value(stratum.subscribed),
        );
        metric(
            &mut output,
            "omo_stratum_authorized",
            bool_value(stratum.authorized),
        );
        metric(
            &mut output,
            "omo_stratum_active_pool_priority",
            stratum.active_pool_priority.map_or(-1_i32, i32::from),
        );
        metric(
            &mut output,
            "omo_stratum_pending_jobs",
            stratum.pending_jobs,
        );
        metric(
            &mut output,
            "omo_stratum_shares_submitted_total",
            stratum.shares_submitted,
        );
        metric(
            &mut output,
            "omo_stratum_shares_accepted_total",
            stratum.shares_accepted,
        );
        metric(
            &mut output,
            "omo_stratum_shares_rejected_total",
            stratum.shares_rejected,
        );
        metric(
            &mut output,
            "omo_stratum_submit_enabled_in_build",
            bool_value(stratum.submit_policy.enabled_in_build),
        );
        metric(
            &mut output,
            "omo_stratum_submit_local_precheck_required",
            bool_value(stratum.submit_policy.local_precheck_required),
        );
        metric(
            &mut output,
            "omo_stratum_submit_max_queue_depth",
            stratum.submit_policy.max_submit_queue_depth,
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
        metric(
            &mut output,
            "omo_pool_latency_warning_ms",
            pool_runtime.policy.latency_warning_ms,
        );
        metric(
            &mut output,
            "omo_pool_job_processing_budget_ms",
            pool_runtime.policy.job_processing_budget_ms,
        );
        metric(
            &mut output,
            "omo_pool_reconnect_min_interval_seconds",
            pool_runtime.policy.reconnect_min_interval_seconds,
        );
        metric(
            &mut output,
            "omo_pool_failover_cooldown_seconds",
            pool_runtime.policy.failover_cooldown_seconds,
        );
        metric(
            &mut output,
            "omo_pool_reconnects_total",
            pool_runtime.reconnects_total,
        );
        metric(
            &mut output,
            "omo_pool_reconnect_suppressed_total",
            pool_runtime.reconnect_suppressed_total,
        );
        metric(
            &mut output,
            "omo_pool_stale_jobs_total",
            pool_runtime.stale_jobs_total,
        );
        metric(
            &mut output,
            "omo_pool_strategy_enabled_candidates_total",
            pool_strategy.plans.len(),
        );
        metric(
            &mut output,
            "omo_pool_strategy_active_priority",
            pool_strategy.active_priority.map_or(-1_i32, i32::from),
        );
        metric(
            &mut output,
            "omo_pool_strategy_persistent_connection_required",
            bool_value(pool_strategy.persistent_connection_required),
        );
        metric(
            &mut output,
            "omo_pool_strategy_reconnect_jitter_allowed",
            bool_value(pool_strategy.reconnect_jitter_allowed),
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
            "omo_tuning_plan_writable",
            bool_value(tuning_plan.writable),
        );
        labeled_metric(
            &mut output,
            "omo_tuning_execution_state",
            &[(
                "state",
                tuning_execution_state_label(tuning_execution.state),
            )],
            1,
        );
        metric(
            &mut output,
            "omo_tuning_execution_write_allowed",
            bool_value(tuning_execution.write_allowed),
        );
        metric(
            &mut output,
            "omo_tuning_execution_current_step",
            tuning_execution.current_step.map_or(0, |step| step.order),
        );
        metric(
            &mut output,
            "omo_tuning_execution_queued_steps",
            tuning_execution.queued_steps.len(),
        );
        metric(
            &mut output,
            "omo_tuning_frequency_step_mhz",
            tuning_plan.guardrails.chip_frequency_step_mhz,
        );
        metric(
            &mut output,
            "omo_tuning_voltage_step_mv",
            tuning_plan.guardrails.voltage_step_mv,
        );
        metric(
            &mut output,
            "omo_tuning_min_step_duration_seconds",
            tuning_plan.guardrails.min_step_duration_seconds,
        );

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

struct StratumEngine {
    pools: Vec<PoolConfig>,
    policy: PoolConnectionPolicy,
    dispatcher: Arc<dyn AsicJobDispatcher + Send + Sync>,
    dispatch_enabled: bool,
    active_pool: Option<PoolConfig>,
    status: StratumEngineStatus,
    writer: Option<TcpStream>,
    reader: Option<BufReader<TcpStream>>,
    last_connect_attempt: Option<Instant>,
    reconnects_total: u64,
    reconnect_suppressed_total: u64,
    stale_jobs_total: u64,
    last_connect_latency_ms: Option<f64>,
}

impl std::fmt::Debug for StratumEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StratumEngine")
            .field("pools", &self.pools)
            .field("policy", &self.policy)
            .field("active_pool", &self.active_pool)
            .field("status", &self.status)
            .field("writer", &self.writer.is_some())
            .field("reader", &self.reader.is_some())
            .field("last_connect_attempt", &self.last_connect_attempt)
            .field("reconnects_total", &self.reconnects_total)
            .field(
                "reconnect_suppressed_total",
                &self.reconnect_suppressed_total,
            )
            .field("stale_jobs_total", &self.stale_jobs_total)
            .field("last_connect_latency_ms", &self.last_connect_latency_ms)
            .finish()
    }
}

impl StratumEngine {
    fn new(
        pools: Vec<PoolConfig>,
        job_pipeline: JobPipelinePolicy,
        policy: PoolConnectionPolicy,
        backend_mode: RuntimeBackendMode,
        active_priority: Option<u8>,
        dispatcher: Arc<dyn AsicJobDispatcher + Send + Sync>,
    ) -> Self {
        let active_pool = select_active_pool(&pools);
        let status = if backend_mode == RuntimeBackendMode::HardwareMining {
            StratumEngineStatus::live(active_priority, &job_pipeline)
        } else {
            StratumEngineStatus::planned(active_priority, &job_pipeline)
        };
        Self {
            pools,
            policy,
            dispatcher,
            active_pool,
            status,
            writer: None,
            reader: None,
            last_connect_attempt: None,
            reconnects_total: 0,
            reconnect_suppressed_total: 0,
            stale_jobs_total: 0,
            last_connect_latency_ms: None,
            dispatch_enabled: false,
        }
    }

    fn poll(&mut self, socket_enabled: bool, dispatch_enabled: bool) {
        self.active_pool = select_active_pool(&self.pools);
        self.status.active_pool_priority = self.active_pool.as_ref().map(|pool| pool.priority);
        self.dispatch_enabled = dispatch_enabled;
        self.status.submit_policy.enabled_in_build = socket_enabled;

        if !socket_enabled {
            self.close_socket();
            self.status.state = StratumEngineState::PlannedNoSocket;
            self.status.connection = StratumConnectionState::NotStarted;
            self.status.socket_open = false;
            self.status.subscribed = false;
            self.status.authorized = false;
            self.status.share_validation = ShareValidationMode::PlannedLocalPrecheck;
            self.status.submit_policy =
                StratumSubmitPolicy::from(&JobPipelinePolicy::from(self.policy));
            self.status.notes = vec!["live stratum is disabled in this backend mode".to_string()];
            return;
        }

        let Some(pool) = self.active_pool.clone() else {
            self.disconnect("no enabled pool is configured");
            self.status.state = StratumEngineState::Degraded;
            self.status.connection = StratumConnectionState::NotStarted;
            return;
        };

        if self.writer.is_none() {
            if !self.may_connect_now() {
                self.reconnect_suppressed_total = self.reconnect_suppressed_total.saturating_add(1);
                return;
            }
            if let Err(error) = self.connect(&pool) {
                self.last_connect_attempt = Some(Instant::now());
                self.reconnects_total = self.reconnects_total.saturating_add(1);
                self.status.state = StratumEngineState::Degraded;
                self.status.connection = StratumConnectionState::Disconnected;
                self.status.socket_open = false;
                self.status.notes = vec![error];
                return;
            }
            self.reconnects_total = self.reconnects_total.saturating_add(1);
        }

        self.read_messages();
        self.update_live_state();
        self.status.submit_policy.enabled_in_build = socket_enabled;
    }

    fn submit_share(&mut self, candidate: StratumShareCandidate) -> SharePrecheckResult {
        let precheck = precheck_share_submit(&candidate, &self.status);
        if precheck.verdict == openmineros_common::SharePrecheckVerdict::RejectedStaleJob {
            self.stale_jobs_total = self.stale_jobs_total.saturating_add(1);
        }
        if !precheck.submit_allowed {
            return precheck;
        }
        let Some(writer) = self.writer.as_mut() else {
            self.disconnect("stratum submit failed because socket is closed");
            return rejected_share("stratum socket is not open during submit");
        };

        let submit = serde_json::json!({
            "id": 4,
            "method": "mining.submit",
            "params": [
                candidate.worker,
                candidate.job_id,
                candidate.extranonce2,
                candidate.ntime,
                candidate.nonce
            ]
        })
        .to_string();
        if writer.write_all(submit.as_bytes()).is_err()
            || writer.write_all(b"\n").is_err()
            || writer.flush().is_err()
        {
            self.disconnect("stratum submit failed due to socket write error");
            return rejected_share("failed to write share submit to pool socket");
        }

        self.status.shares_submitted += 1;
        precheck
    }

    fn connect(&mut self, pool: &PoolConfig) -> Result<(), String> {
        let endpoint = parse_tcp_endpoint(&pool.url)
            .ok_or_else(|| format!("unsupported pool url for live backend: {}", pool.url))?;
        let connect_started = Instant::now();
        self.status.state = StratumEngineState::Connecting;
        self.status.connection = StratumConnectionState::Dialing;

        let stream = TcpStream::connect(&endpoint)
            .map_err(|error| format!("failed to connect to {}: {}", endpoint, error))?;
        stream
            .set_nodelay(true)
            .map_err(|error| format!("failed to set nodelay on {}: {}", endpoint, error))?;
        stream
            .set_read_timeout(Some(Duration::from_millis(25)))
            .map_err(|error| format!("failed to set read timeout: {}", error))?;
        stream
            .set_write_timeout(Some(Duration::from_millis(250)))
            .map_err(|error| format!("failed to set write timeout: {}", error))?;

        let mut writer = stream;
        let reader = BufReader::new(
            writer
                .try_clone()
                .map_err(|error| format!("failed to clone pool socket: {}", error))?,
        );

        let subscribe = serde_json::json!({
            "id": 1,
            "method": "mining.subscribe",
            "params": ["openmineros/0.1.0"]
        })
        .to_string();
        let authorize = serde_json::json!({
            "id": 2,
            "method": "mining.authorize",
            "params": [pool.user, pool.password]
        })
        .to_string();

        writer
            .write_all(subscribe.as_bytes())
            .and_then(|_| writer.write_all(b"\n"))
            .and_then(|_| writer.write_all(authorize.as_bytes()))
            .and_then(|_| writer.write_all(b"\n"))
            .and_then(|_| writer.flush())
            .map_err(|error| format!("failed to send subscribe/authorize: {}", error))?;

        self.writer = Some(writer);
        self.reader = Some(reader);
        self.last_connect_attempt = Some(Instant::now());
        self.last_connect_latency_ms = Some(connect_started.elapsed().as_secs_f64() * 1000.0);
        self.status.connection = StratumConnectionState::Connected;
        self.status.socket_open = true;
        self.status.subscribed = false;
        self.status.authorized = false;
        self.status.state = StratumEngineState::Connecting;
        self.status.notes = vec![format!("connected to {}", endpoint)];

        Ok(())
    }

    fn read_messages(&mut self) {
        if self.reader.is_none() {
            return;
        }

        for _ in 0..16 {
            let read_result = {
                let reader = self.reader.as_mut().expect("reader must exist");
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => ReadResult::Closed,
                    Ok(_) => ReadResult::Line(line),
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            || error.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        ReadResult::Timeout
                    }
                    Err(error) => ReadResult::Error(error.to_string()),
                }
            };

            match read_result {
                ReadResult::Closed => {
                    self.disconnect("pool socket closed by remote peer");
                    return;
                }
                ReadResult::Line(line) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    match classify_stratum_message(line) {
                        Ok(classified) => self.handle_message(classified),
                        Err(error) => {
                            self.status.state = StratumEngineState::Degraded;
                            self.status.notes = vec![format!("stratum parse error: {}", error)];
                        }
                    }
                }
                ReadResult::Timeout => break,
                ReadResult::Error(error) => {
                    self.disconnect(&format!("stratum read error: {}", error));
                    return;
                }
            }
        }
    }

    fn handle_message(&mut self, classified: openmineros_common::StratumMessageClassification) {
        match classified.kind {
            StratumMessageKind::MiningNotify => {
                if let Some(job) = classified.notify {
                    self.status.pending_jobs = if job.clean_jobs {
                        1
                    } else {
                        self.status
                            .pending_jobs
                            .saturating_add(1)
                            .min(self.status.submit_policy.max_submit_queue_depth)
                    };
                    self.status.active_job = Some(job.clone());
                    if self.dispatch_enabled {
                        if let Err(error) = self.dispatcher.dispatch_job(&job) {
                            self.status.state = StratumEngineState::Degraded;
                            self.status.notes = vec![format!("asic dispatch failed: {}", error)];
                        }
                    }
                }
            }
            StratumMessageKind::MiningSetDifficulty => {
                self.status.current_difficulty = classified.difficulty;
            }
            StratumMessageKind::SubscribeResult => {
                self.status.subscribed = true;
            }
            StratumMessageKind::AuthorizeResult => {
                self.status.authorized = classified.result_success.unwrap_or(false);
            }
            StratumMessageKind::SubmitResult => {
                if classified.result_success.unwrap_or(false) {
                    self.status.shares_accepted += 1;
                } else {
                    self.status.shares_rejected += 1;
                }
            }
            StratumMessageKind::Unknown => {}
        }
    }

    fn update_live_state(&mut self) {
        if self.status.socket_open && self.status.subscribed && self.status.authorized {
            self.status.state = StratumEngineState::Live;
        } else if self.status.socket_open {
            self.status.state = StratumEngineState::Connecting;
        }
    }

    fn disconnect(&mut self, note: &str) {
        self.close_socket();
        self.status.socket_open = false;
        self.status.subscribed = false;
        self.status.authorized = false;
        self.status.connection = StratumConnectionState::Disconnected;
        self.status.state = StratumEngineState::Degraded;
        self.status.notes = vec![note.to_string()];
    }

    fn close_socket(&mut self) {
        if let Some(stream) = self.writer.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        self.reader = None;
    }

    fn may_connect_now(&self) -> bool {
        self.last_connect_attempt.is_none_or(|last| {
            last.elapsed().as_secs() >= u64::from(self.policy.reconnect_min_interval_seconds)
        })
    }
}

fn select_active_pool(pools: &[PoolConfig]) -> Option<PoolConfig> {
    pools
        .iter()
        .filter(|pool| pool.enabled)
        .min_by_key(|pool| pool.priority)
        .cloned()
}

fn parse_tcp_endpoint(url: &str) -> Option<String> {
    url.strip_prefix("stratum+tcp://").map(ToString::to_string)
}

enum ReadResult {
    Line(String),
    Closed,
    Timeout,
    Error(String),
}

fn rejected_share(reason: &str) -> SharePrecheckResult {
    SharePrecheckResult {
        verdict: openmineros_common::SharePrecheckVerdict::RejectedSocketClosed,
        submit_allowed: false,
        reasons: vec![reason.to_string()],
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

fn tuning_execution_state_label(state: TuningExecutionState) -> &'static str {
    match state {
        TuningExecutionState::Disabled => "disabled",
        TuningExecutionState::PlannedReadOnly => "planned_read_only",
        TuningExecutionState::ReadyToArm => "ready_to_arm",
        TuningExecutionState::Blocked => "blocked",
    }
}

fn slot_state_label(state: openmineros_common::SlotState) -> &'static str {
    match state {
        openmineros_common::SlotState::Active => "active",
        openmineros_common::SlotState::Inactive => "inactive",
        openmineros_common::SlotState::Unknown => "unknown",
    }
}

fn hardware_readiness_state_label(state: HardwareReadinessState) -> &'static str {
    match state {
        HardwareReadinessState::SimulationReady => "simulation_ready",
        HardwareReadinessState::ReadOnlyIdentified => "read_only_identified",
        HardwareReadinessState::ReadOnlyNeedsIdentity => "read_only_needs_identity",
        HardwareReadinessState::Blocked => "blocked",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openmineros_common::{TuningMode, TuningTargetType};
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
        time::Duration,
    };

    #[derive(Debug, Default)]
    struct MockDispatcher {
        dispatched_jobs: Mutex<Vec<String>>,
    }

    impl openmineros_asic_backend::AsicJobDispatcher for MockDispatcher {
        fn dispatch_job(
            &self,
            job: &openmineros_common::StratumJobTemplate,
        ) -> Result<(), openmineros_asic_backend::BackendError> {
            self.dispatched_jobs
                .lock()
                .unwrap()
                .push(job.job_id.clone());
            Ok(())
        }
    }

    fn spawn_mock_stratum_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();

            let mut writer = stream;
            writer
                .write_all(
                    br#"{"id":1,"result":[true,"extranonce1",4],"error":null}
{"id":2,"result":true,"error":null}
{"id":null,"method":"mining.set_difficulty","params":[4096]}
{"id":null,"method":"mining.notify","params":["job-7","aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","","",[],"20000000","1a2b3c4d","5e6f7788",true]}
"#,
                )
                .unwrap();
            writer.flush().unwrap();
            thread::sleep(Duration::from_millis(200));
        });

        format!("stratum+tcp://{}", addr)
    }

    fn spawn_mock_stratum_submit_server() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_worker = captured.clone();

        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            for _ in 0..2 {
                line.clear();
                reader.read_line(&mut line).unwrap();
                captured_worker
                    .lock()
                    .unwrap()
                    .push(line.trim().to_string());
            }

            let mut writer = stream;
            writer
                .write_all(
                    br#"{"id":1,"result":[true,"extranonce1",4],"error":null}
{"id":2,"result":true,"error":null}
{"id":null,"method":"mining.notify","params":["job-7","aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","","",[],"20000000","1a2b3c4d","5e6f7788",true]}
"#,
                )
                .unwrap();
            writer.flush().unwrap();

            line.clear();
            reader.read_line(&mut line).unwrap();
            captured_worker
                .lock()
                .unwrap()
                .push(line.trim().to_string());
        });

        (format!("stratum+tcp://{}", addr), captured)
    }

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
        assert!(
            events
                .events
                .iter()
                .any(|event| event.event_type == "job.pipeline_loaded")
        );
        assert!(
            events
                .events
                .iter()
                .any(|event| event.event_type == "hardware.identity_evaluated")
        );
        assert!(
            events
                .events
                .iter()
                .any(|event| event.event_type == "hardware.readiness_evaluated")
        );
        assert!(
            events
                .events
                .iter()
                .any(|event| event.event_type == "stratum.engine_planned")
        );
    }

    #[test]
    fn hardware_mining_notify_dispatches_job_to_asic_backend() {
        let dispatcher = Arc::new(MockDispatcher::default());
        let mut engine = StratumEngine::new(
            Vec::new(),
            JobPipelinePolicy::from(PoolConnectionPolicy::default()),
            PoolConnectionPolicy::default(),
            RuntimeBackendMode::HardwareMining,
            Some(0),
            dispatcher.clone(),
        );
        let classified = classify_stratum_message(
            r#"{
                "id": null,
                "method": "mining.notify",
                "params": [
                    "job-7",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    [],
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    [],
                    "20000000",
                    "1a2b3c4d",
                    "5e6f7788",
                    true
                ]
            }"#,
        )
        .unwrap();

        engine.dispatch_enabled = true;
        engine.handle_message(classified);

        assert_eq!(engine.status.active_job.as_ref().unwrap().job_id, "job-7");
        assert_eq!(engine.status.pending_jobs, 1);
        assert_eq!(engine.status.state, StratumEngineState::Connecting);
        assert_eq!(
            dispatcher.dispatched_jobs.lock().unwrap().as_slice(),
            ["job-7"]
        );
    }

    #[test]
    fn hardware_mining_poll_connects_to_mock_stratum_and_reaches_live_state() {
        let dispatcher = Arc::new(MockDispatcher::default());
        let pool_url = spawn_mock_stratum_server();
        let mut engine = StratumEngine::new(
            vec![PoolConfig {
                priority: 0,
                url: pool_url,
                user: "acct.worker".to_string(),
                password: "x".to_string(),
                enabled: true,
            }],
            JobPipelinePolicy::from(PoolConnectionPolicy::default()),
            PoolConnectionPolicy::default(),
            RuntimeBackendMode::HardwareMining,
            Some(0),
            dispatcher.clone(),
        );

        engine.poll(true, true);

        assert_eq!(engine.status.state, StratumEngineState::Live);
        assert_eq!(engine.status.connection, StratumConnectionState::Connected);
        assert!(engine.status.socket_open);
        assert!(engine.status.subscribed);
        assert!(engine.status.authorized);
        assert!(engine.status.submit_policy.enabled_in_build);
        assert_eq!(engine.status.active_job.as_ref().unwrap().job_id, "job-7");
        assert_eq!(
            dispatcher.dispatched_jobs.lock().unwrap().as_slice(),
            ["job-7"]
        );
    }

    #[test]
    fn hardware_mining_live_socket_accepts_share_submit_after_local_precheck() {
        let dispatcher = Arc::new(MockDispatcher::default());
        let (pool_url, captured) = spawn_mock_stratum_submit_server();
        let mut engine = StratumEngine::new(
            vec![PoolConfig {
                priority: 0,
                url: pool_url,
                user: "acct.worker".to_string(),
                password: "x".to_string(),
                enabled: true,
            }],
            JobPipelinePolicy::from(PoolConnectionPolicy::default()),
            PoolConnectionPolicy::default(),
            RuntimeBackendMode::HardwareMining,
            Some(0),
            dispatcher,
        );

        engine.poll(true, true);
        engine.poll(true, true);
        engine.status.current_difficulty = Some(4096.0);
        let active_job_id = engine.status.active_job.as_ref().unwrap().job_id.clone();
        let candidate = StratumShareCandidate {
            worker: "acct.worker".to_string(),
            job_id: active_job_id,
            extranonce2: "00000002".to_string(),
            ntime: "69fc901d".to_string(),
            nonce: "00000001".to_string(),
        };
        let result = engine.submit_share(candidate);
        thread::sleep(Duration::from_millis(50));

        assert_eq!(
            result.verdict,
            openmineros_common::SharePrecheckVerdict::AcceptedForSubmit
        );
        assert!(result.submit_allowed);
        assert!(
            captured
                .lock()
                .unwrap()
                .iter()
                .any(|line| line.contains(r#""method":"mining.submit""#))
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
            events
                .events
                .iter()
                .any(|event| event.event_type == "pool.strategy_loaded")
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
        let readiness = supervisor.hardware_readiness_report();
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
        assert_eq!(
            readiness.state,
            HardwareReadinessState::ReadOnlyNeedsIdentity
        );
        assert!(readiness.actions.iter().all(|action| !action.allowed));
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
        let identity = supervisor.hardware_identity_report();

        assert_eq!(report.backend, RuntimeBackendMode::HardwareProbe);
        assert_eq!(report.model, Model::S19jPro);
        assert_eq!(report.board_family, BoardFamily::Xilinx);
        assert!(report.safe_read_only);
        assert_eq!(identity.backend, RuntimeBackendMode::HardwareProbe);
        assert!(identity.safe_read_only);
        assert_eq!(identity.configured_board, BoardFamily::Xilinx);
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
        assert_eq!(
            overview.identity.state,
            openmineros_common::HardwareIdentityState::ConfiguredOnly
        );
        assert_eq!(
            overview.readiness.state,
            HardwareReadinessState::SimulationReady
        );
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
    fn pool_runtime_policy_is_exposed_without_fake_latency() {
        let config = RuntimeConfig::from_toml_str(
            r#"
            [pool_policy]
            latency_warning_ms = 250
            job_processing_budget_ms = 25
            reconnect_min_interval_seconds = 20
            failover_cooldown_seconds = 90
            keepalive_interval_seconds = 15

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
        let runtime = supervisor.pool_runtime();
        let job_pipeline = supervisor.job_pipeline();
        let overview = supervisor.dashboard_overview();
        let metrics = supervisor.prometheus_metrics();

        assert_eq!(runtime.policy.latency_warning_ms, 250);
        assert_eq!(runtime.policy.job_processing_budget_ms, 25);
        assert_eq!(runtime.active_latency_ms, None);
        assert_eq!(
            overview.pool_runtime.policy.reconnect_min_interval_seconds,
            20
        );
        assert_eq!(overview.pool_strategy.active_priority, Some(0));
        assert_eq!(overview.pool_strategy.plans.len(), 1);
        assert!(overview.pool_strategy.persistent_connection_required);
        assert!(!overview.pool_strategy.reconnect_jitter_allowed);
        assert_eq!(
            overview.pool_strategy.plans[0].keepalive_interval_seconds,
            15
        );
        assert_eq!(job_pipeline.notify_to_dispatch_budget_ms, 25);
        assert_eq!(overview.job_pipeline.stale_job_retirement_ms, 250);
        assert!(overview.job_pipeline.prefer_newest_job);
        assert!(overview.job_pipeline.drop_stale_jobs);
        assert!(!overview.stratum.socket_open);
        assert_eq!(overview.stratum.active_pool_priority, Some(0));
        assert!(!overview.stratum.authorized);
        assert!(!overview.stratum.submit_policy.enabled_in_build);
        assert!(overview.stratum.submit_policy.local_precheck_required);
        assert!(metrics.contains("omo_pool_latency_warning_ms 250"));
        assert!(metrics.contains("omo_pool_job_processing_budget_ms 25"));
        assert!(metrics.contains("omo_pool_strategy_enabled_candidates_total 1"));
        assert!(metrics.contains("omo_pool_strategy_persistent_connection_required 1"));
        assert!(metrics.contains("omo_job_notify_to_dispatch_budget_ms 25"));
        assert!(metrics.contains("omo_job_drop_stale 1"));
        assert!(metrics.contains("omo_stratum_socket_open 0"));
        assert!(metrics.contains("omo_stratum_active_pool_priority 0"));
        assert!(metrics.contains("omo_stratum_submit_enabled_in_build 0"));
        assert!(metrics.contains("omo_stratum_submit_local_precheck_required 1"));
        assert!(metrics.contains("omo_hardware_identity_evidence_total"));
        assert!(metrics.contains("omo_hardware_identity_conflict 0"));
        assert!(metrics.contains("omo_hardware_readiness_state{state=\"simulation_ready\"} 1"));
        assert!(metrics.contains("omo_hardware_readiness_actions_allowed_total 1"));
        assert!(!metrics.contains("super-secret"));
    }

    #[test]
    fn tuning_plan_is_read_only_and_voltage_trim_last() {
        let config = RuntimeConfig::from_toml_str(
            r#"
            [tuning]
            mode = "balanced"
            autotune = true
            "#,
        )
        .unwrap();
        let supervisor =
            Supervisor::with_config(Model::S19jPro, BoardFamily::Xilinx, config).unwrap();
        let overview = supervisor.dashboard_overview();
        let metrics = supervisor.prometheus_metrics();

        assert!(!overview.tuning_plan.writable);
        assert_eq!(
            overview.tuning_plan.steps.last().unwrap().phase,
            openmineros_common::TuningPhase::VoltageTrim
        );
        assert!(overview.tuning_plan.guardrails.rollback_on_rejected_shares);
        assert!(metrics.contains("omo_tuning_plan_writable 0"));
        assert!(metrics.contains("omo_tuning_frequency_step_mhz 5"));
        assert!(metrics.contains("omo_tuning_voltage_step_mv 5"));
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
