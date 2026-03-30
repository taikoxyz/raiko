use zisk_sdk::ZiskProofWithPublicValues;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "test-proof/final_snark_proof.bin".to_string());

    let p = ZiskProofWithPublicValues::load(&path).unwrap_or_else(|e| {
        eprintln!("Error: failed to load '{}': {}", path, e);
        std::process::exit(1);
    });

    // programVK as 4 × uint64 (little-endian)
    let vk: Vec<u64> = p.program_vk.vk[..32]
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
        .collect();

    println!("programVK (4 x uint64, for Solidity):");
    for (i, v) in vk.iter().enumerate() {
        println!("  uint64({}),  // vk[{}]", v, i);
    }
    println!();

    // publicValues (Solidity encoding)
    let pub_sol = p.publics.public_bytes_solidity();
    println!("publicValues (hex, {} bytes):", pub_sol.len());
    println!("  0x{}", hex::encode(&pub_sol));
    println!();

    // proofBytes
    match &p.proof {
        zisk_sdk::ZiskProof::Plonk(bytes) => {
            println!("proofBytes (PLONK, {} bytes):", bytes.len());
            println!("  0x{}", hex::encode(bytes));
        }
        zisk_sdk::ZiskProof::Fflonk(bytes) => {
            println!("proofBytes (Fflonk, {} bytes):", bytes.len());
            println!("  0x{}", hex::encode(bytes));
        }
        other => {
            eprintln!(
                "Warning: not a SNARK proof (variant: {:?})",
                std::mem::discriminant(other)
            );
        }
    }
}
