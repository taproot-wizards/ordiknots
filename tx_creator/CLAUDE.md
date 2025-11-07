# tx_creator

A Rust CLI tool for creating Bitcoin transactions with emoji OP_RETURN data on regtest.

## How It Works

1. **Connects to Bitcoin Core RPC** at `http://localhost:18443` with credentials `mempool:mempool`
2. **Finds a spendable UTXO** using `listunspent`
3. **Builds transaction** with two outputs:
   - OP_RETURN output with your message/emoji as UTF-8 bytes (0 sats)
   - Change output returning funds minus 1000 sat fee
4. **Signs and broadcasts** the transaction

## Usage

With justfile from parent directory:
```bash
just create_tx "🚀 Hello Bitcoin!"
```

Direct usage:
```bash
cargo run -- --message "🚀 Hello!" --broadcast
```

## Key Details

- Uses `bitcoin` and `bitcoincore-rpc` Rust crates
- Fixed 1000 sat fee
- OP_RETURN encoded as UTF-8 via `PushBytesBuf`
