// #![no_main]
// sp1_zkvm::entrypoint!(dummy);
// fn dummy() {}

// const ZKVM_ENTRY: fn() = dummy;

// #![no_main]
// use sp1_zkvm::heap::SimpleAlloc;

// #[global_allocator]
// static HEAP: SimpleAlloc = SimpleAlloc;

// mod zkvm_generated_main {

//     #[no_mangle]
//     fn main() {
//         super::ZKVM_ENTRY()
//     }
// }

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(add(1, 2), 3);
    }
}
