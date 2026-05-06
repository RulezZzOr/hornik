use crate::status::HealthStatusResponse;
use crate::{
    ChainStatus, ContributionStatus, EventsResponse, MinerStatus, PoolSummary, ProfilesResponse,
    SystemInfo,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SupportBundle {
    pub schema_version: u32,
    pub generated_uptime_seconds: u64,
    pub privacy: SupportBundlePrivacy,
    pub system: SystemInfo,
    pub health: HealthStatusResponse,
    pub miner: MinerStatus,
    pub chains: Vec<ChainStatus>,
    pub pools: PoolSummary,
    pub profiles: ProfilesResponse,
    pub contribution: ContributionStatus,
    pub events: EventsResponse,
}

impl SupportBundle {
    pub const SCHEMA_VERSION: u32 = 1;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportBundlePrivacy {
    pub pool_passwords_redacted: bool,
    pub session_tokens_included: bool,
    pub private_keys_included: bool,
    pub raw_logs_included: bool,
}

impl Default for SupportBundlePrivacy {
    fn default() -> Self {
        Self {
            pool_passwords_redacted: true,
            session_tokens_included: false,
            private_keys_included: false,
            raw_logs_included: false,
        }
    }
}
