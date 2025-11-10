# Ordiknots 🧙‍♂️

magic transactions with arbitrary data, powered by Bitcoin Knots 🚀

## Setup

To test any of these techniques, you first need to run a Bitcoin Knots node:

```bash
just knots
```

This will spin up an isolated regtest node in a Docker container.

## Data Embedding Techniques

### 1. Chained OP_RETURN

- Max file size: ~1 KB (25 chunks × 40 bytes)
- Requires multiple chained transactions

```bash
# Encode:
just encode {path/to/file.png} --type=chained-op-return

# Decode:
just decode {first-tx-id} ./decoded_image.png
```

Files are split into 40-byte chunks across chained transactions.
Each transaction spends the continuation output from the previous, creating a blockchain-native linked list:

- **vout 0**: OP_RETURN (40 bytes: `444` + index + total + 35 bytes data)
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

- Max file size: ~242 KB theoretical max (limited by 400k WU tx weight)
- Max per input: 605 bytes (19 fake pubkeys × 32 bytes - 3 byte prefix)

```bash
# Encode:
just encode {path/to/file.png} --type=pw2sh-fake-multisig

# Decode:
just decode {reveal-tx-id} ./decoded_image.png
```

Embeds data in P2WSH witness using fake pubkeys in a CHECKMULTISIG script:

1. **Generate real keypair** for signing
2. **Encode data as fake pubkeys**: Split data into 32-byte chunks, prefix each with 0x02/0x03
3. **Build 1-of-N multisig**: `OP_1 <real_pk> <fake_pk1> ... <fake_pkN> OP_N OP_CHECKMULTISIG`
4. **Create P2WSH outputs**: Hash each witnessScript to create P2WSH addresses (one per 605-byte chunk)
5. **Spend with witnesses**: Transaction with multiple inputs, each with `[OP_0, signature, witnessScript]`

Files >605 bytes automatically get split across multiple P2WSH inputs.

## Knotworks

A **knotwork** is any data embedded on the Bitcoin blockchain using one of the techniques above. All knotworks are identified by the `444` prefix in their encoded data.

Scan the entire blockchain to discover all knotworks:

```bash
just index
```

## Contributing

If you find more ways to encode arbitrary data in a way that gets relayed by Bitcoin Knots:

1. Create a new technique with custom encoding/decoding in `src/techniques`
2. Add an integration test for it in `tests/integration_test.rs` (test it with `just test-integration`)
3. Document it in `README.md`
