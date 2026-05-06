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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StratumConnectionState {
    NotStarted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareValidationMode {
    PlannedLocalPrecheck,
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

fn classify_result(id: Option<&Value>, result: Option<&Value>) -> StratumMessageKind {
    match (id.and_then(Value::as_i64), result) {
        (Some(1), Some(_)) => StratumMessageKind::SubscribeResult,
        (Some(2), Some(_)) => StratumMessageKind::AuthorizeResult,
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
}
