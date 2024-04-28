#![no_main]
sp1_zkvm::entrypoint!(main);

use std::hint::black_box;

fn fibonacci(n: u32) -> u32 {
    let mut nums = vec![1, 1];
    for _ in 0..n {
        let mut c = nums[nums.len() - 1] + nums[nums.len() - 2];
        c %= 7919;
        nums.push(c);
    }
    nums[nums.len() - 1]
}

pub fn main() {
    let result = black_box(fibonacci(black_box(1000)));
    println!("result: {}", result);
}

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
