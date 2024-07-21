pub const SHA256_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/sha256");
pub const SHA256_ID: [u32; 8] = [
    2276776674, 1530092210, 953274986, 4292586102, 1671654623, 3605429373, 2703161450, 1602935363,
];
