use anyhow::Context;
use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{Html, IntoResponse},
    routing::get,
};
use clap::Parser;
use openmineros_common::status::HealthStatusResponse;
use openmineros_common::{
    BoardFamily, ChainStatus, ContributionStatus, DashboardOverview, EventEnvelope, EventsResponse,
    MinerStatus, Model, PoolSummary, ProfilesResponse, RuntimeConfig, SupportBundle, SystemInfo,
    UpdateStatus,
};
use openmineros_supervisor::Supervisor;
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::time;
use tracing::info;

#[derive(Debug, Parser)]
#[command(name = "openmineros-control-plane")]
#[command(about = "OpenMinerOS local control plane")]
struct Cli {
    #[arg(long, default_value = "s19-xil")]
    board: String,
    #[arg(long, default_value = "s19j-pro")]
    model: String,
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
    let supervisor = Arc::new(Supervisor::with_config(model, board, config)?);

    let app = Router::new()
        .route("/", get(index))
        .route("/api/v1/overview", get(overview))
        .route("/api/v1/system/info", get(system_info))
        .route("/api/v1/system/health", get(system_health))
        .route("/api/v1/miner/status", get(miner_status))
        .route("/api/v1/chains", get(chains))
        .route("/api/v1/pools", get(pools))
        .route("/api/v1/profiles", get(profiles))
        .route("/api/v1/events", get(events))
        .route("/api/v1/ws", get(ws_events))
        .route("/api/v1/support/bundle", get(support_bundle))
        .route("/api/v1/update/status", get(update_status))
        .route("/api/v1/contribution/status", get(contribution_status))
        .route("/metrics", get(metrics))
        .with_state(supervisor);

    let listener = tokio::net::TcpListener::bind(cli.listen).await?;
    info!(
        "OpenMinerOS control plane listening on http://{}",
        cli.listen
    );
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../../../web/index.html"))
}

async fn overview(State(supervisor): State<Arc<Supervisor>>) -> Json<DashboardOverview> {
    Json(supervisor.dashboard_overview())
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

async fn chains(State(supervisor): State<Arc<Supervisor>>) -> Json<Vec<ChainStatus>> {
    Json(supervisor.chains())
}

async fn pools(State(supervisor): State<Arc<Supervisor>>) -> Json<PoolSummary> {
    Json(supervisor.pools())
}

async fn profiles(State(supervisor): State<Arc<Supervisor>>) -> Json<ProfilesResponse> {
    Json(supervisor.profiles())
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
