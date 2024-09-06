pub const ECDSA_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/ecdsa");
pub const ECDSA_ID: [u32; 8] = [
    280072277, 3449831886, 1373009816, 2385564905, 1152759676, 1992023991, 393821110, 566805524,
];
