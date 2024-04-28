// cfg_if::cfg_if! {
//     if #[cfg(feature = "risc0")] {
//         risc0_zkvm::guest::entry!(main);
//         fn main() {
//             call_foo();
//         }
//     } else {
//         sp1_zkvm::entrypoint!(main);
//         fn main() {
//             call_foo();
//         }
//     }
// }
#![no_main]
risc0_zkvm::guest::entry!(main);

use bar;

fn main() {
    call_foo();
}

fn call_foo() {
    bar::add(1, 2);
}
