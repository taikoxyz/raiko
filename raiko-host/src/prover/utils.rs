use super::consts::RAIKO_GUEST_EXECUTABLE;

pub fn cache_file_path(cache_path: &str, block_no: u64, is_l1: bool) -> String {
    let prefix = if is_l1 { "l1" } else { "l2" };
    format!("{}/{}.{}.json.gz", cache_path, block_no, prefix)
}

pub fn guest_executable_path(guest_path: &str, proof_type: &str) -> String {
    format!("{}/{}/{}", guest_path, proof_type, RAIKO_GUEST_EXECUTABLE)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_file_prefix() {
        let path = std::path::Path::new("/tmp/ethereum/1234.l1.json.gz");
        let prefix = path.file_prefix().unwrap();
        assert_eq!(prefix, "1234");
    }
}
