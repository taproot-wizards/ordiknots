use bitcoin::Transaction;

use super::decode::extract_op_return_data;
use crate::techniques::ORDIKNOT_PREFIX;

/// Detects if a transaction contains chained OP_RETURN data
/// Only detects the first chunk (index 0) to avoid treating every chunk in the chain as a new knotwork
pub fn detect(tx: &Transaction) -> bool {
    for output in &tx.output {
        if output.script_pubkey.is_op_return() {
            if let Some(data) = extract_op_return_data(&output.script_pubkey) {
                // Check for "444" prefix and that it's the first chunk (index 0)
                // Format: "444" + index + total_chunks + [file_size if index==0] + data
                if data.len() >= 5 && &data[0..3] == ORDIKNOT_PREFIX && data[3] == 0 {
                    return true;
                }
            }
        }
    }
    false
}
