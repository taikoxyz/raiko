//! Helpers for working with EIP-4844 blob fee.

// re-exports from revm for calculating blob fee
pub use revm_primitives::{calc_blob_gasprice, calc_excess_blob_gas as calculate_excess_blob_gas};
#[cfg(feature = "c-kzg")]
use sha2::{Digest, Sha256};

#[cfg(feature = "c-kzg")]
use crate::B256;

/// Calculates the versioned hash for a KzgCommitment
///
/// Specified in [EIP-4844](https://eips.ethereum.org/EIPS/eip-4844#header-extension)
#[cfg(feature = "c-kzg")]
pub fn kzg_to_versioned_hash(commitment: &c_kzg::KzgCommitment) -> B256 {
    let mut res = Sha256::digest(commitment.as_slice());
    res[0] = VERSIONED_HASH_VERSION_KZG;
    B256::new(res.into())
}

/// Constants for EIP-4844
/// from https://github.com/paradigmxyz/reth/blob/79452eadaf4963f1e8d78a18b1f490d7c560aa54/crates/primitives/src/constants/eip4844.rs#L2
pub use alloy_eips::eip4844::{
    BLOB_GASPRICE_UPDATE_FRACTION, BLOB_TX_MIN_BLOB_GASPRICE, DATA_GAS_PER_BLOB,
    FIELD_ELEMENTS_PER_BLOB, FIELD_ELEMENT_BYTES, MAX_BLOBS_PER_BLOCK, MAX_DATA_GAS_PER_BLOCK,
    TARGET_BLOBS_PER_BLOCK, TARGET_DATA_GAS_PER_BLOCK, VERSIONED_HASH_VERSION_KZG,
};
/// [EIP-4844](https://eips.ethereum.org/EIPS/eip-4844#parameters) protocol constants and utils for shard Blob Transactions.
#[cfg(feature = "c-kzg")]
pub use trusted_setup::*;

#[cfg(feature = "c-kzg")]
mod trusted_setup {
    use std::{io::Write, sync::Arc};

    use once_cell::sync::Lazy;
    pub use revm_primitives::kzg::parse_kzg_trusted_setup;

    use crate::kzg::KzgSettings;

    /// KZG trusted setup
    pub static MAINNET_KZG_TRUSTED_SETUP: Lazy<Arc<KzgSettings>> = Lazy::new(|| {
        Arc::new(
            c_kzg::KzgSettings::load_trusted_setup(
                &revm_primitives::kzg::G1_POINTS.0,
                &revm_primitives::kzg::G2_POINTS.0,
            )
            .expect("failed to load trusted setup"),
        )
    });

    /// Loads the trusted setup parameters from the given bytes and returns the
    /// [KzgSettings].
    ///
    /// This creates a temp file to store the bytes and then loads the [KzgSettings] from
    /// the file via [KzgSettings::load_trusted_setup_file].
    pub fn load_trusted_setup_from_bytes(
        bytes: &[u8],
    ) -> Result<KzgSettings, LoadKzgSettingsError> {
        let mut file = tempfile::NamedTempFile::new().map_err(LoadKzgSettingsError::TempFileErr)?;
        file.write_all(bytes)
            .map_err(LoadKzgSettingsError::TempFileErr)?;
        KzgSettings::load_trusted_setup_file(file.path()).map_err(LoadKzgSettingsError::KzgError)
    }

    /// Error type for loading the trusted setup.
    #[derive(Debug, thiserror::Error)]
    pub enum LoadKzgSettingsError {
        /// Failed to create temp file to store bytes for loading [KzgSettings] via
        /// [KzgSettings::load_trusted_setup_file].
        #[error("failed to setup temp file: {0}")]
        TempFileErr(#[from] std::io::Error),
        /// Kzg error
        #[error("KZG error: {0:?}")]
        KzgError(#[from] c_kzg::Error),
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn ensure_load_kzg_settings() {
            let _settings = Arc::clone(&MAINNET_KZG_TRUSTED_SETUP);
        }
    }
}
