mod demo;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use undiluted_client::{
    ClientError, LocalAnchors, LocalTransport, NearAnchors, NearAnchorsConfig, OutLayerConfig,
    OutLayerTransport,
};

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
    /// Run the enclave code and anchor rules in this process. Needs no account,
    /// key or network.
    Local,
    /// Run against a deployed enclave and anchor contract, paying from your account.
    Deployed(Box<Deployment>),
}

#[derive(Args)]
struct Deployment {
    /// The deployed enclave project, as owner/name.
    #[arg(long)]
    project: String,
    /// The account holding the enclave's secret.
    #[arg(long)]
    secrets_account: String,
    #[arg(long, default_value = "default")]
    secrets_profile: String,
    /// Path to the outlayer command-line tool.
    #[arg(long, default_value = "outlayer")]
    outlayer_cli: PathBuf,
    /// The account running the anchor contract.
    #[arg(long)]
    anchor_contract: String,
    /// The account that relays anchors and pays for their storage.
    #[arg(long)]
    anchor_signer: String,
    /// Path to the near command-line tool.
    #[arg(long, default_value = "near")]
    near_cli: PathBuf,
    #[arg(long, default_value = "testnet")]
    network: String,
    #[arg(long, default_value = "https://rpc.testnet.fastnear.com")]
    rpc_url: String,
    /// Attached for storage. Whatever is not used comes back.
    #[arg(long, default_value = "0.01 NEAR")]
    anchor_deposit: String,
    #[arg(long, default_value = "30 Tgas")]
    anchor_gas: String,
}

fn run_deployed(deployment: Deployment) -> Result<(), ClientError> {
    let transport = OutLayerTransport::new(OutLayerConfig {
        cli: deployment.outlayer_cli,
        project: deployment.project,
        secrets_account: deployment.secrets_account,
        secrets_profile: deployment.secrets_profile,
    });
    let anchors = NearAnchors::new(NearAnchorsConfig {
        near_cli: deployment.near_cli,
        contract: deployment.anchor_contract,
        signer: deployment.anchor_signer,
        network: deployment.network,
        rpc_url: deployment.rpc_url,
        deposit: deployment.anchor_deposit,
        prepaid_gas: deployment.anchor_gas,
    });
    demo::run(transport, anchors)
}

fn main() -> ExitCode {
    let Commands::Demo { target } = Cli::parse().command;
    let result = match target {
        Target::Local => LocalTransport::new()
            .and_then(|transport| demo::run(transport, LocalAnchors::default())),
        Target::Deployed(deployment) => run_deployed(*deployment),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
