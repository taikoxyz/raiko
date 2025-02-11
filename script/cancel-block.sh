#!/usr/bin/env bash

getBlockNumber() {
	# Get the latest block number from the node
	output=$(curl $rpc -s -X POST -H "Content-Type: application/json" --data '{"method":"eth_blockNumber","params":[],"id":1,"jsonrpc":"2.0"}')

	# Extract the hexadecimal number using jq and remove the surrounding quotes
	hex_number=$(echo $output | jq -r '.result')

	# Convert the hexadecimal to decimal
	block_number=$(echo $((${hex_number})))

	# Return the block number by echoing it
	echo "$block_number"
}

# Use the first command line argument as the chain name
chain="$1"
# Use the second command line argument as the proof type
proof="$2"
# Use the third(/fourth) parameter(s) as the block number as a range
# Use the special value "sync" as the third parameter to follow the tip of the chain
rangeStart="$3"
rangeEnd="$4"

# Check the chain name and set the corresponding RPC values
if [ "$chain" == "ethereum" ]; then
	l1_network="ethereum"
elif [ "$chain" == "holesky" ]; then
	l1_network="holesky"
elif [ "$chain" == "taiko_mainnet" ]; then
	l1_network="ethereum"
elif [ "$chain" == "taiko_a7" ]; then
	l1_network="holesky"
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
elif [ "$proof" == "sp1" ]; then
	proofParam='
    "proof_type": "sp1"
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

if [ "$rangeStart" == "sync" ]; then
	sync="true"
	rangeStart=$(getBlockNumber)
	rangeEnd=$((rangeStart + 1000000))
	sleep 1.0
fi

if [ "$rangeStart" == "" ]; then
	echo "Please specify a valid block range like \"10\" or \"10 20\""
	exit 1
fi

if [ "$rangeEnd" == "" ]; then
	rangeEnd=$rangeStart
fi

prover="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
graffiti="8008500000000000000000000000000000000000000000000000000000000000"

for block in $(eval echo {$rangeStart..$rangeEnd}); do
	# Special sync logic to follow the tip of the chain
	if [ "$sync" == "true" ]; then
		block_number=$(getBlockNumber)
		# While the current block is greater than the block number from the blockchain
		while [ "$block" -gt "$block_number" ]; do
			sleep 0.1                      # Wait for 100ms
			block_number=$(getBlockNumber) # Query again to get the updated block number
		done
		# Sleep a bit longer because sometimes the block data isn't available yet
		sleep 1.0
	fi

	echo "- proving block $block"
	curl --location --request POST 'http://localhost:8080/proof/cancel' \
		--header 'Content-Type: application/json' \
		--header 'Authorization: Bearer 4cbd753fbcbc2639de804f8ce425016a50e0ecd53db00cb5397912e83f5e570e' \
		--data-raw "{
         \"network\": \"$chain\",
         \"l1_network\": \"$l1_network\",
         \"block_number\": $block,
         \"prover\": \"$prover\",
         \"graffiti\": \"$graffiti\",
         $proofParam
       }"
	echo ""
done
