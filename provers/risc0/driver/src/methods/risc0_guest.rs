pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    2426111784, 2252773481, 4093155148, 2853313326, 836865213, 1159934005, 790932950, 229907112,
];
