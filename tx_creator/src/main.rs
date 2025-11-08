use anyhow::{Context, Result};
use bitcoincore_rpc::{Auth, Client};
use clap::Parser;
use std::path::PathBuf;

use tx_creator::techniques::TechniqueType;
use tx_creator::utils::rpc;

#[derive(Parser, Debug)]
#[command(name = "tx_creator")]
#[command(about = "Create Bitcoin transactions with embedded data on regtest")]
struct Args {
    /// File to encode
    #[arg(short, long, conflicts_with = "decode")]
    file: Option<PathBuf>,

    /// Decode from blockchain starting from this TXID
    #[arg(short, long, conflicts_with = "file")]
    decode: Option<String>,

    /// Output file path for decoded data (required with --decode)
    #[arg(short, long, requires = "decode")]
    output: Option<PathBuf>,

    /// Embedding technique to use
    #[arg(short = 't', long, default_value = "chained-op-return")]
    technique: String,

    /// Bitcoin RPC URL
    #[arg(long, default_value = "http://localhost:18443")]
    rpc_url: String,

    /// RPC username
    #[arg(long, default_value = "mempool")]
    rpc_user: String,

    /// RPC password
    #[arg(long, default_value = "mempool")]
    rpc_password: String,

    /// Broadcast transactions to the network
    #[arg(short, long)]
    broadcast: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Parse technique
    let technique = TechniqueType::from_str(&args.technique)?;

    // Connect to Bitcoin Core RPC
    let client = Client::new(
        &args.rpc_url,
        Auth::UserPass(args.rpc_user.clone(), args.rpc_password.clone()),
    )
    .context("Failed to connect to Bitcoin Core RPC")?;

    // Validate connection and network
    rpc::validate_connection(&client, "regtest")?;
    println!("✓ Connected to Bitcoin Core on regtest network");

    // Route to encode or decode
    if let Some(txid_str) = args.decode {
        // Decode mode
        let output_path = args
            .output
            .context("--output is required when using --decode")?;

        let txid = txid_str
            .parse()
            .context("Invalid TXID format")?;

        technique.decode(&client, &txid, &output_path)?;
    } else if let Some(file_path) = args.file {
        // Encode mode
        let txid = technique.encode(&client, &file_path, args.broadcast)?;

        if !args.broadcast {
            println!("\nFirst TXID for decoding: {}", txid);
        }
    } else {
        anyhow::bail!("Must provide either --file or --decode");
    }

    Ok(())
}
