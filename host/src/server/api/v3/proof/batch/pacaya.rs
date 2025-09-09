use raiko_core::interfaces::{BatchMetadata, BatchProofRequest};
use raiko_lib::primitives::ChainId;
use raiko_reqpool::{BatchGuestInputRequestEntity, BatchProofRequestEntity, ImageId, RequestKey};

/// Process regular (non-Shasta) batch requests and return the necessary data for the handler
pub fn process_pacaya_batch(
    batch_request: &BatchProofRequest,
    chain_id: ChainId,
    image_id: &ImageId,
) -> (
    Vec<RequestKey>,
    Vec<RequestKey>,
    Vec<raiko_reqpool::RequestEntity>,
    Vec<raiko_reqpool::RequestEntity>,
    Vec<u64>,
) {
    let mut sub_input_request_keys = Vec::with_capacity(batch_request.batches.len());
    let mut sub_input_request_entities = Vec::with_capacity(batch_request.batches.len());
    let mut sub_request_keys = Vec::with_capacity(batch_request.batches.len());
    let mut sub_request_entities = Vec::with_capacity(batch_request.batches.len());
    let mut sub_batch_ids = Vec::with_capacity(batch_request.batches.len());

    for BatchMetadata {
        batch_id,
        l1_inclusion_block_number,
    } in batch_request.batches.iter()
    {
        let input_request_key =
            RequestKey::batch_guest_input(chain_id, *batch_id, *l1_inclusion_block_number);

        let request_key = RequestKey::batch_proof_with_image_id(
            chain_id,
            *batch_id,
            *l1_inclusion_block_number,
            batch_request.proof_type,
            batch_request.prover.to_string(),
            image_id.clone(),
        );

        let input_request_entity = BatchGuestInputRequestEntity::new(
            *batch_id,
            *l1_inclusion_block_number,
            batch_request.network.clone(),
            batch_request.l1_network.clone(),
            batch_request.graffiti.clone(),
            batch_request.blob_proof_type.clone(),
        );
        let request_entity = BatchProofRequestEntity::new_with_guest_input_entity(
            input_request_entity.clone(),
            batch_request.prover.clone(),
            batch_request.proof_type,
            batch_request.prover_args.clone().into(),
        );

        sub_input_request_keys.push(input_request_key.into());
        sub_request_keys.push(request_key.into());
        sub_input_request_entities.push(input_request_entity.into());
        sub_request_entities.push(request_entity.into());
        sub_batch_ids.push(*batch_id);
    }

    (
        sub_input_request_keys,
        sub_request_keys,
        sub_input_request_entities,
        sub_request_entities,
        sub_batch_ids,
    )
}
