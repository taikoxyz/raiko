use risc0_zkvm::{compute_image_id, sha::Digest};
use std::sync::Arc;
use tokio::sync::RwLock;
use url::Url;
use alloy_primitives::hex;
use crate::AgentError;

/// Information about an uploaded ELF image
#[derive(Debug, Clone)]
pub struct ImageInfo {
    /// RISC0 image ID computed from the ELF
    pub image_id: Digest,
    /// URL where the image is stored in Boundless Market
    pub market_url: Url,
    /// The original ELF bytes (cached for potential re-upload)
    pub elf_bytes: Vec<u8>,
}

/// Manages ELF images for both batch and aggregation proving
#[derive(Debug, Clone)]
pub struct ImageManager {
    batch_image: Arc<RwLock<Option<ImageInfo>>>,
    aggregation_image: Arc<RwLock<Option<ImageInfo>>>,
}

impl ImageManager {
    /// Create a new empty image manager
    pub fn new() -> Self {
        Self {
            batch_image: Arc::new(RwLock::new(None)),
            aggregation_image: Arc::new(RwLock::new(None)),
        }
    }

    /// Store an image and upload it to Boundless Market if not already present
    pub async fn store_and_upload_image(
        &self,
        image_type: &str,
        elf_bytes: Vec<u8>,
        client: &boundless_market::Client,
    ) -> Result<ImageInfo, AgentError> {
        // Compute image_id from the ELF
        let image_id = compute_image_id(&elf_bytes).map_err(|e| {
            AgentError::ProgramUploadError(format!("Failed to compute image_id: {e}"))
        })?;

        // Check if we already have this exact image
        let existing = match image_type {
            "batch" => self.batch_image.read().await.clone(),
            "aggregation" => self.aggregation_image.read().await.clone(),
            _ => {
                return Err(AgentError::RequestBuildError(format!(
                    "Invalid image type: {}. Must be 'batch' or 'aggregation'",
                    image_type
                )))
            }
        };

        if let Some(existing_info) = existing {
            if existing_info.image_id == image_id {
                tracing::info!(
                    "{} image already uploaded. Image ID: {:?}",
                    image_type,
                    image_id
                );
                return Ok(existing_info);
            } else {
                tracing::warn!(
                    "{} image replaced. Old ID: {:?}, New ID: {:?}",
                    image_type,
                    existing_info.image_id,
                    image_id
                );
            }
        }

        // Upload to Boundless Market
        tracing::info!(
            "Uploading {} image to market ({:.2} MB)...",
            image_type,
            elf_bytes.len() as f64 / 1_000_000.0
        );

        let market_url = client
            .upload_program(&elf_bytes)
            .await
            .map_err(|e| AgentError::ProgramUploadError(format!("{} upload failed: {e}", image_type)))?;

        tracing::info!(
            "{} image uploaded successfully. Image ID: {:?}, URL: {}",
            image_type,
            image_id,
            market_url
        );

        // Create and store the image info
        let info = ImageInfo {
            image_id,
            market_url,
            elf_bytes,
        };

        // Store in the appropriate slot
        match image_type {
            "batch" => *self.batch_image.write().await = Some(info.clone()),
            "aggregation" => *self.aggregation_image.write().await = Some(info.clone()),
            _ => unreachable!(), // Already validated above
        }

        Ok(info)
    }

    /// Get the batch image info if available
    pub async fn get_batch_image(&self) -> Option<ImageInfo> {
        self.batch_image.read().await.clone()
    }

    /// Get the aggregation image info if available
    pub async fn get_aggregation_image(&self) -> Option<ImageInfo> {
        self.aggregation_image.read().await.clone()
    }

    /// Get the market URL for batch image if available
    pub async fn get_batch_image_url(&self) -> Option<Url> {
        self.batch_image.read().await.as_ref().map(|i| i.market_url.clone())
    }

    /// Get the market URL for aggregation image if available
    pub async fn get_aggregation_image_url(&self) -> Option<Url> {
        self.aggregation_image.read().await.as_ref().map(|i| i.market_url.clone())
    }

    /// Check if a given image_id matches our stored batch image
    pub async fn is_batch_image(&self, image_id: &Digest) -> bool {
        self.batch_image
            .read()
            .await
            .as_ref()
            .map(|i| &i.image_id == image_id)
            .unwrap_or(false)
    }

    /// Check if a given image_id matches our stored aggregation image
    pub async fn is_aggregation_image(&self, image_id: &Digest) -> bool {
        self.aggregation_image
            .read()
            .await
            .as_ref()
            .map(|i| &i.image_id == image_id)
            .unwrap_or(false)
    }

    /// Get comprehensive information about both uploaded images
    pub async fn get_batch_info(&self) -> Option<ImageDetails> {
        self.batch_image.read().await.as_ref().map(|img| {
            ImageDetails {
                uploaded: true,
                image_id: Self::digest_to_vec(&img.image_id),
                image_id_hex: format!("0x{}", hex::encode(img.image_id.as_bytes())),
                market_url: img.market_url.to_string(),
                elf_size_bytes: img.elf_bytes.len(),
            }
        })
    }

    /// Get aggregation image details
    pub async fn get_aggregation_info(&self) -> Option<ImageDetails> {
        self.aggregation_image.read().await.as_ref().map(|img| {
            ImageDetails {
                uploaded: true,
                image_id: Self::digest_to_vec(&img.image_id),
                image_id_hex: format!("0x{}", hex::encode(img.image_id.as_bytes())),
                market_url: img.market_url.to_string(),
                elf_size_bytes: img.elf_bytes.len(),
            }
        })
    }

    /// Convert Digest to Vec<u32> for JSON serialization
    pub fn digest_to_vec(digest: &Digest) -> Vec<u32> {
        digest
            .as_bytes()
            .chunks(4)
            .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect()
    }

    /// Convert Vec<u32> back to Digest
    pub fn vec_to_digest(vec: &[u32]) -> Result<Digest, AgentError> {
        if vec.len() != 8 {
            return Err(AgentError::RequestBuildError(format!(
                "Invalid image_id length: expected 8 u32s, got {}",
                vec.len()
            )));
        }

        let bytes: Vec<u8> = vec
            .iter()
            .flat_map(|&n| n.to_le_bytes())
            .collect();

        Digest::try_from(bytes.as_slice()).map_err(|e| {
            AgentError::RequestBuildError(format!("Invalid image_id format: {e}"))
        })
    }
}

/// Details about an uploaded image for API responses
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct ImageDetails {
    pub uploaded: bool,
    pub image_id: Vec<u32>,
    pub image_id_hex: String,
    pub market_url: String,
    pub elf_size_bytes: usize,
}

impl Default for ImageManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_digest_conversion() {
        let original = vec![
            3537337764u32,
            1055695413,
            664197713,
            1225410428,
            3705161813,
            2151977348,
            4164639052,
            2614443474,
        ];

        let digest = ImageManager::vec_to_digest(&original).unwrap();
        let converted = ImageManager::digest_to_vec(&digest);

        assert_eq!(original, converted, "Digest conversion should be reversible");
    }
}
