use crate::{
    interfaces::{ProofRequest, ProofType},
    provider::rpc::RpcBlockDataProvider,
    ChainSpec, Raiko,
};
use alloy_primitives::Address;
use alloy_provider::Provider;
use clap::ValueEnum;
use raiko_lib::{
    consts::{Network, SupportedChainSpecs},
    input::BlobProofType,
    primitives::B256,
};
use serde_json::{json, Value};
use std::{collections::HashMap, env};

fn get_proof_type_from_env() -> ProofType {
    let proof_type = env::var("TARGET").unwrap_or("native".to_string());
    ProofType::from_str(&proof_type, true).unwrap()
}

fn is_ci() -> bool {
    let ci = env::var("CI").unwrap_or("0".to_string());
    ci == "1"
}

fn test_proof_params() -> HashMap<String, Value> {
    let mut prover_args = HashMap::new();
    prover_args.insert(
        "native".to_string(),
        json! {
            {
                "write_guest_input_path": null
            }
        },
    );
    prover_args.insert(
        "risc0".to_string(),
        json! {
            {
                "bonsai": false,
                "snark": false,
                "profile": true,
                "execution_po2": 18
            }
        },
    );
    prover_args.insert(
        "sgx".to_string(),
        json! {
            {
                "instance_id": 121,
                "setup": true,
                "bootstrap": true,
                "prove": true,
            }
        },
    );
    prover_args
}

async fn prove_block(
    l1_chain_spec: ChainSpec,
    taiko_chain_spec: ChainSpec,
    proof_request: ProofRequest,
) {
    let provider = RpcBlockDataProvider::new(&taiko_chain_spec.rpc, proof_request.block_number - 1)
        .expect("Could not create RpcBlockDataProvider");
    let raiko = Raiko::new(l1_chain_spec, taiko_chain_spec, proof_request.clone());
    let input = raiko
        .generate_input(provider)
        .await
        .expect("input generation failed");
    let output = raiko.get_output(&input).expect("output generation failed");
    let _proof = raiko
        .prove(input, &output, None)
        .await
        .expect("proof generation failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_prove_block_taiko_a7() {
    let proof_type = get_proof_type_from_env();
    let l1_network = Network::Holesky.to_string();
    let network = Network::TaikoA7.to_string();
    // Give the CI an simpler block to test because it doesn't have enough memory.
    // Unfortunately that also means that kzg is not getting fully verified by CI.
    let block_number = if is_ci() { 105987 } else { 101368 };
    let taiko_chain_spec = SupportedChainSpecs::default()
        .get_chain_spec(&network)
        .unwrap();
    let l1_chain_spec = SupportedChainSpecs::default()
        .get_chain_spec(&l1_network)
        .unwrap();

    let proof_request = ProofRequest {
        block_number,
        network,
        graffiti: B256::ZERO,
        prover: Address::ZERO,
        l1_network,
        proof_type,
        blob_proof_type: BlobProofType::ProofOfEquivalence,
        prover_args: test_proof_params(),
    };
    prove_block(l1_chain_spec, taiko_chain_spec, proof_request).await;
}

async fn get_recent_block_num(chain_spec: &ChainSpec) -> u64 {
    let provider = RpcBlockDataProvider::new(&chain_spec.rpc, 0).unwrap();
    let height = provider.provider.get_block_number().await.unwrap();
    height - 100
}

#[tokio::test(flavor = "multi_thread")]
async fn test_prove_block_ethereum() {
    let proof_type = get_proof_type_from_env();
    // Skip test on SP1 for now because it's too slow on CI
    if !(is_ci() && proof_type == ProofType::Sp1) {
        let network = Network::Ethereum.to_string();
        let l1_network = Network::Ethereum.to_string();
        let taiko_chain_spec = SupportedChainSpecs::default()
            .get_chain_spec(&network)
            .unwrap();
        let l1_chain_spec = SupportedChainSpecs::default()
            .get_chain_spec(&l1_network)
            .unwrap();
        let block_number = get_recent_block_num(&taiko_chain_spec).await;
        println!(
            "test_prove_block_ethereum in block_number: {}",
            block_number
        );
        let proof_request = ProofRequest {
            block_number,
            network,
            graffiti: B256::ZERO,
            prover: Address::ZERO,
            l1_network,
            proof_type,
            blob_proof_type: BlobProofType::ProofOfEquivalence,
            prover_args: test_proof_params(),
        };
        prove_block(l1_chain_spec, taiko_chain_spec, proof_request).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_prove_block_taiko_mainnet() {
    let proof_type = get_proof_type_from_env();
    // Skip test on SP1 for now because it's too slow on CI
    if !(is_ci() && proof_type == ProofType::Sp1) {
        let network = Network::TaikoMainnet.to_string();
        let l1_network = Network::Ethereum.to_string();
        let taiko_chain_spec = SupportedChainSpecs::default()
            .get_chain_spec(&network)
            .unwrap();
        let l1_chain_spec = SupportedChainSpecs::default()
            .get_chain_spec(&l1_network)
            .unwrap();
        let block_number = get_recent_block_num(&taiko_chain_spec).await;
        println!(
            "test_prove_block_taiko_mainnet in block_number: {}",
            block_number
        );
        let proof_request = ProofRequest {
            block_number,
            network,
            graffiti: B256::ZERO,
            prover: Address::ZERO,
            l1_network,
            proof_type,
            blob_proof_type: BlobProofType::ProofOfEquivalence,
            prover_args: test_proof_params(),
        };
        prove_block(l1_chain_spec, taiko_chain_spec, proof_request).await;
    }
}
