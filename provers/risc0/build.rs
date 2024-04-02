fn main() {
    #[cfg(not(feature = "enable"))]
    println!("Risc0 not enabled");

    #[cfg(feature = "enable")]
    risc0_build::embed_methods();
}
