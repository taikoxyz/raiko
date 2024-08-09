pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    4088744154, 2973055310, 1548081772, 1076887772, 3229391767, 1145789405, 2599692868, 3836658436,
];
