pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest.bin");
pub const RISC0_GUEST_ID: [u32; 8] = [
    3545989516, 3928336881, 3715721783, 15987746, 335467095, 2128491358, 1997626621, 279656044,
];
