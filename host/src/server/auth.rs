use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub key: String,
    pub name: String,
    pub permissions: Vec<String>,
    pub rate_limit: Option<u32>, // requests per minute
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
    pub is_active: bool,
}

impl ApiKey {
    // TODO: load from DB, currently we just use a simple map
    pub fn new(key: String, name: String) -> Self {
        let env_rate_limit = std::env::var("RAIKO_RATE_LIMIT").unwrap_or("600".to_string());
        let rate_limit = env_rate_limit.parse::<u32>().unwrap_or(600);
        Self {
            key,
            name,
            permissions: vec!["read".to_string(), "write".to_string()],
            rate_limit: Some(rate_limit),
            created_at: chrono::Utc::now(),
            last_used: None,
            is_active: true,
        }
    }

    pub fn with_permissions(mut self, permissions: Vec<String>) -> Self {
        self.permissions = permissions;
        self
    }

    pub fn with_rate_limit(mut self, rate_limit: u32) -> Self {
        self.rate_limit = Some(rate_limit);
        self
    }
}

#[derive(Debug, Clone)]
pub struct ApiKeyStore {
    enabled: bool,
    keys: Arc<RwLock<DashMap<String, ApiKey>>>,
    rate_limits: Arc<RwLock<DashMap<String, Vec<chrono::DateTime<chrono::Utc>>>>>,
}

impl ApiKeyStore {
    pub fn new(api_keys: String) -> Self {
        if api_keys.is_empty() {
            return Self {
                enabled: false,
                keys: Arc::new(RwLock::new(DashMap::new())),
                rate_limits: Arc::new(RwLock::new(DashMap::new())),
            };
        }

        let parsed: Result<std::collections::HashMap<String, String>, _> =
            serde_json::from_str(&api_keys);
        let dashmap = DashMap::new();
        let rate_limits = DashMap::new();
        match parsed {
            Ok(map) => {
                for (name, apikey) in map {
                    dashmap.insert(apikey.clone(), ApiKey::new(apikey.clone(), name));
                    rate_limits.insert(apikey, Vec::new());
                }
                Self {
                    enabled: true,
                    keys: Arc::new(RwLock::new(dashmap)),
                    rate_limits: Arc::new(RwLock::new(rate_limits)),
                }
            }
            Err(_) => Self {
                enabled: false,
                keys: Arc::new(RwLock::new(DashMap::new())),
                rate_limits: Arc::new(RwLock::new(DashMap::new())),
            },
        }
    }

    pub async fn add_key(&self, api_key: ApiKey) -> Result<(), String> {
        let keys = self.keys.write().await;
        keys.insert(api_key.key.clone(), api_key.clone());
        info!("Added API key: {}", api_key.name);
        Ok(())
    }

    pub async fn remove_key(&self, key: &str) -> Result<(), String> {
        let keys = self.keys.write().await;
        if keys.remove(key).is_some() {
            info!("Removed API key: {}", key);
            Ok(())
        } else {
            Err("API key not found".to_string())
        }
    }

    pub async fn get_key(&self, key: &str) -> Option<ApiKey> {
        let keys = self.keys.read().await;
        keys.get(key).map(|k| k.clone())
    }

    pub async fn list_keys(&self) -> Vec<ApiKey> {
        let keys = self.keys.read().await;
        keys.iter().map(|entry| entry.value().clone()).collect()
    }

    pub async fn update_key_usage(&self, key: &str) -> Result<(), String> {
        let keys = self.keys.write().await;
        if let Some(mut api_key) = keys.get_mut(key) {
            api_key.last_used = Some(chrono::Utc::now());
        }
        Ok(())
    }

    pub async fn check_rate_limit(&self, key: &str) -> Result<bool, String> {
        let api_key = self.get_key(key).await;
        if let Some(api_key) = api_key {
            if let Some(rate_limit) = api_key.rate_limit {
                let now = chrono::Utc::now();
                let window_start = now - chrono::Duration::minutes(1);

                let rate_limits = self.rate_limits.write().await;
                let mut requests = rate_limits.entry(key.to_string()).or_insert_with(Vec::new);

                // clear expired requests
                requests.retain(|&time| time >= window_start);

                if requests.len() >= rate_limit as usize {
                    return Ok(false); // rate limit exceeded
                }

                requests.push(now);
                Ok(true)
            } else {
                Ok(true) // no rate limit
            }
        } else {
            Err("API key not found".to_string())
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthenticatedApiKey {
    pub key: String,
    pub name: String,
}

pub async fn api_key_auth_middleware(
    State(api_key_store): State<Arc<ApiKeyStore>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let api_key = extract_api_key_from_request(&req);

    if !api_key_store.enabled {
        let mut req = req;
        req.extensions_mut().insert(AuthenticatedApiKey {
            key: "anonymous".to_string(),
            name: "anonymous".to_string(),
        });

        return Ok(next.run(req).await);
    }

    if api_key.is_empty() {
        warn!("No API key provided, from: {:?}", req);
        return Err(StatusCode::UNAUTHORIZED);
    }

    let key_info = api_key_store.get_key(&api_key).await;
    if let Some(key_info) = key_info {
        if !key_info.is_active {
            error!("API key is inactive: {}", api_key);
            return Err(StatusCode::UNAUTHORIZED);
        }

        // check rate limit
        match api_key_store.check_rate_limit(&api_key).await {
            Ok(true) => {
                if let Err(e) = api_key_store.update_key_usage(&api_key).await {
                    error!("Failed to update API key usage: {}", e);
                }

                debug!("API key authenticated: {}", key_info.name);

                // store the authenticated key in the request extension, for later use
                let authenticated_key = AuthenticatedApiKey {
                    key: api_key.clone(),
                    name: key_info.name.clone(),
                };

                let mut req = req;
                req.extensions_mut().insert(authenticated_key);

                Ok(next.run(req).await)
            }
            Ok(false) => {
                error!("Rate limit exceeded for API key: {}", api_key);
                Err(StatusCode::TOO_MANY_REQUESTS)
            }
            Err(e) => {
                warn!("Rate limit check failed: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        error!("Invalid API key: {}", api_key);
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn extract_api_key_from_request<B>(req: &Request<B>) -> String {
    // extract from X-API-KEY header
    if let Some(api_key_header) = req.headers().get("x-api-key") {
        if let Ok(api_key) = api_key_header.to_str() {
            return api_key.to_string();
        }
    }

    String::new()
}

// helper functions for managing API keys
pub async fn create_api_key(store: &ApiKeyStore, name: &str) -> Result<String, String> {
    let key = generate_api_key();
    let api_key = ApiKey::new(key.clone(), name.to_string());

    store.add_key(api_key).await?;
    Ok(key)
}

pub async fn revoke_api_key(store: &ApiKeyStore, key: &str) -> Result<(), String> {
    store.remove_key(key).await
}

pub async fn list_api_keys(store: &ApiKeyStore) -> Vec<ApiKey> {
    store.list_keys().await
}

fn generate_api_key() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    format!("raiko_{}", hex::encode(bytes))
}
