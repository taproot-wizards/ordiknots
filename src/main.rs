use anyhow::{Context, Result};
use bitcoin::Network;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use clap::{Parser, Subcommand};
use redb::Database;
use std::fs;
use std::path::PathBuf;

use ordiknots::indexer;
use ordiknots::server;
use ordiknots::techniques::{self, Technique};
use ordiknots::utils::broadcast;

fn parse_network(s: &str) -> Result<Network, String> {
    match s {
        "bitcoin" => Ok(Network::Bitcoin),
        "regtest" => Ok(Network::Regtest),
        _ => Err(format!(
            "Invalid network: '{}'. Must be 'bitcoin' or 'regtest'",
            s
        )),
    }
}

#[derive(Parser, Debug)]
#[command(name = "ordiknots")]
#[command(about = "magic transactions with arbitrary data, accepted by Bitcoin Knots 🧙‍♂️")]
struct Args {
    #[arg(long, default_value = "regtest", value_parser = parse_network)]
    network: Network,

    #[arg(
        long,
        default_value = "http://localhost:18443",
        default_value_if("network", "bitcoin", "http://localhost:8332")
    )]
    rpc_url: String,

    #[arg(
        long,
        default_value = "mempool",
        default_value_if("network", "bitcoin", None)
    )]
    rpc_user: Option<String>,

    #[arg(
        long,
        default_value = "mempool",
        default_value_if("network", "bitcoin", None)
    )]
    rpc_password: Option<String>,

    #[arg(long)]
    cookie: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Encode {
        file: PathBuf,

        #[arg(short = 't', long = "type", default_value = "p2wsh-fake-multisig")]
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
        #[arg(short, long)]
        database: Option<PathBuf>,

        #[arg(short, long, default_value_t = 4000)]
        port: u16,
    },
}

#[derive(Subcommand, Debug)]
enum IndexAction {
    Start {
        #[arg(short, long)]
        database: Option<PathBuf>,
    },
    Stats {
        #[arg(short, long)]
        database: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let client = get_rpc_client(args.cookie, args.rpc_url, args.rpc_user, args.rpc_password)?;

    match args.command {
        Command::Encode {
            file,
            technique: technique_str,
            broadcast,
        } => {
            let technique: Technique = technique_str.parse().context("Invalid technique")?;

            let data =
                fs::read(&file).context(format!("Failed to read file: {}", file.display()))?;

            let (transactions, decode_txid) = technique.encode(&data, &client)?;

            if broadcast {
                println!("\nBroadcasting {} transaction(s)...", transactions.len());
                for (i, tx) in transactions.iter().enumerate() {
                    let label = format!("Transaction {}/{}", i + 1, transactions.len());
                    broadcast::broadcast_or_dryrun(&client, tx, true, Some(&label))?;
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
            let data = technique.decode(&txid, &client)?;

            fs::write(&output, &data)
                .context(format!("Failed to write to {}", output.display()))?;

            println!("Wrote {} bytes to {}", data.len(), output.display());
        }
        Command::Index { action } => match action {
            IndexAction::Start { database } => {
                let db = get_db(database, &args.network)?;
                indexer::sync(&db, &client)?;
            }
            IndexAction::Stats { database } => {
                let db = get_db(database, &args.network)?;
                indexer::get_stats(&db)?;
            }
        },
        Command::Server { database, port } => {
            let db = get_db(database, &args.network)?;

            // Wrap in Arc for sharing between indexer and server
            let db = std::sync::Arc::new(db);
            let client = std::sync::Arc::new(client);

            // Spawn indexer in background
            {
                let db = db.clone();
                let client = client.clone();
                tokio::task::spawn_blocking(move || indexer::sync(&db, &client));
            }

            // Start server (blocks forever)
            server::start_server(db, port, &args.network, client).await?;
        }
    }

    Ok(())
}

fn get_db(database: Option<PathBuf>, network: &Network) -> Result<Database> {
    let default_db_path =
        || -> PathBuf { PathBuf::from(format!("data/{}/ordiknots.db", &network)) };

    let db_path = database.unwrap_or_else(default_db_path);

    indexer::open_database(&db_path)
        .context(format!("Failed to open database: {}", db_path.display()))
}

fn get_rpc_client(
    cookie_path: Option<PathBuf>,
    rpc_url: String,
    rpc_user: Option<String>,
    rpc_password: Option<String>,
) -> Result<Client> {
    let auth = if let Some(cookie_path) = cookie_path {
        Auth::CookieFile(cookie_path.clone())
    } else {
        match (rpc_user, rpc_password) {
            (Some(user), Some(password)) => Auth::UserPass(user, password),
            (None, None) => Auth::None,
            _ => {
                return Err(anyhow::anyhow!(
                    "Both rpc_user and rpc_password must be provided, or neither"
                ))
            }
        }
    };

    Client::new(&rpc_url, auth).context("Failed to connect to Bitcoin RPC")
}
