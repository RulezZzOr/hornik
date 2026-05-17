use crate::{
    ChainStatus, ContributionStatus, EventsResponse, HardwareIdentityReport,
    HardwareReadinessReport, HardwareSafetyGate, HealthStatusResponse, JobPipelinePolicy,
    MinerStatus, PoolRuntimeSummary, PoolStrategyResponse, PoolSummary, ProfilesResponse,
    RuntimeControlReport, StratumEngineStatus, SystemInfo, TuningExecutionStatus,
    TuningPlanResponse, UpdateStatus,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DashboardOverview {
    pub schema_version: u8,
    pub system: SystemInfo,
    pub identity: HardwareIdentityReport,
    pub safety: HardwareSafetyGate,
    pub readiness: HardwareReadinessReport,
    pub control: RuntimeControlReport,
    pub health: HealthStatusResponse,
    pub miner: MinerStatus,
    pub job_pipeline: JobPipelinePolicy,
    pub stratum: StratumEngineStatus,
    pub chains: Vec<ChainStatus>,
    pub pools: PoolSummary,
    pub pool_runtime: PoolRuntimeSummary,
    pub pool_strategy: PoolStrategyResponse,
    pub profiles: ProfilesResponse,
    pub tuning_plan: TuningPlanResponse,
    pub tuning_execution: TuningExecutionStatus,
    pub contribution: ContributionStatus,
    pub update: UpdateStatus,
    pub events: EventsResponse,
}

impl DashboardOverview {
    pub const SCHEMA_VERSION: u8 = 1;
}
