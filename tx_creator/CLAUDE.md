# tx_creator

Rust CLI tool for creating Bitcoin transactions with OP_RETURN data. Supports text messages and binary file encoding via chained transactions.

## Bitcoin Knots Compatibility

This tool is designed to work with **Bitcoin Knots' stricter OP_RETURN policies**:

1. **Size limit**: Knots allows only 42 bytes for OP_RETURN scriptPubKey (vs 83 bytes in Bitcoin Core)
   - After accounting for `OP_RETURN` opcode (1 byte) and push opcode (1 byte), we have **40 bytes** for actual data
   - Our chunks: first chunk has 33 bytes data (5 bytes metadata + 2 bytes file size), subsequent chunks have 35 bytes data (5 bytes metadata)

2. **Bare data carrier rejection**: Knots rejects transactions with only OP_RETURN outputs and no "monetary" outputs
   - Solution: All transactions include a change/continuation output (~9000 sats) that counts as monetary
   - This includes the final transaction in a chain, which now has a change output instead of just OP_RETURN

These constraints reduce the data capacity per chunk but ensure compatibility with Knots' default settings without requiring configuration changes.

## How It Works

### Text Messages
Single transaction with OP_RETURN output containing UTF-8 encoded message.

### File Encoding
Files split into 75-byte chunks across chained transactions. Each transaction:
- **vout 0**: OP_RETURN (80 bytes: `IMG` + index + total + 75 bytes data)
- **vout 1**: Continuation output (~9000 sats) spent by next transaction
- **vout 2**: Change (first tx only)

**Chain Flow:**
```
TX1 (wallet UTXO) -> [OP_RETURN chunk 0] [continuation 10000 sats] [change]
                              ↓
TX2 (spends vout1) -> [OP_RETURN chunk 1] [continuation 9000 sats]
                              ↓
TX3 (spends vout1) -> [OP_RETURN chunk 2]
```

Each transaction spends the continuation output from the previous, creating a blockchain-native linked list. Decoder follows chain recursively from first TXID.

## Usage

### Encode text message
```bash
just create_tx "🚀 Hello Bitcoin!"
```

### Encode file
```bash
just encode_file punk.png
```

Returns first TXID for decoding.

### Decode file
```bash
just decode_image <first_txid> output.png
```

## Key Details

- Standard-compliant: 80-byte OP_RETURN limit
- Max file size: ~19 KB (255 chunks × 75 bytes)
- Initial continuation: 10,000 sats
- Uses `bitcoin` and `bitcoincore-rpc` crates
