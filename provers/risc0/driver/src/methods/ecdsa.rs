pub const ECDSA_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/ecdsa");
pub const ECDSA_ID: [u32; 8] = [
    3314277365, 903638368, 2823387338, 975292771, 2962241176, 3386670094, 1262198564, 423457744,
];
