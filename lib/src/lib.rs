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
#![cfg_attr(any(not(feature = "std")), no_std)]

#[cfg(not(feature = "std"))]
mod no_std {
    extern crate alloc;
    extern crate core;
    pub use alloc::{
        borrow::ToOwned,
        format,
        string::{String, ToString},
        vec::Vec,
    };
}

pub mod builder;
pub mod consts;
pub mod input;
pub mod mem_db;
pub mod protocol_instance;
pub mod prover;
pub mod taiko_utils;

#[cfg(not(target_os = "zkvm"))]
mod time {
    pub use core::ops::AddAssign;
    pub use std::time::{Duration, Instant};

    pub fn now() -> Instant {
        Instant::now()
    }
}
#[cfg(target_os = "zkvm")]
mod time {
    pub trait AddAssign<Rhs = Self> {
        fn add_assign(&mut self, rhs: Self);
    }

    #[derive(Default)]
    pub struct Instant {}

    impl Instant {
        pub fn now() -> Instant {
            Instant::default()
        }
        pub fn duration_since(&self, _instant: Instant) -> Duration {
            Duration::default()
        }
    }

    #[derive(Default)]
    pub struct Duration {}

    impl Duration {
        pub fn as_secs(&self) -> u64 {
            0
        }

        pub fn subsec_millis(&self) -> u64 {
            0
        }
    }

    impl AddAssign for Duration {
        fn add_assign(&mut self, _rhs: Duration) {}
    }

    pub fn now() -> Instant {
        Instant::default()
    }
}

/// call forget only if running inside the guest
pub fn guest_mem_forget<T>(_t: T) {
    #[cfg(target_os = "zkvm")] // TODO: seperate for risc0
    core::mem::forget(_t)
}

pub trait RlpBytes: Sized {
    /// Decodes the blob into the appropriate type.
    /// The input must contain exactly one value and no trailing data.
    fn decode_bytes(bytes: impl AsRef<[u8]>) -> Result<Self, alloy_rlp::Error>;
}

impl<T> RlpBytes for T
where
    T: alloy_rlp::Decodable,
{
    #[inline]
    fn decode_bytes(bytes: impl AsRef<[u8]>) -> Result<Self, alloy_rlp::Error> {
        let mut buf = bytes.as_ref();
        let this = T::decode(&mut buf)?;
        if buf.is_empty() {
            Ok(this)
        } else {
            Err(alloy_rlp::Error::Custom("Trailing data"))
        }
    }
}

pub mod serde_with {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_with::{DeserializeAs, SerializeAs};

    use super::RlpBytes as _;

    pub struct RlpBytes {}

    impl<T> SerializeAs<T> for RlpBytes
    where
        T: alloy_rlp::Encodable,
    {
        fn serialize_as<S>(source: &T, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let bytes = alloy_rlp::encode(source);
            bytes.serialize(serializer)
        }
    }

    impl<'de, T> DeserializeAs<'de, T> for RlpBytes
    where
        T: alloy_rlp::Decodable,
    {
        fn deserialize_as<D>(deserializer: D) -> Result<T, D::Error>
        where
            D: Deserializer<'de>,
        {
            let bytes = <Vec<u8>>::deserialize(deserializer)?;
            T::decode_bytes(bytes).map_err(serde::de::Error::custom)
        }
    }
}
