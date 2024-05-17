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

//! Constants for the Ethereum protocol.
extern crate alloc;

use alloc::{collections::BTreeMap, str::FromStr};

use alloy_primitives::Address;
use anyhow::{bail, Result};
use raiko_primitives::{uint, BlockNumber, ChainId, U256};
use revm::primitives::SpecId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(not(feature = "std"))]
use crate::no_std::*;

use std::collections::HashMap;
use std::path::PathBuf;

/// U256 representation of 0.
pub const ZERO: U256 = U256::ZERO;
/// U256 representation of 1.
pub const ONE: U256 = uint!(1_U256);

/// Maximum size of extra data.
pub const MAX_EXTRA_DATA_BYTES: usize = 32;

/// Maximum allowed block number difference for the `block_hash` call.
pub const MAX_BLOCK_HASH_AGE: u64 = 256;

/// Multiplier for converting gwei to wei.
pub const GWEI_TO_WEI: U256 = uint!(1_000_000_000_U256);

const DEFAULT_CHAIN_SPECS: &str = include_str!("../../host/config/chain_spec_list_default.json");

#[derive(Clone, Debug)]
pub struct SupportedChainSpecs(HashMap<String, ChainSpec>);

impl SupportedChainSpecs {
    pub fn default() -> Self {
        let deserialized: Vec<ChainSpec> = serde_json::from_str(&DEFAULT_CHAIN_SPECS).unwrap();
        let chain_spec_list = deserialized
            .iter()
            .map(|cs| (cs.name.clone(), cs.clone()))
            .collect::<HashMap<String, ChainSpec>>();
        SupportedChainSpecs(chain_spec_list)
    }

    #[cfg(feature = "std")]
    pub fn merge_from_file(file_path: PathBuf) -> Result<SupportedChainSpecs> {
        let mut known_chain_specs = SupportedChainSpecs::default();
        let file = std::fs::File::open(&file_path)?;
        let reader = std::io::BufReader::new(file);
        let config: Value = serde_json::from_reader(reader)?;
        let chain_spec_list: Vec<ChainSpec> = serde_json::from_value(config)?;
        let new_chain_specs = chain_spec_list
            .iter()
            .map(|cs| (cs.name.clone(), cs.clone()))
            .collect::<HashMap<String, ChainSpec>>();

        // override known specs
        known_chain_specs.0.extend(new_chain_specs);
        Ok(known_chain_specs)
    }

    pub fn supported_networks(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }

    pub fn get_chain_spec(&self, network: &String) -> Option<ChainSpec> {
        self.0.get(network).cloned()
    }
}

/// The condition at which a fork is activated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ForkCondition {
    /// The fork is activated with a certain block.
    Block(BlockNumber),
    /// The fork is activated with a specific timestamp.
    Timestamp(u64),
    /// The fork is not yet active.
    TBD,
}

impl ForkCondition {
    /// Returns whether the condition has been met.
    pub fn active(&self, block_no: BlockNumber, timestamp: u64) -> bool {
        match self {
            ForkCondition::Block(block) => *block <= block_no,
            ForkCondition::Timestamp(ts) => *ts <= timestamp,
            ForkCondition::TBD => false,
        }
    }
}

/// [EIP-1559](https://eips.ethereum.org/EIPS/eip-1559) parameters.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct Eip1559Constants {
    pub base_fee_change_denominator: U256,
    pub base_fee_max_increase_denominator: U256,
    pub base_fee_max_decrease_denominator: U256,
    pub elasticity_multiplier: U256,
}

impl Default for Eip1559Constants {
    /// Defaults to Ethereum network values
    fn default() -> Self {
        Self {
            base_fee_change_denominator: uint!(8_U256),
            base_fee_max_increase_denominator: uint!(8_U256),
            base_fee_max_decrease_denominator: uint!(8_U256),
            elasticity_multiplier: uint!(2_U256),
        }
    }
}

/// Specification of a specific chain.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChainSpec {
    pub name: String,
    pub chain_id: ChainId,
    pub max_spec_id: SpecId,
    pub hard_forks: BTreeMap<SpecId, ForkCondition>,
    pub eip_1559_constants: Eip1559Constants,
    pub l1_contract: Option<Address>,
    pub l2_contract: Option<Address>,
    pub rpc: String,
    pub beacon_rpc: Option<String>,
    // TRICKY: the sgx_verifier_addr is in l1, not in itself
    pub sgx_verifier_address: Option<Address>,
    pub genesis_time: u64,
    pub seconds_per_slot: u64,
    pub is_taiko: bool,
}

impl ChainSpec {
    /// Creates a new configuration consisting of only one specification ID.
    pub fn new_single(
        name: String,
        chain_id: ChainId,
        spec_id: SpecId,
        eip_1559_constants: Eip1559Constants,
        is_taiko: bool,
    ) -> Self {
        ChainSpec {
            name,
            chain_id,
            max_spec_id: spec_id,
            hard_forks: BTreeMap::from([(spec_id, ForkCondition::Block(0))]),
            eip_1559_constants,
            l1_contract: None,
            l2_contract: None,
            rpc: "".to_string(),
            beacon_rpc: None,
            sgx_verifier_address: None,
            genesis_time: 0u64,
            seconds_per_slot: 1u64,
            is_taiko,
        }
    }
    /// Returns the network chain ID.
    pub fn chain_id(&self) -> ChainId {
        self.chain_id
    }
    /// Returns the [SpecId] for a given block number and timestamp or an error if not
    /// supported.
    pub fn active_fork(&self, block_no: BlockNumber, timestamp: u64) -> Result<SpecId> {
        match self.spec_id(block_no, timestamp) {
            Some(spec_id) => {
                if spec_id > self.max_spec_id {
                    bail!("expected <= {:?}, got {spec_id:?}", self.max_spec_id);
                }
                Ok(spec_id)
            }
            None => bail!("no supported fork for block {block_no}"),
        }
    }
    /// Returns the Eip1559 constants
    pub fn gas_constants(&self) -> &Eip1559Constants {
        &self.eip_1559_constants
    }

    fn spec_id(&self, block_no: BlockNumber, timestamp: u64) -> Option<SpecId> {
        for (spec_id, fork) in self.hard_forks.iter().rev() {
            if fork.active(block_no, timestamp) {
                return Some(*spec_id);
            }
        }
        None
    }

    pub fn is_taiko(&self) -> bool {
        self.is_taiko
    }

    pub fn network(&self) -> String {
        self.name.clone()
    }
}

// network enum here either has fixed setting or need known patch fix
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Network {
    /// The Ethereum Mainnet
    #[default]
    Ethereum,
    /// Ethereum testnet holesky
    Holesky,
    /// Taiko A7 tesnet
    TaikoA7,
}

impl ToString for Network {
    fn to_string(&self) -> String {
        match self {
            Network::Ethereum => "ethereum".to_string(),
            Network::Holesky => "holesky".to_string(),
            Network::TaikoA7 => "taiko_a7".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revm_spec_id() {
        let eth_mainnet_spec =
            SupportedChainSpecs::default().get_chain_spec(&Network::Ethereum.to_string());
        assert!(eth_mainnet_spec.spec_id(15_537_393, 0) < Some(SpecId::MERGE));
        assert_eq!(eth_mainnet_spec.spec_id(15_537_394, 0), Some(SpecId::MERGE));
        assert_eq!(eth_mainnet_spec.spec_id(17_034_869, 0), Some(SpecId::MERGE));
        assert_eq!(
            eth_mainnet_spec.spec_id(17_034_870, 0),
            Some(SpecId::SHANGHAI)
        );
    }

    #[ignore]
    #[test]
    fn serde_chain_spec() {
        let spec = ChainSpec {
            name: "test".to_string(),
            chain_id: 1,
            max_spec_id: SpecId::CANCUN,
            hard_forks: BTreeMap::from([
                (SpecId::FRONTIER, ForkCondition::Block(0)),
                (SpecId::MERGE, ForkCondition::Block(15537394)),
                (SpecId::SHANGHAI, ForkCondition::Block(17034870)),
                (SpecId::CANCUN, ForkCondition::Timestamp(1710338135)),
            ]),
            eip_1559_constants: Eip1559Constants {
                base_fee_change_denominator: uint!(8_U256),
                base_fee_max_increase_denominator: uint!(8_U256),
                base_fee_max_decrease_denominator: uint!(8_U256),
                elasticity_multiplier: uint!(2_U256),
            },
            l1_contract: None,
            l2_contract: None,
            rpc: "".to_string(),
            beacon_rpc: None,
            sgx_verifier_address: None,
            genesis_time: 0u64,
            seconds_per_slot: 1u64,
            is_taiko: false,
        };

        let json = serde_json::to_string(&spec).unwrap();
        // write to a file called chain_specs.json
        std::fs::write("chain_spec.json", json).unwrap();

        // read back from the file
        let json = std::fs::read_to_string("chain_spec.json").unwrap();
        let deserialized: ChainSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(spec, deserialized);
    }
}
