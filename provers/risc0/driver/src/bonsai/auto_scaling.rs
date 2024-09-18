#![cfg(feature = "bonsai-auto-scaling")]

use anyhow::{Error, Ok, Result};
use lazy_static::lazy_static;
use log::info;
use once_cell::sync::Lazy;
use reqwest::{header::HeaderMap, header::HeaderValue, header::CONTENT_TYPE, Client};
use serde::Deserialize;
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error as trace_err};

#[derive(Debug, Deserialize, Default)]
struct ScalerResponse {
    desired: u32,
    current: u32,
    pending: u32,
}
struct BonsaiAutoScaler {
    url: String,
    headers: HeaderMap,
    client: Client,
    on_setting_status: Option<ScalerResponse>,
}

impl BonsaiAutoScaler {
    fn new(bonsai_api_url: String, api_key: String) -> Self {
        let url = bonsai_api_url + "/workers";
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("x-api-key", HeaderValue::from_str(&api_key).unwrap());

        Self {
            url,
            headers,
            client: Client::new(),
            on_setting_status: None,
        }
    }

    async fn get_bonsai_gpu_num(&self) -> Result<ScalerResponse> {
        debug!("Requesting scaler status from: {}", self.url);
        let response = self
            .client
            .get(self.url.clone())
            .headers(self.headers.clone())
            .send()
            .await?;

        // Check if the request was successful
        if response.status().is_success() {
            // Parse the JSON response
            let data: ScalerResponse = response.json().await.unwrap_or_default();
            debug!("Scaler status: {data:?}");
            Ok(data)
        } else {
            trace_err!("Request failed with status: {}", response.status());
            Err(Error::msg("Failed to get bonsai gpu num".to_string()))
        }
    }

    async fn set_bonsai_gpu_num(&mut self, gpu_num: u32) -> Result<()> {
        if self.on_setting_status.is_some() {
            // log an err if there is a race adjustment.
            trace_err!("Last bonsai setting is not active, please check.");
        }

        debug!("Requesting scaler status from: {}", self.url);
        let response = self
            .client
            .post(self.url.clone())
            .headers(self.headers.clone())
            .body(gpu_num.to_string())
            .send()
            .await?;

        // Check if the request was successful
        if response.status().is_success() {
            self.on_setting_status = Some(ScalerResponse {
                desired: gpu_num,
                current: 0,
                pending: 0,
            });
            Ok(())
        } else {
            trace_err!("Request failed with status: {}", response.status());
            Err(Error::msg("Failed to get bonsai gpu num".to_string()))
        }
    }

    async fn wait_for_bonsai_config_active(&mut self, time_out_sec: u64) -> Result<()> {
        match &self.on_setting_status {
            None => Ok(()),
            Some(setting) => {
                // loop until some timeout
                let start_time = std::time::Instant::now();
                let mut check_time = std::time::Instant::now();
                while check_time.duration_since(start_time).as_secs() < time_out_sec {
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                    check_time = std::time::Instant::now();
                    let current_bonsai_gpu_num = self.get_bonsai_gpu_num().await?;
                    if current_bonsai_gpu_num.current == setting.desired {
                        self.on_setting_status = None;
                        return Ok(());
                    }
                }
                Err(Error::msg(
                    "checking bonsai config active timeout".to_string(),
                ))
            }
        }
    }
}

lazy_static! {
    static ref BONSAI_API_URL: String =
        env::var("BONSAI_API_URL").expect("BONSAI_API_URL must be set");
    static ref BONSAI_API_KEY: String =
        env::var("BONSAI_API_KEY").expect("BONSAI_API_KEY must be set");
    static ref MAX_BONSAI_GPU_NUM: u32 = env::var("MAX_BONSAI_GPU_NUM")
        .unwrap_or_else(|_| "15".to_string())
        .parse()
        .unwrap();
}

static AUTO_SCALER: Lazy<Arc<Mutex<BonsaiAutoScaler>>> = Lazy::new(|| {
    Arc::new(Mutex::new(BonsaiAutoScaler::new(
        BONSAI_API_URL.to_string(),
        BONSAI_API_KEY.to_string(),
    )))
});

static REF_COUNT: Lazy<Arc<Mutex<u32>>> = Lazy::new(|| Arc::new(Mutex::new(0)));

pub(crate) async fn maxpower_bonsai() -> Result<()> {
    let mut ref_count = REF_COUNT.lock().await;
    *ref_count += 1;

    let mut auto_scaler = AUTO_SCALER.lock().await;
    let current_gpu_num = auto_scaler.get_bonsai_gpu_num().await?;
    // either already maxed out or pending to be maxed out
    if current_gpu_num.current == *MAX_BONSAI_GPU_NUM
        && current_gpu_num.desired == *MAX_BONSAI_GPU_NUM
        && current_gpu_num.pending == 0
    {
        Ok(())
    } else {
        info!("setting bonsai gpu num to: {:?}", *MAX_BONSAI_GPU_NUM);
        auto_scaler.set_bonsai_gpu_num(*MAX_BONSAI_GPU_NUM).await?;
        auto_scaler.wait_for_bonsai_config_active(900).await
    }
}

pub(crate) async fn shutdown_bonsai() -> Result<()> {
    let mut ref_count = REF_COUNT.lock().await;
    *ref_count = ref_count.saturating_sub(1);

    if *ref_count == 0 {
        let mut auto_scaler = AUTO_SCALER.lock().await;
        let current_gpu_num = auto_scaler.get_bonsai_gpu_num().await?;
        if current_gpu_num.current == 0
            && current_gpu_num.desired == 0
            && current_gpu_num.pending == 0
        {
            Ok(())
        } else {
            info!("setting bonsai gpu num to: 0");
            auto_scaler.set_bonsai_gpu_num(0).await?;
            auto_scaler.wait_for_bonsai_config_active(90).await
        }
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::env;
    use tokio;

    #[ignore]
    #[tokio::test]
    async fn test_bonsai_auto_scaler_get() {
        let bonsai_url = env::var("BONSAI_API_URL").expect("BONSAI_API_URL must be set");
        let bonsai_key = env::var("BONSAI_API_KEY").expect("BONSAI_API_KEY must be set");
        let max_bonsai_gpu: u32 = env::var("MAX_BONSAI_GPU_NUM")
            .unwrap_or_else(|_| "15".to_string())
            .parse()
            .unwrap();
        let auto_scaler = BonsaiAutoScaler::new(bonsai_url, bonsai_key);
        let scalar_status = auto_scaler.get_bonsai_gpu_num().await.unwrap();
        assert!(scalar_status.current <= max_bonsai_gpu);
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
        let mut auto_scaler = BonsaiAutoScaler::new(bonsai_url, bonsai_key);

        auto_scaler
            .set_bonsai_gpu_num(7)
            .await
            .expect("Failed to set bonsai gpu num");
        auto_scaler
            .wait_for_bonsai_config_active(600)
            .await
            .unwrap();
        let current_gpu_num = auto_scaler.get_bonsai_gpu_num().await.unwrap().current;
        assert_eq!(current_gpu_num, 7);

        auto_scaler
            .set_bonsai_gpu_num(0)
            .await
            .expect("Failed to set bonsai gpu num");
        auto_scaler.wait_for_bonsai_config_active(60).await.unwrap();
        let current_gpu_num = auto_scaler.get_bonsai_gpu_num().await.unwrap().current;
        assert_eq!(current_gpu_num, 0);
    }
}
