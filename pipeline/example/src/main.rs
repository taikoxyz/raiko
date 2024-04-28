// #![no_main]
// // If you want to try std support, also update the guest Cargo.toml file

// // cfg_if::cfg_if! {
// //     if #[cfg(feature = "risc0")] {
// //         use risc0_zkvm::guest::env;
// //         risc0_zkvm::guest::entry!(main);
// //         fn main() {
// //             let input: u32 = env::read();
// //             env::commit(&input);
// //         }
// //     } else {
// //         sp1_zkvm::entrypoint!(main);
// //         fn main() {
// //             println!("Hello, world!");
// //         }
// //     }
// // }

// use risc0_zkvm::guest::env;
// risc0_zkvm::guest::entry!(main);
// fn main() {
//     let input: u32 = env::read();
//     env::commit(&input);
// }

// #[test]
// fn test_one() {
//     bar::add(1, 2);
// }

#![no_main]
// If you want to try std support, also update the guest Cargo.toml file
// #![no_std]  // std support is experimental

use risc0_zkvm::guest::env;

risc0_zkvm::guest::entry!(main);

fn main() {
    // TODO: Implement your guest code here

    // read the input
    let input: u32 = env::read();

    // TODO: do something with the input

    // write public output to the journal
    env::commit(&input);
}

// use k256 as risc0_k256;

#[cfg(test)]
mod test {

    #[test]
    fn fib() {
        let mut a = 1;
        let mut b = 1;
        for _ in 0..10 {
            let c = a + b;
            a = b;
            b = c;
        }
        assert_eq!(b, 144);
    }
}
