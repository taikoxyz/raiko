use std::{
    fs::{self, File, OpenOptions},
    io::prelude::*,
    path::PathBuf,
    str::FromStr,
};

use anyhow::{anyhow, bail, Error, Result};
use secp256k1::{
    ecdsa::Signature, hashes::sha256, rand::rngs::OsRng, Message, PublicKey, SecretKey, SECP256K1,
};
use zeth_lib::{
    consts::{ETH_MAINNET_CHAIN_SPEC, TAIKO_MAINNET_CHAIN_SPEC},
    taiko::{
        block_builder::{TaikoBlockBuilder, TaikoStrategyBundle},
        host::TaikoInit,
        input::TaikoInput,
    },
};
use zeth_primitives::{
    taiko::{string_to_bytes32, EvidenceType},
    Address,
};

use crate::app_args::{GlobalOpts, OneShotArgs};

pub const ATTESTATION_QUOTE_DEVICE_FILE: &str = "/dev/attestation/quote";
pub const ATTESTATION_TYPE_DEVICE_FILE: &str = "/dev/attestation/attestation_type";
pub const ATTESTATION_USER_REPORT_DATA_DEVICE_FILE: &str = "/dev/attestation/user_report_data";
pub const PRIV_KEY_FILENAME: &str = "priv.key";

pub fn bootstrap(global_opts: GlobalOpts) -> Result<()> {
    let privkey_path = global_opts.secrets_dir.join(PRIV_KEY_FILENAME);
    let (privkey, _pubkey) = generate_new_keypair()?;
    fs::write(privkey_path, SecretKey::secret_bytes(&privkey))?;
    Ok(())
}

pub async fn one_shot(global_opts: GlobalOpts, args: OneShotArgs) -> Result<()> {
    if !is_bootstrapped(&global_opts.secrets_dir) {
        bail!("Application was not bootstrapped. Bootstrap it first.")
    }

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
    let prev_privkey = read_privkey(&privkey_path)?;
    let (new_privkey, new_pubkey) = generate_new_keypair()?;
    fs::write(privkey_path, SecretKey::secret_bytes(&new_privkey))?;
    let new_pubkey_str = new_pubkey.clone().to_string();
    let pi_hash_str = get_data_to_sign(
        path_str,
        args.l1_blocks_data_file.to_string_lossy().to_string(),
        args.prover,
        &args.graffiti,
        block_no,
        new_pubkey_str,
    )
    .await?;

    println!("Data to be signed: {}", pi_hash_str);

    let sig = sgx_sign(prev_privkey, pi_hash_str)?;

    println!("Signature: 0x{}", sig);

    save_attestation_user_report_data(new_pubkey)?;
    print_sgx_info()
}

fn is_bootstrapped(secrets_dir: &PathBuf) -> bool {
    let privkey_path = secrets_dir.join(PRIV_KEY_FILENAME);
    privkey_path.is_file() && privkey_path.metadata().unwrap().permissions().readonly() == false
}

fn read_privkey(privkey_path: &PathBuf) -> Result<SecretKey, secp256k1::Error> {
    let privkey_vec: Vec<u8> = fs::read(privkey_path).unwrap();
    SecretKey::from_slice(&privkey_vec)
}

fn generate_new_keypair() -> Result<(SecretKey, PublicKey), Error> {
    return Ok(SECP256K1.generate_keypair(&mut OsRng));
}

async fn get_data_to_sign(
    path_str: String,
    l1_blocks_path: String,
    prover: Address,
    graffiti: &str,
    block_no: u64,
    new_pubkey: String,
) -> Result<String> {
    let init = parse_to_init(path_str, l1_blocks_path, prover, block_no, graffiti).await?;
    let input: TaikoInput<zeth_lib::EthereumTxEssence> = init.clone().into();
    let output = TaikoBlockBuilder::build_from(&TAIKO_MAINNET_CHAIN_SPEC, input.l2_input.clone())
        .expect("Failed to build the resulting block");
    let pi = zeth_lib::taiko::protocol_instance::assemble_protocol_instance(&input, &output)?;
    let pi_hash = pi.hash(EvidenceType::Sgx { new_pubkey });
    let pi_hash_str = pi_hash.to_string();
    Ok(pi_hash_str)
}

async fn parse_to_init(
    blocks_path: String,
    l1_blocks_path: String,
    prover: Address,
    block_no: u64,
    graffiti: &str,
) -> Result<TaikoInit<zeth_lib::EthereumTxEssence>, Error> {
    let graffiti = string_to_bytes32(graffiti);
    let init = tokio::task::spawn_blocking(move || {
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

    Ok(init)
}

fn sgx_sign(privkey: SecretKey, protocol_intance_hash: String) -> Result<Signature> {
    let message = Message::from_hashed_data::<sha256::Hash>(protocol_intance_hash.as_bytes());
    let sig = SECP256K1.sign_ecdsa(&message, &privkey);
    let pubkey = privkey.public_key(&SECP256K1);
    assert!(SECP256K1.verify_ecdsa(&message, &sig, &pubkey).is_ok());
    Ok(sig)
}

fn save_attestation_user_report_data(pubkey: PublicKey) -> Result<()> {
    let mut user_report_data_file = OpenOptions::new()
        .write(true)
        .open(ATTESTATION_USER_REPORT_DATA_DEVICE_FILE)?;
    let pubkey_hash: Vec<u8> = pubkey.to_string().as_bytes().to_vec();
    let mut padded_pubkey_hash = pubkey_hash.clone();
    padded_pubkey_hash.resize(64, 0);
    user_report_data_file
        .write_all(&padded_pubkey_hash)
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
    println!("Quote: {}", hex::encode(&quote));
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
