use anyhow::{Context, Result};
use bitcoin::consensus::encode;
use bitcoin::script::PushBytesBuf;
use bitcoin::{absolute, transaction, Amount, Network, ScriptBuf, Transaction, TxIn, TxOut};
use bitcoincore_rpc::{Client, RpcApi};
use std::path::PathBuf;

use crate::image_encoder;

/// Standard transaction fee in satoshis
pub const TX_FEE_SATS: u64 = 1000;

/// Amount for continuation outputs (in satoshis) - initial amount from first tx
pub const CONTINUATION_AMOUNT: u64 = 10_000;

/// Position of continuation output in transaction outputs
pub const CONTINUATION_OUTPUT_INDEX: u32 = 1;

// ============================================================================
// Mode Handlers
// ============================================================================

/// Handles file mode: encodes a file across chained transactions
/// Returns the first TXID if broadcast is true, otherwise returns the computed TXID
pub fn handle_file_mode(
    rpc: &Client,
    file_path: &PathBuf,
    amount: u64,
    broadcast: bool,
) -> Result<String> {
    let chunks = image_encoder::chunk_file(file_path)?;
    println!("\nCreating {} chained transactions...\n", chunks.len());

    let mut txids = Vec::new();
    let mut continuation_amount = CONTINUATION_AMOUNT;

    for (i, chunk) in chunks.iter().enumerate() {
        let chunk_data = chunk.encode();
        let has_continuation = i < chunks.len() - 1;

        let tx = if i == 0 {
            // First transaction: spend from wallet UTXO
            create_chained_tx_first(rpc, &chunk_data, amount, has_continuation)?
        } else {
            // Subsequent transactions: spend continuation output from previous tx
            let prev_txid = txids[i - 1];
            create_chained_tx_next(
                rpc,
                &chunk_data,
                amount,
                prev_txid,
                CONTINUATION_OUTPUT_INDEX,
                continuation_amount,
                has_continuation,
            )?
        };

        println!(
            "Chunk {}/{} ({} bytes):",
            chunk.index + 1,
            chunk.total_chunks,
            chunk_data.len()
        );

        let txid = if broadcast {
            let txid = rpc
                .send_raw_transaction(&tx)
                .context(format!("Failed to broadcast transaction for chunk {}", i))?;
            println!("  TXID: {}", txid);
            txid
        } else {
            let txid = tx.compute_txid();
            println!("  TXID: {}", txid);
            println!("  Hex: {}", encode::serialize_hex(&tx));
            txid
        };

        txids.push(txid);

        // Update continuation amount for next transaction
        if has_continuation {
            continuation_amount = continuation_amount
                .checked_sub(TX_FEE_SATS + amount)
                .context("Continuation amount depleted")?;
        }
    }

    if broadcast {
        println!("\nAll {} transactions broadcast successfully!", txids.len());
        println!("\nTo decode this image, use the first TXID:");
        println!("  {}", txids[0]);
    }

    Ok(txids[0].to_string())
}

/// Handles message mode: creates a single OP_RETURN transaction
pub fn handle_message_mode(
    rpc: &Client,
    message: &str,
    amount: u64,
    broadcast: bool,
) -> Result<()> {
    let tx = create_op_return_transaction_raw(rpc, message.as_bytes(), amount)?;
    let tx_hex = encode::serialize_hex(&tx);

    println!("\nTransaction created successfully!");
    println!("Transaction ID: {}", tx.compute_txid());
    println!("Transaction hex: {}", tx_hex);
    println!("\nOP_RETURN message: {}", message);

    if broadcast {
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

// ============================================================================
// Helper Functions
// ============================================================================

/// Creates an OP_RETURN script from the provided data
fn create_op_return_script(data: &[u8]) -> Result<ScriptBuf> {
    let push_bytes =
        PushBytesBuf::try_from(data.to_vec()).context("Failed to create push bytes from data")?;
    Ok(ScriptBuf::new_op_return(&push_bytes))
}

/// Creates a transaction input spending from the specified output
fn create_tx_input(txid: bitcoin::Txid, vout: u32) -> TxIn {
    TxIn {
        previous_output: bitcoin::OutPoint { txid, vout },
        script_sig: ScriptBuf::new(),
        sequence: bitcoin::Sequence::MAX,
        witness: bitcoin::Witness::new(),
    }
}

/// Signs a transaction using the wallet and validates it's complete
fn sign_transaction(rpc: &Client, tx: Transaction) -> Result<Transaction> {
    let signed_tx = rpc
        .sign_raw_transaction_with_wallet(&tx, None, None)
        .context("Failed to sign transaction")?;

    if !signed_tx.complete {
        anyhow::bail!("Transaction signing incomplete");
    }

    signed_tx
        .transaction()
        .context("Failed to get signed transaction")
}

/// Gets a new address and validates it's on regtest network
fn get_regtest_address(rpc: &Client) -> Result<bitcoin::Address> {
    rpc.get_new_address(None, None)
        .context("Failed to get new address")?
        .require_network(Network::Regtest)
        .context("Address is not on regtest network")
}

/// Selects the largest UTXO from the wallet
fn select_largest_utxo(
    rpc: &Client,
    verbose: bool,
) -> Result<bitcoincore_rpc::json::ListUnspentResultEntry> {
    let mut unspent = rpc
        .list_unspent(None, None, None, None, None)
        .context("Failed to list unspent outputs")?;

    if unspent.is_empty() {
        anyhow::bail!("No unspent outputs available. Generate some blocks first.");
    }

    unspent.sort_by(|a, b| b.amount.cmp(&a.amount));
    let utxo = unspent.into_iter().next().unwrap();

    if verbose {
        println!("\nUsing UTXO:");
        println!("  TXID: {}", utxo.txid);
        println!("  Vout: {}", utxo.vout);
        println!("  Amount: {} BTC", utxo.amount);
    }

    Ok(utxo)
}

// ============================================================================
// Transaction Creation Functions
// ============================================================================

/// Creates the first transaction in a chain, spending from a wallet UTXO
///
/// This transaction includes:
/// - An OP_RETURN output with the chunk data
/// - A continuation output (if has_continuation is true) for the next transaction
/// - A change output returning funds to the wallet
///
/// # Arguments
/// * `rpc` - Bitcoin Core RPC client
/// * `data` - Encoded chunk data for OP_RETURN
/// * `op_return_amount` - Value to assign to OP_RETURN output (usually 0)
/// * `has_continuation` - Whether to create a continuation output for the next transaction
fn create_chained_tx_first(
    rpc: &Client,
    data: &[u8],
    op_return_amount: u64,
    has_continuation: bool,
) -> Result<Transaction> {
    let change_address = get_regtest_address(rpc)?;
    let utxo = select_largest_utxo(rpc, false)?;

    let input = create_tx_input(utxo.txid, utxo.vout);

    // Create OP_RETURN output
    let op_return_script = create_op_return_script(data)?;
    let mut outputs = vec![TxOut {
        value: Amount::from_sat(op_return_amount),
        script_pubkey: op_return_script,
    }];

    // Calculate amounts
    let input_amount = utxo.amount.to_sat();
    let mut total_out = op_return_amount + TX_FEE_SATS;

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
    let tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![input],
        output: outputs,
    };

    sign_transaction(rpc, tx)
}

/// Creates a subsequent transaction in a chain by spending a continuation output
///
/// # Arguments
/// * `rpc` - Bitcoin Core RPC client
/// * `data` - Encoded chunk data for OP_RETURN
/// * `op_return_amount` - Value to assign to OP_RETURN output (usually 0)
/// * `prev_txid` - Transaction ID of previous transaction to spend from
/// * `prev_vout` - Output index of continuation output
/// * `continuation_amount` - Amount available in the continuation output being spent
/// * `has_continuation` - Whether to create continuation output for next transaction
fn create_chained_tx_next(
    rpc: &Client,
    data: &[u8],
    op_return_amount: u64,
    prev_txid: bitcoin::Txid,
    prev_vout: u32,
    continuation_amount: u64,
    has_continuation: bool,
) -> Result<Transaction> {
    let address = get_regtest_address(rpc)?;
    let input = create_tx_input(prev_txid, prev_vout);

    // Create OP_RETURN output
    let op_return_script = create_op_return_script(data)?;
    let mut outputs = vec![TxOut {
        value: Amount::from_sat(op_return_amount),
        script_pubkey: op_return_script,
    }];

    // Add continuation output if needed
    if has_continuation {
        let next_continuation_amount = continuation_amount
            .checked_sub(TX_FEE_SATS + op_return_amount)
            .context("Insufficient funds for continuation and fee")?;

        outputs.push(TxOut {
            value: Amount::from_sat(next_continuation_amount),
            script_pubkey: address.script_pubkey(),
        });
    }

    // Build the transaction
    let tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![input],
        output: outputs,
    };

    sign_transaction(rpc, tx)
}

/// Creates a single OP_RETURN transaction with a message
fn create_op_return_transaction_raw(
    rpc: &Client,
    data: &[u8],
    op_return_amount: u64,
) -> Result<Transaction> {
    let change_address = get_regtest_address(rpc)?;
    let utxo = select_largest_utxo(rpc, true)?;

    let input = create_tx_input(utxo.txid, utxo.vout);

    // Create OP_RETURN output
    let op_return_script = create_op_return_script(data)?;
    let op_return_output = TxOut {
        value: Amount::from_sat(op_return_amount),
        script_pubkey: op_return_script,
    };

    // Calculate change
    let input_amount = utxo.amount.to_sat();
    let change_amount = input_amount
        .checked_sub(op_return_amount + TX_FEE_SATS)
        .context("Insufficient funds for transaction")?;

    // Create change output
    let change_output = TxOut {
        value: Amount::from_sat(change_amount),
        script_pubkey: change_address.script_pubkey(),
    };

    // Build the transaction
    let tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![input],
        output: vec![op_return_output, change_output],
    };

    sign_transaction(rpc, tx)
}
