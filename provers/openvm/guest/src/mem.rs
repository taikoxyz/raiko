// Memory allocation configuration for OpenVM guest programs
// This module provides custom allocator settings if needed

use core::alloc::Layout;

#[cfg(target_os = "zkvm")]
#[global_allocator]
static ALLOCATOR: openvm::allocator::DefaultAllocator = openvm::allocator::DefaultAllocator;

#[cfg(target_os = "zkvm")]
#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}
