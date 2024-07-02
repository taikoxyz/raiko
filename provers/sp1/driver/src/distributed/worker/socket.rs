use raiko_lib::prover::WorkerError;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};

use crate::{WorkerEnvelope, WorkerProtocol};

pub struct WorkerSocket {
    pub socket: tokio::net::TcpStream,
}

impl WorkerSocket {
    pub async fn connect(url: &str) -> Result<Self, WorkerError> {
        let stream = tokio::net::TcpStream::connect(url).await?;

        Ok(WorkerSocket { socket: stream })
    }

    pub fn new(socket: tokio::net::TcpStream) -> Self {
        WorkerSocket { socket }
    }

    pub async fn send(&mut self, packet: WorkerProtocol) -> Result<(), WorkerError> {
        let envelope: WorkerEnvelope = packet.into();

        let data = bincode::serialize(&envelope)?;

        self.socket.write_u64(data.len() as u64).await?;
        self.socket.write_all(&data).await?;

        Ok(())
    }

    pub async fn receive(&mut self) -> Result<WorkerProtocol, WorkerError> {
        let data = self.read_data().await?;

        let envelope: WorkerEnvelope = bincode::deserialize(&data)?;

        if envelope.magic != 0xdeadbeef {
            return Err(WorkerError::InvalidMagicNumber);
        }

        Ok(envelope.data)
    }

    // TODO: Add a timeout
    pub async fn read_data(&mut self) -> Result<Vec<u8>, std::io::Error> {
        // TODO: limit the size of the data
        let size = self.socket.read_u64().await? as usize;

        let mut data = Vec::new();

        let mut buf_data = BufWriter::new(&mut data);
        let mut buf = [0; 1024];
        let mut total_read = 0;

        loop {
            match self.socket.read(&mut buf).await {
                // socket closed
                Ok(n) if n == 0 => return Ok(data),
                Ok(n) => {
                    buf_data.write_all(&buf[..n]).await?;

                    total_read += n;

                    if total_read == size {
                        buf_data.flush().await?;

                        return Ok(data);
                    }

                    // TODO: handle the case where the data is bigger than expected
                }
                Err(e) => {
                    log::error!("failed to read from socket; err = {:?}", e);

                    return Err(e);
                }
            };
        }
    }
}
