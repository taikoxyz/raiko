use crate::ProverState;
use raiko_lib::prover::{ProverError, WorkerError};
use sp1_driver::{PartialProofRequest, WorkerProtocol, WorkerSocket};
use tokio::net::TcpListener;
use tracing::{error, info, warn};

async fn handle_worker_socket(mut socket: WorkerSocket) -> Result<(), ProverError> {
    let protocol = socket.receive().await?;

    info!("Received request: {}", protocol);

    match protocol {
        WorkerProtocol::Ping => {
            socket.send(WorkerProtocol::Ping).await?;
        }
        WorkerProtocol::PartialProofRequest(data) => {
            process_partial_proof_request(socket, data).await?;
        }
        _ => Err(WorkerError::InvalidRequest)?,
    }

    Ok(())
}

async fn process_partial_proof_request(
    mut socket: WorkerSocket,
    data: PartialProofRequest,
) -> Result<(), ProverError> {
    let result = sp1_driver::Sp1DistributedProver::run_as_worker(data).await;

    match result {
        Ok(data) => Ok(socket
            .send(WorkerProtocol::PartialProofResponse(data))
            .await?),
        Err(e) => {
            error!("Error while processing worker request: {:?}", e);

            Err(e)
        }
    }
}

async fn listen_worker(state: ProverState) {
    info!(
        "Listening as a SP1 worker on: {}",
        state.opts.worker_address
    );

    let listener = TcpListener::bind(state.opts.worker_address).await.unwrap();

    loop {
        let Ok((socket, addr)) = listener.accept().await else {
            error!("Error while accepting connection from orchestrator: Closing socket");

            return;
        };

        if let Some(orchestrator_address) = &state.opts.orchestrator_address {
            if addr.ip().to_string() != *orchestrator_address {
                warn!("Unauthorized orchestrator connection from: {}", addr);

                continue;
            }
        }

        info!("Receiving connection from orchestrator: {}", addr);

        // We purposely don't spawn the task here, as we want to block to limit the number
        // of concurrent connections to one.

        if let Err(e) = handle_worker_socket(WorkerSocket::new(socket)).await {
            error!("Error while handling worker socket: {:?}", e);
        }
    }
}

pub async fn serve(state: ProverState) {
    if state.opts.orchestrator_address.is_some() {
        tokio::spawn(listen_worker(state));
    }
}
