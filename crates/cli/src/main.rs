mod demo;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use undiluted_client::{
    ClientError, LocalAnchors, LocalRail, LocalTransport, NearAnchors, NearAnchorsConfig, NearCli,
    OneClickConfig, OneClickRail, OutLayerConfig, OutLayerTransport,
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
    /// The account that relays anchors and makes the demo's payments.
    #[arg(long)]
    signer: String,
    /// Base URL of a 1Click-compatible API.
    #[arg(long)]
    payment_api: String,
    /// Name of an environment variable holding that API's token. Only set this
    /// for an API you trust with it.
    #[arg(long)]
    payment_token_env: Option<String>,
    /// The token investors pay with, as nep141:<contract>, with six decimals.
    #[arg(long)]
    origin_asset: String,
    /// The token the treasury receives, as nep141:<contract>, with six decimals.
    #[arg(long)]
    destination_asset: String,
    /// The account receiving investments.
    #[arg(long)]
    treasury: String,
    #[arg(long, default_value_t = 100)]
    slippage_basis_points: u32,
    #[arg(long, default_value_t = 3600)]
    quote_validity_seconds: u64,
    #[arg(long, default_value_t = 6)]
    poll_seconds: u64,
    #[arg(long, default_value_t = 40)]
    max_polls: u32,
    #[arg(long, default_value = "30 Tgas")]
    payment_gas: String,
    /// Registers the deposit address with the token contract.
    #[arg(long, default_value = "0.00125 NEAR")]
    token_storage_deposit: String,
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
    let near = NearCli {
        path: deployment.near_cli,
        network: deployment.network,
        signer: deployment.signer,
    };
    let transport = OutLayerTransport::new(OutLayerConfig {
        cli: deployment.outlayer_cli,
        project: deployment.project,
        secrets_account: deployment.secrets_account,
        secrets_profile: deployment.secrets_profile,
    });
    let anchors = NearAnchors::new(NearAnchorsConfig {
        near: near.clone(),
        contract: deployment.anchor_contract,
        rpc_url: deployment.rpc_url,
        deposit: deployment.anchor_deposit,
        prepaid_gas: deployment.anchor_gas,
    });
    let api_token = deployment
        .payment_token_env
        .map(|name| {
            std::env::var(&name).map_err(|_| ClientError::Payment(format!("{name} is not set")))
        })
        .transpose()?;
    let rail = OneClickRail::new(
        OneClickConfig {
            api_url: deployment.payment_api,
            api_token,
            origin_asset: deployment.origin_asset,
            destination_asset: deployment.destination_asset,
            treasury: deployment.treasury,
            slippage_basis_points: deployment.slippage_basis_points,
            quote_validity: Duration::from_secs(deployment.quote_validity_seconds),
            poll_interval: Duration::from_secs(deployment.poll_seconds),
            max_polls: deployment.max_polls,
            gas: deployment.payment_gas,
            storage_deposit: deployment.token_storage_deposit,
        },
        near,
    );
    demo::run(transport, anchors, rail)
}

fn main() -> ExitCode {
    let Commands::Demo { target } = Cli::parse().command;
    let result = match target {
        Target::Local => LocalTransport::new().and_then(|transport| {
            demo::run(transport, LocalAnchors::default(), LocalRail::default())
        }),
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
