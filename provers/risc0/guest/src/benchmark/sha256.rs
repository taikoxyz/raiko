#![no_main]
risc0_zkvm::guest::entry!(run);
use risc0_zkvm::guest::env;

use revm_precompile::zk_op::ZkvmOperator;
use zk::Risc0Operator;

fn run() {
    let input: [u8; 32] = env::read();
    let op = Risc0Operator{};
    let res = op.sha256_run(&input).unwrap();

    env::commit::<[u8; 32]>(&res);
}