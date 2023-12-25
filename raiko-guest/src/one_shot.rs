use std::{
    fs::{self, File, OpenOptions},
    io::prelude::*,
    path::Path,
};

use anyhow::{anyhow, bail, Error, Result};
use zeth_lib::{
    consts::{ETH_MAINNET_CHAIN_SPEC, TAIKO_MAINNET_CHAIN_SPEC},
    host::Init,
    input::Input,
    taiko::{
        block_builder::{TaikoBlockBuilder, TaikoStrategyBundle},
        host::TaikoExtra,
        FileUrl,
    },
    EthereumTxEssence,
};
use zeth_primitives::{taiko::EvidenceType, Address, B256};

use crate::{
    app_args::{GlobalOpts, OneShotArgs},
    signature::*,
};

pub const ATTESTATION_QUOTE_DEVICE_FILE: &str = "/dev/attestation/quote";
pub const ATTESTATION_TYPE_DEVICE_FILE: &str = "/dev/attestation/attestation_type";
pub const ATTESTATION_USER_REPORT_DATA_DEVICE_FILE: &str = "/dev/attestation/user_report_data";
pub const PRIV_KEY_FILENAME: &str = "priv.key";

pub fn bootstrap(global_opts: GlobalOpts) -> Result<()> {
    let privkey_path = global_opts.secrets_dir.join(PRIV_KEY_FILENAME);
    let key_pair = generate_key();
    fs::write(privkey_path, key_pair.secret_bytes())?;
    Ok(())
}

pub async fn one_shot(global_opts: GlobalOpts, args: OneShotArgs) -> Result<()> {
    if !is_bootstrapped(&global_opts.secrets_dir) {
        bail!("Application was not bootstrapped. Bootstrap it first.")
    }

    println!(
        "Global options: {:?}, OneShot options: {:?}",
        global_opts, args
    );

    let privkey_path = global_opts.secrets_dir.join(PRIV_KEY_FILENAME);
    let prev_privkey = load_private_key(&privkey_path)?;
    // println!("Private key: {}", prev_privkey.display_secret());
    // let (new_privkey, new_pubkey) = generate_new_keypair()?;
    let new_pubkey = public_key(&prev_privkey);
    let new_instance = public_key_to_address(&new_pubkey);

    // fs::write(privkey_path, new_privkey.to_bytes())?;
    let pi_hash = get_data_to_sign(
        (args.l1_blocks_data_file, args.l1_rpc),
        (args.l2_blocks_data_file, args.l2_rpc),
        args.prover,
        args.graffiti,
        args.block,
        new_instance,
    )
    .await?;

    println!("Data to be signed: {}", pi_hash);

    let sig = sign_message(&prev_privkey, pi_hash)?;

    const SGX_PROOF_LEN: usize = 89;

    let mut proof = Vec::with_capacity(SGX_PROOF_LEN);
    proof.extend(args.sgx_instance_id.to_be_bytes());
    proof.extend(new_instance);
    proof.extend(sig.to_bytes());
    let proof = hex::encode(proof);
    println!("Proof: 0x{}", proof);
    println!("Public key: {}", new_pubkey);

    save_attestation_user_report_data(new_instance)?;
    print_sgx_info()
}

fn is_bootstrapped(secrets_dir: &Path) -> bool {
    let privkey_path = secrets_dir.join(PRIV_KEY_FILENAME);
    privkey_path.is_file() && !privkey_path.metadata().unwrap().permissions().readonly()
}

async fn get_data_to_sign<T: Into<FileUrl> + Send + 'static>(
    l1_file_url: T,
    l2_file_url: T,
    prover: Address,
    graffiti: B256,
    block_no: u64,
    new_pubkey: Address,
) -> Result<B256> {
    let (init, extra) = parse_to_init(l1_file_url, l2_file_url, prover, block_no, graffiti).await?;
    let input: Input<zeth_lib::EthereumTxEssence> = init.clone().into();
    let output = TaikoBlockBuilder::build_from(&TAIKO_MAINNET_CHAIN_SPEC, input)
        .expect("Failed to build the resulting block");
    let pi = zeth_lib::taiko::protocol_instance::assemble_protocol_instance(&extra, &output)?;
    let pi_hash = pi.hash(EvidenceType::Sgx { new_pubkey });
    Ok(pi_hash)
}

async fn parse_to_init<T: Into<FileUrl> + Send + 'static>(
    l1_file_url: T,
    l2_file_url: T,
    prover: Address,
    block_no: u64,
    graffiti: B256,
) -> Result<(Init<zeth_lib::EthereumTxEssence>, TaikoExtra), Error> {
    let (init, extra) = tokio::task::spawn_blocking(move || {
        zeth_lib::taiko::host::get_taiko_initial_data::<T, TaikoStrategyBundle>(
            l1_file_url,
            l2_file_url,
            ETH_MAINNET_CHAIN_SPEC.clone(),
            prover,
            TAIKO_MAINNET_CHAIN_SPEC.clone(),
            block_no,
            graffiti,
        )
        .expect("Could not init")
    })
    .await?;

    Ok::<(Init<EthereumTxEssence>, TaikoExtra), _>((init, extra))
}

fn save_attestation_user_report_data(pubkey: Address) -> Result<()> {
    let mut extended_pubkey = pubkey.to_vec();
    extended_pubkey.resize(64, 0);
    let mut user_report_data_file = OpenOptions::new()
        .write(true)
        .open(ATTESTATION_USER_REPORT_DATA_DEVICE_FILE)?;
    user_report_data_file
        .write_all(&extended_pubkey)
        .map_err(|err| anyhow!("Failed to save user report data: {}", err))
}

fn print_sgx_info() -> Result<()> {
    let attestation_type = get_sgx_attestation_type()?;
    println!("Detected attestation type: {}", attestation_type.trim());

    let mut quote_file = File::open(ATTESTATION_QUOTE_DEVICE_FILE)?;
    let mut quote = Vec::new();
    quote_file.read_to_end(&mut quote)?;
    println!(
        "Extracted SGX quote with size = {} and the following fields:",
        quote.len()
    );
    // println!("Quote: {}", hex::encode(&quote));
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
    // considers relevant for its security model. The REPORTDATA field allows the
    // application to include additional contextual information that might be necessary for
    // the particular security model or usage scenario.
    println!("  REPORTDATA:       {}", hex::encode(&quote[368..400]));
    println!("                    {}", hex::encode(&quote[400..432]));

    Ok(())
}

fn get_sgx_attestation_type() -> Result<String> {
    let mut attestation_type = String::new();
    if File::open(ATTESTATION_TYPE_DEVICE_FILE)
        .and_then(|mut file| file.read_to_string(&mut attestation_type))
        .is_ok()
    {
        return Ok(attestation_type.trim().to_string());
    }

    bail!(
        "Cannot find `{}`; are you running under SGX, with remote attestation enabled?",
        ATTESTATION_TYPE_DEVICE_FILE
    );
}
