use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{sse::Event, IntoResponse, Response, Sse},
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::{self, Stream};
use serde_json::json;
use std::{convert::Infallible, sync::Arc, time::Duration};

use crate::handlers::execute_tool;
use crate::tools::get_tools;
use crate::types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpSharedState};

pub fn create_router(state: McpSharedState, auth_token: Option<String>) -> Router {
    let mcp_routes = Router::new()
        .route("/mcp", post(handle_mcp_request))
        .route("/mcp/sse", get(handle_sse));

    let mcp_routes = match auth_token {
        Some(token) => mcp_routes.layer(middleware::from_fn_with_state(
            Arc::new(token),
            require_bearer_auth,
        )),
        None => mcp_routes,
    };

    Router::new()
        .merge(mcp_routes)
        .route("/health", get(handle_health))
        .with_state(state)
}

/// Bearer-token gate on /mcp and /mcp/sse. The token is compared in constant
/// time to avoid leaking its length or prefix through response-time timing.
async fn require_bearer_auth(
    State(expected): State<Arc<String>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let presented = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::trim);

    match presented {
        Some(t) if ct_eq(t.as_bytes(), expected.as_bytes()) => next.run(req).await,
        _ => (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub async fn handle_mcp_request(
    State(state): State<McpSharedState>,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    if request.method.starts_with("notifications/") {
        return (StatusCode::NO_CONTENT, "").into_response();
    }

    let response = dispatch(&state, request).await;
    Json(response).into_response()
}

pub async fn dispatch(state: &McpSharedState, request: JsonRpcRequest) -> JsonRpcResponse {
    match request.method.as_str() {
        "initialize" => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": "hathor-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "instructions": include_str!("instructions.md")
            })),
            error: None,
        },

        "tools/list" => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: Some(json!({
                "tools": get_tools(state.is_orchestrator_mode())
            })),
            error: None,
        },

        "tools/call" => {
            let tool_name = request
                .params
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("");
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or(json!({}));

            match execute_tool(state, tool_name, &arguments).await {
                Ok(result) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: Some(json!({
                        "content": [{
                            "type": "text",
                            "text": result
                        }]
                    })),
                    error: None,
                },
                Err(e) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: Some(json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Error: {}", e)
                        }],
                        "isError": true
                    })),
                    error: None,
                },
            }
        }

        "ping" => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: Some(json!({})),
            error: None,
        },

        _ => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
                data: None,
            }),
        },
    }
}

pub async fn handle_sse(
    State(_state): State<McpSharedState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream::unfold((), |_| async {
        tokio::time::sleep(Duration::from_secs(30)).await;
        Some((Ok(Event::default().comment("keepalive")), ()))
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    )
}

pub async fn handle_health() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}
