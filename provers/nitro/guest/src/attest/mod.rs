use anyhow::Result;
use aws_nitro_enclaves_nsm_api::api::{Request, Response};
use aws_nitro_enclaves_nsm_api::driver as nsm_driver;

pub mod cmd_handler;
pub mod signature;
pub mod vsock_client;

pub fn get_pcr(pcr_index: u16) -> Result<Vec<u8>> {
    let request = Request::DescribePCR { index: pcr_index };
    let nsm_fd = nsm_driver::nsm_init();
    let response = nsm_driver::nsm_process_request(nsm_fd, request);
    nsm_driver::nsm_exit(nsm_fd);

    match response {
        Response::DescribePCR { lock, data } => {
            log::info!("Got PCR response:");
            log::info!("Lock status: {}", lock);
            log::info!("Raw data: {:02x?}", data);

            Ok(data.to_vec())
        }
        Response::Attestation { document } => {
            log::info!("Attestation document: {:?}", document);
            Ok(document)
        }
        _ => panic!("nsm driver returned invalid response: {:?}", response),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_pcr0() {
        let pcr0 = get_pcr(0);
        println!("pcr0 = {:?}", pcr0);
    }
}
