pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    3081612924, 3607021821, 3447357908, 4021531326, 181386186, 1121032291, 3850993439, 1990175839,
];
