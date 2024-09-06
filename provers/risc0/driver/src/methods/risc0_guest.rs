pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    2903229055, 3793750968, 1852917165, 927627463, 2879457596, 4006859083, 3898157589, 3630990634,
];
