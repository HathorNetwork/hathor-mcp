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

/// Execute an MCP tool by name with the given parameters.
pub async fn execute_tool(state: &McpState, name: &str, params: &Value) -> Result<String, String> {
    let client = state.http_client.clone();
    let fullnode_url = state.fullnode_url.read().await.clone();
    // Use orchestrator-aware URL resolution for wallet-headless
    let wallet_headless_url = state.get_headless_url().await?;
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

            let wallet_seed = match seed {
                Some(s) => s.to_string(),
                None => generate_seed()?,
            };

            state
                .wallet_seeds
                .lock()
                .await
                .insert(wallet_id.to_string(), wallet_seed.clone());

            let resp = client
                .post(format!("{}/start", wallet_headless_url))
                .json(&json!({
                    "wallet-id": wallet_id,
                    "seed": wallet_seed,
                }))
                .send()
                .await
                .map_err(|e| format!("Failed to create wallet: {}", e))?;

            let result: Value = resp
                .json()
                .await
                .unwrap_or(json!({"error": "Failed to parse response"}));
            let success = result
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let message = if success {
                if seed.is_some() {
                    "Wallet created with provided seed".to_string()
                } else {
                    "Wallet created with generated seed (use get_wallet_seed to retrieve)"
                        .to_string()
                }
            } else {
                result
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Failed to create wallet in wallet-headless")
                    .to_string()
            };

            Ok(json!({
                "success": success,
                "wallet_id": wallet_id,
                "seed_stored": true,
                "message": message,
                "details": if !success { Some(&result) } else { None }
            })
            .to_string())
        }

        "get_wallet_seed" => {
            let wallet_id = require_str(params, "wallet_id")?;

            let seeds = state.wallet_seeds.lock().await;
            match seeds.get(wallet_id) {
                Some(seed) => Ok(json!({"wallet_id": wallet_id, "seed": seed}).to_string()),
                None => Ok(json!({"error": "Seed not found. Only seeds from wallets created in this session are stored."}).to_string()),
            }
        }

        "get_wallet_status" => {
            let wallet_id = require_str(params, "wallet_id")?;

            let resp = client
                .get(format!("{}/wallet/status", wallet_headless_url))
                .header("X-Wallet-Id", wallet_id)
                .send()
                .await
                .map_err(|e| format!("Failed to get wallet status: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "get_wallet_balance" => {
            let wallet_id = require_str(params, "wallet_id")?;

            let resp = client
                .get(format!("{}/wallet/balance", wallet_headless_url))
                .header("X-Wallet-Id", wallet_id)
                .send()
                .await
                .map_err(|e| format!("Failed to get wallet balance: {}", e))?;

            let text = resp.text().await.unwrap_or_default();
            Ok(text)
        }

        "get_wallet_addresses" => {
            let wallet_id = require_str(params, "wallet_id")?;

            let resp = client
                .get(format!("{}/wallet/addresses", wallet_headless_url))
                .header("X-Wallet-Id", wallet_id)
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

            let resp = client
                .post(format!("{}/wallet/simple-send-tx", wallet_headless_url))
                .header("X-Wallet-Id", wallet_id)
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

            let resp = client
                .post(format!("{}/wallet/stop", wallet_headless_url))
                .header("X-Wallet-Id", wallet_id)
                .send()
                .await
                .map_err(|e| format!("Failed to close wallet: {}", e))?;

            state.wallet_seeds.lock().await.remove(wallet_id);

            let text = resp.text().await.unwrap_or_default();
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

            let addresses_resp = client
                .get(format!("{}/wallet/addresses", wallet_headless_url))
                .header("X-Wallet-Id", wallet_id)
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

        _ => Err(format!("Unknown tool: {}", name)),
    }
}
