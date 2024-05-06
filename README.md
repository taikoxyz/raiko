# raiko

## Usage

### Installing

To download all dependencies for all provers you can run

```console
$ make install
```

You can also download all required dependencies for each prover separately, for example to install SP1:

```console
$ TARGET="sp1" make install
```
### Building

After installing dependencies of selected prover, the following command internally calls cargo to build the prover's guest target with the `--release` profile by default, for example:
```console
$ TARGET="sp1" make build
```
If you set `DEBUG=1` then the target will be compiled without optimization (not recomended for ZkVM elfs).

### Running

Note that you have to `make build` first before running ZkVM provers, otherwise the guest elf may not be up to date and can result in poof failures.
```console
$ TARGET="sp1" make run
```
Just for development with the native prover which runs through the block execution without producing any ZK/SGX proof:
```
cargo run
```
`run` camand will start the host service that listens to proof requests, then in another terminal you can do requests like this:
```
// Prove the 10th block with native prover on Taiko A7 testnet
./prove_block.sh taiko_a7 native 10
```
Look into `prove_block.sh` for the available options or run the script without inputs for hints. You can also automatically sync with the tip of the chain and prove all new blocks:

```
./prove_block.sh taiko_a7 native sync
```

## Provers

### risc zero

Build using
```console
$ export TARGET="risc0" 
$ make install && make build && make run
```

#### Running
```
TARGET="risc0" make run
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
TARGET="risc0" make run
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
TARGET="sp1" make build
TARGET="sp1" make run
```

### SGX:
```
TARGET="sgx" make build
TARGET="sgx" make run
```

If your CPU doesn't support SGX, you can still run the SGX code through gramine like it would on an SGX machine:

```
SGX_DIRECT=1 TARGET="sgx" make run
```