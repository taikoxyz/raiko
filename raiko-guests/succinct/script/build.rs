fn main() {

    #[cfg(not(feature = "enable"))]
    println!("Sp1 not enabled");
    
    #[cfg(feature = "enable")]
    sp1_helper::build_program("../program");
}
