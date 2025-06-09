pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest.bin");
pub const RISC0_GUEST_ID: [u32; 8] = [
    2884957422, 3999345994, 3108961590, 1071903963, 1708621478, 2126228364, 1634567936, 1029991411,
];
