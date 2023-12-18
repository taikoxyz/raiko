use std::{
    fs::{self, File, OpenOptions},
    io::prelude::*,
    path::Path,
    str::FromStr,
};

use anyhow::{anyhow, bail, Error, Result};
use zeth_lib::{
    consts::{ETH_MAINNET_CHAIN_SPEC, TAIKO_MAINNET_CHAIN_SPEC},
    host::Init,
    input::Input,
    taiko::{
        block_builder::{TaikoBlockBuilder, TaikoStrategyBundle},
        host::TaikoExtra,
    },
    EthereumTxEssence,
};
use zeth_primitives::{
    taiko::{string_to_bytes32, EvidenceType},
    Address, B256,
};

use crate::{
    app_args::{GlobalOpts, OneShotArgs},
    signature::*,
};

pub const PRIV_KEY_FILENAME: &str = "priv.key";


pub async fn one_shot(global_opts: GlobalOpts, args: OneShotArgs) -> Result<()> {

    println!(
        "Global options: {:?}, OneShot options: {:?}",
        global_opts, args
    );

    let path_str = args.blocks_data_file.to_string_lossy().to_string();
    let block_no = u64::from_str(&String::from(
        args.blocks_data_file
            .file_prefix()
            .unwrap()
            .to_str()
            .unwrap(),
    ))?;

    println!("Reading input file {} (block no: {})", path_str, block_no);

    let privkey_path = global_opts.secrets_dir.join(PRIV_KEY_FILENAME);
    let prev_privkey = load_private_key(&privkey_path)?;
    println!("Private key: {}", prev_privkey.display_secret());
    // let (new_privkey, new_pubkey) = generate_new_keypair()?;
    let new_pubkey = public_key(&prev_privkey);
    let new_instance = public_key_to_address(&new_pubkey);

    // fs::write(privkey_path, new_privkey.to_bytes())?;
    let pi_hash = generate_proof(
        path_str,
        args.l1_blocks_data_file.to_string_lossy().to_string(),
        args.prover,
        args.graffiti,
        block_no,
        new_instance,
    )
    .await?;

    println!("Data to be signed: {}", pi_hash);

    let sig = sign_message(&prev_privkey, pi_hash)?;

    const SGX_PROOF_LEN: usize = 89;

    let mut proof = Vec::with_capacity(SGX_PROOF_LEN);
    proof.extend(new_instance);
    proof.extend(sig.to_bytes());
    let proof = hex::encode(proof);
    println!("Proof: 0x{}", proof);
    println!("Public key: {}", new_pubkey);

    print_powdr_info()
}


async fn generate_proof(
    path_str: String,
    l1_blocks_path: String,
    prover: Address,
    graffiti: B256,
    block_no: u64,
    new_pubkey: Address,
) -> Result<B256> {

    // TODO: run Powdr here, Init<EthereumTxEssence> should be the same 
    
    let (init, extra) = parse_to_init(path_str, l1_blocks_path, prover, block_no, graffiti).await?;
    let input: Input<zeth_lib::EthereumTxEssence> = init.clone().into();
    let output = TaikoBlockBuilder::build_from(&TAIKO_MAINNET_CHAIN_SPEC, input)
        .expect("Failed to build the resulting block");
    let pi = zeth_lib::taiko::protocol_instance::assemble_protocol_instance(&extra, &output)?;
    let pi_hash = pi.hash(EvidenceType::Sgx { new_pubkey });
    Ok(pi_hash)
}

async fn parse_to_init(
    blocks_path: String,
    l1_blocks_path: String,
    prover: Address,
    block_no: u64,
    graffiti: B256,
) -> Result<(Init<zeth_lib::EthereumTxEssence>, TaikoExtra), Error> {
    let (init, extra) = tokio::task::spawn_blocking(move || {
        zeth_lib::taiko::host::get_taiko_initial_data::<TaikoStrategyBundle>(
            Some(l1_blocks_path),
            ETH_MAINNET_CHAIN_SPEC.clone(),
            None,
            prover,
            Some(blocks_path),
            TAIKO_MAINNET_CHAIN_SPEC.clone(),
            None,
            block_no,
            graffiti,
        )
        .expect("Could not init")
    })
    .await?;

    Ok::<(Init<EthereumTxEssence>, TaikoExtra), _>((init, extra))
}


fn print_powdr_info() -> Result<()> {
    // TODO change to powdr info
    Ok(())
}
