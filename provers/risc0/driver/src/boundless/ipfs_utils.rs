/*
Uploads the guest program and creates the proof request.
Sends the proof request to Boundless.
Retrieves the proof from Boundless.
Sends the proof to the application contract.
 */

use anyhow::Error;

pub struct IpfsUtils;

impl IpfsUtils {
    pub fn new() -> Self {
        Self {}
    }

    pub fn upload_guest_program(guest_program: &[u8]) -> Result<String, Error> {
        todo!()
    }
}