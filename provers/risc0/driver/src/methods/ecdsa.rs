pub const ECDSA_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/ecdsa");
pub const ECDSA_ID: [u32; 8] = [
    3688490884, 2127892678, 3137078981, 1193344426, 4105663218, 3901516424, 3225864022, 13950036,
];
