# Hathor MCP

MCP (Model Context Protocol) server for Hathor Network. Connects AI assistants like Claude to any Hathor fullnode.

## Features

- Query blockchain state (blocks, transactions, node status)
- Manage wallets via wallet-headless (create, fund, send, check balance)
- Work with nano contracts/blueprints (publish, create, execute, query state)
- Faucet operations (send from fullnode's built-in wallet)
- Two transport modes: HTTP and stdio (for Claude Desktop)

## Quick Start

```bash
# Build
cargo build --release

# Run with default settings (fullnode at localhost:8080)
./target/release/hathor-mcp

# Connect to a specific fullnode
./target/release/hathor-mcp \
  --fullnode-url http://my-node:8080 \
  --wallet-headless-url http://my-node:8001

# Use stdio transport for Claude Desktop
./target/release/hathor-mcp --stdio
```

## CLI Options

| Flag | Default | Description |
|------|---------|-------------|
| `--bind` | 127.0.0.1 | HTTP listen address. Use `0.0.0.0` only when you mean to expose the port |
| `--port` | 9876 | HTTP server port |
| `--fullnode-url` | http://127.0.0.1:8080 | Hathor fullnode API URL |
| `--wallet-headless-url` | http://localhost:8001 | Wallet-headless service URL |
| `--tx-mining-url` | http://localhost:8002 | Tx-mining service URL |
| `--orchestrator-url` | (unset) | Headless orchestrator URL for multi-tenant mode |
| `--auth-token` | (generated) | Bearer token clients must present; also reads `HATHOR_MCP_TOKEN`. Auto-generated to stderr if unset |
| `--no-auth` | false | Disable bearer auth on /mcp (loopback only) |
| `--stdio` | false | Use stdio transport instead of HTTP |

## Authentication

HTTP mode requires a bearer token on every `/mcp` and `/mcp/sse` request:

```
Authorization: Bearer <token>
```

If `--auth-token` and `HATHOR_MCP_TOKEN` are both unset, a random 32-byte token is generated at startup and printed to stderr. Set `HATHOR_MCP_TOKEN` (or `--auth-token`) to reuse a fixed value across restarts. Use `--no-auth` only on a loopback bind when you're sure no other process can reach the port.

The `/health` endpoint is unauthenticated so container orchestrators can health-check it.

## MCP Configuration

### Claude Code (.mcp.json)

```json
{
  "mcpServers": {
    "hathor": {
      "type": "http",
      "url": "http://127.0.0.1:9876/mcp"
    }
  }
}
```

### Claude Desktop

```json
{
  "mcpServers": {
    "hathor": {
      "command": "/path/to/hathor-mcp",
      "args": ["--stdio", "--fullnode-url", "http://127.0.0.1:8080"]
    }
  }
}
```

## Available Tools

### Blockchain
- `get_node_status` — Node status, block height, network info
- `get_blocks` — Recent blocks (configurable count)
- `get_transaction` — Transaction details by hash

### Wallet Operations
- `generate_seed` — Generate BIP39 seed phrase
- `create_wallet` / `close_wallet` — Manage wallets. Generated seeds are returned inline in `create_wallet`'s response and are never stored server-side
- `get_wallet_status` / `get_wallet_balance` / `get_wallet_addresses`
- `send_from_wallet` — Send HTR

### Faucet
- `get_faucet_balance` — Fullnode wallet balance
- `send_from_faucet` — Send from fullnode wallet
- `fund_wallet` — Auto-fund a wallet from faucet

### Nano Contracts
- `list_blueprints` / `get_blueprint_info`
- `publish_blueprint` — Deploy Python blueprint on-chain
- `create_nano_contract` — Instantiate a blueprint
- `execute_nano_contract` — Call contract methods
- `get_nano_contract_state` / `get_nano_contract_history` / `get_nano_contract_logs`

### Configuration
- `get_service_urls` — Inspect configured endpoint URLs (URLs are fixed at startup)

## License

MIT
