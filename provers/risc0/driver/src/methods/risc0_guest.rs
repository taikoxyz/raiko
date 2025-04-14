pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest.bin");
pub const RISC0_GUEST_ID: [u32; 8] = [
    1095459983, 1406338564, 3500532327, 2251670090, 1544526449, 3367186052, 2772914985, 2149869005,
];
