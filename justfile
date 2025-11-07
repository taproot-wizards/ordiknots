start:
  docker compose up

cli +args:
  docker compose exec -u bitcoin bitcoind bitcoin-cli -regtest {{args}}
