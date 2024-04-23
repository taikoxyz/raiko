include!(concat!(env!("OUT_DIR"), "/test.rs"));

#[test]
fn test_guest_list() {
    println!("elf code length: {}", RISC0_METHODS_TEST_ELF.len());
}
