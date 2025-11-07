# == Run and manage containers: ==

bitcoin:
  docker compose -f docker-compose.bitcoind.yml up

knots:
  docker compose -f docker-compose.knots.yml up

mempool:
  docker compose -f docker-compose.mempool.yml up

reset:
  docker compose -f docker-compose.bitcoind.yml down -v
  docker compose -f docker-compose.mempool.yml down -v

[working-directory: 'tx_creator']
test:
  cargo test

[working-directory: 'tx_creator']
test_roundtrip:
  just ensure_spendable_outputs
  cargo test --test roundtrip_test -- --ignored --nocapture

# == Interact with chain: ==

cli +args:
  @docker compose -f docker-compose.bitcoind.yml exec -u bitcoin bitcoind bitcoin-cli -regtest -rpcuser=mempool -rpcpassword=mempool {{args}}

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

# == Image encoder/decoder: ==

ensure_spendable_outputs:
  #!/usr/bin/env bash
  # Ensure we have spendable UTXOs by mining blocks if needed
  UNSPENT=$(just cli listunspent 2>/dev/null | grep "txid" | wc -l)
  if [ "$UNSPENT" -eq 0 ]; then
    echo "No spendable UTXOs found. Mining 101 blocks..." >&2
    just mine 101 > /dev/null 2>&1
  fi

[working-directory: 'tx_creator']
create_tx message:
  #!/usr/bin/env bash
  just ensure_spendable_outputs
  OUTPUT=$(cargo run -- --message "{{message}}" --broadcast | tee /dev/tty)
  TXID=$(echo "$OUTPUT" | grep "TXID:" | tail -1 | awk '{print $2}' | tr -d '\r\n')

  echo "$TXID"

[working-directory: 'tx_creator']
encode file_path:
  just ensure_spendable_outputs
  cargo run -- --file "{{file_path}}" --broadcast

[working-directory: 'tx_creator']
decode txid output_path:
  cargo run -- --decode "{{txid}}" --output "{{output_path}}"

