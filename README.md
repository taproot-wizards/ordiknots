# Ordiknots 🧙‍♂️

magic transactions with arbitrary data, accepted by Bitcoin Knots.

## Setup

To test any of these techniques, you first need to run a Bitcoin Knots node:

```bash
just knots
```

This will spin up an isolated regtest node in a Docker container.

## Data Embedding Techniques

### 1. Chained OP_RETURN

- Max file size: ~10 KB (255 chunks × 40 bytes)
- Requires multiple chained transactions

```bash
# Encode:
just encode {path/to/file.png} --type=chained-op-return

# Decode:
just decode {first-tx-id} ./decoded_image.png --type=chained-op-return
```

Files are split into 40-byte chunks across chained transactions.
Each transaction spends the continuation output from the previous, creating a blockchain-native linked list:

- **vout 0**: OP_RETURN (80 bytes: `IMG` + index + total + 75 bytes data)
- **vout 1**: Continuation output (~9000 sats) spent by next transaction
- **vout 2**: Change (first tx only)

```
TX1 (wallet UTXO) -> [OP_RETURN chunk 0] [continuation 10000 sats] [change]
                              ↓
TX2 (spends vout1) -> [OP_RETURN chunk 1] [continuation 9000 sats]
                              ↓
TX3 (spends vout1) -> [OP_RETURN chunk 2]
```

The decoder follows the tx chain recursively from the first TXID.

### 2. P2WSH CHECKMULTISIG

- Max file size: 608 bytes (single tx)

```bash
# Encode:
just encode {path/to/file.png} --type=pw2sh-fake-multisig

# Decode:
just decode {reveal-tx-id} ./decoded_image.png --type=pw2sh-fake-multisig
```

Embeds data in a single P2WSH transaction using fake pubkeys in a CHECKMULTISIG script:

1. **Generate real keypair** for signing (1 legitimate pubkey)
2. **Encode data as fake pubkeys**: Split data into 32-byte chunks, prefix each with 0x02/0x03
3. **Build 1-of-20 multisig**: `OP_1 <real_pk> <fake_pk1> ... <fake_pk19> OP_20 OP_CHECKMULTISIG`
4. **Create P2WSH output**: Hash the witnessScript to create P2WSH address
5. **Spend with witness**: `[OP_0, signature, witnessScript]`
   - OP_0 required due to CHECKMULTISIG stack bug
   - Only the real pubkey is cryptographically validated

**Data extraction**: Decoder parses witnessScript from spending transaction, extracts fake pubkeys (skip first byte prefix), concatenates 32-byte data chunks.

## Contributing

If you find more ways to encode arbitrary data in a way that gets relayed by Bitcoin Knots:

1. Create a new technique with custom encoding/decoding in `src/techniques`
2. Add an integration test for it in `tests/integration_test.rs`
3. Document it in `README.md`
