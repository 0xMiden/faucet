# CLI configuration and usage

This guide shows the available commands and their configuration options to run with the Miden Faucet CLI.

The faucet comes with two CLI tools:

- **miden-faucet**: Runs the faucet, used for initializing and starting the faucet.
- **miden-faucet-client**: Used for interacting with a live faucet, i.e. for requesting tokens from a running faucet.

## Available Commands

| Command | Description |
|---------|-------------|
| `start` | Start the faucet server |
| `api-key create` | Generate an API key and append it to the API keys file |
| `api-key remove` | Remove an API key from the API keys file |
| `api-key list` | List all API keys in the API keys file |
| `help` | Show help information |

## Configuration Methods

The Miden Faucet can be configured using:

1. **Command-line arguments**
2. **Environment variables**

## `start` Configuration

### Basic Configuration

| Option | Description | Default | Required |
|--------|-------------|---------|----------|
| `--funding-service-url` | Base URL of the funding service that emits the notes | - | Yes |
| `--decimals` | Decimals of the token, used by the frontend to convert base units into token amounts | - | Yes |
| `--api-bind-port` | Port to bind the API server | 8000 | No |
| `--api-public-url` | Public URL to access the faucet API | http://localhost:8000 | No |
| `--frontend-bind-port` | Port to bind the frontend server | 8080 | No |
| `--no-frontend` | Optionally disable the frontend server | false | No |
| `--node-url` | Miden node RPC endpoint. If not set, it will be derived from the network | - | No |
| `--network` | Network configuration | `localhost` | No |
| `--timeout` | Funding service request timeout | `5s` | No |
| `--max-claimable-amount` | Max claimable base units per request | `1000000000` | No |
| `--file` | Path to the API keys file | `api_keys.txt` | No |
| `--explorer-url` | Midenscan URL | - | No |
| `--base-amount` | Token amount (in base units) at which the difficulty of the challenge starts to increase. | `100000000` | No |

`start` reads the funding service's `/status` before serving and fails if it cannot be reached. It
also fails if `--max-claimable-amount` is larger than the funding service's own maximum, since the
faucet must never hand out more than the service accepts.

### Proof of Work Configuration

| Option | Description | Default | Required |
|--------|-------------|---------|----------|
| `--pow-secret` | Secret to sign PoW challenges. This should NOT be shared | - | No |
| `--pow-baseline` | Base PoW difficulty (0-32). It's the starting difficulty when no requests are pending | `16` | No |
| `--pow-challenge-lifetime` | Challenge validity duration, i.e. how long challenges remain valid. This affects the rate limiting, since it works by rejecting new submissions while the previous submitted challenge is still valid | `30s` | No |
| `--pow-cleanup-interval` | Cache cleanup interval, i.e. how often expired challenges are removed | `2s` | No |
| `--pow-growth-rate` | Difficulty growth rate, i.e. how quickly difficulty increases with load. | `0.1` | No |

### Advanced Configuration

| Option | Description | Default | Required |
|--------|-------------|---------|----------|
| `--enable-otel` | Enable OpenTelemetry | `false` | No |

## Environment Variables

All configuration options can be set using environment variables:

```bash
# Faucet Service Configuration
export MIDEN_FAUCET_FUNDING_SERVICE_URL=http://localhost:50401
export MIDEN_FAUCET_DECIMALS=6
export MIDEN_FAUCET_API_BIND_PORT=8000
export MIDEN_FAUCET_FRONTEND_BIND_PORT=8080
export MIDEN_FAUCET_NO_FRONTEND=false
export MIDEN_FAUCET_API_PUBLIC_URL=http://localhost:8000
export MIDEN_FAUCET_MAX_CLAIMABLE_AMOUNT=1000000000
export MIDEN_FAUCET_API_KEYS=api_keys.txt
export MIDEN_FAUCET_ENABLE_OTEL=true
export MIDEN_FAUCET_BASE_AMOUNT=100000000

# Network & Node Configuration
export MIDEN_FAUCET_NODE_URL=https://rpc.testnet.miden.io
export MIDEN_FAUCET_NETWORK=testnet
export MIDEN_FAUCET_TIMEOUT=10s
export MIDEN_FAUCET_EXPLORER_URL=https://testnet.midenscan.com

# Rate Limiting Configuration
export MIDEN_FAUCET_POW_SECRET=your-secret-here
export MIDEN_FAUCET_POW_BASELINE=16
export MIDEN_FAUCET_POW_CHALLENGE_LIFETIME=30s
export MIDEN_FAUCET_POW_CLEANUP_INTERVAL=2s
export MIDEN_FAUCET_POW_GROWTH_RATE=0.1
```

## Network Configurations

### Predefined Networks

#### Localhost
```bash
--network localhost
```
- **Explorer URL**: Not available
- **Address Display**: `mlcl`
- **Use Case**: Local development

#### Devnet
```bash
--network devnet
```
- **Explorer URL**: Not available
- **Address Display**: `mdev`
- **Use Case**: Development testing

#### Testnet
```bash
--network testnet
```
- **Explorer URL**: https://testnet.midenscan.com/
- **Address Display**: `mtst`
- **Use Case**: Integration testing

### Custom Network
```bash
--network custom
```

- **Explorer URL**: Not available
- **Address Display**: `mcst`
- **Use Case**: Run your custom network

## API Key Management

API keys live in a newline-delimited file of encoded keys, which the faucet reads at startup.

### Create an API Key

```bash
miden-faucet api-key create
```

Generates a new API key, appends it to the file, and prints it to stdout.

| Option | Description | Default | Required |
|--------|-------------|---------|----------|
| `--file` | Path to the API keys file | `api_keys.txt` | No |

### List API Keys

```bash
miden-faucet api-key list
```

Lists all API keys in the file.

| Option | Description | Default | Required |
|--------|-------------|---------|----------|
| `--file` | Path to the API keys file | `api_keys.txt` | No |

### Remove an API Key

```bash
miden-faucet api-key remove <KEY>
```

Removes an API key from the file. Fails if the key is not there.

| Argument/Option | Description | Default | Required |
|--------|-------------|---------|----------|
| `<KEY>` | The API key to remove (encoded string) | - | Yes |
| `--file` | Path to the API keys file | `api_keys.txt` | No |

### API Key Loading

When the faucet starts, it loads every API key in the file.

### API Key Benefits

- **Rate Limiting**: Separate rate limits per API key
- **Access Control**: Distribute keys to different users/teams

## Monitoring Configuration

### OpenTelemetry

Enable OpenTelemetry for production monitoring:

```bash
--enable-otel
```

## Configuration Example

```bash
miden-faucet start \
  --funding-service-url http://localhost:50401 \
  --decimals 6 \
  --frontend-bind-port 8080 \
  --api-bind-port 8000 \
  --node-url http://localhost:57291 \
  --network localhost
```

For detailed options, run `miden-faucet [COMMAND] --help`.

## Requesting tokens from a live faucet

You can use the `miden-faucet-client` binary to request tokens from any running faucet instance, whether it's your local faucet or the remote testnet faucet:
```bash
miden-faucet-client mint --url <FAUCET_API_URL> --target-account <ACCOUNT_ID> --amount <BASE_UNITS>
```

Although the command is named `mint`, in technical terms it makes a request to the faucet to request a public P2ID note.

To see available options:
```bash
miden-faucet-client mint --help
```
