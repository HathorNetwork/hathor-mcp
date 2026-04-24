use reqwest::RequestBuilder;
use serde_json::{json, Value};

use crate::types::McpState;

// ============================================================================
// Input Validation Helpers
// ============================================================================

fn require_str<'a>(params: &'a Value, field: &str) -> Result<&'a str, String> {
    let value = params
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("'{}' is required and must be a non-empty string", field))?;
    if value.trim().is_empty() {
        return Err(format!("'{}' must not be empty or whitespace-only", field));
    }
    Ok(value)
}

fn require_positive_amount(params: &Value, field: &str) -> Result<f64, String> {
    let value = params
        .get(field)
        .and_then(|v| v.as_f64())
        .ok_or_else(|| format!("'{}' is required and must be a number", field))?;
    if value <= 0.0 {
        return Err(format!("'{}' must be greater than 0, got {}", field, value));
    }
    if value > 1_000_000_000.0 {
        return Err(format!(
            "'{}' exceeds maximum allowed value (1,000,000,000), got {}",
            field, value
        ));
    }
    Ok(value)
}

fn optional_positive_amount(params: &Value, field: &str) -> Result<Option<f64>, String> {
    match params.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let value = v
                .as_f64()
                .ok_or_else(|| format!("'{}' must be a number", field))?;
            if value <= 0.0 {
                return Err(format!("'{}' must be greater than 0, got {}", field, value));
            }
            if value > 1_000_000_000.0 {
                return Err(format!(
                    "'{}' exceeds maximum allowed value (1,000,000,000), got {}",
                    field, value
                ));
            }
            Ok(Some(value))
        }
    }
}

fn optional_str<'a>(params: &'a Value, field: &str) -> Result<Option<&'a str>, String> {
    match params.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let s = v
                .as_str()
                .ok_or_else(|| format!("'{}' must be a string", field))?;
            if s.trim().is_empty() {
                return Err(format!(
                    "'{}' must not be empty or whitespace-only when provided",
                    field
                ));
            }
            Ok(Some(s))
        }
    }
}

fn optional_count(
    params: &Value,
    field: &str,
    default: usize,
    max: usize,
) -> Result<usize, String> {
    match params.get(field) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => {
            let n = v
                .as_i64()
                .ok_or_else(|| format!("'{}' must be an integer", field))?;
            if n <= 0 {
                return Err(format!("'{}' must be a positive integer, got {}", field, n));
            }
            let n = n as usize;
            if n > max {
                return Err(format!("'{}' exceeds maximum of {}, got {}", field, max, n));
            }
            Ok(n)
        }
    }
}

fn validate_url(url: &str, field: &str) -> Result<(), String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(format!(
            "'{}' must be a valid URL starting with http:// or https://, got '{}'",
            field, url
        ));
    }
    Ok(())
}

/// Generate a 24-word BIP39 seed phrase.
fn generate_seed() -> Result<String, String> {
    let mut entropy = [0u8; 32]; // 256 bits = 24 words
    getrandom::getrandom(&mut entropy).map_err(|e| format!("Failed to generate entropy: {}", e))?;
    let mnemonic = bip39::Mnemonic::from_entropy(&entropy)
        .map_err(|e| format!("Failed to generate seed: {}", e))?;
    Ok(mnemonic.to_string())
}

/// Attach `x-api-key` to a request when we're in orchestrator mode. The
/// orchestrator rejects unauthenticated calls with 401; in direct mode the
/// wallet-headless container runs without auth so the header is skipped.
fn with_api_key(req: RequestBuilder, api_key: Option<&str>) -> RequestBuilder {
    match api_key {
        Some(k) => req.header("x-api-key", k),
        None => req,
    }
}

/// Execute an MCP tool by name with the given parameters.
pub async fn execute_tool(state: &McpState, name: &str, params: &Value) -> Result<String, String> {
    let client = state.http_client.clone();
    let fullnode_url = state.fullnode_url.read().await.clone();
    let _tx_mining_url = state.tx_mining_url.read().await.clone();

    match name {
        // Node Status (read-only)
        "get_node_status" => {
            match client
                .get(format!("{}/v1a/status/", fullnode_url))
                .send()
                .await
            {
                Ok(resp) => {
                    let text = resp.text().await.unwrap_or_default();
                    Ok(format!(r#"{{"running": true, "status": {}}}"#, text))
                }
                Err(e) => Ok(json!({"running": false, "error": e.to_string()}).to_string()),
            }
        }

        // Wallet Operations
        "generate_seed" => generate_seed(),

        "create_wallet" => {
            let wallet_id = require_str(params, "wallet_id")?;
            let seed = optional_str(params, "seed")?;
            let seed_was_generated = seed.is_none();

            let wallet_seed = match seed {
                Some(s) => s.to_string(),
                None => generate_seed()?,
            };

            // In orchestrator mode, every create_wallet provisions a fresh
            // wallet-headless container with its own api_key. The api_key IS
            // the handle the caller uses to reach this wallet afterwards.
            let (api_key_opt, wallet_headless_url) = if state.is_orchestrator_mode() {
                let (key, url) = state.provision_session().await?;
                (Some(key), url)
            } else {
                (None, state.wallet_headless_url.read().await.clone())
            };

            // Seeds are only stored server-side in direct (single-tenant) mode.
            // In orchestrator mode a shared MCP deployment can't safely hold
            // seeds under a wallet_id keyspace — the caller owns their seed,
            // and we surface it once inline if we generated it (see below).
            if !state.is_orchestrator_mode() {
                state
                    .wallet_seeds
                    .lock()
                    .await
                    .insert(wallet_id.to_string(), wallet_seed.clone());
            }

            let send_result = with_api_key(
                client.post(format!("{}/start", wallet_headless_url)),
                api_key_opt.as_deref(),
            )
            .json(&json!({
                "wallet-id": wallet_id,
                "seed": wallet_seed,
            }))
            .send()
            .await;

            let resp = match send_result {
                Ok(r) => r,
                Err(e) => {
                    // Don't leak the container we just spun up.
                    if let Some(ref k) = api_key_opt {
                        let _ = state.destroy_session(k).await;
                    }
                    return Err(format!("Failed to create wallet: {}", e));
                }
            };

            let result: Value = resp
                .json()
                .await
                .unwrap_or(json!({"error": "Failed to parse response"}));
            let success = result
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let is_orchestrator = api_key_opt.is_some();
            let message = if success {
                match (seed_was_generated, is_orchestrator) {
                    (false, _) => "Wallet created with provided seed".to_string(),
                    (true, true) => {
                        "Wallet created with generated seed (returned inline — store it, it is NOT retrievable later)".to_string()
                    }
                    (true, false) => {
                        "Wallet created with generated seed (use get_wallet_seed to retrieve)".to_string()
                    }
                }
            } else {
                result
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Failed to create wallet in wallet-headless")
                    .to_string()
            };

            // If wallet-headless failed to start the wallet we have no use for
            // the session we just provisioned — tear it down so containers
            // don't leak behind a failed create.
            if !success {
                if let Some(ref k) = api_key_opt {
                    let _ = state.destroy_session(k).await;
                }
            }

            let mut response = json!({
                "success": success,
                "wallet_id": wallet_id,
                "seed_stored": !is_orchestrator,
                "message": message,
                "details": if !success { Some(&result) } else { None }
            });

            if let Some(ref key) = api_key_opt {
                response["api_key"] = json!(key);
                response["api_key_notice"] = json!(
                    "Store this api_key. You MUST include it as the `api_key` parameter on every subsequent tool call that touches this wallet. It is NOT recoverable — if you lose it, the wallet becomes unreachable and you'll need to create a new one."
                );

                // In orchestrator mode seeds are NOT persisted server-side, so if
                // we generated one we hand it back exactly once. This is the only
                // chance the caller has to retain it.
                if success && seed_was_generated {
                    response["seed"] = json!(wallet_seed);
                    response["seed_notice"] = json!(
                        "This seed was generated for you and is NOT stored server-side. Save it somewhere safe — it's the only way to restore this wallet."
                    );
                }
            }

            Ok(response.to_string())
        }

        "get_wallet_seed" => {
            let wallet_id = require_str(params, "wallet_id")?;

            // Seeds are never stored in orchestrator mode — they were handed
            // back inline by create_wallet. Refuse rather than returning an
            // empty result that invites cross-tenant probing.
            if state.is_orchestrator_mode() {
                return Err(
                    "Seeds are not stored server-side in orchestrator mode. The seed was returned once by create_wallet — if you didn't save it, create a new wallet.".to_string()
                );
            }

            let seeds = state.wallet_seeds.lock().await;
            match seeds.get(wallet_id) {
                Some(seed) => Ok(json!({"wallet_id": wallet_id, "seed": seed}).to_string()),
                None => Ok(json!({"error": "Seed not found. Only seeds from wallets created in this session are stored."}).to_string()),
            }
        }

        "get_wallet_status" => {
            let wallet_id = require_str(params, "wallet_id")?;
            let api_key = optional_str(params, "api_key")?;
            let wallet_headless_url = state.get_url_for(api_key).await?;

            let resp = with_api_key(
                client
                    .get(format!("{}/wallet/status", wallet_headless_url))
                    .header("X-Wallet-Id", wallet_id),
                api_key,
            )
            .send()
            .await
            .map_err(|e| format!("Failed to get wallet status: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "get_wallet_balance" => {
            let wallet_id = require_str(params, "wallet_id")?;
            let api_key = optional_str(params, "api_key")?;
            let wallet_headless_url = state.get_url_for(api_key).await?;

            let resp = with_api_key(
                client
                    .get(format!("{}/wallet/balance", wallet_headless_url))
                    .header("X-Wallet-Id", wallet_id),
                api_key,
            )
            .send()
            .await
            .map_err(|e| format!("Failed to get wallet balance: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "get_wallet_addresses" => {
            let wallet_id = require_str(params, "wallet_id")?;
            let api_key = optional_str(params, "api_key")?;
            let wallet_headless_url = state.get_url_for(api_key).await?;

            let resp = with_api_key(
                client
                    .get(format!("{}/wallet/addresses", wallet_headless_url))
                    .header("X-Wallet-Id", wallet_id),
                api_key,
            )
            .send()
            .await
            .map_err(|e| format!("Failed to get wallet addresses: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "send_from_wallet" => {
            let wallet_id = require_str(params, "wallet_id")?;
            let address = require_str(params, "address")?;
            let amount = require_positive_amount(params, "amount")?;
            let api_key = optional_str(params, "api_key")?;
            let wallet_headless_url = state.get_url_for(api_key).await?;

            let resp = with_api_key(
                client
                    .post(format!("{}/wallet/simple-send-tx", wallet_headless_url))
                    .header("X-Wallet-Id", wallet_id),
                api_key,
            )
            .json(&json!({
                "address": address,
                "value": (amount * 100.0).round() as i64,
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to send transaction: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "close_wallet" => {
            let wallet_id = require_str(params, "wallet_id")?;
            let api_key = optional_str(params, "api_key")?;

            // In orchestrator mode we destroy the whole session (= container),
            // which makes a per-wallet /wallet/stop redundant. Doing the stop
            // anyway would be wasteful and — worse — if it failed we'd bubble
            // out before destroy_session and leak the container.
            if state.is_orchestrator_mode() {
                let key = api_key.ok_or(
                    "api_key is required in orchestrator mode — get it from create_wallet",
                )?;
                state.destroy_session(key).await?;
                return Ok(json!({
                    "success": true,
                    "wallet_id": wallet_id,
                    "message": "Orchestrator session destroyed. The wallet-headless container has been torn down.",
                })
                .to_string());
            }

            let wallet_headless_url = state.get_url_for(api_key).await?;
            let resp = client
                .post(format!("{}/wallet/stop", wallet_headless_url))
                .header("X-Wallet-Id", wallet_id)
                .send()
                .await
                .map_err(|e| format!("Failed to close wallet: {}", e))?;

            let text = resp.text().await.unwrap_or_default();

            state.wallet_seeds.lock().await.remove(wallet_id);

            Ok(text)
        }

        // Faucet (fullnode built-in wallet)
        "get_faucet_balance" => {
            let resp = client
                .get(format!("{}/v1a/wallet/balance/", fullnode_url))
                .send()
                .await
                .map_err(|e| format!("Failed to get faucet balance: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "send_from_faucet" => {
            let address = require_str(params, "address")?;
            let amount = require_positive_amount(params, "amount")?;

            let resp = client
                .post(format!("{}/v1a/wallet/send_tokens/", fullnode_url))
                .json(&json!({
                    "data": {
                        "inputs": [],
                        "outputs": [{
                            "address": address,
                            "value": (amount * 100.0).round() as i64,
                        }]
                    }
                }))
                .send()
                .await
                .map_err(|e| format!("Failed to send from faucet: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "fund_wallet" => {
            let wallet_id = require_str(params, "wallet_id")?;
            let amount = optional_positive_amount(params, "amount")?;
            let api_key = optional_str(params, "api_key")?;
            let wallet_headless_url = state.get_url_for(api_key).await?;

            let addresses_resp = with_api_key(
                client
                    .get(format!("{}/wallet/addresses", wallet_headless_url))
                    .header("X-Wallet-Id", wallet_id),
                api_key,
            )
            .send()
            .await
            .map_err(|e| format!("Failed to get wallet addresses: {}", e))?;

            let addresses: Value = addresses_resp
                .json()
                .await
                .map_err(|_| "Failed to parse addresses")?;

            let first_address = addresses
                .get("addresses")
                .and_then(|a| a.as_array())
                .and_then(|a| a.first())
                .and_then(|a| a.as_str())
                .ok_or("Wallet has no addresses. Wait for it to sync.")?;

            let balance_resp = client
                .get(format!("{}/v1a/wallet/balance/", fullnode_url))
                .send()
                .await
                .map_err(|e| format!("Failed to get faucet balance: {}", e))?;

            let balance: Value = balance_resp
                .json()
                .await
                .map_err(|_| "Failed to parse faucet balance")?;

            let available = balance
                .get("balance")
                .and_then(|b| b.get("available"))
                .and_then(|a| a.as_i64())
                .unwrap_or(0);

            if available <= 0 {
                return Err("Faucet has no funds. Mine some blocks first.".to_string());
            }

            let fund_amount = match amount {
                Some(a) => (a * 100.0).round() as i64,
                None => {
                    let ten_percent = available / 10;
                    ten_percent.clamp(100, 10000)
                }
            };

            let send_resp = client
                .post(format!("{}/v1a/wallet/send_tokens/", fullnode_url))
                .json(&json!({
                    "data": {
                        "inputs": [],
                        "outputs": [{
                            "address": first_address,
                            "value": fund_amount,
                        }]
                    }
                }))
                .send()
                .await
                .map_err(|e| format!("Failed to send from faucet: {}", e))?;

            let text = send_resp.text().await.unwrap_or_default();

            Ok(format!(
                r#"{{"funded": true, "wallet_id": "{}", "amount": {}, "result": {}}}"#,
                wallet_id,
                fund_amount as f64 / 100.0,
                text
            ))
        }

        // Blockchain
        "get_blocks" => {
            let count = optional_count(params, "count", 10, 100)?;

            let status_resp = client
                .get(format!("{}/v1a/status/", fullnode_url))
                .send()
                .await
                .map_err(|e| format!("Failed to get status: {}", e))?;

            let status: Value = status_resp
                .json()
                .await
                .map_err(|_| "Failed to parse status")?;

            let height = status
                .get("dag")
                .and_then(|d| d.get("best_block"))
                .and_then(|b| b.get("height"))
                .and_then(|h| h.as_i64())
                .unwrap_or(0) as usize;

            let mut blocks = Vec::new();
            for i in (height.saturating_sub(count)..=height).rev() {
                if let Ok(resp) = client
                    .get(format!("{}/v1a/block_at_height?height={}", fullnode_url, i))
                    .send()
                    .await
                {
                    if let Ok(block) = resp.json::<Value>().await {
                        blocks.push(block);
                    }
                }
            }

            Ok(json!({"blocks": blocks, "currentHeight": height}).to_string())
        }

        "get_transaction" => {
            let tx_id = require_str(params, "tx_id")?;

            let resp = client
                .get(format!("{}/v1a/transaction?id={}", fullnode_url, tx_id))
                .send()
                .await
                .map_err(|e| format!("Failed to get transaction: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        // Nano Contracts & Blueprints
        "list_blueprints" => {
            let resp = client
                .get(format!("{}/v1a/nano_contract/blueprints", fullnode_url))
                .send()
                .await
                .map_err(|e| format!("Failed to list blueprints: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "get_blueprint_info" => {
            let blueprint_id = require_str(params, "blueprint_id")?;

            let resp = client
                .get(format!(
                    "{}/v1a/nano_contract/blueprint?id={}",
                    fullnode_url, blueprint_id
                ))
                .send()
                .await
                .map_err(|e| format!("Failed to get blueprint info: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "publish_blueprint" => {
            let wallet_id = require_str(params, "wallet_id")?;
            let code = require_str(params, "code")?;
            let address = require_str(params, "address")?;
            let api_key = optional_str(params, "api_key")?;
            let wallet_headless_url = state.get_url_for(api_key).await?;

            let resp = with_api_key(
                client
                    .post(format!(
                        "{}/wallet/nano-contracts/create-on-chain-blueprint",
                        wallet_headless_url
                    ))
                    .header("X-Wallet-Id", wallet_id),
                api_key,
            )
            .json(&json!({
                "code": code,
                "address": address,
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to publish blueprint: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "create_nano_contract" => {
            let wallet_id = require_str(params, "wallet_id")?;
            let blueprint_id = require_str(params, "blueprint_id")?;
            let address = require_str(params, "address")?;
            let args = params.get("args").cloned().unwrap_or(json!([]));
            let actions = params.get("actions").cloned().unwrap_or(json!([]));
            let api_key = optional_str(params, "api_key")?;
            let wallet_headless_url = state.get_url_for(api_key).await?;

            let resp = with_api_key(
                client
                    .post(format!(
                        "{}/wallet/nano-contracts/create",
                        wallet_headless_url
                    ))
                    .header("X-Wallet-Id", wallet_id),
                api_key,
            )
            .json(&json!({
                "blueprint_id": blueprint_id,
                "address": address,
                "data": {
                    "args": args,
                    "actions": actions,
                },
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to create nano contract: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "execute_nano_contract" => {
            let wallet_id = require_str(params, "wallet_id")?;
            let nc_id = require_str(params, "nc_id")?;
            let method = require_str(params, "method")?;
            let address = require_str(params, "address")?;
            let args = params.get("args").cloned().unwrap_or(json!([]));
            let actions = params.get("actions").cloned().unwrap_or(json!([]));
            let api_key = optional_str(params, "api_key")?;
            let wallet_headless_url = state.get_url_for(api_key).await?;

            let resp = with_api_key(
                client
                    .post(format!(
                        "{}/wallet/nano-contracts/execute",
                        wallet_headless_url
                    ))
                    .header("X-Wallet-Id", wallet_id),
                api_key,
            )
            .json(&json!({
                "nc_id": nc_id,
                "method": method,
                "address": address,
                "data": {
                    "args": args,
                    "actions": actions,
                },
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to execute nano contract: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "get_nano_contract_state" => {
            let nc_id = require_str(params, "nc_id")?;

            let resp = client
                .get(format!(
                    "{}/v1a/nano_contract/state?id={}",
                    fullnode_url, nc_id
                ))
                .send()
                .await
                .map_err(|e| format!("Failed to get nano contract state: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "get_nano_contract_history" => {
            let nc_id = require_str(params, "nc_id")?;

            let resp = client
                .get(format!(
                    "{}/v1a/nano_contract/history?id={}",
                    fullnode_url, nc_id
                ))
                .send()
                .await
                .map_err(|e| format!("Failed to get nano contract history: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "get_nano_contract_logs" => {
            let tx_id = require_str(params, "tx_id")?;

            let resp = client
                .get(format!(
                    "{}/v1a/nano_contract/logs?id={}",
                    fullnode_url, tx_id
                ))
                .send()
                .await
                .map_err(|e| format!("Failed to get nano contract logs: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        // Service URL Configuration
        "get_service_urls" => {
            // In orchestrator mode, wallet-headless URLs are per-session and
            // handed out by `create_wallet` — there's no single URL to report.
            let wallet_headless_url = if state.is_orchestrator_mode() {
                Value::Null
            } else {
                Value::String(state.wallet_headless_url.read().await.clone())
            };
            let orchestrator_url = state
                .orchestrator_url
                .as_ref()
                .map(|s| Value::String(s.clone()))
                .unwrap_or(Value::Null);

            Ok(json!({
                "fullnode_url": fullnode_url,
                "wallet_headless_url": wallet_headless_url,
                "orchestrator_url": orchestrator_url,
                "tx_mining_url": _tx_mining_url,
            })
            .to_string())
        }

        "set_service_urls" => {
            if let Some(url) = optional_str(params, "fullnode_url")? {
                validate_url(url, "fullnode_url")?;
                *state.fullnode_url.write().await = url.to_string();
            }
            if let Some(url) = optional_str(params, "wallet_headless_url")? {
                validate_url(url, "wallet_headless_url")?;
                *state.wallet_headless_url.write().await = url.to_string();
            }
            if let Some(url) = optional_str(params, "tx_mining_url")? {
                validate_url(url, "tx_mining_url")?;
                *state.tx_mining_url.write().await = url.to_string();
            }

            let fullnode_url = state.fullnode_url.read().await.clone();
            let wallet_headless_url = state.wallet_headless_url.read().await.clone();
            let tx_mining_url = state.tx_mining_url.read().await.clone();

            Ok(json!({
                "updated": true,
                "fullnode_url": fullnode_url,
                "wallet_headless_url": wallet_headless_url,
                "tx_mining_url": tx_mining_url,
            })
            .to_string())
        }

        _ => Err(format!("Unknown tool: {}", name)),
    }
}
