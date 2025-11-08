use anyhow::{Context, Result};
use bitcoin::script::PushBytesBuf;
use bitcoin::secp256k1::{Message, Secp256k1 as Secp256k1Context, SecretKey};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{
    absolute, opcodes, script, transaction, Amount, Network, PublicKey, ScriptBuf, Transaction,
    TxIn, TxOut, Witness,
};
use bitcoin_hashes::Hash;
use bitcoincore_rpc::Client;

use crate::techniques::ORDIKNOT_PREFIX;
use crate::utils::{transaction as tx_utils, wallet};

/// Maximum pubkeys in CHECKMULTISIG (Bitcoin consensus)
const MAX_PUBKEYS_PER_MULTISIG: usize = 20;

/// Bytes per fake pubkey after prefix (compressed pubkey format)
const DATA_BYTES_PER_PUBKEY: usize = 32;

/// Maximum data capacity (19 fake pubkeys, 1 real pubkey for signing, minus 3 bytes for prefix)
pub const MAX_DATA_CAPACITY: usize = (MAX_PUBKEYS_PER_MULTISIG - 1) * DATA_BYTES_PER_PUBKEY - ORDIKNOT_PREFIX.len();

/// Encodes data using P2WSH CHECKMULTISIG witness script
pub fn encode(data: &[u8], client: &Client) -> Result<(Vec<Transaction>, bitcoin::Txid)> {
    if data.len() > MAX_DATA_CAPACITY {
        anyhow::bail!(
            "Data too large: {} bytes (max {} bytes)",
            data.len(),
            MAX_DATA_CAPACITY
        );
    }

    // Generate real keypair for signing
    let secp = Secp256k1Context::new();
    let (real_secret_key, real_pubkey) = generate_real_keypair(&secp)?;

    // Prepend "444" prefix to data
    let mut prefixed_data = Vec::with_capacity(ORDIKNOT_PREFIX.len() + data.len());
    prefixed_data.extend_from_slice(ORDIKNOT_PREFIX);
    prefixed_data.extend_from_slice(data);

    // Encode data as fake pubkeys
    let fake_pubkeys = encode_data_as_fake_pubkeys(&prefixed_data)?;
    println!("Encoded data into {} fake pubkeys", fake_pubkeys.len());

    // Build witnessScript (1-of-N multisig)
    let witnessscript = build_multisig_witnessscript(&real_pubkey, &fake_pubkeys)?;

    // Create P2WSH address
    let p2wsh_address = bitcoin::Address::p2wsh(&witnessscript, Network::Regtest);

    // Create funding transaction to P2WSH address
    let funding_amount = 100_000; // 100k sats
    let funding_tx = create_funding_transaction(client, &p2wsh_address, funding_amount)?;
    let funding_txid = funding_tx.compute_txid();

    // Create spending transaction with witness data
    let spending_tx = create_spending_transaction(
        client,
        funding_txid,
        funding_amount,
        &real_secret_key,
        &witnessscript,
    )?;

    let spending_txid = spending_tx.compute_txid();

    Ok((vec![funding_tx, spending_tx], spending_txid))
}

/// Encodes data as fake pubkeys (0x02/0x03 prefix + 32 bytes)
fn encode_data_as_fake_pubkeys(data: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut fake_pubkeys = Vec::new();

    for chunk in data.chunks(DATA_BYTES_PER_PUBKEY) {
        let mut fake_pubkey = Vec::with_capacity(33);

        // Choose prefix based on first bit of data
        let prefix = if !chunk.is_empty() && (chunk[0] & 0x80) != 0 {
            0x03
        } else {
            0x02
        };
        fake_pubkey.push(prefix);

        // Add the data bytes
        fake_pubkey.extend_from_slice(chunk);

        // Pad to 32 bytes if this is the last chunk
        if chunk.len() < DATA_BYTES_PER_PUBKEY {
            fake_pubkey.extend(vec![0u8; DATA_BYTES_PER_PUBKEY - chunk.len()]);
        }

        fake_pubkeys.push(fake_pubkey);
    }

    Ok(fake_pubkeys)
}

/// Generates a real keypair for spending the multisig
fn generate_real_keypair(
    secp: &Secp256k1Context<bitcoin::secp256k1::All>,
) -> Result<(SecretKey, PublicKey)> {
    let secret_key = SecretKey::new(&mut rand::thread_rng());
    let secp_pubkey = bitcoin::secp256k1::PublicKey::from_secret_key(secp, &secret_key);
    let public_key = PublicKey::new(secp_pubkey);

    Ok((secret_key, public_key))
}

/// Builds a 1-of-N multisig witnessScript
fn build_multisig_witnessscript(
    real_pubkey: &PublicKey,
    fake_pubkeys: &[Vec<u8>],
) -> Result<ScriptBuf> {
    let total_pubkeys = 1 + fake_pubkeys.len();

    if total_pubkeys > MAX_PUBKEYS_PER_MULTISIG {
        anyhow::bail!(
            "Too many pubkeys: {} (max {})",
            total_pubkeys,
            MAX_PUBKEYS_PER_MULTISIG
        );
    }

    let mut builder = script::Builder::new();

    // OP_1 (we need 1 signature)
    builder = builder.push_opcode(opcodes::all::OP_PUSHNUM_1);

    // Push real pubkey first
    builder = builder.push_key(real_pubkey);

    // Push all fake pubkeys
    for fake_pk in fake_pubkeys {
        let push_bytes = PushBytesBuf::try_from(fake_pk.clone())
            .context("Failed to create push bytes from fake pubkey")?;
        builder = builder.push_slice(push_bytes);
    }

    // OP_N (total number of pubkeys)
    let n_opcode = match total_pubkeys {
        1 => opcodes::all::OP_PUSHNUM_1,
        2 => opcodes::all::OP_PUSHNUM_2,
        3 => opcodes::all::OP_PUSHNUM_3,
        4 => opcodes::all::OP_PUSHNUM_4,
        5 => opcodes::all::OP_PUSHNUM_5,
        6 => opcodes::all::OP_PUSHNUM_6,
        7 => opcodes::all::OP_PUSHNUM_7,
        8 => opcodes::all::OP_PUSHNUM_8,
        9 => opcodes::all::OP_PUSHNUM_9,
        10 => opcodes::all::OP_PUSHNUM_10,
        11 => opcodes::all::OP_PUSHNUM_11,
        12 => opcodes::all::OP_PUSHNUM_12,
        13 => opcodes::all::OP_PUSHNUM_13,
        14 => opcodes::all::OP_PUSHNUM_14,
        15 => opcodes::all::OP_PUSHNUM_15,
        16 => opcodes::all::OP_PUSHNUM_16,
        n @ 17..=20 => {
            builder = builder.push_int(n as i64);
            opcodes::all::OP_CHECKMULTISIG
        }
        _ => anyhow::bail!("Invalid number of pubkeys: {}", total_pubkeys),
    };

    if total_pubkeys <= 16 {
        builder = builder.push_opcode(n_opcode);
    }

    builder = builder.push_opcode(opcodes::all::OP_CHECKMULTISIG);

    Ok(builder.into_script())
}

/// Creates a funding transaction that pays to a P2WSH address
fn create_funding_transaction(
    client: &Client,
    p2wsh_address: &bitcoin::Address,
    funding_amount: u64,
) -> Result<Transaction> {
    let change_address = wallet::get_new_address(client)?;
    let utxo = wallet::select_largest_utxo(client)?;

    let input = tx_utils::create_tx_input(bitcoin::OutPoint {
        txid: utxo.txid,
        vout: utxo.vout,
    });

    let total_out = funding_amount + tx_utils::TX_FEE_SATS;
    let change_amount = utxo
        .amount
        .to_sat()
        .checked_sub(total_out)
        .context("Insufficient funds for funding transaction")?;

    let outputs = vec![
        TxOut {
            value: Amount::from_sat(funding_amount),
            script_pubkey: p2wsh_address.script_pubkey(),
        },
        TxOut {
            value: Amount::from_sat(change_amount),
            script_pubkey: change_address.script_pubkey(),
        },
    ];

    let tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![input],
        output: outputs,
    };

    wallet::sign_transaction(client, &tx)
}

/// Creates a spending transaction that spends the P2WSH output
fn create_spending_transaction(
    client: &Client,
    funding_txid: bitcoin::Txid,
    input_amount: u64,
    secret_key: &SecretKey,
    witnessscript: &ScriptBuf,
) -> Result<Transaction> {
    let change_address = wallet::get_new_address(client)?;

    let input = TxIn {
        previous_output: bitcoin::OutPoint {
            txid: funding_txid,
            vout: 0,
        },
        script_sig: ScriptBuf::new(),
        sequence: bitcoin::Sequence::MAX,
        witness: Witness::new(),
    };

    let change_amount = input_amount
        .checked_sub(tx_utils::TX_FEE_SATS)
        .context("Insufficient funds for spending transaction")?;

    let output = TxOut {
        value: Amount::from_sat(change_amount),
        script_pubkey: change_address.script_pubkey(),
    };

    let mut tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![input],
        output: vec![output],
    };

    sign_p2wsh_multisig(&mut tx, secret_key, witnessscript, input_amount)?;

    Ok(tx)
}

/// Signs a P2WSH multisig transaction
fn sign_p2wsh_multisig(
    tx: &mut Transaction,
    secret_key: &SecretKey,
    witnessscript: &ScriptBuf,
    input_amount: u64,
) -> Result<()> {
    let secp = Secp256k1Context::new();

    let mut sighash_cache = SighashCache::new(tx.clone());

    let sighash = sighash_cache
        .p2wsh_signature_hash(
            0,
            witnessscript,
            Amount::from_sat(input_amount),
            EcdsaSighashType::All,
        )
        .context("Failed to compute P2WSH sighash")?;

    let message = Message::from_digest_slice(sighash.as_byte_array())
        .context("Failed to create message from sighash")?;
    let signature = secp.sign_ecdsa(&message, secret_key);

    let mut sig_bytes = signature.serialize_der().to_vec();
    sig_bytes.push(EcdsaSighashType::All.to_u32() as u8);

    // Build witness: [OP_0, signature, witnessScript]
    let mut witness = Witness::new();
    witness.push([]); // OP_0 for CHECKMULTISIG bug
    witness.push(sig_bytes);
    witness.push(witnessscript.as_bytes());

    tx.input[0].witness = witness;

    Ok(())
}
