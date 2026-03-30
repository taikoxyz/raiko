use std::{fs, path::PathBuf};

use anyhow::{anyhow, Context, Result};
use raiko_lib::{primitives::Address, proof_type::ProofType};
use serde::{Deserialize, Serialize};

use crate::TdxConfig;

pub fn get_tdx_config(config: &serde_json::Value) -> Result<TdxConfig> {
    let tdx_section = config
        .get("tdx")
        .ok_or_else(|| anyhow!("TDX configuration not found in config"))?;
    TdxConfig::deserialize(tdx_section).map_err(|e| anyhow!("Failed to parse TDX config: {}", e))
}

pub fn get_config_dir() -> Result<PathBuf> {
    let home_dir = dirs::home_dir().ok_or_else(|| anyhow!("Failed to get home directory"))?;
    let config_dir = home_dir.join(".config").join("raiko").join("tdx");
    fs::create_dir_all(&config_dir)?;
    Ok(config_dir)
}

pub fn bootstrap_exists() -> Result<bool> {
    let config_dir = get_config_dir()?;
    let bootstrap_file = config_dir.join("bootstrap.json");
    let key_file = config_dir.join("secrets").join("priv.key");

    Ok(bootstrap_file.exists() && key_file.exists())
}

pub fn generate_private_key() -> Result<secp256k1::SecretKey> {
    let secp = secp256k1::Secp256k1::new();
    let (secret_key, _) = secp.generate_keypair(&mut secp256k1::rand::thread_rng());

    save_private_key(&secret_key)?;

    Ok(secret_key)
}

pub fn save_private_key(private_key: &secp256k1::SecretKey) -> Result<()> {
    let config_dir = get_config_dir()?;

    let secrets_dir = config_dir.join("secrets");
    fs::create_dir_all(&secrets_dir)?;

    let key_file = secrets_dir.join("priv.key");
    fs::write(&key_file, private_key.secret_bytes())?;

    // Set file permissions to 0600 (read/write for owner only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&key_file)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&key_file, perms)?;
    }

    Ok(())
}

pub fn load_private_key() -> Result<secp256k1::SecretKey> {
    let config_dir = get_config_dir()?;
    let key_file = config_dir.join("secrets").join("priv.key");
    let key_bytes = fs::read(&key_file)
        .with_context(|| format!("Failed to read private key from {}", key_file.display()))?;

    secp256k1::SecretKey::from_slice(&key_bytes).map_err(|e| anyhow!("Invalid private key: {}", e))
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapData {
    pub issuer_type: String,
    pub public_key: String,
    pub quote: String,
    pub nonce: String,
    pub metadata: serde_json::Value,
}

pub fn read_bootstrap() -> Result<BootstrapData> {
    let config_dir = get_config_dir()?;
    let bootstrap_file = config_dir.join("bootstrap.json");
    let bootstrap_data: BootstrapData =
        serde_json::from_str(&fs::read_to_string(&bootstrap_file)?)?;

    Ok(bootstrap_data)
}

pub fn write_bootstrap(
    issuer_type: &str,
    quote: &Vec<u8>,
    public_key: &Address,
    nonce: &Vec<u8>,
    metadata: serde_json::Value,
) -> Result<()> {
    let config_dir = get_config_dir()?;
    let bootstrap_file = config_dir.join("bootstrap.json");

    let bootstrap_data = BootstrapData {
        issuer_type: issuer_type.to_string(),
        public_key: public_key.to_string(),
        quote: hex::encode(quote),
        nonce: hex::encode(nonce),
        metadata,
    };
    fs::write(
        &bootstrap_file,
        serde_json::to_string_pretty(&bootstrap_data)?,
    )?;
    Ok(())
}

pub fn get_issuer_type() -> Result<ProofType> {
    let bootstrap_data = read_bootstrap()?;
    let proof_type = match bootstrap_data.issuer_type.as_str() {
        "tdx" | "simulator" => ProofType::Tdx,
        "azure" => ProofType::Tdx,
        _ => {
            return Err(anyhow!(
                "Invalid issuer type: {}",
                bootstrap_data.issuer_type
            ))
        }
    };
    Ok(proof_type)
}

pub fn validate_issuer_type(proof_type: ProofType) -> Result<()> {
    let expected_issuer = get_issuer_type()?;
    if expected_issuer != proof_type {
        return Err(anyhow!(
            "Bootstrap issuer type '{}' does not match expected '{}'",
            expected_issuer,
            proof_type,
        ));
    }
    Ok(())
}
