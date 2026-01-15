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

use tracing::debug;

pub mod builder;
pub mod consts;
pub mod input;
pub mod libhash;
pub mod manifest;
pub mod mem_db;
pub mod primitives;
pub mod proof_type;
pub mod protocol_instance;
pub mod prover;
pub mod utils;

#[cfg(not(target_os = "zkvm"))]
mod time {
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
        //pub fn duration_since(&self, _instant: Instant) -> Duration {
        //    Duration::default()
        //}
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

pub struct CycleTracker {
    #[allow(dead_code)]
    title: String,
}

impl CycleTracker {
    pub fn start(title: &str) -> CycleTracker {
        let ct = CycleTracker {
            title: title.to_string(),
        };
        #[cfg(all(
            all(target_os = "zkvm", not(target_vendor = "risc0")),
            feature = "sp1-cycle-tracker"
        ))]
        println!("cycle-tracker-start: {0}", title);
        ct
    }

    pub fn end(&self) {
        #[cfg(all(
            all(target_os = "zkvm", not(target_vendor = "risc0")),
            feature = "sp1-cycle-tracker"
        ))]
        println!("cycle-tracker-end: {0}", self.title);
    }

    pub fn println(_inner: impl Fn()) {
        #[cfg(all(
            all(target_os = "zkvm", not(target_vendor = "risc0")),
            feature = "sp1-cycle-tracker"
        ))]
        _inner()
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
            debug!("{title}");
            #[cfg(feature = "std")]
            io::stdout().flush().unwrap();
        } else if !title.is_empty() {
            debug!("{title}");
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
    debug!(
        "{title}{}.{:03} seconds",
        duration.as_secs(),
        duration.subsec_millis()
    );
}

pub fn inplace_print(title: &str) {
    if consts::IN_CONTAINER.is_some() {
        return;
    }
    print!("\r{title}");
    #[cfg(all(feature = "std", debug_assertions))]
    io::stdout().flush().unwrap();
}

pub fn clear_line() {
    if consts::IN_CONTAINER.is_some() {
        return;
    }
    print!("\r\x1B[2K");
}

/// call forget only if running inside the guest
pub fn guest_mem_forget<T>(_t: T) {
    #[cfg(target_os = "zkvm")] // TODO: separate for risc0
    core::mem::forget(_t)
}
