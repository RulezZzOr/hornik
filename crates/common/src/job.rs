use crate::PoolConnectionPolicy;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPipelineState {
    PlannedReadOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobPipelinePolicy {
    pub state: JobPipelineState,
    pub notify_to_dispatch_budget_ms: u32,
    pub stale_job_retirement_ms: u32,
    pub max_pending_jobs: u8,
    pub prefer_newest_job: bool,
    pub drop_stale_jobs: bool,
    pub reset_nonce_on_new_prev_hash: bool,
    pub notes: Vec<String>,
}

impl From<PoolConnectionPolicy> for JobPipelinePolicy {
    fn from(policy: PoolConnectionPolicy) -> Self {
        Self {
            state: JobPipelineState::PlannedReadOnly,
            notify_to_dispatch_budget_ms: policy.job_processing_budget_ms,
            stale_job_retirement_ms: policy.latency_warning_ms,
            max_pending_jobs: 2,
            prefer_newest_job: true,
            drop_stale_jobs: true,
            reset_nonce_on_new_prev_hash: true,
            notes: vec![
                "build 0.1.0 exposes the job pipeline contract before stratum networking is enabled".to_string(),
                "future mining code must prioritize the newest job and avoid mining stale work after a new prev_hash".to_string(),
                "the queue stays intentionally short to reduce latency between pool notify and ASIC dispatch".to_string(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_pipeline_uses_pool_job_budget_and_prefers_newest_jobs() {
        let policy = JobPipelinePolicy::from(PoolConnectionPolicy {
            latency_warning_ms: 300,
            job_processing_budget_ms: 25,
            ..PoolConnectionPolicy::default()
        });

        assert_eq!(policy.notify_to_dispatch_budget_ms, 25);
        assert_eq!(policy.stale_job_retirement_ms, 300);
        assert_eq!(policy.max_pending_jobs, 2);
        assert!(policy.prefer_newest_job);
        assert!(policy.drop_stale_jobs);
        assert!(policy.reset_nonce_on_new_prev_hash);
    }
}
