# tx_creator

Rust CLI tool for creating Bitcoin transactions with arbitrary data.

## Bitcoin Knots Compatibility

This tool is designed to work with **Bitcoin Knots' stricter OP_RETURN policies**:

1. **Size limit**: Knots allows only 42 bytes for OP_RETURN scriptPubKey (vs 83 bytes in Bitcoin Core)
   - After accounting for `OP_RETURN` opcode (1 byte) and push opcode (1 byte), we have **40 bytes** for actual data
   - Our chunks: first chunk has 33 bytes data (5 bytes metadata + 2 bytes file size), subsequent chunks have 35 bytes data (5 bytes metadata)

2. **Bare data carrier rejection**: Knots rejects transactions with only OP_RETURN outputs and no "monetary" outputs
   - Solution: All transactions include a change/continuation output (~9000 sats) that counts as monetary
   - This includes the final transaction in a chain, which now has a change output instead of just OP_RETURN

These constraints reduce the data capacity per chunk but ensure compatibility with Knots' default settings without requiring configuration changes.

## Data Embedding Techniques

### Chained OP_RETURN (Default Technique)

#### Text Messages
Single transaction with OP_RETURN output containing UTF-8 encoded message.

#### File Encoding
Files split into 40-byte chunks across chained transactions. Each transaction:
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

### P2WSH CHECKMULTISIG

This technique embeds data in a single P2WSH transaction using fake pubkeys in a CHECKMULTISIG script:

1. **Generate real keypair** for signing (1 legitimate pubkey)
2. **Encode data as fake pubkeys**: Split data into 32-byte chunks, prefix each with 0x02/0x03
3. **Build 1-of-20 multisig**: `OP_1 <real_pk> <fake_pk1> ... <fake_pk19> OP_20 OP_CHECKMULTISIG`
4. **Create P2WSH output**: Hash the witnessScript to create P2WSH address
5. **Spend with witness**: `[OP_0, signature, witnessScript]`
   - OP_0 required due to CHECKMULTISIG stack bug
   - Only the real pubkey is cryptographically validated
   - 19 fake pubkeys pass format check but never undergo EC validation

**Data extraction**: Decoder parses witnessScript from spending transaction, extracts fake pubkeys (skip first byte prefix), concatenates 32-byte data chunks.

## Usage

### Encode text message
```bash
just create_tx "🚀 Hello Bitcoin!"
```

### Encode file (chained OP_RETURN)
```bash
just encode punk.png
# or explicitly:
just encode punk.png chained-op-return
```

### Encode file (P2WSH CHECKMULTISIG)
```bash
just encode punk.png p2wsh-multisig
```

Returns TXID for decoding.

### Decode file
```bash
# Decode chained OP_RETURN (default):
just decode <first_txid> output.png

# Decode P2WSH CHECKMULTISIG:
just decode <txid> output.png p2wsh-multisig
```

## Key Details

**Chained OP_RETURN:**
- Max file size: ~10 KB (255 chunks × 40 bytes)
- Requires multiple chained transactions for files

**P2WSH CHECKMULTISIG:**
- Max file size: 608 bytes (single tx)
