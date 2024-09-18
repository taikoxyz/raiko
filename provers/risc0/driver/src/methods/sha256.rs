pub const SHA256_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/sha256");
pub const SHA256_ID: [u32; 8] = [
    2716313044, 1330228279, 3217062305, 3693970552, 3654025276, 4270078228, 3722528174, 3151280396,
];
