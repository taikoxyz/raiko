use alloy_sol_types::SolValue;
use anyhow::Result;
use zeth_primitives::{
    block::Header,
    ethers::{from_ethers_h256, from_ethers_u256},
    keccak,
    taiko::{
        deposits_hash, string_to_bytes32, BlockMetadata, EthDeposit, ProtocolInstance, Transition,
        ANCHOR_GAS_LIMIT,
    },
    TxHash, U256,
};

use crate::taiko::host::TaikoExtra;

pub fn assemble_protocol_instance(extra: &TaikoExtra, header: &Header) -> Result<ProtocolInstance> {
    let tx_list_hash = extra
        .tx_blob_hash
        .unwrap_or(TxHash::from(keccak::keccak(extra.l2_tx_list.as_slice())));
    let deposits: Vec<EthDeposit> = extra
        .l2_withdrawals
        .iter()
        .map(|w| EthDeposit {
            recipient: w.address,
            amount: w.amount as u128,
            id: w.index,
        })
        .collect();
    let deposits_hash = deposits_hash(&deposits);
    let extra_data = string_to_bytes32(&header.extra_data);
    let prevrando = if cfg!(feature = "pos") {
        from_ethers_h256(extra.l1_next_block.mix_hash.unwrap_or_default()).into()
    } else {
        from_ethers_u256(extra.l1_next_block.difficulty)
    };
    let difficulty = keccak::keccak(
        (
            prevrando,
            header.number,
            U256::from(extra.l1_next_block.number.unwrap_or_default().as_u64()),
        )
            .abi_encode_packed(),
    );
    let gas_limit: u64 = header.gas_limit.try_into().unwrap();
    let mut pi = ProtocolInstance {
        transition: Transition {
            parentHash: header.parent_hash,
            blockHash: header.hash(),
            stateRoot: from_ethers_h256(extra.l2_fini_block.state_root),
            graffiti: extra.graffiti,
        },
        block_metadata: BlockMetadata {
            l1Hash: extra.l1_hash,
            difficulty: difficulty.into(),
            blobHash: tx_list_hash,
            extraData: extra_data.into(),
            depositsHash: deposits_hash,
            coinbase: header.beneficiary,
            id: header.number,
            gasLimit: (gas_limit - ANCHOR_GAS_LIMIT) as u32,
            timestamp: header.timestamp.try_into().unwrap(),
            l1Height: extra.l1_height,
            minTier: extra.block_proposed.meta.minTier,
            blobUsed: extra.tx_blob_hash.is_some(),
            parentMetaHash: extra.block_proposed.meta.parentMetaHash,
            sender: extra.block_proposed.meta.sender,
        },
        prover: extra.prover,
        chain_id: extra.chain_id,
        sgx_verifier_address: extra.sgx_verifier_address,
    };
    #[cfg(not(target_os = "zkvm"))]
    {
        crate::taiko::verify::verify(header, &mut pi, extra)?;
    }
    Ok(pi)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_assemble_protocol_difficulty() {
        let difficulty =
            keccak::keccak((U256::from(1234u64), 5678u64, U256::from(4321u64)).abi_encode_packed());
        assert_eq!(
            hex::encode(difficulty),
            "ed29e631be1dc988025d0e874bf84fe27894c9c0a8034b3a0a212ccbf4216a79"
        );

        let difficulty = keccak::keccak((U256::from(0), 0u64, U256::from(0)).abi_encode_packed());
        assert_eq!(
            hex::encode(difficulty),
            "3cac317908c699fe873a7f6ee4e8cd63fbe9918b2315c97be91585590168e301"
        );
    }

    #[ignore]
    #[test]
    fn test_calc_difficulty() {
        let buf = (U256::from(0), 1000u64, U256::from(6093)).abi_encode_packed();
        println!("buf: {:?} ", buf);

        let difficulty =
            keccak::keccak((U256::from(0), 1000u64, U256::from(6093)).abi_encode_packed());
        println!(
            "{} {} {} difficulty: {:?}",
            U256::from(0),
            1000,
            U256::from(6093),
            hex::encode(difficulty)
        );
    }
}
