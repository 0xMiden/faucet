# Usage

The faucet can be accessed by the HTTP API interactively through the frontend or programmatically by building the requests manually.

## Programmatic API Usage

The faucet provides a REST API. The typical flow to request tokens involves:

1. **Requesting a Proof-of-Work challenge** from `/pow`

```typescript
const baseUrl = 'http://localhost:8080';
const accountId = 'mlcl1qq8mcy8pdvl0cgqfkjzf8efjjsnlzf7q';

const powUrl = new URL('/pow', baseUrl);
powUrl.searchParams.set('account_id', accountId);
const powResp = await fetch(powUrl);
if (!powResp.ok) throw new Error(`PoW error: ${powResp.status} ${await powResp.text()}`);
const powJson: any = await powResp.json();
const challenge: string = powJson.challenge;
const target: bigint = BigInt(powJson.target);
```

2. **Solving the computational challenge**

```typescript
// Dependencies: npm i @noble/hashes
import { sha_256 } from '@noble/hashes/sha2.js';
import { hexToBytes } from '@noble/hashes/utils.js';

let nonce = 0;
const challengeBytes = hexToBytes(challenge);
while (true) {
    nonce = Math.floor(Math.random() * Number.MAX_SAFE_INTEGER);

    try {
        // Compute hash using SHA-256 with the challenge and nonce
        let hash = sha_256.create();
        hash.update(challengeBytes);

        // Convert nonce to 8-byte big-endian format to match backend
        const nonceBytes = new ArrayBuffer(8);
        const nonceView = new DataView(nonceBytes);
        nonceView.setBigUint64(0, BigInt(nonce), false); // false = big-endian
        const nonceByteArray = new Uint8Array(nonceBytes);
        hash.update(nonceByteArray);

        // Take the first 8 bytes of the hash and parse them as u64 in big-endian
        const hashBytes: Uint8Array = hash.digest().slice(0, 8);
        let digest = BigInt('0x' + Array.from(hashBytes).map(b => b.toString(16).padStart(2, '0')).join(''));

        // Check if the hash is less than the target
        if (digest < target) {
            console.log('Found nonce:', nonce);
            return nonce;
        }
    } catch (error: any) {
        throw new Error('Failed to compute hash: ' + error.message);
    }
}
```

3. **Requesting tokens** along with your solved challenge to `/get_tokens`

```typescript
const params = new URLSearchParams({
    account_id: accountId,
    asset_amount: '100',
    challenge: challenge,
    nonce: nonce.toString()
});

const response = await fetch(`${baseUrl}/get_tokens?${params}`);
if (!response.ok) throw new Error(`Get tokens error: ${response.status} ${await response.text()}`);

const text = await response.text();
const json = JSON.parse(text);
const noteId = json.note_id;
const txId = json.tx_id;
const explorerUrl = json.explorer_url;
```

## Examples

Check out the complete working examples below. Make sure the faucet API is running at `http://localhost:8080` before using them.
- [Rust](examples/rust/request_tokens.rs)
- [TypeScript](examples/typescript/request_tokens.ts)

