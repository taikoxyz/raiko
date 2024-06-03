// Copyright 2023 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
extern crate core;

pub use alloc::{vec, vec::Vec};

pub mod eip4844;
pub mod keccak;
pub mod mpt;
pub mod receipt;
pub mod signature;

#[cfg(feature = "c-kzg")]
pub use c_kzg as kzg;

pub mod revm;
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
