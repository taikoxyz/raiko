#![no_main]
ziskos::entrypoint!(main);

mod zisk_crypto;

use std::sync::Arc;

use raiko_lib::{
    builder::calculate_batch_blocks_final_header, input::GuestBatchInput, proof_type::ProofType,
    protocol_instance::ProtocolInstance,
};

pub fn main() {
    // Route all crypto through ZiskCrypto high-level syscalls instead of the
    // k256 patch field-op path. The k256 patch uses many individual
    // arith256_mod + secp256k1_add/dbl calls which trigger a ZisK prover bug
    // ("Fixed MT verification failed" / "VerifyEvaluations0" ~8% of the time).
    // ZiskCrypto uses single high-level C calls (e.g. secp256k1_ecdsa_address_recover_c)
    // that avoid the problematic precompile pattern.
    raiko_lib::revm::install_crypto(zisk_crypto::ZiskCrypto);
    let crypto = Arc::new(zisk_crypto::ZiskCrypto);
    raiko_lib::alloy_consensus::crypto::install_default_provider(crypto.clone())
        .expect("crypto provider already set");

    // Initialize hints stream (native build only — emits precompile hint requests)
    #[cfg(zisk_hints)]
    ziskos::hints::init_hints_file("/tmp/zisk-hints.bin".into(), None)
        .expect("failed to init hints");

    // Read the batch input data from ziskos
    let input_data = ziskos::io::read_vec();

    // Deserialize the batch input using the standard GuestBatchInput format
    let mut batch_input: GuestBatchInput =
        bincode::deserialize(&input_data).expect("failed to deserialize GuestBatchInput");

    // This executes all transactions and validates state transitions
    let final_blocks = calculate_batch_blocks_final_header(&mut batch_input);

    // Create protocol instance from executed blocks
    let protocol_instance =
        ProtocolInstance::new_batch(&batch_input, final_blocks, ProofType::Zisk)
            .expect("failed to build Zisk protocol instance");

    // Get the instance hash in LE uint32 format for zisk publics
    let instance_hash = protocol_instance.instance_hash_le();
    ziskos::io::write(&instance_hash.0);

    // Close hints stream (flushes all pending hints)
    #[cfg(zisk_hints)]
    ziskos::hints::close_hints().expect("failed to close hints");
}
