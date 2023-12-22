//! Generate different proofs for the taiko protocol.

pub mod cache;
pub mod pse_zk;
pub mod sgx;

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
pub enum ProofType {
    #[allow(dead_code)]
    PseZk,
    Sgx,
}
