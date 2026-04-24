use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::warn;

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
                write_line(&mut stdout, &error_response.to_string()).await;
                continue;
            }
        };

        // Notifications get no response
        if request.method.starts_with("notifications/") {
            continue;
        }

        let response = dispatch(&state, request).await;

        match serde_json::to_string(&response) {
            Ok(json) => write_line(&mut stdout, &json).await,
            Err(e) => warn!(error = %e, "Failed to serialize JSON-RPC response"),
        }
    }
}

async fn write_line(stdout: &mut io::Stdout, payload: &str) {
    if let Err(e) = stdout.write_all(format!("{}\n", payload).as_bytes()).await {
        warn!(error = %e, "Failed to write JSON-RPC response to stdout");
        return;
    }
    if let Err(e) = stdout.flush().await {
        warn!(error = %e, "Failed to flush stdout after JSON-RPC response");
    }
}
