use bar;

fn main() {
    println!("Hello, world!");
}

#[test]
fn test_one() {
    bar::add(1, 2);
}
