# raiko

## Usage

### Building

- To download all dependencies for all provers you can run

```console
$ make install
```

You can also download all required dependencies for each prover separately, for example for SP1:

```console
$ TARGET="sp1" make install
```

- Clone the repository and build with `cargo`:

```console
$ cargo build
```

### Running

Run the host in a terminal that will listen to requests:

Just for development with the native prover:
```
cargo run
```

Then in another terminal you can do requests like this:

```
./prove_block.sh taiko_a7 native 10
```

Look into `prove_block.sh` for the available options or run the script without inputs and it will tell you.

You can also automatically sync with the tip of the chain and prove all new blocks:

```
./prove_block.sh taiko_a7 native sync
```

## Provers

### risc zero

Build using
```
make risc0
```

#### Testing
```
RISC0_DEV_MODE=1 RUN=1 make risc0
```

#### Bonsai
```
# edit run_bonsai.sh and run
run_bonsai.sh
# then
prove_block.sh taiko_a7 risc0-bonsai 10
```

#### CPU
```
RUN=1 make risc0
```

#### GPU

```
cargo run -F cuda --release --features risc0
```
OR
```
cargo run -F metal --release --features risc0
```

CUDA needs to be installed when using `cuda`: https://docs.nvidia.com/cuda/cuda-installation-guide-linux/index.html

### SP1:
```
make sp1
RUN=1 make sp1
```

### SGX:
```
make sgx
RUN=1 make sgx
```

If your CPU doesn't support SGX, you can still run the SGX code through gramine like it would on an SGX machine:

```
SGX_DIRECT=1 RUN=1 make sgx
```