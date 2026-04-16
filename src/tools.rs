use serde_json::{json, Value};

use crate::types::McpTool;

/// Description attached to the `api_key` tool parameter, explaining where it
/// comes from and why it must be retained.
const API_KEY_DESCRIPTION: &str =
    "The api_key returned by create_wallet in orchestrator mode. Required on \
every tool call that touches this wallet. Obtain it once from create_wallet \
and reuse it — it is NOT recoverable.";

/// Inject the `api_key` property into a tool schema and, in orchestrator mode,
/// mark it required. Callers passing `is_orchestrator=false` get the direct-mode
/// schema where api_key is ignored.
fn with_api_key_param(mut schema: Value, is_orchestrator: bool) -> Value {
    let obj = schema.as_object_mut().expect("tool schema must be an object");

    let props = obj
        .entry("properties")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("properties must be an object");
    props.insert(
        "api_key".to_string(),
        json!({
            "type": "string",
            "description": API_KEY_DESCRIPTION,
        }),
    );

    if is_orchestrator {
        let required = obj
            .entry("required")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("required must be an array");
        if !required.iter().any(|v| v.as_str() == Some("api_key")) {
            required.push(json!("api_key"));
        }
    }

    schema
}

/// Returns the list of all available MCP tools with their schemas.
///
/// `is_orchestrator` toggles whether wallet-scoped tools require an `api_key`
/// parameter. In orchestrator mode every wallet call must present the key
/// returned by `create_wallet`; in direct mode there is no per-session auth
/// and the parameter is omitted.
pub fn get_tools(is_orchestrator: bool) -> Vec<McpTool> {
    let create_wallet_desc = if is_orchestrator {
        "Provision a fresh isolated wallet-headless container and create a new wallet inside \
it. Returns an `api_key` — YOU MUST STORE IT and pass it back via the `api_key` parameter on \
every subsequent tool call that touches this wallet (get_wallet_balance, send_from_wallet, \
close_wallet, etc). The api_key is not recoverable; losing it means the wallet becomes \
unreachable and you'll need to create a new one. Generates a BIP39 seed if none is provided."
    } else {
        "Create a new wallet via the wallet-headless service. Generates a seed if not provided."
    };

    vec![
        // Node Status
        McpTool {
            name: "get_node_status".to_string(),
            description: "Get the current status of the Hathor fullnode including block height and network info.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        // Wallet Operations
        McpTool {
            name: "generate_seed".to_string(),
            description: "Generate a new 24-word BIP39 seed phrase for wallet creation.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        McpTool {
            name: "create_wallet".to_string(),
            description: create_wallet_desc.to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "wallet_id": {
                        "type": "string",
                        "description": "Unique identifier for the wallet"
                    },
                    "seed": {
                        "type": "string",
                        "description": "24-word BIP39 seed phrase (generated if not provided)"
                    }
                },
                "required": ["wallet_id"]
            }),
        },
        McpTool {
            name: "get_wallet_seed".to_string(),
            description: "Retrieve the seed phrase for a wallet created in this session.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "wallet_id": {
                        "type": "string",
                        "description": "The wallet ID"
                    }
                },
                "required": ["wallet_id"]
            }),
        },
        McpTool {
            name: "get_wallet_status".to_string(),
            description: "Get the sync status of a wallet (statusCode 3 = Ready).".to_string(),
            input_schema: with_api_key_param(json!({
                "type": "object",
                "properties": {
                    "wallet_id": {
                        "type": "string",
                        "description": "The wallet ID"
                    }
                },
                "required": ["wallet_id"]
            }), is_orchestrator),
        },
        McpTool {
            name: "get_wallet_balance".to_string(),
            description: "Get the balance of a wallet (available and locked HTR in cents).".to_string(),
            input_schema: with_api_key_param(json!({
                "type": "object",
                "properties": {
                    "wallet_id": {
                        "type": "string",
                        "description": "The wallet ID"
                    }
                },
                "required": ["wallet_id"]
            }), is_orchestrator),
        },
        McpTool {
            name: "get_wallet_addresses".to_string(),
            description: "Get the addresses of a wallet.".to_string(),
            input_schema: with_api_key_param(json!({
                "type": "object",
                "properties": {
                    "wallet_id": {
                        "type": "string",
                        "description": "The wallet ID"
                    }
                },
                "required": ["wallet_id"]
            }), is_orchestrator),
        },
        McpTool {
            name: "send_from_wallet".to_string(),
            description: "Send HTR from a wallet to an address.".to_string(),
            input_schema: with_api_key_param(json!({
                "type": "object",
                "properties": {
                    "wallet_id": {
                        "type": "string",
                        "description": "The wallet ID to send from"
                    },
                    "address": {
                        "type": "string",
                        "description": "Destination Hathor address"
                    },
                    "amount": {
                        "type": "number",
                        "description": "Amount of HTR to send"
                    }
                },
                "required": ["wallet_id", "address", "amount"]
            }), is_orchestrator),
        },
        McpTool {
            name: "close_wallet".to_string(),
            description: if is_orchestrator {
                "Close a wallet and destroy its orchestrator session (the wallet-headless container goes away). Call this when you're done — otherwise the container lingers until the orchestrator's idle sweeper reaps it.".to_string()
            } else {
                "Close a wallet and remove it from the wallet-headless service.".to_string()
            },
            input_schema: with_api_key_param(json!({
                "type": "object",
                "properties": {
                    "wallet_id": {
                        "type": "string",
                        "description": "The wallet ID"
                    }
                },
                "required": ["wallet_id"]
            }), is_orchestrator),
        },
        // Faucet
        McpTool {
            name: "get_faucet_balance".to_string(),
            description: "Get the balance of the fullnode's built-in wallet (faucet). Only available if the fullnode was started with --wallet.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        McpTool {
            name: "send_from_faucet".to_string(),
            description: "Send HTR from the fullnode's built-in wallet (faucet) to an address.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "address": {
                        "type": "string",
                        "description": "Destination Hathor address"
                    },
                    "amount": {
                        "type": "number",
                        "description": "Amount of HTR to send"
                    }
                },
                "required": ["address", "amount"]
            }),
        },
        McpTool {
            name: "fund_wallet".to_string(),
            description: "Send HTR from the faucet to a wallet. Auto-determines address and reasonable amount.".to_string(),
            input_schema: with_api_key_param(json!({
                "type": "object",
                "properties": {
                    "wallet_id": {
                        "type": "string",
                        "description": "The wallet ID to fund"
                    },
                    "amount": {
                        "type": "number",
                        "description": "Amount of HTR to send (auto-calculated if not provided)"
                    }
                },
                "required": ["wallet_id"]
            }), is_orchestrator),
        },
        // Blockchain
        McpTool {
            name: "get_blocks".to_string(),
            description: "Get recent blocks from the blockchain.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "count": {
                        "type": "integer",
                        "description": "Number of blocks to retrieve (default: 10, max: 100)"
                    }
                },
                "required": []
            }),
        },
        McpTool {
            name: "get_transaction".to_string(),
            description: "Get details of a specific transaction.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "tx_id": {
                        "type": "string",
                        "description": "Transaction ID (hash)"
                    }
                },
                "required": ["tx_id"]
            }),
        },
        // Nano Contracts & Blueprints
        McpTool {
            name: "list_blueprints".to_string(),
            description: "List all available blueprints on the network.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        McpTool {
            name: "get_blueprint_info".to_string(),
            description: "Get detailed information about a blueprint including its methods and arguments.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "blueprint_id": {
                        "type": "string",
                        "description": "The blueprint ID"
                    }
                },
                "required": ["blueprint_id"]
            }),
        },
        McpTool {
            name: "publish_blueprint".to_string(),
            description: "Publish an on-chain blueprint (Python source code) to the Hathor network. Requires wallet-headless running.".to_string(),
            input_schema: with_api_key_param(json!({
                "type": "object",
                "properties": {
                    "wallet_id": {
                        "type": "string",
                        "description": "The wallet ID to use for publishing"
                    },
                    "code": {
                        "type": "string",
                        "description": "The blueprint Python source code"
                    },
                    "address": {
                        "type": "string",
                        "description": "The caller address (must belong to the wallet)"
                    }
                },
                "required": ["wallet_id", "code", "address"]
            }), is_orchestrator),
        },
        McpTool {
            name: "create_nano_contract".to_string(),
            description: "Create (initialize) a new nano contract from a blueprint. Requires wallet-headless running.".to_string(),
            input_schema: with_api_key_param(json!({
                "type": "object",
                "properties": {
                    "wallet_id": {
                        "type": "string",
                        "description": "The wallet ID to use"
                    },
                    "blueprint_id": {
                        "type": "string",
                        "description": "The blueprint ID to instantiate"
                    },
                    "address": {
                        "type": "string",
                        "description": "The caller address (must belong to the wallet)"
                    },
                    "args": {
                        "type": "array",
                        "description": "Constructor arguments for the blueprint's initialize method",
                        "items": {}
                    },
                    "actions": {
                        "type": "array",
                        "description": "Actions to perform (deposit/withdrawal). Each action: {type: 'deposit'|'withdrawal', token: string, amount: number, address?: string}",
                        "items": {
                            "type": "object",
                            "properties": {
                                "type": { "type": "string", "enum": ["deposit", "withdrawal"] },
                                "token": { "type": "string" },
                                "amount": { "type": "number" },
                                "address": { "type": "string" }
                            },
                            "required": ["type", "token", "amount"]
                        }
                    }
                },
                "required": ["wallet_id", "blueprint_id", "address"]
            }), is_orchestrator),
        },
        McpTool {
            name: "execute_nano_contract".to_string(),
            description: "Execute a method on an existing nano contract. Requires wallet-headless running.".to_string(),
            input_schema: with_api_key_param(json!({
                "type": "object",
                "properties": {
                    "wallet_id": {
                        "type": "string",
                        "description": "The wallet ID to use"
                    },
                    "nc_id": {
                        "type": "string",
                        "description": "The nano contract ID"
                    },
                    "method": {
                        "type": "string",
                        "description": "The method name to call"
                    },
                    "address": {
                        "type": "string",
                        "description": "The caller address (must belong to the wallet)"
                    },
                    "args": {
                        "type": "array",
                        "description": "Arguments for the method call",
                        "items": {}
                    },
                    "actions": {
                        "type": "array",
                        "description": "Actions to perform (deposit/withdrawal). Each action: {type: 'deposit'|'withdrawal', token: string, amount: number, address?: string}",
                        "items": {
                            "type": "object",
                            "properties": {
                                "type": { "type": "string", "enum": ["deposit", "withdrawal"] },
                                "token": { "type": "string" },
                                "amount": { "type": "number" },
                                "address": { "type": "string" }
                            },
                            "required": ["type", "token", "amount"]
                        }
                    }
                },
                "required": ["wallet_id", "nc_id", "method", "address"]
            }), is_orchestrator),
        },
        McpTool {
            name: "get_nano_contract_state".to_string(),
            description: "Get the current state of a nano contract from the fullnode.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "nc_id": {
                        "type": "string",
                        "description": "The nano contract ID"
                    }
                },
                "required": ["nc_id"]
            }),
        },
        McpTool {
            name: "get_nano_contract_history".to_string(),
            description: "Get the transaction history of a nano contract.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "nc_id": {
                        "type": "string",
                        "description": "The nano contract ID"
                    }
                },
                "required": ["nc_id"]
            }),
        },
        McpTool {
            name: "get_nano_contract_logs".to_string(),
            description: "Get the execution logs for a nano contract transaction.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "tx_id": {
                        "type": "string",
                        "description": "The transaction ID (hash) of the nano contract transaction"
                    }
                },
                "required": ["tx_id"]
            }),
        },
        // Service URL Configuration
        McpTool {
            name: "get_service_urls".to_string(),
            description: "Get the current service endpoint URLs (fullnode, wallet-headless, tx-mining, orchestrator). In orchestrator mode, wallet_headless_url is null — each wallet gets its own URL from create_wallet.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        McpTool {
            name: "set_service_urls".to_string(),
            description: "Update service endpoint URLs at runtime. Only provided URLs are changed.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "fullnode_url": {
                        "type": "string",
                        "description": "Fullnode API URL (e.g. http://127.0.0.1:8080)"
                    },
                    "wallet_headless_url": {
                        "type": "string",
                        "description": "Wallet-headless service URL (e.g. http://localhost:8001)"
                    },
                    "tx_mining_url": {
                        "type": "string",
                        "description": "Tx-mining service URL (e.g. http://localhost:8002)"
                    }
                },
                "required": []
            }),
        },
    ]
}
