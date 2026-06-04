pub mod board;
pub mod config;
pub mod contribution;
pub mod control;
pub mod event;
pub mod firmware;
pub mod identity;
pub mod job;
pub mod manifest;
pub mod mining;
pub mod overview;
pub mod pool;
pub mod probe;
pub mod readiness;
pub mod recovery;
pub mod safety;
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
pub use control::{BoardControlState, LockedTuningProfile, RuntimeControlReport, TuningLockState};
pub use event::{EventBuilder, EventEnvelope, EventRecord, EventSeverity, EventsResponse};
pub use firmware::{FirmwareDeploymentReport, FirmwareGap, FirmwareGapState};
pub use identity::{
    HardwareIdentityConfidence, HardwareIdentityEvidence, HardwareIdentityObservation,
    HardwareIdentityReport, HardwareIdentityState, infer_hardware_identity,
};
pub use job::{JobPipelinePolicy, JobPipelineState};
pub use manifest::{
    ArtifactBudget, ArtifactBudgetEntry, ArtifactBudgetReport, ArtifactManifest, ManifestError,
    ReleaseManifest, ReleaseSignature, VerificationReport, check_manifest_budget_file,
    load_release_manifest_file, verify_manifest_file,
};
pub use overview::DashboardOverview;
pub use pool::{
    PoolConfig, PoolConfigError, PoolConnectionPlan, PoolConnectionPolicy, PoolConnectionRole,
    PoolInfo, PoolRuntimeState, PoolRuntimeSummary, PoolStrategyResponse, PoolStrategyState,
    PoolSummary, plan_pool_strategy, summarize_pool_runtime, summarize_pools, validate_pools,
};
pub use probe::{HardwareProbeReport, HardwareProbeSummary, ProbeCheck, ProbeStatus};
pub use readiness::{
    HardwareActionReadiness, HardwareReadinessReport, HardwareReadinessState,
    evaluate_hardware_readiness,
};
pub use recovery::{AntiBrickCheck, AntiBrickReport, AntiBrickState};
pub use safety::{HardwareSafetyGate, HardwareSafetyState, evaluate_hardware_safety};
pub use status::{
    ChainStatus, HealthStatus, HealthStatusResponse, MinerMode, MinerStatus, RuntimeBackendMode,
    RuntimeBackendModeParseError, Severity, SystemInfo,
};
pub use stratum::{
    SharePrecheckResult, SharePrecheckVerdict, ShareValidationMode, StratumConnectionState,
    StratumEngineState, StratumEngineStatus, StratumJobTemplate, StratumMessageClassification,
    StratumMessageError, StratumMessageKind, StratumProtocol, StratumShareCandidate,
    StratumSubmitPolicy, classify_stratum_message, precheck_share_submit,
};
pub use support::{SupportBundle, SupportBundlePrivacy};
pub use tuning::{
    ProfileInfo, ProfilesResponse, TuningConfig, TuningConfigError, TuningExecutionState,
    TuningExecutionStatus, TuningExecutionStep, TuningGuardrails, TuningMode, TuningPhase,
    TuningPlanResponse, TuningPlanState, TuningProtocolFrame, TuningProtocolSequenceSpec,
    TuningProtocolTranscript, TuningStep, TuningTargetType, profile_catalog, tuning_plan_steps,
};
pub use update::{SlotInfo, SlotState, UpdateStatus};
