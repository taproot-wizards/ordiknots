use anyhow::{Context, Result};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;

use ordiknots::indexer;
use ordiknots::server;
use ordiknots::techniques::{self, Technique};
use ordiknots::utils::{broadcast, rpc};

#[derive(Parser, Debug)]
#[command(name = "ordiknots")]
#[command(about = "magic transactions with arbitrary data, accepted by Bitcoin Knots 🧙‍♂️")]
struct Args {
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

        #[arg(short = 't', long = "type", default_value = "chained-op-return")]
        technique: String,

        #[arg(short, long)]
        broadcast: bool,
    },
    Decode {
        txid: String,

        #[arg(short, long)]
        output: PathBuf,
    },
    Index {
        #[command(subcommand)]
        action: IndexAction,
    },
    Server {
        #[arg(short, long, default_value = "ordiknots.db")]
        database: PathBuf,

        #[arg(short, long, default_value_t = 4000)]
        port: u16,
    },
}

#[derive(Subcommand, Debug)]
enum IndexAction {
    Start {
        #[arg(short, long, default_value = "ordiknots.db")]
        database: PathBuf,
    },
    Stats {
        #[arg(short, long, default_value = "ordiknots.db")]
        database: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Only create RPC client for commands that need it
    let needs_rpc = matches!(
        args.command,
        Command::Encode { .. } | Command::Decode { .. } | Command::Index { action: IndexAction::Start { .. } } | Command::Server { .. }
    );

    let client = if needs_rpc {
        let client = Client::new(
            &args.rpc_url,
            Auth::UserPass(args.rpc_user.clone(), args.rpc_password.clone()),
        )
        .context("Failed to connect to Bitcoin RPC")?;

        rpc::validate_connection(&client, "regtest")?;
        Some(client)
    } else {
        None
    };

    match args.command {
        Command::Encode {
            file,
            technique: technique_str,
            broadcast: should_broadcast,
        } => {
            let client = client.as_ref().unwrap();
            let technique: Technique = technique_str.parse().context("Invalid technique")?;

            let data =
                fs::read(&file).context(format!("Failed to read file: {}", file.display()))?;

            let (transactions, decode_txid) = technique.encode(&data, client)?;

            if should_broadcast {
                println!("\nBroadcasting {} transaction(s)...", transactions.len());
                for (i, tx) in transactions.iter().enumerate() {
                    let label = format!("Transaction {}/{}", i + 1, transactions.len());
                    broadcast::broadcast_or_dryrun(client, tx, true, Some(&label))?;
                }
                println!(
                    "\n✓ {} transaction(s) broadcast successfully!",
                    transactions.len()
                );
            }

            println!("\nTo decode this file, use the TXID:");
            println!("{}", decode_txid);
        }
        Command::Decode {
            txid: txid_str,
            output,
        } => {
            let client = client.as_ref().unwrap();
            let txid = txid_str.parse().context("Invalid TXID format")?;

            // Fetch the transaction to detect the technique
            println!("Fetching transaction {}...", txid);
            let tx = client
                .get_raw_transaction(&txid, None)
                .context("Failed to fetch transaction")?;

            // Auto-detect which technique was used
            let technique = techniques::detect_technique(&tx)
                .context("Could not detect encoding technique in transaction")?;

            println!("Detected technique: {}", technique);

            // Decode the data
            let data = technique.decode(&txid, client)?;

            fs::write(&output, &data)
                .context(format!("Failed to write to {}", output.display()))?;

            println!("Wrote {} bytes to {}", data.len(), output.display());
        }
        Command::Index { action } => match action {
            IndexAction::Start { database } => {
                let client = client.as_ref().unwrap();
                let db = indexer::open_database(&database)
                    .context(format!("Failed to open database: {}", database.display()))?;
                indexer::start_indexing(&db, client)?;
            }
            IndexAction::Stats { database } => {
                let db = indexer::open_database(&database)
                    .context(format!("Failed to open database: {}", database.display()))?;
                indexer::get_stats(&db)?;
            }
        },
        Command::Server { database, port } => {
            server::start_server(database, port, client.unwrap()).await?;
        }
    }

    Ok(())
}
