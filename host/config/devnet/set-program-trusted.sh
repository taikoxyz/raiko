#!/bin/bash

if [ -z "$1" ]; then
  echo "Usage: $0 ./host/config/devnet/.env"
  exit 1
fi

set -a
grep -v -E '^\s*($|#|=)' "$1" > /tmp/filtered_env_$$
source /tmp/filtered_env_$$
rm /tmp/filtered_env_$$
set +a

IMAGE="${IMAGE:-nethsurge/protocol:devnet}"

docker run --rm \
  -e SP1_VERIFIER_ADDRESS="$SP1_VERIFIER_ADDRESS" \
  -e GUEST_PROOF_PROGRAM_VK="$GUEST_PROOF_PROGRAM_VK" \
  -e AGGREGATION_PROOF_PROGRAM_VK="$AGGREGATION_PROOF_PROGRAM_VK" \
  -e PRIVATE_KEY="$PRIVATE_KEY" \
  -e FORK_URL="$FORK_URL" \
  $IMAGE sh -c '
    cast send $SP1_VERIFIER_ADDRESS "setProgramTrusted(bytes32,bool)" $GUEST_PROOF_PROGRAM_VK true --rpc-url $FORK_URL --private-key $PRIVATE_KEY
    cast send $SP1_VERIFIER_ADDRESS "setProgramTrusted(bytes32,bool)" $AGGREGATION_PROOF_PROGRAM_VK true --rpc-url $FORK_URL --private-key $PRIVATE_KEY
  '