# raiko

Taiko's multi-prover of Taiko & Etheruem block, currently supports Risc0, Sp1, and SGX.

## Usage

### Installing

To download all dependencies for all provers you can run

```console
$ make install
```

You can also download all required dependencies for each prover separately, for example to install SP1:

```console
$ TARGET=sp1 make install
```
### Building

After installing dependencies of selected prover, the following command internally calls cargo to build the prover's guest target with the `--release` profile by default, for example:
```console
$ TARGET=sp1 make build
```
If you set `DEBUG=1` then the target will be compiled without optimization (not recomended for ZkVM elfs).

### Running

Note that you have to `make build` first before running ZkVM provers, otherwise the guest elf may not be up to date and can result in poof failures.
```console
$ TARGET=sp1 make run
```
Just for development with the native prover which runs through the block execution without producing any ZK/SGX proof:
```
cargo run
```
`run` camand will start the host service that listens to proof requests, then in another terminal you can do requests like this, which proves the 10th block with native prover on Taiko A7 testnet:
```
./script/prove-block.sh taiko_a7 native 10
```
Look into `prove-block.sh` for the available options or run the script without inputs for hints. You can also automatically sync with the tip of the chain and prove all new blocks:

```
./script/prove-block.sh taiko_a7 native sync
```

## Provers
### Risc zero
To install, build, and run in one step:
```console
$ export TARGET=risc0
$ make install && make build && make run
```
To build and run test on Risc0 Zkvm:
```console
$ TARGET=risc0 make test
```
#### Bonsai
If you are using Bonsai service, edit `run-bonsai.sh` to setup your API key, endpoint and on-chain verifier address.
```console
$ ./script/setup-bonsai.sh
$ ./script/prove-block.sh taiko_a7 risc0-bonsai 10
```
#### GPU
If you have GPU with CUDA or Apple's GPU API to accelerate risc0 proof, you can do:

```console
// cuda
$ cargo run -F cuda --release --features risc0
// metal
$ cargo run -F metal --release --features risc0
```

Note that CUDA needs to be installed when using `cuda`: https://docs.nvidia.com/cuda/cuda-installation-guide-linux/index.html

### SP1
To install, build, and run in one step:
```console
$ export TARGET=sp1
$ make install && make build && make run
```
To build and run test on Sp1 Zkvm:
```console
$ TARGET=sp1 make test
```

### SGX:
To install, build, and run in one step:
```console
$ export TARGET=sgx
$ make install && make build && make run
```
To build and run test related SGX provers:
```console
$ TARGET=sgx make test
```
If your CPU doesn't support SGX, you can still run the SGX code through gramine like it would on an SGX machine:

```console
$ MOCK=1 TARGET=sgx make run
```

### Execution Trace

You can generate an execution trace for the block that is being proven by enabling the `tracer` feature:
```console
$ cargo run --features tracer
```

A `traces` folder will be created inside the root directory. This folder will contain json files with the trace of each valid transaction in the block.