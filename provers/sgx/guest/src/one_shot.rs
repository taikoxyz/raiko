use std::{
    fs::{self, File, OpenOptions},
    io::{prelude::*, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Error, Result};
use base64_serde::base64_serde_type;
use raiko_lib::{
    builder::{calculate_batch_blocks_final_header, calculate_block_header},
    input::{
        GuestBatchInput, GuestInput, RawAggregationGuestInput, ShastaRawAggregationGuestInput,
    },
    libhash::hash_shasta_subproof_input,
    primitives::{
        keccak::{self},
        Address, B256,
    },
    proof_type::ProofType,
    protocol_instance::{
        aggregation_output_combine, shasta_pcd_aggregation_hash,
        validate_shasta_aggregate_proof_carry_data, ProtocolInstance,
    },
};
use secp256k1::{Keypair, PublicKey, SecretKey};
use serde::Serialize;
use serde_json::Value;

base64_serde_type!(Base64Standard, base64::engine::general_purpose::STANDARD);

use crate::{
    app_args::{GlobalOpts, OneShotArgs},
    signature::*,
};

pub const ATTESTATION_QUOTE_DEVICE_FILE: &str = "/dev/attestation/quote";
pub const ATTESTATION_TYPE_DEVICE_FILE: &str = "/dev/attestation/attestation_type";
pub const ATTESTATION_USER_REPORT_DATA_DEVICE_FILE: &str = "/dev/attestation/user_report_data";
pub const BOOTSTRAP_INFO_FILENAME: &str = "bootstrap.json";
pub const PRIV_KEY_FILENAME: &str = "priv.key";

#[derive(Serialize)]
struct BootstrapData {
    public_key: String,
    new_instance: Address,
    quote: String,
}

fn save_priv_key(key_pair: &Keypair, privkey_path: &PathBuf) -> Result<()> {
    let mut file = fs::File::create(privkey_path).with_context(|| {
        format!(
            "Failed to create private key file {}",
            privkey_path.display()
        )
    })?;
    let permissions = std::fs::Permissions::from_mode(0o600);
    file.set_permissions(permissions)
        .context("Failed to set restrictive permissions of the private key file")?;
    file.write_all(&key_pair.secret_bytes())
        .context("Failed to save encrypted private key file")?;
    Ok(())
}

fn get_sgx_quote() -> Result<Vec<u8>> {
    let mut quote_file = File::open(ATTESTATION_QUOTE_DEVICE_FILE)?;
    let mut quote = Vec::new();
    quote_file.read_to_end(&mut quote)?;
    Ok(quote)
}

fn save_bootstrap_details(
    key_pair: &Keypair,
    new_instance: Address,
    quote: Vec<u8>,
    bootstrap_details_file_path: &Path,
) -> Result<(), Error> {
    let bootstrap_details = BootstrapData {
        public_key: format!("0x{}", key_pair.public_key()),
        new_instance,
        quote: hex::encode(quote),
    };

    println!("{}", serde_json::json!(&bootstrap_details));
    let json = serde_json::to_string_pretty(&bootstrap_details)?;
    fs::write(bootstrap_details_file_path, json).context(format!(
        "Saving bootstrap data file {} failed",
        bootstrap_details_file_path.display()
    ))?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum BootStrapError {
    // file does not exist
    #[error("File does not exist")]
    NotExist,
    // file exists but has wrong permissions
    #[error("File exists but has wrong permissions")]
    WithWrongPermissions,
    // file exists but could not be read normally due to mrenclave change
    #[error("File exists but could not be read normally due to mrenclave change: {0}")]
    DecryptionError(String),
}

pub fn bootstrap(global_opts: GlobalOpts) -> Result<()> {
    // Generate a new key pair
    let key_pair = generate_key();
    // Store it on disk encrypted inside SGX so we can reuse it between program runs
    let privkey_path = global_opts.secrets_dir.join(PRIV_KEY_FILENAME);
    save_priv_key(&key_pair, &privkey_path)?;
    // Get the public key from the pair
    println!("Public key: 0x{}", key_pair.public_key());
    let new_instance = public_key_to_address(&key_pair.public_key());
    println!("Instance address: {new_instance}");
    // Store the attestation with the new public key
    save_attestation_user_report_data(new_instance)?;
    // Store all this data for future use on disk (no encryption necessary)
    let quote = get_sgx_quote()?;
    let bootstrap_details_file_path = global_opts.config_dir.join(BOOTSTRAP_INFO_FILENAME);
    save_bootstrap_details(&key_pair, new_instance, quote, &bootstrap_details_file_path)?;
    println!(
        "Bootstrap details saved in {}",
        bootstrap_details_file_path.display()
    );
    println!("Encrypted private key saved in {}", privkey_path.display());
    Ok(())
}

pub async fn one_shot(
    global_opts: GlobalOpts,
    args: OneShotArgs,
    input: GuestInput,
) -> Result<Value> {
    // Make sure this SGX instance was bootstrapped
    let prev_privkey = load_bootstrap(&global_opts.secrets_dir)
        .or_else(|_| bail!("Application was not bootstrapped or has a deprecated bootstrap."))
        .unwrap();

    println!("Global options: {global_opts:?}, OneShot options: {args:?}");

    let new_pubkey = public_key(&prev_privkey);
    let new_instance = public_key_to_address(&new_pubkey);

    // Process the block
    let header = calculate_block_header(&input);
    // Calculate the public input hash
    let pi = ProtocolInstance::new(&input, &header, ProofType::Sgx)?.sgx_instance(new_instance);
    let pi_hash = pi.instance_hash();

    println!(
        "Block {}. PI data to be signed: {pi_hash}",
        input.block.number
    );

    // Sign the public input hash which contains all required block inputs and outputs
    let sig = sign_message(&prev_privkey, pi_hash)?;

    // Create the proof for the onchain SGX verifier
    // 4(id) + 20(new) + 65(sig) = 89
    const SGX_PROOF_LEN: usize = 89;
    let mut proof = Vec::with_capacity(SGX_PROOF_LEN);
    proof.extend(args.sgx_instance_id.to_be_bytes());
    proof.extend(new_instance);
    proof.extend(sig);
    let proof = hex::encode(proof);

    // Store the public key address in the attestation data
    save_attestation_user_report_data(new_instance)?;

    // Print out the proof and updated public info
    let quote = get_sgx_quote()?;
    let data = serde_json::json!({
        "proof": format!("0x{proof}"),
        "quote": hex::encode(quote),
        "public_key": format!("0x{new_pubkey}"),
        "instance_address": new_instance.to_string(),
        "input": pi_hash.to_string(),
    });
    println!("{data}");

    // Print out general SGX information
    let _ = print_sgx_info();
    Ok(data)
}

pub fn load_bootstrap_privkey(secrets_dir: &Path) -> Result<(SecretKey, PublicKey, Address)> {
    // Make sure this SGX instance was bootstrapped
    let prev_privkey = load_bootstrap(secrets_dir)
        .or_else(|_| bail!("Application was not bootstrapped or has a deprecated bootstrap."))
        .unwrap();

    let new_pubkey = public_key(&prev_privkey);
    let new_instance = public_key_to_address(&new_pubkey);
    Ok((prev_privkey, new_pubkey, new_instance))
}

pub async fn one_shot_batch(
    global_opts: GlobalOpts,
    args: OneShotArgs,
    batch_input: GuestBatchInput,
) -> Result<Value> {
    println!("Global options: {global_opts:?}, OneShot options: {args:?}");
    let (prev_privkey, new_pubkey, new_instance) =
        load_bootstrap_privkey(&global_opts.secrets_dir)?;

    // Process the block
    let final_blocks = calculate_batch_blocks_final_header(&batch_input);
    // Calculate the public input hash
    let pi = ProtocolInstance::new_batch(&batch_input, final_blocks, ProofType::Sgx)?
        .sgx_instance(new_instance);
    let pi_hash = pi.instance_hash();

    println!(
        "Block batch {} for blocks[{}..={}]. PI data to be signed: {pi_hash}",
        batch_input.taiko.batch_id,
        batch_input.inputs.first().unwrap().block.number,
        batch_input.inputs.last().unwrap().block.number
    );

    // Sign the public input hash which contains all required block inputs and outputs
    let sig = sign_message(&prev_privkey, pi_hash)?;

    // Create the proof for the onchain SGX verifier
    // 4(id) + 20(new) + 65(sig) = 89
    const SGX_PROOF_LEN: usize = 89;
    let mut proof = Vec::with_capacity(SGX_PROOF_LEN);
    proof.extend(args.sgx_instance_id.to_be_bytes());
    proof.extend(new_instance);
    proof.extend(sig);
    let proof = hex::encode(proof);

    // Store the public key address in the attestation data
    save_attestation_user_report_data(new_instance)?;

    // Print out the proof and updated public info
    let quote = get_sgx_quote()?;
    let data = serde_json::json!({
        "proof": format!("0x{proof}"),
        "quote": hex::encode(quote),
        "public_key": format!("0x{new_pubkey}"),
        "instance_address": new_instance.to_string(),
        "input": pi_hash.to_string(),
    });
    println!("{data}");

    // Print out general SGX information
    let _ = print_sgx_info();
    Ok(data)
}

pub async fn aggregate(
    global_opts: GlobalOpts,
    args: OneShotArgs,
    input: RawAggregationGuestInput,
) -> Result<Value> {
    // Make sure this SGX instance was bootstrapped
    let prev_privkey = load_bootstrap(&global_opts.secrets_dir)
        .or_else(|_| bail!("Application was not bootstrapped or has a deprecated bootstrap."))
        .unwrap();

    println!("Global options: {global_opts:?}, OneShot options: {args:?}");

    let new_pubkey = public_key(&prev_privkey);
    let new_instance = public_key_to_address(&new_pubkey);

    // Make sure the chain of old/new public keys is preserved
    let old_instance = Address::from_slice(&input.proofs[0].proof.clone()[4..24]);
    let mut cur_instance = old_instance;

    // Verify the proofs
    for proof in input.proofs.iter() {
        // TODO: verify protocol instance data so we can trust the old/new instance data
        assert_eq!(
            recover_signer_unchecked(&proof.proof.clone()[24..].try_into().unwrap(), &proof.input,)
                .unwrap(),
            cur_instance,
        );
        cur_instance = Address::from_slice(&proof.proof.clone()[4..24]);
    }

    // Current public key needs to match latest proof new public key
    assert_eq!(cur_instance, new_instance);

    // Calculate the aggregation hash
    let aggregation_hash = keccak::keccak(aggregation_output_combine(
        [
            vec![
                B256::left_padding_from(old_instance.as_ref()),
                B256::left_padding_from(new_instance.as_ref()),
            ],
            input
                .proofs
                .iter()
                .map(|proof| proof.input)
                .collect::<Vec<_>>(),
        ]
        .concat(),
    ));

    // Sign the public aggregation hash
    let sig = sign_message(&prev_privkey, aggregation_hash.into())?;

    // Create the proof for the onchain SGX verifier
    const SGX_PROOF_LEN: usize = 109;
    // 4(id) + 20(old) + 20(new) + 65(sig) = 109
    let mut proof = Vec::with_capacity(SGX_PROOF_LEN);
    proof.extend(args.sgx_instance_id.to_be_bytes());
    proof.extend(old_instance);
    proof.extend(new_instance);
    proof.extend(sig);
    let proof = hex::encode(proof);

    // Store the public key address in the attestation data
    save_attestation_user_report_data(new_instance)?;

    // Print out the proof and updated public info
    let quote = get_sgx_quote()?;
    let data = serde_json::json!({
        "proof": format!("0x{proof}"),
        "quote": hex::encode(quote),
        "public_key": format!("0x{new_pubkey}"),
        "instance_address": new_instance.to_string(),
        "input": B256::from(aggregation_hash).to_string(),
    });
    println!("{data}");

    // Print out general SGX information
    let _ = print_sgx_info();
    Ok(data)
}

pub async fn shasta_aggregate(
    global_opts: GlobalOpts,
    args: OneShotArgs,
    input: ShastaRawAggregationGuestInput,
) -> Result<Value> {
    // Make sure this SGX instance was bootstrapped
    let prev_privkey = load_bootstrap(&global_opts.secrets_dir)
        .or_else(|_| bail!("Application was not bootstrapped or has a deprecated bootstrap."))
        .unwrap();

    println!("Global options: {global_opts:?}, OneShot options: {args:?}");

    let new_pubkey = public_key(&prev_privkey);
    let sgx_instance = public_key_to_address(&new_pubkey);

    // No key-rotation: every sub-proof must be signed by the same instance address and the
    // embedded instance bytes must match the recovered signer.
    let expected_instance = Address::from_slice(&input.proofs[0].proof.clone()[4..24]);
    assert_eq!(expected_instance, sgx_instance);

    assert!(validate_shasta_aggregate_proof_carry_data(&input));

    // Verify the proofs
    for (i, proof) in input.proofs.iter().enumerate() {
        // TODO: verify protocol instance data so we can trust the old/new instance data
        assert_eq!(
            proof.input,
            hash_shasta_subproof_input(&input.proof_carry_data_vec[i])
        );
        let instance = Address::from_slice(&proof.proof.clone()[4..24]);
        assert_eq!(instance, expected_instance);
        assert_eq!(
            recover_signer_unchecked(proof.proof[24..].try_into().unwrap(), &proof.input,).unwrap(),
            expected_instance,
        );
    }

    let aggregation_hash =
        shasta_pcd_aggregation_hash(&input.proof_carry_data_vec, sgx_instance)
            .ok_or_else(|| anyhow!("invalid shasta proof carry data for aggregation"))?;

    // Sign the public aggregation hash
    let sig = sign_message(&prev_privkey, aggregation_hash.into())?;

    // Create the proof for the onchain SGX verifier
    const SHASTA_SGX_PROOF_LEN: usize = 89;
    // 4(id) + 20(current) + 65(sig) = 89
    let mut proof = Vec::with_capacity(SHASTA_SGX_PROOF_LEN);
    proof.extend(args.sgx_instance_id.to_be_bytes());
    proof.extend(sgx_instance);
    proof.extend(sig);
    let proof = hex::encode(proof);

    // Store the public key address in the attestation data
    save_attestation_user_report_data(sgx_instance)?;

    // Print out the proof and updated public info
    let quote = get_sgx_quote()?;
    let data = serde_json::json!({
        "proof": format!("0x{proof}"),
        "quote": hex::encode(quote),
        "public_key": format!("0x{new_pubkey}"),
        "instance_address": sgx_instance.to_string(),
        "input": B256::from(aggregation_hash).to_string(),
    });
    println!("{data}");

    // Print out general SGX information
    let _ = print_sgx_info();
    Ok(data)
}

pub fn load_bootstrap(secrets_dir: &Path) -> Result<SecretKey, BootStrapError> {
    let privkey_path = secrets_dir.join(PRIV_KEY_FILENAME);
    if privkey_path.is_file() && !privkey_path.metadata().unwrap().permissions().readonly() {
        load_private_key(&privkey_path).map_err(|e| {
            BootStrapError::DecryptionError(format!(
                "Failed to load private key from {}: {e}",
                privkey_path.display(),
            ))
        })
    } else if privkey_path.is_file() {
        Err(BootStrapError::WithWrongPermissions)
    } else {
        Err(BootStrapError::NotExist)
    }
}

fn save_attestation_user_report_data(pubkey: Address) -> Result<()> {
    let mut extended_pubkey = pubkey.to_vec();
    extended_pubkey.resize(64, 0);
    let mut user_report_data_file = OpenOptions::new()
        .write(true)
        .open(ATTESTATION_USER_REPORT_DATA_DEVICE_FILE)?;
    user_report_data_file
        .write_all(&extended_pubkey)
        .map_err(|err| anyhow!("Failed to save user report data: {err}"))
}

fn print_sgx_info() -> Result<()> {
    let attestation_type = get_sgx_attestation_type()?;
    println!("Detected attestation type: {}", attestation_type.trim());

    let quote = get_sgx_quote()?;
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
        .is_err()
    {
        bail!(
            "Cannot find `{ATTESTATION_TYPE_DEVICE_FILE}`; are you running under SGX, with remote attestation enabled?"
        );
    }

    Ok(attestation_type.trim().to_string())
}
