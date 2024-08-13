use anyhow::Result;
use lazy_static::lazy_static;
use reqwest::{header::HeaderMap, header::HeaderValue, header::CONTENT_TYPE, Client};
use serde::Deserialize;
use std::env;
use tracing::{debug, error};

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

    async fn get_bonsai_gpu_num(&self) -> u32 {
        // Create a new client
        let client = Client::new();
        let url = self.url.clone() + "/workers";

        // Create custom headers
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("x-api-key", HeaderValue::from_str(&self.api_key).unwrap());

        println!("Requesting scaler status from: {}", url);
        // Make the POST request
        let response = client.get(url).headers(headers).send().await.unwrap();

        // Check if the request was successful
        if response.status().is_success() {
            // Parse the JSON response
            let data: ScalerResponse = response.json().await.unwrap_or_default();

            // Use the parsed data
            println!("Scaler status: {:?}", data);
            data.current
        } else {
            error!("Request failed with status: {}", response.status());
            0
        }
    }

    async fn set_bonsai_gpu_num(&self, gpu_num: u32) -> Result<()> {
        // Create a new client
        let client = Client::new();
        let url = self.url.clone() + "/workers";

        // Create custom headers
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("x-api-key", HeaderValue::from_str(&self.api_key).unwrap());

        // Make the POST request
        let response = client
            .post(url)
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
            error!("Request failed with status: {}", response.status());
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

pub(crate) async fn maxpower_bonsai() -> Result<()> {
    let auto_scaler = BonsaiAutoScaler::new(BONSAI_API_URL.to_string(), BONSAI_API_KEY.to_string());

    let current_gpu_num = auto_scaler.get_bonsai_gpu_num().await;
    if current_gpu_num == 15 {
        Ok(())
    } else {
        auto_scaler.set_bonsai_gpu_num(15).await?;
        // wait 3 minute for the bonsai to heat up
        tokio::time::sleep(tokio::time::Duration::from_secs(180)).await;
        assert!(auto_scaler.get_bonsai_gpu_num().await == 15);
        Ok(())
    }
}

pub(crate) async fn shutdown_bonsai() -> Result<()> {
    let auto_scaler = BonsaiAutoScaler::new(BONSAI_API_URL.to_string(), BONSAI_API_KEY.to_string());
    let current_gpu_num = auto_scaler.get_bonsai_gpu_num().await;
    if current_gpu_num == 15 {
        Ok(())
    } else {
        auto_scaler.set_bonsai_gpu_num(0).await?;

        // wait 1 minute for the bonsai to cool down
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        assert!(auto_scaler.get_bonsai_gpu_num().await == 0);
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
        let gpu_num = auto_scaler.get_bonsai_gpu_num().await;
        assert!(gpu_num >= 0 && gpu_num <= 15);
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
        let current_gpu_num = auto_scaler.get_bonsai_gpu_num().await;
        assert_eq!(current_gpu_num, 7);

        auto_scaler
            .set_bonsai_gpu_num(0)
            .await
            .expect("Failed to set bonsai gpu num");
        // wait few minute for the bonsai to cool down
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        let current_gpu_num = auto_scaler.get_bonsai_gpu_num().await;
        assert_eq!(current_gpu_num, 0);
    }
}
