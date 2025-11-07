start:
  docker compose up

cli +args:
  docker compose exec -u bitcoin bitcoind bitcoin-cli -regtest {{args}}

[working-directory: 'mempool/docker']
mempool:
  docker compose up
