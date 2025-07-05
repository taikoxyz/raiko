#!/usr/bin/env bash

prover=${prover:-"0x70997970C51812dc3A010C7d01b50e0d17dc79C8"}
graffiti=${graffiti:-"8008500000000000000000000000000000000000000000000000000000000000"}
raiko_endpoint=${raiko_endpoint:-"http://localhost:8080"}
raiko_api_key=${raiko_api_key:-"4cbd753fbcbc2639de804f8ce425016a50e0ecd53db00cb5397912e83f5e570e"}

usage() {
    echo "Usage:"
    echo "  prove.sh batch <chain> <proof_type> [<batch_id>:<batch_proposal_height>,...] [aggregate(default: false)]"
    echo "  prove.sh block <chain> <proof_type> <block_id> [aggregate(default: false)]"
    exit 1
}

# Check the chain name and set the corresponding RPC values
get_l1_network() {
    local chain="$1"
    local l1_network

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
        echo "Invalid chain name. Please use 'ethereum', 'holesky', 'taiko_mainnet', 'taiko_a7', or 'taiko_dev'."
        exit 1
    fi

    echo "$l1_network"
}

prove_batch() {
    local chain="${1?"chain is required"}"
    local proof_type="${2?"proof_type is required"}"
    local batch_inputs="${3?"batch inputs are required"}"
    local aggregate="${4:-"false"}"
    local l1_network="${5?"l1_network is required"}"

    # Convert comma-separated batch inputs into JSON array
    local batches_json="["
    local first=true
    IFS=',' read -ra BATCHES <<< "$batch_inputs"
    for batch in "${BATCHES[@]}"; do
        IFS=':' read -r batch_id batch_proposal_height <<< "$batch"
        if [ "$first" = true ]; then
            first=false
        else
            batches_json+=","
        fi
        batches_json+="{\"batch_id\": $batch_id, \"l1_inclusion_block_number\": $batch_proposal_height}"
    done
    batches_json+="]"

    set -x
    curl --location --request POST "$raiko_endpoint/v3/proof/batch" \
        --header "Content-Type: application/json" \
        --header "Authorization: Bearer $raiko_api_key" \
        --data-raw "{
            \"network\": \"$chain\",
            \"l1_network\": \"$l1_network\",
            \"batches\": $batches_json,
            \"prover\": \"$prover\",
            \"graffiti\": \"$graffiti\",
            \"aggregate\": $aggregate,
            \"proof_type\": \"$proof_type\"
        }"
    set +x
}

prove_block() {
    local chain="${1?"chain is required"}"
    local proof_type="${2?"proof_type is required"}"
    local block_id="${3?"block_id is required"}"
    local aggregate="${4:-"false"}"
    local l1_network="${5?"l1_network is required"}"

    set -x
    curl --location --request POST "$raiko_endpoint/v2/proof" \
        --header "Content-Type: application/json" \
		--header "Authorization: Bearer $raiko_api_key" \
        --data-raw "{
            \"network\": \"$chain\",
            \"l1_network\": \"$l1_network\",
            \"block_numbers\": [[$block_id, null], [$(($block_id+1)), null]],
            \"block_number\": $block_id,
            \"prover\": \"$prover\",
            \"graffiti\": \"$graffiti\",
            \"aggregate\": $aggregate,
            \"proof_type\": \"$proof_type\"
        }"
    set +x
}

main() {
    mode="$1"
    case "$mode" in
        batch)
            chain="$2"
            proof_type="$3"
            batch_inputs="$4"
            aggregate="${5:-"false"}"
            l1_network=$(get_l1_network "$chain")
            prove_batch "$chain" "$proof_type" "$batch_inputs" "$aggregate" "$l1_network"
            ;;
        block)
            chain="$2"
            proof_type="$3"
            block_id="$4"
            aggregate="${5:-"false"}"
            l1_network=$(get_l1_network "$chain")
            prove_block "$chain" "$proof_type" "$block_id" "$aggregate" "$l1_network"
            ;;
        *)
            usage
            ;;
    esac
    echo ""
}

main "$@"