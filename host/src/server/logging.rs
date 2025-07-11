use crate::server::auth::AuthenticatedApiKey;
use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc, sync::Mutex, time::Instant};
use tokio::{fs::OpenOptions, io::AsyncWriteExt, sync::mpsc, task::JoinHandle};
use tracing::error;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyRequestLog {
    pub api_key: String,
    pub request_id: String,
    pub method: String,
    pub path: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
    pub status_code: Option<u16>,
    pub error: Option<String>,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApiKeyStats {
    pub request_count: u64,
    pub total_duration_ms: u64,
    pub last_request_time: u64,
    pub success_count: u64,
    pub error_count: u64,
}

impl ApiKeyStats {
    pub fn new() -> Self {
        Self {
            request_count: 0,
            total_duration_ms: 0,
            last_request_time: 0,
            success_count: 0,
            error_count: 0,
        }
    }

    pub fn average_duration_ms(&self) -> f64 {
        if self.request_count > 0 {
            self.total_duration_ms as f64 / self.request_count as f64
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestStats {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub average_response_time_ms: f64,
    pub api_key_stats: Arc<DashMap<String, ApiKeyStats>>,
}

impl RequestStats {
    pub fn new() -> Self {
        Self {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            average_response_time_ms: 0.0,
            api_key_stats: Arc::new(DashMap::new()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestLoggingConfig {
    pub enabled: bool,
    pub log_file_path: Option<PathBuf>,
    pub log_format: LogFormat,
    pub retention_days: u32,
    pub include_headers: Vec<String>,
    pub exclude_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum LogFormat {
    Json,
    Csv,
}

impl Default for RequestLoggingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            log_file_path: None,
            log_format: LogFormat::Json,
            retention_days: 30,
            include_headers: vec!["user-agent".to_string()],
            exclude_paths: vec!["/health".to_string(), "/metrics".to_string()],
        }
    }
}

pub struct AsyncRequestLogger {
    sender: mpsc::UnboundedSender<ApiKeyRequestLog>,
    worker_handle: JoinHandle<()>,
    stats: Arc<Mutex<RequestStats>>,
    config: RequestLoggingConfig,
}

impl AsyncRequestLogger {
    pub fn new(config: RequestLoggingConfig) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let stats = Arc::new(Mutex::new(RequestStats::new()));
        let stats_clone = stats.clone();
        let config_clone = config.clone();

        let worker_handle = tokio::spawn(Self::log_worker(receiver, config_clone, stats_clone));

        Self {
            sender,
            worker_handle,
            stats,
            config,
        }
    }

    async fn log_worker(
        mut receiver: mpsc::UnboundedReceiver<ApiKeyRequestLog>,
        config: RequestLoggingConfig,
        stats: Arc<Mutex<RequestStats>>,
    ) {
        while let Some(log_entry) = receiver.recv().await {
            Self::update_stats(stats.clone(), &log_entry);
            if let Err(e) = Self::write_log_entry(&config, &log_entry).await {
                error!("Failed to write log entry: {}", e);
            }
        }
    }

    fn update_stats(stats: Arc<Mutex<RequestStats>>, log_entry: &ApiKeyRequestLog) {
        let mut stats = stats.lock().unwrap();
        // update overall stats
        stats.total_requests += 1;

        if let Some(status_code) = log_entry.status_code {
            if status_code < 400 {
                stats.successful_requests += 1;
            } else {
                stats.failed_requests += 1;
            }
        }

        // update api key stats
        let mut api_key_stats = stats
            .api_key_stats
            .entry(log_entry.api_key.clone())
            .or_insert_with(ApiKeyStats::new);
        api_key_stats.request_count += 1;

        if let Some(duration_ms) = log_entry.duration_ms {
            api_key_stats.total_duration_ms += duration_ms;
        }

        if let Some(end_time) = log_entry.end_time {
            api_key_stats.last_request_time = end_time.timestamp() as u64;
        }

        if let Some(status_code) = log_entry.status_code {
            if status_code < 400 {
                api_key_stats.success_count += 1;
            } else {
                api_key_stats.error_count += 1;
            }
        }
    }

    async fn write_log_entry(
        config: &RequestLoggingConfig,
        log_entry: &ApiKeyRequestLog,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(log_file_path) = &config.log_file_path {
            let log_line = match config.log_format {
                LogFormat::Json => serde_json::to_string(log_entry)?,
                LogFormat::Csv => Self::format_csv(log_entry),
            };

            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_file_path)
                .await?;

            file.write_all((log_line + "\n").as_bytes()).await?;
            file.flush().await?;
        }

        Ok(())
    }

    fn format_csv(log_entry: &ApiKeyRequestLog) -> String {
        format!(
            "{},{},{},{},{},{},{},{},{},{},{}",
            log_entry.api_key,
            log_entry.request_id,
            log_entry.method,
            log_entry.path,
            log_entry.start_time,
            log_entry
                .end_time
                .map(|t| t.to_string())
                .unwrap_or_default(),
            log_entry.duration_ms.unwrap_or(0),
            log_entry.status_code.unwrap_or(0),
            log_entry.error.as_deref().unwrap_or(""),
            log_entry.user_agent.as_deref().unwrap_or(""),
            log_entry.ip_address.as_deref().unwrap_or(""),
        )
    }

    pub async fn log_request_start(
        &self,
        request_id: &str,
        api_key: &str,
        req: &Request,
    ) -> Result<(), StatusCode> {
    // ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // let log_entry = ApiKeyRequestLog {
        //     api_key: api_key.to_string(),
        //     request_id: request_id.to_string(),
        //     method: req.method().to_string(),
        //     path: req.uri().path().to_string(),
        //     start_time: Utc::now(),
        //     end_time: None,
        //     duration_ms: None,
        //     status_code: None,
        //     error: None,
        //     user_agent: req
        //         .headers()
        //         .get("user-agent")
        //         .and_then(|h| h.to_str().ok())
        //         .map(|s| s.to_string()),
        //     ip_address: req
        //         .headers()
        //         .get("x-forwarded-for")
        //         .or_else(|| req.headers().get("x-real-ip"))
        //         .and_then(|h| h.to_str().ok())
        //         .map(|s| s.to_string()),
        // };

        // self.sender.send(log_entry)?;
        Ok(())
    }

    pub async fn log_request_end(
        &self,
        request_id: &str,
        api_key: &str,
        response: &Response,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // TODO: store request start time, currently we just use the current time
        let log_entry = ApiKeyRequestLog {
            api_key: api_key.to_string(),
            request_id: request_id.to_string(),
            method: "".to_string(),
            path: "".to_string(),
            start_time: Utc::now(),
            end_time: Some(Utc::now()),
            duration_ms: Some(0),
            status_code: Some(response.status().as_u16()),
            error: None,
            user_agent: None,
            ip_address: None,
        };

        self.sender.send(log_entry)?;
        Ok(())
    }

    pub fn get_stats(&self) -> RequestStats {
        let stats = &self.stats;
        let stats = stats.lock().unwrap();
        RequestStats {
            total_requests: stats.total_requests,
            successful_requests: stats.successful_requests,
            failed_requests: stats.failed_requests,
            average_response_time_ms: stats.average_response_time_ms,
            api_key_stats: stats.api_key_stats.clone(),
        }
    }
}

impl Drop for AsyncRequestLogger {
    fn drop(&mut self) {
        self.worker_handle.abort();
    }
}

pub async fn api_key_logging_middleware(
    State(logger): State<Arc<AsyncRequestLogger>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let start_time = Instant::now();
    let request_id = Uuid::new_v4().to_string();

    // get authenticated api key from request extension
    let api_key = if let Some(authenticated_key) = req.extensions().get::<AuthenticatedApiKey>() {
        authenticated_key.key.clone()
    } else {
        // fallback to extract from request header
        extract_api_key(&req)
    };

    // log request start
    // if let Err(e) = logger.log_request_start(&request_id, &api_key, &req).await {
    //     error!("Failed to log request start: {}", e);
    // }
    // let _ = logger.log_request_start(&request_id, &api_key, &req).await;

    // // handle request
    // let response = next.run(req).await;

    // // log request end
    // if let Err(e) = logger
    //     .log_request_end(&request_id, &api_key, &response)
    //     .await
    // {
    //     error!("Failed to log request end: {}", e);
    // }

    // Ok(response)
    Ok(next.run(req).await)
}

fn extract_api_key(req: &Request) -> String {
    if let Some(api_key_header) = req.headers().get("x-api-key") {
        if let Ok(api_key) = api_key_header.to_str() {
            return api_key.to_string();
        }
    }

    "anonymous".to_string()
}
