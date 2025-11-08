use bitcoin::Transaction;

use super::decode::{decode_fake_pubkeys, extract_fake_pubkeys_from_script};
use crate::techniques::ORDIKNOT_PREFIX;

/// Detects if a transaction contains P2WSH fake multisig data
pub fn detect(tx: &Transaction) -> bool {
    if tx.input.is_empty() {
        return false;
    }

    let witness = &tx.input[0].witness;
    if witness.len() < 3 {
        return false;
    }

    // Extract witnessScript (last element)
    let Some(witnessscript_bytes) = witness.last() else {
        return false;
    };

    // Try to extract fake pubkeys
    let Ok(fake_pubkeys) = extract_fake_pubkeys_from_script(witnessscript_bytes) else {
        return false;
    };

    if fake_pubkeys.is_empty() {
        return false;
    }

    // Decode and check for "444" prefix
    let Ok(data) = decode_fake_pubkeys(&fake_pubkeys) else {
        return false;
    };

    data.len() >= ORDIKNOT_PREFIX.len() && &data[..ORDIKNOT_PREFIX.len()] == ORDIKNOT_PREFIX
}
