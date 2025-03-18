use raiko_lib::proof_type::ProofType;

pub trait ToLabel {
    fn to_label(&self) -> &'static str;
}

impl ToLabel for &'static str {
    fn to_label(&self) -> &'static str {
        self
    }
}

impl ToLabel for &ProofType {
    fn to_label(&self) -> &'static str {
        match self {
            ProofType::Native => "native",
            ProofType::Sp1 => "sp1",
            ProofType::Sgx => "sgx",
            ProofType::Risc0 => "risc0",
        }
    }
}
