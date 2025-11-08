use anyhow::Result;
use bitcoin::{Transaction, Txid};
use bitcoin_hashes::Hash;
use bitcoincore_rpc::{Client, RpcApi};
use redb::{Database, ReadableDatabase, ReadableTableMetadata, TableDefinition};
use std::path::Path;

use crate::techniques::Technique;

/// Table storing indexed transactions
/// Key: txid (36 bytes), Value: (technique: u8, block_height: u64, file_size: u64)
const KNOTWORKS_TABLE: TableDefinition<&[u8; 32], (u8, u64, u64)> =
    TableDefinition::new("knotworks");

/// Table storing indexer state
/// Key: "last_block", Value: block height
const STATUS_TABLE: TableDefinition<&str, u64> = TableDefinition::new("status");

const STATUS_KEY_LAST_BLOCK: &str = "last_block";

/// Prefix used by both encoding techniques
const DATA_PREFIX: &[u8] = b"444";

/// Opens or creates the database
pub fn open_database<P: AsRef<Path>>(path: P) -> Result<Database> {
    let db = Database::create(path)?;

    // Initialize tables
    let write_txn = db.begin_write()?;
    {
        let _table = write_txn.open_table(KNOTWORKS_TABLE)?;
        let _status = write_txn.open_table(STATUS_TABLE)?;
    }
    write_txn.commit()?;

    Ok(db)
}

/// Gets the last indexed block height
fn get_last_indexed_block(db: &Database) -> Result<u64> {
    let read_txn = db.begin_read()?;
    let table = read_txn.open_table(STATUS_TABLE)?;

    match table.get(STATUS_KEY_LAST_BLOCK)? {
        Some(height) => Ok(height.value()),
        None => Ok(0), // Start from genesis
    }
}

/// Updates the last indexed block height
fn update_last_indexed_block(db: &Database, height: u64) -> Result<()> {
    let write_txn = db.begin_write()?;
    {
        let mut table = write_txn.open_table(STATUS_TABLE)?;
        table.insert(STATUS_KEY_LAST_BLOCK, height)?;
    }
    write_txn.commit()?;
    Ok(())
}

/// Stores an indexed transaction
fn store_transaction(
    db: &Database,
    txid: &Txid,
    technique: Technique,
    block_height: u64,
    file_size: u64,
) -> Result<()> {
    let write_txn = db.begin_write()?;
    {
        let mut table = write_txn.open_table(KNOTWORKS_TABLE)?;
        let txid_bytes: [u8; 32] = txid.to_byte_array();
        table.insert(&txid_bytes, (technique as u8, block_height, file_size))?;
    }
    write_txn.commit()?;
    Ok(())
}

/// Scans the blockchain for encoded data and indexes it
pub fn start_indexing(db: &Database, client: &Client) -> Result<()> {
    let last_indexed = get_last_indexed_block(db)?;
    let current_height = client.get_block_count()?;

    println!("Starting indexer...");
    println!("Last indexed block: {}", last_indexed);
    println!("Current blockchain height: {}", current_height);
    println!(
        "Blocks to scan: {}",
        current_height.saturating_sub(last_indexed)
    );
    println!();

    let start_height = last_indexed + 1;

    for height in start_height..=current_height {
        if height % 100 == 0 || height == start_height {
            println!("Scanning block {} / {}", height, current_height);
        }

        let block_hash = client.get_block_hash(height)?;
        let block = client.get_block(&block_hash)?;

        for tx in &block.txdata {
            if let Some((technique, file_size)) = detect_encoded_data(tx)? {
                let txid = tx.compute_txid();
                println!(
                    "  Found {} at {} (block {}, size {} bytes)",
                    technique, txid, height, file_size
                );
                store_transaction(db, &txid, technique, height, file_size)?;
            }
        }

        // Update progress every 100 blocks
        if height % 100 == 0 {
            update_last_indexed_block(db, height)?;
        }
    }

    // Final update
    if current_height >= start_height {
        update_last_indexed_block(db, current_height)?;
    }

    println!("✓ Indexing complete!");

    Ok(())
}

/// Detects if a transaction contains encoded data and returns the technique type and file size
fn detect_encoded_data(tx: &Transaction) -> Result<Option<(Technique, u64)>> {
    // Check for Chained OP_RETURN (look for OP_RETURN with "444" prefix)
    if let Some(file_size) = detect_chained_op_return(tx)? {
        return Ok(Some((Technique::ChainedOpReturn, file_size)));
    }

    // Check for P2WSH Fake Multisig (witness script with "444" prefix)
    if let Some(file_size) = detect_p2wsh_fake_multisig(tx)? {
        return Ok(Some((Technique::P2wshFakeMultisig, file_size)));
    }

    Ok(None)
}

/// Detects Chained OP_RETURN transactions
fn detect_chained_op_return(tx: &Transaction) -> Result<Option<u64>> {
    for output in &tx.output {
        if output.script_pubkey.is_op_return() {
            if let Some(data) = extract_op_return_data(&output.script_pubkey) {
                // Check for "444" prefix
                if data.len() >= 7 && &data[0..3] == DATA_PREFIX {
                    // First chunk format: "444" + index + total + file_size (2 bytes)
                    let index = data[3];
                    if index == 0 {
                        // This is the first chunk, extract file size
                        let file_size = u16::from_le_bytes([data[5], data[6]]) as u64;
                        return Ok(Some(file_size));
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Detects P2WSH Fake Multisig transactions
fn detect_p2wsh_fake_multisig(tx: &Transaction) -> Result<Option<u64>> {
    if tx.input.is_empty() {
        return Ok(None);
    }

    let witness = &tx.input[0].witness;
    if witness.len() < 3 {
        return Ok(None);
    }

    // Extract witnessScript (last element)
    let witnessscript_bytes = match witness.last() {
        Some(bytes) => bytes,
        None => return Ok(None),
    };

    // Try to extract fake pubkeys
    let fake_pubkeys = match extract_fake_pubkeys_from_script(witnessscript_bytes) {
        Ok(pubkeys) => pubkeys,
        Err(_) => return Ok(None),
    };

    if fake_pubkeys.is_empty() {
        return Ok(None);
    }

    // Decode the first few bytes to check for "444" prefix
    let mut data_prefix = Vec::new();
    for pubkey in fake_pubkeys.iter().take(1) {
        if pubkey.len() == 33 {
            data_prefix.extend_from_slice(&pubkey[1..33]);
        }
    }

    if data_prefix.len() >= 3 && &data_prefix[0..3] == DATA_PREFIX {
        // Calculate approximate file size (total fake pubkey data minus prefix and padding)
        let total_data_bytes = fake_pubkeys.len() * 32;
        // Estimate file size (we can't know exact size without decoding fully, but we know max)
        let file_size = total_data_bytes.saturating_sub(3) as u64; // Subtract prefix
        return Ok(Some(file_size));
    }

    Ok(None)
}

/// Extracts raw data from an OP_RETURN script
fn extract_op_return_data(script: &bitcoin::ScriptBuf) -> Option<Vec<u8>> {
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

/// Extracts fake pubkeys from a CHECKMULTISIG witnessScript
fn extract_fake_pubkeys_from_script(script_bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut pubkeys = Vec::new();
    let mut pos = 0;

    // Skip OP_1 (required signatures)
    if pos >= script_bytes.len() {
        anyhow::bail!("Script too short");
    }
    pos += 1;

    // Extract all pubkeys
    while pos < script_bytes.len() {
        let opcode = script_bytes[pos];

        // Stop if we hit OP_N or OP_CHECKMULTISIG
        if opcode == 0xae {
            // OP_CHECKMULTISIG
            break;
        }
        if (0x51..=0x60).contains(&opcode) {
            // OP_1 through OP_16
            break;
        }
        if (0x01..=0x4b).contains(&opcode) {
            // Direct push (1-75 bytes)
            pos += 1;
            if pos + (opcode as usize) > script_bytes.len() {
                anyhow::bail!("Invalid script: push extends beyond script");
            }
            let pubkey = script_bytes[pos..pos + (opcode as usize)].to_vec();
            pubkeys.push(pubkey);
            pos += opcode as usize;
        } else {
            break;
        }
    }

    if pubkeys.is_empty() {
        anyhow::bail!("No pubkeys found in script");
    }

    // Remove the first pubkey (real signing key)
    pubkeys.remove(0);

    Ok(pubkeys)
}

/// Gets statistics about indexed knotworks
pub fn get_stats(db: &Database) -> Result<()> {
    let read_txn = db.begin_read()?;
    let table = read_txn.open_table(KNOTWORKS_TABLE)?;
    let status_table = read_txn.open_table(STATUS_TABLE)?;

    let total_count = table.len()?;
    let last_block = match status_table.get(STATUS_KEY_LAST_BLOCK)? {
        Some(height) => height.value(),
        None => 0,
    };

    println!("Ordiknots Stats");
    println!("===============");
    println!("Last indexed block: {}", last_block);
    println!("Total knotworks: {}", total_count);

    Ok(())
}
