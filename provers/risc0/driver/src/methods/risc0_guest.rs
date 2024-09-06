pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    3668986418, 864756305, 2034163314, 3234293955, 2343455451, 1929078992, 2810820419, 2620184861,
];
