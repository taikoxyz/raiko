# Raiko

# Intro

This branch introduces SGX-enabled Zeth/Raiko. It consists of 2 'modules': `raiko-guest` and `raiko-host`.

`raiko-host` is capable of fetching relevant block data and saving it to the `*.json.gz` file. `raiko-host` is *not* being run inside SGX enclave.

`raiko-guest` is responsible for generating public-private key pair and signing. It is being run inside SGX enclave.


## Building

To build the project make sure you have correct toolchain selected:

```console
ubuntu:~/zeth$ rustup default
nightly-x86_64-unknown-linux-gnu (default)
```

and compile the project:

```console
ubuntu:~/zeth$ cargo build
```

The above command creates `/target` directory with `raiko-host` and `raiko-guest` compilation artifacts.


### `raiko-guest`


#### SGX disabled

To run `raiko-guest` without using SGX:

```console
ubuntu:~/zeth/target/debug$ cd target/debug
ubuntu:~/zeth/target/debug$ cp ../../raiko-host/testdata/ethereum/16424130.json.gz /tmp
ubuntu:~/zeth/target/debug$ ./raiko-guest --no-sgx --file /tmp/16424130.json.gz
Reading input file /tmp/16424130.json.gz (block no: 16424130)
0x3f841e7f8e56223202e174a94524e33cb7aa3a0cc5141b6efd24be3520655ec7
Public key: 0x02a5103b31a9f16c579f9d96a3cb32c9cb7e2702effdec8d0ae9d01d3ce326a15b
Signature: 0x3044022018a8f8b8a7ae249631af825dcd5c414197f79c56d9ea9ed224b1abdf3b589a2002205f33dec087a5fe032d47de4da9544ec4eb903323ba2812c4f07a48fc314393fb
```


#### SGX enabled

To run `raiko-guest` with SGX using Gramine:

```console
ubuntu:~/zeth/target/debug$ cd target/debug
ubuntu:~/zeth/target/debug$ cp ../../raiko-guest/raiko-guest.manifest.template .
ubuntu:~/zeth/target/debug$ gramine-manifest -Dlog_level=error -Darch_libdir=/lib/x86_64-linux-gnu/ raiko-guest.manifest.template raiko-guest.manifest
ubuntu:~/zeth/target/debug$ gramine-sgx-sign --manifest raiko-guest.manifest --output raiko-guest.manifest.sgx
Attributes:
    size:        0x10000000000
    edmm:        True
    max_threads: 16
    isv_prod_id: 0
    isv_svn:     0
    attr.flags:  0x4
    attr.xfrm:   0x3
    misc_select: 0x0
SGX remote attestation:
    DCAP/ECDSA
Memory:
    000000ffffff3000-0000010000000000 [REG:R--] (manifest) measured
    000000fffff73000-000000ffffff3000 [REG:RW-] (ssa) measured
    000000fffff63000-000000fffff73000 [TCS:---] (tcs) measured
    000000fffff53000-000000fffff63000 [REG:RW-] (tls) measured
    000000fffff13000-000000fffff53000 [REG:RW-] (stack) measured
    000000ffffed3000-000000fffff13000 [REG:RW-] (stack) measured
    000000ffffe93000-000000ffffed3000 [REG:RW-] (stack) measured
    000000ffffe53000-000000ffffe93000 [REG:RW-] (stack) measured
    000000ffffe13000-000000ffffe53000 [REG:RW-] (stack) measured
    000000ffffdd3000-000000ffffe13000 [REG:RW-] (stack) measured
    000000ffffd93000-000000ffffdd3000 [REG:RW-] (stack) measured
    000000ffffd53000-000000ffffd93000 [REG:RW-] (stack) measured
    000000ffffd13000-000000ffffd53000 [REG:RW-] (stack) measured
    000000ffffcd3000-000000ffffd13000 [REG:RW-] (stack) measured
    000000ffffc93000-000000ffffcd3000 [REG:RW-] (stack) measured
    000000ffffc53000-000000ffffc93000 [REG:RW-] (stack) measured
    000000ffffc13000-000000ffffc53000 [REG:RW-] (stack) measured
    000000ffffbd3000-000000ffffc13000 [REG:RW-] (stack) measured
    000000ffffb93000-000000ffffbd3000 [REG:RW-] (stack) measured
    000000ffffb53000-000000ffffb93000 [REG:RW-] (stack) measured
    000000ffffb43000-000000ffffb53000 [REG:RW-] (sig_stack) measured
    000000ffffb33000-000000ffffb43000 [REG:RW-] (sig_stack) measured
    000000ffffb23000-000000ffffb33000 [REG:RW-] (sig_stack) measured
    000000ffffb13000-000000ffffb23000 [REG:RW-] (sig_stack) measured
    000000ffffb03000-000000ffffb13000 [REG:RW-] (sig_stack) measured
    000000ffffaf3000-000000ffffb03000 [REG:RW-] (sig_stack) measured
    000000ffffae3000-000000ffffaf3000 [REG:RW-] (sig_stack) measured
    000000ffffad3000-000000ffffae3000 [REG:RW-] (sig_stack) measured
    000000ffffac3000-000000ffffad3000 [REG:RW-] (sig_stack) measured
    000000ffffab3000-000000ffffac3000 [REG:RW-] (sig_stack) measured
    000000ffffaa3000-000000ffffab3000 [REG:RW-] (sig_stack) measured
    000000ffffa93000-000000ffffaa3000 [REG:RW-] (sig_stack) measured
    000000ffffa83000-000000ffffa93000 [REG:RW-] (sig_stack) measured
    000000ffffa73000-000000ffffa83000 [REG:RW-] (sig_stack) measured
    000000ffffa63000-000000ffffa73000 [REG:RW-] (sig_stack) measured
    000000ffffa53000-000000ffffa63000 [REG:RW-] (sig_stack) measured
    000000ffff9f9000-000000ffffa49000 [REG:R-X] (code) measured
    000000ffffa49000-000000ffffa53000 [REG:RW-] (data) measured
Measurement:
    6146388af08ec2b1f94219da41d0cae1a890ddd3e80cacad8aac69d0ed533d6d
ubuntu:~/zeth/target/debug$ cp ../../raiko-host/testdata/ethereum/16424130.json.gz /tmp
ubuntu:~/zeth/target/debug$ gramine-sgx ./raiko-guest --file /tmp/16424130.json.gz
Gramine is starting. Parsing TOML manifest file, this may take some time...
-----------------------------------------------------------------------------------------------------------------------
Gramine detected the following insecure configurations:

  - loader.insecure__use_cmdline_argv = true   (forwarding command-line args from untrusted host to the app)
  - sys.insecure__allow_eventfd = true         (host-based eventfd is enabled)
  - sgx.allowed_files = [ ... ]                (some files are passed through from untrusted host without verification)

Gramine will continue application execution, but this configuration must not be used in production!
-----------------------------------------------------------------------------------------------------------------------

Reading input file /tmp/16424130.json.gz (block no: 16424130)
0x3f841e7f8e56223202e174a94524e33cb7aa3a0cc5141b6efd24be3520655ec7
Public key: 0x022898448ef5976f4636a3624b0fb26c9b27f7f46623c994f86d3b1d32f2fdc587
Signature: 0x30450221008ba47f82b54ecabab3d30e29e276708a366e4a67eab74cf26515ff442961146d02206d3b415ed850342dd2fe2b474e53da5e43a1b1bed005919256d045fb50f91c5d
Detected attestation type: dcap
Successfully wrote zeros to user_report_data
Extracted SGX quote with size = 4734 and the following fields:
  ATTRIBUTES.FLAGS: 0500000000000000  [ Debug bit: false ]
  ATTRIBUTES.XFRM:  e700000000000000
  MRENCLAVE:        6146388af08ec2b1f94219da41d0cae1a890ddd3e80cacad8aac69d0ed533d6d
  MRSIGNER:         669b80648c2d9c97f32263fa1961f95f83818682d6359758221f0e7acb9584c0
  ISVPRODID:        0000
  ISVSVN:           0000
  REPORTDATA:       3032323839383434386566353937366634363336613336323462306662323663
                    3962323766376634363632336339393466383664336231643332663266646335
```


### `raiko-host`

```console
ubuntu:~/zeth/target/debug$ ./raiko-host --rpc-url="https://rpc.internal.taiko.xyz/" --block-no=169 --cache=/tmp
thread 'tokio-runtime-worker' panicked at raiko-host/src/main.rs:92:14:
Could not init: Error at transaction 0: Transaction(InvalidChainId)
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
Error: task 9 panicked
```

(FIXME - John's Taiko-specific code needed)
