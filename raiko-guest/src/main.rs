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

use std::{fs::File, io::prelude::*, path::PathBuf, str::FromStr};

use anyhow::Result;
use clap::Parser;
use zeth_lib::{
    block_builder::{TaikoBlockBuilder, TaikoStrategyBundle},
    consts::TAIKO_MAINNET_CHAIN_SPEC,
};

#[derive(Parser, Debug)]
struct Args {
    #[clap(short, long)]
    file: PathBuf,

    #[clap(short, long)]
    no_sgx: bool,
}

// Prerequisites:
//
//   $ rustup default
//   nightly-x86_64-unknown-linux-gnu (default)
//
// Go to raiko-guest directory and compile with:
//
//   $ cargo build
//
// Go to /target/debug and run with `gramine-sgx`:
//
//   $ cd target/debug/
//   $ cp ../../raiko-guest/raiko-guest.manifest.template .
//   $ gramine-manifest -Dlog_level=error -Darch_libdir=/lib/x86_64-linux-gnu/ \
//       raiko-guest.manifest.template raiko-guest.manifest
//   $ gramine-sgx-sign --manifest raiko-guest.manifest --output raiko-guest.manifest.sgx
//   $ cp ../../host/testdata/ethereum/16424130.json.gz /tmp
//   $ gramine-sgx ./raiko-guest --file /tmp/16424130.json.gz
//
// If you want to run without Intel SGX add `--no-sgx` param:
//
//   $ cargo run -- --file /tmp/16424130.json.gz --no-sgx

#[tokio::main]
pub async fn main() -> Result<()> {
    // read input file with block data (assume `XYZ.tar.gz` file where XYZ is a block number)

    let args = Args::parse();
    let path = args.file;
    let path_str = path.to_string_lossy().to_string();
    let block_no =
        u64::from_str(&String::from(path.file_prefix().unwrap().to_str().unwrap())).unwrap();

    println!("Reading input file {} (block no: {})", path_str, block_no);

    // parse file's content to Init struct

    let rpc_cache = Some(path_str);
    let init = tokio::task::spawn_blocking(move || {
        zeth_lib::host::get_initial_data::<TaikoStrategyBundle>(
            TAIKO_MAINNET_CHAIN_SPEC.clone(),
            rpc_cache,
            None,
            block_no,
            None,
        )
        .expect("Could not init")
    })
    .await?;

    // run block builder

    let input = init.clone().into();
    let output = TaikoBlockBuilder::build_from(&TAIKO_MAINNET_CHAIN_SPEC, input)
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
    println!("Public key: 0x{}", public_key);
    println!("Signature: 0x{}", sig);
    assert!(secp.verify_ecdsa(&message, &sig, &public_key).is_ok());

    // SGX/gramine-specific code

    if args.no_sgx == false {
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
            let public_key_hash: Vec<u8> = public_key.to_string().as_bytes().to_vec();
            // pad the data with null bytes to make it 64 bytes long
            let mut padded_public_key_hash = public_key_hash.clone();
            padded_public_key_hash.resize(64, 0);
            if let Ok(_) = user_report_data_file.write_all(&padded_public_key_hash) {
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
                println!("Quote: {}", hex::encode(&quote));
                println!(
                    "  ATTRIBUTES.FLAGS: {}  [ Debug bit: {} ]",
                    hex::encode(&quote[96..104]),
                    quote[96] & 2 > 0
                );
                println!("  ATTRIBUTES.XFRM:  {}", hex::encode(&quote[104..112]));
                // Enclave's measurement (hash of code and data). MRENCLAVE is a 256-bit value that
                // represents the hash (message digest) of the code and data within an enclave. It
                // is a critical security feature of SGX and provides integrity protection for the
                // enclave's contents. When an enclave is instantiated, its MRENCLAVE value is
                // computed and stored in the SGX quote. This value can be used to ensure that the
                // enclave being run is the intended and correct version.
                println!("  MRENCLAVE:        {}", hex::encode(&quote[112..144]));
                // MRSIGNER is a 256-bit value that identifies the entity or signer responsible for
                // signing the enclave code. It represents the microcode revision of the software
                // entity that created the enclave. Each entity or signer, such as a software vendor
                // or developer, has a unique MRSIGNER value associated with their signed enclaves.
                // The MRSIGNER value provides a way to differentiate between different signers or
                // entities, allowing applications to make trust decisions based on the signer's
                // identity and trustworthiness.
                println!("  MRSIGNER:         {}", hex::encode(&quote[176..208]));
                println!("  ISVPRODID:        {}", hex::encode(&quote[304..306]));
                println!("  ISVSVN:           {}", hex::encode(&quote[306..308]));
                // The REPORTDATA field in the SGX report structure is a 64-byte array used for
                // providing additional data to the reporting process. The contents of this field
                // are application-defined and can be used to convey information that the
                // application considers relevant for its security model. The REPORTDATA field
                // allows the application to include additional contextual information that might be
                // necessary for the particular security model or usage scenario.
                println!("  REPORTDATA:       {}", hex::encode(&quote[368..400]));
                println!("                    {}", hex::encode(&quote[400..432]));
            }
        }
    }

    Ok(())
}
