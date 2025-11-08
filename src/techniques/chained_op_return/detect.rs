use bitcoin::Transaction;

use super::decode::extract_op_return_data;

/// Detects if a transaction contains chained OP_RETURN data
pub fn detect(tx: &Transaction) -> bool {
    for output in &tx.output {
        if output.script_pubkey.is_op_return() {
            if let Some(data) = extract_op_return_data(&output.script_pubkey) {
                // Check for "444" prefix
                if data.len() >= 3 && &data[0..3] == b"444" {
                    return true;
                }
            }
        }
    }
    false
}
