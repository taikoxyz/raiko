use std::fs::File;
use std::io::Seek;
use std::io::Write;
use std::time::Instant;

use p3_challenger::CanObserve;

use sp1_core::{
    runtime::{ExecutionRecord, ExecutionState, Program, Runtime, ShardingConfig},
    stark::{LocalProver, MachineRecord, RiscvAir, ShardProof, StarkGenericConfig},
    utils::{baby_bear_poseidon2::Val, BabyBearPoseidon2, SP1CoreOpts, SP1CoreProverError},
};
use sp1_sdk::CoreSC;
use sp1_sdk::{SP1PublicValues, SP1Stdin};

use crate::ELF;

use super::partial_proof_request::PartialProofRequest;

fn trace_checkpoint(program: Program, file: &File, opts: SP1CoreOpts) -> ExecutionRecord {
    let mut reader = std::io::BufReader::new(file);

    let state = bincode::deserialize_from(&mut reader).expect("failed to deserialize state");

    let mut runtime = Runtime::recover(program.clone(), state, opts);

    let (events, _) =
        tracing::debug_span!("runtime.trace").in_scope(|| runtime.execute_record().unwrap());

    events
}

pub fn commit(
    program: Program,
    stdin: &SP1Stdin,
    nb_workers: usize,
) -> Result<(Vec<ExecutionState>, SP1PublicValues, PartialProofRequest), SP1CoreProverError> {
    let proving_start = Instant::now();

    let config = CoreSC::default();

    let now = Instant::now();

    log::debug!("Starting prover");

    let mut opts = SP1CoreOpts::default();

    // FIXME: Is this the most efficient ?
    opts.shard_batch_size = 1;

    // Execute the program.
    let mut runtime = Runtime::new(program.clone(), opts);

    runtime.write_vecs(&stdin.buffer);

    for proof in stdin.proofs.iter() {
        runtime.write_proof(proof.0.clone(), proof.1.clone());
    }

    log::debug!("Runtime setup took {:?}", now.elapsed());

    let now = Instant::now();

    // Setup the machine.
    let machine = RiscvAir::machine(config.clone());

    let (_pk, vk) = machine.setup(runtime.program.as_ref());

    log::debug!("Machine setup took {:?}", now.elapsed());

    let now = Instant::now();

    // Execute the program, saving checkpoints at the start of every `shard_batch_size` cycle range.
    let mut checkpoints_files = Vec::new();
    let mut checkpoints_states = Vec::new();

    let (public_values_stream, public_values) = loop {
        let checkpoint_start = Instant::now();

        // Execute the runtime until we reach a checkpoint.
        let (checkpoint, done) = runtime
            .execute_state()
            .map_err(SP1CoreProverError::ExecutionError)?;

        checkpoints_states.push(checkpoint.clone());

        log::debug!("Checkpoint took {:?}", checkpoint_start.elapsed());

        // Save the checkpoint to a temp file.
        let mut tempfile = tempfile::tempfile().map_err(SP1CoreProverError::IoError)?;

        let mut writer = std::io::BufWriter::new(&mut tempfile);

        bincode::serialize_into(&mut writer, &checkpoint)
            .map_err(SP1CoreProverError::SerializationError)?;

        writer.flush().map_err(SP1CoreProverError::IoError)?;

        drop(writer);

        tempfile
            .seek(std::io::SeekFrom::Start(0))
            .map_err(SP1CoreProverError::IoError)?;

        checkpoints_files.push(tempfile);

        // If we've reached the final checkpoint, break out of the loop.
        if done {
            break (
                std::mem::take(&mut runtime.state.public_values_stream),
                runtime.record.public_values,
            );
        }
    };

    log::debug!("Total checkpointing took {:?}", now.elapsed());

    let now = Instant::now();

    // For each checkpoint, generate events, shard them, commit shards, and observe in challenger.
    let sharding_config = ShardingConfig::default();

    let mut challenger = machine.config().challenger();

    vk.observe_into(&mut challenger);

    log::debug!("Challenger setup took {:?}", now.elapsed());

    opts.shard_batch_size = (checkpoints_files.len() as f64 / nb_workers as f64).ceil() as usize;

    let checkpoints_files = checkpoints_files
        .chunks(opts.shard_batch_size)
        .map(|chunk| &chunk[0])
        .collect::<Vec<_>>();

    let checkpoints_states = checkpoints_states
        .chunks(opts.shard_batch_size)
        .map(|chunk| chunk[0].clone())
        .collect::<Vec<_>>();

    let now = Instant::now();

    for checkpoint_file in checkpoints_files.iter() {
        let trace_checkpoint_time = Instant::now();

        let mut record = trace_checkpoint(program.clone(), checkpoint_file, opts);
        record.public_values = public_values;

        log::debug!(
            "Checkpoint trace took {:?}",
            trace_checkpoint_time.elapsed()
        );

        let sharding_time = Instant::now();

        // Shard the record into shards.
        let checkpoint_shards =
            tracing::info_span!("shard").in_scope(|| machine.shard(record, &sharding_config));

        log::debug!("Checkpoint sharding took {:?}", sharding_time.elapsed());

        let commit_time = Instant::now();

        // Commit to each shard.
        let (commitments, _commit_data) = tracing::info_span!("commit")
            .in_scope(|| LocalProver::commit_shards(&machine, &checkpoint_shards, opts));

        log::debug!("Checkpoint commit took {:?}", commit_time.elapsed());

        // Observe the commitments.
        for (commitment, shard) in commitments.into_iter().zip(checkpoint_shards.iter()) {
            challenger.observe(commitment);
            challenger.observe_slice(&shard.public_values::<Val>()[0..machine.num_pv_elts()]);
        }
    }

    log::debug!("Checkpoints commitment took {:?}", now.elapsed());

    log::debug!("Total setup took {:?}", proving_start.elapsed());

    let partial_proof_request = PartialProofRequest {
        checkpoint_id: 0,
        checkpoint_data: ExecutionState::default(),
        challenger,
        public_values,
        shard_batch_size: opts.shard_batch_size,
    };

    Ok((
        checkpoints_states,
        SP1PublicValues::from(&public_values_stream),
        partial_proof_request,
    ))
}

pub fn prove_partial(request_data: &PartialProofRequest) -> Vec<ShardProof<BabyBearPoseidon2>> {
    let now = Instant::now();

    let program = Program::from(ELF);

    let config = CoreSC::default();

    let mut opts = SP1CoreOpts::default();
    opts.shard_batch_size = request_data.shard_batch_size;

    let machine = RiscvAir::machine(config.clone());

    let (pk, _vk) = machine.setup(&program);

    let sharding_config = ShardingConfig::default();

    log::debug!("Machine setup took {:?}", now.elapsed());

    let mut res = vec![];

    let now = Instant::now();

    let proving_time = Instant::now();

    let mut events = {
        let mut runtime =
            Runtime::recover(program.clone(), request_data.checkpoint_data.clone(), opts);

        let (events, _) =
            tracing::debug_span!("runtime.trace").in_scope(|| runtime.execute_record().unwrap());

        events
    };

    log::debug!("Runtime recover took {:?}", now.elapsed());

    let now = Instant::now();

    events.public_values = request_data.public_values;

    let checkpoint_shards =
        tracing::info_span!("shard").in_scope(|| machine.shard(events, &sharding_config));

    log::debug!("Checkpoint sharding took {:?}", now.elapsed());

    let mut proofs = checkpoint_shards
        .into_iter()
        .enumerate()
        .map(|(i, shard)| {
            log::info!("Proving shard {}/{}", i + 1, request_data.shard_batch_size);

            let config = machine.config();

            log::debug!("Commit main");

            let now = Instant::now();

            let shard_data =
                LocalProver::commit_main(config, &machine, &shard, shard.index() as usize);

            log::debug!("Commit main took {:?}", now.elapsed());

            let now = Instant::now();

            let chip_ordering = shard_data.chip_ordering.clone();
            let ordered_chips = machine
                .shard_chips_ordered(&chip_ordering)
                .collect::<Vec<_>>()
                .to_vec();

            log::debug!("Shard chips ordering took {:?}", now.elapsed());

            let now = Instant::now();

            log::debug!("Actually prove shard");

            let proof = LocalProver::prove_shard(
                config,
                &pk,
                &ordered_chips,
                shard_data,
                &mut request_data.challenger.clone(),
            );

            log::debug!("Prove shard took {:?}", now.elapsed());

            proof
        })
        .collect::<Vec<_>>();

    res.append(&mut proofs);

    log::info!("Proving took {:?}", proving_time.elapsed());

    return res;
}
