cfg_if::cfg_if! {
    if #[cfg(test)] {
        pub mod test_risc0_guest;
        pub mod ecdsa;
        pub mod sha256;
        pub mod risc0_guest;
    } else {
        pub mod risc0_guest;
    }
}
