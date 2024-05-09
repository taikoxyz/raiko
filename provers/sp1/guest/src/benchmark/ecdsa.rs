#![no_main]
sp1_zkvm::entrypoint!(run);
use revm_precompile::zk_op::ZkvmOperator;
use zk::Sp1Operator;

fn run() {

    let sig_hi: [u8; 32] = sp1_zkvm::io::read();
    let sig_lo: [u8; 32] = sp1_zkvm::io::read();
    let mut recid: u8 = sp1_zkvm::io::read();
    let msg: [u8; 32] = sp1_zkvm::io::read();

    // parse signature
    let mut sig = [0u8; 64];
    sig[0..32].copy_from_slice(&sig_hi);
    sig[32..64].copy_from_slice(&sig_lo);

    let op = Sp1Operator{};
    let res = op.secp256k1_ecrecover(&sig, recid, &msg).unwrap();

    sp1_zkvm::io::commit(&res);

}
