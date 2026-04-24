use clap::Parser;
use std::sync::Arc;
use tracing::info;

mod handlers;
mod routes;
mod stdio;
mod tools;
mod types;

use types::McpState;

#[derive(Parser, Debug)]
#[command(name = "hathor-mcp", about = "MCP server for Hathor Network")]
struct Args {
    /// Port to listen on (HTTP transport)
    #[arg(long, default_value = "9876")]
    port: u16,

    /// Hathor fullnode API URL
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    fullnode_url: String,

    /// Wallet-headless service URL (direct mode, mutually exclusive with --orchestrator-url)
    #[arg(long, default_value = "http://localhost:8001")]
    wallet_headless_url: String,

    /// Tx-mining service URL
    #[arg(long, default_value = "http://localhost:8002")]
    tx_mining_url: String,

    /// Headless orchestrator URL (multi-tenant mode).
    /// When set, the MCP server auto-provisions an isolated wallet-headless
    /// container per session via the orchestrator instead of using a shared instance.
    #[arg(long)]
    orchestrator_url: Option<String>,

    /// Use stdio transport instead of HTTP (for Claude Desktop)
    #[arg(long)]
    stdio: bool,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hathor_mcp=info".into()),
        )
        .init();

    let args = Args::parse();

    let state = Arc::new(McpState::new(
        Some(args.fullnode_url),
        Some(args.wallet_headless_url),
        Some(args.tx_mining_url),
        args.orchestrator_url,
    ));

    if args.stdio {
        info!("Starting MCP server in stdio mode");
        stdio::run_stdio(state.clone()).await;
    } else {
        let app = routes::create_router(state.clone());

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", args.port))
            .await
            .expect("Failed to bind");

        info!(
            port = args.port,
            "MCP server listening on http://127.0.0.1:{}",
            args.port
        );

        axum::serve(listener, app).await.expect("Server error");
    }

    // Clean up orchestrator session on shutdown
    state.cleanup_session().await;
}
