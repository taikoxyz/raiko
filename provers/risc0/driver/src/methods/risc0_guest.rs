pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    4100500636, 2940851050, 449541811, 2971637610, 767229547, 1377210058, 4158545120, 1337570084,
];
