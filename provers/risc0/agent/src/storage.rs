use crate::boundless::{AgentError, AgentResult, AsyncProofRequest, ProofRequestStatus, ProofType};
use alloy_primitives_v1p2p0::U256;
use serde_json;
use alloy_primitives_v1p2p0::keccak256;
use tokio_rusqlite::params;
use tracing;

/// SQLite storage for persistent boundless request tracking
#[derive(Debug, Clone)]
pub struct BoundlessStorage {
    db_path: String,
}

impl BoundlessStorage {
    pub fn new(db_path: String) -> Self {
        Self { db_path }
    }

    /// Initialize the database and create tables if they don't exist
    pub async fn initialize(&self) -> AgentResult<()> {
        let db_path = self.db_path.clone();
        tokio_rusqlite::Connection::open(db_path)
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to open SQLite database: {}", e)))?
            .call(move |conn| {
                conn.execute(
                    r#"
                    CREATE TABLE IF NOT EXISTS boundless_requests (
                        request_id TEXT PRIMARY KEY,
                        market_request_id TEXT NOT NULL,
                        status TEXT NOT NULL,
                        proof_type TEXT NOT NULL,
                        input_data BLOB NOT NULL,
                        config_data TEXT NOT NULL,
                        updated_at INTEGER NOT NULL,
                        proof_data BLOB,
                        error_message TEXT,
                        input_hash TEXT,
                        proof_type_str TEXT
                    )
                    "#,
                    [],
                ).map_err(|e| e)?;

                // Create index for faster status queries
                conn.execute(
                    "CREATE INDEX IF NOT EXISTS idx_status ON boundless_requests(status)",
                    [],
                ).map_err(|e| e)?;

                // Migrate existing database by adding new columns if they don't exist
                let _ = conn.execute("ALTER TABLE boundless_requests ADD COLUMN input_hash TEXT", []);
                let _ = conn.execute("ALTER TABLE boundless_requests ADD COLUMN proof_type_str TEXT", []);

                // Create unique index for input deduplication
                conn.execute(
                    "CREATE UNIQUE INDEX IF NOT EXISTS idx_input_dedup ON boundless_requests(input_hash, proof_type_str) WHERE input_hash IS NOT NULL AND proof_type_str IS NOT NULL",
                    [],
                ).map_err(|e| e)?;

                Ok(())
            })
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Database initialization failed: {}", e)))?;

        tracing::info!("SQLite database initialized at: {}", self.db_path);
        Ok(())
    }

    /// Compute Keccak256 hash of input data for deduplication
    fn compute_input_hash(input: &[u8]) -> String {
        format!("{:x}", keccak256(input))
    }

    /// Convert ProofType to string for database storage
    fn proof_type_to_string(proof_type: &ProofType) -> String {
        match proof_type {
            ProofType::Batch => "batch".to_string(),
            ProofType::Aggregate => "aggregate".to_string(),
            ProofType::Update(_) => "update".to_string(),
        }
    }

    /// Store a new async request
    pub async fn store_request(&self, request: &AsyncProofRequest) -> AgentResult<()> {
        let db_path = self.db_path.clone();
        let request = request.clone();
        
        tokio_rusqlite::Connection::open(db_path)
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to open database: {}", e)))?
            .call(move |conn| {
                let status_json = serde_json::to_string(&request.status)
                    .map_err(|e| tokio_rusqlite::Error::Other(Box::new(e)))?;
                let proof_type_json = serde_json::to_string(&request.proof_type)
                    .map_err(|e| tokio_rusqlite::Error::Other(Box::new(e)))?;
                let config_json = serde_json::to_string(&request.config)
                    .map_err(|e| tokio_rusqlite::Error::Other(Box::new(e)))?;
                let market_request_id_str = format!("0x{:x}", request.market_request_id);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;

                // Compute input hash and proof type string for deduplication
                let input_hash = Self::compute_input_hash(&request.input);
                let proof_type_str = Self::proof_type_to_string(&request.proof_type);

                conn.execute(
                    r#"
                    INSERT OR REPLACE INTO boundless_requests 
                    (request_id, market_request_id, status, proof_type, input_data, config_data, 
                     updated_at, proof_data, error_message, input_hash, proof_type_str)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                    "#,
                    params![
                        request.request_id,
                        market_request_id_str,
                        status_json,
                        proof_type_json,
                        request.input,
                        config_json,
                        now,
                        Option::<Vec<u8>>::None, // proof_data initially None
                        Option::<String>::None,   // error_message initially None
                        input_hash,
                        proof_type_str
                    ],
                ).map_err(|e| e)?;

                Ok(())
            })
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to store request: {}", e)))?;

            Ok(())
    }

    /// Update request status
    pub async fn update_status(&self, request_id: &str, status: &ProofRequestStatus) -> AgentResult<()> {
        let db_path = self.db_path.clone();
        let request_id = request_id.to_string();
        let status = status.clone();
        
        tokio_rusqlite::Connection::open(db_path)
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to open database: {}", e)))?
            .call(move |conn| {
                let status_json = serde_json::to_string(&status)
                    .map_err(|e| tokio_rusqlite::Error::Other(Box::new(e)))?;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;

                // Extract proof data and error message from status
                let (proof_data, error_message) = match &status {
                    ProofRequestStatus::Fulfilled { proof, .. } => (Some(proof.clone()), None),
                    ProofRequestStatus::Failed { error } => (None, Some(error.clone())),
                    _ => (None, None),
                };

                conn.execute(
                    r#"
                    UPDATE boundless_requests 
                    SET status = ?1, updated_at = ?2, proof_data = ?3, error_message = ?4
                    WHERE request_id = ?5
                    "#,
                    params![status_json, now, proof_data, error_message, request_id],
                ).map_err(|e| e)?;

                Ok(())
            })
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to update status: {}", e)))?;

        Ok(())
    }

    /// Get a request by request ID
    pub async fn get_request(&self, request_id: &str) -> AgentResult<Option<AsyncProofRequest>> {
        let db_path = self.db_path.clone();
        let request_id = request_id.to_string();
        
        tokio_rusqlite::Connection::open(db_path)
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to open database: {}", e)))?
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT request_id, market_request_id, status, proof_type, input_data, config_data
                    FROM boundless_requests 
                    WHERE request_id = ?1
                    "#
                ).map_err(|e| e)?;

                let mut rows = stmt.query_map([request_id], |row| {
                    Self::parse_request_row(row)
                }).map_err(|e| e)?;

                match rows.next() {
                    Some(Ok(request)) => Ok(Some(request)),
                    Some(Err(e)) => Err(tokio_rusqlite::Error::Rusqlite(e)),
                    None => Ok(None),
                }
            })
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to get request: {}", e)))
    }

    /// List all active (non-completed) requests
    pub async fn list_active_requests(&self) -> AgentResult<Vec<AsyncProofRequest>> {
        let db_path = self.db_path.clone();
        
        tokio_rusqlite::Connection::open(db_path)
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to open database: {}", e)))?
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT request_id, market_request_id, status, proof_type, input_data, config_data
                    FROM boundless_requests 
                    WHERE status NOT LIKE '%Fulfilled%' AND status NOT LIKE '%Failed%'
                    ORDER BY updated_at DESC
                    "#
                ).map_err(|e| e)?;

                let rows = stmt.query_map([], |row| {
                    Self::parse_request_row(row)
                }).map_err(|e| e)?;

                let mut requests = Vec::new();
                for row in rows {
                    match row {
                        Ok(request) => requests.push(request),
                        Err(e) => tracing::warn!("Failed to parse request from database: {}", e),
                    }
                }

                Ok(requests)
            })
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to list requests: {}", e)))
    }

    /// Get all requests that need status polling (submitted or locked)
    pub async fn get_pending_requests(&self) -> AgentResult<Vec<AsyncProofRequest>> {
        let db_path = self.db_path.clone();
        
        tokio_rusqlite::Connection::open(db_path)
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to open database: {}", e)))?
            .call(move |conn| {

                let mut stmt = conn.prepare(
                    r#"
                    SELECT request_id, market_request_id, status, proof_type, input_data, config_data
                    FROM boundless_requests 
                    WHERE (status LIKE '%Submitted%' OR status LIKE '%Locked%')
                    ORDER BY updated_at ASC
                    "#
                ).map_err(|e| e)?;

                let rows = stmt.query_map([], |row| {
                    Self::parse_request_row(row)
                }).map_err(|e| e)?;

                let mut requests = Vec::new();
                for row in rows {
                    match row {
                        Ok(request) => requests.push(request),
                        Err(e) => tracing::warn!("Failed to parse pending request: {}", e),
                    }
                }

                Ok(requests)
            })
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to get pending requests: {}", e)))
    }

    /// Helper function to parse a database row into AsyncProofRequest
    fn parse_request_row(row: &rusqlite::Row) -> Result<AsyncProofRequest, rusqlite::Error> {
        let request_id: String = row.get(0)?;
        let market_request_id_str: String = row.get(1)?;
        let status_json: String = row.get(2)?;
        let proof_type_json: String = row.get(3)?;
        let input_data: Vec<u8> = row.get(4)?;
        let config_json: String = row.get(5)?;

        // Parse market_request_id from hex string
        let market_request_id = if market_request_id_str.starts_with("0x") {
            U256::from_str_radix(&market_request_id_str[2..], 16)
                .map_err(|_| rusqlite::Error::InvalidColumnType(1, "market_request_id".to_string(), rusqlite::types::Type::Text))?
        } else {
            U256::ZERO
        };

        // Deserialize JSON fields
        let status: ProofRequestStatus = serde_json::from_str(&status_json)
            .map_err(|_| rusqlite::Error::InvalidColumnType(2, "status".to_string(), rusqlite::types::Type::Text))?;
        let proof_type: ProofType = serde_json::from_str(&proof_type_json)
            .map_err(|_| rusqlite::Error::InvalidColumnType(3, "proof_type".to_string(), rusqlite::types::Type::Text))?;
        let config: serde_json::Value = serde_json::from_str(&config_json)
            .map_err(|_| rusqlite::Error::InvalidColumnType(5, "config".to_string(), rusqlite::types::Type::Text))?;


        Ok(AsyncProofRequest {
            request_id,
            market_request_id,
            status,
            proof_type,
            input: input_data,
            config,
        })
    }

    /// Get a request by input hash and proof type for deduplication
    pub async fn get_request_by_input_hash(&self, input: &[u8], proof_type: &ProofType) -> AgentResult<Option<AsyncProofRequest>> {
        let db_path = self.db_path.clone();
        let input_hash = Self::compute_input_hash(input);
        let proof_type_str = Self::proof_type_to_string(proof_type);
        
        tokio_rusqlite::Connection::open(db_path)
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to open database: {}", e)))?
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT request_id, market_request_id, status, proof_type, input_data, config_data
                    FROM boundless_requests 
                    WHERE input_hash = ?1 AND proof_type_str = ?2
                    ORDER BY updated_at DESC
                    LIMIT 1
                    "#
                ).map_err(|e| e)?;

                let mut rows = stmt.query_map([input_hash, proof_type_str], |row| {
                    Self::parse_request_row(row)
                }).map_err(|e| e)?;

                match rows.next() {
                    Some(Ok(request)) => Ok(Some(request)),
                    Some(Err(e)) => Err(tokio_rusqlite::Error::Rusqlite(e)),
                    None => Ok(None),
                }
            })
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to get request by input hash: {}", e)))
    }

    /// Delete expired non-successful requests (older than 1 hour)
    /// Returns list of deleted request IDs for memory cleanup
    pub async fn delete_expired_requests(&self) -> AgentResult<Vec<String>> {
        let db_path = self.db_path.clone();
        
        tokio_rusqlite::Connection::open(db_path)
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to open database: {}", e)))?
            .call(move |conn| {
                // First, get the request IDs that will be deleted
                let mut stmt = conn.prepare(
                    r#"
                    SELECT request_id FROM boundless_requests 
                    WHERE updated_at < (strftime('%s', 'now') - 3600)
                    AND status NOT LIKE '%Fulfilled%'
                    "#
                ).map_err(|e| e)?;

                let rows = stmt.query_map([], |row| {
                    let request_id: String = row.get(0)?;
                    Ok(request_id)
                }).map_err(|e| e)?;

                let mut deleted_ids = Vec::new();
                for row in rows {
                    match row {
                        Ok(request_id) => deleted_ids.push(request_id),
                        Err(e) => tracing::warn!("Failed to parse request_id during cleanup: {}", e),
                    }
                }

                // Now delete the expired requests
                let deleted_count = conn.execute(
                    r#"
                    DELETE FROM boundless_requests 
                    WHERE updated_at < (strftime('%s', 'now') - 3600)
                    AND status NOT LIKE '%Fulfilled%'
                    "#,
                    [],
                ).map_err(|e| e)?;

                if deleted_count > 0 {
                    tracing::info!("Deleted {} expired non-successful requests from database", deleted_count);
                }

                Ok(deleted_ids)
            })
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to delete expired requests: {}", e)))
    }

    /// Delete all requests from the database
    /// Returns the number of deleted requests
    pub async fn delete_all_requests(&self) -> AgentResult<usize> {
        let db_path = self.db_path.clone();
        
        tokio_rusqlite::Connection::open(db_path)
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to open database: {}", e)))?
            .call(move |conn| {
                let deleted_count = conn.execute(
                    "DELETE FROM boundless_requests",
                    [],
                ).map_err(|e| e)?;
                
                tracing::info!("Deleted {} requests from database", deleted_count);
                Ok(deleted_count)
            })
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to delete all requests: {}", e)))
    }

    /// Get database file path (useful for backups)
    pub fn db_path(&self) -> &str {
        &self.db_path
    }

    /// Get database stats
    pub async fn get_stats(&self) -> AgentResult<DatabaseStats> {
        let db_path = self.db_path.clone();
        
        tokio_rusqlite::Connection::open(db_path)
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to open database: {}", e)))?
            .call(move |conn| {
                // Get total count
                let total: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM boundless_requests",
                    [],
                    |row| row.get(0)
                )?;

                // Get active count
                let active: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM boundless_requests WHERE status NOT LIKE '%Fulfilled%' AND status NOT LIKE '%Failed%'",
                    [],
                    |row| row.get(0)
                )?;

                // Get completed count
                let completed: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM boundless_requests WHERE status LIKE '%Fulfilled%'",
                    [],
                    |row| row.get(0)
                )?;

                // Get failed count
                let failed: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM boundless_requests WHERE status LIKE '%Failed%'",
                    [],
                    |row| row.get(0)
                )?;

                Ok(DatabaseStats {
                    total_requests: total as u64,
                    active_requests: active as u64,
                    completed_requests: completed as u64,
                    failed_requests: failed as u64,
                })
            })
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to get database stats: {}", e)))
    }

    /// Store ELF URL for a given ELF type
    pub async fn store_elf_url(&self, _elf_type: &str, _url: &str) -> AgentResult<()> {
        todo!()
    }

    /// Retrieve ELF URL for a given ELF type
    pub async fn get_elf_url(&self, _elf_type: &str) -> AgentResult<Option<String>> {
        todo!()
    }

    /// Get all stored ELF URLs
    pub async fn get_all_elf_urls(&self) -> AgentResult<Vec<(String, String)>> {
        todo!()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DatabaseStats {
    pub total_requests: u64,
    pub active_requests: u64,
    pub completed_requests: u64,
    pub failed_requests: u64,
}