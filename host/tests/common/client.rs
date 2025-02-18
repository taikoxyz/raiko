use serde::de::DeserializeOwned;
use serde::Serialize;

/// Raiko client.
///
/// Example:
/// ```
/// let client = Client::new("http://localhost:8080");
/// let request =  raiko_host::server::api::v1::ProofRequest::default();
/// let response = client.send_request("/v1/proof", &request).await?;
/// ```
pub struct Client {
    url: String,
    pub reqwest_client: reqwest::Client,
}

impl Client {
    pub fn new(url: String) -> Self {
        Self {
            url,
            reqwest_client: reqwest::Client::new(),
        }
    }

    pub async fn post<Request: Serialize, Response: DeserializeOwned + ?Sized>(
        &self,
        path: &str,
        request: &Request,
    ) -> Result<Response, reqwest::Error> {
        let response = self
            .reqwest_client
            .post(self.build_url(path))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(response.error_for_status().unwrap_err());
        }

        response.json().await
    }

    pub async fn get(&self, path: &str) -> Result<reqwest::Response, reqwest::Error> {
        let response = self.reqwest_client.get(self.build_url(path)).send().await?;

        if !response.status().is_success() {
            return Err(response.error_for_status().unwrap_err());
        }

        Ok(response)
    }

    pub fn build_url(&self, path: &str) -> String {
        format!("{}/{}", self.url, path.trim_start_matches('/'))
    }
}
