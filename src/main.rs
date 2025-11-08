use anyhow::{Context, Result};
use bitcoincore_rpc::{Auth, Client};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use ordiknots::techniques::TechniqueType;
use ordiknots::utils::rpc;

#[derive(Parser, Debug)]
#[command(name = "ordiknots")]
#[command(about = "magic transactions with arbitrary data, accepted by Bitcoin Knots 🧙‍♂️")]
struct Args {
    #[arg(short = 't', long = "type", default_value = "chained-op-return")]
    technique: String,

    #[arg(long, default_value = "http://localhost:18443")]
    rpc_url: String,

    #[arg(long, default_value = "mempool")]
    rpc_user: String,

    #[arg(long, default_value = "mempool")]
    rpc_password: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Encode {
        file: PathBuf,

        #[arg(short, long)]
        broadcast: bool,
    },
    Decode {
        txid: String,

        #[arg(short, long)]
        output: PathBuf,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    let technique = TechniqueType::from_str(&args.technique)?;

    let client = Client::new(
        &args.rpc_url,
        Auth::UserPass(args.rpc_user.clone(), args.rpc_password.clone()),
    )
    .context("Failed to connect to Bitcoin RPC")?;

    rpc::validate_connection(&client, "regtest")?;
    println!("✓ Connected to Bitcoin on regtest network");

    match args.command {
        Command::Decode {
            txid: txid_str,
            output,
        } => {
            let txid = txid_str.parse().context("Invalid TXID format")?;

            technique.decode(&client, &txid, &output)?;
        }
        Command::Encode { file, broadcast } => {
            let txid = technique.encode(&client, &file, broadcast)?;

            if !broadcast {
                println!("\nFirst TXID for decoding: {}", txid);
            }
        }
    }

    Ok(())
}
