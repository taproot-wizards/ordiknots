use bitcoin::Transaction;

use crate::techniques::ORDIKNOT_PREFIX;

/// Detects if a transaction contains P2WSH fake multisig data
/// Checks for witness data and looks for "444" prefix in the witnessScript
pub fn detect(tx: &Transaction) -> bool {
    if tx.input.is_empty() {
        return false;
    }

    // Check first input has witness data (P2WSH has at least 3 elements)
    let witness = &tx.input[0].witness;
    if witness.len() < 3 {
        return false;
    }

    // Get witnessScript (last element)
    let Some(witnessscript_bytes) = witness.last() else {
        return false;
    };

    // Look for "444" prefix somewhere in the witnessScript
    // The prefix should appear after pubkey data (which starts with 0x02 or 0x03)
    // We just search for the byte pattern b"444" in the script
    if witnessscript_bytes.len() < ORDIKNOT_PREFIX.len() {
        return false;
    }

    // Search for "444" in the witnessScript
    witnessscript_bytes
        .windows(ORDIKNOT_PREFIX.len())
        .any(|window| window == ORDIKNOT_PREFIX)
}
