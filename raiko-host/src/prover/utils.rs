use super::constant::RAIKO_GUEST_EXECUTABLE;

pub fn cache_file_path(cache_path: &str, block_no: u64) -> String {
    format!("{}/{}.json.gz", cache_path, block_no)
}

pub fn guest_executable_path(guest_path: &str, proof_type: &str) -> String {
    format!("{}/{}/{}", guest_path, proof_type, RAIKO_GUEST_EXECUTABLE)
}
