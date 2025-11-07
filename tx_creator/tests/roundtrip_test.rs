use anyhow::{Context, Result};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use std::fs;
use std::path::PathBuf;

// Import from the main crate
use tx_creator::image_decoder;
use tx_creator::image_encoder;

/// Integration test that encodes punk.png, broadcasts transactions,
/// then decodes and verifies the result matches the original
#[test]
#[ignore] // Run with: cargo test --test roundtrip_test -- --ignored
fn test_encode_decode_roundtrip() -> Result<()> {
    // Setup
    // When tests run, the working directory is tx_creator/
    let input_file = PathBuf::from("test_data/punk.png");
    let output_file = PathBuf::from("/tmp/punk_decoded.png");

    // Ensure input file exists
    if !input_file.exists() {
        eprintln!(
            "Skipping test: punk.png not found at {:?}",
            input_file.canonicalize()
        );
        return Ok(());
    }

    // Read original file
    let original_data = fs::read(&input_file).context("Failed to read punk.png")?;

    println!("Original file size: {} bytes", original_data.len());

    // Connect to Bitcoin Core RPC
    let rpc = Client::new(
        "http://localhost:18443",
        Auth::UserPass("mempool".to_string(), "mempool".to_string()),
    )
    .context("Failed to connect to Bitcoin Core RPC")?;

    // Verify we're on regtest
    let blockchain_info = rpc
        .get_blockchain_info()
        .context("Failed to get blockchain info")?;

    assert_eq!(
        blockchain_info.chain.to_string(),
        "regtest",
        "Must be running on regtest network"
    );

    println!("Connected to Bitcoin Core on regtest");

    // Step 1: Encode the file into chunks
    let chunks = image_encoder::chunk_file(&input_file).context("Failed to chunk file")?;

    println!("Created {} chunks", chunks.len());

    // Step 2: Create and broadcast chained transactions
    let mut txids = Vec::new();
    let mut prev_txid = None;
    let mut prev_vout = None;
    let mut prev_continuation_amount = 10_000u64; // CONTINUATION_AMOUNT

    for (i, chunk) in chunks.iter().enumerate() {
        let chunk_data = chunk.encode();
        let is_last = i == chunks.len() - 1;
        let op_return_amount = 0u64;

        let tx = if i == 0 {
            // First transaction: spend from wallet UTXO
            create_chained_tx_first(&rpc, &chunk_data, op_return_amount, !is_last)?
        } else {
            // Subsequent transactions: spend continuation output from previous tx
            create_chained_tx_next(
                &rpc,
                &chunk_data,
                op_return_amount,
                prev_txid.unwrap(),
                prev_vout.unwrap(),
                prev_continuation_amount,
                !is_last,
            )?
        };

        // Broadcast transaction
        let txid = rpc
            .send_raw_transaction(&tx)
            .context(format!("Failed to broadcast transaction for chunk {}", i))?;

        println!("  Chunk {}/{}: {}", i + 1, chunks.len(), txid);
        txids.push(txid);

        // Update for next iteration
        prev_txid = Some(txid);
        prev_vout = Some(1);

        if !is_last {
            prev_continuation_amount =
                prev_continuation_amount.saturating_sub(1000 + op_return_amount);
        }
    }

    let first_txid = txids[0];
    println!("\nAll {} transactions broadcast successfully!", txids.len());
    println!("First TXID: {}", first_txid);

    // Step 3: Decode from blockchain using first TXID
    println!("\nDecoding from blockchain...");
    image_decoder::decode_from_blockchain(&rpc, &first_txid.to_string(), &output_file)
        .context("Failed to decode from blockchain")?;

    // Step 4: Verify decoded file matches original
    let decoded_data = fs::read(&output_file).context("Failed to read decoded file")?;

    println!("Decoded file size: {} bytes", decoded_data.len());

    assert_eq!(
        original_data.len(),
        decoded_data.len(),
        "File sizes don't match"
    );

    assert_eq!(original_data, decoded_data, "File contents don't match");

    println!("\n✓ Round-trip test PASSED - files match!");

    // Cleanup
    fs::remove_file(&output_file).ok();

    Ok(())
}

// Helper functions copied from main.rs
// (We need these because they're not public in the library)

fn create_chained_tx_first(
    rpc: &Client,
    data: &[u8],
    op_return_amount: u64,
    has_continuation: bool,
) -> Result<bitcoin::Transaction> {
    use bitcoin::script::PushBytesBuf;
    use bitcoin::{absolute, transaction, Amount, Network, ScriptBuf, Transaction, TxIn, TxOut};

    let change_address = rpc
        .get_new_address(None, None)
        .context("Failed to get new address")?
        .require_network(Network::Regtest)
        .context("Address is not on regtest network")?;

    let mut unspent = rpc
        .list_unspent(None, None, None, None, None)
        .context("Failed to list unspent outputs")?;

    if unspent.is_empty() {
        anyhow::bail!("No unspent outputs available. Generate some blocks first.");
    }

    // Sort by amount descending to use the largest UTXO
    unspent.sort_by(|a, b| b.amount.cmp(&a.amount));
    let utxo = &unspent[0];

    let input = TxIn {
        previous_output: bitcoin::OutPoint {
            txid: utxo.txid,
            vout: utxo.vout,
        },
        script_sig: ScriptBuf::new(),
        sequence: bitcoin::Sequence::MAX,
        witness: bitcoin::Witness::new(),
    };

    let push_bytes =
        PushBytesBuf::try_from(data.to_vec()).context("Failed to create push bytes from data")?;
    let op_return_script = ScriptBuf::new_op_return(&push_bytes);

    let mut outputs = vec![TxOut {
        value: Amount::from_sat(op_return_amount),
        script_pubkey: op_return_script,
    }];

    let input_amount = utxo.amount.to_sat();
    let fee = 1000;
    let mut total_out = op_return_amount + fee;

    if has_continuation {
        outputs.push(TxOut {
            value: Amount::from_sat(10_000), // CONTINUATION_AMOUNT
            script_pubkey: change_address.script_pubkey(),
        });
        total_out += 10_000;
    }

    let change_amount = input_amount
        .checked_sub(total_out)
        .context("Insufficient funds for transaction")?;

    outputs.push(TxOut {
        value: Amount::from_sat(change_amount),
        script_pubkey: change_address.script_pubkey(),
    });

    let mut tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![input],
        output: outputs,
    };

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

fn create_chained_tx_next(
    rpc: &Client,
    data: &[u8],
    op_return_amount: u64,
    prev_txid: bitcoin::Txid,
    prev_vout: u32,
    input_amount: u64,
    has_continuation: bool,
) -> Result<bitcoin::Transaction> {
    use bitcoin::script::PushBytesBuf;
    use bitcoin::{absolute, transaction, Amount, Network, ScriptBuf, Transaction, TxIn, TxOut};

    let address = rpc
        .get_new_address(None, None)
        .context("Failed to get new address")?
        .require_network(Network::Regtest)
        .context("Address is not on regtest network")?;

    let input = TxIn {
        previous_output: bitcoin::OutPoint {
            txid: prev_txid,
            vout: prev_vout,
        },
        script_sig: ScriptBuf::new(),
        sequence: bitcoin::Sequence::MAX,
        witness: bitcoin::Witness::new(),
    };

    let push_bytes =
        PushBytesBuf::try_from(data.to_vec()).context("Failed to create push bytes from data")?;
    let op_return_script = ScriptBuf::new_op_return(&push_bytes);

    let mut outputs = vec![TxOut {
        value: Amount::from_sat(op_return_amount),
        script_pubkey: op_return_script,
    }];

    let fee = 1000;

    let continuation_out_amount = if has_continuation {
        input_amount
            .checked_sub(fee + op_return_amount)
            .context("Insufficient funds for continuation and fee")?
    } else {
        0
    };

    if has_continuation {
        outputs.push(TxOut {
            value: Amount::from_sat(continuation_out_amount),
            script_pubkey: address.script_pubkey(),
        });
    }

    let mut tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![input],
        output: outputs,
    };

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
