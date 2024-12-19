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
    reqwest_client: reqwest::Client,
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

        assert!(
            response.status().is_success(),
            "reqwest post error: {}",
            response.text().await?
        );

        response.json().await
    }

    pub async fn get(&self, path: &str) -> Result<reqwest::Response, reqwest::Error> {
        let response = self.reqwest_client.get(self.build_url(path)).send().await?;

        assert!(
            response.status().is_success(),
            "reqwest get error: {}",
            response.text().await?
        );

        Ok(response)
    }

    fn build_url(&self, path: &str) -> String {
        format!("{}/{}", self.url, path.trim_start_matches('/'))
    }
}
