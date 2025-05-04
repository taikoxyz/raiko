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
  $IMAGE sh -c 'cast send $SP1_VERIFIER_ADDRESS "setProgramTrusted(bytes32,bool)" $GUEST_PROOF_PROGRAM_VK true --rpc-url $FORK_URL --private-key $PRIVATE_KEY
  && cast send $SP1_VERIFIER_ADDRESS "setProgramTrusted(bytes32,bool)" $AGGREGATION_PROOF_PROGRAM_VK true --rpc-url $FORK_URL --private-key $PRIVATE_KEY'