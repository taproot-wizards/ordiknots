use anyhow::Result;
use bitcoin::Txid;
use bitcoin_hashes::Hash;
use bitcoincore_rpc::{Client, RpcApi};
use indicatif::{ProgressBar, ProgressStyle};
use redb::{Database, ReadableDatabase, ReadableTableMetadata, TableDefinition};
use std::path::Path;

use crate::techniques::{self, Technique};

/// Table storing indexed transactions
/// Key: txid (36 bytes), Value: (technique: u8, block_height: u64, file_size: u64)
const KNOTWORKS_TABLE: TableDefinition<&[u8; 32], (u8, u64, u64)> =
    TableDefinition::new("knotworks");

/// Table storing indexer state
/// Key: "last_block", Value: block height
const STATUS_TABLE: TableDefinition<&str, u64> = TableDefinition::new("status");

const STATUS_KEY_LAST_BLOCK: &str = "last_block";

/// Opens or creates the database
pub fn open_database<P: AsRef<Path>>(path: P) -> Result<Database> {
    // Create parent directories if they don't exist
    if let Some(parent) = path.as_ref().parent() {
        std::fs::create_dir_all(parent)?;
    }

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

/// Scans the blockchain for knotworks and indexes them:
pub fn start_indexing(db: &Database, client: &Client) -> Result<()> {
    let last_indexed = get_last_indexed_block(db)?;
    let current_height = client.get_block_count()?;

    println!("Starting indexer...");
    println!();

    let start_height = last_indexed + 1;

    // Create a nice progress bar showing actual block heights
    let pb = ProgressBar::new(current_height);
    pb.set_position(last_indexed);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} blocks)")
            .unwrap()
            .progress_chars("█▓░"),
    );

    for height in start_height..=current_height {
        let block_hash = client.get_block_hash(height)?;
        let block = client.get_block(&block_hash)?;

        for tx in &block.txdata {
            // Quick detection to see if this tx uses any encoding technique
            if let Some(technique) = techniques::detect_technique(tx) {
                let txid = tx.compute_txid();

                // Run full decode to confirm and get the actual data
                match technique.decode(&txid, client) {
                    Ok(data) => {
                        let file_size = data.len() as u64;
                        pb.println(format!(
                            "  ✓ Found {} at {} (block {}, size {} bytes)",
                            technique, txid, height, file_size
                        ));
                        store_transaction(db, &txid, technique, height, file_size)?;
                    }
                    Err(e) => {
                        // Detection matched but decode failed - likely false positive
                        pb.println(format!(
                            "  ⚠ Warning: {} detected at {} but decode failed: {}",
                            technique, txid, e
                        ));
                    }
                }
            }
        }

        // Update progress every 100 blocks
        if height % 100 == 0 {
            update_last_indexed_block(db, height)?;
        }

        pb.set_position(height);
    }

    // Final update
    if current_height >= start_height {
        update_last_indexed_block(db, current_height)?;
    }

    pb.finish_with_message("✓ Indexing complete!");

    Ok(())
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
