#!/usr/bin/env bash

# Use the first command line argument as the chain name
chain="$1"
# Use the second command line argument as the proof type
proof="$2"
# Use the third parameter(s) as the batch number as a range
batch_id="$3"
# Use the fourth parameter(s) as the batch number as a range
batch_proposal_height="$4"
aggregate="${5:-"false"}"

$(dirname "$0")/prove.sh batch "$chain" "$proof" "$batch_id":"$batch_proposal_height" "$aggregate"