#!/usr/bin/env bash

if [ "$#" -ne 3 ]; then
  echo "Usage: prove-batch.sh <chain> <proof> <batch_info>"
  echo "  chain: taiko_mainnet, taiko_a7, taiko_dev"
  echo "  proof: native, risc0[-bonsai], sp1, sgx, sgxgeth"
  echo "  batch_info: \"[(batch_id, batch_proposal_height)]\""
  echo "Example:"
  echo "  prove-batch.sh ethereum native \"[(1, 2)]\" "
  exit 1
fi

# Use the first command line argument as the chain name
chain="$1"
# Use the second command line argument as the proof type
proof="$2"

# # Use the third parameter(s) as the batch number as a range
# batch_id="$3"
# # Use the fourth parameter(s) as the batch number as a range
# batch_proposal_height="$4"
# aggregate="${5:-"false"}"

# $(dirname "$0")/prove.sh batch "$chain" "$proof" "$batch_id":"$batch_proposal_height" "$aggregate"

# Use the third command line argument as a tuple of the batch number and l1 inclusion number
batch_info="$3"

batch_id=""
height=""

parse_batch_pair() {
  local input="$1"

  local cleaned
  cleaned=$(echo "$input" | sed 's/[][]//g' | sed 's/ //g' | tr -d '()')

  local pair
  pair=$(echo "$cleaned" | grep -oE '[0-9]+,[0-9]+')

  if [[ -z "$pair" ]]; then
    echo "❌ Invalid input format: expected something like \"[(1,2)]\"" >&2
    return 1
  fi

  local json_array="["
  local first=1

  while IFS=',' read -r batch_id height; do
    if [[ $first -eq 0 ]]; then
      json_array+=", "
    fi
    json_array+="{\"batch_id\": $batch_id, \"l1_inclusion_block_number\": $height}"
    first=0
  done <<< "$pair"

  json_array+="]"
  echo "$json_array"
}

batch_request=$(parse_batch_pair "$batch_info")
if [[ $? -ne 0 ]]; then
  exit 1
fi

echo "Parsed batch request: $batch_request"

# Check the chain name and set the corresponding RPC values
if [ "$chain" == "ethereum" ]; then
	l1_network="ethereum"
elif [ "$chain" == "holesky" ]; then
	l1_network="holesky"
elif [ "$chain" == "taiko_mainnet" ]; then
	l1_network="ethereum"
elif [ "$chain" == "taiko_a7" ]; then
	l1_network="holesky"
elif [ "$chain" == "taiko_hoodi" ]; then
	l1_network="hoodi"
elif [ "$chain" == "taiko_dev" ]; then
	l1_network="taiko_dev_l1"
else
	echo "Using customized chain name $1. Please double check the RPCs."
	l1_network="holesky"
fi

if [ "$proof" == "native" ]; then
	proofParam='
    "proof_type": "NATIVE",
    "blob_proof_type": "proof_of_equivalence",
	"native" : {
        "json_guest_input": null
	}
  '
elif [ "$proof" == "sp1" ]; then
	proofParam='
    "proof_type": "sp1",
    "blob_proof_type": "proof_of_equivalence",
	"sp1": {
		"recursion": "plonk",
		"prover": "network",
		"verify": true
	}
  '
elif [ "$proof" == "sp1-aggregation" ]; then
	proofParam='
    "proof_type": "sp1",
    "blob_proof_type": "proof_of_equivalence",
	"sp1": {
		"recursion": "compressed",
		"prover": "network",
		"verify": false
	}
  '
elif [ "$proof" == "sgx" ]; then
	proofParam='
    "proof_type": "sgx",
    "sgx" : {
        "instance_id": 123,
        "setup": false,
        "bootstrap": false,
        "prove": true,
        "input_path": null
    }
'
elif [ "$proof" == "sgxgeth" ]; then
	proofParam='
    "proof_type": "sgxgeth",
    "sgxgeth" : {
        "instance_id": 456,
        "setup": false,
        "bootstrap": false,
        "prove": true,
        "input_path": null
    }
'
elif [ "$proof" == "risc0" ]; then
	proofParam='
    "proof_type": "risc0",
    "blob_proof_type": "proof_of_equivalence",
    "risc0": {
        "bonsai": false,
        "snark": false,
        "profile": true,
        "execution_po2": 18
    }
  '
elif [ "$proof" == "risc0-bonsai" ]; then
	proofParam='
    "proof_type": "risc0",
    "blob_proof_type": "proof_of_equivalence",
    "risc0": {
        "bonsai": true,
        "snark": true,
        "profile": false,
        "execution_po2": 20
    }
  '
else
	echo "Invalid proof name. Please use 'native', 'risc0[-bonsai]', 'sp1', 'sgxgeth' or 'sgx'."
	exit 1
fi

prover="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"

AGG=${AGG:-false}

curl --location --request POST 'http://localhost:8080/v3/proof/batch' \
    --header 'Content-Type: application/json' \
    --header 'Authorization: Bearer' \
    --data-raw "{
        \"network\": \"$chain\",
        \"l1_network\": \"$l1_network\",
        \"batches\": $batch_request,
        \"prover\": \"$prover\",
        \"aggregate\": $AGG,
        $proofParam
    }"
echo ""
sleep 1.0
