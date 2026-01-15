use raiko_core::interfaces::{ShastaProofRequest, ShastaProposal};
use raiko_reqpool::{
    ImageId, RequestKey, ShastaInputRequestEntity, ShastaInputRequestKey, ShastaProofRequestEntity,
    ShastaProofRequestKey,
};

/// Create Shasta-specific request keys and entities for a batch
pub fn create_shasta_requests(
    batch_request: &ShastaProofRequest,
    image_id: &ImageId,
) -> Vec<(
    RequestKey,
    RequestKey,
    ShastaInputRequestEntity,
    ShastaProofRequestEntity,
)> {
    let mut requests = Vec::with_capacity(batch_request.proposals.len());

    for proposal in batch_request.proposals.iter() {
        let ShastaProposal {
            proposal_id,
            checkpoint,
            l1_inclusion_block_number,
            l2_block_numbers,
            last_anchor_block_number,
        } = proposal;
        // Create Shasta input request key
        let input_request_key = RequestKey::ShastaGuestInput(ShastaInputRequestKey::new(
            *proposal_id, // proposal_id
            batch_request.l1_network.clone(),
            batch_request.network.clone(),
        ));

        // Create Shasta proof request key
        let actual_prover_address = batch_request.prover.to_string();
        let request_key =
            RequestKey::ShastaProof(ShastaProofRequestKey::new_with_input_key_and_image_id(
                ShastaInputRequestKey::new(
                    *proposal_id, // proposal_id
                    batch_request.l1_network.clone(),
                    batch_request.network.clone(),
                ),
                batch_request.proof_type,
                actual_prover_address,
                image_id.clone(),
            ));

        // Create Shasta input request entity
        let input_request_entity = ShastaInputRequestEntity::new(
            *proposal_id, // proposal_id
            *l1_inclusion_block_number,
            batch_request.network.clone(),
            batch_request.l1_network.clone(),
            batch_request.prover,
            batch_request.blob_proof_type.clone(),
            l2_block_numbers.clone(),
            checkpoint.clone().into(),
            last_anchor_block_number.clone(),
        );

        // Create Shasta proof request entity
        let proof_request_entity = ShastaProofRequestEntity::new_with_guest_input_entity(
            input_request_entity.clone(),
            batch_request.proof_type,
            batch_request.prover_args.clone().into(),
        );

        requests.push((
            input_request_key,
            request_key,
            input_request_entity,
            proof_request_entity,
        ));
    }

    requests
}

/// Process Shasta batch requests and return the necessary data for the handler
pub fn process_shasta_batch(
    batch_request: &ShastaProofRequest,
    image_id: &ImageId,
) -> (
    Vec<RequestKey>,
    Vec<RequestKey>,
    Vec<raiko_reqpool::RequestEntity>,
    Vec<raiko_reqpool::RequestEntity>,
    Vec<u64>,
) {
    let shasta_requests = create_shasta_requests(batch_request, image_id);

    let mut sub_input_request_keys = Vec::with_capacity(shasta_requests.len());
    let mut sub_request_keys = Vec::with_capacity(shasta_requests.len());
    let mut sub_input_request_entities = Vec::with_capacity(shasta_requests.len());
    let mut sub_request_entities = Vec::with_capacity(shasta_requests.len());
    let mut sub_batch_ids = Vec::with_capacity(shasta_requests.len());

    for (input_key, request_key, input_entity, request_entity) in shasta_requests {
        sub_input_request_keys.push(input_key);
        sub_request_keys.push(request_key);
        sub_input_request_entities.push(input_entity.into());
        sub_request_entities.push(request_entity.into());
        sub_batch_ids.push(batch_request.proposals[sub_batch_ids.len()].proposal_id);
    }

    (
        sub_input_request_keys,
        sub_request_keys,
        sub_input_request_entities,
        sub_request_entities,
        sub_batch_ids,
    )
}
