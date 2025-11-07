use anyhow::{Context, Result};
use bitcoin::consensus::encode;
use bitcoin::script::PushBytesBuf;
use bitcoin::{absolute, transaction, Amount, Network, ScriptBuf, Transaction, TxIn, TxOut};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use clap::Parser;
use std::path::PathBuf;

mod image_decoder;
mod image_encoder;

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
        // Decode mode
        let output_path = args
            .output
            .context("--output is required when using --decode")?;
        image_decoder::decode_from_blockchain(&rpc, &txid, &output_path)?;
    } else if let Some(file_path) = args.file {
        // Chunk the file and create chained transactions
        let chunks = image_encoder::chunk_file(&file_path)?;
        println!("\nCreating {} chained transactions...\n", chunks.len());

        let mut txids = Vec::new();
        let mut prev_txid = None;
        let mut prev_vout = None;
        let mut prev_continuation_amount = CONTINUATION_AMOUNT;

        for (i, chunk) in chunks.iter().enumerate() {
            let chunk_data = chunk.encode();
            let is_last = i == chunks.len() - 1;

            let tx = if i == 0 {
                // First transaction: spend from wallet UTXO
                create_chained_tx_first(&rpc, &chunk_data, args.amount, !is_last)?
            } else {
                // Subsequent transactions: spend continuation output from previous tx
                create_chained_tx_next(
                    &rpc,
                    &chunk_data,
                    args.amount,
                    prev_txid.unwrap(),
                    prev_vout.unwrap(),
                    prev_continuation_amount,
                    !is_last,
                )?
            };

            println!(
                "Chunk {}/{} ({} bytes):",
                chunk.index + 1,
                chunk.total_chunks,
                chunk_data.len()
            );

            if args.broadcast {
                let txid = rpc
                    .send_raw_transaction(&tx)
                    .context(format!("Failed to broadcast transaction for chunk {}", i))?;
                println!("  TXID: {}", txid);
                txids.push(txid);

                // Store for next iteration - continuation output is always vout 1 (if it exists)
                // Note: Each continuation gets smaller by the fee amount
                prev_txid = Some(txid);
                prev_vout = Some(1);

                // Update continuation amount for next transaction (decreases by fee each time)
                if !is_last {
                    prev_continuation_amount =
                        prev_continuation_amount.saturating_sub(1000 + args.amount);
                }
            } else {
                let txid = tx.compute_txid();
                println!("  TXID: {}", txid);
                println!("  Hex: {}", encode::serialize_hex(&tx));
                prev_txid = Some(txid);
                prev_vout = Some(1);
            }
        }

        if args.broadcast {
            println!("\nAll {} transactions broadcast successfully!", txids.len());
            println!("\nTo decode this image, use the first TXID:");
            println!("  {}", txids[0]);
        }
    } else if let Some(message) = args.message {
        // Create single OP_RETURN transaction with message
        let tx = create_op_return_transaction_raw(&rpc, message.as_bytes(), args.amount)?;
        let tx_hex = encode::serialize_hex(&tx);

        println!("\nTransaction created successfully!");
        println!("Transaction ID: {}", tx.compute_txid());
        println!("Transaction hex: {}", tx_hex);
        println!("\nOP_RETURN message: {}", message);

        if args.broadcast {
            println!("\nBroadcasting transaction...");
            let txid = rpc
                .send_raw_transaction(&tx)
                .context("Failed to broadcast transaction")?;
            println!("Transaction broadcast successfully!");
            println!("TXID: {}", txid);
        } else {
            println!("\nTo broadcast this transaction, run:");
            println!("just cli sendrawtransaction {}", tx_hex);
        }
    } else {
        anyhow::bail!("Must provide either --message, --file, or --decode");
    }

    Ok(())
}

/// Amount for continuation outputs (in satoshis) - initial amount from first tx
const CONTINUATION_AMOUNT: u64 = 10_000;

/// Create the first transaction in the chain (spends from wallet UTXO)
fn create_chained_tx_first(
    rpc: &Client,
    data: &[u8],
    op_return_amount: u64,
    has_continuation: bool,
) -> Result<Transaction> {
    // Get an address to send change to
    let change_address = rpc
        .get_new_address(None, None)
        .context("Failed to get new address")?
        .require_network(Network::Regtest)
        .context("Address is not on regtest network")?;

    // List unspent outputs
    let unspent = rpc
        .list_unspent(None, None, None, None, None)
        .context("Failed to list unspent outputs")?;

    if unspent.is_empty() {
        anyhow::bail!("No unspent outputs available. Generate some blocks first.");
    }

    // Use the first available UTXO
    let utxo = &unspent[0];

    // Create transaction inputs
    let input = TxIn {
        previous_output: bitcoin::OutPoint {
            txid: utxo.txid,
            vout: utxo.vout,
        },
        script_sig: ScriptBuf::new(),
        sequence: bitcoin::Sequence::MAX,
        witness: bitcoin::Witness::new(),
    };

    // Create OP_RETURN output
    let push_bytes =
        PushBytesBuf::try_from(data.to_vec()).context("Failed to create push bytes from data")?;
    let op_return_script = ScriptBuf::new_op_return(&push_bytes);

    let mut outputs = vec![TxOut {
        value: Amount::from_sat(op_return_amount),
        script_pubkey: op_return_script,
    }];

    // Calculate amounts
    let input_amount = utxo.amount.to_sat();
    let fee = 1000;
    let mut total_out = op_return_amount + fee;

    // Add continuation output if needed
    if has_continuation {
        outputs.push(TxOut {
            value: Amount::from_sat(CONTINUATION_AMOUNT),
            script_pubkey: change_address.script_pubkey(),
        });
        total_out += CONTINUATION_AMOUNT;
    }

    // Calculate and add change output
    let change_amount = input_amount
        .checked_sub(total_out)
        .context("Insufficient funds for transaction")?;

    outputs.push(TxOut {
        value: Amount::from_sat(change_amount),
        script_pubkey: change_address.script_pubkey(),
    });

    // Build the transaction
    let mut tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![input],
        output: outputs,
    };

    // Sign the transaction
    let signed_tx = rpc
        .sign_raw_transaction_with_wallet(&tx, None, None)
        .context("Failed to sign transaction")?;

    if !signed_tx.complete {
        anyhow::bail!("Transaction signing incomplete");
    }

    tx = signed_tx
        .transaction()
        .context("Failed to get signed transaction")?;

    Ok(tx)
}

/// Create a subsequent transaction in the chain (spends continuation output)
fn create_chained_tx_next(
    rpc: &Client,
    data: &[u8],
    op_return_amount: u64,
    prev_txid: bitcoin::Txid,
    prev_vout: u32,
    input_amount: u64,
    has_continuation: bool,
) -> Result<Transaction> {
    // Get an address for outputs
    let address = rpc
        .get_new_address(None, None)
        .context("Failed to get new address")?
        .require_network(Network::Regtest)
        .context("Address is not on regtest network")?;

    // Create transaction input (spends continuation output from previous tx)
    let input = TxIn {
        previous_output: bitcoin::OutPoint {
            txid: prev_txid,
            vout: prev_vout,
        },
        script_sig: ScriptBuf::new(),
        sequence: bitcoin::Sequence::MAX,
        witness: bitcoin::Witness::new(),
    };

    // Create OP_RETURN output
    let push_bytes =
        PushBytesBuf::try_from(data.to_vec()).context("Failed to create push bytes from data")?;
    let op_return_script = ScriptBuf::new_op_return(&push_bytes);

    let mut outputs = vec![TxOut {
        value: Amount::from_sat(op_return_amount),
        script_pubkey: op_return_script,
    }];

    // Calculate amounts (input is the continuation amount from previous tx)
    // input_amount is passed as a parameter
    let fee = 1000; // Fee for these transactions

    // From the input, we need to:
    // 1. Pay for OP_RETURN output (op_return_amount, usually 0)
    // 2. Create continuation output for next tx (if needed)
    // 3. Pay transaction fee
    // The continuation output will be smaller to account for the fee

    let continuation_out_amount = if has_continuation {
        // Continuation output is input minus fee (and OP_RETURN amount if any)
        input_amount
            .checked_sub(fee + op_return_amount)
            .context("Insufficient funds for continuation and fee")?
    } else {
        0
    };

    // Add continuation output if needed
    if has_continuation {
        outputs.push(TxOut {
            value: Amount::from_sat(continuation_out_amount),
            script_pubkey: address.script_pubkey(),
        });
    }

    // Build the transaction
    let mut tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![input],
        output: outputs,
    };

    // Sign the transaction
    let signed_tx = rpc
        .sign_raw_transaction_with_wallet(&tx, None, None)
        .context("Failed to sign transaction")?;

    if !signed_tx.complete {
        anyhow::bail!("Transaction signing incomplete");
    }

    tx = signed_tx
        .transaction()
        .context("Failed to get signed transaction")?;

    Ok(tx)
}

fn create_op_return_transaction_raw(
    rpc: &Client,
    data: &[u8],
    op_return_amount: u64,
) -> Result<Transaction> {
    // Get an address to send change to
    let change_address = rpc
        .get_new_address(None, None)
        .context("Failed to get new address")?
        .require_network(Network::Regtest)
        .context("Address is not on regtest network")?;

    // List unspent outputs
    let unspent = rpc
        .list_unspent(None, None, None, None, None)
        .context("Failed to list unspent outputs")?;

    if unspent.is_empty() {
        anyhow::bail!("No unspent outputs available. Generate some blocks first.");
    }

    // Use the first available UTXO
    let utxo = &unspent[0];
    println!("\nUsing UTXO:");
    println!("  TXID: {}", utxo.txid);
    println!("  Vout: {}", utxo.vout);
    println!("  Amount: {} BTC", utxo.amount);

    // Create transaction inputs
    let input = TxIn {
        previous_output: bitcoin::OutPoint {
            txid: utxo.txid,
            vout: utxo.vout,
        },
        script_sig: ScriptBuf::new(),
        sequence: bitcoin::Sequence::MAX,
        witness: bitcoin::Witness::new(),
    };

    // Create OP_RETURN output
    let push_bytes =
        PushBytesBuf::try_from(data.to_vec()).context("Failed to create push bytes from data")?;
    let op_return_script = ScriptBuf::new_op_return(&push_bytes);
    let op_return_output = TxOut {
        value: Amount::from_sat(op_return_amount),
        script_pubkey: op_return_script,
    };

    // Calculate change
    let input_amount = utxo.amount.to_sat();
    let fee = 1000; // 1000 satoshis fee
    let change_amount = input_amount
        .checked_sub(op_return_amount + fee)
        .context("Insufficient funds for transaction")?;

    // Create change output
    let change_output = TxOut {
        value: Amount::from_sat(change_amount),
        script_pubkey: change_address.script_pubkey(),
    };

    // Build the transaction
    let mut tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![input],
        output: vec![op_return_output, change_output],
    };

    // Sign the transaction
    let signed_tx = rpc
        .sign_raw_transaction_with_wallet(&tx, None, None)
        .context("Failed to sign transaction")?;

    if !signed_tx.complete {
        anyhow::bail!("Transaction signing incomplete");
    }

    tx = signed_tx
        .transaction()
        .context("Failed to get signed transaction")?;

    Ok(tx)
}
