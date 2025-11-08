use anyhow::{Context, Result};
use bitcoin::Txid;
use bitcoincore_rpc::{Client, RpcApi};

/// Decodes data from a P2WSH CHECKMULTISIG transaction
pub fn decode_from_blockchain(
    txid: &Txid,
    client: &Client,
) -> Result<Vec<u8>> {
    println!("Decoding P2WSH CHECKMULTISIG transaction: {}", txid);

    let tx = client
        .get_raw_transaction(txid, None)
        .context(format!("Failed to fetch transaction {}", txid))?;

    if tx.input.is_empty() {
        anyhow::bail!("Transaction has no inputs");
    }

    let witness = &tx.input[0].witness;
    if witness.is_empty() {
        anyhow::bail!("Transaction input has no witness data");
    }

    let witness_len = witness.len();
    if witness_len < 3 {
        anyhow::bail!(
            "Invalid witness: expected at least 3 elements (OP_0, sig, script), got {}",
            witness_len
        );
    }

    let witnessscript_bytes = witness.last().context("No witnessScript in witness")?;

    println!("WitnessScript size: {} bytes", witnessscript_bytes.len());

    let fake_pubkeys = extract_fake_pubkeys_from_script(witnessscript_bytes)?;

    println!("Extracted {} fake pubkeys", fake_pubkeys.len());

    let data = decode_fake_pubkeys(&fake_pubkeys)?;

    println!("\n✓ Successfully decoded {} bytes", data.len());

    Ok(data)
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
        if opcode >= 0x51 && opcode <= 0x60 {
            // OP_1 through OP_16
            break;
        }
        if opcode >= 0x01 && opcode <= 0x4b {
            // Direct push (1-75 bytes)
            pos += 1;
            if pos + (opcode as usize) > script_bytes.len() {
                anyhow::bail!("Invalid script: push extends beyond script");
            }
            let pubkey = script_bytes[pos..pos + (opcode as usize)].to_vec();
            pubkeys.push(pubkey);
            pos += opcode as usize;
        } else {
            anyhow::bail!("Unexpected opcode in script: 0x{:02x}", opcode);
        }
    }

    if pubkeys.is_empty() {
        anyhow::bail!("No pubkeys found in script");
    }

    // Remove the first pubkey (real signing key)
    pubkeys.remove(0);

    Ok(pubkeys)
}

/// Decodes fake pubkeys back into original data
fn decode_fake_pubkeys(fake_pubkeys: &[Vec<u8>]) -> Result<Vec<u8>> {
    let mut data = Vec::new();

    for (i, pubkey) in fake_pubkeys.iter().enumerate() {
        if pubkey.len() != 33 {
            anyhow::bail!(
                "Invalid fake pubkey at index {}: expected 33 bytes, got {}",
                i,
                pubkey.len()
            );
        }

        let prefix = pubkey[0];
        if prefix != 0x02 && prefix != 0x03 {
            anyhow::bail!(
                "Invalid fake pubkey prefix at index {}: expected 0x02 or 0x03, got 0x{:02x}",
                i,
                prefix
            );
        }

        // Extract the 32 data bytes
        data.extend_from_slice(&pubkey[1..33]);
    }

    // Remove trailing null padding
    while data.last() == Some(&0) && data.len() > 1 {
        data.pop();
    }

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_fake_pubkeys() {
        let fake_pubkeys = vec![
            vec![
                0x02, 0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
            vec![
                0x03, 0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
        ];

        let data = decode_fake_pubkeys(&fake_pubkeys).unwrap();

        assert_eq!(&data[0..4], &[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(&data[32..36], &[0xCA, 0xFE, 0xBA, 0xBE]);
    }
}
