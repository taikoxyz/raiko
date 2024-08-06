pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    4175756782, 445162214, 4177377390, 1094180202, 3909523218, 3532437525, 1083104193, 3682235852,
];
