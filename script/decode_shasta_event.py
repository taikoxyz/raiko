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
    "0000000005e43c44cdddb6a900fa2b585dd299e03d12fa4293bc000069099ef000000000000000000000376ecb12ca3f9efe78a0ee210177d4a1381d398ecf1bd6d810dd6abcc62441f12d8a4b0001000001018afd26c637b2e9f6d48b192ba79c36d9c7ec004dd8bb2206677115748ff948000000000069099ef01209923e67f6848991132845c5f6057af444cad8c5356d381d626ff84eb1dda51817d901934ec82607babfd4e21433ac448e2eadcd1b4acdc7d6bc3047168caa0000000005e500000000376f0000000005d7000069099ef025bdf0b41f8005db751468701fd31760512b232eaf85d40a927914a23d9042c17709af84e02ab35142f25fca8caa907cb974d8872024c27874b2362ad52e3f0f0000",
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
