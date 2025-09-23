// rust impl of taiko-mono/packages/protocol/contracts/layer1/shasta/libs/LibHashing.sol

use crate::input::shasta::Transition as ShastaTransition;

use crate::input::shasta::Checkpoint;
use crate::primitives::keccak::keccak;
use alloy_primitives::{Address, B256, U256};
use reth_primitives::b256;

/// Hash a transition using the same logic as the Solidity implementation
pub fn hash_transition(transition: &ShastaTransition) -> B256 {
    hash_five_values(
        transition.proposalHash,
        transition.parentTransitionHash,
        hash_checkpoint(&transition.checkpoint),
        address_to_b256(transition.designatedProver),
        address_to_b256(transition.actualProver),
    )
}

/// Returns `keccak256(abi.encode(value0, .., value4))` - equivalent to Solidity's EfficientHashLib.hash
pub fn hash_five_values(
    value0: B256,
    value1: B256,
    value2: B256,
    value3: B256,
    value4: B256,
) -> B256 {
    let mut data = Vec::with_capacity(160); // 5 * 32 bytes
    data.extend_from_slice(value0.as_slice());
    data.extend_from_slice(value1.as_slice());
    data.extend_from_slice(value2.as_slice());
    data.extend_from_slice(value3.as_slice());
    data.extend_from_slice(value4.as_slice());

    keccak(&data).into()
}

/// Hash a checkpoint using the same logic as the Solidity implementation
pub fn hash_checkpoint(checkpoint: &Checkpoint) -> B256 {
    hash_three_values(
        U256::from(checkpoint.blockNumber).into(),
        checkpoint.blockHash,
        checkpoint.stateRoot,
    )
}

/// Returns `keccak256(abi.encode(value0, value1))` - equivalent to Solidity's EfficientHashLib.hash
pub fn hash_two_values(value0: B256, value1: B256) -> B256 {
    let mut data = Vec::with_capacity(64); // 2 * 32 bytes
    data.extend_from_slice(value0.as_slice());
    data.extend_from_slice(value1.as_slice());

    keccak(&data).into()
}

/// Returns `keccak256(abi.encode(value0, value1, value2))`
pub fn hash_three_values(value0: B256, value1: B256, value2: B256) -> B256 {
    let mut data = Vec::with_capacity(96); // 3 * 32 bytes
    data.extend_from_slice(value0.as_slice());
    data.extend_from_slice(value1.as_slice());
    data.extend_from_slice(value2.as_slice());

    keccak(&data).into()
}

/// Convert an Address to B256 by zero-padding (equivalent to bytes32(uint256(uint160(address))))
fn address_to_b256(address: Address) -> B256 {
    let mut result = [0u8; 32];
    result[12..32].copy_from_slice(address.as_slice());
    B256::from(result)
}

const EMPTY_BYTES_HASH: B256 =
    b256!("c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");

/// Hash an array of transitions using the same logic as the Solidity implementation
pub fn hash_transitions_array(transitions: &[ShastaTransition]) -> B256 {
    if transitions.is_empty() {
        return EMPTY_BYTES_HASH;
    }

    // For small arrays (most common case), use direct hashing with length
    if transitions.len() == 1 {
        return hash_two_values(
            U256::from(transitions.len()).into(),
            hash_transition(&transitions[0]),
        );
    }

    if transitions.len() == 2 {
        return hash_three_values(
            U256::from(transitions.len()).into(),
            hash_transition(&transitions[0]),
            hash_transition(&transitions[1]),
        );
    }

    // For larger arrays, use memory-optimized approach
    // Pre-allocate exact buffer size: 32 bytes for length + 32 bytes per hash
    let array_length = transitions.len();
    let buffer_size = 32 + (array_length * 32);
    let mut buffer = Vec::with_capacity(buffer_size);

    // Write array length at start of buffer
    buffer.extend_from_slice(&U256::from(array_length).to_be_bytes::<32>());

    // Write each transition hash directly to buffer
    for transition in transitions {
        let transition_hash = hash_transition(transition);
        buffer.extend_from_slice(transition_hash.as_slice());
    }

    // Return keccak256 hash of the buffer
    keccak(&buffer).into()
}

#[cfg(test)]
mod test {
    use reth_primitives::{address, b256};

    use crate::protocol_instance::shasta_aggregation_output;

    use super::*;

    #[test]
    fn test_shasta_transition_hash() {
        // Create a transition with fixed test values
        let transition = ShastaTransition {
            proposalHash: b256!("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            parentTransitionHash: b256!(
                "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
            ),
            checkpoint: Checkpoint {
                blockNumber: 999999,
                blockHash: b256!(
                    "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                ),
                stateRoot: b256!(
                    "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
                ),
            },
            designatedProver: address!("7777777777777777777777777777777777777777"),
            actualProver: address!("8888888888888888888888888888888888888888"),
        };

        let transition_hash = hash_transition(&transition);
        assert_eq!(
            hex::encode(transition_hash),
            "6afc3bc3d1036f35d3f2de001af27f5e9baffd6a5ebec284acc46ccd08efe639"
        );
    }

    #[test]
    fn test_hash_transitions_array_fixed_values() {
        // Create array with 3 transitions using fixed values
        let transitions = vec![
            // Transition 1
            ShastaTransition {
                proposalHash: b256!(
                    "1111111111111111111111111111111111111111111111111111111111111111"
                ),
                parentTransitionHash: b256!(
                    "2222222222222222222222222222222222222222222222222222222222222222"
                ),
                checkpoint: Checkpoint {
                    blockNumber: 1000,
                    blockHash: b256!(
                        "3333333333333333333333333333333333333333333333333333333333333333"
                    ),
                    stateRoot: b256!(
                        "4444444444444444444444444444444444444444444444444444444444444444"
                    ),
                },
                designatedProver: address!("1111111111111111111111111111111111111111"),
                actualProver: address!("2222222222222222222222222222222222222222"),
            },
            // Transition 2
            ShastaTransition {
                proposalHash: b256!(
                    "5555555555555555555555555555555555555555555555555555555555555555"
                ),
                parentTransitionHash: b256!(
                    "6666666666666666666666666666666666666666666666666666666666666666"
                ),
                checkpoint: Checkpoint {
                    blockNumber: 2000,
                    blockHash: b256!(
                        "7777777777777777777777777777777777777777777777777777777777777777"
                    ),
                    stateRoot: b256!(
                        "8888888888888888888888888888888888888888888888888888888888888888"
                    ),
                },
                designatedProver: address!("3333333333333333333333333333333333333333"),
                actualProver: address!("4444444444444444444444444444444444444444"),
            },
            // Transition 3
            ShastaTransition {
                proposalHash: b256!(
                    "9999999999999999999999999999999999999999999999999999999999999999"
                ),
                parentTransitionHash: b256!(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                ),
                checkpoint: Checkpoint {
                    blockNumber: 3000,
                    blockHash: b256!(
                        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    ),
                    stateRoot: b256!(
                        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                    ),
                },
                designatedProver: address!("5555555555555555555555555555555555555555"),
                actualProver: address!("6666666666666666666666666666666666666666"),
            },
        ];

        // Calculate hash using hashTransitionsArray equivalent
        let result = hash_transitions_array(&transitions);
        assert_eq!(
            hex::encode(result),
            "bc7907af6f75e8774d953600770dea1912688f8492bedee719be809143545c33"
        );

        // Test individual transition hashes
        let individual_hashes: Vec<String> = transitions
            .iter()
            .map(|t| hex::encode(hash_transition(t)))
            .collect();

        assert_eq!(
            individual_hashes[0],
            "08ecd80c33f12026e5210a422049ec0aaca3fe92ae201138143e1e30be78106e"
        );
        assert_eq!(
            individual_hashes[1],
            "593fcc44adfa8722be8b59f2f77b63ab82897fc745b7248244d1157d7720f12b"
        );
        assert_eq!(
            individual_hashes[2],
            "9a14e589f1966e9677942d75e6e3f0f68e6bb8635c37866f0a154b29a7a8fd0c"
        );
    }
}
