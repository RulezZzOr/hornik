use crate::{
    ChainStatus, ContributionStatus, EventsResponse, HealthStatusResponse, MinerStatus,
    PoolRuntimeSummary, PoolSummary, ProfilesResponse, SystemInfo, UpdateStatus,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DashboardOverview {
    pub schema_version: u8,
    pub system: SystemInfo,
    pub health: HealthStatusResponse,
    pub miner: MinerStatus,
    pub chains: Vec<ChainStatus>,
    pub pools: PoolSummary,
    pub pool_runtime: PoolRuntimeSummary,
    pub profiles: ProfilesResponse,
    pub contribution: ContributionStatus,
    pub update: UpdateStatus,
    pub events: EventsResponse,
}

impl DashboardOverview {
    pub const SCHEMA_VERSION: u8 = 1;
}
