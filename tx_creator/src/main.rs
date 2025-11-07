use anyhow::{Context, Result};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use clap::Parser;
use std::path::PathBuf;

use tx_creator::image_decoder;
use tx_creator::tx_builder::{handle_file_mode, handle_message_mode};

#[derive(Parser, Debug)]
#[command(name = "tx_creator")]
#[command(about = "Create Bitcoin transactions with emoji OP_RETURN data on regtest")]
struct Args {
    /// The emoji or text to include in the OP_RETURN
    #[arg(short, long, conflicts_with_all = ["file", "decode"])]
    message: Option<String>,

    /// File to encode in OP_RETURN (will be chunked across multiple transactions)
    #[arg(short, long, conflicts_with_all = ["message", "decode"])]
    file: Option<PathBuf>,

    /// Decode an image from blockchain starting from this TXID
    #[arg(short, long, conflicts_with_all = ["message", "file"])]
    decode: Option<String>,

    /// Output file path for decoded image (required with --decode)
    #[arg(short, long, requires = "decode")]
    output: Option<PathBuf>,

    /// Amount to send to OP_RETURN output (in satoshis)
    #[arg(short, long, default_value = "0")]
    amount: u64,

    /// Bitcoin RPC URL
    #[arg(long, default_value = "http://localhost:18443")]
    rpc_url: String,

    /// RPC username
    #[arg(long, default_value = "mempool")]
    rpc_user: String,

    /// RPC password
    #[arg(long, default_value = "mempool")]
    rpc_password: String,

    /// Broadcast the transaction after creating it
    #[arg(short, long)]
    broadcast: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Connect to Bitcoin Core RPC
    let rpc = Client::new(
        &args.rpc_url,
        Auth::UserPass(args.rpc_user.clone(), args.rpc_password.clone()),
    )
    .context("Failed to connect to Bitcoin Core RPC")?;

    // Check connection and network
    let blockchain_info = rpc
        .get_blockchain_info()
        .context("Failed to get blockchain info")?;
    println!(
        "Connected to Bitcoin Core on network: {}",
        blockchain_info.chain
    );

    // Handle decode, file, or message
    if let Some(txid) = args.decode {
        let output_path = args
            .output
            .context("--output is required when using --decode")?;
        image_decoder::decode_from_blockchain(&rpc, &txid, &output_path)?;
    } else if let Some(file_path) = args.file {
        let _txid = handle_file_mode(&rpc, &file_path, args.amount, args.broadcast)?;
    } else if let Some(message) = args.message {
        handle_message_mode(&rpc, &message, args.amount, args.broadcast)?;
    } else {
        anyhow::bail!("Must provide either --message, --file, or --decode");
    }

    Ok(())
}
