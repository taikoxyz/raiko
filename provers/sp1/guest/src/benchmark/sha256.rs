#![no_main]
sp1_zkvm::entrypoint!(run);
use revm_precompile::zk_op::ZkvmOperator;
use zk::Sp1Operator;

fn run() {
    let input = sp1_zkvm::io::read::<[u8; 32]>();
    let op = Sp1Operator{};
    let res = op.sha256_run(&input).unwrap();

    sp1_zkvm::io::commit(&res);
}