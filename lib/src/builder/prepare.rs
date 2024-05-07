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

use core::fmt::Debug;

use alloy_consensus::Header as AlloyConsensusHeader;
use anyhow::{bail, Context, Result};
use revm::{Database, DatabaseCommit};

use crate::{builder::BlockBuilder, consts::MAX_EXTRA_DATA_BYTES, utils::HeaderHasher};

pub trait HeaderPrepStrategy {
    fn prepare_header<D>(block_builder: BlockBuilder<D>) -> Result<BlockBuilder<D>>
    where
        D: Database + DatabaseCommit,
        <D as Database>::Error: core::fmt::Debug;
}

pub struct TaikoHeaderPrepStrategy {}

impl HeaderPrepStrategy for TaikoHeaderPrepStrategy {
    fn prepare_header<D>(mut block_builder: BlockBuilder<D>) -> Result<BlockBuilder<D>>
    where
        D: Database + DatabaseCommit,
        <D as Database>::Error: Debug,
    {
        // Validate timestamp
        let timestamp: u64 = block_builder.input.timestamp;
        if timestamp < block_builder.input.parent_header.timestamp {
            bail!(
                "Invalid timestamp: expected >= {}, got {}",
                block_builder.input.parent_header.timestamp,
                block_builder.input.timestamp,
            );
        }
        // Validate extra data
        let extra_data_bytes = block_builder.input.extra_data.len();
        if extra_data_bytes > MAX_EXTRA_DATA_BYTES {
            bail!("Invalid extra data: expected <= {MAX_EXTRA_DATA_BYTES}, got {extra_data_bytes}")
        }
        // Derive header
        let number: u64 = block_builder.input.parent_header.number;
        block_builder.header = Some(AlloyConsensusHeader {
            // Initialize fields that we can compute from the parent
            parent_hash: block_builder.input.parent_header.hash(),
            number: number
                .checked_add(1)
                .with_context(|| "Invalid block number: too large")?,
            base_fee_per_gas: Some(block_builder.input.base_fee_per_gas.into()),
            // Initialize metadata from input
            beneficiary: block_builder.input.beneficiary,
            gas_limit: block_builder.input.gas_limit.into(),
            timestamp: block_builder.input.timestamp,
            mix_hash: block_builder.input.mix_hash,
            extra_data: block_builder.input.extra_data.clone(),
            blob_gas_used: block_builder.input.blob_gas_used.map(|b| b.into()),
            excess_blob_gas: block_builder.input.excess_blob_gas.map(|b| b.into()),
            parent_beacon_block_root: block_builder.input.parent_beacon_block_root,
            // do not fill the remaining fields
            ..Default::default()
        });
        Ok(block_builder)
    }
}
