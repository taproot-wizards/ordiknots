use anyhow::{Context, Result};
use bitcoin::Transaction;
use bitcoin::consensus::encode::serialize_hex;
use bitcoincore_rpc::{Client, RpcApi};

/// Broadcasts a transaction to the network or performs a dry-run
pub fn broadcast_or_dryrun(
    client: &Client,
    tx: &Transaction,
    broadcast: bool,
    label: Option<&str>,
) -> Result<bitcoin::Txid> {
    let txid = tx.compute_txid();

    if broadcast {
        client
            .send_raw_transaction(tx)
            .context("Failed to broadcast transaction")?;

        if let Some(label) = label {
            println!("✓ Broadcasted {} - txid: {}", label, txid);
        } else {
            println!("✓ Broadcasted transaction: {}", txid);
        }
    } else {
        if let Some(label) = label {
            println!("[DRY RUN] {} - txid: {}", label, txid);
        } else {
            println!("[DRY RUN] Transaction: {}", txid);
        }
        println!("Raw hex: {}", serialize_hex(tx));
    }

    Ok(txid)
}

/// Broadcasts multiple transactions in sequence
pub fn broadcast_multiple(
    client: &Client,
    transactions: &[(Transaction, Option<&str>)],
    broadcast: bool,
) -> Result<Vec<bitcoin::Txid>> {
    let mut txids = Vec::new();

    for (tx, label) in transactions {
        let txid = broadcast_or_dryrun(client, tx, broadcast, *label)?;
        txids.push(txid);
    }

    Ok(txids)
}
