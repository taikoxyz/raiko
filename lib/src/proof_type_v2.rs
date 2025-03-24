use serde::de::Visitor;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[macro_export]
macro_rules! generate_proof_types_from_type_alias_paris {
    ( ($first:ident, $first_alias:expr) $(, ($other:ident, $other_alias:expr) )* $(,)? ) => {
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "lowercase")]
        pub enum BaseProofType {
            #[serde(alias = $first_alias)]
            #[default]
            $first,
            $(
                #[serde(alias = $other_alias)]
                $other,
            )*
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum ProofTypeV2 {
            $first,
            $(
                $other,
            )*
            #[doc = r" Pivot variant carrying one of the BaseProofType variants."]
            PivotAnd(BaseProofType),
        }

        impl Default for ProofTypeV2 {
            fn default() -> Self {
                ProofTypeV2::$first
            }
        }
    };
}

// create ProofTypeV2 with BaseTypes & PivotAnd(BaseTypes)
// todo: handle zk_any here?
generate_proof_types_from_type_alias_paris!(
    (Native, "NATIVE"),
    (Sp1, "SP1"),
    (Sgx, "SGX"),
    (Risc0, "RISC0"),
    (Pivot, "PIVOT"),
);

impl Serialize for ProofTypeV2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = match self {
            ProofTypeV2::Native => "native".to_string(),
            ProofTypeV2::Sgx => "sgx".to_string(),
            ProofTypeV2::Sp1 => "sp1".to_string(),
            ProofTypeV2::Risc0 => "risc0".to_string(),
            ProofTypeV2::Pivot => "pivot".to_string(),
            ProofTypeV2::PivotAnd(base) => {
                format!("pivotand{}", format!("{:?}", base).to_lowercase())
            }
        };
        serializer.serialize_str(&s)
    }
}

impl<'de> Deserialize<'de> for ProofTypeV2 {
    fn deserialize<D>(deserializer: D) -> Result<ProofTypeV2, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ProofTypeVisitor;

        impl<'de> Visitor<'de> for ProofTypeVisitor {
            type Value = ProofTypeV2;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(
                    f,
                    "a proof type string like 'native', 'pivotandnative', etc."
                )
            }

            fn visit_str<E>(self, value: &str) -> Result<ProofTypeV2, E>
            where
                E: de::Error,
            {
                match value {
                    "native" => Ok(ProofTypeV2::Native),
                    "sgx" => Ok(ProofTypeV2::Sgx),
                    "sp1" => Ok(ProofTypeV2::Sp1),
                    "risc0" => Ok(ProofTypeV2::Risc0),
                    "pivot" => Ok(ProofTypeV2::Pivot),
                    _ if value.starts_with("pivotand") => {
                        let inner = &value["pivotand".len()..];
                        match inner {
                            "native" => Ok(ProofTypeV2::PivotAnd(BaseProofType::Native)),
                            "sgx" => Ok(ProofTypeV2::PivotAnd(BaseProofType::Sgx)),
                            "sp1" => Ok(ProofTypeV2::PivotAnd(BaseProofType::Sp1)),
                            _ => Err(E::custom(format!("Unknown pivotand variant: {}", inner))),
                        }
                    }
                    _ => Err(E::unknown_variant(
                        value,
                        &["native", "sgx", "sp1", "pivotand<variant>"],
                    )),
                }
            }
        }

        deserializer.deserialize_str(ProofTypeVisitor)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_lowercase_ser() {
        let proof_type = ProofTypeV2::Native;
        let serialized = serde_json::to_string(&proof_type).unwrap();
        assert_eq!(serialized, "\"native\"");
    }

    #[test]
    fn test_lowercase_de() {
        let deserialized: ProofTypeV2 = serde_json::from_str("\"native\"").unwrap();
        assert_eq!(deserialized, ProofTypeV2::Native);
    }

    #[test]
    fn test_proof_type_v2_se() {
        let proof_type = ProofTypeV2::PivotAnd(BaseProofType::Native);
        let serialized = serde_json::to_string(&proof_type).unwrap();
        assert_eq!(serialized, "\"pivotandnative\"");
    }

    #[test]
    fn test_proof_type_v2_de() {
        let deserialized: ProofTypeV2 = serde_json::from_str("\"pivotandsgx\"").unwrap();
        assert_eq!(deserialized, ProofTypeV2::PivotAnd(BaseProofType::Sgx));
    }
}
