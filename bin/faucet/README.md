# Miden faucet

This crate contains a binary CLI that allows to run the faucet behind a REST API and serves a frontend to interact with it.

#### API Endpoints Reference

**GET /pow**
- **Purpose**: Request a proof-of-work challenge
- **Query Parameters**:
  - `account_id` (string, required): The account ID requesting the challenge
  - `api_key` (string, optional): API key for authentication
- **Response**: JSON object containing:
  - `challenge` (string): The encoded challenge string in hexadecimal format
  - `target` (number): The target value for the proof-of-work challenge. A solution is valid if the hash `H(challenge, nonce)` is less than this target. As the hashing function we use `SHA-256`
  - `timestamp` (number): The timestamp when the challenge was created (seconds since UNIX epoch)

**GET /get_tokens**
- **Purpose**: Request tokens
- **Query Parameters**:
  - `account_id` (string, required): The account ID requesting tokens
  - `asset_amount` (number, required): Requested asset amount (in base units)
  - `challenge` (string, required): The encoded challenge from the `/pow` endpoint
  - `nonce` (number, required): The nonce used to solve the challenge
  - `api_key` (string, optional): API key for authentication
- **Response**: JSON object containing:
  - `note_id` (string): ID of the created note

See more detail in the [API Documentation](../../docs/src/rest-api.md).

#### Error Handling

The API returns appropriate HTTP status codes:
- `200`: Success
- `400`: Bad request
- `429`: Rate limited
- `500`: Server error

Error responses include a `message` field with details about the error.

## License

This project is [MIT licensed](../../LICENSE).
