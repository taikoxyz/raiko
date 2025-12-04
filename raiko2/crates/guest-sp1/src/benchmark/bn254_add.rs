#![no_main]
harness::entrypoint!(main);
use raiko_lib::CycleTracker;
use revm_precompile::zk_op::ZkvmOperator;
use std::hint::black_box;
use zk_op::Sp1Operator;

fn main() {
    let input: [u8; 128] = black_box([
        24, 177, 138, 207, 180, 194, 195, 2, 118, 219, 84, 17, 54, 142, 113, 133, 179, 17, 221, 18,
        70, 145, 97, 12, 93, 59, 116, 3, 78, 9, 61, 201, 6, 60, 144, 156, 71, 32, 132, 12, 181, 19,
        76, 185, 245, 159, 167, 73, 117, 87, 150, 129, 150, 88, 211, 46, 252, 13, 40, 129, 152,
        243, 114, 102, 7, 194, 183, 245, 138, 132, 189, 97, 69, 240, 12, 156, 43, 192, 187, 26, 24,
        127, 32, 255, 44, 146, 150, 58, 136, 1, 158, 124, 106, 1, 78, 237, 6, 97, 78, 32, 193, 71,
        233, 64, 242, 215, 13, 163, 247, 76, 154, 23, 223, 54, 23, 6, 164, 72, 92, 116, 43, 214,
        120, 132, 120, 250, 23, 215,
    ]);

    let op = Sp1Operator {};

    let ct = CycleTracker::start("bn128_run_add");
    let res = op.bn128_run_add(&input).unwrap();
    ct.end();

    let hi = res[..32].to_vec();
    let lo = res[32..].to_vec();

    // Longer than 32 bit register
    sp1_zkvm::io::commit(&hi);
    sp1_zkvm::io::commit(&lo);
}
