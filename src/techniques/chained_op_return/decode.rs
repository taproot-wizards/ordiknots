use anyhow::{Context, Result};
use bitcoin::{Transaction, Txid};
use bitcoincore_rpc::{Client, RpcApi};

use crate::techniques::ORDIKNOT_PREFIX;

/// Metadata sizes (must match encoding)
const METADATA_SIZE: usize = 5;
const FIRST_CHUNK_METADATA_SIZE: usize = 7;

#[derive(Debug)]
struct DecodedChunk {
    index: u8,
    total_chunks: u8,
    file_size: Option<u16>,
    data: Vec<u8>,
}

/// Decodes data from the blockchain starting from a given TXID
pub fn decode(start_txid: &Txid, client: &Client) -> Result<Vec<u8>> {
    let chunks = follow_chain(client, *start_txid)?;

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
                "Inconsistent total_chunks at chunk {}: expected {}",
                i,
                total_chunks
            );
        }
    }

    // Reconstruct file
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

    Ok(file_data)
}

/// Recursively follows the transaction chain and collects all chunks
fn follow_chain(client: &Client, txid: Txid) -> Result<Vec<DecodedChunk>> {
    let mut chunks = Vec::new();
    let mut current_txid = txid;

    loop {
        let tx = client
            .get_raw_transaction(&current_txid, None)
            .context(format!("Failed to fetch transaction {}", current_txid))?;

        let chunk = extract_chunk_from_tx(&tx, &current_txid)?;
        chunks.push(chunk);

        match find_continuation_output(client, &tx, &current_txid)? {
            Some(next_txid) => {
                current_txid = next_txid;
            }
            None => break,
        }
    }

    Ok(chunks)
}

/// Extracts the OP_RETURN chunk data from a transaction
fn extract_chunk_from_tx(tx: &Transaction, txid: &Txid) -> Result<DecodedChunk> {
    let op_return_output = tx
        .output
        .iter()
        .find(|out| out.script_pubkey.is_op_return())
        .context(format!("No OP_RETURN output in transaction {}", txid))?;

    let script = &op_return_output.script_pubkey;
    let data = extract_op_return_data(script).context("Failed to extract OP_RETURN data")?;

    if data.len() < METADATA_SIZE {
        anyhow::bail!("OP_RETURN data too small: {} bytes", data.len());
    }

    if &data[0..3] != ORDIKNOT_PREFIX {
        anyhow::bail!("Invalid chunk prefix: expected '444'");
    }

    let index = data[3];
    let total_chunks = data[4];

    let (file_size, chunk_data) = if index == 0 {
        if data.len() < FIRST_CHUNK_METADATA_SIZE {
            anyhow::bail!("First chunk too small: {} bytes", data.len());
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

/// Extracts raw data from an OP_RETURN script
pub(crate) fn extract_op_return_data(script: &bitcoin::ScriptBuf) -> Option<Vec<u8>> {
    let bytes = script.as_bytes();

    if bytes.is_empty() || bytes[0] != 0x6a {
        // OP_RETURN is 0x6a
        return None;
    }

    let mut pos = 1;

    if pos >= bytes.len() {
        return None;
    }

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
        // Direct push
        bytes[pos] as usize
    } else {
        return None;
    };

    pos += 1;

    if pos + data_len > bytes.len() {
        return None;
    }

    Some(bytes[pos..pos + data_len].to_vec())
}

/// Finds the next transaction in the chain by looking for what spends the continuation output
/// Requires bitcoind to be running with -txindex flag
/// Checks both mempool and confirmed blocks
fn find_continuation_output(
    client: &Client,
    tx: &Transaction,
    current_txid: &Txid,
) -> Result<Option<Txid>> {
    // Check if there's a continuation output at vout 1
    if tx.output.len() < 2 || tx.output[1].script_pubkey.is_op_return() {
        return Ok(None);
    }

    // First check mempool (fast path for unconfirmed chains)
    let mempool = client.get_raw_mempool()?;
    for mempool_txid in mempool {
        if let Ok(mempool_tx) = client.get_raw_transaction(&mempool_txid, None) {
            for input in &mempool_tx.input {
                if input.previous_output.txid == *current_txid && input.previous_output.vout == 1 {
                    return Ok(Some(mempool_txid));
                }
            }
        }
    }

    // Use txindex to get the block info for the current transaction
    let tx_info = client.get_raw_transaction_info(current_txid, None)?;
    let start_height = match tx_info.blockhash {
        Some(block_hash) => {
            let block_header = client.get_block_header_info(&block_hash)?;
            block_header.height as u64
        }
        None => {
            // Transaction not confirmed yet, only in mempool
            return Ok(None);
        }
    };

    // Scan forward from the tx's block to find the spender
    // For chained OP_RETURN, continuation tx should be in same or next few blocks
    let current_height = client.get_block_count()?;
    const MAX_BLOCKS_TO_SCAN: u64 = 100;

    for height in start_height..=current_height.min(start_height + MAX_BLOCKS_TO_SCAN) {
        let block_hash = client.get_block_hash(height)?;
        let block = client.get_block(&block_hash)?;

        for candidate_tx in &block.txdata {
            for input in &candidate_tx.input {
                if input.previous_output.txid == *current_txid && input.previous_output.vout == 1 {
                    return Ok(Some(candidate_tx.compute_txid()));
                }
            }
        }
    }

    // Couldn't find spending transaction within reasonable range
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_op_return_data() {
        let script_bytes = vec![0x6a, 0x04, 0xDE, 0xAD, 0xBE, 0xEF];
        let script = bitcoin::ScriptBuf::from_bytes(script_bytes);

        let data = extract_op_return_data(&script).unwrap();
        assert_eq!(data, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }
}
