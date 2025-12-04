#![no_main]
harness::entrypoint!(main);
use raiko_lib::CycleTracker;
use revm_precompile::zk_op::ZkvmOperator;
use std::hint::black_box;
use zk_op::Sp1Operator;

fn main() {
    let input: [u8; 96] = black_box([
        24, 177, 138, 207, 180, 194, 195, 2, 118, 219, 84, 17, 54, 142, 113, 133, 179, 17, 221, 18,
        70, 145, 97, 12, 93, 59, 116, 3, 78, 9, 61, 201, 6, 60, 144, 156, 71, 32, 132, 12, 181, 19,
        76, 185, 245, 159, 167, 73, 117, 87, 150, 129, 150, 88, 211, 46, 252, 13, 40, 129, 152,
        243, 114, 102, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 5,
    ]);

    let op = Sp1Operator {};

    let ct = CycleTracker::start("bn128_run_mul");
    let res = op.bn128_run_mul(&input).unwrap();
    ct.end();

    let hi = res[..32].to_vec();
    let lo = res[32..].to_vec();
    sp1_zkvm::io::commit(&hi);
    sp1_zkvm::io::commit(&lo);
}
