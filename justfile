install:
  docker pull ruimarinho/bitcoin-core

create_network:
  docker network create bitcoin-net

start:
  docker run -d --name bitcoind --network bitcoin-net \
  -v ./bitcoin-data:/home/bitcoin/.bitcoin \
  -p 18443:18443 \
  ruimarinho/bitcoin-core \
  -regtest=1 -server=1 -rpcbind=0.0.0.0 -rpcallowip=0.0.0.0/0 -printtoconsole

bcli +args:
  docker run --rm --network bitcoin-net \
  -v ./bitcoin-data:/home/bitcoin/.bitcoin \
  ruimarinho/bitcoin-core bitcoin-cli \
  -regtest -rpcconnect=bitcoind {{args}}
