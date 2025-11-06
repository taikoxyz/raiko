#!/usr/bin/env python3
"""
Quick helper to decode a Shasta `Proposed(bytes data)` payload.

Usage:
    1. Copy the `data` field from the `Proposed` event (omit the leading 0x).
    2. Paste it into EVENT_DATA below (or export EVENT_DATA in your shell).
    3. Run `python3 script/decode_shasta_event.py`.

The script prints the proposal id, proposer, origin information, blob slices and
the latest L2 block id (`last_proposal_block_id`).  Once you know how many L2
blocks the manifest contains, you can derive the full range:

    start_block = last_proposal_block_id - block_count + 1
"""

from __future__ import annotations

import os
import sys
sys.path.append(os.path.join(os.path.dirname(__file__)))
from shasta_event_decoder import ShastaEventDecoder  # type: ignore  # noqa: E402

EVENT_DATA = os.environ.get(
    "EVENT_DATA",
    "0000000000000000000000000000000000000000000000000000000000000000",
)


def format_hash(value: str) -> str:
    if value.startswith("0x"):
        return value
    return f"0x{value}"


def main() -> None:
    payload = EVENT_DATA.strip().lower()
    if payload.startswith("0x"):
        payload = payload[2:]
    if not payload or len(payload) % 2 != 0:
        raise SystemExit("EVENT_DATA must be a non-empty hex string")

    decoder = ShastaEventDecoder()
    event = decoder.decode_event_data(bytes.fromhex(payload))

    print("=== Proposal ===")
    print(f"  id: {event.proposal.id}")
    print(f"  proposer: {event.proposal.proposer}")
    print(f"  timestamp: {event.proposal.timestamp}")
    print(f"  end_of_submission_window: {event.proposal.end_of_submission_window_timestamp}")

    print("\n=== Derivation ===")
    print(f"  origin_block_number: {event.derivation.origin_block_number}")
    print(f"  origin_block_hash: {event.derivation.origin_block_hash}")
    print(f"  basefee_sharing_pctg: {event.derivation.basefee_sharing_pctg}")
    print(f"  sources: {len(event.derivation.sources)}")
    for idx, source in enumerate(event.derivation.sources):
        print(
            f"    source[{idx}]: forced_inclusion={source.is_forced_inclusion}, "
            f"offset={source.blob_slice.offset}, timestamp={source.blob_slice.timestamp}"
        )
        for hidx, blob_hash in enumerate(source.blob_slice.blob_hashes):
            print(f"      blob_hash[{hidx}]: {format_hash(blob_hash)}")

    print("\n=== Core State ===")
    print(f"  next_proposal_id: {event.core_state.next_proposal_id}")
    print(f"  last_proposal_block_id (latest L2 block): {event.core_state.last_proposal_block_id}")
    print(f"  last_finalized_proposal_id: {event.core_state.last_finalized_proposal_id}")
    print(f"  last_checkpoint_timestamp: {event.core_state.last_checkpoint_timestamp}")
    print(f"  last_finalized_transition_hash: {event.core_state.last_finalized_transition_hash}")
    print(f"  bond_instructions_hash: {event.core_state.bond_instructions_hash}")

    print("\nNOTE:")
    print(
        "  • `last_proposal_block_id` is the newest L2 block in this proposal.\n"
        "  • Decode the blob(s) listed above, count the number of `blocks` in the manifest,\n"
        "    then compute: start = last_proposal_block_id - block_count + 1.\n"
        "  • Example: proposal 72’s manifest contains a single block, so the L2 range is [582]."
    )


if __name__ == "__main__":
    main()
