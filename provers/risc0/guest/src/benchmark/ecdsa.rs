#![no_main]
risc0_zkvm::guest::entry!(run);
use risc0_zkvm::guest::env;

use revm_precompile::zk_op::ZkvmOperator;
use zk::Risc0Operator;

fn run() {

    let sig_hi: [u8; 32] = env::read();
    let sig_lo: [u8; 32] = env::read();
    let mut recid: u8 = env::read();
    let msg: [u8; 32] = env::read();

    // parse signature
    let mut sig = [0u8; 64];
    sig[0..32].copy_from_slice(&sig_hi);
    sig[32..64].copy_from_slice(&sig_lo);

    let op = Risc0Operator{};
    let res = op.secp256k1_ecrecover(&sig, recid, &msg).unwrap();

    env::commit::<[u8; 32]>(&res);

}
