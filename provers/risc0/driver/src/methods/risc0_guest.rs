pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    2099256191, 2718055137, 279178920, 3515214460, 3279826300, 3596190255, 2265684480, 978122828,
];
