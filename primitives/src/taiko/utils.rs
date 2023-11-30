pub fn string_to_bytes32(input: &[u8]) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(input);
    bytes
}
