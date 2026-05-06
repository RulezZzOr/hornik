pub mod board;
pub mod config;
pub mod contribution;
pub mod event;
pub mod job;
pub mod manifest;
pub mod overview;
pub mod pool;
pub mod probe;
pub mod status;
pub mod stratum;
pub mod support;
pub mod tuning;
pub mod update;

pub use board::{
    BoardCatalogEntry, BoardFamily, BoardProfile, Capability, CapabilitySet, Model, SupportLevel,
    SupportedTarget, TargetCatalog, TargetError, board_catalog, supported_targets, target_catalog,
};
pub use config::{ConfigError, RuntimeConfig};
pub use contribution::{
    CONTRIBUTION_TARGET_LOCKED, ContributionConfig, ContributionConfigError, ContributionEndpoint,
    ContributionStatus, ContributionWindow, DEFAULT_CONTRIBUTION_BENEFICIARY,
    MAX_CONTRIBUTION_RATE_PERCENT, MUTABLE_CONTRIBUTION_FIELDS, default_contribution_endpoints,
};
pub use event::{EventBuilder, EventEnvelope, EventRecord, EventSeverity, EventsResponse};
pub use job::{JobPipelinePolicy, JobPipelineState};
pub use manifest::{
    ArtifactManifest, ManifestError, ReleaseManifest, ReleaseSignature, VerificationReport,
    verify_manifest_file,
};
pub use overview::DashboardOverview;
pub use pool::{
    PoolConfig, PoolConfigError, PoolConnectionPlan, PoolConnectionPolicy, PoolConnectionRole,
    PoolInfo, PoolRuntimeState, PoolRuntimeSummary, PoolStrategyResponse, PoolStrategyState,
    PoolSummary, plan_pool_strategy, summarize_pool_runtime, summarize_pools, validate_pools,
};
pub use probe::{HardwareProbeReport, HardwareProbeSummary, ProbeCheck, ProbeStatus};
pub use status::{
    ChainStatus, HealthStatus, HealthStatusResponse, MinerMode, MinerStatus, RuntimeBackendMode,
    RuntimeBackendModeParseError, Severity, SystemInfo,
};
pub use stratum::{
    ShareValidationMode, StratumConnectionState, StratumEngineState, StratumEngineStatus,
    StratumJobTemplate, StratumMessageClassification, StratumMessageError, StratumMessageKind,
    StratumProtocol, classify_stratum_message,
};
pub use support::{SupportBundle, SupportBundlePrivacy};
pub use tuning::{
    ProfileInfo, ProfilesResponse, TuningConfig, TuningConfigError, TuningGuardrails, TuningMode,
    TuningPhase, TuningPlanResponse, TuningPlanState, TuningStep, TuningTargetType,
    profile_catalog, tuning_plan_steps,
};
pub use update::{SlotInfo, SlotState, UpdateStatus};
