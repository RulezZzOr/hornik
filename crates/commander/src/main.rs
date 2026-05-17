use anyhow::Context;
use clap::{Parser, Subcommand};
use openmineros_common::{
    ArtifactBudget, BoardFamily, ContributionStatus, Model, ProfilesResponse, ReleaseManifest,
    RuntimeConfig, SupportLevel, check_manifest_budget_file, load_release_manifest_file,
    summarize_pools, supported_targets, target_catalog, verify_manifest_file,
};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "omo-commander")]
#[command(about = "OpenMinerOS fleet helper")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Matrix,
    Identify {
        #[arg(long)]
        board: String,
        #[arg(long)]
        model: String,
    },
    Scan {
        #[arg(long, default_value = "192.168.1.0/24")]
        subnet: String,
    },
    VerifyManifest {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        artifact_dir: Option<PathBuf>,
        #[arg(long)]
        allow_flashable_unsigned: bool,
    },
    CheckManifestBudget {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        artifact_dir: Option<PathBuf>,
        #[arg(long, default_value_t = ArtifactBudget::default().max_install_image_bytes)]
        max_install_image_bytes: u64,
        #[arg(long, default_value_t = ArtifactBudget::default().max_total_artifact_bytes)]
        max_total_artifact_bytes: u64,
    },
    InstallPlan {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        artifact_dir: Option<PathBuf>,
        #[arg(long)]
        allow_flashable_unsigned: bool,
    },
    ValidateConfig {
        #[arg(long)]
        config: PathBuf,
    },
}

#[derive(Debug, Serialize)]
struct InstallArtifactSummary {
    path: String,
    kind: String,
    sha256: String,
    bytes: u64,
}

#[derive(Debug, Serialize)]
struct InstallPlanReport {
    manifest: String,
    artifact_dir: String,
    board: String,
    model: String,
    flashable: bool,
    signature_required: bool,
    signature_present: bool,
    install_ready: bool,
    install_image: Option<InstallArtifactSummary>,
    verified_artifacts: Vec<String>,
    notes: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Matrix => {
            println!("{}", serde_json::to_string_pretty(&target_catalog())?);
        }
        Command::Identify { board, model } => {
            let board: BoardFamily = board.parse().context("invalid board")?;
            let model: Model = model.parse().context("invalid model")?;
            let support = supported_targets()
                .into_iter()
                .find(|target| target.board == board && target.model == model)
                .map(|target| target.support)
                .unwrap_or(SupportLevel::Unsupported);

            println!(
                "{}",
                serde_json::json!({
                    "board": board,
                    "model": model,
                    "support": support,
                    "flashable_by_commander": support == SupportLevel::MvpStable
                })
            );
        }
        Command::Scan { subnet } => {
            println!(
                "{}",
                serde_json::json!({
                    "subnet": subnet,
                    "devices": [],
                    "note": "network probing is intentionally not implemented in build 0.1.0"
                })
            );
        }
        Command::VerifyManifest {
            manifest,
            artifact_dir,
            allow_flashable_unsigned,
        } => {
            let artifact_dir = artifact_dir.unwrap_or_else(|| {
                manifest
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."))
            });
            let report = verify_manifest_file(&manifest, artifact_dir, allow_flashable_unsigned)?;

            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::CheckManifestBudget {
            manifest,
            artifact_dir,
            max_install_image_bytes,
            max_total_artifact_bytes,
        } => {
            let artifact_dir = artifact_dir.unwrap_or_else(|| {
                manifest
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."))
            });
            let report = check_manifest_budget_file(
                &manifest,
                artifact_dir,
                ArtifactBudget {
                    max_install_image_bytes,
                    max_total_artifact_bytes,
                },
            )?;

            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::InstallPlan {
            manifest,
            artifact_dir,
            allow_flashable_unsigned,
        } => {
            let artifact_dir = artifact_dir.unwrap_or_else(|| {
                manifest
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."))
            });

            let verification =
                verify_manifest_file(&manifest, &artifact_dir, allow_flashable_unsigned)?;
            let release: ReleaseManifest = load_release_manifest_file(&manifest)?;
            let _budget_report =
                check_manifest_budget_file(&manifest, &artifact_dir, ArtifactBudget::default())?;
            let install_image = release
                .artifacts
                .iter()
                .find(|artifact| artifact.kind == "install-image")
                .map(|artifact| InstallArtifactSummary {
                    path: artifact.path.clone(),
                    kind: artifact.kind.clone(),
                    sha256: artifact.sha256.clone(),
                    bytes: artifact.bytes,
                });

            let install_ready = release.flashable && install_image.is_some();

            let notes = if release.flashable {
                vec![
                    "flashable release verified for installation planning".to_string(),
                    "use INSTALL.md for the board-specific recovery path".to_string(),
                ]
            } else {
                vec![
                    "this build is not flashable yet".to_string(),
                    "the current artifact packages the compiled device-side runtime and safe boot hooks"
                        .to_string(),
                ]
            };

            let report = InstallPlanReport {
                manifest: manifest.display().to_string(),
                artifact_dir: artifact_dir.display().to_string(),
                board: release.board,
                model: release.model,
                flashable: release.flashable,
                signature_required: verification.signature_required,
                signature_present: verification.signature_present,
                install_ready,
                install_image,
                verified_artifacts: verification.verified_artifacts,
                notes,
            };

            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::ValidateConfig { config } => {
            let config = RuntimeConfig::from_file(config)?;
            let contribution = ContributionStatus::from(config.contribution);

            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "valid": true,
                    "contribution": {
                        "enabled": contribution.enabled,
                        "rate_percent": contribution.rate_percent,
                        "target_locked": contribution.target_locked,
                        "mutable_fields": contribution.mutable_fields,
                        "beneficiary": contribution.beneficiary,
                        "endpoints": contribution.endpoints,
                    },
                    "tuning": ProfilesResponse::from(config.tuning),
                    "pools": summarize_pools(&config.pools),
                }))?
            );
        }
    }

    Ok(())
}
