#![cfg(feature = "enable")]

use alloy_primitives::B256;
use hex::ToHex;
use log::warn;
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestInput, GuestOutput,
        ZkAggregationGuestInput,
    },
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::fmt::Debug;
use tracing::{debug, info as traicing_info};

use powdr::Session;

pub struct PowdrProver;

const INPUT_FD: u32 = 42;
const OUTPUT_FD: u32 = 1;

impl Prover for PowdrProver {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        // TODO CBOR fails at serializing GuestInput, so we're wrapping it in bincode.
        // Should change this internally in powdr.
        let input_vec = bincode::serialize(&input).unwrap();
        let mut session = Session::builder()
            // TODO Should read the compiled guest.asm directly.
            .guest_path("provers/powdr/guest")
            .out_path("provers/powdr/driver/powdr-target")
            .build()
            .write(INPUT_FD, &input_vec);

        session.run();

        Ok(Default::default())
    }

    async fn aggregate(
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        println!("PowdrAggregate run");
        Ok(Default::default())
    }

    async fn cancel(key: ProofKey, id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        Ok(())
    }
}
