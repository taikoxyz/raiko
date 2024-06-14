# raiko

Taiko's multi-prover for Taiko & Ethereum blocks. Currently supports Risc0, SP1, and SGX.

## Usage

### Installing

To download all dependencies for all provers you can run:

```shell
make install
```

You can also download all required dependencies for each prover separately, for example to install SP1:

```shell
TARGET=sp1 make install
```

### Building

After installing dependencies of the selected prover, the following command internally calls cargo to build the prover's guest target with the `--release` profile by default, for example:

```shell
TARGET=sp1 make build
```

If you set `DEBUG=1`, then the target will be compiled without optimization (not recommended for zkVM elfs).

### Running

Note that you have to run `make build` first before running zkVM provers, otherwise the guest elf may not be up to date and can result in proof failures.

```shell
TARGET=sp1 make run
```

Just for development with the native prover which runs through the block execution without producing any ZK/SGX proof:

```shell
cargo run
```

The `run` command will start the host service that listens to proof requests, then in another terminal you can do requests like this, which proves the 10th block with the native prover on Taiko's A7 testnet:

```shell
./script/prove-block.sh taiko_a7 native 10
```

Look into `prove-block.sh` for the available options or run the script without inputs for hints. You can also automatically sync with the tip of the chain and prove all new blocks:

```
./script/prove-block.sh taiko_a7 native sync
```

## Provers

For all host programs, you can enable CPU optimization through exporting `CPU_OPT=1`.

### Risc zero

To install, build, and run in one step:

```shell
export TARGET=risc0
make install && make build && make run
```

To build and run tests on Risc0 zkVM:

```shell
TARGET=risc0 make test
```

#### Bonsai

If you are using the Bonsai service, edit `run-bonsai.sh` to setup your API key, endpoint, and on-chain verifier address.

```shell
./script/setup-bonsai.sh
./script/prove-block.sh taiko_a7 risc0-bonsai 10
```

#### GPU

If you have a GPU with CUDA or Apple's GPU API to accelerate Risc0 proofs, you can do:

```shell
// cuda
cargo run -F cuda --release --features risc0
// metal
cargo run -F metal --release --features risc0
```

Note that CUDA needs to be installed when using `cuda`: https://docs.nvidia.com/cuda/cuda-installation-guide-linux/index.html.

### SP1

To install, build, and run in one step:

```shell
export TARGET=sp1
make install && make build && make run
```

To build and run tests on the SP1 zkVM:

```shell
TARGET=sp1 make test
```

Some optimized configurations tailored to the host can be found [here](docs/README_SP1.md).

### SGX

To install, build, and run in one step:

```shell
export TARGET=sgx
make install && make build && make run
```

To build and run test related SGX provers:

```shell
TARGET=sgx make test
```

If your CPU doesn't support SGX, you can still run the SGX code through gramine like it would on an SGX machine:

```shell
MOCK=1 TARGET=sgx make run
```

## Misc docs

- [Docker & Remote Attestation Support](docs/README_Docker_and_RA.md)
- [Metrics](docs/README_Metrics.md)

## Execution Trace

You can generate an execution trace for the block that is being proven by enabling the `tracer` feature:

```shell
cargo run --features tracer
```

A `traces` folder will be created inside the root directory. This folder will contain json files with the trace of each valid transaction in the block.

## OpenAPI

When running any of the features/provers, OpenAPI UIs are available in both Swagger and Scalar flavors on `/swagger-ui` and `/scalar` respectively.
