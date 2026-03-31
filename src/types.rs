use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, RwLock};
use tracing::info;

// ============================================================================
// JSON-RPC Protocol Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// ============================================================================
// MCP Tool Definition
// ============================================================================

#[derive(Debug, Serialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

// ============================================================================
// MCP Server State
// ============================================================================

pub struct McpState {
    pub wallet_seeds: Mutex<HashMap<String, String>>,
    pub fullnode_url: RwLock<String>,
    pub wallet_headless_url: RwLock<String>,
    pub tx_mining_url: RwLock<String>,
    pub http_client: reqwest::Client,
    /// Orchestrator URL (if using multi-tenant mode)
    pub orchestrator_url: Option<String>,
    /// Session ID from the orchestrator (lazily provisioned on first wallet call)
    pub orchestrator_session: Mutex<Option<String>>,
}

impl McpState {
    pub fn new(
        fullnode_url: Option<String>,
        wallet_headless_url: Option<String>,
        tx_mining_url: Option<String>,
        orchestrator_url: Option<String>,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .pool_max_idle_per_host(5)
            .build()
            .expect("Failed to build HTTP client");

        Self {
            wallet_seeds: Mutex::new(HashMap::new()),
            fullnode_url: RwLock::new(
                fullnode_url.unwrap_or_else(|| "http://127.0.0.1:8080".to_string()),
            ),
            wallet_headless_url: RwLock::new(
                wallet_headless_url.unwrap_or_else(|| "http://localhost:8001".to_string()),
            ),
            tx_mining_url: RwLock::new(
                tx_mining_url.unwrap_or_else(|| "http://localhost:8002".to_string()),
            ),
            http_client,
            orchestrator_url,
            orchestrator_session: Mutex::new(None),
        }
    }

    /// Get the effective wallet-headless URL.
    /// In orchestrator mode, this provisions a session on first call and returns
    /// the orchestrator proxy URL. In direct mode, returns the configured URL.
    pub async fn get_headless_url(&self) -> Result<String, String> {
        if let Some(ref orch_url) = self.orchestrator_url {
            let mut session = self.orchestrator_session.lock().await;
            if let Some(ref sid) = *session {
                return Ok(format!("{}/sessions/{}/api", orch_url, sid));
            }

            // Provision a new session
            info!("Provisioning wallet-headless session via orchestrator");
            let resp = self
                .http_client
                .post(format!("{}/sessions", orch_url))
                .send()
                .await
                .map_err(|e| format!("Failed to create orchestrator session: {}", e))?;

            let body: Value = resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse orchestrator response: {}", e))?;

            let session_id = body
                .get("session_id")
                .and_then(|v| v.as_str())
                .ok_or("Orchestrator did not return session_id")?
                .to_string();

            info!(session_id, "Orchestrator session created");
            let url = format!("{}/sessions/{}/api", orch_url, session_id);
            *session = Some(session_id);
            Ok(url)
        } else {
            Ok(self.wallet_headless_url.read().await.clone())
        }
    }

    /// Clean up the orchestrator session (called on shutdown).
    pub async fn cleanup_session(&self) {
        if let Some(ref orch_url) = self.orchestrator_url {
            let session = self.orchestrator_session.lock().await;
            if let Some(ref sid) = *session {
                info!(session_id = sid, "Cleaning up orchestrator session");
                let _ = self
                    .http_client
                    .delete(format!("{}/sessions/{}", orch_url, sid))
                    .send()
                    .await;
            }
        }
    }
}

pub type McpSharedState = Arc<McpState>;
