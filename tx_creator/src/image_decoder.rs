use anyhow::{Context, Result};
use bitcoin::{Transaction, Txid};
use bitcoincore_rpc::{Client, RpcApi};
use std::fs;
use std::path::Path;
use std::str::FromStr;

use crate::image_encoder::{FIRST_CHUNK_METADATA_SIZE, METADATA_SIZE};

#[derive(Debug)]
struct DecodedChunk {
    index: u8,
    total_chunks: u8,
    file_size: Option<u16>, // Only present in first chunk
    data: Vec<u8>,
}

/// Decode an image from the blockchain starting from a given TXID
pub fn decode_from_blockchain(
    rpc: &Client,
    start_txid: &str,
    output_path: &Path,
) -> Result<()> {
    println!("Starting decode from TXID: {}", start_txid);

    let txid = Txid::from_str(start_txid)
        .context("Invalid TXID format")?;

    // Follow the transaction chain and collect all chunks
    let chunks = follow_chain(rpc, txid)?;

    println!("\nCollected {} chunks", chunks.len());

    // Validate we have all chunks
    if chunks.is_empty() {
        anyhow::bail!("No chunks found in transaction chain");
    }

    let total_chunks = chunks[0].total_chunks;
    if chunks.len() != total_chunks as usize {
        anyhow::bail!(
            "Missing chunks: expected {}, found {}",
            total_chunks,
            chunks.len()
        );
    }

    // Get file size from first chunk
    let file_size = chunks[0]
        .file_size
        .context("First chunk missing file size")? as usize;

    // Sort by index
    let mut sorted_chunks = chunks;
    sorted_chunks.sort_by_key(|c| c.index);

    // Validate indices are sequential
    for (i, chunk) in sorted_chunks.iter().enumerate() {
        if chunk.index != i as u8 {
            anyhow::bail!("Missing chunk index {}", i);
        }
        if chunk.total_chunks != total_chunks {
            anyhow::bail!(
                "Inconsistent total_chunks: chunk {} has {}, expected {}",
                i,
                chunk.total_chunks,
                total_chunks
            );
        }
    }

    // Reconstruct the file
    let mut file_data = Vec::new();
    for chunk in sorted_chunks {
        file_data.extend_from_slice(&chunk.data);
    }

    // Trim to actual file size (removes padding)
    if file_data.len() > file_size {
        file_data.truncate(file_size);
    } else if file_data.len() < file_size {
        anyhow::bail!(
            "Incomplete file data: expected {} bytes, got {}",
            file_size,
            file_data.len()
        );
    }

    // Write to disk
    fs::write(output_path, &file_data)
        .context(format!("Failed to write to {}", output_path.display()))?;

    println!("\nSuccessfully decoded {} bytes to {}", file_data.len(), output_path.display());

    Ok(())
}

/// Recursively follow the transaction chain and collect all chunks
fn follow_chain(rpc: &Client, txid: Txid) -> Result<Vec<DecodedChunk>> {
    let mut chunks = Vec::new();
    let mut current_txid = txid;

    loop {
        // Fetch the transaction
        let tx_result = rpc.get_raw_transaction(&current_txid, None);

        let tx = match tx_result {
            Ok(t) => t,
            Err(e) => {
                anyhow::bail!("Failed to fetch transaction {}: {}", current_txid, e);
            }
        };

        // Extract OP_RETURN data
        let chunk = extract_chunk_from_tx(&tx, &current_txid)?;
        println!("  Chunk {}/{} from {}", chunk.index + 1, chunk.total_chunks, current_txid);
        chunks.push(chunk);

        // Look for continuation output
        match find_continuation_output(rpc, &tx, &current_txid)? {
            Some(next_txid) => {
                println!("  → Following to next transaction");
                current_txid = next_txid;
            }
            None => {
                // No continuation output, we're done
                break;
            }
        }
    }

    Ok(chunks)
}

/// Extract the OP_RETURN chunk data from a transaction
fn extract_chunk_from_tx(tx: &Transaction, txid: &Txid) -> Result<DecodedChunk> {
    // Find OP_RETURN output (should be vout 0)
    let op_return_output = tx
        .output
        .iter()
        .find(|out| out.script_pubkey.is_op_return())
        .context(format!("No OP_RETURN output found in transaction {}", txid))?;

    // Extract the data from OP_RETURN
    let script = &op_return_output.script_pubkey;
    let data = extract_op_return_data(script)
        .context("Failed to extract data from OP_RETURN")?;

    // Parse chunk metadata
    if data.len() < METADATA_SIZE {
        anyhow::bail!(
            "OP_RETURN data too small: {} bytes (expected at least {})",
            data.len(),
            METADATA_SIZE
        );
    }

    // Verify "IMG" prefix
    if &data[0..3] != b"IMG" {
        anyhow::bail!("Invalid chunk prefix: expected 'IMG', got {:?}", &data[0..3]);
    }

    let index = data[3];
    let total_chunks = data[4];

    // First chunk (index 0) has file size
    let (file_size, chunk_data) = if index == 0 {
        if data.len() < FIRST_CHUNK_METADATA_SIZE {
            anyhow::bail!(
                "First chunk too small: {} bytes (expected at least {})",
                data.len(),
                FIRST_CHUNK_METADATA_SIZE
            );
        }
        let size = u16::from_le_bytes([data[5], data[6]]);
        (Some(size), data[FIRST_CHUNK_METADATA_SIZE..].to_vec())
    } else {
        (None, data[METADATA_SIZE..].to_vec())
    };

    Ok(DecodedChunk {
        index,
        total_chunks,
        file_size,
        data: chunk_data,
    })
}

/// Extract raw data from an OP_RETURN script
fn extract_op_return_data(script: &bitcoin::ScriptBuf) -> Option<Vec<u8>> {
    let bytes = script.as_bytes();

    // OP_RETURN is 0x6a
    if bytes.is_empty() || bytes[0] != 0x6a {
        return None;
    }

    // Skip OP_RETURN byte
    let mut pos = 1;

    // Handle push opcodes
    if pos >= bytes.len() {
        return None;
    }

    // Check for OP_PUSHDATA opcodes
    let data_len = if bytes[pos] == 0x4c {
        // OP_PUSHDATA1
        pos += 1;
        if pos >= bytes.len() {
            return None;
        }
        bytes[pos] as usize
    } else if bytes[pos] == 0x4d {
        // OP_PUSHDATA2
        pos += 1;
        if pos + 1 >= bytes.len() {
            return None;
        }
        u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize
    } else if bytes[pos] == 0x4e {
        // OP_PUSHDATA4
        pos += 1;
        if pos + 3 >= bytes.len() {
            return None;
        }
        u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as usize
    } else if bytes[pos] <= 75 {
        // Direct push (OP_PUSH1-75)
        bytes[pos] as usize
    } else {
        return None;
    };

    pos += 1;

    // Extract the data
    if pos + data_len > bytes.len() {
        return None;
    }

    Some(bytes[pos..pos + data_len].to_vec())
}

/// Find the next transaction in the chain by looking for what spends the continuation output
/// Returns the TXID of the spending transaction
fn find_continuation_output(rpc: &Client, tx: &Transaction, current_txid: &Txid) -> Result<Option<Txid>> {
    // Check if there's a continuation output at vout 1
    if tx.output.len() < 2 {
        return Ok(None);
    }

    // Vout 0 is OP_RETURN, vout 1 is continuation (if it exists), vout 2+ is change
    // Check if vout 1 is not an OP_RETURN (i.e., it's spendable)
    if tx.output[1].script_pubkey.is_op_return() {
        return Ok(None);
    }

    // We have a continuation output at vout 1
    // Search for a transaction that spends this output
    // We'll check the mempool and recent transactions

    // First, check mempool
    let mempool = rpc.get_raw_mempool()?;
    for mempool_txid in mempool {
        let mempool_tx = rpc.get_raw_transaction(&mempool_txid, None)?;

        // Check if this transaction spends our continuation output
        for input in &mempool_tx.input {
            if input.previous_output.txid == *current_txid && input.previous_output.vout == 1 {
                return Ok(Some(mempool_txid));
            }
        }
    }

    // If not in mempool, check recent blocks (it's likely already mined if encoded together)
    // Get all transactions from wallet that might be related
    let transactions = rpc.list_transactions(None, Some(100), None, Some(true))?;

    for tx_info in transactions {
        let txid = tx_info.info.txid;
        let candidate_tx = rpc.get_raw_transaction(&txid, None)?;

        // Check if this transaction spends our continuation output
        for input in &candidate_tx.input {
            if input.previous_output.txid == *current_txid && input.previous_output.vout == 1 {
                return Ok(Some(txid));
            }
        }
    }

    // No spending transaction found
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_op_return_data() {
        // Create a simple OP_RETURN script: OP_RETURN OP_PUSH4 0xDEADBEEF
        let script_bytes = vec![0x6a, 0x04, 0xDE, 0xAD, 0xBE, 0xEF];
        let script = bitcoin::ScriptBuf::from_bytes(script_bytes);

        let data = extract_op_return_data(&script).unwrap();
        assert_eq!(data, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_parse_first_chunk() {
        // First chunk with file size: "IMG" + index(0) + total(3) + size(200) + data
        let mut data = vec![b'I', b'M', b'G', 0, 3];
        data.extend_from_slice(&200u16.to_le_bytes());
        data.extend_from_slice(&[0xDE, 0xAD]);

        let script = bitcoin::ScriptBuf::new_op_return(&bitcoin::script::PushBytesBuf::try_from(data).unwrap());
        let extracted = extract_op_return_data(&script).unwrap();

        assert_eq!(&extracted[0..3], b"IMG");
        assert_eq!(extracted[3], 0);
        assert_eq!(extracted[4], 3);
        assert_eq!(u16::from_le_bytes([extracted[5], extracted[6]]), 200);
        assert_eq!(&extracted[7..], &[0xDE, 0xAD]);
    }
}
