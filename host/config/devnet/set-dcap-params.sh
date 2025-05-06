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
  -e TASK_ENABLE="$TASK_ENABLE" \
  -e TCB_LINK="$TCB_LINK" \
  -e TCB_FILE="$TCB_FILE" \
  -e TCB_INFO_PATH="$TCB_PATH" \
  -e QE_IDENTITY_LINK="$QE_IDENTITY_LINK" \
  -e QE_IDENTITY_FILE="$QE_IDENTITY_FILE" \
  -e QEID_PATH="$QE_IDENTITY_PATH" \
  -e MR_ENCLAVE="$MR_ENCLAVE" \
  -e MR_SIGNER="$MR_SIGNER" \
  -e V3_QUOTE_BYTES="$V3_QUOTE_BYTES" \
  -e SGX_VERIFIER_ADDRESS="$SGX_VERIFIER_ADDRESS" \
  -e ATTESTATION_ADDRESS="$ATTESTATION_ADDRESS" \
  -e PEM_CERTCHAIN_ADDRESS="$PEM_CERTCHAIN_ADDRESS" \
  -e PRIVATE_KEY="$PRIVATE_KEY" \
  -e FORK_URL="$FORK_URL" \
  $IMAGE sh -c '
    # Download TCB and QE identity files
    curl -X GET "$TCB_LINK" > "$TCB_FILE"
    curl -X GET "$QE_IDENTITY_LINK" > "$QE_IDENTITY_FILE"

    # Process TCB file
    jq ".tcbInfo.fmspc |= ascii_downcase" "$TCB_FILE" > temp.json
    mv temp.json "$TCB_FILE"

    # Run forge script
    forge script ./script/layer1/SetDcapParams.s.sol:SetDcapParams \
      --private-key "$PRIVATE_KEY" \
      --fork-url "$FORK_URL" \
      $FORGE_FLAGS
  '