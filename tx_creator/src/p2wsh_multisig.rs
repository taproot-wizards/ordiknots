use anyhow::{Context, Result};
use bitcoin::script::PushBytesBuf;
use bitcoin::secp256k1::{Message, Secp256k1 as Secp256k1Context, SecretKey};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{
    absolute, opcodes, script, transaction, Amount, Network, PublicKey, ScriptBuf, Transaction,
    TxIn, TxOut, Witness,
};
use bitcoin_hashes::Hash;
use bitcoincore_rpc::{Client, RpcApi};

/// Standard transaction fee in satoshis
const TX_FEE_SATS: u64 = 1000;

/// Maximum pubkeys in CHECKMULTISIG (Bitcoin consensus)
const MAX_PUBKEYS_PER_MULTISIG: usize = 20;

/// Bytes per fake pubkey after prefix (compressed pubkey format)
const DATA_BYTES_PER_PUBKEY: usize = 32;

/// Maximum data capacity (19 fake pubkeys, 1 real pubkey for signing)
const MAX_DATA_CAPACITY: usize = (MAX_PUBKEYS_PER_MULTISIG - 1) * DATA_BYTES_PER_PUBKEY; // 608 bytes

/// 1. Embed data in 19 "fake" pubkeys (0x02/0x03 + 32 bytes arbitrary data)
/// 2. Create 1 real pubkey for actual signing
/// 3. Build witnessScript: OP_1 <real_pk> <fake_pk1> ... <fake_pk19> OP_20 OP_CHECKMULTISIG
/// 4. Create P2WSH output with hash of witnessScript
/// 5. Spend with 1 signature + witnessScript
pub fn create_p2wsh_multisig_tx(rpc: &Client, data: &[u8], broadcast: bool) -> Result<String> {
    println!("\n=== P2WSH CHECKMULTISIG Data Embedding ===");
    println!("File size: {} bytes", data.len());

    if data.len() > MAX_DATA_CAPACITY {
        anyhow::bail!(
            "Data too large: {} bytes (max {} bytes)",
            data.len(),
            MAX_DATA_CAPACITY
        );
    }

    // Step 1: Generate real keypair for signing
    let secp = Secp256k1Context::new();
    let (real_secret_key, real_pubkey) = generate_real_keypair(&secp)?;
    println!("Generated real keypair for signing");

    // Step 2: Encode data as fake pubkeys
    let fake_pubkeys = encode_data_as_fake_pubkeys(data)?;
    println!("Encoded data into {} fake pubkeys", fake_pubkeys.len());
    println!(
        "Total capacity used: {} / {} bytes",
        data.len(),
        MAX_DATA_CAPACITY
    );

    // Step 3: Build witnessScript (1-of-N multisig)
    let witnessscript = build_multisig_witnessscript(&real_pubkey, &fake_pubkeys)?;
    println!("WitnessScript size: {} bytes", witnessscript.len());

    // Step 4: Create P2WSH address
    let p2wsh_address = bitcoin::Address::p2wsh(&witnessscript, Network::Regtest);
    println!("P2WSH address: {}", p2wsh_address);

    // Step 5: Create funding transaction to P2WSH address
    let funding_amount = 100_000; // 100k sats
    let funding_tx = create_funding_transaction(rpc, &p2wsh_address, funding_amount)?;
    let funding_txid = if broadcast {
        rpc.send_raw_transaction(&funding_tx)
            .context("Failed to broadcast funding transaction")?
    } else {
        funding_tx.compute_txid()
    };
    println!("\nFunding transaction: {}", funding_txid);

    // Step 6: Create spending transaction with witness data
    let spending_tx = create_spending_transaction(
        rpc,
        funding_txid,
        funding_amount,
        &real_secret_key,
        &witnessscript,
    )?;

    let spending_txid = spending_tx.compute_txid();
    let tx_size = bitcoin::consensus::encode::serialize(&spending_tx).len();
    let tx_weight = spending_tx.weight();

    println!("\n=== Transaction Details ===");
    println!("Spending transaction: {}", spending_txid);
    println!("Size: {} bytes", tx_size);
    println!("Weight: {} WU", tx_weight);
    println!("WitnessScript size: {} bytes", witnessscript.len());

    if broadcast {
        println!("\nBroadcasting transaction...");
        match rpc.send_raw_transaction(&spending_tx) {
            Ok(broadcast_txid) => {
                println!("✓ Transaction accepted by Bitcoin Knots!");
                println!("TXID: {}", broadcast_txid);
            }
            Err(e) => {
                println!("✗ Transaction rejected by Bitcoin Knots!");
                println!("Error: {}", e);
                anyhow::bail!("Transaction rejected: {}", e);
            }
        }
    }

    Ok(spending_txid.to_string())
}

/// Each chunk of 32 bytes becomes a "pubkey": 0x02 or 0x03 prefix + 32 bytes data
/// The prefix is chosen based on the first bit of the data to minimize data loss
fn encode_data_as_fake_pubkeys(data: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut fake_pubkeys = Vec::new();

    for chunk in data.chunks(DATA_BYTES_PER_PUBKEY) {
        let mut fake_pubkey = Vec::with_capacity(33);

        // Choose prefix based on first bit of data (to encode 1 bit of info in prefix)
        let prefix = if !chunk.is_empty() && (chunk[0] & 0x80) != 0 {
            0x03 // High bit set
        } else {
            0x02 // High bit clear
        };
        fake_pubkey.push(prefix);

        // Add the data bytes
        fake_pubkey.extend_from_slice(chunk);

        // Pad to 32 bytes if this is the last chunk and it's short
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
    // Generate a random secret key
    let secret_key = SecretKey::new(&mut rand::thread_rng());
    let secp_pubkey = bitcoin::secp256k1::PublicKey::from_secret_key(secp, &secret_key);
    let public_key = PublicKey::new(secp_pubkey);

    Ok((secret_key, public_key))
}

/// Builds a 1-of-N multisig witnessScript
///
/// Format: OP_1 <real_pk> <fake_pk1> ... <fake_pkN> OP_(N+1) OP_CHECKMULTISIG
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
            // For 17-20, we need to use OP_PUSH with the number
            builder = builder.push_int(n as i64);
            opcodes::all::OP_CHECKMULTISIG
        }
        _ => anyhow::bail!("Invalid number of pubkeys: {}", total_pubkeys),
    };

    if total_pubkeys <= 16 {
        builder = builder.push_opcode(n_opcode);
    }

    // OP_CHECKMULTISIG
    builder = builder.push_opcode(opcodes::all::OP_CHECKMULTISIG);

    Ok(builder.into_script())
}

/// Creates a funding transaction that pays to a P2WSH address
fn create_funding_transaction(
    rpc: &Client,
    p2wsh_address: &bitcoin::Address,
    funding_amount: u64,
) -> Result<Transaction> {
    let change_address = get_regtest_address(rpc)?;
    let utxo = select_largest_utxo(rpc)?;

    let input = TxIn {
        previous_output: bitcoin::OutPoint {
            txid: utxo.txid,
            vout: utxo.vout,
        },
        script_sig: ScriptBuf::new(),
        sequence: bitcoin::Sequence::MAX,
        witness: Witness::new(),
    };

    // Calculate amounts
    let total_out = funding_amount + TX_FEE_SATS;
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

    // Sign with wallet
    sign_transaction(rpc, tx)
}

/// Creates a spending transaction that spends the P2WSH output
fn create_spending_transaction(
    rpc: &Client,
    funding_txid: bitcoin::Txid,
    input_amount: u64,
    secret_key: &SecretKey,
    witnessscript: &ScriptBuf,
) -> Result<Transaction> {
    let change_address = get_regtest_address(rpc)?;

    let input = TxIn {
        previous_output: bitcoin::OutPoint {
            txid: funding_txid,
            vout: 0, // P2WSH output is first
        },
        script_sig: ScriptBuf::new(),
        sequence: bitcoin::Sequence::MAX,
        witness: Witness::new(), // Will be filled after signing
    };

    // Calculate change
    let change_amount = input_amount
        .checked_sub(TX_FEE_SATS)
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

    // Sign the transaction
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

    // Create sighash cache
    let mut sighash_cache = SighashCache::new(tx.clone());

    // Compute sighash for P2WSH
    let sighash = sighash_cache
        .p2wsh_signature_hash(
            0,
            witnessscript,
            Amount::from_sat(input_amount),
            EcdsaSighashType::All,
        )
        .context("Failed to compute P2WSH sighash")?;

    // Sign
    let message = Message::from_digest_slice(sighash.as_byte_array())
        .context("Failed to create message from sighash")?;
    let signature = secp.sign_ecdsa(&message, secret_key);

    // Create signature with sighash type
    let mut sig_bytes = signature.serialize_der().to_vec();
    sig_bytes.push(EcdsaSighashType::All.to_u32() as u8);

    // Build witness: [OP_0, signature, witnessScript]
    // OP_0 is needed due to a bug in CHECKMULTISIG that pops an extra element
    let mut witness = Witness::new();
    witness.push([]); // OP_0 (empty vector)
    witness.push(sig_bytes);
    witness.push(witnessscript.as_bytes());

    tx.input[0].witness = witness;

    Ok(())
}

/// Signs a transaction using the wallet
fn sign_transaction(rpc: &Client, tx: Transaction) -> Result<Transaction> {
    let signed_tx = rpc
        .sign_raw_transaction_with_wallet(&tx, None, None)
        .context("Failed to sign transaction")?;

    if !signed_tx.complete {
        anyhow::bail!("Transaction signing incomplete");
    }

    signed_tx
        .transaction()
        .context("Failed to get signed transaction")
}

/// Gets a new address from the wallet
fn get_regtest_address(rpc: &Client) -> Result<bitcoin::Address> {
    rpc.get_new_address(None, None)
        .context("Failed to get new address")?
        .require_network(Network::Regtest)
        .context("Address is not on regtest network")
}

/// Selects the largest UTXO from the wallet
fn select_largest_utxo(rpc: &Client) -> Result<bitcoincore_rpc::json::ListUnspentResultEntry> {
    let mut unspent = rpc
        .list_unspent(None, None, None, None, None)
        .context("Failed to list unspent outputs")?;

    if unspent.is_empty() {
        anyhow::bail!("No unspent outputs available. Generate some blocks first.");
    }

    unspent.sort_by(|a, b| b.amount.cmp(&a.amount));
    Ok(unspent.into_iter().next().unwrap())
}
