reference: https://succinctlabs.github.io/sp1/generating-proofs/advanced.html

## CPU Acceleration

To enable CPU acceleration, you can use the `RUSTFLAGS` environment variable to enable the `target-cpu=native` flag when running your script. This will enable the compiler to generate code that is optimized for your CPU. We encapsulate this with `CPU_OPT=1`

```bash
RUSTFLAGS='-C target-cpu=native' cargo run --release
```

Currently there is support for AVX512 SIMD instructions.

## Performance

For maximal performance, you should run proof generation with the following command and vary your `shard_size` depending on your program's number of cycles.

```rust,noplayground
SHARD_SIZE=4194304 RUST_LOG=info RUSTFLAGS='-C target-cpu=native' cargo run --release
```

## Memory Usage

To reduce memory usage, set the `SHARD_BATCH_SIZE` environment variable depending on how much RAM
your machine has. A higher number will use more memory, but will be faster.

```rust,noplayground
SHARD_BATCH_SIZE=1 SHARD_SIZE=2097152 RUST_LOG=info RUSTFLAGS='-C target-cpu=native' cargo run --release
```
