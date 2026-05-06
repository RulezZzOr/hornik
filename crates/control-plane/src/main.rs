use anyhow::Context;
use axum::{Json, Router, extract::State, response::Html, routing::get};
use clap::Parser;
use openmineros_common::status::HealthStatusResponse;
use openmineros_common::{
    BoardFamily, ChainStatus, ContributionStatus, EventsResponse, MinerStatus, Model, PoolSummary,
    ProfilesResponse, RuntimeConfig, SystemInfo,
};
use openmineros_supervisor::Supervisor;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
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
        .route("/api/v1/system/info", get(system_info))
        .route("/api/v1/system/health", get(system_health))
        .route("/api/v1/miner/status", get(miner_status))
        .route("/api/v1/chains", get(chains))
        .route("/api/v1/pools", get(pools))
        .route("/api/v1/profiles", get(profiles))
        .route("/api/v1/events", get(events))
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

async fn contribution_status(
    State(supervisor): State<Arc<Supervisor>>,
) -> Json<ContributionStatus> {
    Json(supervisor.contribution_status())
}

async fn metrics(State(supervisor): State<Arc<Supervisor>>) -> String {
    let miner = supervisor.miner_status();
    let contribution = supervisor.contribution_status();
    let mut output = String::new();

    output.push_str(&format!("omo_miner_hashrate_ths {}\n", miner.hashrate_ths));
    output.push_str(&format!("omo_miner_power_watts {}\n", miner.power_w));
    output.push_str(&format!(
        "omo_miner_efficiency_j_th {}\n",
        miner.efficiency_j_th
    ));
    output.push_str(&format!(
        "omo_shares_accepted_total {}\n",
        miner.accepted_shares
    ));
    output.push_str(&format!(
        "omo_shares_rejected_total {}\n",
        miner.rejected_shares
    ));
    output.push_str(&format!(
        "omo_contribution_rate_percent {}\n",
        contribution.rate_percent
    ));

    for chain in supervisor.chains() {
        output.push_str(&format!(
            "omo_chain_up{{chain=\"{}\"}} {}\n",
            chain.id,
            u8::from(chain.present && chain.enabled)
        ));
        output.push_str(&format!(
            "omo_chain_asic_detected{{chain=\"{}\"}} {}\n",
            chain.id, chain.asic_detected
        ));
        output.push_str(&format!(
            "omo_temp_board_celsius{{chain=\"{}\"}} {}\n",
            chain.id, chain.temp_board_c
        ));
        output.push_str(&format!(
            "omo_temp_chip_max_celsius{{chain=\"{}\"}} {}\n",
            chain.id, chain.temp_chip_max_c
        ));
    }

    output
}
