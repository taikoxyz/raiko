pub const ECDSA_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/ecdsa");
pub const ECDSA_ID: [u32; 8] = [
    3210704183, 2476344651, 1483125302, 3595451496, 516702876, 1076221675, 3247113162, 2941986366,
];
