# bitcoin-regtest

## Get started

```bash
# Run regtest bitcoin:
just start

# Run mempool.space instance (optional)
just mempool
```

You can now visit your mempool instance on:
http://localhost:5000

You can also use the mempool API, e.g:
http://localhost:5000/api/tx/{txid}

List of Bitcoin CLI commands (to run with `just cli {xxx}`): 
https://thunderbiscuit.github.io/Learning-Bitcoin-from-the-Command-Line/03_2_Knowing_Your_Bitcoin_Setup.html#know-your-bitcoin-cli-commands

## Commands:

```bash
# Get block height (or run any bitcoin-cli command):
just cli getblockcount

# Mine new blocks:
just mine

# Get block info (for genesis block, i.e. block 0):
just block 0
```
