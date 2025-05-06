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
  -e FORGE_FLAGS="--broadcast --evm-version cancun --ffi -vvvv --block-gas-limit 100000000 --legacy" \
  $IMAGE sh -c 'curl -X GET $TCB_LINK > $TCB_FILE \
    && curl -X GET $QE_IDENTITY_LINK > $QE_IDENTITY_FILE \
    && jq '.tcbInfo.fmspc |= ascii_downcase' $TCB_FILE > temp.json \
    && mv temp.json $TCB_FILE \
    && forge script ./script/layer1/SetDcapParams.s.sol:SetDcapParams --private-key $PRIVATE_KEY --fork-url $FORK_URL $FORGE_FLAGS'