use bitcoin::{
    absolute::LockTime, script::PushBytesBuf, transaction::Version, OutPoint, ScriptBuf, Sequence,
    Transaction, TxIn, TxOut, Witness,
};

/// Standard transaction fee in satoshis (for simple transactions)
pub const TX_FEE_SATS: u64 = 1000;

/// Fee rate in satoshis per virtual byte (vB)
/// 1 vB = 4 weight units (WU)
pub const FEE_RATE_SATS_PER_VB: u64 = 5;

/// Calculates the fee for a transaction based on its weight
/// Uses FEE_RATE_SATS_PER_VB to determine the fee
pub fn calculate_fee_from_weight(weight: u64) -> u64 {
    // Weight in WU / 4 = virtual bytes (vB)
    // Fee = vB × fee_rate
    let vbytes = (weight + 3) / 4; // Round up
    vbytes * FEE_RATE_SATS_PER_VB
}

pub fn create_tx_input(outpoint: OutPoint) -> TxIn {
    TxIn {
        previous_output: outpoint,
        script_sig: ScriptBuf::new(),
        sequence: Sequence::MAX,
        witness: Witness::new(),
    }
}

pub fn create_base_transaction(inputs: Vec<TxIn>, outputs: Vec<TxOut>) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: inputs,
        output: outputs,
    }
}

pub fn create_op_return_output(data: &[u8]) -> TxOut {
    let push_bytes = PushBytesBuf::try_from(data.to_vec()).expect("Data too large for OP_RETURN");

    TxOut {
        value: bitcoin::Amount::ZERO,
        script_pubkey: ScriptBuf::new_op_return(&push_bytes),
    }
}
