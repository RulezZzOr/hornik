use anyhow::Context;
use clap::{Parser, Subcommand};
use openmineros_common::{BoardFamily, Model, SupportLevel, supported_targets};

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
    }

    Ok(())
}
