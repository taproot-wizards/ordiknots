use anyhow::{Context, Result};
use bitcoin::consensus::encode;
use bitcoin::script::PushBytesBuf;
use bitcoin::{
    absolute, transaction, Amount, Network, ScriptBuf, Transaction, TxIn, TxOut,
};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "tx_creator")]
#[command(about = "Create Bitcoin transactions with emoji OP_RETURN data on regtest")]
struct Args {
    /// The emoji or text to include in the OP_RETURN
    #[arg(short, long)]
    message: String,

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
    println!("Connected to Bitcoin Core on network: {}", blockchain_info.chain);

    // Create OP_RETURN transaction
    let tx = create_op_return_transaction(&rpc, &args.message, args.amount)?;
    let tx_hex = encode::serialize_hex(&tx);

    println!("\nTransaction created successfully!");
    println!("Transaction ID: {}", tx.compute_txid());
    println!("Transaction hex: {}", tx_hex);
    println!("\nOP_RETURN message: {}", args.message);

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

    Ok(())
}

fn create_op_return_transaction(
    rpc: &Client,
    message: &str,
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
    let push_bytes = PushBytesBuf::try_from(message.as_bytes().to_vec())
        .context("Failed to create push bytes from message")?;
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

    tx = signed_tx.transaction().context("Failed to get signed transaction")?;

    Ok(tx)
}
