//! Aggregates Shasta proposal proofs using ZisK
#![no_main]
ziskos::entrypoint!(main);

mod precompile_shims;
mod ruint_shims;

use raiko_lib::{
    input::ShastaRisc0AggregationGuestInput,
    libhash::hash_shasta_subproof_input,
    primitives::B256,
    protocol_instance::{shasta_aggregation_hash_for_zk, words_to_bytes_le},
};

pub fn main() {
    // // Route ecrecover through the ziskos high-level syscall instead of the
    // // k256 patch field-op path (reduces ROM size from ~500+ calls to 1).
    // raiko_lib::revm::precompile::install_crypto(zisk_crypto::ZiskCrypto);
    // let crypto = Arc::new(zisk_crypto::ZiskCrypto);
    // raiko_lib::alloy_consensus::crypto::install_default_provider(crypto.clone())
    //     .expect("crypto provider already set");

    // Initialize hints stream (native build only — emits precompile hint requests)
    #[cfg(zisk_hints)]
    ziskos::hints::init_hints_file("/tmp/zisk-hints.bin".into(), None)
        .expect("failed to init hints");

    let input_data = ziskos::io::read_vec();
    assert!(!input_data.is_empty(), "shasta aggregation input is empty");

    let input: ShastaRisc0AggregationGuestInput = bincode::deserialize(&input_data)
        .expect("failed to deserialize ShastaRisc0AggregationGuestInput");

    assert!(
        !input.block_inputs.is_empty(),
        "shasta aggregation input has no block inputs"
    );

    assert_eq!(
        input.block_inputs.len(),
        input.proof_carry_data_vec.len(),
        "block inputs and proof carry data length mismatch"
    );

    for (i, block_input) in input.block_inputs.iter().enumerate() {
        let expected = hash_shasta_subproof_input(&input.proof_carry_data_vec[i]);
        assert_eq!(
            *block_input, expected,
            "shasta block input {} does not match expected hash",
            i
        );
    }

    let sub_image_id = B256::from(words_to_bytes_le(&input.image_id));
    let agg_public_input_hash =
        shasta_aggregation_hash_for_zk(sub_image_id, &input.proof_carry_data_vec)
            .expect("invalid shasta proof carry data");

    ziskos::io::write(&agg_public_input_hash.0);

    // Close hints stream (flushes all pending hints)
    #[cfg(zisk_hints)]
    ziskos::hints::close_hints().expect("failed to close hints");
}
