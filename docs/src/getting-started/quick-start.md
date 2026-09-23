# Quick Start

Get the Miden Faucet running in minutes.

## Prerequisites

- Miden Faucet installed (see [Installation](./installation.md))
- Access to a Miden node (testnet, devnet, or local)

## Step 1: Start the Faucet

Start the faucet by specifying the URL of the funding service, the network, the token decimals, and optionally the explorer URL. This will start a frontend server to interact with the faucet with an UI and an API server that will handle incoming token requests and forward the requests to the funding service.

```bash
miden-faucet start \
  --funding-service-url http://localhost:50401 \
  --decimals 6 \
  --explorer-url https://testnet.midenscan.com \
  --network testnet
```

## Step 2: Request Test Tokens

Once the faucet is running, you can request test tokens through either the web interface, the client CLI, or the REST API.

### Via Client CLI

Use the dedicated mint command:

```bash
miden-faucet-client mint \
  --url http://localhost:8000 \
  --target-account <ACCOUNT_ID_OR_ADDRESS> \
  --amount 1000
```

Although the command is named `mint`, in technical terms it makes a request to the faucet, solves the PoW challenge and creates a public P2ID note.

### Via Web Interface (if frontend is enabled)

Open `http://localhost:8080` in your browser to access the web interface for generating token requests. Then:

1. Enter your Miden account ID or account bech32 address.
2. Select token amount
3. Submit request

### Via API

You can also programmatically interact with the REST API to mint tokens. Check out the complete working examples below. Make sure the faucet REST API is running at `http://localhost:8000` before using them.

- [Rust](../examples/rust/request_tokens.rs)
- [TypeScript](../examples/typescript/request_tokens.ts)

## Common Configurations

### Localhost

If you have a Miden Node and a funding service instance running locally, you can run the faucet against them.

```bash
miden-faucet start \
  --funding-service-url http://localhost:50401 \
  --decimals 6 \
  --network localhost
```

### Faucet API Only (No Frontend)

If you only need the API and don't want to serve the web interface:

```bash
miden-faucet start \
  --funding-service-url http://localhost:50401 \
  --decimals 6 \
  --no-frontend \
  --network testnet
```
