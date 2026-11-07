mod demo;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use demo::{Destinations, Environment};
use undiluted_client::{
    ClientError, LOCAL_TOKEN, LOCAL_TREASURY, LocalAnchors, LocalPayouts, LocalRail,
    LocalTransport, NearAnchors, NearAnchorsConfig, NearCli, NearPayouts, NearPayoutsConfig,
    OneClickConfig, OneClickRail, OutLayerConfig, OutLayerTransport, RetryPolicy,
};
use undiluted_ledger::Treasury;

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
    /// The account running the distribution contract.
    #[arg(long)]
    distribution_contract: String,
    /// Where each holder gets paid at exit.
    #[arg(long)]
    founder_account: String,
    #[arg(long)]
    ana_account: String,
    #[arg(long)]
    ben_account: String,
    #[arg(long, default_value = "30 Tgas")]
    distribution_gas: String,
    #[arg(long, default_value = "60 Tgas")]
    payout_gas: String,
    #[arg(long, default_value = "100 Tgas")]
    funding_gas: String,
    /// Attached for contract storage. Whatever is not used comes back.
    #[arg(long, default_value = "0.01 NEAR")]
    distribution_deposit: String,
    /// How many times to relay a payout that has not landed.
    #[arg(long, default_value_t = 5)]
    relay_attempts: u32,
    /// How many times to look for the result of each relay.
    #[arg(long, default_value_t = 6)]
    settle_checks: u32,
    #[arg(long, default_value_t = 5)]
    settle_seconds: u64,
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
        rpc_url: deployment.rpc_url.clone(),
        deposit: deployment.anchor_deposit,
        prepaid_gas: deployment.anchor_gas,
    });
    let api_token = deployment
        .payment_token_env
        .map(|name| {
            std::env::var(&name).map_err(|_| ClientError::Payment(format!("{name} is not set")))
        })
        .transpose()?;
    let token = token_contract(&deployment.destination_asset)?;
    let treasury = Treasury::new(deployment.treasury.clone(), token.clone())?;
    let payouts = NearPayouts::new(NearPayoutsConfig {
        near: near.clone(),
        contract: deployment.distribution_contract,
        token,
        rpc_url: deployment.rpc_url,
        call_gas: deployment.distribution_gas,
        pay_gas: deployment.payout_gas,
        fund_gas: deployment.funding_gas,
        storage_deposit: deployment.distribution_deposit,
        token_storage_deposit: deployment.token_storage_deposit.clone(),
        retry: RetryPolicy {
            attempts: deployment.relay_attempts,
            checks: deployment.settle_checks,
            check_interval: Duration::from_secs(deployment.settle_seconds),
        },
    });
    let rail = OneClickRail::new(
        OneClickConfig {
            api_url: deployment.payment_api,
            api_token,
            origin_asset: deployment.origin_asset,
            destination_asset: deployment.destination_asset,
            treasury: deployment.treasury.clone(),
            slippage_basis_points: deployment.slippage_basis_points,
            quote_validity: Duration::from_secs(deployment.quote_validity_seconds),
            poll_interval: Duration::from_secs(deployment.poll_seconds),
            max_polls: deployment.max_polls,
            gas: deployment.payment_gas,
            storage_deposit: deployment.token_storage_deposit,
        },
        near,
    );
    demo::run(Environment {
        transport,
        anchors,
        rail,
        payouts,
        destinations: Destinations {
            founder: deployment.founder_account,
            ana: deployment.ana_account,
            ben: deployment.ben_account,
        },
        treasury,
    })
}

fn token_contract(asset: &str) -> Result<String, ClientError> {
    asset
        .strip_prefix("nep141:")
        .map(str::to_owned)
        .ok_or_else(|| ClientError::Payout(format!("not a NEAR token: {asset}")))
}

fn run_local() -> Result<(), ClientError> {
    demo::run(Environment {
        transport: LocalTransport::new()?,
        anchors: LocalAnchors::default(),
        rail: LocalRail::default(),
        payouts: LocalPayouts::default(),
        destinations: Destinations {
            founder: "founder.local".to_owned(),
            ana: "ana.local".to_owned(),
            ben: "ben.local".to_owned(),
        },
        treasury: Treasury::new(LOCAL_TREASURY, LOCAL_TOKEN)?,
    })
}

fn main() -> ExitCode {
    let Commands::Demo { target } = Cli::parse().command;
    let result = match target {
        Target::Local => run_local(),
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
