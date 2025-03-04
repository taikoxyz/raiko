use raiko_lib::input::{GuestInput, GuestOutput};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use utoipa::ToSchema;

use crate::interfaces::{ProofRequest, RaikoError, RaikoResult};

/// for raiko use
/// to support use case like raiko reads guest input from remote raiko
///

#[derive(Debug, Serialize, ToSchema, Deserialize)]
/// The response body of a proof request.
pub struct ProofResponse {
    #[schema(value_type = Option<GuestInput>)]
    /// The input of the prover.
    pub input: Option<GuestInput>,
    #[schema(value_type = Option<GuestOutputDoc>)]
    /// The output of the prover.
    pub output: Option<GuestOutput>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok { data: ProofResponse },
    Error { error: String, message: String },
}

#[allow(async_fn_in_trait)]
pub trait GuestInputProvider {
    async fn get_guest_input(&self, url: &str, request: &ProofRequest) -> RaikoResult<GuestInput>;
}

pub struct GuestInputProviderImpl;

impl GuestInputProvider for GuestInputProviderImpl {
    async fn get_guest_input(
        &self,
        input_provider_url: &str,
        request: &ProofRequest,
    ) -> RaikoResult<GuestInput> {
        // use request client to post request
        let url = format!("{}/v1/input", input_provider_url.trim_end_matches('/'),);
        info!("Retrieve side car guest input from {url}.");

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let response = reqwest::Client::new()
            .post(url.clone())
            .headers(headers)
            .json(request)
            .send()
            .await
            .map_err(|e| {
                RaikoError::RPC(
                    format!("Failed to send POST request: {}", e.to_string()).to_owned(),
                )
            })?;

        if !response.status().is_success() {
            warn!(
                "Request {url} failed with status code: {}",
                response.status()
            );
            return Err(RaikoError::RPC(
                format!("Request failed with status code: {}", response.status()).to_owned(),
            ));
        }
        let response_text = response.text().await.map_err(|e| {
            RaikoError::RPC(format!("Failed to get response text: {}", e.to_string()).to_owned())
        })?;
        debug!("Received response: {}", &response_text);
        let response_json: Status = serde_json::from_str(&response_text).map_err(|e| {
            RaikoError::RPC(format!("Failed to parse JSON: {}", e.to_string()).to_owned())
        })?;

        match response_json {
            Status::Ok { data } => data
                .input
                .ok_or_else(|| RaikoError::RPC("No guest input in response".to_string())),
            Status::Error { error, message } => {
                Err(RaikoError::RPC(format!("Error: {} - {}", error, message)))
            }
        }
    }
}

#[cfg(test)]
mod test {
    use core::fmt::Debug;
    use std::u128;

    use reth_primitives::{TransactionSigned, TxEip1559};
    use serde::{Deserialize, Serialize};
    use serde_with::serde_as;

    use utoipa::ToSchema;

    #[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]

    pub enum BlockProposedForkRef {
        #[default]
        Nothing,
        Ontake(BlockProposedV2Ref),
    }

    #[serde_as]
    #[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
    pub struct TaikoGuestInputRef {
        pub anchor_tx: Option<TransactionSigned>,
        pub block_proposed: BlockProposedForkRef,
    }

    #[serde_as]
    #[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
    pub struct GuestInputRef {
        /// Taiko specific data
        pub taiko: TaikoGuestInputRef,
    }

    #[derive(Debug, Serialize, ToSchema, Deserialize, PartialEq)]
    /// The response body of a proof request.
    pub struct ProofResponse {
        pub input: Option<GuestInputRef>,
    }

    #[derive(Debug, Deserialize, Serialize, ToSchema, PartialEq)]
    // #[serde(tag = "status", rename_all = "lowercase")]
    #[serde(rename_all = "lowercase")]
    pub enum Status {
        Ok { data: ProofResponse },
        Error { error: String, message: String },
    }

    #[test]
    fn test_ser_der_status_for_u128() {
        let mut tx = TxEip1559::default();
        tx.max_fee_per_gas = u128::MAX - 5;
        let anchor_tx = TransactionSigned {
            hash: Default::default(),
            signature: Default::default(),
            transaction: reth_primitives::Transaction::Eip1559(tx),
        };
        let input = GuestInputRef {
            taiko: TaikoGuestInputRef {
                block_proposed: BlockProposedForkRef::Ontake(BlockProposedV2Ref {
                    blockId: Default::default(),
                    meta: BlockMetadataV2Ref {
                        livenessBond: u128::MAX >> 33,
                    },
                }),
                anchor_tx: Some(anchor_tx),
            },
        };

        let status = Status::Ok {
            data: ProofResponse { input: Some(input) },
        };

        let serialized = serde_json::to_string(&status).unwrap();
        let deserialized: Status = serde_json::from_str(&serialized).unwrap();

        // print!("{:?}", deserialized);
        assert_eq!(deserialized, status);
    }

    #[test]
    fn test_ser_der() {
        let input = GuestInputRef {
            taiko: TaikoGuestInputRef {
                block_proposed: BlockProposedForkRef::Ontake(BlockProposedV2Ref {
                    blockId: Default::default(),
                    meta: BlockMetadataV2Ref {
                        livenessBond: u128::MAX >> 33,
                    },
                }),
                ..Default::default()
            },
        };

        let serialized = serde_json::to_string(&input).unwrap();
        let deserialized: GuestInputRef = serde_json::from_str(&serialized).unwrap();

        // print!("{:?}", deserialized);
        assert_eq!(input, deserialized);
    }

    #[test]
    fn test_deser_files() {
        let json_str = include_str!("../../../response.log");
        let deserialized = serde_json::from_str::<Status>(json_str);
        println!("{:?}", deserialized);
    }

    use alloy_sol_types::sol;
    sol! {
        #[derive(Serialize, Deserialize, Debug)]
        struct SolTypeReference {
            uint96 data;
        }

        #[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
        struct BlockMetadataV2Ref {
            uint96 livenessBond;
        }

        #[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
        event BlockProposedV2Ref(uint256 indexed blockId, BlockMetadataV2Ref meta);

    }

    #[derive(Serialize, Deserialize, Debug)]
    struct JsonRef {
        data: u128,
    }

    #[test]
    fn test_simple_u128_json() {
        let json_str = serde_json::to_string(&JsonRef {
            data: u128::MAX - 1,
        });
        println!("json_str = {:?}.", json_str);

        let json_obj = serde_json::from_str::<JsonRef>(&json_str.unwrap());
        println!("json_obj = {:?}.", json_obj);

        let test_json_str = "{\"data\": 125000000000000000000}";
        let json_obj = serde_json::from_str::<JsonRef>(test_json_str);
        println!("json_obj from str= {:?}.", json_obj);
    }

    #[test]
    fn test_simple_sol_u96_json() {
        let test_json_str = "{\"data\": 125000000000000000000}";
        let json_obj = serde_json::from_str::<SolTypeReference>(test_json_str);
        println!("json_obj from str= {:?}.", json_obj);
    }
}
