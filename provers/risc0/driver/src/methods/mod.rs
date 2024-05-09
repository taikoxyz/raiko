cfg_if::cfg_if! {
    if #[cfg(feature = "dummy-elf")] {
        pub mod risc0_guest {
            pub const RISC0_GUEST_ELF: &[u8] = &[1,1,1,1];
            pub const RISC0_GUEST_ID: [u32; 8] = [
                3882122475, 907119021, 1372741864, 753360312, 3895311118, 3935171204, 3455168641, 1953052349,
            ];
            pub const RISC0_GUEST_PATH: &str =
                r#"/home/ubuntu/raiko/provers/risc0/guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest"#;
        }
        pub mod test_risc0_guest {
            pub const TEST_RISC0_GUEST_ELF: &[u8] = &[1,1,1,1];
            pub const TEST_RISC0_GUEST_ID: [u32; 8] = [
                4062756167, 3269589994, 322973908, 2704116299, 4279699347, 2052262230, 1777782203, 1042194693,
            ];
            pub const TEST_RISC0_GUEST_PATH: &str = r#"provers/risc0/guest/target/riscv32im-risc0-zkvm-elf/release/deps/risc0_guest-696e5094d99ac2bd"#;
        }
    } else {
        pub mod risc0_guest;
        pub mod test_risc0_guest;
        pub mod ecdsa;
        pub mod sha256;
    }
}
