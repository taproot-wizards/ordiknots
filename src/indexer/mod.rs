use anyhow::Result;
use bitcoin::Txid;
use bitcoin_hashes::Hash;
use bitcoincore_rpc::{Client, RpcApi};
use indicatif::{ProgressBar, ProgressStyle};
use redb::{Database, ReadableDatabase, ReadableTableMetadata, TableDefinition};
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::techniques::{self, Technique};

/// Key: txid (36 bytes), Value: (technique: u8, block_height: u64, file_size: u64)
const KNOTWORKS_TABLE: TableDefinition<&[u8; 32], (u8, u64, u64)> =
    TableDefinition::new("knotworks");

/// Key: "last_block", Value: block height
const STATUS_TABLE: TableDefinition<&str, u64> = TableDefinition::new("status");

const STATUS_KEY_LAST_BLOCK: &str = "last_block";

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

fn get_last_indexed_block(db: &Database) -> Result<u64> {
    let read_txn = db.begin_read()?;
    let table = read_txn.open_table(STATUS_TABLE)?;

    match table.get(STATUS_KEY_LAST_BLOCK)? {
        Some(height) => Ok(height.value()),
        None => Ok(0), // Start from genesis
    }
}

fn update_last_indexed_block(db: &Database, height: u64) -> Result<()> {
    let write_txn = db.begin_write()?;
    {
        let mut table = write_txn.open_table(STATUS_TABLE)?;
        table.insert(STATUS_KEY_LAST_BLOCK, height)?;
    }
    write_txn.commit()?;
    Ok(())
}

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

fn process_block(db: &Database, client: &Client, height: u64, pb: &ProgressBar) -> Result<()> {
    let block_hash = client.get_block_hash(height)?;
    let block = client.get_block(&block_hash)?;

    for tx in &block.txdata {
        if let Some(technique) = techniques::detect_technique(tx) {
            let txid = tx.compute_txid();

            match technique.decode(&txid, client) {
                Ok(data) => {
                    let file_size = data.len() as u64;
                    let message = format!(
                        "  ✓ Found {} at {} (block {}, size {} bytes)",
                        technique, txid, height, file_size
                    );

                    pb.println(message);

                    store_transaction(db, &txid, technique, height, file_size)?;
                }
                Err(e) => {
                    // Detection matched but decode failed - likely false positive
                    let message = format!(
                        "  ⚠ Warning: {} detected at {} but decode failed: {}",
                        technique, txid, e
                    );

                    pb.println(message);
                }
            }
        }
    }

    Ok(())
}

fn listen_for_new_blocks(
    db: &Database,
    client: &Client,
    current_height: u64,
    pb: &ProgressBar,
) -> Result<()> {
    let mut last_height = current_height;
    let poll_interval = Duration::from_secs(5);

    loop {
        thread::sleep(poll_interval);

        match client.get_block_count() {
            Ok(new_height) => {
                if new_height > last_height {
                    // New blocks detected! Update progress bar length
                    pb.set_length(new_height);

                    for height in (last_height + 1)..=new_height {
                        match process_block(db, client, height, pb) {
                            Ok(_) => {
                                update_last_indexed_block(db, height)?;
                                pb.set_position(height);
                                last_height = height;
                            }
                            Err(e) => {
                                pb.println(format!("Error processing block {}: {}", height, e));
                                break;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                pb.println(format!("Error checking for new blocks: {}", e));
            }
        }
    }
}

pub fn sync(db: &Database, client: &Client) -> Result<()> {
    let last_indexed = get_last_indexed_block(db)?;
    let current_height = client.get_block_count()?;

    let pb = ProgressBar::new(current_height);
    pb.set_position(last_indexed);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} {bar:40.cyan/blue} {pos}/{len} blocks")
            .unwrap()
            .progress_chars("█▓░"),
    );

    let start_height = last_indexed + 1;

    for height in start_height..=current_height {
        process_block(db, client, height, &pb)?;
        update_last_indexed_block(db, height)?;

        pb.set_position(height);
    }

    update_last_indexed_block(db, current_height)?;

    pb.finish_with_message(format!("✓ Synced to block {}", current_height));

    listen_for_new_blocks(db, client, current_height, &pb)
}

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
