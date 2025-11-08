use anyhow::{Context, Result};
use bitcoin::Transaction;
use bitcoincore_rpc::{
    json::{ListUnspentResultEntry, SignRawTransactionInput},
    Client, RpcApi,
};

pub fn sign_transaction(client: &Client, tx: &Transaction) -> Result<Transaction> {
    sign_transaction_with_prevtxs(client, tx, None)
}

pub fn sign_transaction_with_prevtxs(
    client: &Client,
    tx: &Transaction,
    prevtxs: Option<&[Transaction]>,
) -> Result<Transaction> {
    let prev_tx_inputs = prevtxs.map(|txs| {
        txs.iter()
            .flat_map(|prev_tx| {
                let txid = prev_tx.compute_txid();
                prev_tx.output.iter().enumerate().map(move |(vout, output)| {
                    SignRawTransactionInput {
                        txid,
                        vout: vout as u32,
                        script_pub_key: output.script_pubkey.clone(),
                        redeem_script: None,
                        amount: Some(output.value),
                    }
                })
            })
            .collect::<Vec<_>>()
    });

    let signed = client
        .sign_raw_transaction_with_wallet(tx, prev_tx_inputs.as_deref(), None)
        .context("Failed to sign transaction with wallet")?;

    if !signed.complete {
        anyhow::bail!("Transaction signing incomplete");
    }

    signed
        .transaction()
        .context("Failed to get signed transaction")
}

pub fn get_new_address(client: &Client) -> Result<bitcoin::Address> {
    let address = client
        .get_new_address(None, None)
        .context("Failed to get new address from wallet")?;

    Ok(address.assume_checked())
}

pub fn select_largest_utxo(client: &Client) -> Result<ListUnspentResultEntry> {
    let utxos = client
        .list_unspent(None, None, None, None, None)
        .context("Failed to list unspent outputs")?;

    utxos
        .into_iter()
        .max_by_key(|utxo| utxo.amount.to_sat())
        .context("No UTXOs available in wallet")
}
