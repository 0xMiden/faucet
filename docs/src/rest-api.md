# REST API Reference

REST API for programmatic access to the faucet service.

The Miden Faucet API follows a two-step process to request tokens:

1. **Request a Challenge** (`GET /pow`): Obtain a proof-of-work challenge that must be solved computationally
2. **Request tokens** (`GET /get_tokens`): Submit the solved challenge along with your token request

For detailed information about the token request flow, see the [Architecture](./architecture/overview.md#token-request-flow) section.

## Responses

## HTTP Status Codes

| Code | Description |
|------|-------------|
| `200` | Success |
| `400` | Bad Request |
| `429` | Rate limited |
| `500` | Internal Server Error |

## Endpoints

### Proof of Work Challenge

**Endpoint**: `GET /pow`

- **Purpose**: Request a proof-of-work challenge

- **Query Parameters**:
  - `account_id` (string, required): The account ID requesting the challenge
  - `api_key` (string, optional): API key for authentication

- **Response**: JSON object containing:
  - `challenge` (string): The encoded challenge string in hexadecimal format
  - `target` (number): The target value for the proof-of-work challenge. A solution is valid if the hash `H(challenge, nonce)` is less than this target. As the hashing function we use `SHA-256`
  - `timestamp` (number): The timestamp when the challenge was created (seconds since UNIX epoch)

### Get Tokens

**Endpoint**: `GET /get_tokens`

- **Purpose**: Request tokens

- **Query Parameters**:
  - `account_id` (string, required): The account ID requesting tokens
  - `is_private_note` (boolean, required): Whether to create a private note
  - `asset_amount` (number, required): Requested asset amount (in base units)
  - `challenge` (string, required): The encoded challenge from the `/pow` endpoint
  - `nonce` (number, required): The nonce used to solve the challenge
  - `api_key` (string, optional): API key for authentication

- **Response**: JSON object containing:
  - `tx_id` (string): ID of the created transaction
  - `note_id` (string): ID of the created note

### Get Metadata

**Endpoint**: `GET /get_metadata`

- **Purpose**: Request the faucet metadata to show on the frontend

- **Response**: JSON object containing:
  - `id` (string): ID of the faucet account
  - `issuance` (number): amount of tokens issued by the faucet (in base units)
  - `max_supply` (number): maximum available supply of the faucet (in base units)
  - `decimals` (number): number of decimals of the token minted by the faucet. It is needed to convert base units into token amounts.
  - `explorer_url` (string): URL to view the transaction in the explorer. Only present if available for the current network.
  - `note_transport_url` (string): URL of the note transport layer (NTL) endpoint used by the faucet. Only present if the faucet was configured with `--note-transport-url`.

### Get Note

**Endpoint**: `GET /get_note`

- **Purpose**: Request a specific note by its ID

- **Query Parameters**:
  - `note_id` (string, required): The ID of the note to retrieve

- **Response**: JSON object containing:
  - `note_id` (string): The ID of the requested note
  - `data_base64` (string): The note data encoded in base64 format. This data should be decoded and saved as a file with `.mno` extension and `application/octet-stream` media type

## Rate Limiting

When a challenge is submitted for an account, that same account cannot submit a new challenge while the previous one is still valid. This effectively rate limits accounts minting requests. Though, the same account can use API keys to submit multiple challenges simultaneously:

- **Without API Key**: Global rate limiting based on account ID
- **With API Key**: Separate rate limits per API key

### API Keys

Some endpoints support API key authentication for enhanced PoW.

The PoW difficulty of the faucet scales with the number of active requests. When using an API key, the difficulty will only depend on the number of active requests under that API key.

To request a challenge using an API key:

```bash
GET /pow?account_id=0xca8203e8e58cf72049b061afca78ce&api_key=miden_faucet_wONsvRXLZ9FgQG+nlkaq9f2X53cLswe4HSzEIUjFIkQ=
```
