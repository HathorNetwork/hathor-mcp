use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

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
    /// Active orchestrator sessions keyed by the per-session api_key the
    /// orchestrator minted. Each entry is a distinct wallet-headless container.
    /// In orchestrator mode, `create_wallet` provisions a fresh session and
    /// hands the api_key back to the caller; every subsequent wallet-scoped
    /// tool call must present that key.
    pub orchestrator_sessions: Mutex<HashMap<String, String>>,
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
            orchestrator_sessions: Mutex::new(HashMap::new()),
        }
    }

    /// True when the server is configured to route through an orchestrator.
    /// In this mode every wallet-scoped tool call must carry an `api_key`.
    pub fn is_orchestrator_mode(&self) -> bool {
        self.orchestrator_url.is_some()
    }

    /// Provision a new orchestrator session (one wallet-headless container).
    /// Returns `(api_key, proxy_url)` — the caller must retain the api_key and
    /// present it on every subsequent request to the same session.
    pub async fn provision_session(&self) -> Result<(String, String), String> {
        let orch_url = self
            .orchestrator_url
            .as_ref()
            .ok_or("Orchestrator mode is not enabled")?;

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

        let api_key = body
            .get("api_key")
            .and_then(|v| v.as_str())
            .ok_or("Orchestrator did not return api_key")?
            .to_string();

        info!(session_id, "Orchestrator session created");
        let url = format!("{}/sessions/{}/api", orch_url, session_id);
        self.orchestrator_sessions
            .lock()
            .await
            .insert(api_key.clone(), session_id);
        Ok((api_key, url))
    }

    /// Resolve the proxy URL for an existing session. In direct mode the
    /// api_key is ignored and the configured wallet-headless URL is returned.
    /// In orchestrator mode the key must match a session provisioned earlier.
    pub async fn get_url_for(&self, api_key: Option<&str>) -> Result<String, String> {
        let Some(ref orch_url) = self.orchestrator_url else {
            return Ok(self.wallet_headless_url.read().await.clone());
        };

        let key = api_key.ok_or(
            "api_key is required in orchestrator mode — get it from create_wallet",
        )?;

        let sessions = self.orchestrator_sessions.lock().await;
        let session_id = sessions
            .get(key)
            .ok_or("Unknown api_key. Create a wallet first with create_wallet.")?;
        Ok(format!("{}/sessions/{}/api", orch_url, session_id))
    }

    /// Destroy the orchestrator session associated with this api_key. No-op in
    /// direct mode. Called by `close_wallet` so containers don't leak.
    pub async fn destroy_session(&self, api_key: &str) -> Result<(), String> {
        let Some(ref orch_url) = self.orchestrator_url else {
            return Ok(());
        };

        let session_id = {
            let mut sessions = self.orchestrator_sessions.lock().await;
            sessions.remove(api_key)
        };

        let Some(sid) = session_id else {
            return Ok(());
        };

        info!(session_id = sid.as_str(), "Destroying orchestrator session");
        self.http_client
            .delete(format!("{}/sessions/{}", orch_url, sid))
            .header("x-api-key", api_key)
            .send()
            .await
            .map_err(|e| format!("Failed to destroy orchestrator session: {}", e))?;
        Ok(())
    }

    /// Tear down every orchestrator session we provisioned (called on
    /// shutdown). Failures are logged but not surfaced.
    pub async fn cleanup_session(&self) {
        let Some(ref orch_url) = self.orchestrator_url else {
            return;
        };

        let drained: Vec<(String, String)> = {
            let mut sessions = self.orchestrator_sessions.lock().await;
            sessions.drain().collect()
        };

        for (api_key, sid) in drained {
            info!(session_id = sid.as_str(), "Cleaning up orchestrator session");
            if let Err(e) = self
                .http_client
                .delete(format!("{}/sessions/{}", orch_url, sid))
                .header("x-api-key", &api_key)
                .send()
                .await
            {
                warn!(session_id = sid.as_str(), error = %e, "Session cleanup failed");
            }
        }
    }
}

pub type McpSharedState = Arc<McpState>;
