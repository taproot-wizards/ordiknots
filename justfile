# == Run and manage containers: ==

# List all commands
default:
  @just --list

# Set up data directories with correct permissions
setup:
  mkdir -p ./data/regtest

# Run Bitcoin Knots node (regtest)
knots: setup
  docker compose -f docker-compose.knots.regtest.yml up

# Run local mempool.space instance
mempool: setup
  docker compose -f docker-compose.mempool.yml up

# Reset all local data
reset:
  rm -rf ./data/regtest

# == Interact with chain: ==

# Run bitcoin-cli command (without wallet)
_cli-no-wallet +args:
  @docker compose -f docker-compose.knots.regtest.yml exec -u bitcoin bitcoind bitcoin-cli -regtest -rpcuser=mempool -rpcpassword=mempool {{args}}

# Run bitcoin-cli command (with wallet)
cli +args:
  @docker compose -f docker-compose.knots.regtest.yml exec -u bitcoin bitcoind bitcoin-cli -regtest -rpcuser=mempool -rpcpassword=mempool -rpcwallet=test {{args}}

_load-or-create-test-wallet:
  #!/usr/bin/env bash
  # Check if wallet is already loaded
  if just _cli-no-wallet listwallets | grep -q '"test"'; then
    exit 0
  fi
  # Load the test wallet if it exists, create it if it doesn't
  just _cli-no-wallet loadwallet "test" &>/dev/null || just _cli-no-wallet createwallet "test" >/dev/null

_ensure-spendable-outputs: _load-or-create-test-wallet
  #!/usr/bin/env bash
  # Ensure we have spendable UTXOs by mining blocks if needed
  UNSPENT=$(just cli listunspent 2>/dev/null | grep "txid" | wc -l)
  if [ "$UNSPENT" -eq 0 ]; then
    echo "No spendable UTXOs found. Mining 101 blocks..." >&2
    just mine 101
  fi

# Mine blocks
mine blocks="1": _load-or-create-test-wallet
  #!/usr/bin/env bash
  ADDRESS=$(just cli getnewaddress | tr -d '\r')
  just cli generatetoaddress {{blocks}} $ADDRESS

# == Image encoder/decoder: ==

# Run the Ordiknots indexer
index cmd: setup
  cargo run -- index {{ cmd }}

# Run "cargo check"
check:
  cargo check

# Test the code:
test:
  cargo test

# Test the full encode/decode flow for all techniques:
test-integration: _ensure-spendable-outputs
  cargo test --test integration_test -- --ignored --test-threads=1 --nocapture

# Encode a file (create a knotwork)
encode file_path +args="": _ensure-spendable-outputs
  cargo run -- encode "{{file_path}}" --broadcast {{args}}

# Decode a knotwork (given its txid)
decode txid output_path +args="":
  cargo run -- decode "{{txid}}" --output "{{output_path}}" {{args}} 

