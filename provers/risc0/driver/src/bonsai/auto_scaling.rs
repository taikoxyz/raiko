use anyhow::{Error, Result};
use lazy_static::lazy_static;
use reqwest::{header::HeaderMap, header::HeaderValue, header::CONTENT_TYPE, Client};
use serde::Deserialize;
use std::env;
use tracing::{debug, error as trace_err};

#[derive(Debug, Deserialize, Default)]
struct ScalerResponse {
    desired: u32,
    current: u32,
    pending: u32,
}
struct BonsaiAutoScaler {
    url: String,
    api_key: String,
}

impl BonsaiAutoScaler {
    fn new(bonsai_api_url: String, api_key: String) -> Self {
        let url = bonsai_api_url + "/workers";
        Self { url, api_key }
    }

    async fn get_bonsai_gpu_num(&self) -> Result<ScalerResponse> {
        // Create a new client
        let client = Client::new();

        // Create custom headers
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("x-api-key", HeaderValue::from_str(&self.api_key).unwrap());

        debug!("Requesting scaler status from: {}", self.url);
        // Make the POST request
        let response = client.get(self.url.clone()).headers(headers).send().await?;

        // Check if the request was successful
        if response.status().is_success() {
            // Parse the JSON response
            let data: ScalerResponse = response.json().await.unwrap_or_default();
            debug!("Scaler status: {:?}", data);
            Ok(data)
        } else {
            trace_err!("Request failed with status: {}", response.status());
            Err(Error::msg("Failed to get bonsai gpu num".to_string()))
        }
    }

    async fn set_bonsai_gpu_num(&self, gpu_num: u32) -> Result<()> {
        // Create a new client
        let client = Client::new();

        // Create custom headers
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("x-api-key", HeaderValue::from_str(&self.api_key).unwrap());

        // Make the POST request
        let response = client
            .post(self.url.clone())
            .headers(headers)
            .body(gpu_num.to_string())
            .send()
            .await?;

        // Check if the request was successful
        if response.status().is_success() {
            // Parse the JSON response
            let data: ScalerResponse = response.json().await?;

            // Use the parsed data
            debug!("Scaler status: {:?}", data);
            assert_eq!(data.desired, gpu_num);
        } else {
            trace_err!("Request failed with status: {}", response.status());
        }

        Ok(())
    }
}

lazy_static! {
    static ref BONSAI_API_URL: String =
        env::var("BONSAI_API_URL").expect("BONSAI_API_URL must be set");
    static ref BONSAI_API_KEY: String =
        env::var("BONSAI_API_KEY").expect("BONSAI_API_KEY must be set");
}

const MAX_BONSAI_GPU_NUM: u32 = 15;

pub(crate) async fn maxpower_bonsai() -> Result<()> {
    let auto_scaler = BonsaiAutoScaler::new(BONSAI_API_URL.to_string(), BONSAI_API_KEY.to_string());
    let current_gpu_num = auto_scaler.get_bonsai_gpu_num().await?;
    // either already maxed out or pending to be maxed out
    if current_gpu_num.current == MAX_BONSAI_GPU_NUM
        || (current_gpu_num.current + current_gpu_num.pending == MAX_BONSAI_GPU_NUM)
    {
        Ok(())
    } else {
        auto_scaler.set_bonsai_gpu_num(MAX_BONSAI_GPU_NUM).await?;
        // wait for the bonsai to heat up
        tokio::time::sleep(tokio::time::Duration::from_secs(180)).await;
        let scaler_status = auto_scaler.get_bonsai_gpu_num().await?;
        assert!(scaler_status.current == MAX_BONSAI_GPU_NUM);
        Ok(())
    }
}

pub(crate) async fn shutdown_bonsai() -> Result<()> {
    let auto_scaler = BonsaiAutoScaler::new(BONSAI_API_URL.to_string(), BONSAI_API_KEY.to_string());
    let current_gpu_num = auto_scaler.get_bonsai_gpu_num().await?;
    if current_gpu_num.current == 0 {
        Ok(())
    } else {
        auto_scaler.set_bonsai_gpu_num(0).await?;
        // wait few minute for the bonsai to cool down
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        let scaler_status = auto_scaler.get_bonsai_gpu_num().await?;
        assert!(scaler_status.current == 0);
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::env;

    #[ignore]
    #[tokio::test]
    async fn test_bonsai_auto_scaler_get() {
        let bonsai_url = env::var("BONSAI_API_URL").expect("BONSAI_API_URL must be set");
        let bonsai_key = env::var("BONSAI_API_KEY").expect("BONSAI_API_KEY must be set");
        let auto_scaler = BonsaiAutoScaler::new(bonsai_url, bonsai_key);
        let scalar_status = auto_scaler.get_bonsai_gpu_num().await.unwrap();
        assert!(scalar_status.current <= MAX_BONSAI_GPU_NUM);
        assert_eq!(
            scalar_status.desired,
            scalar_status.current + scalar_status.pending
        );
    }

    #[ignore]
    #[tokio::test]
    async fn test_bonsai_auto_scaler_set() {
        let bonsai_url = env::var("BONSAI_API_URL").expect("BONSAI_API_URL must be set");
        let bonsai_key = env::var("BONSAI_API_KEY").expect("BONSAI_API_KEY must be set");
        let auto_scaler = BonsaiAutoScaler::new(bonsai_url, bonsai_key);

        auto_scaler
            .set_bonsai_gpu_num(7)
            .await
            .expect("Failed to set bonsai gpu num");
        // wait few minute for the bonsai to heat up
        tokio::time::sleep(tokio::time::Duration::from_secs(200)).await;
        let current_gpu_num = auto_scaler.get_bonsai_gpu_num().await.unwrap().current;
        assert_eq!(current_gpu_num, 7);

        auto_scaler
            .set_bonsai_gpu_num(0)
            .await
            .expect("Failed to set bonsai gpu num");
        // wait few minute for the bonsai to cool down
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        let current_gpu_num = auto_scaler.get_bonsai_gpu_num().await.unwrap().current;
        assert_eq!(current_gpu_num, 0);
    }
}
