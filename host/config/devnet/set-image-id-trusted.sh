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
  -e RISC0_VERIFIER_ADDRESS="$RISC0_VERIFIER_ADDRESS" \
  -e GUEST_PROOF_IMAGE="$GUEST_PROOF_IMAGE" \
  -e AGGREGATION_PROOF_IMAGE="$AGGREGATION_PROOF_IMAGE" \
  -e PRIVATE_KEY="$PRIVATE_KEY" \
  -e FORK_URL="$FORK_URL" \
  $IMAGE sh -c '
    cast send $RISC0_VERIFIER_ADDRESS "setImageIdTrusted(bytes32,bool)" $GUEST_PROOF_IMAGE true --rpc-url $FORK_URL --private-key $PRIVATE_KEY
    cast send $RISC0_VERIFIER_ADDRESS "setImageIdTrusted(bytes32,bool)" $AGGREGATION_PROOF_IMAGE true --rpc-url $FORK_URL --private-key $PRIVATE_KEY
  '