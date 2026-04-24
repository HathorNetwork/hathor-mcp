use axum::{
    extract::State,
    http::StatusCode,
    response::{sse::Event, IntoResponse, Sse},
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::{self, Stream};
use serde_json::json;
use std::{convert::Infallible, time::Duration};
use tower_http::cors::CorsLayer;

use crate::handlers::execute_tool;
use crate::tools::get_tools;
use crate::types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpSharedState};

pub fn create_router(state: McpSharedState) -> Router {
    Router::new()
        .route("/mcp", post(handle_mcp_request))
        .route("/mcp/sse", get(handle_sse))
        .route("/health", get(handle_health))
        .layer(CorsLayer::permissive())
        .with_state(state)
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
