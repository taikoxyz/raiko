use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_my_function(c: &mut Criterion) {
    c.bench_function("my_function", |b| {
        b.iter(|| my_function(black_box(10))) // `black_box` is used to prevent compiler optimizations on the code.
    });
}

fn my_function(x: i32) -> i32 {
    x + 1
}

criterion_group!(benches, benchmark_my_function);
criterion_main!(benches);
