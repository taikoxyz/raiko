#!/bin/bash

if [ -z "$1" ]; then
  echo "Usage: $0 ./host/config/devnet/.env"
  exit 1
fi

set -a
source "$1"
set +a

IMAGE="${IMAGE:-nethsurge/protocol:devnet}"

docker run --rm \
  $IMAGE sh -c 'cast send $RISC0_VERIFIER_ADDRESS "setImageIdTrusted(bytes32,bool)" $GUEST_PROOF_IMAGE true --rpc-url $FORK_URL --private-key $PRIVATE_KEY
  && cast send $RISC0_VERIFIER_ADDRESS "setImageIdTrusted(bytes32,bool)" $AGGREGATION_PROOF_IMAGE true --rpc-url $FORK_URL --private-key $PRIVATE_KEY'