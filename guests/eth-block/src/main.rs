// Copyright 2023 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![feature(path_file_prefix)]

extern crate rand;
extern crate secp256k1;

// use clap::Parser;
use std::{fs::File, io::prelude::*, path::PathBuf, str::FromStr};

use anyhow::Result;
use zeth_lib::{
    block_builder::{TaikoBlockBuilder, TaikoStrategyBundle},
    consts::ETH_MAINNET_CHAIN_SPEC,
};

// #[derive(Parser, Debug)]
// struct Args {
//     #[clap(short, long)]
//     file: PathBuf,
// }

// Prerequisites:
//
//   $ rustup default
//   nightly-x86_64-unknown-linux-gnu (default)
//
// Go to /guests/eth-block directory and compile with:
//
//   $ cargo build
//
// Go to /guests/eth-block/target/debug and run with `gramine-sgx`:
//
//   $ cd target/debug/
//   $ cp ../../eth-block.manifest.template .
//   $ gramine-manifest -Dlog_level=error -Darch_libdir=/lib/x86_64-linux-gnu/ \
//       eth-block.manifest.template eth-block.manifest
//   $ gramine-sgx-sign --manifest eth-block.manifest --output eth-block.manifest.sgx
//   $ cp ../../../../host/testdata/ethereum/162.json.gz /tmp
//   $ gramine-sgx ./eth-block

#[tokio::main]
pub async fn main() -> Result<()> {
    // read input file with block data (assume `XYZ.tar.gz` file where XYZ is a block number)

    // let args = Args::parse();
    // let path = args.file;
    let path = PathBuf::from("/tmp/162.json.gz");
    let path_str = path.to_string_lossy().to_string();
    let block_no =
        u64::from_str(&String::from(path.file_prefix().unwrap().to_str().unwrap())).unwrap();

    eprintln!("Reading input file {} (block no: {})", path_str, block_no);

    // parse file's content to Init struct

    let rpc_cache = Some(path_str);
    let init = tokio::task::spawn_blocking(move || {
        zeth_lib::host::get_initial_data::<TaikoStrategyBundle>(
            ETH_MAINNET_CHAIN_SPEC.clone(),
            rpc_cache,
            None,
            block_no,
        )
        .expect("Could not init")
    })
    .await?;

    // run block builder

    let input = init.clone().into();
    let output = TaikoBlockBuilder::build_from(&ETH_MAINNET_CHAIN_SPEC, input)
        .expect("Failed to build the resulting block");

    // get hash of the block header and print it

    let output_hash = output.hash();
    let output_hash_str = output_hash.to_string();
    println!("{}", output_hash_str);

    // generate random Ethereum public & private keys

    use secp256k1::{hashes::sha256, rand::rngs::OsRng, Message, Secp256k1};

    let secp = Secp256k1::new();
    let (secret_key, public_key) = secp.generate_keypair(&mut OsRng);

    // sign the above hash of the block header with the above secret key

    let message = Message::from_hashed_data::<sha256::Hash>(output_hash_str.as_bytes());
    let sig = secp.sign_ecdsa(&message, &secret_key);
    // eprintln!("Private key: {}", secret_key.);
    eprintln!("Public key: 0x{}", public_key);
    eprintln!("Signature: 0x{}", sig);
    assert!(secp.verify_ecdsa(&message, &sig, &public_key).is_ok());

    // assert /dev/attestation/quote exists

    if !std::path::Path::new("/dev/attestation/quote").exists() {
        eprintln!("Cannot find `/dev/attestation/quote`; are you running under SGX, with remote attestation enabled?");
        std::process::exit(1);
    }

    if let Ok(mut attestation_type_file) = File::open("/dev/attestation/attestation_type") {
        let mut attestation_type = String::new();
        if let Ok(_) = attestation_type_file.read_to_string(&mut attestation_type) {
            println!("Detected attestation type: {}", attestation_type.trim());
        }
    }

    if let Ok(mut user_report_data_file) = File::create("/dev/attestation/user_report_data") {
        let zeros = vec![0u8; 64];
        if let Ok(_) = user_report_data_file.write_all(&zeros) {
            println!("Successfully wrote zeros to user_report_data");
        }
    }

    if let Ok(mut quote_file) = File::open("/dev/attestation/quote") {
        let mut quote = Vec::new();
        if let Ok(_) = quote_file.read_to_end(&mut quote) {
            println!(
                "Extracted SGX quote with size = {} and the following fields:",
                quote.len()
            );
            println!(
                "  ATTRIBUTES.FLAGS: {}  [ Debug bit: {} ]",
                hex::encode(&quote[96..104]),
                quote[96] & 2 > 0
            );
            println!("  ATTRIBUTES.XFRM:  {}", hex::encode(&quote[104..112]));
            // Enclave's measurement (hash of code and data). MRENCLAVE is a 256-bit value that
            // represents the hash (message digest) of the code and data within an enclave. It is a
            // critical security feature of SGX and provides integrity protection for the enclave's
            // contents. When an enclave is instantiated, its MRENCLAVE value is computed and stored
            // in the SGX quote. This value can be used to ensure that the enclave being run is the
            // intended and correct version.
            println!("  MRENCLAVE:        {}", hex::encode(&quote[112..144]));
            // MRSIGNER is a 256-bit value that identifies the entity or signer responsible for
            // signing the enclave code. It represents the microcode revision of the software entity
            // that created the enclave. Each entity or signer, such as a software vendor or
            // developer, has a unique MRSIGNER value associated with their signed enclaves. The
            // MRSIGNER value provides a way to differentiate between different signers or entities,
            // allowing applications to make trust decisions based on the signer's identity and
            // trustworthiness.
            println!("  MRSIGNER:         {}", hex::encode(&quote[176..208]));
            println!("  ISVPRODID:        {}", hex::encode(&quote[304..306]));
            println!("  ISVSVN:           {}", hex::encode(&quote[306..308]));
            // The REPORTDATA field in the SGX report structure is a 64-byte array used for
            // providing additional data to the reporting process. The contents of this field are
            // application-defined and can be used to convey information that the application
            // considers relevant for its security model. The REPORTDATA field allows
            // the application to include additional contextual information that might be necessary
            // for the particular security model or usage scenario.
            println!("  REPORTDATA:       {}", hex::encode(&quote[368..400]));
            println!("                    {}", hex::encode(&quote[400..432]));
        }
    }

    Ok(())
}

// Sample output of this binary:
//
// ubuntu@VM-0-6-ubuntu:~/zeth-john/guests/eth-block/target/debug$ sudo gramine-sgx
// ./eth-block --file=/tmp/162.json.gz Gramine is starting. Parsing TOML manifest file,
// this may take some time...
// ------------------------------------------------------------------------------------------------
// Gramine detected the following insecure configurations:

//   - sgx.debug = true                           (this is a debug enclave)
//   - loader.insecure__use_cmdline_argv = true   (forwarding command-line args from
//     untrusted host to the app)
//   - sys.insecure__allow_eventfd = true         (host-based eventfd is enabled)
//   - sgx.allowed_files = [ ... ]                (some files are passed through from
//     untrusted host without verification)

// Gramine will continue application execution, but this configuration must not be used in
// production!
// ------------------------------------------------------------------------------------------------

// Reading input file /tmp/162.json.gz (block no: 162)
// 0x0026e27a3f45ff73bd7f950b9749c75c6d32b6512e09ed159ede314dd97ee55b
// Public key: 0x02c9d6e78225271ce6a2e027fd6b5d71c4c2cf84b2f0312175daf2b6cd6827e6cb
// Signature:
// 0x3045022100bd4fafb014bfc07fc12cf77fd94ca76bfc7703268816ac9bedc278cf2d15957a02205140ac1e67f6fe2503db03096c520d67481293d0a07da12b633f2aa975f6a62c
// Detected attestation type: dcap
// Successfully wrote zeros to user_report_data
// Extracted SGX quote with size = 4734 and the following fields:
//   ATTRIBUTES.FLAGS: 0700000000000000  [ Debug bit: true ]
//   ATTRIBUTES.XFRM:  e700000000000000
//   MRENCLAVE:        ba8952e970f1d2405908a8ba75021d8144d6b83e9c23f8cf3f523485e25c9aad
//   MRSIGNER:         669b80648c2d9c97f32263fa1961f95f83818682d6359758221f0e7acb9584c0
//   ISVPRODID:        0000
//   ISVSVN:           0000
//   REPORTDATA:       0000000000000000000000000000000000000000000000000000000000000000
//                     0000000000000000000000000000000000000000000000000000000000000000
