#!/usr/bin/env bash

# Use the first command line argument as the chain name
chain="$1"
# Use the second command line argument as the proof type
proof="$2"
# Use the third parameter(s) as the batch number as a range
batch_id="$3"
# Use the fourth parameter(s) as the batch number as a range
batch_proposal_height="$4"


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
    "proof_type": "NATIVE",
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
	echo "Invalid proof name. Please use 'native', 'risc0[-bonsai]', 'sp1', or 'sgx'."
	exit 1
fi


prover="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
graffiti="8008500000000000000000000000000000000000000000000000000000000000"


echo "- proving batch $batch_id @ $batch_proposal_height on $chain with $proof proof"
curl --location --request POST 'http://localhost:8080/v4/proof' \
    --header 'Content-Type: application/json' \
    --header 'Authorization: Bearer' \
    --data-raw "{
        \"network\": \"$chain\",
        \"l1_network\": \"$l1_network\",
        \"batch_id\": $batch_id,
        \"l1_inclusion_block_number\": $batch_proposal_height,
        \"prover\": \"$prover\",
        \"graffiti\": \"$graffiti\",
        $proofParam
    }"
echo ""

sleep 1.0
