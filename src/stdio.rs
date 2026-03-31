use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::routes::dispatch;
use crate::types::{JsonRpcRequest, McpSharedState};

/// Run the MCP server over stdio (for Claude Desktop and similar clients).
/// Reads newline-delimited JSON-RPC from stdin, writes responses to stdout.
pub async fn run_stdio(state: McpSharedState) {
    let stdin = BufReader::new(io::stdin());
    let mut stdout = io::stdout();
    let mut lines = stdin.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                let error_response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    }
                });
                let _ = stdout
                    .write_all(format!("{}\n", error_response).as_bytes())
                    .await;
                let _ = stdout.flush().await;
                continue;
            }
        };

        // Notifications get no response
        if request.method.starts_with("notifications/") {
            continue;
        }

        let response = dispatch(&state, request).await;

        if let Ok(json) = serde_json::to_string(&response) {
            let _ = stdout.write_all(format!("{}\n", json).as_bytes()).await;
            let _ = stdout.flush().await;
        }
    }
}
