pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    3527203587, 3267130343, 162472994, 3369435975, 2933762177, 3858582747, 248068170, 3677912328,
];
