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

#[cfg(feature = "std")]
use std::io::{self, Write};

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
pub mod utils;

#[cfg(not(target_os = "zkvm"))]
mod time {
    pub use core::ops::AddAssign;
    pub use std::time::{Duration, Instant};
}

#[cfg(target_os = "zkvm")]
// Dummy time implementation
mod time {
    pub trait AddAssign<Rhs = Self> {
        fn add_assign(&mut self, rhs: Self);
    }

    #[derive(Default, Clone, Copy)]
    pub struct Instant {}

    impl Instant {
        pub fn now() -> Instant {
            Instant::default()
        }
        pub fn duration_since(&self, _instant: Instant) -> Duration {
            Duration::default()
        }
        pub fn elapsed(&self) -> Duration {
            Duration::default()
        }
    }

    #[derive(Default, Clone, Copy)]
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
}

pub struct Measurement {
    start: time::Instant,
    title: String,
    inplace: bool,
}

impl Measurement {
    pub fn start(title: &str, inplace: bool) -> Measurement {
        if inplace {
            print!("{title}");
            #[cfg(feature = "std")]
            io::stdout().flush().unwrap();
        } else if !title.is_empty() {
            println!("{title}");
        }

        Self {
            start: time::Instant::now(),
            title: title.to_string(),
            inplace,
        }
    }

    pub fn stop(&self) {
        self.stop_with(&format!("{} Done", self.title));
    }

    pub fn stop_with_count(&self, count: &str) {
        self.stop_with(&format!("{} {count} done", self.title));
    }

    pub fn stop_with(&self, title: &str) -> time::Duration {
        let time_elapsed = self.start.elapsed();
        print_duration(
            &format!("{}{title} in ", if self.inplace { "\r" } else { "" }),
            time_elapsed,
        );
        time_elapsed
    }
}

pub fn print_duration(title: &str, duration: time::Duration) {
    println!(
        "{title}{}.{:03} seconds",
        duration.as_secs(),
        duration.subsec_millis()
    );
}

pub fn inplace_print(title: &str) {
    print!("\r{title}");
    #[cfg(feature = "std")]
    io::stdout().flush().unwrap();
}

pub fn clear_line() {
    print!("\r\x1B[2K");
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

    pub struct RlpHexBytes {}

    impl<T> SerializeAs<T> for RlpHexBytes
    where
        T: alloy_rlp::Encodable,
    {
        fn serialize_as<S>(source: &T, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let bytes = alloy_rlp::encode(source);
            let hex_str = hex::encode(bytes);
            hex_str.serialize(serializer)
        }
    }

    impl<'de, T> DeserializeAs<'de, T> for RlpHexBytes
    where
        T: alloy_rlp::Decodable,
    {
        fn deserialize_as<D>(deserializer: D) -> Result<T, D::Error>
        where
            D: Deserializer<'de>,
        {
            let hex_str = <String>::deserialize(deserializer)?;
            let bytes = hex::decode(hex_str).unwrap();
            T::decode_bytes(bytes).map_err(serde::de::Error::custom)
        }
    }
}
