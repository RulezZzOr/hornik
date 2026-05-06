use anyhow::Context;
use clap::{Parser, Subcommand};
use openmineros_common::{
    BoardFamily, Model, SupportLevel, supported_targets, verify_manifest_file,
};
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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Matrix => {
            println!("{}", serde_json::to_string_pretty(&supported_targets())?);
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
    }

    Ok(())
}
