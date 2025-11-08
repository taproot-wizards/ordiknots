# == Run and manage containers: ==

knots:
  docker compose -f docker-compose.knots.yml up

mempool:
  docker compose -f docker-compose.mempool.yml up

reset:
  docker compose -f docker-compose.knots.yml down -v
  docker compose -f docker-compose.mempool.yml down -v

# == Interact with chain: ==

cli +args:
  @docker compose -f docker-compose.knots.yml exec -u bitcoin bitcoind bitcoin-cli -regtest -rpcuser=mempool -rpcpassword=mempool {{args}}

mine blocks="1":
  #!/usr/bin/env bash
  # Create wallet if it doesn't exist (ignore error if it already exists)
  just cli createwallet "default" &>/dev/null || true
  ADDRESS=$(just cli getnewaddress | tr -d '\r')
  just cli generatetoaddress {{blocks}} $ADDRESS

block height:
  #!/usr/bin/env bash
  BLOCKHASH=$(just cli getblockhash {{height}} | tr -d '\r')
  echo "Block #{{height}}"
  echo "Hash: $BLOCKHASH"
  echo ""
  echo "Transactions:"
  just cli getblock $BLOCKHASH | grep -A 1000 '"tx"' | grep '"' | sed 's/.*"\(.*\)".*/\1/' | grep -v "tx"

load-or-create-test-wallet:
  #!/usr/bin/env bash
  # Check if wallet is already loaded
  if just cli listwallets | grep -q '"test"'; then
    exit 0
  fi
  # Load the test wallet if it exists, create it if it doesn't
  just cli loadwallet "test" 2>/dev/null || just cli createwallet "test"

ensure-spendable-outputs: load-or-create-test-wallet
  #!/usr/bin/env bash
  # Ensure we have spendable UTXOs by mining blocks if needed
  UNSPENT=$(just cli listunspent 2>/dev/null | grep "txid" | wc -l)
  if [ "$UNSPENT" -eq 0 ]; then
    echo "No spendable UTXOs found. Mining 101 blocks..." >&2
    just mine 101 > /dev/null 2>&1
  fi

# == Image encoder/decoder: ==

check:
  cargo check

test:
  cargo test

integration-test: ensure-spendable-outputs
  cargo test --test integration_test -- --ignored --test-threads=1 --nocapture

encode file_path +args="": ensure-spendable-outputs
  cargo run -- {{args}} encode "{{file_path}}" --broadcast

decode txid output_path +args="":
  cargo run -- {{args}} decode "{{txid}}" --output "{{output_path}}"

