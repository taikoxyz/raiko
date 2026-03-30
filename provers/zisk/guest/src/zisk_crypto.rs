//! ZisK-native ecrecover via the ziskos high-level syscall.
//!
//! The k256 patch routes ecrecover through ~500+ `arith256_mod` syscalls
//! (one per field operation).  This replaces that path with a single call to
//! `secp256k1_ecdsa_address_recover_c`, which is exported by ziskos v0.16.0
//! and performs the full recover in one high-level operation — drastically
//! reducing the ROM size and proving time.
//!
//! All other patches (sha2, sha3, tiny-keccak, ruint) remain in place.

#[cfg(all(not(all(target_os = "zkvm", target_vendor = "zisk")), zisk_hints_debug))]
use std::os::raw::c_char;

#[cfg(all(target_os = "zkvm", target_vendor = "zisk"))]
extern "C" {
    pub fn sha256_c(input: *const u8, input_len: usize, output: *mut u8);

    pub fn bn254_g1_add_c(p1: *const u8, p2: *const u8, ret: *mut u8) -> u8;

    pub fn bn254_g1_mul_c(point: *const u8, scalar: *const u8, ret: *mut u8) -> u8;

    pub fn bn254_pairing_check_c(pairs: *const u8, num_pairs: usize) -> u8;

    pub fn secp256k1_ecdsa_verify_and_address_recover_c(
        sig: *const u8,
        msg: *const u8,
        pk: *const u8,
        output: *mut u8,
    ) -> u8;

    pub fn secp256k1_ecdsa_address_recover_c(
        sig: *const u8,
        recid: u8,
        msg: *const u8,
        output: *mut u8,
    ) -> u8;

    pub fn modexp_bytes_c(
        base_ptr: *const u8,
        base_len: usize,
        exp_ptr: *const u8,
        exp_len: usize,
        modulus_ptr: *const u8,
        modulus_len: usize,
        ret_ptr: *mut u8,
    ) -> usize;

    pub fn blake2b_compress_c(rounds: u32, h: *mut u64, m: *const u64, t: *const u64, f: u8);

    pub fn secp256r1_ecdsa_verify_c(msg: *const u8, sig: *const u8, pk: *const u8) -> bool;

    pub fn verify_kzg_proof_c(
        z: *const u8,
        y: *const u8,
        commitment: *const u8,
        proof: *const u8,
    ) -> bool;

    pub fn bls12_381_g1_add_c(ret: *mut u8, a: *const u8, b: *const u8) -> u8;

    pub fn bls12_381_g1_msm_c(ret: *mut u8, pairs: *const u8, num_pairs: usize) -> u8;

    pub fn bls12_381_g2_add_c(ret: *mut u8, a: *const u8, b: *const u8) -> u8;

    pub fn bls12_381_g2_msm_c(ret: *mut u8, pairs: *const u8, num_pairs: usize) -> u8;

    pub fn bls12_381_pairing_check_c(pairs: *const u8, num_pairs: usize) -> u8;

    pub fn bls12_381_fp_to_g1_c(ret: *mut u8, fp: *const u8) -> u8;

    pub fn bls12_381_fp2_to_g2_c(ret: *mut u8, fp2: *const u8) -> u8;
}

#[cfg(all(not(all(target_os = "zkvm", target_vendor = "zisk")), zisk_hints))]
extern "C" {
    pub fn hint_sha256(f: *const u8, len: usize);

    pub fn hint_bn254_g1_add(p1: *const u8, p2: *const u8);

    pub fn hint_bn254_g1_mul(point: *const u8, scalar: *const u8);

    pub fn hint_bls12_381_g1_add(a: *const u8, b: *const u8);

    pub fn hint_bls12_381_g2_add(a: *const u8, b: *const u8);

    pub fn hint_secp256k1_ecdsa_verify_and_address_recover(
        sig: *const u8,
        msg: *const u8,
        pk: *const u8,
    );

    pub fn hint_secp256k1_ecdsa_address_recover(sig: *const u8, recid: *const u8, msg: *const u8);

    pub fn hint_modexp_bytes(
        base_ptr: *const u8,
        base_len: usize,
        exp_ptr: *const u8,
        exp_len: usize,
        modulus_ptr: *const u8,
        modulus_len: usize,
    );

    pub fn hint_blake2b_compress(rounds: u32, h: *mut u64, m: *const u64, t: *const u64, f: u8);

    pub fn hint_secp256r1_ecdsa_verify(msg: *const u8, sig: *const u8, pk: *const u8);

    pub fn hint_verify_kzg_proof(
        z: *const u8,
        y: *const u8,
        commitment: *const u8,
        proof: *const u8,
    );

    pub fn hint_bn254_pairing_check(pairs: *const u8, num_pairs: usize);

    pub fn hint_bls12_381_g1_msm(pairs: *const u8, num_pairs: usize);

    pub fn hint_bls12_381_g2_msm(pairs: *const u8, num_pairs: usize);

    pub fn hint_bls12_381_pairing_check(pairs: *const u8, num_pairs: usize);

    pub fn hint_bls12_381_fp_to_g1(fp: *const u8);

    pub fn hint_bls12_381_fp2_to_g2(fp2: *const u8);

    pub fn pause_hints() -> bool;

    pub fn resume_hints();
}

#[cfg(all(not(all(target_os = "zkvm", target_vendor = "zisk")), zisk_hints_debug))]
extern "C" {
    pub fn hint_log_c(msg: *const c_char);
}

use raiko_lib::k256::ecdsa::signature::hazmat::PrehashVerifier;
use raiko_lib::k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use raiko_lib::revm::precompile::DefaultCrypto;
use raiko_lib::tiny_keccak::{Hasher, Keccak};

use raiko_lib::alloy_consensus::crypto::{CryptoProvider, RecoveryError};
use raiko_lib::alloy_primitives::Address;

use raiko_lib::revm::precompile::{
    bls12_381::{G1Point, G1PointScalar, G2Point, G2PointScalar},
    Crypto, PrecompileError,
};

/// Unit struct wired into `revm_precompile::install_crypto` to dispatch all
/// precompile crypto operations through ziskos syscalls (on zkvm target) or
/// hint + native fallback (on native target).
#[derive(Clone, Debug)]
pub struct ZiskCrypto;

#[cfg(zisk_hints_debug)]
pub fn hint_log<S: AsRef<str>>(msg: S) {
    // On native we call external C function to log hints, since it controls if hints are paused or not
    #[cfg(not(all(target_os = "zkvm", target_vendor = "zisk")))]
    {
        use std::ffi::CString;

        if let Ok(c) = CString::new(msg.as_ref()) {
            unsafe { hint_log_c(c.as_ptr()) };
        }
    }
    // On zkvm/zisk, we can just print directly
    #[cfg(all(target_os = "zkvm", target_vendor = "zisk"))]
    {
        println!("{}", msg.as_ref());
    }
}

impl Crypto for ZiskCrypto {
    /// Compute SHA-256 hash
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    #[inline]
    fn sha256(&self, input: &[u8]) -> [u8; 32] {
        DefaultCrypto.sha256(input)
    }

    // /// Compute RIPEMD-160 hash
    // #[inline]
    // fn ripemd160(&self, input: &[u8]) -> [u8; 32] {
    //     use ripemd::Digest;
    //     let mut hasher = ripemd::Ripemd160::new();
    //     hasher.update(input);

    //     let mut output = [0u8; 32];
    //     hasher.finalize_into((&mut output[12..]).into());
    //     output
    // }

    /// BN254 elliptic curve addition.
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    #[inline]
    fn bn254_g1_add(&self, p1: &[u8], p2: &[u8]) -> Result<[u8; 64], PrecompileError> {
        DefaultCrypto.bn254_g1_add(p1, p2)
    }

    /// BN254 elliptic curve scalar multiplication.
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    #[inline]
    fn bn254_g1_mul(&self, point: &[u8], scalar: &[u8]) -> Result<[u8; 64], PrecompileError> {
        DefaultCrypto.bn254_g1_mul(point, scalar)
    }

    /// BN254 pairing check.
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    #[inline]
    fn bn254_pairing_check(&self, pairs: &[(&[u8], &[u8])]) -> Result<bool, PrecompileError> {
        DefaultCrypto.bn254_pairing_check(pairs)
    }

    /// secp256k1 ECDSA signature recovery.
    #[inline]
    fn secp256k1_ecrecover(
        &self,
        sig: &[u8; 64],
        recid: u8,
        msg: &[u8; 32],
    ) -> Result<[u8; 32], PrecompileError> {
        #[cfg(any(all(target_os = "zkvm", target_vendor = "zisk"), zisk_hints))]
        {
            #[cfg(zisk_hints)]
            unsafe {
                let recid_bytes = (recid as u64).to_le_bytes();
                hint_secp256k1_ecdsa_address_recover(
                    sig.as_ptr(),
                    recid_bytes.as_ptr(),
                    msg.as_ptr(),
                );
            }

            #[cfg(zisk_hints_debug)]
            {
                let recid_bytes = (recid as u64).to_le_bytes();
                hint_log(format!(
                    "hint_secp256k1_ecdsa_address_recover (sig: {:x?}, recid: {:x?}, msg: {:x?})",
                    &sig, &recid_bytes, &msg
                ));
            }

            #[cfg(all(target_os = "zkvm", target_vendor = "zisk"))]
            {
                let mut output = [0u8; 32];
                let ret = unsafe {
                    secp256k1_ecdsa_address_recover_c(
                        sig.as_ptr(),
                        recid,
                        msg.as_ptr(),
                        output.as_mut_ptr(),
                    )
                };
                match ret {
                    0 => Ok(output),
                    _ => Err(PrecompileError::Secp256k1RecoverFailed),
                }
            }
        }

        #[cfg(not(all(target_os = "zkvm", target_vendor = "zisk")))]
        {
            // Pause hint emission here so default_crypto.secp256k1_ecrecover cannot produce extra hints (e.g. keccak256)
            #[cfg(zisk_hints)]
            let already_paused = unsafe { pause_hints() };

            let result = DefaultCrypto.secp256k1_ecrecover(sig, recid, msg);

            #[cfg(zisk_hints)]
            {
                if !already_paused {
                    unsafe { resume_hints() };
                }
            }

            result
        }
    }

    /// Modular exponentiation.
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    #[inline]
    fn modexp(&self, base: &[u8], exp: &[u8], modulus: &[u8]) -> Result<Vec<u8>, PrecompileError> {
        DefaultCrypto.modexp(base, exp, modulus)
    }

    /// Blake2 compression function.
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    #[inline]
    fn blake2_compress(&self, rounds: u32, h: &mut [u64; 8], m: [u64; 16], t: [u64; 2], f: bool) {
        DefaultCrypto.blake2_compress(rounds, h, m, t, f);
    }

    /// secp256r1 (P-256) signature verification.
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    #[inline]
    fn secp256r1_verify_signature(&self, msg: &[u8; 32], sig: &[u8; 64], pk: &[u8; 64]) -> bool {
        #[cfg(any(all(target_os = "zkvm", target_vendor = "zisk"), zisk_hints))]
        {
            #[cfg(zisk_hints)]
            unsafe {
                hint_secp256r1_ecdsa_verify(msg.as_ptr(), sig.as_ptr(), pk.as_ptr());
            }

            #[cfg(zisk_hints_debug)]
            hint_log(format!(
                "hint_secp256r1_ecdsa_verify (msg: {:x?}, sig: {:x?}, pk: {:x?})",
                &msg, &sig, &pk
            ));

            #[cfg(all(target_os = "zkvm", target_vendor = "zisk"))]
            {
                unsafe { secp256r1_ecdsa_verify_c(msg.as_ptr(), sig.as_ptr(), pk.as_ptr()) }
            }
        }

        #[cfg(not(all(target_os = "zkvm", target_vendor = "zisk")))]
        {
            DefaultCrypto.secp256r1_verify_signature(msg, sig, pk)
        }
    }

    /// KZG point evaluation.
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    #[inline]
    fn verify_kzg_proof(
        &self,
        z: &[u8; 32],
        y: &[u8; 32],
        commitment: &[u8; 48],
        proof: &[u8; 48],
    ) -> Result<(), PrecompileError> {
        DefaultCrypto.verify_kzg_proof(z, y, commitment, proof)
    }

    /// BLS12-381 G1 addition (returns 96-byte unpadded G1 point)
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    fn bls12_381_g1_add(&self, a: G1Point, b: G1Point) -> Result<[u8; 96], PrecompileError> {
        DefaultCrypto.bls12_381_g1_add(a, b)
    }

    /// BLS12-381 G1 multi-scalar multiplication (returns 96-byte unpadded G1 point)
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    fn bls12_381_g1_msm(
        &self,
        pairs: &mut dyn Iterator<Item = Result<G1PointScalar, PrecompileError>>,
    ) -> Result<[u8; 96], PrecompileError> {
        DefaultCrypto.bls12_381_g1_msm(pairs)
    }

    /// BLS12-381 G2 addition (returns 192-byte unpadded G2 point)
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    fn bls12_381_g2_add(&self, a: G2Point, b: G2Point) -> Result<[u8; 192], PrecompileError> {
        DefaultCrypto.bls12_381_g2_add(a, b)
    }

    /// BLS12-381 G2 multi-scalar multiplication (returns 192-byte unpadded G2 point)
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    fn bls12_381_g2_msm(
        &self,
        pairs: &mut dyn Iterator<Item = Result<G2PointScalar, PrecompileError>>,
    ) -> Result<[u8; 192], PrecompileError> {
        DefaultCrypto.bls12_381_g2_msm(pairs)
    }

    /// BLS12-381 pairing check.
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    fn bls12_381_pairing_check(
        &self,
        pairs: &[(G1Point, G2Point)],
    ) -> Result<bool, PrecompileError> {
        DefaultCrypto.bls12_381_pairing_check(pairs)
    }

    /// BLS12-381 map field element to G1.
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    fn bls12_381_fp_to_g1(&self, fp: &[u8; 48]) -> Result<[u8; 96], PrecompileError> {
        DefaultCrypto.bls12_381_fp_to_g1(fp)
    }

    /// BLS12-381 map field element to G2.
    /// Delegated to DefaultCrypto to avoid activating extra ZisK circuits.
    fn bls12_381_fp2_to_g2(&self, fp2: ([u8; 48], [u8; 48])) -> Result<[u8; 192], PrecompileError> {
        DefaultCrypto.bls12_381_fp2_to_g2(fp2)
    }
}

impl CryptoProvider for ZiskCrypto {
    /// Recover signer from signature and message hash, without ensuring low S values.
    fn recover_signer_unchecked(
        &self,
        sig: &[u8; 65],
        msg: &[u8; 32],
    ) -> Result<Address, RecoveryError> {
        #[cfg(zisk_hints)]
        struct ResumeHintsGuard {
            already_paused: bool,
        }

        #[cfg(zisk_hints)]
        impl Drop for ResumeHintsGuard {
            fn drop(&mut self) {
                if !self.already_paused {
                    unsafe { resume_hints() };
                }
            }
        }

        // Pause hint emission here so non-Zisk target execution cannot produce extra hints (e.g. keccak256)
        #[cfg(zisk_hints)]
        let already_paused = unsafe { pause_hints() };

        // Ensure hints are always resumed on early returns.
        #[cfg(zisk_hints)]
        let _resume_hints_guard = ResumeHintsGuard { already_paused };

        // Direct k256 implementation (same as alloy_consensus::impl_k256)
        let mut signature = Signature::from_slice(&sig[0..64]).map_err(|_| RecoveryError::new())?;
        let mut recid = sig[64];

        // normalize signature and flip recovery id if needed.
        if let Some(sig_normalized) = signature.normalize_s() {
            signature = sig_normalized;
            recid ^= 1;
        }
        let recid = RecoveryId::from_byte(recid).ok_or_else(RecoveryError::new)?;

        // recover key
        let recovered_key = VerifyingKey::recover_from_prehash(&msg[..], &signature, recid)
            .map_err(|_| RecoveryError::new())?;
        Ok(public_key_to_address(&recovered_key))
    }

    /// Verify a signature against a public key and message hash, without ensuring low S values.
    fn verify_and_compute_signer_unchecked(
        &self,
        pubkey: &[u8; 65],
        sig: &[u8; 64],
        msg: &[u8; 32],
    ) -> Result<Address, RecoveryError> {
        let vk = VerifyingKey::from_sec1_bytes(pubkey).map_err(|_| RecoveryError::new())?;

        let mut signature = Signature::from_slice(sig).map_err(|_| RecoveryError::new())?;

        // normalize signature if needed
        if let Some(sig_normalized) = signature.normalize_s() {
            signature = sig_normalized;
        }

        vk.verify_prehash(msg, &signature)
            .map_err(|_| RecoveryError::new())?;

        Ok(public_key_to_address(&vk))
    }
}

fn public_key_to_address(key: &VerifyingKey) -> Address {
    let mut hasher = Keccak::v256();
    hasher.update(&key.to_encoded_point(/* compress = */ false).as_bytes()[1..]);

    let mut hash = [0u8; 32];
    hasher.finalize(&mut hash);

    Address::from_slice(&hash[12..])
}
