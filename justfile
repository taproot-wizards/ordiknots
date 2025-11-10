# == Run and manage containers: ==

default:
    @just --list

knots:
  docker compose -f docker-compose.knots.yml up

mempool:
  docker compose -f docker-compose.mempool.yml up

reset:
  docker compose -f docker-compose.knots.yml down -v
  docker compose -f docker-compose.mempool.yml down -v
  rm ./ordiknots.db

# == Interact with chain: ==

# CLI command without wallet (for wallet management operations)
cli-no-wallet +args:
  @docker compose -f docker-compose.knots.yml exec -u bitcoin bitcoind bitcoin-cli -regtest -rpcuser=mempool -rpcpassword=mempool {{args}}

# CLI command that uses the test wallet (for most operations)
cli +args:
  @docker compose -f docker-compose.knots.yml exec -u bitcoin bitcoind bitcoin-cli -regtest -rpcuser=mempool -rpcpassword=mempool -rpcwallet=test {{args}}

load-or-create-test-wallet:
  #!/usr/bin/env bash
  # Check if wallet is already loaded
  if just cli-no-wallet listwallets | grep -q '"test"'; then
    exit 0
  fi
  # Load the test wallet if it exists, create it if it doesn't
  just cli-no-wallet loadwallet "test" &>/dev/null || just cli-no-wallet createwallet "test" >/dev/null

mine blocks="1": load-or-create-test-wallet
  #!/usr/bin/env bash
  ADDRESS=$(just cli getnewaddress | tr -d '\r')
  just cli generatetoaddress {{blocks}} $ADDRESS

ensure-spendable-outputs: load-or-create-test-wallet
  #!/usr/bin/env bash
  # Ensure we have spendable UTXOs by mining blocks if needed
  UNSPENT=$(just cli listunspent 2>/dev/null | grep "txid" | wc -l)
  if [ "$UNSPENT" -eq 0 ]; then
    echo "No spendable UTXOs found. Mining 101 blocks..." >&2
    just mine 101
  fi

# == Image encoder/decoder: ==

index:
  cargo run -- index start

stats:
  cargo run -- index stats

server: index
  cargo run -- server

check:
  cargo check

test:
  cargo test

test-integration: ensure-spendable-outputs
  cargo test --test integration_test -- --ignored --test-threads=1 --nocapture

encode file_path +args="": ensure-spendable-outputs
  cargo run -- encode "{{file_path}}" --broadcast {{args}}

decode txid output_path +args="":
  cargo run -- decode "{{txid}}" --output "{{output_path}}" {{args}} 

