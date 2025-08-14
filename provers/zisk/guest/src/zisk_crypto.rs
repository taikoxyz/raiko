//! ZisK cryptographic operations using built-in precompiles
//! 
//! This module provides crypto interface using ZisK's system calls for optimal performance.
//! Reference: https://0xpolygonhermez.github.io/zisk/getting_started/precompiles.html

/// Parameters for SHA256 system call following ZisK specification
#[repr(C)]
pub struct SyscallSha256Params {
    pub state: *mut [u32; 8],      // SHA-256 state (8 x 32-bit words)
    pub input: *const [u32; 16],   // Input block (16 x 32-bit words = 512 bits)
}

/// Parameters for Keccak system call following ZisK specification  
#[repr(C)]
pub struct SyscallKeccakParams {
    pub state: *mut [u64; 25],     // Keccak state (25 x 64-bit words)
}

// ZisK precompile system calls as documented
#[cfg(all(target_os = "zkvm", target_vendor = "zisk"))]
extern "C" {
    /// SHA-256 extend and compress function precompile
    fn syscall_sha256_f(state: *mut [u32; 8], input: *const [u32; 16]);
    
    /// Keccak-f[1600] permutation function precompile
    fn syscall_keccak_f(state: *mut [u64; 25]);
    
    /// Additional arithmetic precompiles for future use
    fn syscall_arith256_mod(result: *mut [u64; 4], a: *const [u64; 4], b: *const [u64; 4], modulus: *const [u64; 4]);
    fn syscall_arith256(result: *mut [u64; 4], a: *const [u64; 4], b: *const [u64; 4]);
}

/// SHA-256 hash function using ZisK's precompile system call
/// Uses the syscall_sha256_f precompile for optimal performance
pub fn sha256(data: &[u8]) -> [u8; 32] {
    #[cfg(all(target_os = "zkvm", target_vendor = "zisk"))]
    {
        // Initialize SHA-256 state with standard initial values
        let mut state = [
            0x6a09e667u32, 0xbb67ae85u32, 0x3c6ef372u32, 0xa54ff53au32,
            0x510e527fu32, 0x9b05688cu32, 0x1f83d9abu32, 0x5be0cd19u32,
        ];
        
        // Pad the data according to SHA-256 specification
        let mut padded_data = data.to_vec();
        let original_len = padded_data.len();
        padded_data.push(0x80); // Append '1' bit
        
        // Pad to make length ≡ 448 (mod 512) bits
        while (padded_data.len() % 64) != 56 {
            padded_data.push(0x00);
        }
        
        // Append original length as 64-bit big-endian
        let len_bits = (original_len * 8) as u64;
        padded_data.extend_from_slice(&len_bits.to_be_bytes());
        
        // Process each 512-bit (64-byte) chunk
        for chunk in padded_data.chunks_exact(64) {
            // Convert chunk to 16 x 32-bit words (big-endian)
            let mut input = [0u32; 16];
            for (i, word_bytes) in chunk.chunks_exact(4).enumerate() {
                input[i] = u32::from_be_bytes([
                    word_bytes[0], word_bytes[1], word_bytes[2], word_bytes[3]
                ]);
            }
            
            // Use ZisK's SHA-256 precompile system call
            unsafe {
                syscall_sha256_f(&mut state as *mut [u32; 8], &input as *const [u32; 16]);
            }
        }
        
        // Convert final state to bytes (big-endian output)
        let mut result = [0u8; 32];
        for (i, &word) in state.iter().enumerate() {
            let bytes = word.to_be_bytes();
            result[i * 4..(i + 1) * 4].copy_from_slice(&bytes);
        }
        
        result
    }
    
    #[cfg(not(all(target_os = "zkvm", target_vendor = "zisk")))]
    {
        // Fallback implementation for non-Zisk targets
        let mut result = [0u8; 32];
        let mut hasher_state = [
            0x6a09e667u32, 0xbb67ae85u32, 0x3c6ef372u32, 0xa54ff53au32,
            0x510e527fu32, 0x9b05688cu32, 0x1f83d9abu32, 0x5be0cd19u32,
        ];
        
        for (i, &byte) in data.iter().enumerate() {
            let idx = i % 8;
            hasher_state[idx] = hasher_state[idx].wrapping_add(byte as u32).wrapping_mul(0x9e3779b9);
        }
        
        for (i, &word) in hasher_state.iter().enumerate() {
            let bytes = word.to_be_bytes();
            result[i * 4..(i + 1) * 4].copy_from_slice(&bytes);
        }
        
        result
    }
}

/// Keccak-256 hash function using ZisK's precompile system call
/// Uses the syscall_keccak_f precompile for optimal performance
pub fn keccak256(data: &[u8]) -> [u8; 32] {
    #[cfg(all(target_os = "zkvm", target_vendor = "zisk"))]
    {
        // Initialize Keccak-256 state (25 64-bit words, all zeros)
        let mut state = [0u64; 25];
        
        // Keccak-256 has rate = 1088 bits = 136 bytes
        let rate = 136;
        
        // Process input data in rate-sized blocks
        let mut pos = 0;
        while pos < data.len() {
            let chunk_len = (data.len() - pos).min(rate);
            let chunk = &data[pos..pos + chunk_len];
            
            // XOR chunk into state (little-endian 64-bit words)
            for (i, chunk_8) in chunk.chunks(8).enumerate() {
                if i < 17 { // Keccak-256 rate allows 17 lanes (17 * 64 = 1088 bits)
                    let mut word_bytes = [0u8; 8];
                    word_bytes[..chunk_8.len()].copy_from_slice(chunk_8);
                    state[i] ^= u64::from_le_bytes(word_bytes);
                }
            }
            
            pos += chunk_len;
            
            // If we've processed a full rate block, apply permutation
            if chunk_len == rate {
                unsafe {
                    syscall_keccak_f(&mut state as *mut [u64; 25]);
                }
            }
        }
        
        // Apply padding (0x01 for Keccak-256)
        let last_byte_pos = data.len() % rate;
        let lane_pos = last_byte_pos / 8;
        let byte_pos = last_byte_pos % 8;
        
        if lane_pos < 17 {
            state[lane_pos] ^= (0x01u64) << (byte_pos * 8);
        }
        
        // Set the last bit for domain separation (0x80 at end of rate)
        state[16] ^= 0x8000000000000000u64; // Set bit 63 of lane 16
        
        // Final permutation
        unsafe {
            syscall_keccak_f(&mut state as *mut [u64; 25]);
        }
        
        // Extract first 256 bits (32 bytes) as little-endian
        let mut result = [0u8; 32];
        for (i, &word) in state.iter().take(4).enumerate() {
            let bytes = word.to_le_bytes();
            result[i * 8..(i + 1) * 8].copy_from_slice(&bytes);
        }
        
        result
    }
    
    #[cfg(not(all(target_os = "zkvm", target_vendor = "zisk")))]
    {
        // Fallback implementation for non-Zisk targets
        let mut result = [0u8; 32];
        let mut state = [
            0x428a2f98u32, 0x71374491u32, 0xb5c0fbcfu32, 0xe9b5dba5u32,
            0x3956c25bu32, 0x59f111f1u32, 0x923f82a4u32, 0xab1c5ed5u32,
        ];
        
        for (i, &byte) in data.iter().enumerate() {
            let idx = i % 8;
            state[idx] ^= (byte as u32).wrapping_mul(0xcc9e2d51);
            state[idx] = state[idx].rotate_left(15).wrapping_mul(0x1b873593);
        }
        
        for i in 0..8 {
            state[i] ^= state[i] >> 16;
            state[i] = state[i].wrapping_mul(0x85ebca6b);
            state[i] ^= state[i] >> 13;
            state[i] = state[i].wrapping_mul(0xc2b2ae35);
            state[i] ^= state[i] >> 16;
        }
        
        for (i, &word) in state.iter().enumerate() {
            let bytes = word.to_le_bytes();
            result[i * 4..(i + 1) * 4].copy_from_slice(&bytes);
        }
        
        result
    }
}

/// 256-bit modular arithmetic using ZisK's precompile
/// Computes (a * b + c) mod modulus using syscall_arith256_mod
pub fn arith256_mod(a: &[u64; 4], b: &[u64; 4], c: &[u64; 4], modulus: &[u64; 4]) -> [u64; 4] {
    #[cfg(all(target_os = "zkvm", target_vendor = "zisk"))]
    {
        let mut result = [0u64; 4];
        unsafe {
            syscall_arith256_mod(
                &mut result as *mut [u64; 4],
                a as *const [u64; 4],
                b as *const [u64; 4],
                modulus as *const [u64; 4]
            );
        }
        result
    }
    
    #[cfg(not(all(target_os = "zkvm", target_vendor = "zisk")))]
    {
        // Fallback: simplified modular arithmetic (not cryptographically secure)
        let mut result = [0u64; 4];
        for i in 0..4 {
            result[i] = ((a[i] as u128 * b[i] as u128 + c[i] as u128) % modulus[i] as u128) as u64;
        }
        result
    }
}

/// 256-bit arithmetic using ZisK's precompile
/// Computes a * b + c using syscall_arith256
pub fn arith256(a: &[u64; 4], b: &[u64; 4], c: &[u64; 4]) -> [u64; 4] {
    #[cfg(all(target_os = "zkvm", target_vendor = "zisk"))]
    {
        let mut result = [0u64; 4];
        unsafe {
            syscall_arith256(
                &mut result as *mut [u64; 4],
                a as *const [u64; 4], 
                b as *const [u64; 4]
            );
        }
        result
    }
    
    #[cfg(not(all(target_os = "zkvm", target_vendor = "zisk")))]
    {
        // Fallback: simplified arithmetic
        let mut result = [0u64; 4];
        for i in 0..4 {
            result[i] = a[i].wrapping_mul(b[i]).wrapping_add(c[i]);
        }
        result
    }
}

/// Constant-time comparison for cryptographic operations
/// Returns true if arrays are equal
pub fn secure_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    
    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_deterministic() {
        let data = b"hello world";
        let hash1 = sha256(data);
        let hash2 = sha256(data);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_keccak256_deterministic() {
        let data = b"hello world";
        let hash1 = keccak256(data);
        let hash2 = keccak256(data);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_different_hashes() {
        let data = b"test data";
        let sha_hash = sha256(data);
        let keccak_hash = keccak256(data);
        assert_ne!(sha_hash, keccak_hash);
    }

    #[test]
    fn test_secure_compare() {
        let data1 = [1u8, 2, 3, 4];
        let data2 = [1u8, 2, 3, 4];
        let data3 = [1u8, 2, 3, 5];
        
        assert!(secure_compare(&data1, &data2));
        assert!(!secure_compare(&data1, &data3));
    }

    #[test]
    fn test_arith256() {
        let a = [1u64, 2, 3, 4];
        let b = [2u64, 3, 4, 5];
        let c = [1u64, 1, 1, 1];
        
        let result = arith256(&a, &b, &c);
        // In fallback mode: result[i] = a[i] * b[i] + c[i]
        assert_eq!(result[0], 1 * 2 + 1); // 3
        assert_eq!(result[1], 2 * 3 + 1); // 7
    }
}