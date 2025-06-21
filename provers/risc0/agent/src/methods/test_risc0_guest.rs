pub const TEST_RISC0_GUEST_ELF: &[u8] = include_bytes!(
    "../../../guest/target/riscv32im-risc0-zkvm-elf/release/deps/risc0_guest-ec21105eb19a1b6f.bin"
);
pub const TEST_RISC0_GUEST_ID: [u32; 8] = [
    125096490, 2519681774, 4263816311, 3060211224, 655017739, 1014246946, 859911810, 2337502337,
];
