#[macro_export]
macro_rules! entrypoint {
    ($main:path, $($test_mod:path),*) => {
        // Define the entry point function for non-test configurations
        #[cfg(not(test))]
        const ZKVM_ENTRY: fn() = $main;

        // Set up a global allocator
        use sp1_zkvm::heap::SimpleAlloc;
        #[global_allocator]
        static HEAP: SimpleAlloc = SimpleAlloc;

        // Generate the main module and function
        mod zkvm_generated_main {
            // Define the main function for non-test configurations
            #[cfg(not(test))]
            #[no_mangle]
            fn main() {
                super::ZKVM_ENTRY() // Call the main function specified by the user
            }

            // Define the main function for test configurations
            #[cfg(test)]
            #[no_mangle]
            fn main() {
                $(
                    #[cfg(test)]
                    $test_mod();
                )*
            }
        }
    };
}
