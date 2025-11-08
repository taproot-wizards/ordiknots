use anyhow::{Context, Result};
use bitcoin::{Amount, Transaction};
use bitcoincore_rpc::Client;

use crate::utils::{transaction as tx_utils, wallet};

/// Maximum bytes per OP_RETURN (Bitcoin Knots default policy limit)
/// This is the total scriptPubKey size, including OP_RETURN opcode and push opcode
pub const MAX_OP_RETURN_SIZE: usize = 40;

/// Metadata prefix to identify chunked data
const METADATA_SIZE: usize = 5; // "IMG" + index + total
const FIRST_CHUNK_METADATA_SIZE: usize = 7; // "IMG" + index + total + file_size

/// Maximum data bytes per chunk (after metadata)
const MAX_DATA_PER_CHUNK: usize = MAX_OP_RETURN_SIZE - METADATA_SIZE;
const FIRST_CHUNK_MAX_DATA: usize = MAX_OP_RETURN_SIZE - FIRST_CHUNK_METADATA_SIZE;

/// Minimum data bytes to ensure transaction meets minimum size (85 bytes)
const MIN_LAST_CHUNK_SIZE: usize = 30;

/// Amount for continuation outputs (in satoshis)
const CONTINUATION_AMOUNT: u64 = 10_000;

/// Position of continuation output in transaction outputs
const CONTINUATION_OUTPUT_INDEX: u32 = 1;

#[derive(Debug)]
struct Chunk {
    index: u8,
    total_chunks: u8,
    file_size: Option<u16>,
    data: Vec<u8>,
}

impl Chunk {
    /// Encodes the chunk into bytes ready for OP_RETURN
    fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(b"IMG");
        encoded.push(self.index);
        encoded.push(self.total_chunks);

        if let Some(size) = self.file_size {
            encoded.extend_from_slice(&size.to_le_bytes());
        }

        encoded.extend_from_slice(&self.data);
        encoded
    }
}

/// Encodes data using chained OP_RETURN transactions
pub fn encode(data: &[u8], client: &Client) -> Result<(Vec<Transaction>, bitcoin::Txid)> {
    let chunks = chunk_data(data)?;
    println!("\nCreating {} chained transactions...\n", chunks.len());

    let mut transactions: Vec<Transaction> = Vec::new();
    let mut continuation_amount = CONTINUATION_AMOUNT;

    for (i, chunk) in chunks.iter().enumerate() {
        let chunk_data = chunk.encode();
        let has_continuation = i < chunks.len() - 1;

        let tx = if i == 0 {
            create_first_tx(client, &chunk_data, has_continuation)?
        } else {
            let prev_tx = &transactions[i - 1];
            create_next_tx(
                client,
                &chunk_data,
                prev_tx,
                CONTINUATION_OUTPUT_INDEX,
                continuation_amount,
            )?
        };

        transactions.push(tx);

        if has_continuation {
            continuation_amount = continuation_amount
                .checked_sub(tx_utils::TX_FEE_SATS)
                .context("Continuation amount depleted")?;
        }
    }

    let first_txid = transactions[0].compute_txid();
    Ok((transactions, first_txid))
}

/// Splits data into chunks suitable for OP_RETURN outputs
fn chunk_data(data: &[u8]) -> Result<Vec<Chunk>> {
    let total_size = data.len();
    if total_size > 65535 {
        anyhow::bail!("File too large (max 65535 bytes)");
    }

    let first_chunk_size = FIRST_CHUNK_MAX_DATA.min(total_size);
    let remaining_size = total_size.saturating_sub(first_chunk_size);
    let total_chunks_usize = if remaining_size > 0 {
        1 + remaining_size.div_ceil(MAX_DATA_PER_CHUNK)
    } else {
        1
    };

    if total_chunks_usize == 0 {
        anyhow::bail!("File is empty");
    }

    if total_chunks_usize > 255 {
        anyhow::bail!(
            "File too large: needs {} chunks but maximum is 255 (max ~{} bytes)",
            total_chunks_usize,
            FIRST_CHUNK_MAX_DATA + (254 * MAX_DATA_PER_CHUNK)
        );
    }

    let total_chunks = total_chunks_usize as u8;

    println!("File size: {} bytes", total_size);
    println!(
        "Chunks needed: {} (first: {} bytes, others: {} bytes each)",
        total_chunks, FIRST_CHUNK_MAX_DATA, MAX_DATA_PER_CHUNK
    );

    let mut chunks = Vec::new();
    let mut offset = 0;

    // First chunk with file size
    let first_data_len = FIRST_CHUNK_MAX_DATA.min(total_size);
    chunks.push(Chunk {
        index: 0,
        total_chunks,
        file_size: Some(total_size as u16),
        data: data[..first_data_len].to_vec(),
    });
    offset += first_data_len;

    // Remaining chunks
    let mut chunk_index: u8 = 1;
    while offset < total_size {
        let chunk_size = MAX_DATA_PER_CHUNK.min(total_size - offset);
        let mut chunk_data = data[offset..offset + chunk_size].to_vec();

        // Pad last chunk if too small
        let is_last = offset + chunk_size >= total_size;
        if is_last && chunk_data.len() < MIN_LAST_CHUNK_SIZE {
            let padding_needed = MIN_LAST_CHUNK_SIZE - chunk_data.len();
            chunk_data.extend(vec![0u8; padding_needed]);
            println!("  Padding last chunk with {} null bytes", padding_needed);
        }

        chunks.push(Chunk {
            index: chunk_index,
            total_chunks,
            file_size: None,
            data: chunk_data,
        });

        offset += chunk_size;
        if offset < total_size {
            chunk_index = chunk_index.checked_add(1).expect("Too many chunks");
        }
    }

    Ok(chunks)
}

/// Creates the first transaction in the chain
fn create_first_tx(client: &Client, data: &[u8], has_continuation: bool) -> Result<Transaction> {
    let address = wallet::get_new_address(client)?;
    let utxo = wallet::select_largest_utxo(client)?;

    let input = tx_utils::create_tx_input(bitcoin::OutPoint {
        txid: utxo.txid,
        vout: utxo.vout,
    });

    let mut outputs = vec![tx_utils::create_op_return_output(data)];

    let input_amount = utxo.amount.to_sat();
    let mut total_out = tx_utils::TX_FEE_SATS;

    // Add continuation output if needed
    if has_continuation {
        outputs.push(bitcoin::TxOut {
            value: Amount::from_sat(CONTINUATION_AMOUNT),
            script_pubkey: address.script_pubkey(),
        });
        total_out += CONTINUATION_AMOUNT;
    }

    // Calculate and add change output
    let change_amount = input_amount
        .checked_sub(total_out)
        .context("Insufficient funds for transaction")?;

    outputs.push(bitcoin::TxOut {
        value: Amount::from_sat(change_amount),
        script_pubkey: address.script_pubkey(),
    });

    let tx = tx_utils::create_base_transaction(vec![input], outputs);
    wallet::sign_transaction(client, &tx)
}

/// Creates a subsequent transaction in the chain
fn create_next_tx(
    client: &Client,
    data: &[u8],
    prev_tx: &Transaction,
    prev_vout: u32,
    continuation_amount: u64,
) -> Result<Transaction> {
    let address = wallet::get_new_address(client)?;

    let input = tx_utils::create_tx_input(bitcoin::OutPoint {
        txid: prev_tx.compute_txid(),
        vout: prev_vout,
    });

    let mut outputs = vec![tx_utils::create_op_return_output(data)];

    // Calculate remaining amount after fee
    let next_amount = continuation_amount
        .checked_sub(tx_utils::TX_FEE_SATS)
        .context("Insufficient funds for continuation")?;

    outputs.push(bitcoin::TxOut {
        value: Amount::from_sat(next_amount),
        script_pubkey: address.script_pubkey(),
    });

    let tx = tx_utils::create_base_transaction(vec![input], outputs);
    wallet::sign_transaction_with_prevtxs(client, &tx, Some(&[prev_tx.clone()]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_encoding() {
        let chunk = Chunk {
            index: 0,
            total_chunks: 3,
            file_size: Some(200),
            data: vec![0xDE, 0xAD, 0xBE, 0xEF],
        };

        let encoded = chunk.encode();
        assert_eq!(&encoded[0..3], b"IMG");
        assert_eq!(encoded[3], 0);
        assert_eq!(encoded[4], 3);
        assert_eq!(u16::from_le_bytes([encoded[5], encoded[6]]), 200);
        assert_eq!(&encoded[7..], &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_chunk_size_constants() {
        assert_eq!(MAX_DATA_PER_CHUNK, 35);
        assert_eq!(FIRST_CHUNK_MAX_DATA, 33);
    }
}
