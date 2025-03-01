#!/usr/bin/env bash

# Use the first command line argument as the chain name
chain="$1"
# Use the second command line argument as the proof type
proof="$2"
# Use the third command line argument as the block number
# Script will aggregate prove the block before this number and this number
block="$3"
# Use the fifth parameter as a custom proofParam
customParam="$4"

if [ $block -lt 2 ]; then
	echo "Block number must be greater than 1."
	exit 1
fi

# Check the chain name and set the corresponding RPC values
if [ "$chain" == "ethereum" ]; then
	l1_network="ethereum"
elif [ "$chain" == "holesky" ]; then
	l1_network="holesky"
elif [ "$chain" == "taiko_mainnet" ]; then
	l1_network="ethereum"
elif [ "$chain" == "taiko_a7" ]; then
	l1_network="holesky"
elif [ "$chain" == "taiko_dev" ]; then
	l1_network="taiko_dev_l1"
else
	echo "Using customized chain name $1. Please double check the RPCs."
	l1_network="holesky"
fi

if [ "$proof" == "native" ]; then
	proofParam='
    "proof_type": "native",
	"native" : {
        "json_guest_input": null
	}
  '
elif [ "$proof" == "zk_any" ]; then
	proofParam='
    "proof_type": "zk_any",
	"native" : {
        "json_guest_input": null
	},
	"zk_any": { "aggregation": false}
  '
elif [ "$proof" == "zk_any_aggregation" ]; then
	proofParam='
    "proof_type": "zk_any",
	"native" : {
        "json_guest_input": null
	},
	"zk_any": { "aggregation": false }
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
elif [ "$proof" == "risc0" ]; then
	proofParam='
    "proof_type": "risc0",
    "blob_proof_type": "proof_of_equivalence",
    "risc0": {
        "bonsai": false,
        "snark": false,
        "profile": true,
        "execution_po2": 21
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
        "execution_po2": 21
    }
  '
else
	echo "Invalid proof name. Please use 'native', 'risc0[-bonsai]', 'sp1', or 'sgx'."
	exit 1
fi

# Override the proofParam if a custom one is provided
if [ -n "$4" ]; then
    proofParam=$customParam
fi

prover="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
graffiti="8008500000000000000000000000000000000000000000000000000000000000"

if [ "$proof" == "native" ]; then
        echo "- proving block $(($block-1)) && $block"
        while true; do
                RESPONSE=$(curl -s --location --request POST 'http://localhost:8080/v3/proof' \
                        --header 'Content-Type: application/json' \
                        --header 'Authorization: Bearer 4cbd753fbcbc2639de804f8ce425016a50e0ecd53db00cb5397912e83f5e570e' \
                        --data-raw "{
                                \"network\": \"$chain\",
                                \"l1_network\": \"$l1_network\",
                                \"block_numbers\": [[$(($block-1)), null], [$block, null]],
                                \"prover\": \"$prover\",
                                \"graffiti\": \"$graffiti\",
                                $proofParam
                        }")

                if [[ "$RESPONSE" == *'"proof":{"input":null,"kzg_proof":null,"proof":null,"quote":null,"uuid":null}'* ]]; then
                        echo "Aggregate proof successful."
                        break
                fi

                echo "Proof still in progress. Retrying in 30 seconds..."
                sleep 30
        done
else
        echo "- proving block $(($block-1)) && $block"
        curl --location --request POST 'http://localhost:8080/v3/proof' \
                                --header 'Content-Type: application/json' \
                                --header 'Authorization: Bearer 4cbd753fbcbc2639de804f8ce425016a50e0ecd53db00cb5397912e83f5e570e' \
                                --data-raw "{
                                        \"network\": \"$chain\",
                                        \"l1_network\": \"$l1_network\",
                                        \"block_numbers\": [[$(($block-1)), null], [$block, null]],
                                        \"prover\": \"$prover\",
                                        \"graffiti\": \"$graffiti\",
                                        $proofParam
                                }"
        echo ""
fi

