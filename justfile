# == Run and manage containers: ==

start:
  docker compose up

mempool:
  docker compose -f docker-compose.mempool.yml up

reset:
  docker compose down -v
  docker compose -f docker-compose.mempool.yml down -v

# == Interact with chain: ==

cli +args:
  @docker compose exec -u bitcoin bitcoind bitcoin-cli -regtest -rpcuser=mempool -rpcpassword=mempool {{args}}

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

[working-directory: 'tx_creator']
create_tx message:
  #!/usr/bin/env bash
  # Ensure we have spendable UTXOs by mining blocks if needed
  UNSPENT=$(just cli listunspent 2>/dev/null | grep "txid" | wc -l)
  if [ "$UNSPENT" -eq 0 ]; then
    echo "No spendable UTXOs found. Mining 101 blocks..." >&2
    just mine 101 > /dev/null 2>&1
  fi

  OUTPUT=$(cargo run -- --message "{{message}}" --broadcast | tee /dev/tty)
  TXID=$(echo "$OUTPUT" | grep "TXID:" | tail -1 | awk '{print $2}' | tr -d '\r\n')

  echo "$TXID"

[working-directory: 'tx_creator']
encode_file file:
  #!/usr/bin/env bash
  # Ensure we have spendable UTXOs by mining blocks if needed
  UNSPENT=$(just cli listunspent 2>/dev/null | grep "txid" | wc -l)
  if [ "$UNSPENT" -eq 0 ]; then
    echo "No spendable UTXOs found. Mining 101 blocks..." >&2
    just mine 101 > /dev/null 2>&1
  fi

  cargo run -- --file "{{file}}" --broadcast

[working-directory: 'tx_creator']
decode_image txid output:
  cargo run -- --decode "{{txid}}" --output "{{output}}"

