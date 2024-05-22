extern crate alloc;
extern crate core;

pub use alloc::{vec, vec::Vec};

pub mod eip4844;
pub mod keccak;
pub mod mpt;

#[cfg(feature = "c-kzg")]
pub use c_kzg as kzg;

pub use alloy_eips;
pub use alloy_primitives::*;
pub use alloy_rlp as rlp;

pub trait RlpBytes {
    /// Returns the RLP-encoding.
    fn to_rlp(&self) -> Vec<u8>;
}

impl<T> RlpBytes for T
where
    T: rlp::Encodable,
{
    #[inline]
    fn to_rlp(&self) -> Vec<u8> {
        let rlp_length = self.length();
        let mut out = Vec::with_capacity(rlp_length);
        self.encode(&mut out);
        debug_assert_eq!(out.len(), rlp_length);
        out
    }
}

pub trait Rlp2718Bytes {
    /// Returns the RLP-encoding.
    fn to_rlp_2718(&self) -> Vec<u8>;
}

impl<T> Rlp2718Bytes for T
where
    T: alloy_eips::eip2718::Encodable2718,
{
    #[inline]
    fn to_rlp_2718(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_2718(&mut out);
        out
    }
}
