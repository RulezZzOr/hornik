use anyhow::Context;
use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{Html, IntoResponse},
    routing::{get, post},
};
use clap::Parser;
use openmineros_common::StratumSubmitPolicy;
use openmineros_common::status::HealthStatusResponse;
use openmineros_common::{
    AntiBrickReport, BoardFamily, ChainStatus, ContributionStatus, DashboardOverview,
    EventEnvelope, EventsResponse, FirmwareDeploymentReport, HardwareIdentityReport,
    HardwareProbeReport, HardwareReadinessReport, HardwareSafetyGate, JobPipelinePolicy,
    MinerStatus, Model, PoolStrategyResponse, PoolSummary, ProfilesResponse, RuntimeBackendMode,
    RuntimeConfig, RuntimeControlReport, SharePrecheckResult, StratumEngineStatus,
    StratumShareCandidate, SupportBundle, SystemInfo, TargetCatalog, ThermalDecision,
    TuningExecutionStatus, TuningPlanResponse, TuningProtocolTranscript, UpdateStatus,
    target_catalog,
};
use openmineros_supervisor::Supervisor;
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::time;
use tracing::info;

#[cfg(test)]
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};

#[derive(Debug, Parser)]
#[command(name = "openmineros-control-plane")]
#[command(about = "OpenMinerOS local control plane")]
struct Cli {
    #[arg(long, default_value = "s19-xil")]
    board: String,
    #[arg(long, default_value = "s19j-pro")]
    model: String,
    #[arg(long, default_value_t = RuntimeBackendMode::Simulated)]
    backend: RuntimeBackendMode,
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: SocketAddr,
    #[arg(long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "openmineros_control_plane=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let board: BoardFamily = cli.board.parse().context("invalid board")?;
    let model: Model = cli.model.parse().context("invalid model")?;
    let config = match cli.config {
        Some(path) => RuntimeConfig::from_file(&path)
            .with_context(|| format!("invalid config {}", path.display()))?,
        None => RuntimeConfig::default(),
    };
    let supervisor = Arc::new(Supervisor::with_backend_mode(
        model,
        board,
        config,
        cli.backend,
    )?);

    let app = build_app(supervisor);

    let listener = tokio::net::TcpListener::bind(cli.listen).await?;
    info!(
        "OpenMinerOS control plane listening on http://{}",
        cli.listen
    );
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_app(supervisor: Arc<Supervisor>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/v1/overview", get(overview))
        .route("/api/v1/hardware/targets", get(hardware_targets))
        .route("/api/v1/hardware/identity", get(hardware_identity))
        .route("/api/v1/hardware/safety", get(hardware_safety))
        .route("/api/v1/hardware/readiness", get(hardware_readiness))
        .route("/api/v1/hardware/probe", get(hardware_probe))
        .route("/api/v1/system/info", get(system_info))
        .route("/api/v1/system/health", get(system_health))
        .route("/api/v1/miner/status", get(miner_status))
        .route("/api/v1/miner/job-pipeline", get(job_pipeline))
        .route("/api/v1/chains", get(chains))
        .route("/api/v1/thermal", get(thermal))
        .route("/api/v1/pools", get(pools))
        .route("/api/v1/pools/strategy", get(pool_strategy))
        .route("/api/v1/stratum/status", get(stratum_status))
        .route("/api/v1/stratum/submit-policy", get(stratum_submit_policy))
        .route("/api/v1/stratum/submit-share", post(stratum_submit_share))
        .route("/api/v1/runtime/control", get(runtime_control))
        .route("/api/v1/board/pause", post(board_pause))
        .route("/api/v1/board/resume", post(board_resume))
        .route("/api/v1/tuning/lock", post(tuning_lock))
        .route("/api/v1/tuning/unlock", post(tuning_unlock))
        .route("/api/v1/profiles", get(profiles))
        .route("/api/v1/firmware/deployment", get(firmware_deployment))
        .route("/api/v1/firmware/anti-brick", get(firmware_anti_brick))
        .route("/api/v1/tuning/plan", get(tuning_plan))
        .route("/api/v1/tuning/execution", get(tuning_execution))
        .route("/api/v1/tuning/transcript", get(tuning_transcript))
        .route("/api/v1/events", get(events))
        .route("/api/v1/ws", get(ws_events))
        .route("/api/v1/support/bundle", get(support_bundle))
        .route("/api/v1/update/status", get(update_status))
        .route("/api/v1/contribution/status", get(contribution_status))
        .route("/metrics", get(metrics))
        .with_state(supervisor)
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../../../web/index.html"))
}

async fn overview(State(supervisor): State<Arc<Supervisor>>) -> Json<DashboardOverview> {
    Json(supervisor.dashboard_overview())
}

async fn hardware_targets() -> Json<TargetCatalog> {
    Json(target_catalog())
}

async fn hardware_identity(
    State(supervisor): State<Arc<Supervisor>>,
) -> Json<HardwareIdentityReport> {
    Json(supervisor.hardware_identity_report())
}

async fn hardware_safety(State(supervisor): State<Arc<Supervisor>>) -> Json<HardwareSafetyGate> {
    Json(supervisor.hardware_safety_gate())
}

async fn hardware_readiness(
    State(supervisor): State<Arc<Supervisor>>,
) -> Json<HardwareReadinessReport> {
    Json(supervisor.hardware_readiness_report())
}

async fn hardware_probe(State(supervisor): State<Arc<Supervisor>>) -> Json<HardwareProbeReport> {
    Json(supervisor.hardware_probe_report())
}

async fn system_info(State(supervisor): State<Arc<Supervisor>>) -> Json<SystemInfo> {
    Json(supervisor.system_info())
}

async fn system_health(State(supervisor): State<Arc<Supervisor>>) -> Json<HealthStatusResponse> {
    Json(supervisor.health())
}

async fn miner_status(State(supervisor): State<Arc<Supervisor>>) -> Json<MinerStatus> {
    Json(supervisor.miner_status())
}

async fn job_pipeline(State(supervisor): State<Arc<Supervisor>>) -> Json<JobPipelinePolicy> {
    Json(supervisor.job_pipeline())
}

async fn chains(State(supervisor): State<Arc<Supervisor>>) -> Json<Vec<ChainStatus>> {
    Json(supervisor.chains())
}

async fn thermal(State(supervisor): State<Arc<Supervisor>>) -> Json<ThermalDecision> {
    Json(supervisor.thermal_status())
}

async fn pools(State(supervisor): State<Arc<Supervisor>>) -> Json<PoolSummary> {
    Json(supervisor.pools())
}

async fn pool_strategy(State(supervisor): State<Arc<Supervisor>>) -> Json<PoolStrategyResponse> {
    Json(supervisor.pool_strategy())
}

async fn stratum_status(State(supervisor): State<Arc<Supervisor>>) -> Json<StratumEngineStatus> {
    Json(supervisor.stratum_status())
}

async fn stratum_submit_policy(
    State(supervisor): State<Arc<Supervisor>>,
) -> Json<StratumSubmitPolicy> {
    Json(supervisor.stratum_submit_policy())
}

async fn stratum_submit_share(
    State(supervisor): State<Arc<Supervisor>>,
    Json(candidate): Json<StratumShareCandidate>,
) -> Json<SharePrecheckResult> {
    Json(supervisor.submit_share(candidate))
}

async fn runtime_control(State(supervisor): State<Arc<Supervisor>>) -> Json<RuntimeControlReport> {
    Json(supervisor.runtime_control_report())
}

async fn board_pause(State(supervisor): State<Arc<Supervisor>>) -> Json<RuntimeControlReport> {
    Json(supervisor.pause_board(None))
}

async fn board_resume(State(supervisor): State<Arc<Supervisor>>) -> Json<RuntimeControlReport> {
    Json(supervisor.resume_board())
}

async fn tuning_lock(State(supervisor): State<Arc<Supervisor>>) -> Json<RuntimeControlReport> {
    Json(supervisor.lock_tuning_profile())
}

async fn tuning_unlock(State(supervisor): State<Arc<Supervisor>>) -> Json<RuntimeControlReport> {
    Json(supervisor.unlock_tuning_profile())
}

async fn profiles(State(supervisor): State<Arc<Supervisor>>) -> Json<ProfilesResponse> {
    Json(supervisor.profiles())
}

async fn firmware_deployment(
    State(supervisor): State<Arc<Supervisor>>,
) -> Json<FirmwareDeploymentReport> {
    Json(supervisor.firmware_deployment_report())
}

async fn firmware_anti_brick(State(supervisor): State<Arc<Supervisor>>) -> Json<AntiBrickReport> {
    Json(supervisor.anti_brick_report())
}

async fn tuning_plan(State(supervisor): State<Arc<Supervisor>>) -> Json<TuningPlanResponse> {
    Json(supervisor.tuning_plan())
}

async fn tuning_execution(
    State(supervisor): State<Arc<Supervisor>>,
) -> Json<TuningExecutionStatus> {
    Json(supervisor.tuning_execution_status())
}

async fn tuning_transcript(
    State(supervisor): State<Arc<Supervisor>>,
) -> Json<TuningProtocolTranscript> {
    Json(supervisor.tuning_transcript())
}

async fn events(State(supervisor): State<Arc<Supervisor>>) -> Json<EventsResponse> {
    Json(supervisor.events())
}

async fn ws_events(
    State(supervisor): State<Arc<Supervisor>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_event_stream(socket, supervisor))
}

async fn ws_event_stream(mut socket: WebSocket, supervisor: Arc<Supervisor>) {
    for event in supervisor.events().events {
        let envelope = EventEnvelope::from_record(event);
        if send_ws_json(&mut socket, &envelope).await.is_err() {
            return;
        }
    }

    let mut interval = time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        let heartbeat = EventEnvelope::heartbeat(supervisor.system_info().uptime_seconds);
        if send_ws_json(&mut socket, &heartbeat).await.is_err() {
            return;
        }
    }
}

async fn send_ws_json(
    socket: &mut WebSocket,
    value: &impl serde::Serialize,
) -> Result<(), axum::Error> {
    let message = match serde_json::to_string(value) {
        Ok(message) => message,
        Err(error) => {
            return Err(axum::Error::new(error));
        }
    };
    socket.send(Message::Text(message.into())).await
}

async fn support_bundle(State(supervisor): State<Arc<Supervisor>>) -> Json<SupportBundle> {
    Json(supervisor.support_bundle())
}

async fn update_status(State(supervisor): State<Arc<Supervisor>>) -> Json<UpdateStatus> {
    Json(supervisor.update_status())
}

async fn contribution_status(
    State(supervisor): State<Arc<Supervisor>>,
) -> Json<ContributionStatus> {
    Json(supervisor.contribution_status())
}

async fn metrics(State(supervisor): State<Arc<Supervisor>>) -> String {
    supervisor.prometheus_metrics()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openmineros_common::{
        AntiBrickReport, AntiBrickState, RuntimeConfig, RuntimeControlReport, SharePrecheckVerdict,
    };
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn post_submit_share_returns_precheck_result() {
        let supervisor = Arc::new(
            Supervisor::with_backend_mode(
                Model::S19jPro,
                BoardFamily::Xilinx,
                RuntimeConfig::default(),
                RuntimeBackendMode::Simulated,
            )
            .unwrap(),
        );
        let app = build_app(supervisor);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/stratum/submit-share")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"worker":"acct.worker","job_id":"job-1","extranonce2":"00000002","ntime":"5f5e1000","nonce":"00000001"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let result: SharePrecheckResult = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.verdict, SharePrecheckVerdict::RejectedSocketClosed);
        assert!(!result.submit_allowed);
    }

    #[tokio::test]
    async fn post_submit_share_rejects_invalid_payload() {
        let supervisor = Arc::new(
            Supervisor::with_backend_mode(
                Model::S19jPro,
                BoardFamily::Xilinx,
                RuntimeConfig::default(),
                RuntimeBackendMode::Simulated,
            )
            .unwrap(),
        );
        let app = build_app(supervisor);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/stratum/submit-share")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"worker":"acct.worker","job_id":"job-1"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn get_tuning_transcript_returns_protocol_frames() {
        let supervisor = Arc::new(
            Supervisor::with_backend_mode(
                Model::S19jPro,
                BoardFamily::Xilinx,
                RuntimeConfig::default(),
                RuntimeBackendMode::Simulated,
            )
            .unwrap(),
        );
        let app = build_app(supervisor);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/tuning/transcript")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let transcript: TuningProtocolTranscript = serde_json::from_slice(&body).unwrap();

        assert_eq!(transcript.schema_version, 1);
        assert_eq!(transcript.board_family, BoardFamily::Xilinx);
        assert_eq!(transcript.chip_id, 0);
        assert_eq!(transcript.frames.len(), 4);
        assert_eq!(transcript.frames[0].target_frequency_mhz, 725);
        assert_eq!(transcript.frames[0].target_voltage_mv, 800);
        assert!(transcript.frames[0].frame.contains("cmd=set_frequency"));
        assert!(transcript.frames[3].frame.contains("cmd=set_voltage"));
    }

    #[tokio::test]
    async fn get_tuning_execution_returns_structured_steps() {
        let supervisor = Arc::new(
            Supervisor::with_backend_mode(
                Model::S19jPro,
                BoardFamily::Xilinx,
                RuntimeConfig::default(),
                RuntimeBackendMode::Simulated,
            )
            .unwrap(),
        );
        let app = build_app(supervisor);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/tuning/execution")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let execution: TuningExecutionStatus = serde_json::from_slice(&body).unwrap();

        assert_eq!(execution.schema_version, 1);
        assert_eq!(
            execution.state,
            openmineros_common::TuningExecutionState::Disabled
        );
        assert!(!execution.autotune);
        assert_eq!(execution.current_step.unwrap().order, 1);
        assert_eq!(execution.queued_steps.len(), 3);
        assert!(
            execution
                .blocked_reason
                .unwrap()
                .contains("autotune is disabled")
        );
    }

    #[tokio::test]
    async fn board_pause_and_resume_update_runtime_control() {
        let supervisor = Arc::new(
            Supervisor::with_backend_mode(
                Model::S19jPro,
                BoardFamily::Xilinx,
                RuntimeConfig::default(),
                RuntimeBackendMode::Simulated,
            )
            .unwrap(),
        );
        let app = build_app(supervisor);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/board/pause")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let report: RuntimeControlReport = serde_json::from_slice(&body).unwrap();
        assert!(report.board_paused);
        assert_eq!(
            report.tuning_lock_state,
            openmineros_common::TuningLockState::Searching
        );

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/runtime/control")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let report: RuntimeControlReport = serde_json::from_slice(&body).unwrap();
        assert!(report.board_paused);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/board/resume")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let report: RuntimeControlReport = serde_json::from_slice(&body).unwrap();
        assert!(!report.board_paused);
    }

    #[tokio::test]
    async fn tuning_lock_and_unlock_update_runtime_control() {
        let supervisor = Arc::new(
            Supervisor::with_backend_mode(
                Model::S19jPro,
                BoardFamily::Xilinx,
                RuntimeConfig::default(),
                RuntimeBackendMode::Simulated,
            )
            .unwrap(),
        );
        let app = build_app(supervisor);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tuning/lock")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let report: RuntimeControlReport = serde_json::from_slice(&body).unwrap();
        assert!(report.tuning_locked);
        assert!(report.locked_profile.is_some());

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tuning/unlock")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let report: RuntimeControlReport = serde_json::from_slice(&body).unwrap();
        assert!(!report.tuning_locked);
        assert!(report.locked_profile.is_none());
    }

    #[tokio::test]
    async fn paused_board_rejects_share_submit() {
        let supervisor = Arc::new(
            Supervisor::with_backend_mode(
                Model::S19jPro,
                BoardFamily::Xilinx,
                RuntimeConfig::default(),
                RuntimeBackendMode::Simulated,
            )
            .unwrap(),
        );
        supervisor.pause_board(None);
        let app = build_app(supervisor);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/stratum/submit-share")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"worker":"acct.worker","job_id":"job-1","extranonce2":"00000002","ntime":"5f5e1000","nonce":"00000001"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let result: SharePrecheckResult = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.verdict, SharePrecheckVerdict::RejectedBoardPaused);
        assert!(!result.submit_allowed);
    }

    #[tokio::test]
    async fn get_firmware_deployment_returns_missing_gaps() {
        let supervisor = Arc::new(
            Supervisor::with_backend_mode(
                Model::S19jPro,
                BoardFamily::Xilinx,
                RuntimeConfig::default(),
                RuntimeBackendMode::Simulated,
            )
            .unwrap(),
        );
        let app = build_app(supervisor);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/firmware/deployment")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let report: FirmwareDeploymentReport = serde_json::from_slice(&body).unwrap();

        assert_eq!(report.schema_version, 1);
        assert!(!report.deployable);
        assert!(report.gaps.iter().any(|gap| gap.key == "asic_transport"));
        assert!(report.gaps.iter().any(|gap| gap.key == "bootable_image"));
        assert!(report.gaps.iter().any(|gap| gap.key == "physical_probe"));
    }

    #[tokio::test]
    async fn get_firmware_anti_brick_reports_safe_probe_boot() {
        let supervisor = Arc::new(
            Supervisor::with_backend_mode(
                Model::S19jPro,
                BoardFamily::Xilinx,
                RuntimeConfig::default(),
                RuntimeBackendMode::HardwareProbe,
            )
            .unwrap(),
        );
        let app = build_app(supervisor);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/firmware/anti-brick")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let report: AntiBrickReport = serde_json::from_slice(&body).unwrap();

        assert_eq!(report.schema_version, 1);
        assert_eq!(report.state, AntiBrickState::SafeFirstBoot);
        assert!(report.safe_to_first_boot);
        assert!(!report.flashing_allowed);
        assert!(!report.nand_writes_allowed);
        assert!(!report.asic_writes_allowed);
    }
}
