pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    2724640415, 1388818056, 2370444677, 1329173777, 2657825669, 1524407056, 1629931902, 314750851,
];
