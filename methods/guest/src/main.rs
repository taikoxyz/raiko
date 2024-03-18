#![no_main]
use risc0_zkvm::guest::env;
risc0_zkvm::guest::entry!(main);

use zeth_lib::{
    consts::{get_taiko_chain_spec, ChainSpec, ETH_MAINNET_CHAIN_SPEC},
    input::{Input, Risc0Input, Output},
    taiko::{
        protocol_instance::{assemble_protocol_instance, TaikoExtra, TaikoExtraForVM},
        block_builder::{TaikoBlockBuilder, TaikoStrategyBundle},
        blob_utils::{calc_blob_versioned_hash, decode_blob_data, zlib_decompress_blob},
    },
    EthereumTxEssence,
};
use zeth_primitives::{taiko::protocol_instance::EvidenceType, Address};



fn main() {
    // let input: u64 = env::read();
    let risc0_input: Risc0Input = env::read();
    let input: Input<EthereumTxEssence> = risc0_input.input;
    let extra_for_vm: TaikoExtraForVM = risc0_input.extra;
    let l2_chain_spec = get_taiko_chain_spec("internal_devnet_a");
    let output = TaikoBlockBuilder::build_from(&l2_chain_spec, input)
        .expect("Failed to build the resulting block");

    let extra: TaikoExtra = TaikoExtra {
        l1_hash: extra_for_vm.l1_hash,
        l1_height: extra_for_vm.l1_height,
        l2_tx_list: extra_for_vm.l2_tx_list,
        tx_blob_hash: extra_for_vm.tx_blob_hash,
        prover: extra_for_vm.prover,
        graffiti: extra_for_vm.graffiti,
        l2_withdrawals: extra_for_vm.l2_withdrawals,
        block_proposed: extra_for_vm.block_proposed,
        chain_id: extra_for_vm.chain_id,
        sgx_verifier_address: extra_for_vm.sgx_verifier_address,
        blob_data: extra_for_vm.blob_data,
        l1_next_block: Default::default(),
        l2_fini_block: Default::default(),

    };

    // let compressed_tx_list = decode_blob_data(&extra.blob_data);
    // let decompressed_tx_list = zlib_decompress_blob(&compressed_tx_list).unwrap_or_default();

    let blob_hash = extra.tx_blob_hash.unwrap().to_vec();
    assert_eq!(
        blob_hash.as_slice(),
        calc_blob_versioned_hash(&extra.blob_data).as_slice()
    );
    
    let pi = zeth_lib::taiko::protocol_instance::assemble_protocol_instance(&extra,&output).unwrap();
    let pi_hash = pi.hash(EvidenceType::Risc0);
    env::commit(&[pi_hash]);
}

use std::alloc::{alloc, handle_alloc_error};
use std::io::Read;
use std::{alloc::Layout, ffi::c_void};

#[no_mangle]
// TODO ideally this is c_size_t, but not stabilized (not guaranteed to be usize on all archs)
unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    let layout = Layout::from_size_align(size, 4).expect("unable to allocate more memory");
    let ptr = alloc(layout);

    if ptr.is_null() {
        handle_alloc_error(layout);
    }

    ptr as *mut c_void
}

#[no_mangle]
// TODO shouldn't need to zero allocated bytes since the zkvm memory is zeroed, might want to zero anyway
unsafe extern "C" fn calloc(size: usize) -> *mut c_void {
    malloc(size)
}

#[no_mangle]
unsafe extern "C" fn free(_size: *const c_void) {
    // Intentionally a no-op, since the zkvm allocator is a bump allocator
}

