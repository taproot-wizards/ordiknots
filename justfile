start:
  docker compose up

cli +args:
  @docker compose exec -u bitcoin bitcoind bitcoin-cli -regtest -rpcuser=mempool -rpcpassword=mempool {{args}}

mine blocks="1":
  #!/usr/bin/env bash
  # Create wallet if it doesn't exist (ignore error if it already exists)
  just cli createwallet "default" &>/dev/null || true
  ADDRESS=$(just cli getnewaddress | tr -d '\r')
  just cli generatetoaddress {{blocks}} $ADDRESS

mempool:
  docker compose -f docker-compose.mempool.yml up

reset:
  docker compose down -v
  docker compose -f docker-compose.mempool.yml down -v
