import { sha256 } from '@noble/hashes/sha2.js';
import { hexToBytes } from '@noble/hashes/utils.js';

async function sendPowRequest(baseUrl: string, accountId: string): Promise<{ challenge: string, target: bigint }> {
    const powUrl = new URL('/pow', baseUrl);
    powUrl.searchParams.set('account_id', accountId);
    const powResp = await fetch(powUrl);
    if (!powResp.ok) throw new Error(`PoW error: ${powResp.status} ${await powResp.text()}`);
    const powJson: any = await powResp.json();
    const challenge: string = powJson.challenge;
    const target: bigint = BigInt(powJson.target);
    return { challenge, target };
}

async function solveChallenge(challenge: string, target: bigint): Promise<number> {
    let nonce = 0;
    let challengeBytes = hexToBytes(challenge);
    while (true) {
        nonce = Math.floor(Math.random() * Number.MAX_SAFE_INTEGER);

        try {
            // Compute hash using SHA-256 with the challenge and nonce
            let hash = sha256.create();
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
                return nonce;
            }
        } catch (error: any) {
            console.error('Error computing hash:', error);
            throw new Error('Failed to compute hash: ' + error.message);
        }
    }
}


async function getTokens(baseUrl: string, account_id: string, nonce: number, challenge: string): Promise<{ noteId: string, txId: string }> {
    const params = new URLSearchParams({
        account_id: account_id,
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
    return { noteId, txId };
}

async function main(): Promise<void> {
    const baseUrl = 'http://localhost:8000';
    const accountId = '0xca8203e8e58cf72049b061afca78ce';

    let { challenge, target } = await sendPowRequest(baseUrl, accountId);
    let nonce = await solveChallenge(challenge, target);
    let { noteId, txId } = await getTokens(baseUrl, accountId, nonce, challenge);
    console.log('Note ID:', noteId);
    console.log('Tx ID:', txId);
}

main();
