# Quick Start

Get the Miden Faucet running in minutes.

## Prerequisites

- Miden Faucet installed (see [Installation](./installation.md))
- Access to a Miden node (testnet, devnet, or local)

## Step 1: Initialize the Faucet

First, we need to initialize the faucet with a new account that will hold the tokens to be distributed. This command generates a new account with the specified token configuration and saves the account data to a local SQLite store. The account is not yet deployed to the network - that will happen when the faucet is running and the first transaction is sent to the node.

```bash
miden-faucet init \
  --token-symbol MIDEN \
  --decimals 6 \
  --max-supply 100000000000000000 \
  --network testnet
```

## Step 2: Start the Faucet

Next, start the faucet by specifying which network to use. This will start an frontend server to interact with the faucet with an UI and an API server that will handle incoming token requests and manage the minting process.

```bash
miden-faucet start \
  --explorer-url https://testnet.midenscan.com \
  --network testnet
```

## Step 3: Request Test Tokens

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

If you have a Miden Node running locally, you can run the faucet against that node.

```bash
miden-faucet init \
  --token-symbol MIDEN \
  --decimals 6 \
  --max-supply 100000000000000000 \
  --network localhost

miden-faucet start --network localhost
```

### Development

Connect to the node deployed in Miden Devnet. Devnet charges transaction fees
(`verification_base_fee = 10000`) in its native `MIDEN` asset, which is issued by the genesis network
faucet. On such a chain a new faucet account cannot be created (it could not pay for its own
deployment), so the faucet is initialized against the genesis faucet with the operator account file
that owns it. Its account id is the one `miden-validator genesis` prints when the network is
bootstrapped:

```bash
miden-faucet init \
  --import faucet_operator.mac \
  --faucet-account-id 0x<faucet_account_id> \
  --network devnet
```

The operator account must hold `MIDEN` to pay for its MINT transactions and to prepay the network
transactions that mint the P2ID notes: on the order of 0.5 MIDEN per request (see
[CLI configuration](./cli.md#fee-charging-chains)). It is funded at genesis, so no manual funding is
needed to get started, but it has to be topped up as its balance drains. `start` fails while the
operator holds none.

```bash
miden-faucet start --network devnet --remote-tx-prover-url https://tx-prover.devnet.miden.io
```

### Testnet

Connect to the node deployed in Miden Testnet.

```bash
miden-faucet init \
  --token-symbol MIDEN \
  --decimals 6 \
  --max-supply 100000000000000000 \
  --network testnet

miden-faucet start \
  --explorer-url https://testnet.midenscan.com \
  --network testnet
```

### Faucet API Only (No Frontend)

If you only need the API and don't want to serve the web interface:

```bash
miden-faucet start \
  --no-frontend \
  --network testnet
```
