pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    480687899, 2174307554, 3476860359, 3243974848, 2470546824, 868279999, 2732634741, 2619104561,
];
