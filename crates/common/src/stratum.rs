use crate::JobPipelinePolicy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StratumProtocol {
    V1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StratumEngineState {
    PlannedNoSocket,
    Connecting,
    Live,
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StratumConnectionState {
    NotStarted,
    Dialing,
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareValidationMode {
    PlannedLocalPrecheck,
    LocalPrecheck,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StratumEngineStatus {
    pub state: StratumEngineState,
    pub protocol: StratumProtocol,
    pub connection: StratumConnectionState,
    pub socket_open: bool,
    pub active_pool_priority: Option<u8>,
    pub subscribed: bool,
    pub authorized: bool,
    pub current_difficulty: Option<f64>,
    pub active_job: Option<StratumJobTemplate>,
    pub pending_jobs: u8,
    pub shares_submitted: u64,
    pub shares_accepted: u64,
    pub shares_rejected: u64,
    pub share_validation: ShareValidationMode,
    pub submit_policy: StratumSubmitPolicy,
    pub notes: Vec<String>,
}

impl StratumEngineStatus {
    pub fn planned(active_pool_priority: Option<u8>, job_pipeline: &JobPipelinePolicy) -> Self {
        Self {
            state: StratumEngineState::PlannedNoSocket,
            protocol: StratumProtocol::V1,
            connection: StratumConnectionState::NotStarted,
            socket_open: false,
            active_pool_priority,
            subscribed: false,
            authorized: false,
            current_difficulty: None,
            active_job: None,
            pending_jobs: 0,
            shares_submitted: 0,
            shares_accepted: 0,
            shares_rejected: 0,
            share_validation: ShareValidationMode::PlannedLocalPrecheck,
            submit_policy: StratumSubmitPolicy::from(job_pipeline),
            notes: vec![
                "build 0.1.0 exposes the Stratum V1 contract without opening pool sockets"
                    .to_string(),
                format!(
                    "future notify-to-dispatch budget is {} ms with max {} pending jobs",
                    job_pipeline.notify_to_dispatch_budget_ms, job_pipeline.max_pending_jobs
                ),
                "future share submit must run local prechecks before sending work to the pool"
                    .to_string(),
            ],
        }
    }

    pub fn live(active_pool_priority: Option<u8>, job_pipeline: &JobPipelinePolicy) -> Self {
        let mut status = Self::planned(active_pool_priority, job_pipeline);
        status.state = StratumEngineState::Connecting;
        status.connection = StratumConnectionState::Dialing;
        status.share_validation = ShareValidationMode::LocalPrecheck;
        status.submit_policy = StratumSubmitPolicy::enabled(job_pipeline);
        status.notes = vec![
            "stratum v1 socket engine is active in hardware-mining backend".to_string(),
            "notify jobs are dispatched to asic backend when permitted and shares are submitted after local precheck"
                .to_string(),
        ];
        status
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StratumSubmitPolicy {
    pub enabled_in_build: bool,
    pub local_precheck_required: bool,
    pub require_socket_open: bool,
    pub require_subscribed: bool,
    pub require_authorized: bool,
    pub require_active_job: bool,
    pub require_current_difficulty: bool,
    pub require_matching_job_id: bool,
    pub require_hex_extranonce2: bool,
    pub require_hex_ntime: bool,
    pub require_hex_nonce: bool,
    pub ntime_hex_len: usize,
    pub nonce_hex_len: usize,
    pub max_submit_queue_depth: u8,
}

impl From<&JobPipelinePolicy> for StratumSubmitPolicy {
    fn from(job_pipeline: &JobPipelinePolicy) -> Self {
        Self {
            enabled_in_build: false,
            local_precheck_required: true,
            require_socket_open: true,
            require_subscribed: true,
            require_authorized: true,
            require_active_job: true,
            require_current_difficulty: true,
            require_matching_job_id: true,
            require_hex_extranonce2: true,
            require_hex_ntime: true,
            require_hex_nonce: true,
            ntime_hex_len: 8,
            nonce_hex_len: 8,
            max_submit_queue_depth: job_pipeline.max_pending_jobs,
        }
    }
}

impl StratumSubmitPolicy {
    pub fn enabled(job_pipeline: &JobPipelinePolicy) -> Self {
        let mut policy = Self::from(job_pipeline);
        policy.enabled_in_build = true;
        policy
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StratumShareCandidate {
    pub worker: String,
    pub job_id: String,
    pub extranonce2: String,
    pub ntime: String,
    pub nonce: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharePrecheckVerdict {
    AcceptedForSubmit,
    RejectedInvalidField,
    RejectedSocketClosed,
    RejectedNotSubscribed,
    RejectedNotAuthorized,
    RejectedNoActiveJob,
    RejectedStaleJob,
    RejectedDifficultyMissing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharePrecheckResult {
    pub verdict: SharePrecheckVerdict,
    pub submit_allowed: bool,
    pub reasons: Vec<String>,
}

pub fn precheck_share_submit(
    candidate: &StratumShareCandidate,
    status: &StratumEngineStatus,
) -> SharePrecheckResult {
    let policy = &status.submit_policy;

    if candidate.worker.trim().is_empty() {
        return rejected(
            SharePrecheckVerdict::RejectedInvalidField,
            "worker must not be empty",
        );
    }
    if candidate.job_id.trim().is_empty() {
        return rejected(
            SharePrecheckVerdict::RejectedInvalidField,
            "job_id must not be empty",
        );
    }
    if policy.require_hex_extranonce2
        && validate_hex(&candidate.extranonce2, "extranonce2").is_err()
    {
        return rejected(
            SharePrecheckVerdict::RejectedInvalidField,
            "extranonce2 must be non-empty even-length hex",
        );
    }
    if policy.require_hex_ntime
        && validate_fixed_hex(&candidate.ntime, policy.ntime_hex_len).is_err()
    {
        return rejected(
            SharePrecheckVerdict::RejectedInvalidField,
            "ntime must be 8 hex characters",
        );
    }
    if policy.require_hex_nonce
        && validate_fixed_hex(&candidate.nonce, policy.nonce_hex_len).is_err()
    {
        return rejected(
            SharePrecheckVerdict::RejectedInvalidField,
            "nonce must be 8 hex characters",
        );
    }
    if policy.require_socket_open && !status.socket_open {
        return rejected(
            SharePrecheckVerdict::RejectedSocketClosed,
            "stratum socket is not open",
        );
    }
    if policy.require_subscribed && !status.subscribed {
        return rejected(
            SharePrecheckVerdict::RejectedNotSubscribed,
            "stratum session is not subscribed",
        );
    }
    if policy.require_authorized && !status.authorized {
        return rejected(
            SharePrecheckVerdict::RejectedNotAuthorized,
            "stratum worker is not authorized",
        );
    }
    let Some(active_job) = &status.active_job else {
        return rejected(
            SharePrecheckVerdict::RejectedNoActiveJob,
            "no active stratum job is available",
        );
    };
    if policy.require_matching_job_id && candidate.job_id != active_job.job_id {
        return rejected(
            SharePrecheckVerdict::RejectedStaleJob,
            "share job_id does not match active job",
        );
    }
    if policy.require_current_difficulty && status.current_difficulty.is_none() {
        return rejected(
            SharePrecheckVerdict::RejectedDifficultyMissing,
            "current pool difficulty is not known",
        );
    }

    SharePrecheckResult {
        verdict: SharePrecheckVerdict::AcceptedForSubmit,
        submit_allowed: policy.enabled_in_build,
        reasons: if policy.enabled_in_build {
            vec!["share passed local precheck".to_string()]
        } else {
            vec!["share passed local precheck but submit is disabled in build 0.1.0".to_string()]
        },
    }
}

fn rejected(verdict: SharePrecheckVerdict, reason: &str) -> SharePrecheckResult {
    SharePrecheckResult {
        verdict,
        submit_allowed: false,
        reasons: vec![reason.to_string()],
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StratumJobTemplate {
    pub job_id: String,
    pub prev_hash: String,
    pub merkle_branch_len: usize,
    pub version: String,
    pub bits: String,
    pub time: String,
    pub clean_jobs: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StratumMessageKind {
    MiningNotify,
    MiningSetDifficulty,
    SubscribeResult,
    AuthorizeResult,
    SubmitResult,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StratumMessageClassification {
    pub kind: StratumMessageKind,
    pub id: Option<Value>,
    pub method: Option<String>,
    pub notify: Option<StratumJobTemplate>,
    pub difficulty: Option<f64>,
    pub result_success: Option<bool>,
}

pub fn classify_stratum_message(
    input: &str,
) -> Result<StratumMessageClassification, StratumMessageError> {
    let value: Value = serde_json::from_str(input).map_err(StratumMessageError::Json)?;
    let object = value
        .as_object()
        .ok_or(StratumMessageError::MessageMustBeObject)?;
    let id = object.get("id").cloned();
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    match method.as_deref() {
        Some("mining.notify") => {
            let params = object.get("params").and_then(Value::as_array).ok_or(
                StratumMessageError::InvalidParams("notify params must be array"),
            )?;
            Ok(StratumMessageClassification {
                kind: StratumMessageKind::MiningNotify,
                id,
                method,
                notify: Some(parse_notify(params)?),
                difficulty: None,
                result_success: None,
            })
        }
        Some("mining.set_difficulty") => {
            let params = object.get("params").and_then(Value::as_array).ok_or(
                StratumMessageError::InvalidParams("set_difficulty params must be array"),
            )?;
            let difficulty = params.first().and_then(Value::as_f64).ok_or(
                StratumMessageError::InvalidParams("set_difficulty requires numeric difficulty"),
            )?;
            if !difficulty.is_finite() || difficulty <= 0.0 {
                return Err(StratumMessageError::InvalidParams(
                    "difficulty must be positive and finite",
                ));
            }
            Ok(StratumMessageClassification {
                kind: StratumMessageKind::MiningSetDifficulty,
                id,
                method,
                notify: None,
                difficulty: Some(difficulty),
                result_success: None,
            })
        }
        _ if method.is_none() && object.contains_key("result") => {
            Ok(StratumMessageClassification {
                kind: classify_result(id.as_ref(), object.get("result")),
                id,
                method,
                notify: None,
                difficulty: None,
                result_success: object.get("result").and_then(Value::as_bool),
            })
        }
        _ => Ok(StratumMessageClassification {
            kind: StratumMessageKind::Unknown,
            id,
            method,
            notify: None,
            difficulty: None,
            result_success: None,
        }),
    }
}

fn parse_notify(params: &[Value]) -> Result<StratumJobTemplate, StratumMessageError> {
    if params.len() < 9 {
        return Err(StratumMessageError::InvalidParams(
            "notify requires at least 9 params",
        ));
    }

    let job_id = string_param(params, 0, "job_id")?;
    let prev_hash = hex_param(params, 1, "prev_hash")?;
    let merkle_branch =
        params
            .get(4)
            .and_then(Value::as_array)
            .ok_or(StratumMessageError::InvalidParams(
                "notify merkle_branch must be array",
            ))?;
    for branch in merkle_branch {
        let branch = branch.as_str().ok_or(StratumMessageError::InvalidParams(
            "merkle branch entries must be hex strings",
        ))?;
        validate_hex(branch, "merkle branch entry")?;
    }

    Ok(StratumJobTemplate {
        job_id,
        prev_hash,
        merkle_branch_len: merkle_branch.len(),
        version: hex_param(params, 5, "version")?,
        bits: hex_param(params, 6, "bits")?,
        time: hex_param(params, 7, "time")?,
        clean_jobs: params.get(8).and_then(Value::as_bool).ok_or(
            StratumMessageError::InvalidParams("notify clean_jobs must be bool"),
        )?,
    })
}

fn string_param(
    params: &[Value],
    index: usize,
    name: &'static str,
) -> Result<String, StratumMessageError> {
    params
        .get(index)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or(StratumMessageError::InvalidParams(name))
}

fn hex_param(
    params: &[Value],
    index: usize,
    name: &'static str,
) -> Result<String, StratumMessageError> {
    let value = string_param(params, index, name)?;
    validate_hex(&value, name)?;
    Ok(value)
}

fn validate_hex(value: &str, name: &'static str) -> Result<(), StratumMessageError> {
    if value.is_empty()
        || value.len() % 2 != 0
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(StratumMessageError::InvalidHex(name));
    }
    Ok(())
}

fn validate_fixed_hex(value: &str, expected_len: usize) -> Result<(), StratumMessageError> {
    if value.len() != expected_len {
        return Err(StratumMessageError::InvalidHex("fixed hex"));
    }
    validate_hex(value, "fixed hex")
}

fn classify_result(id: Option<&Value>, result: Option<&Value>) -> StratumMessageKind {
    match (id.and_then(Value::as_i64), result) {
        (Some(1), Some(_)) => StratumMessageKind::SubscribeResult,
        (Some(2), Some(_)) => StratumMessageKind::AuthorizeResult,
        (Some(4), Some(_)) => StratumMessageKind::SubmitResult,
        _ => StratumMessageKind::Unknown,
    }
}

#[derive(Debug, Error)]
pub enum StratumMessageError {
    #[error("invalid stratum json: {0}")]
    Json(#[source] serde_json::Error),
    #[error("stratum message must be a json object")]
    MessageMustBeObject,
    #[error("invalid stratum params: {0}")]
    InvalidParams(&'static str),
    #[error("invalid hex field: {0}")]
    InvalidHex(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PoolConnectionPolicy;

    #[test]
    fn planned_status_never_opens_socket_or_claims_authorization() {
        let pipeline = JobPipelinePolicy::from(PoolConnectionPolicy::default());
        let status = StratumEngineStatus::planned(Some(0), &pipeline);

        assert_eq!(status.state, StratumEngineState::PlannedNoSocket);
        assert_eq!(status.active_pool_priority, Some(0));
        assert!(!status.socket_open);
        assert!(!status.subscribed);
        assert!(!status.authorized);
        assert_eq!(status.pending_jobs, 0);
        assert!(!status.submit_policy.enabled_in_build);
        assert!(status.submit_policy.local_precheck_required);
    }

    #[test]
    fn classifies_notify_preview_without_coinbase_secrets() {
        let message = r#"{
            "id": null,
            "method": "mining.notify",
            "params": [
                "job-1",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "abcd",
                "ef01",
                ["0011", "2233"],
                "20000000",
                "1d00ffff",
                "5f5e1000",
                true
            ]
        }"#;

        let classified = classify_stratum_message(message).unwrap();
        let notify = classified.notify.unwrap();

        assert_eq!(classified.kind, StratumMessageKind::MiningNotify);
        assert_eq!(notify.job_id, "job-1");
        assert_eq!(notify.merkle_branch_len, 2);
        assert!(notify.clean_jobs);
    }

    #[test]
    fn classifies_positive_set_difficulty() {
        let classified = classify_stratum_message(
            r#"{"id": null, "method": "mining.set_difficulty", "params": [8192]}"#,
        )
        .unwrap();

        assert_eq!(classified.kind, StratumMessageKind::MiningSetDifficulty);
        assert_eq!(classified.difficulty, Some(8192.0));
    }

    #[test]
    fn rejects_notify_with_invalid_hex() {
        let err = classify_stratum_message(
            r#"{
                "id": null,
                "method": "mining.notify",
                "params": ["job-1", "not-hex", "abcd", "ef01", [], "20000000", "1d00ffff", "5f5e1000", true]
            }"#,
        )
        .unwrap_err();

        assert!(matches!(err, StratumMessageError::InvalidHex("prev_hash")));
    }

    #[test]
    fn rejects_non_positive_difficulty() {
        let err = classify_stratum_message(
            r#"{"id": null, "method": "mining.set_difficulty", "params": [0]}"#,
        )
        .unwrap_err();

        assert!(matches!(err, StratumMessageError::InvalidParams(_)));
    }

    #[test]
    fn share_precheck_rejects_invalid_nonce_before_socket_state() {
        let pipeline = JobPipelinePolicy::from(PoolConnectionPolicy::default());
        let status = StratumEngineStatus::planned(Some(0), &pipeline);
        let candidate = share_candidate("job-1", "bad");
        let result = precheck_share_submit(&candidate, &status);

        assert_eq!(result.verdict, SharePrecheckVerdict::RejectedInvalidField);
        assert!(!result.submit_allowed);
    }

    #[test]
    fn share_precheck_rejects_when_socket_is_closed() {
        let pipeline = JobPipelinePolicy::from(PoolConnectionPolicy::default());
        let status = StratumEngineStatus::planned(Some(0), &pipeline);
        let candidate = share_candidate("job-1", "00000001");
        let result = precheck_share_submit(&candidate, &status);

        assert_eq!(result.verdict, SharePrecheckVerdict::RejectedSocketClosed);
        assert!(!result.submit_allowed);
    }

    #[test]
    fn share_precheck_rejects_stale_job_ids() {
        let pipeline = JobPipelinePolicy::from(PoolConnectionPolicy::default());
        let mut status = StratumEngineStatus::planned(Some(0), &pipeline);
        status.socket_open = true;
        status.subscribed = true;
        status.authorized = true;
        status.current_difficulty = Some(8192.0);
        status.active_job = Some(job_template("active-job"));
        let candidate = share_candidate("old-job", "00000001");
        let result = precheck_share_submit(&candidate, &status);

        assert_eq!(result.verdict, SharePrecheckVerdict::RejectedStaleJob);
        assert!(!result.submit_allowed);
    }

    #[test]
    fn share_precheck_accepts_locally_but_submit_stays_disabled_in_build() {
        let pipeline = JobPipelinePolicy::from(PoolConnectionPolicy::default());
        let mut status = StratumEngineStatus::planned(Some(0), &pipeline);
        status.socket_open = true;
        status.subscribed = true;
        status.authorized = true;
        status.current_difficulty = Some(8192.0);
        status.active_job = Some(job_template("active-job"));
        let candidate = share_candidate("active-job", "00000001");
        let result = precheck_share_submit(&candidate, &status);

        assert_eq!(result.verdict, SharePrecheckVerdict::AcceptedForSubmit);
        assert!(!result.submit_allowed);
        assert!(result.reasons[0].contains("disabled"));
    }

    fn share_candidate(job_id: &str, nonce: &str) -> StratumShareCandidate {
        StratumShareCandidate {
            worker: "acct.worker".to_string(),
            job_id: job_id.to_string(),
            extranonce2: "00000002".to_string(),
            ntime: "5f5e1000".to_string(),
            nonce: nonce.to_string(),
        }
    }

    fn job_template(job_id: &str) -> StratumJobTemplate {
        StratumJobTemplate {
            job_id: job_id.to_string(),
            prev_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            merkle_branch_len: 0,
            version: "20000000".to_string(),
            bits: "1d00ffff".to_string(),
            time: "5f5e1000".to_string(),
            clean_jobs: true,
        }
    }
}
