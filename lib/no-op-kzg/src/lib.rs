//! No-op replacement for c-kzg library for ZISK zkVM
//! 
//! This library provides the same API as c-kzg but without C compilation,
//! making it suitable for bare-metal RISC-V environments like ZISK.
//! 
//! All functions panic or return errors since ZISK uses pure Rust KZG
//! implementation in raiko-lib instead.

use core::fmt;
use std::path::Path;

#[derive(Debug)]
pub struct Error {
    message: &'static str,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "c-kzg error: {}", self.message)
    }
}

impl std::error::Error for Error {}

impl Error {
    #[allow(non_snake_case)]
    pub fn MismatchLength(msg: String) -> Self {
        let _ = msg; // Ignore the message for now
        Self { message: "Length mismatch" }
    }
}

// Constants that c-kzg exports
pub const BYTES_PER_G1_POINT: usize = 48;
pub const BYTES_PER_G2_POINT: usize = 96;

// Wrapper types to enable impl blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KzgSettings(());

#[derive(Debug, Clone, Copy)]
pub struct KzgCommitment([u8; 48]);

#[derive(Debug, Clone, Copy)]
pub struct KzgProof([u8; 48]);

pub type Blob = [u8; 131072];
pub type Bytes32 = [u8; 32];
pub type Bytes48 = [u8; 48];

// Implement methods on the types like c-kzg does
impl KzgSettings {
    pub fn load_trusted_setup(_g1_points: &[[u8; 48]], _g2_points: &[[u8; 96]]) -> Result<Self, Error> {
        Ok(KzgSettings(()))
    }
    
    pub fn load_trusted_setup_file(_file_path: &Path) -> Result<Self, Error> {
        Ok(KzgSettings(()))
    }
}

impl KzgCommitment {
    pub fn blob_to_kzg_commitment(_blob: &Blob, _settings: &KzgSettings) -> Result<Self, Error> {
        panic!("c-kzg functions should not be called in ZISK - use pure Rust implementation")
    }
    
    pub fn to_bytes(&self) -> [u8; 48] {
        self.0
    }
    
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl From<[u8; 48]> for KzgCommitment {
    fn from(bytes: [u8; 48]) -> Self {
        KzgCommitment(bytes)
    }
}

impl KzgProof {
    pub fn compute_blob_kzg_proof(
        _blob: &Blob,
        _commitment: &[u8; 48],
        _settings: &KzgSettings
    ) -> Result<Self, Error> {
        panic!("c-kzg functions should not be called in ZISK - use pure Rust implementation")
    }
    
    pub fn verify_blob_kzg_proof_batch(
        _blobs: &[Blob],
        _commitments: &[Bytes48], 
        _proofs: &[Bytes48],
        _settings: &KzgSettings
    ) -> Result<bool, Error> {
        panic!("c-kzg functions should not be called in ZISK - use pure Rust implementation")
    }
    
    pub fn verify_kzg_proof(
        _commitment: &Bytes48,
        _z: &Bytes32,
        _y: &Bytes32,
        _proof: &Bytes48,
        _settings: &KzgSettings
    ) -> Result<bool, Error> {
        panic!("c-kzg functions should not be called in ZISK - use pure Rust implementation")
    }
    
    pub fn to_bytes(&self) -> [u8; 48] {
        self.0
    }
}

// Functions that would panic - these should never be called in ZISK
pub fn load_trusted_setup_file(_path: &str) -> Result<KzgSettings, Error> {
    panic!("c-kzg functions should not be called in ZISK - use pure Rust implementation")
}

pub fn blob_to_kzg_commitment(_blob: &Blob, _settings: &KzgSettings) -> Result<KzgCommitment, Error> {
    panic!("c-kzg functions should not be called in ZISK - use pure Rust implementation")  
}

pub fn compute_kzg_proof(
    _blob: &Blob,
    _z_bytes: &[u8; 32], 
    _settings: &KzgSettings
) -> Result<KzgProof, Error> {
    panic!("c-kzg functions should not be called in ZISK - use pure Rust implementation")
}

pub fn verify_kzg_proof(
    _commitment_bytes: &[u8; 48],
    _z_bytes: &[u8; 32],
    _y_bytes: &[u8; 32], 
    _proof_bytes: &[u8; 48],
    _settings: &KzgSettings
) -> Result<bool, Error> {
    panic!("c-kzg functions should not be called in ZISK - use pure Rust implementation")
}

// Re-export common items that might be expected
pub use Error as CkzgError;