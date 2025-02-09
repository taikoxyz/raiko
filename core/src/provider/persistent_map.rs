use alloy_rpc_types::EIP1186AccountProofResponse;
use alloy_rpc_types::EIP1186StorageProof;
use reth_primitives::{Address, U256};
use reth_revm::primitives::AccountInfo;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs::File, hash::Hash, io::Write, path::PathBuf};

#[derive(Hash, Eq, PartialEq)]
pub struct StorageSlotKey {
    address: Address,
    slot: U256,
}

impl Serialize for StorageSlotKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        format!("{}:{}", self.address, self.slot).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StorageSlotKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let mut parts = s.split(':');
        let address = parts
            .next()
            .ok_or_else(|| serde::de::Error::custom("Missing address"))?
            .parse()
            .map_err(serde::de::Error::custom)?;
        let slot = parts
            .next()
            .ok_or_else(|| serde::de::Error::custom("Missing slot"))?
            .parse()
            .map_err(serde::de::Error::custom)?;
        Ok(Self { address, slot })
    }
}

impl From<(Address, U256)> for StorageSlotKey {
    fn from((address, slot): (Address, U256)) -> Self {
        Self { address, slot }
    }
}

impl From<StorageSlotKey> for (Address, U256) {
    fn from(key: StorageSlotKey) -> Self {
        (key.address, key.slot)
    }
}

pub struct PersistentBlockData {
    base: PathBuf,
}

impl PersistentBlockData {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    pub fn accounts(&self, block_number: u64) -> PersistentMap<Address, AccountInfo> {
        PersistentMap::new(format!(
            "{}/{}-accounts.json",
            self.base.display(),
            block_number
        ))
    }

    pub fn storage_values(&self, block_number: u64) -> PersistentMap<StorageSlotKey, U256> {
        PersistentMap::new(format!(
            "{}/{}-storage_values.json",
            self.base.display(),
            block_number
        ))
    }

    pub fn account_proofs(
        &self,
        block_number: u64,
    ) -> PersistentMap<Address, EIP1186AccountProofResponse> {
        PersistentMap::new(format!(
            "{}/{}-account_proofs.json",
            self.base.display(),
            block_number
        ))
    }

    pub fn account_storage_proofs(
        &self,
        block_number: u64,
    ) -> PersistentMap<StorageSlotKey, EIP1186StorageProof> {
        PersistentMap::new(format!(
            "{}/{}-account_storage_proofs.json",
            self.base.display(),
            block_number
        ))
    }
}

/// A simple cache that implements most of the methods of `HashMap` and ensures that the data is persisted to a file.
pub struct PersistentMap<
    K: Hash + Eq + Serialize + for<'de> Deserialize<'de>,
    V: Serialize + for<'de> Deserialize<'de>,
> {
    file_path: PathBuf,
    map: HashMap<K, V>,
}

impl<
        K: Hash + Eq + Serialize + for<'de> Deserialize<'de>,
        V: Serialize + for<'de> Deserialize<'de>,
    > PersistentMap<K, V>
{
    pub fn new(file_path: impl Into<PathBuf>) -> Self {
        let file_path = file_path.into();
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                tracing::warn!("Failed to create directory {}: {}", parent.display(), e);
            });
        }

        // Load the storage from the file
        let map = File::open(&file_path)
            .and_then(|file| {
                serde_json::from_reader::<_, HashMap<K, V>>(file)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })
            .unwrap_or(HashMap::new());

        Self { file_path, map }
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.map.insert(key, value)
    }

    pub fn save(&self) -> std::io::Result<()> {
        let mut file = File::create(&self.file_path)?;
        serde_json::to_writer(&mut file, &self.map)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        file.flush()?;
        Ok(())
    }
}
