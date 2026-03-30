//! Aggregates multiple batch proofs (verification handled by the host).

#![no_main]
ziskos::entrypoint!(main);

mod precompile_shims;
mod ruint_shims;

use raiko_lib::{
    input::ZkAggregationGuestInput,
    primitives::B256,
    protocol_instance::{aggregation_output, words_to_bytes_le},
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

    // Read the aggregation input data from ziskos
    let input_data = ziskos::io::read_vec();
    assert!(!input_data.is_empty(), "aggregation input is empty");

    // Deserialize input using the standard ZkAggregationGuestInput format
    let input: ZkAggregationGuestInput =
        bincode::deserialize(&input_data).expect("failed to deserialize ZkAggregationGuestInput");

    assert!(
        !input.block_inputs.is_empty(),
        "aggregation input has no block inputs"
    );

    // Use the same aggregation_output function for consistency
    let program_id = B256::from(words_to_bytes_le(&input.image_id));
    let aggregated_output = aggregation_output(program_id, input.block_inputs.clone());

    // Commit the aggregation output as public output
    ziskos::io::write(&aggregated_output);

    // Close hints stream (flushes all pending hints)
    #[cfg(zisk_hints)]
    ziskos::hints::close_hints().expect("failed to close hints");
}
