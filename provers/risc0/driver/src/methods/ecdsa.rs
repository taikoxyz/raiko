pub const ECDSA_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/ecdsa");
pub const ECDSA_ID: [u32; 8] = [
    2403615864, 3944723361, 1721066448, 3334617775, 1874245592, 1853933074, 3347288103, 3484988018,
];
