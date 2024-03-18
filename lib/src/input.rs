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

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    block::Header,
    transactions::{ethereum::EthereumTxEssence, Transaction, TxEssence},
    trie::MptNode,
    withdrawal::Withdrawal,
    Address, Bytes, FixedBytes, B256, U256,
};

use crate::taiko::protocol_instance::{TaikoExtra, TaikoExtraForVM};

/// External block input.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Input<E: TxEssence> {
    /// Previous block header
    pub parent_header: Header,
    /// Address to which all priority fees in this block are transferred.
    pub beneficiary: Address,
    /// Scalar equal to the current limit of gas expenditure per block.
    pub gas_limit: U256,
    /// Scalar corresponding to the seconds since Epoch at this block's inception.
    pub timestamp: U256,
    /// Arbitrary byte array containing data relevant for this block.
    pub extra_data: Bytes,
    /// Hash previously used for the PoW now containing the RANDAO value.
    pub mix_hash: B256,
    /// List of transactions for execution
    pub transactions: Vec<Transaction<E>>,
    /// List of stake withdrawals for execution
    pub withdrawals: Vec<Withdrawal>,
    /// State trie of the parent block.
    pub parent_state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    pub parent_storage: HashMap<Address, StorageEntry>,
    /// The code of all unique contracts.
    pub contracts: Vec<Bytes>,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<Header>,
    /// Base fee per gas
    pub base_fee_per_gas: U256,
}


#[derive(Serialize, Deserialize)]
pub struct Risc0Input{
    pub input: Input<EthereumTxEssence>,
    pub extra: TaikoExtraForVM,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Output {
    Success((Header, FixedBytes<32>)),
    Failure,
}

pub type StorageEntry = (MptNode, Vec<U256>);

#[cfg(test)]
mod tests {
    use zeth_primitives::transactions::ethereum::EthereumTxEssence;
    use risc0_zkvm::serde::{from_slice, to_vec};
    use super::*;

    #[test]
    fn input_serde_roundtrip() {
        let input = Input::<EthereumTxEssence> {
            parent_header: Default::default(),
            beneficiary: Default::default(),
            gas_limit: Default::default(),
            timestamp: Default::default(),
            extra_data: Default::default(),
            mix_hash: Default::default(),
            transactions: vec![],
            withdrawals: vec![],
            parent_state_trie: Default::default(),
            parent_storage: Default::default(),
            contracts: vec![],
            ancestor_headers: vec![],
            base_fee_per_gas: Default::default(),
        };
        let _: Input<EthereumTxEssence> =
            bincode::deserialize(&bincode::serialize(&input).unwrap()).unwrap();
        let input_vec = to_vec(&input).unwrap();
        // println!("{:?}", input_vec);
        let input_vec_de: Input::<EthereumTxEssence> = from_slice(&input_vec).unwrap();
        println!("{:?}", input_vec_de);

        let mut extra = TaikoExtraForVM {
            l1_hash: Default::default(),
            l1_height: Default::default(),
            l2_tx_list: Default::default(),
            tx_blob_hash: Default::default(),
            prover:Default::default(),
            graffiti: Default::default(),
            l2_withdrawals:Default::default(),
            block_proposed:Default::default(),
            chain_id: Default::default(),
            sgx_verifier_address: Default::default(),
            blob_data: Default::default(),
        };
        // extra.l1_next_block.other.insert("test".to_string(), serde_json::json!(true));
        // extra.l2_fini_block.other.insert("test".to_string(), serde_json::json!(true));
        let extra_vec = to_vec(&extra).unwrap();
        // println!("{:?}", extra_vec);
        let extra_de: TaikoExtraForVM = from_slice(&extra_vec).unwrap();
        println!("{:?}", extra_de);

        let r0_input = Risc0Input {
            input,
            extra,
        };
        let r0_input_vec = to_vec(&r0_input).unwrap();
        println!("{:?}", r0_input_vec);

        let r0_input_de: Risc0Input = from_slice(&r0_input_vec).unwrap();
        println!("{:?}", r0_input_de.extra);
    }
}
