use clap::Parser;
use std::sync::Arc;
use tracing::{info, warn};

mod handlers;
mod routes;
mod stdio;
mod tools;
mod types;

use types::McpState;

#[derive(Parser, Debug)]
#[command(name = "hathor-mcp", about = "MCP server for Hathor Network")]
struct Args {
    /// Address to bind the HTTP listener to. Defaults to loopback so the
    /// server is not reachable from the LAN or other hosts. Use 0.0.0.0 only
    /// inside a container or when you deliberately want to expose the port.
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,

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

    /// Bearer token clients must present on every /mcp request (HTTP mode).
    /// If unset and --no-auth is not given, a random token is generated at
    /// startup and printed to stderr. May also be supplied via the
    /// HATHOR_MCP_TOKEN environment variable.
    #[arg(long, env = "HATHOR_MCP_TOKEN", hide_env_values = true)]
    auth_token: Option<String>,

    /// Disable bearer-token auth on the HTTP transport. Only safe on a
    /// loopback bind — skipping auth on any other bind lets every reachable
    /// device call every wallet tool.
    #[arg(long)]
    no_auth: bool,

    /// Use stdio transport instead of HTTP (for Claude Desktop)
    #[arg(long)]
    stdio: bool,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        // Keep logs off stdout: in --stdio mode stdout carries JSON-RPC
        // protocol traffic and any log line there would break the client.
        .with_writer(std::io::stderr)
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
        let auth_token = resolve_auth_token(args.auth_token, args.no_auth, &args.bind);
        let app = routes::create_router(state.clone(), auth_token);

        let listener = tokio::net::TcpListener::bind(format!("{}:{}", args.bind, args.port))
            .await
            .expect("Failed to bind");

        info!(
            bind = %args.bind,
            port = args.port,
            "MCP server listening on http://{}:{}",
            args.bind,
            args.port
        );

        axum::serve(listener, app).await.expect("Server error");
    }

    // Clean up orchestrator sessions on shutdown
    state.cleanup_session().await;
}

/// Decide what bearer token (if any) guards the HTTP transport. Generates a
/// random token and prints it to stderr when neither `--auth-token` /
/// `HATHOR_MCP_TOKEN` nor `--no-auth` is given.
fn resolve_auth_token(provided: Option<String>, no_auth: bool, bind: &str) -> Option<String> {
    if no_auth {
        if !is_loopback_bind(bind) {
            warn!(
                bind,
                "--no-auth on a non-loopback bind: any device able to reach this port can call every wallet tool"
            );
        }
        return None;
    }

    if let Some(token) = provided {
        return Some(token);
    }

    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("Failed to generate bearer token");
    let token: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();

    eprintln!();
    eprintln!("Hathor MCP bearer token (clients must send `Authorization: Bearer <token>`):");
    eprintln!("  {}", token);
    eprintln!("Set --auth-token or HATHOR_MCP_TOKEN to reuse a fixed value across restarts.");
    eprintln!();

    Some(token)
}

fn is_loopback_bind(bind: &str) -> bool {
    matches!(bind, "127.0.0.1" | "::1" | "localhost")
}
