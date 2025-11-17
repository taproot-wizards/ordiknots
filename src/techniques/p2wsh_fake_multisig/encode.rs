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
pub const MAX_DATA_CAPACITY: usize =
    (MAX_PUBKEYS_PER_MULTISIG - 1) * DATA_BYTES_PER_PUBKEY - ORDIKNOT_PREFIX.len();

/// Maximum data per single P2WSH input (same as MAX_DATA_CAPACITY)
const MAX_DATA_PER_INPUT: usize = MAX_DATA_CAPACITY;

/// Maximum standard transaction weight (Bitcoin Core policy)
const MAX_STANDARD_TX_WEIGHT: u64 = 400_000;

/// Bytes per weight unit
const BYTES_PER_WEIGHT_UNIT: u64 = 4;

/// Dust threshold for P2WSH outputs (in sats)
const DUST_THRESHOLD: u64 = 546;

/// Calculates the number of inputs needed for given data length
fn calculate_inputs_needed(data_len: usize) -> usize {
    if data_len == 0 {
        return 1;
    }
    // First input has ORDIKNOT_PREFIX, subsequent inputs don't
    // First input: MAX_DATA_PER_INPUT bytes (including prefix)
    // Subsequent inputs: MAX_DATA_PER_INPUT + ORDIKNOT_PREFIX.len() bytes each
    if data_len <= MAX_DATA_PER_INPUT {
        1
    } else {
        let remaining_after_first = data_len.saturating_sub(MAX_DATA_PER_INPUT);
        let bytes_per_additional_input = MAX_DATA_PER_INPUT + ORDIKNOT_PREFIX.len();
        1 + remaining_after_first.div_ceil(bytes_per_additional_input)
    }
}

/// Splits data into chunks for multiple inputs
/// First chunk includes ORDIKNOT_PREFIX, subsequent chunks are raw data
fn split_data_for_inputs(data: &[u8]) -> Vec<Vec<u8>> {
    if data.len() <= MAX_DATA_PER_INPUT {
        // Single chunk with prefix
        let mut chunk = Vec::with_capacity(ORDIKNOT_PREFIX.len() + data.len());
        chunk.extend_from_slice(ORDIKNOT_PREFIX);
        chunk.extend_from_slice(data);
        return vec![chunk];
    }

    let mut chunks = Vec::new();

    // First chunk: prefix + data
    let mut first_chunk = Vec::with_capacity(MAX_DATA_PER_INPUT + ORDIKNOT_PREFIX.len());
    first_chunk.extend_from_slice(ORDIKNOT_PREFIX);
    let first_data_len = MAX_DATA_PER_INPUT;
    first_chunk.extend_from_slice(&data[..first_data_len]);
    chunks.push(first_chunk);

    // Remaining chunks: raw data only
    let mut offset = first_data_len;
    let bytes_per_chunk = MAX_DATA_PER_INPUT + ORDIKNOT_PREFIX.len();

    while offset < data.len() {
        let end = std::cmp::min(offset + bytes_per_chunk, data.len());
        chunks.push(data[offset..end].to_vec());
        offset = end;
    }

    chunks
}

/// Validates that transaction weight doesn't exceed policy limit
fn validate_transaction_weight(tx: &Transaction) -> Result<()> {
    let weight = tx.weight().to_wu();
    if weight > MAX_STANDARD_TX_WEIGHT {
        anyhow::bail!(
            "Transaction weight {} exceeds maximum {} (size limit: ~{} KB)",
            weight,
            MAX_STANDARD_TX_WEIGHT,
            MAX_STANDARD_TX_WEIGHT / BYTES_PER_WEIGHT_UNIT / 1024
        );
    }
    Ok(())
}

/// Encodes data using P2WSH CHECKMULTISIG witness script
pub fn encode(
    data: &[u8],
    client: &Client,
    fee_rate: u64,
) -> Result<(Vec<Transaction>, bitcoin::Txid)> {
    // Calculate number of inputs needed
    let inputs_needed = calculate_inputs_needed(data.len());
    println!(
        "Data size: {} bytes, requires {} input(s)",
        data.len(),
        inputs_needed
    );

    // Split data into chunks (first chunk has prefix, rest are raw)
    let data_chunks = split_data_for_inputs(data);

    // Generate real keypair for signing (reuse for all inputs)
    let secp = Secp256k1Context::new();
    let (real_secret_key, real_pubkey) = generate_real_keypair(&secp)?;

    // For each chunk, create witnessscript and P2WSH address
    let mut witnessscripts = Vec::new();
    let mut p2wsh_addresses = Vec::new();

    for chunk in data_chunks.iter() {
        // Encode chunk as fake pubkeys
        let fake_pubkeys = encode_data_as_fake_pubkeys(chunk)?;

        // Build witnessScript (1-of-N multisig)
        let witnessscript = build_multisig_witnessscript(&real_pubkey, &fake_pubkeys)?;
        let p2wsh_address = bitcoin::Address::p2wsh(&witnessscript, Network::Regtest);

        witnessscripts.push(witnessscript);
        p2wsh_addresses.push(p2wsh_address);
    }

    // Create funding transaction with multiple P2WSH outputs
    let (funding_tx, output_amounts) = create_funding_transaction(
        client,
        &p2wsh_addresses,
        &witnessscripts,
        &real_secret_key,
        fee_rate,
    )?;
    let funding_txid = funding_tx.compute_txid();

    // Create spending transaction with multiple inputs
    let spending_tx = create_spending_transaction(
        client,
        funding_txid,
        &real_secret_key,
        &witnessscripts,
        &output_amounts,
        fee_rate,
    )?;

    // Validate transaction weight
    validate_transaction_weight(&spending_tx)?;

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

/// Creates a funding transaction that pays to multiple P2WSH addresses
/// Returns (transaction, vec of output amounts)
fn create_funding_transaction(
    client: &Client,
    p2wsh_addresses: &[bitcoin::Address],
    witnessscripts: &[ScriptBuf],
    secret_key: &SecretKey,
    fee_rate: u64,
) -> Result<(Transaction, Vec<u64>)> {
    let change_address = wallet::get_new_address(client)?;

    // Start with all outputs at dust threshold
    let mut output_amounts = vec![DUST_THRESHOLD; p2wsh_addresses.len()];

    // Create initial P2WSH outputs
    let p2wsh_outputs: Vec<TxOut> = p2wsh_addresses
        .iter()
        .map(|addr| TxOut {
            value: Amount::from_sat(DUST_THRESHOLD),
            script_pubkey: addr.script_pubkey(),
        })
        .collect();

    let mut total_funding: u64 = output_amounts.iter().sum();

    // Make rough estimate for initial UTXO selection (~100 vB per input, ~43 bytes per output)
    let estimated_fee = fee_rate * 100; // Conservative estimate for 1 input
    let target_amount = total_funding + estimated_fee;

    // Select UTXOs to cover target amount
    let (selected_utxos, total_input_amount) =
        wallet::select_utxos_by_amount(client, target_amount)?;

    // Create inputs from selected UTXOs
    let inputs: Vec<TxIn> = selected_utxos
        .iter()
        .map(|utxo| {
            tx_utils::create_tx_input(bitcoin::OutPoint {
                txid: utxo.txid,
                vout: utxo.vout,
            })
        })
        .collect();

    // Build temporary transaction with placeholder change to measure actual size
    let mut temp_outputs = p2wsh_outputs.clone();
    temp_outputs.push(TxOut {
        value: Amount::from_sat(total_input_amount), // Placeholder
        script_pubkey: change_address.script_pubkey(),
    });

    let temp_tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: inputs.clone(),
        output: temp_outputs,
    };

    // Sign to get actual witness data and measure real weight
    let signed_temp_tx = wallet::sign_transaction(client, &temp_tx)?;
    let actual_weight = signed_temp_tx.weight().to_wu();
    let fee = tx_utils::calculate_fee_from_weight(actual_weight, fee_rate);

    println!(
        "Selected {} UTXO(s) with total {} sats, actual fee: {} sats ({} WU)",
        inputs.len(),
        total_input_amount,
        fee,
        actual_weight
    );

    // Now build a temporary spending transaction to measure its actual fee requirement
    let temp_spending_inputs: Vec<TxIn> = (0..witnessscripts.len())
        .map(|vout| TxIn {
            previous_output: bitcoin::OutPoint {
                txid: bitcoin::Txid::all_zeros(), // Dummy txid for measurement
                vout: vout as u32,
            },
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::Sequence::MAX,
            witness: Witness::new(),
        })
        .collect();

    let temp_spending_output = TxOut {
        value: Amount::from_sat(total_funding), // Placeholder
        script_pubkey: change_address.script_pubkey(),
    };

    let mut temp_spending_tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: temp_spending_inputs,
        output: vec![temp_spending_output],
    };

    // Sign to get actual witness data and measure spending transaction weight
    sign_p2wsh_multisig(&mut temp_spending_tx, secret_key, witnessscripts, &output_amounts)?;
    let spending_weight = temp_spending_tx.weight().to_wu();
    let spending_fee = tx_utils::calculate_fee_from_weight(spending_weight, fee_rate);

    println!(
        "Spending tx weight: {} WU, fee: {} sats",
        spending_weight, spending_fee
    );

    // Add spending fee to first output
    output_amounts[0] += spending_fee;
    total_funding += spending_fee;

    // Rebuild P2WSH outputs with updated first amount
    let final_p2wsh_outputs: Vec<TxOut> = p2wsh_addresses
        .iter()
        .enumerate()
        .map(|(i, addr)| TxOut {
            value: Amount::from_sat(output_amounts[i]),
            script_pubkey: addr.script_pubkey(),
        })
        .collect();

    // Calculate change amount with actual funding fee
    let total_out = total_funding + fee;
    let change_amount = total_input_amount.checked_sub(total_out).context(format!(
        "Insufficient funds for funding transaction (requires {} sats, have {} sats)",
        total_out, total_input_amount
    ))?;

    // Build final transaction with correct amounts
    let mut outputs = final_p2wsh_outputs;
    outputs.push(TxOut {
        value: Amount::from_sat(change_amount),
        script_pubkey: change_address.script_pubkey(),
    });

    let tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: inputs,
        output: outputs,
    };

    let signed_tx = wallet::sign_transaction(client, &tx)?;
    Ok((signed_tx, output_amounts))
}

/// Creates a spending transaction that spends multiple P2WSH outputs
fn create_spending_transaction(
    client: &Client,
    funding_txid: bitcoin::Txid,
    secret_key: &SecretKey,
    witnessscripts: &[ScriptBuf],
    output_amounts: &[u64],
    fee_rate: u64,
) -> Result<Transaction> {
    let change_address = wallet::get_new_address(client)?;

    // Create one input per P2WSH output from funding transaction
    let inputs: Vec<TxIn> = (0..witnessscripts.len())
        .map(|vout| TxIn {
            previous_output: bitcoin::OutPoint {
                txid: funding_txid,
                vout: vout as u32,
            },
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::Sequence::MAX,
            witness: Witness::new(),
        })
        .collect();

    // Calculate total input amount from the funding transaction outputs
    let total_input_amount: u64 = output_amounts.iter().sum();

    // Create a temporary transaction to estimate the weight
    let temp_output = TxOut {
        value: Amount::from_sat(total_input_amount),
        script_pubkey: change_address.script_pubkey(),
    };

    let mut temp_tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: inputs.clone(),
        output: vec![temp_output],
    };

    // Sign to get accurate weight
    sign_p2wsh_multisig(&mut temp_tx, secret_key, witnessscripts, output_amounts)?;

    // Calculate fee based on actual weight
    let tx_weight = temp_tx.weight().to_wu();
    let fee = tx_utils::calculate_fee_from_weight(tx_weight, fee_rate);

    println!(
        "Transaction weight: {} WU, calculated fee: {} sats",
        tx_weight, fee
    );

    // Calculate change amount with proper fee
    let change_amount = total_input_amount.checked_sub(fee).context(format!(
        "Insufficient funds for spending transaction (requires {} sats, current balance: {} sats)",
        fee, total_input_amount
    ))?;

    // Create final transaction with correct output amount
    let output = TxOut {
        value: Amount::from_sat(change_amount),
        script_pubkey: change_address.script_pubkey(),
    };

    let mut tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: inputs,
        output: vec![output],
    };

    // Sign the final transaction
    sign_p2wsh_multisig(&mut tx, secret_key, witnessscripts, output_amounts)?;

    Ok(tx)
}

/// Signs all inputs in a P2WSH multisig transaction
fn sign_p2wsh_multisig(
    tx: &mut Transaction,
    secret_key: &SecretKey,
    witnessscripts: &[ScriptBuf],
    output_amounts: &[u64],
) -> Result<()> {
    let secp = Secp256k1Context::new();

    // Sign each input with its corresponding witnessscript
    for (input_index, witnessscript) in witnessscripts.iter().enumerate() {
        let mut sighash_cache = SighashCache::new(tx.clone());

        let sighash = sighash_cache
            .p2wsh_signature_hash(
                input_index,
                witnessscript,
                Amount::from_sat(output_amounts[input_index]),
                EcdsaSighashType::All,
            )
            .context(format!(
                "Failed to compute P2WSH sighash for input {}",
                input_index
            ))?;

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

        tx.input[input_index].witness = witness;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_inputs_needed() {
        // Empty data
        assert_eq!(calculate_inputs_needed(0), 1);

        // Small data (fits in one input with prefix)
        assert_eq!(calculate_inputs_needed(100), 1);
        assert_eq!(calculate_inputs_needed(605), 1);

        // Just over one input
        assert_eq!(calculate_inputs_needed(606), 2);

        // Multiple inputs
        // First input: 605 bytes (with prefix)
        // Second input: 605 + 3 = 608 bytes (no prefix needed)
        assert_eq!(calculate_inputs_needed(605 + 608), 2);
        assert_eq!(calculate_inputs_needed(605 + 609), 3);

        // Large data
        assert_eq!(calculate_inputs_needed(2000), 4);
    }

    #[test]
    fn test_split_data_for_inputs_single_chunk() {
        let data = vec![0xAB; 100];
        let chunks = split_data_for_inputs(&data);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 100 + ORDIKNOT_PREFIX.len());
        assert_eq!(&chunks[0][..ORDIKNOT_PREFIX.len()], ORDIKNOT_PREFIX);
        assert_eq!(&chunks[0][ORDIKNOT_PREFIX.len()..], &data[..]);
    }

    #[test]
    fn test_split_data_for_inputs_multiple_chunks() {
        // 1500 bytes should split into 3 chunks
        let data = vec![0xAB; 1500];
        let chunks = split_data_for_inputs(&data);

        assert_eq!(chunks.len(), 3);

        // First chunk: prefix + 605 bytes
        assert_eq!(chunks[0].len(), 605 + ORDIKNOT_PREFIX.len());
        assert_eq!(&chunks[0][..ORDIKNOT_PREFIX.len()], ORDIKNOT_PREFIX);

        // Second chunk: 608 bytes (no prefix)
        assert_eq!(chunks[1].len(), 608);

        // Third chunk: remaining bytes
        assert_eq!(chunks[2].len(), 1500 - 605 - 608);

        // Verify data integrity (excluding prefix)
        let mut reconstructed = Vec::new();
        reconstructed.extend_from_slice(&chunks[0][ORDIKNOT_PREFIX.len()..]);
        reconstructed.extend_from_slice(&chunks[1]);
        reconstructed.extend_from_slice(&chunks[2]);

        assert_eq!(reconstructed, data);
    }

    #[test]
    fn test_split_data_at_boundary() {
        // Exactly 605 bytes should be a single chunk
        let data = vec![0xCD; 605];
        let chunks = split_data_for_inputs(&data);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 605 + ORDIKNOT_PREFIX.len());

        // 606 bytes should split into 2 chunks
        let data = vec![0xCD; 606];
        let chunks = split_data_for_inputs(&data);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 605 + ORDIKNOT_PREFIX.len());
        assert_eq!(chunks[1].len(), 1);
    }
}
