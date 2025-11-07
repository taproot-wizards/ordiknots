# CLAUDE.md

## Project Overview

This is a Bitcoin regtest environment for local Bitcoin development and testing. It provides:

1. **Bitcoin Core node** running in regtest mode via Docker
2. **Mempool.space** visualization interface (as a git submodule)

## Architecture

### Docker Setup

The main service is defined in `docker-compose.yml`:
- **bitcoind**: Bitcoin Core node running in regtest mode
  - Image: `ruimarinho/bitcoin-core`
  - RPC port: 18443 (exposed to host)
  - Data persistence: `./bitcoin-data` volume (gitignored)
  - RPC accessible from anywhere (development only)

### Mempool Submodule

Located at `mempool/` - this is the full Mempool.space project as a git submodule. It has its own Docker setup in `mempool/docker/` that can be run independently to visualize blockchain data.

## Common Commands

All commands are managed via the `justfile` (use with the `just` command):

### Bitcoin Node

```bash
# Start Bitcoin Core in regtest mode
just start

# Execute bitcoin-cli commands in the running container
# Examples:
just cli getblockchaininfo
just cli getnewaddress
just cli generatetoaddress 101 <address>
just cli getbalance
```

The `cli` command is a wrapper that runs `bitcoin-cli -regtest` inside the Docker container as the bitcoin user.

### Mempool Visualization

```bash
# Start Mempool.space interface
just mempool
```

This runs `docker compose up` in the `mempool/docker/` directory, which starts the Mempool.space stack (frontend, backend, database) configured to connect to your regtest node.

## Development Workflow

### Starting Development

1. Start Bitcoin Core: `just start`
2. Generate initial blocks (needed for mining rewards to mature):
   ```bash
   just cli getnewaddress
   just cli generatetoaddress 101 <address>
   ```
3. (Optional) Start Mempool interface: `just mempool`

### Testing Scenarios

Common regtest operations using `just cli`:

- **Mine blocks**: `generatetoaddress <nblocks> <address>`
- **Send transactions**: `sendtoaddress <address> <amount>`
- **Get UTXOs**: `listunspent`
- **Get mempool**: `getrawmempool`
- **Get block**: `getblock <blockhash>`

### Network Configuration

- RPC endpoint: `http://localhost:18443`
- Network: `regtest`
- RPC credentials: Default (no auth in development setup)

## Key Directories

- `bitcoin-data/`: Bitcoin Core data directory (gitignored, persists blockchain state)
- `mempool/`: Git submodule containing Mempool.space project
- `mempool/docker/`: Mempool Docker Compose setup
- `mempool/docker/backend/mempool-config.json`: Mempool backend configuration

## Notes

- The Bitcoin node accepts RPC connections from anywhere (`-rpcallowip=0.0.0.0/0`) - this is for local development only
- Regtest mode allows instant block generation without proof-of-work
- The bitcoin-data directory persists between container restarts
- The mempool submodule may need initialization: `git submodule update --init --recursive`
