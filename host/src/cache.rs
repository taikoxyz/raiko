use std::{fs::File, path::PathBuf};

use raiko_core::{
    interfaces::RaikoError,
    provider::{rpc::RpcBlockDataProvider, BlockDataProvider},
};
use raiko_lib::input::{get_input_path, GuestInput};
use tracing::{debug, info};

use crate::interfaces::{HostError, HostResult};

pub fn get_input(
    cache_path: &Option<PathBuf>,
    block_number: u64,
    network: &str,
) -> Option<GuestInput> {
    let dir = cache_path.as_ref()?;

    let path = get_input_path(dir, block_number, network);

    let file = File::open(path).ok()?;

    bincode::deserialize_from(file).ok()
}

pub fn set_input(
    cache_path: &Option<PathBuf>,
    block_number: u64,
    network: &str,
    input: &GuestInput,
) -> HostResult<()> {
    let Some(dir) = cache_path.as_ref() else {
        return Ok(());
    };

    let path = get_input_path(dir, block_number, network);
    info!("caching input for {path:?}");

    let file = File::create(&path).map_err(<std::io::Error as Into<HostError>>::into)?;
    bincode::serialize_into(file, input).map_err(|e| HostError::Anyhow(e.into()))
}

pub async fn validate_input(
    cached_input: Option<GuestInput>,
    provider: &RpcBlockDataProvider,
) -> HostResult<GuestInput> {
    if let Some(cache_input) = cached_input {
        debug!("Using cached input");
        let blocks = provider
            .get_blocks(&[(cache_input.block.number, false)])
            .await?;
        let block = blocks
            .first()
            .ok_or_else(|| RaikoError::RPC("No block data for the requested block".to_owned()))?;

        let cached_block_hash = cache_input.block.header.hash_slow();
        let real_block_hash = block.header.hash.unwrap();
        debug!("cache_block_hash={cached_block_hash:?}, real_block_hash={real_block_hash:?}");

        // double check if cache is valid
        if cached_block_hash == real_block_hash {
            Ok(cache_input)
        } else {
            Err(HostError::InvalidRequestConfig(
                "Cached input is not valid".to_owned(),
            ))
        }
    } else {
        Err(HostError::InvalidRequestConfig(
            "Cached input is not enabled".to_owned(),
        ))
    }
}

