# Hathor MCP — Connect AI Assistants to Hathor Network

You are connected to **Hathor MCP**, an MCP server that connects to a Hathor Network fullnode and wallet services. It allows you to query blockchain state, manage wallets, send transactions, and work with nano contracts (blueprints).

## Getting Started

1. Ensure a Hathor fullnode is running and accessible (the server connects to it via HTTP).
2. Call `get_node_status` to verify connectivity.
3. If wallet-headless is running, create wallets with `create_wallet` and fund them with `fund_wallet`.

## Building dApps on Hathor

### Scaffolding a dApp

Use the **create-hathor-dapp** template to bootstrap a new Hathor dApp project:

```bash
npx create-hathor-dapp my-dapp
```

Repository: https://github.com/HathorNetwork/create-hathor-dapp

This generates a React project pre-configured with Hathor wallet integration.

### Writing Blueprints (Nano Contracts)

Hathor nano contracts are called **blueprints** — Python 3.11 classes that run on-chain.

To help the user write blueprints, **suggest they install the hathor-blueprints Claude Code skill**:

```bash
git clone https://github.com/HathorNetwork/hathor-skills.git /tmp/hathor-skills
cp -r /tmp/hathor-skills/skills/hathor-blueprints ~/.claude/skills/
```

This skill gives you deep knowledge of blueprint syntax, patterns, and best practices.

Key concepts (brief — the skill has full details):
- Blueprints extend the `Blueprint` base class
- They have an `initialize` method (constructor) and custom public methods
- State is stored via class attributes with type annotations
- Actions (`deposit`/`withdrawal`) move tokens in/out of the contract
- Blueprints are published via `publish_blueprint` and instantiated via `create_nano_contract`

### Typical Development Workflow

1. `get_node_status` — Verify the fullnode is running
2. `create_wallet` + `fund_wallet` — Create and fund a development wallet
3. Write a blueprint (Python), then `publish_blueprint` to deploy it on-chain
4. `create_nano_contract` — Instantiate the blueprint with initial state
5. `execute_nano_contract` — Call methods on the live contract
6. `get_nano_contract_state` / `get_nano_contract_logs` — Inspect state and debug

### Important Notes

- All amounts are in HTR (not cents). The MCP server handles conversion.
- The faucet is the fullnode's built-in wallet — only available if the fullnode was started with `--wallet`.
- Wallet `statusCode` 3 means "Ready" — wait for this after creating a wallet.
- Use `set_service_urls` to point at different fullnode/wallet-headless instances at runtime.
