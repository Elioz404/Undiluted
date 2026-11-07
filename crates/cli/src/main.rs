mod demo;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use undiluted_client::{LocalTransport, OutLayerConfig, OutLayerTransport};

#[derive(Parser)]
#[command(name = "undiluted", about = "A confidential, verifiable cap table")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a fictional raise from start to finish.
    Demo {
        #[command(subcommand)]
        target: Target,
    },
}

#[derive(Subcommand)]
enum Target {
    /// Run the enclave code in this process. Needs no account, key or network.
    Local,
    /// Run against an enclave deployed on `OutLayer`, paying from your account.
    Outlayer {
        /// The deployed project, as owner/name.
        #[arg(long)]
        project: String,
        /// The account holding the enclave's secret.
        #[arg(long)]
        secrets_account: String,
        #[arg(long, default_value = "default")]
        secrets_profile: String,
        /// Path to the outlayer command-line tool.
        #[arg(long, default_value = "outlayer")]
        cli: PathBuf,
    },
}

fn main() -> ExitCode {
    let Commands::Demo { target } = Cli::parse().command;
    let result = match target {
        Target::Local => LocalTransport::new().and_then(demo::run),
        Target::Outlayer {
            project,
            secrets_account,
            secrets_profile,
            cli,
        } => demo::run(OutLayerTransport::new(OutLayerConfig {
            cli,
            project,
            secrets_account,
            secrets_profile,
        })),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
