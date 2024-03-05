#![no_main]
use risc0_zkvm::guest::env;
risc0_zkvm::guest::entry!(main);


use zeth_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy}, consts::{ChainSpec, TKO_MAINNET_CHAIN_SPEC},
    input::{self, Input},
    taiko::protocol_instance::{assemble_protocol_instance, EvidenceType},
    EthereumTxEssence
};
use zeth_primitives::{Address, B256, FixedBytes};

fn main() -> GuestOutput<32> {
    let input: Input<EthereumTxEssence> = env::read();

    let (header, _mpt_node) = TaikoStrategy::build_from(&input)
        .expect("Failed to build the resulting block");

    let pi = assemble_protocol_instance(&input, &header)
        .expect("Failed to assemble the protocol instance");
    let pi_hash = pi.instance_hash(EvidenceType::Succinct);

    GuestOutput { header }
}