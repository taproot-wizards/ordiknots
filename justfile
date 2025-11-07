start:
  docker compose up

cli +args:
  @docker compose exec -u bitcoin bitcoind bitcoin-cli -regtest {{args}}

mine blocks="1":
  #!/usr/bin/env bash
  # Create wallet if it doesn't exist (ignore error if it already exists)
  just cli createwallet "default" &>/dev/null || true
  ADDRESS=$(just cli getnewaddress | tr -d '\r')
  just cli generatetoaddress {{blocks}} $ADDRESS

[working-directory: 'mempool/docker']
mempool:
  docker compose up
