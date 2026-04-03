use raiko_core::interfaces::RealTimeProofRequest;
use raiko_reqpool::{
    ImageId, RealTimeInputRequestEntity, RealTimeInputRequestKey, RealTimeProofRequestEntity,
    RealTimeProofRequestKey, RequestKey,
};

/// Create RealTime-specific request keys and entities
fn create_realtime_requests(
    request: &RealTimeProofRequest,
    image_id: &ImageId,
) -> (
    RequestKey,
    RequestKey,
    RealTimeInputRequestEntity,
    RealTimeProofRequestEntity,
) {
    // RealTime: one proposal per request, no batching
    let input_request_key = RequestKey::RealTimeGuestInput(RealTimeInputRequestKey::new(
        request.l2_block_numbers.clone(),
        request.l2_block_hashes.clone(),
        request.l1_network.clone(),
        request.network.clone(),
        request.last_finalized_block_hash,
    ));

    let actual_prover_address = request.prover.to_string();
    let proof_request_key =
        RequestKey::RealTimeProof(RealTimeProofRequestKey::new_with_input_key_and_image_id(
            RealTimeInputRequestKey::new(
                request.l2_block_numbers.clone(),
                request.l2_block_hashes.clone(),
                request.l1_network.clone(),
                request.network.clone(),
                request.last_finalized_block_hash,
            ),
            request.proof_type,
            actual_prover_address,
            image_id.clone(),
        ));

    let input_request_entity = RealTimeInputRequestEntity::new(
        request.l2_block_numbers.clone(),
        request.network.clone(),
        request.l1_network.clone(),
        request.prover,
        request.blob_proof_type.clone(),
        request.max_anchor_block_number,
        request.signal_slots.clone(),
        request.last_finalized_block_hash,
        request.basefee_sharing_pctg,
        request.checkpoint.clone(),
        request.sources.clone(),
        request.blobs.clone(),
    );

    let proof_request_entity = RealTimeProofRequestEntity::new_with_guest_input_entity(
        input_request_entity.clone(),
        request.proof_type,
        request.prover_args.clone().into(),
    );

    (
        input_request_key,
        proof_request_key,
        input_request_entity,
        proof_request_entity,
    )
}

/// Build only the proof request key for status lookups (polling).
pub fn make_proof_request_key(request: &RealTimeProofRequest, image_id: &ImageId) -> RequestKey {
    let actual_prover_address = request.prover.to_string();
    RequestKey::RealTimeProof(RealTimeProofRequestKey::new_with_input_key_and_image_id(
        RealTimeInputRequestKey::new(
            request.l2_block_numbers.clone(),
            request.l2_block_hashes.clone(),
            request.l1_network.clone(),
            request.network.clone(),
            request.last_finalized_block_hash,
        ),
        request.proof_type,
        actual_prover_address,
        image_id.clone(),
    ))
}

/// Process a RealTime request and return the necessary data for the handler.
/// Unlike Shasta, there is exactly one proposal per request (no batching).
pub fn process_realtime_request(
    request: &RealTimeProofRequest,
    image_id: &ImageId,
) -> (
    RequestKey,
    RequestKey,
    raiko_reqpool::RequestEntity,
    raiko_reqpool::RequestEntity,
) {
    let (input_key, proof_key, input_entity, proof_entity) =
        create_realtime_requests(request, image_id);

    (
        input_key,
        proof_key,
        input_entity.into(),
        proof_entity.into(),
    )
}
